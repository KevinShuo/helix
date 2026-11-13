use super::helpers::*;
use helix_core::text_annotations::InlineAnnotation;
use helix_term::application::Application;
use helix_view::{
    ai_completion::{AiCompletion, AiCompletionPromptMode},
    current, current_ref,
    input::parse_macro,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for key in parse_macro(input)? {
        tx.send(Ok(crossterm_event(key)))?;
    }
    let mut stream = UnboundedReceiverStream::new(rx);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.event_loop_until_idle(&mut stream),
    )
    .await?;
    Ok(())
}

async fn idle(app: &mut Application) {
    // The previous idle event clears this timer. Arm it even if a canceled
    // request will never deliver another event.
    app.editor.reset_idle_timer();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_event_loop_until_idle(app),
    )
    .await
    .expect("editor did not become idle");
}

#[cfg(windows)]
fn crossterm_event(key: helix_view::input::KeyEvent) -> crossterm::event::Event {
    crossterm::event::Event::Key(key.into())
}

#[cfg(not(windows))]
fn crossterm_event(key: helix_view::input::KeyEvent) -> termina::event::Event {
    termina::event::Event::Key(key.into())
}

fn show(app: &mut Application, text: &str) {
    let (view, doc) = current!(app.editor);
    doc.ai_completion = Some(AiCompletion {
        view: view.id,
        version: doc.version(),
        annotation: InlineAnnotation::new(
            doc.selection(view.id)
                .primary()
                .cursor(doc.text().slice(..)),
            text,
        ),
    });
    assert!(
        doc.ai_completion.as_ref().unwrap().is_valid(doc, view.id),
        "invalid preview: selection {:?}, version {}",
        doc.selection(view.id),
        doc.version()
    );
}

fn text(app: &Application) -> String {
    current_ref!(app.editor).1.text().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_accepts_whole_multiline_preview_and_undoes() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().with_input_text("ab#[|]#Z\n").build()?;
    keys(&mut app, "i").await?;
    let version = current_ref!(app.editor).1.version();
    show(&mut app, "值\nx");
    assert_eq!(text(&app), "abZ\n");
    assert_eq!(current_ref!(app.editor).1.version(), version);
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "ab值\nxZ\n");
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..)),
        5
    );
    assert!(doc.ai_completion.is_none());
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), "abZ\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_movement_invalidates_and_empty_accept_does_nothing() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().with_input_text("ab#[|]#Z\n").build()?;
    keys(&mut app, "i").await?;
    show(&mut app, "incorrect");
    keys(&mut app, "<left><S-right>").await?;
    assert_eq!(text(&app), "abZ\n");
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_repeat_reuses_accepted_text() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().with_input_text("#[|\n]#").build()?;
    keys(&mut app, "iab").await?;
    show(&mut app, "c(\"值\")");
    keys(&mut app, "<S-right><esc>:new<ret>.").await?;
    assert_eq!(
        text(&app),
        format!("abc(\"值\"){}", helix_core::NATIVE_LINE_ENDING.as_str())
    );
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_macro_replays_literal_without_autopairing() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().with_input_text("#[|\n]#").build()?;
    keys(&mut app, "Qiab").await?;
    show(&mut app, "c(\"值\")");
    keys(&mut app, "<S-right><esc>Q:new<ret>q").await?;
    assert_eq!(
        text(&app),
        format!("abc(\"值\"){}", helix_core::NATIVE_LINE_ENDING.as_str())
    );
    app.close().await;
    Ok(())
}

fn popup_app() -> anyhow::Result<Application> {
    let mut config = test_config();
    config.editor.word_completion.enable = true;
    config.editor.word_completion.trigger_length = std::num::NonZeroU8::new(2).unwrap();
    AppBuilder::new()
        .with_config(config)
        .with_input_text("ab_from_popup\nab#[|\n]#")
        .build()
}

