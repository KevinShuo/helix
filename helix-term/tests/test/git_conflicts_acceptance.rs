//! Full user workflow: two conflicts, one clean merge, an independently staged file.
use super::*;

async fn wait_text(app: &mut Application, expected: &str) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !screen(app).contains(expected) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(30)).await;
        keys(app, "").await?;
    }
    assert!(
        screen(app).contains(expected),
        "Missing {expected}:\n{}",
        screen(app)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_end_to_end_preserves_other_staged_paths_and_commits_two_parents(
) -> anyhow::Result<()> {
    let repo = repository(true);
    let root = repo.path();
    let parents = [
        checked_git(root, &["rev-parse", "HEAD"]),
        checked_git(root, &["rev-parse", "MERGE_HEAD"]),
    ];
    assert_eq!(
        fs::read_to_string(root.join("clean.txt"))?,
        "clean incoming\n"
    );
    fs::write(root.join("unrelated.txt"), "staged independently\n")?;
    checked_git(root, &["add", "--", "unrelated.txt"]);
    let clean_oid = checked_git(root, &["rev-parse", ":clean.txt"]);
    let unrelated_oid = checked_git(root, &["rev-parse", ":unrelated.txt"]);
    let second_unmerged = checked_git(root, &["ls-files", "-u", "--", "second.txt"]);
    let mut app = AppBuilder::new()
        .with_file(root.join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vmbggjA manual<esc>").await?;
    let expected = "before\ncurrent manual\nincoming\nafter\n";
    assert_eq!(current_ref!(app.editor).1.text().to_string(), expected);
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    keys(&mut app, "<space>vmm<space>vma").await?;
    assert_eq!(fs::read_to_string(root.join("file.txt"))?, expected);
    assert_eq!(
        checked_git(root, &["show", ":file.txt"]),
        expected.trim_end()
    );
    assert_eq!(
        checked_git(root, &["ls-files", "-u", "--", "second.txt"]),
        second_unmerged
    );
    assert_eq!(
        checked_git(root, &["rev-parse", ":unrelated.txt"]),
        unrelated_oid
    );
    assert_eq!(checked_git(root, &["rev-parse", ":clean.txt"]), clean_oid);
    assert!(
        root.join(".git/MERGE_HEAD").exists(),
        "Apply must not commit"
    );

    keys(&mut app, "<space>vmf").await?;
    wait_text(&mut app, "Enter resolve").await?;
    keys(&mut app, "j<ret>").await?;
    wait_merge(&mut app).await?;
    keys(&mut app, "<space>vmr<space>vma<space>vmf").await?;
    wait_text(&mut app, "Enter resolve").await?;
    assert!(checked_git(root, &["ls-files", "-u"]).is_empty());
    keys(&mut app, "f").await?;
    wait_text(&mut app, "Confirm Finish").await?;
    for path in ["file.txt", "second.txt", "clean.txt", "unrelated.txt"] {
        assert!(screen(&app).contains(path), "Finish preview omitted {path}");
    }
    // The second confirmation owns an immutable index snapshot.
    assert_eq!(checked_git(root, &["rev-parse", "HEAD"]), parents[0]);
    keys(&mut app, "y").await?;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while root.join(".git/MERGE_HEAD").exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(30)).await;
        keys(&mut app, "").await?;
    }
    assert!(!root.join(".git/MERGE_HEAD").exists());
    let ancestry = checked_git(root, &["rev-list", "--parents", "-1", "HEAD"]);
    assert_eq!(
        ancestry.split_whitespace().skip(1).collect::<Vec<_>>(),
        parents.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(
        checked_git(root, &["show", "HEAD:file.txt"]),
        expected.trim_end()
    );
    assert_eq!(
        checked_git(root, &["show", "HEAD:second.txt"]),
        "before\nincoming\nafter"
    );
    assert_eq!(
        checked_git(root, &["rev-parse", "HEAD:unrelated.txt"]),
        unrelated_oid
    );
    assert_eq!(
        checked_git(root, &["rev-parse", "HEAD:clean.txt"]),
        clean_oid
    );
    assert!(checked_git(root, &["status", "--porcelain"]).is_empty());
    keys(&mut app, "q:qa!<ret>").await?;
    assert!(app.close().await.is_empty());
    Ok(())
}
