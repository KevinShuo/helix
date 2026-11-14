use super::*;
use std::{fs, path::Path, process::Command};

struct Repo(tempfile::TempDir);
impl Repo {
    fn new() -> Self {
        let repo = Self(tempfile::tempdir().unwrap());
        repo.git(&["init", "-b", "main"]);
        for (key, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.test"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("core.quotepath", "false"),
            ("core.hooksPath", ""),
        ] {
            repo.git(&["config", key, value]);
        }
        repo
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    fn write(&self, path: &str, text: &str) {
        fs::write(self.path().join(path), text).unwrap();
    }
    fn initial(&self) {
        self.write("a", "base\n");
        self.git(&["add", "--", "a"]);
        self.git(&["commit", "-m", "initial"]);
    }
    async fn open(&self) -> GitRepository {
        GitRepository::open(self.path().to_owned(), true)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn commit_snapshot_is_read_only_and_separates_partial_diffs() {
    let r = Repo::new();
    r.initial();
    r.write("a", "staged\n");
    r.git(&["add", "--", "a"]);
    r.write("a", "worktree\n");
    let index = fs::read(r.path().join(".git/index")).unwrap();
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert_eq!(index, fs::read(r.path().join(".git/index")).unwrap());
    let f = &s.files[0];
    assert!(f.partial && f.staged && f.unstaged);
    assert!(f.staged_diff.contains("+staged"));
    assert!(!f.staged_diff.contains("+worktree"));
    assert!(f.worktree_diff.contains("+worktree"));
}

#[tokio::test]
async fn commit_exact_paths_unborn_stage_unstage_and_commit() {
    let r = Repo::new();
    r.write("-中文 file", "selected\n");
    r.write("other", "leave\n");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert!(s.head.is_none());
    let id = s
        .files
        .iter()
        .find(|f| f.path == Path::new("-中文 file"))
        .unwrap()
        .id;
    let s = git.stage_files(&s, &[id]).await.unwrap();
    assert_eq!(r.git(&["diff", "--cached", "--name-only"]), "-中文 file");
    let id = s.files.iter().find(|f| f.staged).unwrap().id;
    let s = git.unstage_files(&s, &[id]).await.unwrap();
    assert!(s.files.iter().all(|f| !f.staged));
    assert_eq!(
        fs::read_to_string(r.path().join("-中文 file")).unwrap(),
        "selected\n"
    );
    let id = s
        .files
        .iter()
        .find(|f| f.path == Path::new("-中文 file"))
        .unwrap()
        .id;
    let s = git.stage_files(&s, &[id]).await.unwrap();
    let outcome = git.commit_staged(&s, "feat: 中文提交\n").await.unwrap();
    assert_eq!(outcome.oid, r.git(&["rev-parse", "HEAD"]));
    assert_eq!(outcome.message.trim(), "feat: 中文提交");
    assert!(!outcome.changed_from_preview);
    assert_eq!(r.git(&["ls-tree", "--name-only", "HEAD"]), "-中文 file");
}

#[tokio::test]
async fn commit_staging_preserves_existing_partial_and_rejects_stale_files() {
    let r = Repo::new();
    r.initial();
    r.write("a", "staged\n");
    r.git(&["add", "a"]);
    r.write("a", "disk\n");
    r.write("b", "new\n");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let b = s
        .files
        .iter()
        .find(|f| f.path == Path::new("b"))
        .unwrap()
        .id;
    let s = git.stage_files(&s, &[b]).await.unwrap();
    assert_eq!(r.git(&["show", ":a"]), "staged");
    r.write("b", "changed\n");
    assert!(git.stage_files(&s, &[b]).await.is_err());
    assert_eq!(r.git(&["show", ":b"]), "new");
}

#[tokio::test]
async fn commit_unstage_unborn_partial_new_file_preserves_disk() {
    let r = Repo::new();
    r.write("a", "index\n");
    r.git(&["add", "a"]);
    r.write("a", "disk\n");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let s = git.unstage_files(&s, &[0]).await.unwrap();
    assert!(!s.files[0].staged);
    assert_eq!(fs::read_to_string(r.path().join("a")).unwrap(), "disk\n");
}

#[tokio::test]
async fn commit_stage_modified_staged_rename_and_deletion() {
    let r = Repo::new();
    r.initial();
    r.git(&["mv", "a", "中文 renamed"]);
    r.write("中文 renamed", "base\nx\n");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert_eq!(s.files[0].old_path.as_deref(), Some(Path::new("a")));
    let s = git.stage_files(&s, &[0]).await.unwrap();
    assert_eq!(r.git(&["show", ":中文 renamed"]), "base\nx");
    let s = git.unstage_files(&s, &[0]).await.unwrap();
    assert!(s.files.iter().all(|f| !f.staged));
    assert!(r.path().join("中文 renamed").exists());
    assert!(!r.path().join("a").exists());
    let id = s
        .files
        .iter()
        .find(|f| f.path == Path::new("a"))
        .unwrap()
        .id;
    let s = git.stage_files(&s, &[id]).await.unwrap();
    assert!(s.files.iter().any(|f| f.path == Path::new("a") && f.staged));
}

#[tokio::test]
async fn commit_staging_rename_does_not_stage_recreated_untracked_old_path() {
    let r = Repo::new();
    r.initial();
    r.git(&["mv", "a", "new.txt"]);
    r.write("new.txt", "base\nx\n");
    r.write("a", "unrelated recreated file\n");
    let git = r.open().await;
    let before = git.commit_snapshot().await.unwrap();
    let renamed = before
        .files
        .iter()
        .find(|f| f.path == Path::new("new.txt"))
        .unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some(Path::new("a")));
    let after = git.stage_files(&before, &[renamed.id]).await.unwrap();
    assert_eq!(r.git(&["show", ":new.txt"]), "base\nx");
    assert!(after
        .files
        .iter()
        .any(|f| f.path == Path::new("a") && f.untracked && !f.staged));
    assert!(!r.git(&["ls-files"]).lines().any(|path| path == "a"));
    assert_eq!(
        fs::read_to_string(r.path().join("a")).unwrap(),
        "unrelated recreated file\n"
    );
}

#[tokio::test]
async fn commit_recreated_staged_deletion_is_partial_candidate_without_staged_diff() {
    let r = Repo::new();
    r.initial();
    r.git(&["rm", "--", "a"]);
    r.write("a", "recreated unstaged content\n");
    let git = r.open().await;
    let before = git.commit_snapshot().await.unwrap();
    let deletion = before
        .files
        .iter()
        .find(|f| f.path == Path::new("a") && f.staged)
        .unwrap();
    let candidate = before
        .files
        .iter()
        .find(|f| f.path == Path::new("a") && f.untracked)
        .unwrap();
    assert_ne!(deletion.id, candidate.id);
    assert!(deletion.staged_diff.contains("-base"));
    assert!(!deletion.staged_diff.contains("recreated unstaged content"));
    assert!(
        candidate.partial,
        "Recreated path would replace an already staged deletion"
    );
    assert!(!candidate.staged);
    assert!(candidate.staged_diff.is_empty());
    assert!(candidate
        .worktree_diff
        .contains("recreated unstaged content"));
    let after = git.stage_files(&before, &[candidate.id]).await.unwrap();
    assert_eq!(r.git(&["show", ":a"]), "recreated unstaged content");
    assert!(after
        .files
        .iter()
        .any(|f| f.path == Path::new("a") && f.staged && !f.partial));
    assert_eq!(
        fs::read_to_string(r.path().join("a")).unwrap(),
        "recreated unstaged content\n"
    );
}

#[tokio::test]
async fn commit_rejects_stale_index_head_unknown_ids_empty_and_overlap() {
    let r = Repo::new();
    r.initial();
    r.write("a", "next\n");
    r.git(&["add", "a"]);
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert!(git.stage_files(&s, &[999]).await.is_err());
    assert!(git.stage_files(&s, &[0, 0]).await.is_err());
    r.write("b", "external\n");
    r.git(&["add", "b"]);
    assert!(git.commit_staged(&s, "fix: stale").await.is_err());
    let s = git.commit_snapshot().await.unwrap();
    r.git(&["checkout", "-b", "other"]);
    assert!(git.commit_staged(&s, "fix: wrong branch").await.is_err());
    let s = git.commit_snapshot().await.unwrap();
    let guard = git.begin_operation().unwrap();
    assert!(git.stage_files(&s, &[0]).await.is_err());
    drop(guard);
    let s = git
        .unstage_files(
            &s,
            &s.files
                .iter()
                .filter(|f| f.staged)
                .map(|f| f.id)
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    assert!(git.commit_staged(&s, "fix: empty").await.is_err());
    assert!(git.commit_staged(&s, "").await.is_err());
}

#[tokio::test]
async fn commit_excludes_sensitive_binary_and_oversized_content() {
    let r = Repo::new();
    r.write(".env.local", "SECRET=123\n");
    fs::write(r.path().join("binary"), [0u8, 1, 2]).unwrap();
    fs::write(r.path().join("large"), vec![b'x'; 300_000]).unwrap();
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert_eq!(s.files.len(), 3);
    for f in &s.files {
        assert!(!f.ai_eligible);
        assert!(f.exclusion.is_some());
        assert!(f.worktree_diff.is_empty());
    }
    // Ineligibility affects AI only: the user's exact selection still stages.
    let s = git.stage_files(&s, &[0, 1, 2]).await.unwrap();
    assert!(s
        .files
        .iter()
        .all(|f| f.staged && !f.ai_eligible && f.staged_diff.is_empty()));
}

fn hook(r: &Repo, name: &str, script: &str) {
    let hooks = r.path().join("hooks");
    fs::create_dir_all(&hooks).unwrap();
    let path = hooks.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    r.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
}

#[tokio::test]
async fn commit_failed_hook_keeps_index_and_reports_actual_head() {
    let r = Repo::new();
    r.initial();
    let before = r.git(&["rev-parse", "HEAD"]);
    r.write("a", "staged\n");
    r.git(&["add", "a"]);
    hook(&r, "pre-commit", "echo denied >&2; exit 1");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let error = git
        .commit_staged(&s, "fix: rejected")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains(&before));
    assert!(error.contains("denied"));
    assert_eq!(before, r.git(&["rev-parse", "HEAD"]));
    assert_eq!(r.git(&["show", ":a"]), "staged");
}

#[tokio::test]
async fn commit_reports_hook_modified_tree_message_and_ignores_unstaged_content() {
    let r = Repo::new();
    r.initial();
    r.write("a", "staged\n");
    r.git(&["add", "a"]);
    r.write("a", "unstaged\n");
    hook(
        &r,
        "pre-commit",
        "printf 'hook\\n' > added; git add -- added",
    );
    hook(&r, "commit-msg", "printf 'fix: hook message\\n' > \"$1\"");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let out = git.commit_staged(&s, "fix: proposed").await.unwrap();
    assert!(out.changed_from_preview);
    assert_eq!(out.message, "fix: hook message");
    assert_eq!(r.git(&["show", "HEAD:a"]), "staged");
    assert_eq!(r.git(&["show", "HEAD:added"]), "hook");
    assert_eq!(
        fs::read_to_string(r.path().join("a")).unwrap(),
        "unstaged\n"
    );
    assert!(git.commit_staged(&s, "fix: retry").await.is_err());
}

#[tokio::test]
async fn commit_blocks_sequencer_and_conflicts_and_handles_detached_head() {
    let r = Repo::new();
    r.initial();
    let git = r.open().await;
    fs::write(
        r.path().join(".git/MERGE_HEAD"),
        r.git(&["rev-parse", "HEAD"]),
    )
    .unwrap();
    assert!(git.commit_snapshot().await.is_err());
    fs::remove_file(r.path().join(".git/MERGE_HEAD")).unwrap();
    r.git(&["checkout", "--detach"]);
    r.write("a", "detached\n");
    r.git(&["add", "a"]);
    let s = git.commit_snapshot().await.unwrap();
    assert_eq!(s.head_name.len(), 8);
    assert!(s.is_detached());
    let out = git.commit_staged(&s, "fix: detached").await.unwrap();
    assert_eq!(out.oid, r.git(&["rev-parse", "HEAD"]));
}

#[tokio::test]
async fn commit_deleted_symlink_stays_metadata_only() {
    let r = Repo::new();
    r.initial();
    let blob = r.git(&["rev-parse", "HEAD:a"]);
    r.git(&["update-index", "--cacheinfo", &format!("120000,{blob},a")]);
    r.git(&["commit", "-m", "symlink"]);
    fs::remove_file(r.path().join("a")).unwrap();
    r.git(&["add", "--update", "--", "a"]);
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert!(!s.files[0].ai_eligible);
    assert!(s.files[0].worktree_diff.is_empty());
}

#[tokio::test]
async fn commit_never_reads_untracked_nested_git_directory_as_text() {
    let r = Repo::new();
    r.initial();
    let nested = Repo::new();
    nested.initial();
    r.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        nested.path().to_str().unwrap(),
        "sub",
    ]);
    r.git(&["commit", "-m", "add submodule"]);
    let child = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-C"])
            .arg(r.path().join("sub"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    child(&["config", "user.name", "Test"]);
    child(&["config", "user.email", "test@example.test"]);
    child(&["config", "commit.gpgsign", "false"]);
    fs::write(r.path().join("sub/a"), "first\n").unwrap();
    child(&["add", "a"]);
    child(&["commit", "-m", "first"]);
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert!(!s.files[0].ai_eligible);
    fs::write(r.path().join("sub/a"), "second\n").unwrap();
    child(&["add", "a"]);
    child(&["commit", "-m", "second"]);
    assert!(
        git.stage_files(&s, &[0]).await.is_err(),
        "Submodule HEAD changed after preview"
    );
}

#[tokio::test]
async fn commit_rejects_real_unmerged_index_without_operation_marker() {
    let r = Repo::new();
    r.initial();
    r.git(&["checkout", "-b", "side"]);
    r.write("a", "side\n");
    r.git(&["add", "a"]);
    r.git(&["commit", "-m", "side"]);
    r.git(&["checkout", "main"]);
    r.write("a", "main\n");
    r.git(&["add", "a"]);
    r.git(&["commit", "-m", "main"]);
    let out = Command::new("git")
        .args(["merge", "side"])
        .current_dir(r.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    fs::remove_file(r.path().join(".git/MERGE_HEAD")).unwrap();
    let git = r.open().await;
    assert!(git
        .commit_snapshot()
        .await
        .unwrap_err()
        .to_string()
        .contains("Unmerged"));
}

#[tokio::test]
async fn commit_local_patch_is_complete_even_when_ai_content_is_excluded() {
    let r = Repo::new();
    r.write(".env", "SECRET=for-local-review\n");
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    assert!(s.files[0].worktree_diff.is_empty());
    let patch = git.commit_file_patch(&s, 0, false).await.unwrap();
    assert!(patch.contains("SECRET=for-local-review"));
    let s = git.stage_files(&s, &[0]).await.unwrap();
    assert!(git
        .commit_file_patch(&s, 0, true)
        .await
        .unwrap()
        .contains("+SECRET=for-local-review"));
    assert!(git.commit_file_patch(&s, 77, true).await.is_err());
    r.write(".env", "changed\n");
    assert!(git.commit_file_patch(&s, 0, false).await.is_err());
}

#[tokio::test]
async fn commit_public_file_metadata_cannot_redirect_assigned_ids() {
    let r = Repo::new();
    r.write("a", "first\n");
    r.write("b", "second\n");
    let git = r.open().await;
    let mut s = git.commit_snapshot().await.unwrap();
    s.files[0].path = "b".into();
    assert!(git.stage_files(&s, &[0]).await.is_err());
    assert!(r.git(&["ls-files"]).is_empty());
}

#[tokio::test]
async fn commit_failed_hook_that_created_commit_reports_new_head_and_blocks_retry() {
    let r = Repo::new();
    r.initial();
    r.write("a", "next\n");
    r.git(&["add", "a"]);
    hook(
        &r,
        "pre-commit",
        "git -c core.hooksPath= -c commit.gpgsign=false commit -m 'hook-created'; exit 1",
    );
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let error = git
        .commit_staged(&s, "fix: proposed")
        .await
        .unwrap_err()
        .to_string();
    let actual = r.git(&["rev-parse", "HEAD"]);
    assert_ne!(Some(&actual), s.head.as_ref());
    assert!(error.contains(&actual));
    assert!(git.commit_staged(&s, "fix: retry").await.is_err());
    assert_eq!(r.git(&["rev-list", "--count", "HEAD"]), "2");
}

#[tokio::test]
async fn commit_respects_signing_failure_and_preserves_staged_content() {
    let r = Repo::new();
    r.initial();
    r.write("a", "next\n");
    r.git(&["add", "a"]);
    r.git(&["config", "commit.gpgsign", "true"]);
    r.git(&[
        "config",
        "gpg.program",
        "definitely-missing-helix-signing-program",
    ]);
    r.git(&["config", "user.signingkey", "missing"]);
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let error = git
        .commit_staged(&s, "fix: signed")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains(s.head.as_deref().unwrap()));
    assert_eq!(r.git(&["show", ":a"]), "next");
}

#[tokio::test]
async fn commit_metadata_only_sensitive_rename_and_forced_text_binary() {
    let r = Repo::new();
    r.write(".env", "secret\n");
    r.git(&["add", ".env"]);
    r.git(&["commit", "-m", "initial"]);
    r.git(&["mv", ".env", "ordinary.txt"]);
    r.write(".gitattributes", "binary diff\n");
    fs::write(r.path().join("binary"), [b'x', 0, b'y']).unwrap();
    r.git(&["add", "binary", ".gitattributes"]);
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let rename = s
        .files
        .iter()
        .find(|f| f.path == Path::new("ordinary.txt"))
        .unwrap();
    assert!(rename.old_path.is_some());
    assert!(!rename.ai_eligible);
    assert!(rename.staged_diff.is_empty());
    let binary = s
        .files
        .iter()
        .find(|f| f.path == Path::new("binary"))
        .unwrap();
    assert!(!binary.ai_eligible);
    assert!(binary.staged_diff.is_empty());
}

#[tokio::test]
async fn commit_reads_disable_external_diff_and_textconv_and_keep_index_semantics() {
    let r = Repo::new();
    r.initial();
    r.write("a", "staged\n");
    r.git(&["add", "a"]);
    r.git(&["config", "diff.external", "sh -c 'touch executed; exit 1'"]);
    r.write(".gitattributes", "a diff=custom\n");
    r.git(&[
        "config",
        "diff.custom.textconv",
        "sh -c 'touch executed; exit 1'",
    ]);
    let git = r.open().await;
    let s = git.commit_snapshot().await.unwrap();
    let id = s
        .files
        .iter()
        .find(|f| f.path == Path::new("a"))
        .unwrap()
        .id;
    assert!(git
        .commit_file_patch(&s, id, true)
        .await
        .unwrap()
        .contains("+staged"));
    assert!(!r.path().join("executed").exists());
    r.write("a", "later disk\n");
    let later = git.commit_snapshot().await.unwrap();
    assert!(s.same_index(&later));
    assert_ne!(s, later);
    let out = git.commit_staged(&s, "fix: staged only").await.unwrap();
    assert_eq!(r.git(&["show", "HEAD:a"]), "staged");
    assert!(!out.changed_from_preview);
}

#[tokio::test]
async fn commit_git_timeout_stops_hook_descendants_before_releasing_guard() {
    use std::time::Duration;
    let r = Repo::new();
    r.initial();
    r.write("a", "next\n");
    r.git(&["add", "a"]);
    hook(
        &r,
        "pre-commit",
        "printf started > hook-started; while ! test -f hook-release; do sleep 0.1; done; printf late > late-file; git add -- late-file",
    );
    let git = r.open().await;
    let guard = git.begin_operation().unwrap();
    let error = git
        .run(
            &super::implementation::args(&["commit", "-m", "timeout"]),
            Duration::from_millis(250),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("timed out"), "{error}");
    assert!(
        r.path().join("hook-started").exists(),
        "Hook must have started before timeout"
    );
    drop(guard);
    // Release any surviving hook only after termination and guard release.
    // A fixed sleep can expire while Windows taskkill is still running under
    // load, which permits an in-flight write rather than a surviving hook.
    r.write("hook-release", "release\n");
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(
        !r.path().join("late-file").exists(),
        "Timed-out hook survived and wrote to disk"
    );
    assert!(!r.git(&["ls-files"]).lines().any(|p| p == "late-file"));
    if error.contains("termination is unconfirmed") {
        assert!(
            git.begin_operation().is_err(),
            "Unconfirmed tree termination must quarantine future writes: {error}"
        );
    } else {
        assert!(
            git.begin_operation().is_ok(),
            "Confirmed tree termination must permit future writes: {error}"
        );
    }
}