async fn popup(app: &mut Application) -> anyhow::Result<()> {
    keys(app, "i").await?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.editor.last_completion.is_none() && std::time::Instant::now() < deadline {
        // The ordinary word index updates on its own debounce interval.
        keys(app, "<C-x>").await?;
    }
    assert!(
        app.editor.last_completion.is_some(),
        "word completion did not open"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_accept_does_not_accept_the_popup() -> anyhow::Result<()> {
    let mut app = popup_app()?;
    popup(&mut app).await?;
    show(&mut app, "_from_ai");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "ab_from_popup\nab_from_ai\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_preserves_popup_tab_and_enter() -> anyhow::Result<()> {
    let mut app = popup_app()?;
    popup(&mut app).await?;
    show(&mut app, "_from_ai");
    keys(&mut app, "<tab>").await?;
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    keys(&mut app, "<ret>").await?;
    assert_eq!(text(&app), "ab_from_popup\nab_from_popup\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_empty_accept_cancels_popup_preview() -> anyhow::Result<()> {
    let mut app = popup_app()?;
    popup(&mut app).await?;
    keys(&mut app, "<tab><S-right>").await?;
    assert_eq!(text(&app), "ab_from_popup\nab\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_respects_remapped_accept_key() -> anyhow::Result<()> {
    let mut config = test_config();
    config.keys =
        toml::from_str("[insert]\nS-right = 'move_char_right'\nF4 = 'ai_completion_accept'\n")?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("ab#[|]#Z\n")
        .build()?;
    keys(&mut app, "i").await?;
    show(&mut app, "unwanted");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "abZ\n");
    show(&mut app, "TAIL");
    keys(&mut app, "<F4>").await?;
    assert_eq!(text(&app), "abZTAIL\n");
    app.close().await;
    Ok(())
}

async fn model_server(
    delay_ms: u64,
    responses: Vec<&'static str>,
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let (sent, received) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let mut tasks = Vec::new();
        for (index, content) in responses.into_iter().enumerate() {
            let (mut socket, _) = listener.accept().await.unwrap();
            let sent = sent.clone();
            tasks.push(tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 { return; }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                        let length: usize = header.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                        if bytes.len() >= end + 4 + length { break; }
                    }
                }
                let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let _ = sent.send(serde_json::from_slice(&bytes[end..]).unwrap());
                if index == 0 { tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await; }
                let body = serde_json::json!({"choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}]}).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            }));
        }
        for task in tasks {
            let _ = task.await;
        }
    });
    (endpoint, received, server)
}

fn model_config(endpoint: String) -> helix_term::config::Config {
    let mut config = test_config();
    config.editor.ai_completion.enabled = true;
    config.editor.ai_completion.endpoint = endpoint;
    config.editor.ai_completion.debounce_ms = 1;
    config.editor.ai_completion.timeout_ms = 1000;
    config
}

async fn wait_for_preview(app: &mut Application) {
    let config = &app.editor.config().ai_completion;
    let startup_ms = if config.model_path.is_some() {
        config.startup_timeout_ms.clamp(100, 600_000)
    } else {
        0
    };
    let until = tokio::time::Instant::now()
        + std::time::Duration::from_millis(startup_ms + config.timeout_ms + 2000);
    loop {
        idle(app).await;
        if current_ref!(app.editor).1.ai_completion.is_some() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "No automatic AI preview arrived"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_automatically_requests_and_displays_without_editing() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(0, vec![" + 1"]).await;
    let mut app = AppBuilder::new()
        .with_config(model_config(endpoint))
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "ix").await?;
    let version = current_ref!(app.editor).1.version();
    wait_for_preview(&mut app).await;
    assert_eq!(text(&app), "x\n");
    assert_eq!(current_ref!(app.editor).1.version(), version);
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "x + 1\n");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_rejects_new_syntax_errors_in_valid_code() -> anyhow::Result<()> {
    let (endpoint, mut requests, server) = model_server(0, vec!["+ * 2"]).await;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Gap;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text("def f(value):\n    return value#[|\n]#")
        .build()?;
    keys(&mut app, "i<space>").await?;
    idle(&mut app).await;
    assert!(requests.recv().await.is_some());
    server.await?;
    idle(&mut app).await;
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    assert_eq!(text(&app), "def f(value):\n    return value \n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_predicts_a_block_accepts_and_undoes() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(
        0,
        vec!["    total = 0\n    for item in items:\n        total += item\n    return total"],
    )
    .await;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let mut config = model_config(endpoint);
    config.editor.default_line_ending = helix_view::editor::LineEndingConfig::LF;
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.predict_next = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text("def total(items):#[|\n]#")
        .build()?;
    keys(&mut app, "i<ret>").await?;
    wait_for_preview(&mut app).await;
    assert_eq!(text(&app), "def total(items):\n    \n");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "def total(items):\n    total = 0\n    for item in items:\n        total += item\n    return total\n");
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), "def total(items):\n    \n");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_prediction_preserves_later_absolute_indentation() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(0, vec!["for value in values:\n        if value > 0:\n            result.append(value)\n    return result"]).await;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.predict_next = true;
    let mut app = AppBuilder::new().with_config(config).with_file(path, None)
        .with_input_text("def positives(values):\n    result = []\n    # Use an explicit for loop to append strictly positive values to result, then return result.\n   #[|\n]#").build()?;
    keys(&mut app, "i<space>").await?;
    wait_for_preview(&mut app).await;
    assert_eq!(current_ref!(app.editor).1.ai_completion.as_ref().unwrap().annotation.text.as_str().replace("\r\n", "\n"), "for value in values:\n        if value > 0:\n            result.append(value)\n    return result");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_prediction_keeps_crlf_and_existing_code() -> anyhow::Result<()> {
    let (endpoint, _, server) =
        model_server(0, vec!["    result = x + 1\r\n    result *= 2"]).await;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.predict_next = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text("def f(x):\r\n   #[|\r\n]#    return result\r\n")
        .build()?;
    keys(&mut app, "i<space>").await?;
    wait_for_preview(&mut app).await;
    assert_eq!(text(&app), "def f(x):\r\n    \r\n    return result\r\n");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(
        text(&app),
        "def f(x):\r\n    result = x + 1\r\n    result *= 2\r\n    return result\r\n"
    );
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), "def f(x):\r\n    \r\n    return result\r\n");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_typing_cancels_a_pending_prediction() -> anyhow::Result<()> {
    let (endpoint, mut requests, server) = model_server(250, vec!["    return sum(items)"]).await;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.predict_next = true;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text("def total(items):\n   #[|\n]#")
        .build()?;
    keys(&mut app, "i<space>").await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), requests.recv())
        .await?
        .unwrap();
    keys(&mut app, "x").await?;
    server.await?;
    idle(&mut app).await;
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    assert_eq!(text(&app), "def total(items):\n    x\n");
    app.close().await;
    Ok(())
}

