use super::*;

#[path = "conflicts_write_test.rs"]
mod writes;

fn raw_git(repo: &Repo, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(repo.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
}

fn conflict_repo(path: &str, base: Option<&[u8]>, current: Option<&[u8]>, incoming: &[u8]) -> Repo {
    let repo = Repo::new();
    repo.git(&["config", "user.name", "Conflict Test"]);
    repo.git(&["config", "user.email", "conflict@example.test"]);
    repo.commit("seed", "seed\n", "seed");
    fs::create_dir_all(repo.path().join(path).parent().unwrap()).unwrap();
    if let Some(base) = base {
        fs::write(repo.path().join(path), base).unwrap();
        repo.git(&["add", "--", path]);
        repo.git(&["commit", "-m", "base"]);
    }
    repo.git(&["switch", "-c", "topic"]);
    fs::write(repo.path().join(path), incoming).unwrap();
    repo.git(&["add", "--", path]);
    repo.git(&["commit", "-m", "incoming"]);
    repo.git(&["switch", "main"]);
    match current {
        Some(bytes) => {
            fs::write(repo.path().join(path), bytes).unwrap();
            repo.git(&["add", "--", path]);
        }
        None => {
            repo.git(&["rm", "--", path]);
        }
    }
    repo.git(&["commit", "-m", "current"]);
    assert!(
        !raw_git(&repo, &["merge", "--no-edit", "--no-autostash", "topic"])
            .status
            .success()
    );
    repo
}

fn text_conflict(path: &str) -> Repo {
    conflict_repo(
        path,
        Some(b"before\nbase\nafter\n"),
        Some(b"before\ncurrent\nafter\n"),
        b"before\nincoming\nafter\n",
    )
}

#[tokio::test]
async fn git_conflicts_reads_index_stages() {
    let repo = text_conflict("中文 file.txt");
    let git = repo.open().await;
    let state = git.conflicts().await.unwrap();
    assert!(matches!(state.operation, ConflictOperation::Merge { .. }));
    assert_eq!(state.files.len(), 1);
    assert_eq!(state.files[0].status, "UU");
    let data = git.conflict_file(Path::new("中文 file.txt")).await.unwrap();
    assert_eq!(data.support, ConflictSupport::Text);
    assert_eq!(
        data.sources[0].as_deref(),
        Some(b"before\nbase\nafter\n".as_slice())
    );
    assert_eq!(
        data.sources[1].as_deref(),
        Some(b"before\ncurrent\nafter\n".as_slice())
    );
    assert_eq!(
        data.sources[2].as_deref(),
        Some(b"before\nincoming\nafter\n".as_slice())
    );
    for stage in data.file.stages.iter().flatten() {
        assert_eq!(stage.mode, 0o100644);
    }
    assert!(data.working.unwrap().windows(7).any(|w| w == b"<<<<<<<"));
}

#[tokio::test]
async fn git_conflicts_missing_stages() {
    let added = conflict_repo("-added.txt", None, Some(b"current\n"), b"incoming\n");
    let data = added
        .open()
        .await
        .conflict_file(Path::new("-added.txt"))
        .await
        .unwrap();
    assert_eq!(data.file.status, "AA");
    assert!(data.file.stages[0].is_none());
    assert!(data.sources[0].is_none());
    assert_eq!(data.support, ConflictSupport::Text);
    let deleted = conflict_repo("deleted.txt", Some(b"base\n"), None, b"incoming\n");
    let data = deleted
        .open()
        .await
        .conflict_file(Path::new("deleted.txt"))
        .await
        .unwrap();
    assert!(data.file.stages[1].is_none());
    assert!(matches!(data.support, ConflictSupport::InspectOnly { .. }));
}

#[tokio::test]
async fn git_conflicts_detects_operation_and_empty_repository() {
    let empty = Repo::new();
    let state = empty.open().await.conflicts().await.unwrap();
    assert_eq!(state.operation, ConflictOperation::None);
    assert!(state.files.is_empty());
    assert!(empty
        .open()
        .await
        .prepare_merge_action(MergeAction::Finish)
        .await
        .is_err());
    assert!(empty
        .open()
        .await
        .prepare_merge_action(MergeAction::Abort)
        .await
        .is_err());
    let repo = text_conflict("file.txt");
    repo.git(&["merge", "--abort"]);
    assert!(!raw_git(&repo, &["rebase", "topic"]).status.success());
    assert_eq!(
        repo.open().await.conflicts().await.unwrap().operation,
        ConflictOperation::Rebase
    );
    assert!(repo
        .open()
        .await
        .prepare_merge_action(MergeAction::Finish)
        .await
        .is_err());
    assert!(repo
        .open()
        .await
        .prepare_merge_action(MergeAction::Abort)
        .await
        .is_err());
    repo.git(&["rebase", "--abort"]);
    assert!(!raw_git(&repo, &["cherry-pick", "topic"]).status.success());
    assert_eq!(
        repo.open().await.conflicts().await.unwrap().operation,
        ConflictOperation::CherryPick
    );
    assert!(repo
        .open()
        .await
        .prepare_merge_action(MergeAction::Finish)
        .await
        .is_err());
    assert!(repo
        .open()
        .await
        .prepare_merge_action(MergeAction::Abort)
        .await
        .is_err());
}

#[tokio::test]
async fn git_conflicts_rejects_lossy_special_and_large_inputs() {
    for current in [b"a\0b".as_slice(), b"a\xffb", b"a\r\nb\nc\n"] {
        let repo = conflict_repo("file.txt", Some(b"base\n"), Some(current), b"incoming\n");
        let data = repo
            .open()
            .await
            .conflict_file(Path::new("file.txt"))
            .await
            .unwrap();
        assert!(matches!(data.support, ConflictSupport::InspectOnly { .. }));
    }
    let repo = text_conflict("file.txt");
    let git = repo.open().await;
    for attr in [
        "filter=custom",
        "merge=custom",
        "working-tree-encoding=UTF-16",
    ] {
        fs::write(
            repo.path().join(".gitattributes"),
            format!("*.txt {attr}\n"),
        )
        .unwrap();
        let data = git.conflict_file(Path::new("file.txt")).await.unwrap();
        assert!(matches!(data.support, ConflictSupport::InspectOnly { .. }));
    }
    fs::remove_file(repo.path().join(".gitattributes")).unwrap();
    fs::write(
        repo.path().join("file.txt"),
        vec![b'x'; 2 * 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(matches!(
        git.conflict_file(Path::new("file.txt"))
            .await
            .unwrap()
            .support,
        ConflictSupport::InspectOnly { .. }
    ));
    assert!(git.conflict_file(Path::new("../file.txt")).await.is_err());
    assert!(GitRepository::open(repo.path().to_owned(), false)
        .await
        .is_err());
}

#[tokio::test]
async fn git_conflicts_linked_worktree_uses_its_own_index() {
    let repo = text_conflict("file.txt");
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        "HEAD",
    ]);
    let git = GitRepository::open(linked, true).await.unwrap();
    assert!(git.conflicts().await.unwrap().files.is_empty());
    assert!(git
        .operate(&GitOperation::Merge {
            reference: "refs/heads/topic".into()
        })
        .await
        .is_err());
    assert_eq!(git.conflicts().await.unwrap().files.len(), 1);
    assert_eq!(repo.open().await.conflicts().await.unwrap().files.len(), 1);
}
