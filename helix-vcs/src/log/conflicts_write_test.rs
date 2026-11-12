use super::*;

#[tokio::test]
async fn git_conflicts_apply_one_path_only() {
    let repo = text_conflict("file.txt");
    fs::write(repo.path().join("unrelated.txt"), "keep staged\n").unwrap();
    repo.git(&["add", "--", "unrelated.txt"]);
    let unrelated = repo.git(&["rev-parse", ":unrelated.txt"]);
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    let outcome = git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"human resolution\n".to_vec(),
        })
        .await
        .unwrap();
    assert!(matches!(outcome, ApplyOutcome::Staged));
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"human resolution\n"
    );
    assert_eq!(repo.git(&["show", ":file.txt"]), "human resolution");
    assert_eq!(repo.git(&["rev-parse", ":unrelated.txt"]), unrelated);
    assert!(git.conflicts().await.unwrap().files.is_empty());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
}

#[tokio::test]
async fn git_conflicts_stale_snapshot_never_writes() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    let request = ResolutionRequest {
        version: data.version,
        contents: b"stale result\n".to_vec(),
    };
    fs::write(repo.path().join("file.txt"), b"external work\n").unwrap();
    assert!(git.apply_resolution(&request).await.is_err());
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"external work\n"
    );
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    repo.git(&["add", "--", "file.txt"]);
    assert!(git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"old\n".to_vec()
        })
        .await
        .is_err());
    assert_eq!(repo.git(&["show", ":file.txt"]), "external work");
}

#[tokio::test]
async fn git_conflicts_changed_head_or_attributes_rejects_apply() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    let before = fs::read(repo.path().join("file.txt")).unwrap();
    fs::write(repo.path().join(".gitattributes"), "*.txt filter=custom\n").unwrap();
    assert!(git
        .apply_resolution(&ResolutionRequest {
            version: data.version.clone(),
            contents: b"bad\n".to_vec()
        })
        .await
        .is_err());
    fs::remove_file(repo.path().join(".gitattributes")).unwrap();
    repo.git(&["update-ref", "HEAD", "refs/heads/topic"]);
    assert!(git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"bad\n".to_vec()
        })
        .await
        .is_err());
    assert_eq!(fs::read(repo.path().join("file.txt")).unwrap(), before);
}

#[tokio::test]
async fn git_conflicts_stage_failure_preserves_result_and_retries_without_rewrite() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    fs::write(repo.path().join(".git/index.lock"), "test-owned lock").unwrap();
    let outcome = git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"saved draft\n".to_vec(),
        })
        .await
        .unwrap();
    let ApplyOutcome::WrittenNotStaged { retry, .. } = outcome else {
        panic!("Expected recoverable stage failure: {outcome:?}");
    };
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"saved draft\n"
    );
    assert_eq!(git.conflicts().await.unwrap().files.len(), 1);
    fs::remove_file(repo.path().join(".git/index.lock")).unwrap();
    let saved_metadata = fs::metadata(repo.path().join("file.txt")).unwrap();
    assert!(matches!(
        git.retry_stage(&retry).await.unwrap(),
        ApplyOutcome::Staged
    ));
    assert_eq!(repo.git(&["show", ":file.txt"]), "saved draft");
    let staged_metadata = fs::metadata(repo.path().join("file.txt")).unwrap();
    assert_eq!(
        saved_metadata.modified().unwrap(),
        staged_metadata.modified().unwrap()
    );
    assert_eq!(
        saved_metadata.created().ok(),
        staged_metadata.created().ok()
    );
}

#[tokio::test]
async fn git_conflicts_retry_refuses_changed_disk() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    fs::write(repo.path().join(".git/index.lock"), "test-owned lock").unwrap();
    let outcome = git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"draft\n".to_vec(),
        })
        .await
        .unwrap();
    let ApplyOutcome::WrittenNotStaged { retry, .. } = outcome else {
        panic!("Expected stage failure")
    };
    fs::remove_file(repo.path().join(".git/index.lock")).unwrap();
    fs::write(repo.path().join("file.txt"), b"new external change\n").unwrap();
    assert!(git.retry_stage(&retry).await.is_err());
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"new external change\n"
    );
    assert_eq!(git.conflicts().await.unwrap().files.len(), 1);
}