async fn check_live_prediction(
    name: &str,
    input: &str,
    checks: &str,
    multiline: bool,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let mut config = model_config(std::env::var("HELIX_TEST_AI_ENDPOINT")?);
    config.editor.ai_completion.model = std::env::var("HELIX_TEST_AI_MODEL")?;
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.predict_next = true;
    config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Gap;
    config.editor.ai_completion.timeout_ms = 15000;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path.clone(), None)
        .with_input_text(input)
        .build()?;
    keys(&mut app, "i<space>").await?;
    wait_for_preview(&mut app).await;
    let before = text(&app);
    let preview = current_ref!(app.editor)
        .1
        .ai_completion
        .as_ref()
        .unwrap()
        .annotation
        .text
        .to_string();
    println!("{name} prediction: {preview:?}");
    assert_eq!(text(&app), before);
    if multiline {
        assert!(preview.lines().count() > 1, "expected a loop block");
    }
    keys(&mut app, "<S-right>").await?;
    let accepted = text(&app);
    std::fs::write(&path, format!("{accepted}\n{checks}"))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::process::Command::new("python")
            .arg("-I")
            .arg(&path)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), before);
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_TEST_AI_ENDPOINT and HELIX_TEST_AI_MODEL"]
async fn ai_completion_live_next_line_prediction() -> anyhow::Result<()> {
    check_live_prediction(
        "next line",
        "def total(items):\n    \"\"\"Return the sum of every item.\"\"\"\n   #[|\n]#",
        "assert total([1,2,3]) == 6\nassert total([]) == 0\nassert total([-4,2]) == -2\n",
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_TEST_AI_ENDPOINT and HELIX_TEST_AI_MODEL"]
async fn ai_completion_live_next_block_prediction() -> anyhow::Result<()> {
    check_live_prediction("short block", "def positives(values):\n    result = []\n    # Use an explicit for loop to append strictly positive values to result, then return result.\n   #[|\n]#", "assert positives([-2,0,3,1]) == [3,1]\nassert positives([]) == []\nassert positives([-1]) == []\n", true).await
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_conservative_blank_line_does_not_send_a_request() -> anyhow::Result<()> {
    let (endpoint, mut started, server) = model_server(0, vec!["unwanted()"]).await;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "i<space>").await?;
    idle(&mut app).await;
    assert!(started.try_recv().is_err());
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    assert_eq!(text(&app), " \n");
    server.abort();
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_conservative_short_preview_accepts_and_undoes() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(0, vec!["total = sum(items)"]).await;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("    total =#[|\n]#")
        .build()?;
    keys(&mut app, "i<space>").await?;
    wait_for_preview(&mut app).await;
    assert_eq!(text(&app), "    total = \n");
    assert_eq!(
        current_ref!(app.editor)
            .1
            .ai_completion
            .as_ref()
            .unwrap()
            .annotation
            .text
            .as_str(),
        "sum(items)"
    );
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "    total = sum(items)\n");
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), "    total = \n");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_gap_accepts_only_the_missing_expression() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(0, vec!["sum(items)"]).await;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Gap;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("    total =#[|\n]#")
        .build()?;
    keys(&mut app, "i<space>").await?;
    wait_for_preview(&mut app).await;
    assert_eq!(text(&app), "    total = \n");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "    total = sum(items)\n");
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), "    total = \n");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_recovers_distant_parameters_and_import_aliases() -> anyhow::Result<()> {
    let (endpoint, mut requests, server) =
        model_server(0, vec!["    return FilePath(base) / \"config.json\""]).await;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.py");
    std::fs::write(&path, "")?;
    let input = format!(
        "from pathlib import Path as FilePath\n\ndef config_path(base: str):\n    \"\"\"Return base / config.json without accessing the filesystem.\"\"\"\n{}    return#[|\n]#",
        "    # Input has already been validated.\n".repeat(100)
    );
    let mut config = model_config(endpoint);
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.prefix_chars = 1500;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text(input)
        .build()?;
    keys(&mut app, "i<space>").await?;
    wait_for_preview(&mut app).await;
    let request = requests.recv().await.unwrap();
    let prompt = request["messages"][1]["content"].as_str().unwrap();
    assert!(
        prompt.contains("def config_path(base: str):"),
        "enclosing parameters were omitted"
    );
    assert!(
        prompt.contains("from pathlib import Path as FilePath"),
        "import alias was omitted"
    );
    assert!(prompt.contains("    return <CURSOR>"));
    assert!(prompt.contains("without accessing the filesystem"));
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires clangd available on PATH"]
async fn ai_completion_live_clangd_grounded_member() -> anyhow::Result<()> {
    let (endpoint, mut requests, server) =
        model_server(0, vec!["    switch (packet.category_code) {"]).await;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("packet.cpp");
    std::fs::write(&path, "")?;
    let input = format!("struct Packet {{ unsigned category_code; unsigned payload_size; }};\nchar classify(const Packet& packet) {{\n{}    // Classify using the packet category code.\n    switch (packet#[|)]# {{\n    case 1: return 'A';\n    default: return 'B';\n    }}\n}}\n", "    // Input has already been validated.\n".repeat(100));
    let mut config = model_config(endpoint);
    config.editor.lsp.enable = true;
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Line;
    config.editor.ai_completion.prefix_chars = 3000;
    config.editor.ai_completion.debounce_ms = 350;
    config.editor.ai_completion.timeout_ms = 15000;
    config.editor.ai_completion.max_tokens = 48;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text(input)
        .build()?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while current_ref!(app.editor)
        .1
        .language_servers()
        .next()
        .is_none()
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "clangd did not initialize"
        );
        idle(&mut app).await;
    }
    keys(&mut app, "i.").await?;
    wait_for_preview(&mut app).await;
    let request = requests.recv().await.unwrap();
    let prompt = request["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(
        prompt.contains("Language server member candidate"),
        "LSP metadata was omitted: {prompt}"
    );
    assert!(
        prompt.contains("payload_size"),
        "the actual clangd field list was not sent"
    );
    let suggestion = current_ref!(app.editor)
        .1
        .ai_completion
        .as_ref()
        .unwrap()
        .annotation
        .text
        .to_string();
    assert_eq!(suggestion, "category_code");
    keys(&mut app, "<S-right>").await?;
    assert!(text(&app).contains("switch (packet.category_code) {"));
    println!("Actual clangd member metadata reached the AI prompt; preview: {suggestion}");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_disabled_local_model_keeps_editing_available() -> anyhow::Result<()> {
    let mut config = test_config();
    config.editor.ai_completion.model_path = Some("missing-model.gguf".into());
    config.editor.ai_completion.server_path = "missing-llama-server".into();
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "ix").await?;
    idle(&mut app).await;
    assert_eq!(text(&app), "x\n");
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .is_none_or(|(message, _)| !message.contains("AI completion")));
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_missing_local_model_reports_error_without_blocking_editing(
) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut config = test_config();
    config.editor.ai_completion.enabled = true;
    config.editor.ai_completion.model_path = Some(directory.path().join("missing.gguf"));
    config.editor.ai_completion.debounce_ms = 1;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[|\n]#")
        .build()?;
    idle(&mut app).await;
    let message = &app
        .editor
        .status_msg
        .as_ref()
        .expect("startup error should be visible")
        .0;
    assert!(message.contains("model not found"), "{message}");
    keys(&mut app, "ix").await?;
    idle(&mut app).await;
    assert_eq!(text(&app), "x\n");
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_line_mode_preserves_typed_code_and_existing_parenthesis(
) -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(
        0,
        vec!["while (smallestCarOfNumber == nextCarToOutput) {\n  unrelated();\n}"],
    )
    .await;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Line;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("while (smallestCarOfNumbe#[|)]#\n")
        .build()?;
    keys(&mut app, "ir").await?;
    wait_for_preview(&mut app).await;
    assert_eq!(text(&app), "while (smallestCarOfNumber)\n");
    assert_eq!(
        current_ref!(app.editor)
            .1
            .ai_completion
            .as_ref()
            .unwrap()
            .annotation
            .text
            .as_str(),
        " == nextCarToOutput"
    );
    keys(&mut app, "<S-right>").await?;
    assert_eq!(
        text(&app),
        "while (smallestCarOfNumber == nextCarToOutput)\n"
    );
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_old_response_cannot_replace_new_response() -> anyhow::Result<()> {
    let (endpoint, mut started, server) = model_server(350, vec!["old", "new"]).await;
    let mut app = AppBuilder::new()
        .with_config(model_config(endpoint))
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "ia").await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), started.recv()).await?;
    keys(&mut app, "b").await?;
    wait_for_preview(&mut app).await;
    server.await?;
    idle(&mut app).await;
    assert_eq!(
        &*current_ref!(app.editor)
            .1
            .ai_completion
            .as_ref()
            .unwrap()
            .annotation
            .text,
        "new"
    );
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "abnew\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_move_cancels_inflight_response() -> anyhow::Result<()> {
    let (endpoint, mut started, server) = model_server(250, vec!["stale"]).await;
    let mut app = AppBuilder::new()
        .with_config(model_config(endpoint))
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "ix").await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), started.recv()).await?;
    keys(&mut app, "<left>").await?;
    server.await?;
    idle(&mut app).await;
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    assert_eq!(text(&app), "x\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_renders_gray_before_the_real_suffix() -> anyhow::Result<()> {
    use helix_core::Position;
    use helix_term::{keymap::Keymaps, ui::EditorView};
    use helix_view::graphics::{Color, Rect};
    let mut app = AppBuilder::new().with_input_text("ab#[|]#Z\n").build()?;
    keys(&mut app, "i").await?;
    let area = Rect::new(0, 0, 80, 24);
    app.editor.resize(area);
    show(&mut app, "值\nx");
    app.editor.cursor_cache.reset();
    let mut surface = tui::buffer::Buffer::empty(area);
    let renderer = EditorView::new(Keymaps::default());
    let (view, doc) = current_ref!(app.editor);
    let inner = view.inner_area(doc);
    renderer.render_view(&app.editor, doc, view, area, &mut surface, true);
    let first = surface.get(inner.x + 2, inner.y).unwrap();
    assert_eq!(&*first.symbol, "值");
    assert_eq!(first.fg, Color::Gray);
    assert_eq!(&*surface.get(inner.x, inner.y + 1).unwrap().symbol, "x");
    assert_eq!(&*surface.get(inner.x + 1, inner.y + 1).unwrap().symbol, "Z");
    assert_eq!(
        app.editor.cursor_cache.get(view, doc),
        Some(Position::new(0, 2))
    );
    assert_eq!(text(&app), "abZ\n");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_timeout_leaves_editing_usable() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(450, vec!["too late"]).await;
    let mut config = model_config(endpoint);
    config.editor.ai_completion.timeout_ms = 100;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "ix").await?;
    server.await?;
    idle(&mut app).await;
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    keys(&mut app, "y<esc>").await?;
    assert_eq!(text(&app), "xy\n");
    assert_status_not_error(&app.editor);
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_uses_document_line_endings() -> anyhow::Result<()> {
    let (endpoint, _, server) = model_server(0, vec!["one\ntwo"]).await;
    let mut app = AppBuilder::new()
        .with_config(model_config(endpoint))
        .with_input_text("#[|\r\n]#")
        .build()?;
    current!(app.editor).1.line_ending = helix_core::LineEnding::Crlf;
    keys(&mut app, "ix").await?;
    wait_for_preview(&mut app).await;
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "xone\r\ntwo\r\n");
    server.await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_TEST_AI_ENDPOINT pointing to a running local Qwen server"]
