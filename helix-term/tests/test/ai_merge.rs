//! Real merge sessions and editor commands; only the external model is substituted.
use super::*;
use helix_vcs::merge::BlockDecision;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn reply(code: Option<&str>) -> Value {
    reply_for(0, code)
}

fn reply_for(block: usize, code: Option<&str>) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"content":json!({
        "block_id":block,"status":if code.is_some() {"resolved"} else {"needs-manual"},
        "code":code,"explanation":"保留双方的校验逻辑。"
    }).to_string()}}]})
}

fn repository_with_texts(texts: [&str; 3]) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    checked_git(repo.path(), &["init", "-b", "main"]);
    for (i, text) in texts.into_iter().enumerate() {
        if i == 1 {
            checked_git(repo.path(), &["switch", "-c", "topic"]);
        }
        if i == 2 {
            checked_git(repo.path(), &["switch", "main"]);
        }
        fs::write(repo.path().join("file.txt"), text).unwrap();
        checked_git(repo.path(), &["add", "--", "file.txt"]);
        checked_git(repo.path(), &["commit", "-m", "fixture"]);
    }
    assert!(!git(repo.path(), &["merge", "--no-edit", "topic"])
        .status
        .success());
    repo
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_retry_uncertainty_cancel_and_empty_deletion() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![
        (0, reply(Some("candidate\n"))),
        (0, reply(None)),
        (0, reply(Some(""))),
    ])
    .await;
    let mut app = app(repo.path(), endpoint).await?;
    keys(&mut app, "<space>vmi").await?;
    received(&mut requests).await?;
    preview(&mut app).await?;
    keys(&mut app, "r").await?;
    received(&mut requests).await?;
    preview(&mut app).await?;
    assert!(compact_screen(&app).contains("需人工处理"));
    keys(&mut app, "<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nbase\nafter\n"
    );
    assert!(compact_screen(&app).contains("需人工处理"));
    keys(&mut app, "<esc>:ai-merge<ret>").await?;
    received(&mut requests).await?;
    preview(&mut app).await?;
    keys(&mut app, "<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nafter\n"
    );
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_late_replies_after_edit_and_cancel_leave_draft_intact() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![
        (700, reply(Some("LATE\n"))),
        (700, reply(Some("CANCELLED\n"))),
    ])
    .await;
    let mut app = app(repo.path(), endpoint).await?;
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    keys(&mut app, "iuser <esc>").await?;
    let edited = current_ref!(app.editor).1.text().to_string();
    tokio::time::sleep(Duration::from_millis(800)).await;
    keys(&mut app, "").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), edited);
    assert!(!compact_screen(&app).contains("保留双方的校验逻辑"));
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    keys(&mut app, ":ai-merge-cancel<ret>").await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    keys(&mut app, "").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), edited);
    assert!(!compact_screen(&app).contains("保留双方的校验逻辑"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_block_switch_cancels_even_after_return_and_preserves_neighbor(
) -> anyhow::Result<()> {
    let common = (0..12).map(|i| format!("共同 {i}\n")).collect::<String>();
    let base = format!("头\nbase A\n{common}base B\n尾\n");
    let incoming = format!("头\nincoming A\n{common}incoming B\n尾\n");
    let current = format!("头\ncurrent A\n{common}current B\n尾\n");
    let repo = repository_with_texts([&base, &incoming, &current]);
    let (endpoint, mut requests, task) = server(vec![
        (600, reply_for(1, Some("late\n"))),
        (0, reply_for(1, Some("新乙\n"))),
    ])
    .await;
    let mut app = app(repo.path(), endpoint).await?;
    assert_eq!(
        current_ref!(app.editor)
            .1
            .merge()
            .unwrap()
            .model
            .blocks()
            .len(),
        2
    );
    keys(&mut app, "<space>vml<space>vmn:ai-merge<ret>").await?;
    let request = received(&mut requests).await?;
    let data: Value = serde_json::from_str(request["messages"][1]["content"].as_str().unwrap())?;
    assert_eq!(data["block_id"], 1);
    assert_eq!(data["base"], "base B\n");
    keys(&mut app, "<space>vmn<space>vmp").await?;
    tokio::time::sleep(Duration::from_millis(700)).await;
    keys(&mut app, "").await?;
    assert!(!compact_screen(&app).contains("保留双方的校验逻辑"));
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    preview(&mut app).await?;
    keys(&mut app, "<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        format!("头\ncurrent A\n{common}新乙\n尾\n")
    );
    assert_eq!(
        current_ref!(app.editor).1.merge().unwrap().model.blocks()[0].decision,
        BlockDecision::Reviewed
    );
    assert_eq!(
        current_ref!(app.editor).1.merge().unwrap().model.blocks()[1].decision,
        BlockDecision::NeedsReview
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_file_and_config_switches_cannot_revive_old_candidates() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![
        (600, reply(Some("late\n"))),
        (600, reply(Some("late\n"))),
    ])
    .await;
    let mut app = app(repo.path(), endpoint).await?;
    let original = current_ref!(app.editor).1.text().to_string();
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    keys(&mut app, ":q<ret>").await?;
    open_ui(&mut app).await?;
    tokio::time::sleep(Duration::from_millis(700)).await;
    keys(&mut app, "").await?;
    assert!(!compact_screen(&app).contains("保留双方的校验逻辑"));
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    keys(
        &mut app,
        ":set-option ai-merge.timeout-ms 30000<ret>:set-option ai-merge.timeout-ms 60000<ret>",
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(700)).await;
    keys(&mut app, "").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), original);
    assert!(!compact_screen(&app).contains("保留双方的校验逻辑"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_preserves_bom_and_crlf_until_reviewed_apply() -> anyhow::Result<()> {
    let repo = repository_with_texts([
        "\u{feff}before\r\nbase\r\nafter\r\n",
        "\u{feff}before\r\nincoming\r\nafter\r\n",
        "\u{feff}before\r\ncurrent\r\nafter\r\n",
    ]);
    let (endpoint, mut requests, task) = server(vec![(0, reply(Some("merged\r\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    let disk = fs::read(repo.path().join("file.txt"))?;
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    preview(&mut app).await?;
    keys(&mut app, "<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nmerged\nafter\n"
    );
    assert_eq!(fs::read(repo.path().join("file.txt"))?, disk);
    keys(&mut app, "<space>vmm<space>vma").await?;
    let expected = b"\xef\xbb\xbfbefore\r\nmerged\r\nafter\r\n";
    assert_eq!(fs::read(repo.path().join("file.txt"))?, expected);
    assert_eq!(git(repo.path(), &["show", ":file.txt"]).stdout, expected);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

async fn server(
    replies: Vec<(u64, Value)>,
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<Value>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        for (delay, reply) in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            let (end, length) = loop {
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|s| s.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < end + length {
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            if tx
                .send(serde_json::from_slice(&bytes[end..end + length]).unwrap())
                .is_err()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let response = reply.to_string();
            let _ = socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await;
        }
    });
    (endpoint, rx, task)
}

async fn app(repo: &Path, endpoint: String) -> anyhow::Result<Application> {
    let mut config = test_config();
    config.editor.ai_merge.enabled = true;
    config.editor.ai_completion.enabled = false;
    config.editor.ai_completion.endpoint = endpoint;
    config.editor.ai_completion.context_size = 8192;
    config.editor.idle_timeout = Duration::from_millis(30);
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(repo.join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    Ok(app)
}

async fn received(
    requests: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> anyhow::Result<Value> {
    tokio::time::timeout(Duration::from_secs(3), requests.recv())
        .await?
        .ok_or_else(|| anyhow::anyhow!("test model closed before receiving a request"))
}

async fn preview(app: &mut Application) -> anyhow::Result<()> {
    for _ in 0..100 {
        keys(app, "").await?;
        if compact_screen(app).contains("保留双方的校验逻辑") {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    anyhow::bail!("AI preview did not open: {}", screen(app))
}

// Test buffers represent the second cell of a wide glyph as whitespace.
fn compact_screen(app: &Application) -> String {
    screen(app).chars().filter(|c| !c.is_whitespace()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_native_preview_accepts_current_block_and_undoes() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![(0, reply(Some("双方();\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    let original = current_ref!(app.editor).1.text().to_string();
    assert_eq!(original, "before\nbase\nafter\n");
    let disk = fs::read(repo.path().join("file.txt"))?;
    let index = checked_git(repo.path(), &["ls-files", "-u"]);
    keys(&mut app, ":ai-merge 保留校验<ret>").await?;
    let request = received(&mut requests).await?;
    let data: Value = serde_json::from_str(request["messages"][1]["content"].as_str().unwrap())?;
    assert_eq!(data["base"], "base\n");
    assert_eq!(data["current"], "current\n");
    assert_eq!(data["incoming"], "incoming\n");
    assert_eq!(data["result"], "base\n");
    assert_eq!(data["intent"], "保留校验");
    preview(&mut app).await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), original);
    keys(&mut app, "ixxx<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\n双方();\nafter\n"
    );
    assert_eq!(
        current_ref!(app.editor).1.merge().unwrap().model.blocks()[0].decision,
        BlockDecision::NeedsReview
    );
    assert_eq!(fs::read(repo.path().join("file.txt"))?, disk);
    assert_eq!(checked_git(repo.path(), &["ls-files", "-u"]), index);
    keys(&mut app, "u").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), original);
    keys(&mut app, "U").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\n双方();\nafter\n"
    );
    keys(&mut app, "<space>vma").await?;
    assert_eq!(fs::read(repo.path().join("file.txt"))?, disk);
    keys(&mut app, "<space>vmm<space>vma").await?;
    assert_eq!(
        checked_git(repo.path(), &["show", ":file.txt"]),
        "before\n双方();\nafter"
    );
    assert_eq!(
        GitRepository::open(repo.path().to_owned(), true)
            .await?
            .conflicts()
            .await?
            .files
            .len(),
        1
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_identical_candidate_still_requires_review() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![(0, reply(Some("base\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    keys(&mut app, "<space>vmm:ai-merge<ret>").await?;
    assert!(current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    received(&mut requests).await?;
    preview(&mut app).await?;
    keys(&mut app, "<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nbase\nafter\n"
    );
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_waits_for_prompt_then_previews_without_stealing_input() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![(500, reply(Some("merged\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    keys(&mut app, ":").await?;
    tokio::time::sleep(Duration::from_millis(600)).await;
    keys(&mut app, "").await?;
    assert!(app
        .editor
        .ai_merge
        .as_ref()
        .is_some_and(|op| op.suggestion.is_some() && !op.preview));
    keys(&mut app, "<esc>").await?;
    preview(&mut app).await?;
    keys(&mut app, "<esc>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nbase\nafter\n"
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_invalid_response_and_timeout_preserve_draft() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut truncated = reply(Some("bad\n"));
    truncated["choices"][0]["finish_reason"] = json!("length");
    let (endpoint, mut requests, task) =
        server(vec![(0, truncated), (800, reply(Some("late\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    let original = current_ref!(app.editor).1.text().to_string();
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    for _ in 0..50 {
        keys(&mut app, "").await?;
        if app.editor.ai_merge.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(app.editor.ai_merge.is_none());
    assert!(app.editor.is_err());
    assert_eq!(current_ref!(app.editor).1.text().to_string(), original);
    keys(&mut app, ":set-option ai-merge.timeout-ms 100<ret>").await?;
    assert_eq!(app.editor.config().ai_merge.timeout_ms, 100);
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    keys(&mut app, "").await?;
    assert!(app.editor.ai_merge.is_none());
    assert!(app.editor.is_err());
    assert_eq!(current_ref!(app.editor).1.text().to_string(), original);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_refuses_locked_stale_untrusted_and_whole_file_targets() -> anyhow::Result<()> {
    use helix_view::{current, merge::MergeWriteStatus};
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![(0, reply(Some("unexpected\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    let original = current_ref!(app.editor).1.text().to_string();
    for status in [
        MergeWriteStatus::Staged,
        MergeWriteStatus::NeedsRefresh("stale".into()),
    ] {
        current!(app.editor).1.merge_mut().unwrap().write_status = status;
        keys(&mut app, ":ai-merge<ret>").await?;
        assert!(app.editor.ai_merge.is_none());
    }
    current!(app.editor).1.merge_mut().unwrap().write_status = MergeWriteStatus::Draft;
    current!(app.editor).1.merge_mut().unwrap().busy = true;
    keys(&mut app, ":ai-merge<ret>").await?;
    assert!(app.editor.ai_merge.is_none());
    current!(app.editor).1.merge_mut().unwrap().busy = false;
    current!(app.editor).1.readonly = true;
    keys(&mut app, ":ai-merge<ret>").await?;
    assert!(app.editor.ai_merge.is_none());
    current!(app.editor).1.readonly = false;
    app.editor.workspace_trust.set_config(Default::default());
    let workspace = helix_loader::find_workspace_in(repo.path()).0;
    app.editor.workspace_trust.untrust(&workspace);
    keys(&mut app, ":ai-merge<ret>").await?;
    assert!(app.editor.ai_merge.is_none());
    app.editor.workspace_trust = helix_loader::workspace_trust::WorkspaceTrust::fully_trusted();
    let sources = current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .sources()
        .clone();
    current!(app.editor).1.merge_mut().unwrap().model =
        helix_vcs::merge::MergeModel::preserve_working(
            current_ref!(app.editor).1.text().clone(),
            sources,
        );
    keys(&mut app, ":ai-merge<ret>").await?;
    assert!(app.editor.ai_merge.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(150), requests.recv())
            .await
            .is_err()
    );
    assert_eq!(current_ref!(app.editor).1.text().to_string(), original);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_merge_preview_keeps_actions_visible_in_narrow_terminal() -> anyhow::Result<()> {
    let repo = repository(true);
    let (endpoint, mut requests, task) = server(vec![(0, reply(Some("merged\n")))]).await;
    let mut app = app(repo.path(), endpoint).await?;
    keys(&mut app, ":ai-merge<ret>").await?;
    received(&mut requests).await?;
    preview(&mut app).await?;
    for (width, height) in [(160, 40), (80, 24), (40, 12)] {
        app.test_resize(width, height).await?;
        keys(&mut app, "").await?;
        let screen = screen(&app);
        assert!(screen.contains("Enter"), "{width}: {screen}");
        assert!(screen.contains("Esc"), "{width}: {screen}");
    }
    app.test_resize(12, 6).await?;
    keys(&mut app, "<esc>").await?;
    assert!(app.editor.ai_merge.is_none());
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nbase\nafter\n"
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    task.abort();
    Ok(())
}