#[tokio::test]
async fn git_conflicts_finish_requires_no_unmerged_and_fresh_preview() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    assert!(git.prepare_merge_action(MergeAction::Finish).await.is_err());
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    git.apply_resolution(&ResolutionRequest {
        version: data.version,
        contents: b"resolved\n".to_vec(),
    })
    .await
    .unwrap();
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    fs::write(repo.path().join("additional.txt"), "explicitly staged\n").unwrap();
    repo.git(&["add", "--", "additional.txt"]);
    assert!(git.execute_merge_action(&plan).await.is_err());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    assert!(plan
        .staged_paths()
        .contains(&PathBuf::from("additional.txt")));
    fs::write(
        repo.path().join("additional.txt"),
        "different staged blob\n",
    )
    .unwrap();
    repo.git(&["add", "--", "additional.txt"]);
    assert!(git.execute_merge_action(&plan).await.is_err());
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    let state = git.execute_merge_action(&plan).await.unwrap();
    assert_eq!(state.operation, ConflictOperation::None);
    assert!(state.files.is_empty());
    assert!(!repo.path().join(".git/MERGE_HEAD").exists());
    assert_eq!(
        repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
}

#[tokio::test]
async fn git_conflicts_abort_is_explicit_and_rechecks_worktree() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let plan = git.prepare_merge_action(MergeAction::Abort).await.unwrap();
    fs::write(repo.path().join("file.txt"), "new user draft\n").unwrap();
    assert!(git.execute_merge_action(&plan).await.is_err());
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"new user draft\n"
    );
    let plan = git.prepare_merge_action(MergeAction::Abort).await.unwrap();
    let state = git.execute_merge_action(&plan).await.unwrap();
    assert_eq!(state.operation, ConflictOperation::None);
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"before\ncurrent\nafter\n"
    );
}

// This catches wildcard pathspec staging (and staging the entire repository).
#[tokio::test]
async fn git_conflicts_literal_path_leaves_second_conflict() {
    let repo = text_conflict("[1].txt");
    // Reproduce a second conflict using the actual recorded stages.
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("[1].txt")).await.unwrap();
    let mut entries = String::new();
    for (i, stage) in data.file.stages.iter().enumerate() {
        let stage = stage.as_ref().unwrap();
        entries.push_str(&format!(
            "{:o} {} {}\t1.txt\n",
            stage.mode,
            stage.oid,
            i + 1
        ));
    }
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["update-index", "--index-info"])
        .current_dir(repo.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(entries.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    fs::write(repo.path().join("1.txt"), "second conflict stays\n").unwrap();
    let before = raw_git(&repo, &["ls-files", "--stage", "--", "1.txt"]).stdout;
    assert!(matches!(
        git.apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"literal target\n".to_vec(),
        })
        .await
        .unwrap(),
        ApplyOutcome::Staged
    ));
    let remaining = git.conflicts().await.unwrap();
    assert_eq!(remaining.files.len(), 1);
    assert_eq!(remaining.files[0].path, Path::new("1.txt"));
    assert_eq!(
        before,
        raw_git(&repo, &["ls-files", "--stage", "--", "1.txt"]).stdout
    );
}

#[tokio::test]
async fn git_conflicts_readonly_write_failure_keeps_original() {
    let repo = text_conflict("file.txt");
    let file = repo.path().join("file.txt");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    let before = fs::read(&file).unwrap();
    assert!(git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"must not save\n".to_vec()
        })
        .await
        .is_err());
    assert_eq!(fs::read(&file).unwrap(), before);
    assert_eq!(git.conflicts().await.unwrap().files.len(), 1);
}

