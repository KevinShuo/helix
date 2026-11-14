use super::helpers::*;
use helix_term::application::Application;
use helix_view::{current_ref, input::parse_macro};
use serde_json::{json, Value};
use std::{io::Write, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

const DOC: &str = r#"{"summary":"返回输入值。","params":[{"name":"a","description":"输入值。"}],"returns":"输入值。"}"#;

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for key in parse_macro(input)? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(key.into());
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(key.into());
        tx.send(Ok(event))?;
    }
    let mut stream = UnboundedReceiverStream::new(rx);
    tokio::time::timeout(
        Duration::from_secs(5),
        app.event_loop_until_idle(&mut stream),
    )
    .await?;
    Ok(())
}

async fn settle(app: &mut Application) {
    app.editor.reset_idle_timer();
    tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(app))
        .await
        .unwrap();
}

async fn server(
    delay: u64,
    replies: Vec<Value>,
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
        for reply in replies {
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
            tx.send(serde_json::from_slice(&bytes[end..end + length]).unwrap())
                .unwrap();
            let response = reply.to_string();
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let _ = socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await;
        }
    });
    (endpoint, rx, task)
}

fn reply(content: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]})
}

fn app(
    endpoint: String,
    suffix: &str,
    source: &str,
) -> anyhow::Result<(Application, tempfile::NamedTempFile)> {
    let mut file = tempfile::Builder::new().suffix(suffix).tempfile()?;
    file.write_all(source.as_bytes())?;
    let mut config = test_config();
    config.editor.ai_doc.enabled = true;
    config.editor.ai_completion.enabled = false;
    config.editor.ai_completion.endpoint = endpoint;
    config.editor.idle_timeout = Duration::from_millis(50);
    config.editor.ai_completion.context_size = 8192;
    let app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    Ok((app, file))
}

fn text(app: &Application) -> String {
    current_ref!(app.editor).1.text().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_binds_target_when_command_is_submitted() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(DOC)]).await;
    let (mut app, _file) = app(
        endpoint,
        ".py",
        "def f(a):\n    return a\n\ndef g(a):\n    return a + 1\n",
    )?;
    keys(&mut app, ":ai-doc<ret>ge").await?;
    let request = tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await?
        .unwrap();
    assert!(request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("\"name\":\"f\""));
    settle(&mut app).await;
    keys(&mut app, "<esc>").await?;
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_native_command_previews_then_accepts_and_undoes() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(DOC)]).await;
    let original = "def f(a: int) -> int:\n    return a\n";
    let (mut app, _file) = app(endpoint, ".py", original)?;
    let version = current_ref!(app.editor).1.version();
    keys(&mut app, ":ai-doc<ret>").await?;
    let request = tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await?
        .unwrap();
    assert_eq!(request["max_tokens"], 768);
    assert!(request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("    return a"));
    settle(&mut app).await;
    assert_eq!(text(&app), original);
    assert_eq!(current_ref!(app.editor).1.version(), version);
    assert!(!current_ref!(app.editor).1.is_modified());
    keys(&mut app, "ixxxx<ret>").await?;
    let accepted = text(&app);
    assert!(accepted.contains(":param a: 输入值。"), "{accepted:?}");
    assert!(!accepted.contains("xxxx"));
    keys(&mut app, "u").await?;
    assert_eq!(text(&app), original);
    keys(&mut app, "U").await?;
    assert_eq!(text(&app), accepted);
    app.close().await;
    task.abort();
    Ok(())
}