async fn ai_completion_live_local_model() -> anyhow::Result<()> {
    let mut config = model_config(std::env::var("HELIX_TEST_AI_ENDPOINT")?);
    if let Ok(model) = std::env::var("HELIX_TEST_AI_MODEL") {
        config.editor.ai_completion.model = model;
    }
    if std::env::var("HELIX_TEST_AI_PROMPT_MODE").as_deref() == Ok("line") {
        config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Line;
    }
    config.editor.ai_completion.timeout_ms = 15000;
    config.editor.ai_completion.max_tokens = 48;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("items = [1, 2, 3]\ntotal =#[|\n]#")
        .build()?;
    let start = std::time::Instant::now();
    keys(&mut app, "i ").await?;
    wait_for_preview(&mut app).await;
    let suggestion = current_ref!(app.editor)
        .1
        .ai_completion
        .as_ref()
        .unwrap()
        .annotation
        .text
        .to_string();
    assert!(!suggestion.trim().is_empty());
    assert_eq!(text(&app), "items = [1, 2, 3]\ntotal = \n");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(
        text(&app),
        format!("items = [1, 2, 3]\ntotal = {suggestion}\n")
    );
    println!(
        "Local model preview and Shift+Right acceptance: {:?}, elapsed {:?}",
        suggestion,
        start.elapsed()
    );
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_TEST_AI_MODEL_PATH and HELIX_TEST_AI_SERVER_PATH"]
async fn ai_completion_live_managed_local_model() -> anyhow::Result<()> {
    use helix_view::editor::ConfigEvent;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    let mut config = model_config(format!("http://{address}/v1/chat/completions"));
    config.editor.ai_completion.model_path =
        Some(std::env::var("HELIX_TEST_AI_MODEL_PATH")?.into());
    config.editor.ai_completion.server_path = std::env::var("HELIX_TEST_AI_SERVER_PATH")?.into();
    config.editor.ai_completion.model = "helix-managed-test-model".into();
    config.editor.ai_completion.conservative = true;
    config.editor.ai_completion.prompt_mode = serde_json::from_value(serde_json::json!(
        std::env::var("HELIX_TEST_AI_PROMPT_MODE").unwrap_or_else(|_| "line".into())
    ))?;
    config.editor.ai_completion.timeout_ms = 15000;
    config.editor.ai_completion.max_tokens = 48;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("items = [1, 2, 3]\ntotal =#[|\n]#")
        .build()?;
    keys(&mut app, "i ").await?;
    wait_for_preview(&mut app).await;
    let suggestion = current_ref!(app.editor)
        .1
        .ai_completion
        .as_ref()
        .unwrap()
        .annotation
        .text
        .to_string();
    assert_eq!(text(&app), "items = [1, 2, 3]\ntotal = \n");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(
        text(&app),
        format!("items = [1, 2, 3]\ntotal = {suggestion}\n")
    );
    println!("Managed local model preview and acceptance: {suggestion:?}");
    assert_eq!(suggestion, "sum(items)");

    let mut editor_config = (*app.editor.config()).clone();
    editor_config.ai_completion.enabled = false;
    app.handle_config_events(ConfigEvent::Update(Box::new(editor_config.clone())));
    assert!(current_ref!(app.editor).1.ai_completion.is_none());
    wait_for_local_port_release(address).await;

    // Re-enable, then cancel during model loading or immediately after readiness.
    editor_config.ai_completion.enabled = true;
    app.handle_config_events(ConfigEvent::Update(Box::new(editor_config.clone())));
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        loop {
            if std::net::TcpListener::bind(address).is_err() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await?;
    editor_config.ai_completion.enabled = false;
    app.handle_config_events(ConfigEvent::Update(Box::new(editor_config.clone())));
    wait_for_local_port_release(address).await;

    editor_config.ai_completion.enabled = true;
    app.handle_config_events(ConfigEvent::Update(Box::new(editor_config)));
    keys(&mut app, "<esc>ototal = ").await?;
    wait_for_preview(&mut app).await;
    assert!(app.close().await.is_empty());
    wait_for_local_port_release(address).await;
    Ok(())
}

async fn wait_for_local_port_release(address: std::net::SocketAddr) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if std::net::TcpListener::bind(address).is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the owned local AI server did not release its port");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_TEST_AI_ENDPOINT and HELIX_TEST_AI_CPP_FILE for the reported C++ case"]
async fn ai_completion_live_cpp_line() -> anyhow::Result<()> {
    let source = std::fs::read_to_string(std::env::var("HELIX_TEST_AI_CPP_FILE")?)?;
    let (before, rest) = source
        .split_once("while (smallestCarOfNumber")
        .ok_or_else(|| anyhow::anyhow!("C++ fixture must contain the reported while condition"))?;
    let suffix = &rest[rest
        .find(')')
        .ok_or_else(|| anyhow::anyhow!("Missing closing parenthesis"))?..];
    let input = format!("{before}while (smallestCarOfNumbe#[|)]#{}", &suffix[1..]);
    let typed = format!("{before}while (smallestCarOfNumber");
    let mut config = model_config(std::env::var("HELIX_TEST_AI_ENDPOINT")?);
    if let Ok(model) = std::env::var("HELIX_TEST_AI_MODEL") {
        config.editor.ai_completion.model = model;
    }
    config.editor.ai_completion.prompt_mode = AiCompletionPromptMode::Line;
    config.editor.ai_completion.timeout_ms = 5000;
    config.editor.ai_completion.debounce_ms = 200;
    config.editor.ai_completion.max_tokens = 48;
    config.editor.ai_completion.prefix_chars = 3000;
    config.editor.ai_completion.suffix_chars = 500;
    // Open a disposable copy so neither accepting nor closing can write to the
    // user's file. Its .hpp name supplies the actual language to the model.
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("stack_track.hpp");
    std::fs::write(&path, "")?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path, None)
        .with_input_text(input)
        .build()?;
    keys(&mut app, "ir").await?;
    wait_for_preview(&mut app).await;
    let suggestion = current_ref!(app.editor)
        .1
        .ai_completion
        .as_ref()
        .unwrap()
        .annotation
        .text
        .to_string();
    assert!(!suggestion.contains(['\r', '\n']));
    assert!(!suggestion.contains("smallestCarOfNumber"));
    assert_eq!(text(&app), format!("{typed}{suffix}"));
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), format!("{typed}{suggestion}{suffix}"));
    println!("Live C++ line preview and Shift+Right acceptance: {suggestion:?}");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_closing_last_view_with_feature_disabled() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().build()?;
    keys(&mut app, "<C-w>q").await?;
    assert!(app.editor.should_close());
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_multi_key_accept_repeats_and_records() -> anyhow::Result<()> {
    let mut config = test_config();
    config.keys = toml::from_str("[insert.j]\nk = 'ai_completion_accept'\n")?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "Qiab").await?;
    show(&mut app, "C");
    keys(&mut app, "jk<esc>Q:new<ret>.").await?;
    assert_eq!(
        text(&app),
        format!("abC{}", helix_core::NATIVE_LINE_ENDING.as_str())
    );
    assert_eq!(app.editor.mode(), helix_view::document::Mode::Normal);
    keys(&mut app, ":new<ret>q").await?;
    assert_eq!(
        text(&app),
        format!("abC{}", helix_core::NATIVE_LINE_ENDING.as_str())
    );
    app.close().await;
    Ok(())
}

