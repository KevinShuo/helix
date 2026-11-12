use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "review.rs"]
mod review;

#[path = "conflicts_test.rs"]
mod conflicts;

struct Repo(tempfile::TempDir);

impl Repo {
    fn new() -> Self {
        let repo = Self(tempfile::tempdir().unwrap());
        repo.git(&["init", "-b", "main"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "core.hooksPath", ""]);
        repo
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Test Author",
                "-c",
                "user.email=test@example.test",
            ])
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .current_dir(self.path())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn commit(&self, path: &str, contents: &str, message: &str) -> String {
        let full = self.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
        self.git(&["add", "--", path]);
        self.git(&["commit", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    async fn open(&self) -> GitRepository {
        GitRepository::open(self.path().to_owned(), true)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn git_log_empty_detached_and_discovery() {
    let repo = Repo::new();
    let git = repo.open().await;
    let empty = git.snapshot(&GitLogFilter::default()).await.unwrap();
    assert!(empty.commits.is_empty());
    assert!(empty.head.is_none());
    assert_eq!(empty.head_name, "main");

    let id = repo.commit("src/中文 file.rs", "first\n", "initial");
    let nested = GitRepository::open(repo.path().join("src/中文 file.rs"), true)
        .await
        .unwrap();
    assert_eq!(nested.root(), git.root());
    repo.git(&["checkout", "--detach", &id]);
    let detached = nested.snapshot(&GitLogFilter::default()).await.unwrap();
    assert_eq!(detached.head.as_deref(), Some(id.as_str()));
    assert!(detached.head_name.starts_with(&id[..8]));
}

#[tokio::test]
async fn git_log_graph_refs_filters_and_paging() {
    let repo = Repo::new();
    let root = repo.commit("base.txt", "base\n", "initial");
    repo.git(&["tag", "-a", "v1", "-m", "version one"]);
    repo.git(&["checkout", "-b", "feature/topic"]);
    let feature = repo.commit("topic.txt", "topic\n", "feature needle");
    repo.git(&["checkout", "main"]);
    repo.commit("main.txt", "main\n", "mainline");
    repo.git(&["merge", "--no-ff", "feature/topic", "-m", "Merge topic"]);
    repo.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let git = repo.open().await;
    let all = git.snapshot(&GitLogFilter::default()).await.unwrap();
    assert_eq!(all.commits.len(), 4);
    assert_eq!(all.commits[0].parents.len(), 2);
    assert!(all
        .commits
        .iter()
        .any(|c| c.graph.contains('|') || c.graph.contains('*')));
    assert!(all
        .references
        .iter()
        .any(|r| r.name == "feature/topic" && r.kind == GitRefKind::Local));
    assert!(all
        .references
        .iter()
        .any(|r| r.name == "origin/main" && r.kind == GitRefKind::Remote));
    assert!(all
        .references
        .iter()
        .any(|r| r.name == "v1" && r.id == root));
    let paged = git
        .snapshot(&GitLogFilter {
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(paged.commits.len(), 2);
    assert!(paged.has_more);
    let filtered = git
        .snapshot(&GitLogFilter {
            text: "needle".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(filtered.commits.len(), 1);
    assert_eq!(filtered.commits[0].id, feature);
    let branch = git
        .snapshot(&GitLogFilter {
            revision: Some("refs/heads/feature/topic".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(branch.commits.len(), 2);
    let path = git
        .snapshot(&GitLogFilter {
            path: Some(PathBuf::from("topic.txt")),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(path.commits.len(), 1);
    assert_eq!(path.commits[0].id, feature);
    let author = git
        .snapshot(&GitLogFilter {
            author: "does not exist".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(author.commits.is_empty());
    let future = git
        .snapshot(&GitLogFilter {
            since: "2099-01-01".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(future.commits.is_empty());
    let hash = git
        .snapshot(&GitLogFilter {
            revision: Some(feature.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(hash.commits[0].id, feature);
    let short_hash = git
        .snapshot(&GitLogFilter {
            text: feature[..8].into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        short_hash.commits.first().map(|c| c.id.as_str()),
        Some(feature.as_str())
    );
}

#[tokio::test]
async fn git_log_root_rename_delete_and_merge_diffs() {
    let repo = Repo::new();
    let original = "first\nsecond\nthird\nfourth\nfifth\n";
    let root = repo.commit("中文 old file.txt", original, "initial\n\nfull message");
    let git = repo.open().await;
    let target = GitTarget::Commit(root.clone());
    let details = git.details(&target).await.unwrap();
    assert!(details.message.contains("full message"));
    assert_eq!(details.files.len(), 1);
    let patch = git.patch(&target, &details.files[0]).await.unwrap();
    assert!(patch.contains("+first"));
    assert_eq!(
        git.file_at(&root, Path::new("中文 old file.txt"))
            .await
            .unwrap(),
        original.as_bytes()
    );

    repo.git(&["mv", "中文 old file.txt", "new name.txt"]);
    repo.git(&["commit", "-m", "rename"]);
    let renamed = GitTarget::Commit(repo.git(&["rev-parse", "HEAD"]));
    let files = git.details(&renamed).await.unwrap().files;
    assert_eq!(files[0].path, Path::new("new name.txt"));
    assert_eq!(
        files[0].old_path.as_deref(),
        Some(Path::new("中文 old file.txt"))
    );
    assert!(git
        .patch(&renamed, &files[0])
        .await
        .unwrap()
        .contains("rename from"));

    repo.git(&["checkout", "-b", "topic"]);
    repo.commit("topic.txt", "branch\n", "topic");
    repo.git(&["checkout", "main"]);
    repo.git(&["merge", "--no-ff", "topic", "-m", "merge"]);
    let merge = GitTarget::Commit(repo.git(&["rev-parse", "HEAD"]));
    let files = git.details(&merge).await.unwrap().files;
    assert_eq!(files.len(), 1);
    assert!(git
        .patch(&merge, &files[0])
        .await
        .unwrap()
        .contains("+branch"));

    repo.git(&["rm", "new name.txt"]);
    repo.git(&["commit", "-m", "delete"]);
    let deleted = GitTarget::Commit(repo.git(&["rev-parse", "HEAD"]));
    let files = git.details(&deleted).await.unwrap().files;
    assert!(files[0].status.starts_with('D'));
    assert!(git
        .patch(&deleted, &files[0])
        .await
        .unwrap()
        .contains("-first"));
}

#[tokio::test]
async fn git_log_status_staged_unstaged_and_untracked() {
    let repo = Repo::new();
    repo.commit("tracked.txt", "old\n", "initial");
    fs::write(repo.path().join("tracked.txt"), "staged\n").unwrap();
    repo.git(&["add", "tracked.txt"]);
    fs::write(repo.path().join("tracked.txt"), "unsaved on disk\n").unwrap();
    fs::write(repo.path().join("new 中文.txt"), "new contents\n").unwrap();
    let git = repo.open().await;
    let details = git.details(&GitTarget::WorkingTree).await.unwrap();
    let tracked = details
        .files
        .iter()
        .find(|f| f.path == Path::new("tracked.txt"))
        .unwrap();
    assert_eq!(tracked.status, "MM");
    assert!(git
        .patch(&GitTarget::WorkingTree, tracked)
        .await
        .unwrap()
        .contains("+unsaved on disk"));
    let new = details
        .files
        .iter()
        .find(|f| f.path == Path::new("new 中文.txt"))
        .unwrap();
    assert_eq!(new.status, "??");
    assert!(git
        .patch(&GitTarget::WorkingTree, new)
        .await
        .unwrap()
        .contains("+new contents"));
    // Restore identical contents so rename detection is independent of the
    // staged/unstaged replacement tested above.
    repo.git(&["restore", "--staged", "--worktree", "tracked.txt"]);
    repo.git(&["mv", "tracked.txt", "renamed.txt"]);
    let renamed = git.details(&GitTarget::WorkingTree).await.unwrap();
    let renamed = renamed
        .files
        .iter()
        .find(|f| f.path == Path::new("renamed.txt"))
        .unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some(Path::new("tracked.txt")));
}

#[tokio::test]
async fn git_log_operations_use_local_repositories() {
    let repo = Repo::new();
    let base = repo.commit("base", "base", "base");
    let git = repo.open().await;
    git.operate(&GitOperation::NewBranch {
        name: "topic".into(),
        start: base.clone(),
    })
    .await
    .unwrap();
    assert_eq!(repo.git(&["branch", "--show-current"]), "topic");
    repo.commit("topic", "topic", "topic");
    let topic_commit = repo.git(&["rev-parse", "HEAD"]);
    let topic = git
        .snapshot(&GitLogFilter::default())
        .await
        .unwrap()
        .references
        .into_iter()
        .find(|r| r.name == "main")
        .unwrap();
    git.operate(&GitOperation::Checkout { reference: topic })
        .await
        .unwrap();
    assert_eq!(repo.git(&["branch", "--show-current"]), "main");
    git.operate(&GitOperation::Merge {
        reference: "refs/heads/topic".into(),
    })
    .await
    .unwrap();
    assert!(repo.path().join("topic").exists());

    let remote = tempfile::tempdir().unwrap();
    repo.git(&["init", "--bare", remote.path().to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
    repo.git(&["push", "-u", "origin", "main"]);
    git.operate(&GitOperation::Fetch).await.unwrap();
    git.operate(&GitOperation::Pull).await.unwrap();
    git.operate(&GitOperation::Push).await.unwrap();
    assert!(git
        .operate(&GitOperation::NewBranch {
            name: "--bad".into(),
            start: "HEAD".into()
        })
        .await
        .is_err());
    assert!(git
        .snapshot(&GitLogFilter {
            revision: Some("--all".into()),
            ..Default::default()
        })
        .await
        .is_err());
    assert!(git
        .details(&GitTarget::Commit("--help".into()))
        .await
        .is_err());
    git.operate(&GitOperation::NewBranch {
        name: "pick".into(),
        start: base,
    })
    .await
    .unwrap();
    git.operate(&GitOperation::CherryPick {
        commit: topic_commit,
    })
    .await
    .unwrap();
    assert!(repo.path().join("topic").exists());
    repo.git(&["update-ref", "refs/remotes/origin/remote-topic", "HEAD"]);
    let remote = git
        .snapshot(&GitLogFilter::default())
        .await
        .unwrap()
        .references
        .into_iter()
        .find(|r| r.name == "origin/remote-topic")
        .unwrap();
    git.operate(&GitOperation::Checkout { reference: remote })
        .await
        .unwrap();
    assert_eq!(repo.git(&["branch", "--show-current"]), "remote-topic");
    repo.git(&["tag", "v-test"]);
    let tag = git
        .snapshot(&GitLogFilter::default())
        .await
        .unwrap()
        .references
        .into_iter()
        .find(|r| r.name == "v-test")
        .unwrap();
    git.operate(&GitOperation::Checkout { reference: tag })
        .await
        .unwrap();
    assert!(repo.git(&["branch", "--show-current"]).is_empty());
}

#[tokio::test]
async fn git_log_linked_worktree_and_trust() {
    let repo = Repo::new();
    repo.commit("file", "base", "base");
    let worktrees = tempfile::tempdir().unwrap();
    let linked = worktrees.path().join("linked");
    repo.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    let git = GitRepository::open(linked, true).await.unwrap();
    assert_eq!(
        git.snapshot(&GitLogFilter::default())
            .await
            .unwrap()
            .head_name,
        "linked"
    );
    assert!(GitRepository::open(repo.path().to_owned(), false)
        .await
        .unwrap_err()
        .to_string()
        .contains("trusted"));
    let outside = tempfile::tempdir().unwrap();
    assert!(GitRepository::open(outside.path().to_owned(), true)
        .await
        .is_err());
}

#[tokio::test]
async fn git_log_unborn_diff_uses_worktree_contents() {
    let repo = Repo::new();
    fs::write(repo.path().join("new.txt"), "staged\n").unwrap();
    repo.git(&["add", "new.txt"]);
    fs::write(repo.path().join("new.txt"), "worktree\n").unwrap();
    let git = repo.open().await;
    let details = git.details(&GitTarget::WorkingTree).await.unwrap();
    let patch = git
        .patch(&GitTarget::WorkingTree, &details.files[0])
        .await
        .unwrap();
    assert!(patch.contains("+worktree"), "{patch}");
    assert!(!patch.contains("+staged"), "{patch}");
}

#[tokio::test]
async fn git_log_push_only_publishes_current_branch_even_with_matching_default() {
    let repo = Repo::new();
    let base = repo.commit("base", "base", "base");
    repo.git(&["branch", "topic"]);
    let remote = tempfile::tempdir().unwrap();
    repo.git(&["init", "--bare", remote.path().to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
    repo.git(&["push", "--all", "origin"]);
    repo.git(&["checkout", "topic"]);
    repo.commit("topic", "topic", "topic");
    repo.git(&["checkout", "main"]);
    let current = repo.commit("main", "main", "main");
    repo.git(&["config", "push.default", "matching"]);
    repo.open()
        .await
        .operate(&GitOperation::Push)
        .await
        .unwrap();
    assert!(repo
        .git(&["ls-remote", "origin", "refs/heads/main"])
        .starts_with(&current));
    assert!(repo
        .git(&["ls-remote", "origin", "refs/heads/topic"])
        .starts_with(&base));
}