fn failing_hook(repo: &Repo, name: &str, body: &str) {
    let hooks = repo.path().join("test-hooks");
    fs::create_dir_all(&hooks).unwrap();
    let path = hooks.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    repo.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
}

#[tokio::test]
async fn git_conflicts_failed_commit_hook_preserves_merge_for_retry() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    assert!(matches!(
        git.apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"resolved\n".to_vec()
        })
        .await
        .unwrap(),
        ApplyOutcome::Staged
    ));
    failing_hook(&repo, "pre-commit", "echo ran > hook-ran\nexit 1");
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    assert!(git.execute_merge_action(&plan).await.is_err());
    assert!(repo.path().join("hook-ran").exists());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    assert_eq!(repo.git(&["show", ":file.txt"]), "resolved");
    repo.git(&["config", "core.hooksPath", ""]);
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    assert_eq!(
        git.execute_merge_action(&plan).await.unwrap().operation,
        ConflictOperation::None
    );
}

#[tokio::test]
async fn git_conflicts_failed_abort_keeps_merge_and_rechecks_preview() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let plan = git.prepare_merge_action(MergeAction::Abort).await.unwrap();
    fs::write(repo.path().join(".git/index.lock"), "owned lock").unwrap();
    assert!(git.execute_merge_action(&plan).await.is_err());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    fs::remove_file(repo.path().join(".git/index.lock")).unwrap();
    assert_eq!(
        git.execute_merge_action(&plan).await.unwrap().operation,
        ConflictOperation::None
    );
}

#[tokio::test]
async fn git_conflicts_wrong_worktree_and_operation_lock_do_not_write() {
    let repo = text_conflict("file.txt");
    let other = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    let request = ResolutionRequest {
        version: data.version,
        contents: b"draft\n".to_vec(),
    };
    let original = fs::read(repo.path().join("file.txt")).unwrap();
    let other_before = fs::read(other.path().join("file.txt")).unwrap();
    assert!(other.open().await.apply_resolution(&request).await.is_err());
    let _guard = git.begin_operation().unwrap();
    assert!(git.apply_resolution(&request).await.is_err());
    assert_eq!(fs::read(repo.path().join("file.txt")).unwrap(), original);
    assert_eq!(
        fs::read(other.path().join("file.txt")).unwrap(),
        other_before
    );
}

#[cfg(windows)]
#[tokio::test]
async fn git_conflicts_junction_parent_never_writes_outside() {
    let repo = text_conflict("nested/file.txt");
    let git = repo.open().await;
    let data = git
        .conflict_file(Path::new("nested/file.txt"))
        .await
        .unwrap();
    let outside = tempfile::tempdir().unwrap();
    let moved = outside.path().join("moved");
    fs::rename(repo.path().join("nested"), &moved).unwrap();
    // Junctions require no Windows symlink privilege.
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(repo.path().join("nested"))
        .arg(&moved)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"escaped\n".to_vec()
        })
        .await
        .is_err());
    assert_eq!(
        fs::read(moved.join("file.txt")).unwrap(),
        data.working.unwrap()
    );
    fs::remove_dir(repo.path().join("nested")).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn git_conflicts_abort_missing_file_still_rejects_junction_parent() {
    let repo = text_conflict("nested/file.txt");
    let outside = tempfile::tempdir().unwrap();
    fs::remove_file(repo.path().join("nested/file.txt")).unwrap();
    fs::remove_dir(repo.path().join("nested")).unwrap();
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(repo.path().join("nested"))
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    let git = repo.open().await;
    let plan = git.prepare_merge_action(MergeAction::Abort).await;
    // Capture result, then remove junction even when an assertion fails.
    fs::remove_dir(repo.path().join("nested")).unwrap();
    assert!(
        plan.is_err(),
        "An absent leaf must not bypass parent validation"
    );
    assert!(!outside.path().join("file.txt").exists());
}

