use super::*;
use std::time::{Duration, Instant};

fn alias_directory(target: &Path) -> (tempfile::TempDir, PathBuf) {
    let holder = tempfile::tempdir().unwrap();
    let alias = holder.path().join("alias");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(target)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &alias).unwrap();
    (holder, alias)
}

#[tokio::test]
async fn git_review_alias_paths_are_repository_relative() {
    let repo = Repo::new();
    let id = repo.commit("file.txt", "content\n", "alias history");
    let (_holder, alias) = alias_directory(repo.path());
    let git = repo.open().await;
    let snapshot = git
        .snapshot(&GitLogFilter {
            path: Some(alias.join("file.txt")),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.commits[0].id, id);
}

#[tokio::test]
async fn git_review_operations_serialize_new_repository_handles() {
    let repo = Repo::new();
    repo.commit("file.txt", "base\n", "base");
    let remote = tempfile::tempdir().unwrap();
    repo.git(&["init", "--bare", remote.path().to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
    repo.git(&["push", "-u", "origin", "main"]);
    let hook = repo.path().join(".git/hooks/pre-push");
    fs::write(&hook, "#!/bin/sh\ntouch .git/git-tools-ready\nsleep 3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    repo.git(&["config", "core.hooksPath", ".git/hooks"]);
    let git = repo.open().await;
    let task = tokio::spawn(async move { git.operate(&GitOperation::Push).await });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !repo.path().join(".git/git-tools-ready").exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let started = repo.path().join(".git/git-tools-ready").exists();
    let other = repo.open().await;
    let conflicting = other
        .operate(&GitOperation::NewBranch {
            name: "overlapping-operation".into(),
            start: "HEAD".into(),
        })
        .await;
    task.await.unwrap().unwrap();
    assert!(started, "pre-push hook did not start");
    assert!(
        conflicting.is_err(),
        "A new repository handle bypassed operation serialization"
    );
}

#[tokio::test]
async fn git_review_merge_never_implicitly_stashes_dirty_worktree() {
    let repo = Repo::new();
    repo.commit("file.txt", "base\n", "base");
    repo.git(&["checkout", "-b", "topic"]);
    repo.commit("file.txt", "topic\n", "topic");
    repo.git(&["checkout", "main"]);
    repo.git(&["config", "merge.autoStash", "true"]);
    fs::write(repo.path().join("file.txt"), "local dirty contents\n").unwrap();
    let result = repo
        .open()
        .await
        .operate(&GitOperation::Merge {
            reference: "refs/heads/topic".into(),
        })
        .await;
    assert!(
        result.is_err(),
        "Merge used the repository's autoStash setting"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("file.txt")).unwrap(),
        "local dirty contents\n"
    );
}