async fn c_language(suffix: &str) -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(DOC)]).await;
    let (mut app, _file) = app(endpoint, suffix, "int f(int a) { return a; }\n")?;
    keys(&mut app, ":ai-doc<ret>").await?;
    tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await
        .expect(&format!("no request, status={:?}", app.editor.get_status()))
        .unwrap();
    settle(&mut app).await;
    keys(&mut app, "<ret>").await?;
    assert!(text(&app).starts_with("/*\n * 返回输入值。\n"));
    assert!(text(&app).ends_with(" */\nint f(int a) { return a; }\n"));
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_c_inserts_rest() -> anyhow::Result<()> {
    c_language(".c").await
}
#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_cpp_inserts_rest() -> anyhow::Result<()> {
    c_language(".cpp").await
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_cancel_retry_and_truncated_response_leave_source_intact() -> anyhow::Result<()> {
    let mut truncated = reply(DOC);
    truncated["choices"][0]["finish_reason"] = json!("length");
    let (endpoint, mut requests, task) = server(0, vec![reply(DOC), reply(DOC), truncated]).await;
    let original = "def f(a):\n    return a\n";
    let (mut app, _file) = app(endpoint, ".py", original)?;
    keys(&mut app, ":ai-doc<ret>").await?;
    requests.recv().await.unwrap();
    settle(&mut app).await;
    keys(&mut app, "r").await?;
    requests.recv().await.unwrap();
    settle(&mut app).await;
    keys(&mut app, "<esc>").await?;
    assert_eq!(text(&app), original);
    keys(&mut app, ":ai-doc<ret>").await?;
    requests.recv().await.unwrap();
    settle(&mut app).await;
    assert_eq!(text(&app), original);
    assert!(app.editor.get_status().unwrap().0.contains("未完整结束"));
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_delayed_result_is_invalidated_by_edit_and_explicit_cancel() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(2000, vec![reply(DOC), reply(DOC)]).await;
    let (mut app, _file) = app(endpoint, ".py", "def f(a):\n    return a\n")?;
    keys(&mut app, ":ai-doc<ret>i#<esc>").await?;
    // A request can be canceled before reaching the socket; either way no result may apply.
    let _ = tokio::time::timeout(Duration::from_millis(500), requests.recv()).await;
    settle(&mut app).await;
    assert!(!text(&app).contains("返回输入值"));
    keys(&mut app, "u:ai-doc<ret>").await?;
    let _ = tokio::time::timeout(Duration::from_millis(500), requests.recv()).await;
    keys(&mut app, ":ai-doc-cancel<ret>").await?;
    settle(&mut app).await;
    assert!(!text(&app).contains("返回输入值"));
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_waits_for_an_existing_prompt_without_stealing_input() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(300, vec![reply(DOC)]).await;
    let original = "def f(a):\n    return a\n";
    let (mut app, _file) = app(endpoint, ".py", original)?;
    keys(&mut app, ":ai-doc<ret>").await?;
    requests.recv().await.unwrap();
    keys(&mut app, ":").await?;
    tokio::time::sleep(Duration::from_millis(350)).await;
    settle(&mut app).await;
    assert!(app
        .editor
        .ai_doc
        .as_ref()
        .is_some_and(|op| op.text.is_some() && !op.preview));
    assert_eq!(text(&app), original);
    keys(&mut app, "<esc>").await?;
    assert!(app.editor.ai_doc.as_ref().is_some_and(|op| op.preview));
    keys(&mut app, "<ret>").await?;
    assert!(text(&app).contains(":param a:"));
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_focus_change_cancels_even_when_returning_to_original_buffer() -> anyhow::Result<()>
{
    let (endpoint, mut requests, task) = server(300, vec![reply(DOC)]).await;
    let (mut app, _file) = app(endpoint, ".py", "def f(a):\n    return a\n")?;
    keys(&mut app, ":ai-doc<ret>").await?;
    requests.recv().await.unwrap();
    keys(&mut app, ":new<ret>:buffer-previous<ret>").await?;
    tokio::time::sleep(Duration::from_millis(350)).await;
    settle(&mut app).await;
    assert!(app.editor.ai_doc.is_none());
    assert!(!text(&app).contains("返回输入值"));
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_preview_refuses_readonly_target_and_preserves_cursor() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(DOC)]).await;
    let original = "def f(a):\n    return a\n";
    let (mut app, _file) = app(endpoint, ".py", original)?;
    keys(&mut app, ":ai-doc<ret>").await?;
    requests.recv().await.unwrap();
    settle(&mut app).await;
    helix_view::current!(app.editor).1.readonly = true;
    keys(&mut app, "<ret>").await?;
    assert_eq!(text(&app), original);
    assert!(app.editor.ai_doc.is_none());
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_accept_reveals_inserted_docs_without_moving_logical_cursor() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(200, vec![reply(DOC)]).await;
    let original = format!("def f(a):\n{}    return a\n", "    a += 0\n".repeat(200));
    let (mut app, _file) = app(endpoint, ".py", &original)?;
    keys(&mut app, ":ai-doc<ret>ge").await?;
    requests.recv().await.unwrap();
    let cursor_before = {
        let (view, doc) = current_ref!(app.editor);
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..))
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    settle(&mut app).await;
    keys(&mut app, "<ret>").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(cursor_before > 50);
    let inserted_len = doc.text().len_chars() - original.chars().count();
    assert_eq!(
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..)),
        cursor_before + inserted_len
    );
    assert!(
        doc.view_offset(view.id).anchor <= 10,
        "new documentation must be visible"
    );
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_language_change_invalidates_pending_documentation() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(1000, vec![reply(DOC)]).await;
    let (mut app, _file) = app(endpoint, ".py", "def f(a):\n    return a\n")?;
    keys(&mut app, ":ai-doc<ret>").await?;
    requests.recv().await.unwrap();
    keys(&mut app, ":lang c<ret>").await?;
    assert!(app.editor.ai_doc.is_none());
    app.close().await;
    task.abort();
    Ok(())
}

#[cfg(feature = "git")]
#[tokio::test(flavor = "multi_thread")]
async fn ai_doc_preview_opens_above_an_unfocused_git_dock() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(DOC)]).await;
    let (mut app, _file) = app(endpoint, ".py", "def f(a):\n    return a\n")?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vg<C-w><esc>").await?;
    assert!(app.editor.tree.area().height < 39, "Git must remain docked");
    keys(&mut app, ":ai-doc<ret>").await?;
    tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await?
        .unwrap();
    settle(&mut app).await;
    assert!(app.editor.ai_doc.as_ref().is_some_and(|op| op.preview));
    keys(&mut app, "<ret>").await?;
    assert!(text(&app).contains(":param a:"));
    app.close().await;
    task.abort();
    Ok(())
}