#[tokio::test]
async fn git_conflicts_stage_hook_external_edit_requires_refresh_without_rollback() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    failing_hook(&repo, "post-index-change", "echo external > file.txt");
    let outcome = git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"saved\n".to_vec(),
        })
        .await
        .unwrap();
    assert!(
        matches!(outcome, ApplyOutcome::NeedsRefresh { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"external\n"
    );
    assert_eq!(repo.git(&["show", ":file.txt"]), "saved");
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
}

#[tokio::test]
async fn git_conflicts_result_bytes_and_normalized_stage_are_verified() {
    let repo = text_conflict("file.txt");
    fs::write(repo.path().join(".gitattributes"), "*.txt text eol=lf\n").unwrap();
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    let outcome = git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"\xef\xbb\xbfexact\r\nbytes".to_vec(),
        })
        .await
        .unwrap();
    assert!(matches!(outcome, ApplyOutcome::Staged));
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        b"\xef\xbb\xbfexact\r\nbytes"
    );
    assert_eq!(
        raw_git(&repo, &["show", ":file.txt"]).stdout,
        b"\xef\xbb\xbfexact\nbytes"
    );
}

#[tokio::test]
async fn git_conflicts_invalid_result_never_touches_disk() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    for contents in [
        b"binary\0data".to_vec(),
        vec![b'x'; MAX_MERGE_BYTES + 1],
        b"mixed\r\nand\n".to_vec(),
        b"invalid\xff".to_vec(),
    ] {
        assert!(git
            .apply_resolution(&ResolutionRequest {
                version: data.version.clone(),
                contents
            })
            .await
            .is_err());
    }
    assert_eq!(
        fs::read(repo.path().join("file.txt")).unwrap(),
        data.working.unwrap()
    );
    assert_eq!(git.conflicts().await.unwrap().files.len(), 1);
}

#[tokio::test]
async fn git_conflicts_hook_operation_continues_with_owned_backend_task() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    git.apply_resolution(&ResolutionRequest {
        version: data.version,
        contents: b"resolved\n".to_vec(),
    })
    .await
    .unwrap();
    failing_hook(
        &repo,
        "pre-commit",
        "echo entered > hook-entered\nwhile test ! -f hook-release; do sleep 0.1; done",
    );
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    let owned = git.clone();
    let operation = tokio::spawn(async move { owned.execute_merge_action(&plan).await });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !repo.path().join("hook-entered").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "Commit did not reach the hook"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(git.operation_in_progress());
    assert!(git.prepare_merge_action(MergeAction::Abort).await.is_err());
    // Dropping a UI's handle doesn't drop this separately owned backend task.
    drop(git);
    fs::write(repo.path().join("hook-release"), b"release").unwrap();
    assert_eq!(
        operation.await.unwrap().unwrap().operation,
        ConflictOperation::None
    );
}

#[cfg_attr(
    windows,
    ignore = "Windows symlink privilege unavailable on this runner (OS error 1314); junction coverage runs separately"
)]
#[tokio::test]
async fn git_conflicts_symlink_parent_never_writes_outside() {
    let repo = text_conflict("nested/file.txt");
    let git = repo.open().await;
    let data = git
        .conflict_file(Path::new("nested/file.txt"))
        .await
        .unwrap();
    let outside = tempfile::tempdir().unwrap();
    let moved = outside.path().join("moved");
    fs::rename(repo.path().join("nested"), &moved).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&moved, repo.path().join("nested")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&moved, repo.path().join("nested")).unwrap();
    assert!(git
        .apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"escaped\n".to_vec()
        })
        .await
        .is_err());
    assert_eq!(
        fs::read(moved.join("file.txt")).unwrap(),
        data.working.unwrap()
    );
    #[cfg(unix)]
    fs::remove_file(repo.path().join("nested")).unwrap();
    #[cfg(windows)]
    fs::remove_dir(repo.path().join("nested")).unwrap();
}