async fn sequence_accept_and_replay(
    command: &str,
    tail: &str,
    expected: &str,
) -> anyhow::Result<()> {
    let mut config = test_config();
    config.keys = toml::from_str(&format!(
        "[insert]\nF4 = ['ai_completion_accept', '{command}']\n"
    ))?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "Qiab").await?;
    show(&mut app, "C");
    keys(&mut app, &format!("<F4>{tail}Q")).await?;
    assert_eq!(text(&app), format!("{expected}\n"));
    assert_eq!(app.editor.mode(), helix_view::document::Mode::Normal);
    keys(&mut app, ":new<ret>.").await?;
    assert_eq!(
        text(&app),
        format!("{expected}{}", helix_core::NATIVE_LINE_ENDING.as_str())
    );
    assert_eq!(app.editor.mode(), helix_view::document::Mode::Normal);
    keys(&mut app, ":new<ret>q").await?;
    assert_eq!(
        text(&app),
        format!("{expected}{}", helix_core::NATIVE_LINE_ENDING.as_str())
    );
    assert_eq!(app.editor.mode(), helix_view::document::Mode::Normal);
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_sequence_accept_then_normal_mode() -> anyhow::Result<()> {
    sequence_accept_and_replay("normal_mode", "", "abC").await
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_completion_sequence_accept_then_move() -> anyhow::Result<()> {
    sequence_accept_and_replay("move_char_left", "X<esc>", "abXC").await
}