#[tokio::test]
async fn git_conflicts_abort_rechecks_other_tracked_content_and_default_message() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let plan = git.prepare_merge_action(MergeAction::Abort).await.unwrap();
    fs::write(repo.path().join("seed"), b"new tracked edits\n").unwrap();
    assert!(git.execute_merge_action(&plan).await.is_err());
    assert_eq!(
        fs::read(repo.path().join("seed")).unwrap(),
        b"new tracked edits\n"
    );
    let plan = git.prepare_merge_action(MergeAction::Abort).await.unwrap();
    fs::write(
        repo.path().join(".git/MERGE_MSG"),
        b"changed merge message\n",
    )
    .unwrap();
    assert!(git.execute_merge_action(&plan).await.is_err());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
}

#[tokio::test]
async fn git_conflicts_autostash_lifecycle_requires_external_resolution() {
    let repo = text_conflict("file.txt");
    repo.git(&["merge", "--abort"]);
    fs::write(repo.path().join("seed"), b"before merge user work\n").unwrap();
    assert!(
        !raw_git(&repo, &["merge", "--no-edit", "--autostash", "topic"])
            .status
            .success()
    );
    assert!(repo.path().join(".git/MERGE_AUTOSTASH").exists());
    let git = repo.open().await;
    assert!(git.prepare_merge_action(MergeAction::Abort).await.is_err());
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    git.apply_resolution(&ResolutionRequest {
        version: data.version,
        contents: b"resolved\n".to_vec(),
    })
    .await
    .unwrap();
    assert!(git.prepare_merge_action(MergeAction::Finish).await.is_err());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    assert!(repo.path().join(".git/MERGE_AUTOSTASH").exists());
}

#[tokio::test]
async fn git_conflicts_preview_includes_both_paths_of_staged_rename() {
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    git.apply_resolution(&ResolutionRequest {
        version: data.version,
        contents: b"resolved\n".to_vec(),
    })
    .await
    .unwrap();
    repo.git(&["mv", "seed", "renamed-seed"]);
    let plan = git.prepare_merge_action(MergeAction::Finish).await.unwrap();
    assert!(plan.staged_paths().contains(&PathBuf::from("seed")));
    assert!(plan.staged_paths().contains(&PathBuf::from("renamed-seed")));
}

#[tokio::test]
async fn git_conflicts_linked_worktree_apply_leaves_main_conflict_untouched() {
    let repo = text_conflict("file.txt");
    let main_index = raw_git(&repo, &["ls-files", "--stage", "-z"]).stdout;
    let main_file = fs::read(repo.path().join("file.txt")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let linked = directory.path().join("linked");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        "HEAD",
    ]);
    let git = GitRepository::open(linked.clone(), true).await.unwrap();
    assert!(git
        .operate(&GitOperation::Merge {
            reference: "refs/heads/topic".into()
        })
        .await
        .is_err());
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    assert!(matches!(
        git.apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"linked resolution\n".to_vec()
        })
        .await
        .unwrap(),
        ApplyOutcome::Staged
    ));
    assert_eq!(
        fs::read(linked.join("file.txt")).unwrap(),
        b"linked resolution\n"
    );
    assert_eq!(fs::read(repo.path().join("file.txt")).unwrap(), main_file);
    assert_eq!(
        raw_git(&repo, &["ls-files", "--stage", "-z"]).stdout,
        main_index
    );
}

#[cfg(unix)]
#[tokio::test]
async fn git_conflicts_apply_preserves_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let repo = text_conflict("file.txt");
    let file = repo.path().join("file.txt");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o751)).unwrap();
    let git = repo.open().await;
    let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
    assert!(matches!(
        git.apply_resolution(&ResolutionRequest {
            version: data.version,
            contents: b"resolved\n".to_vec()
        })
        .await
        .unwrap(),
        ApplyOutcome::Staged
    ));
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o751
    );
}
