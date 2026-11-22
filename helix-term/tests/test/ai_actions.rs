use super::{
    ai_doc::{keys, server, settle},
    helpers::{test_config, AppBuilder},
};
use anyhow::{Context, Result};
use helix_term::application::Application;
use helix_view::{
    ai::{AiProviderConfig, AiProviderType, AiProxy},
    current_ref,
};
use serde_json::{json, Value};
use std::{io::Write, time::Duration};
use tempfile::NamedTempFile;
use tokio::sync::mpsc::UnboundedReceiver;

fn reply(content: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":content}}]})
}

fn app(endpoint: String, source: &str) -> Result<(Application, NamedTempFile)> {
    app_with_models(endpoint, source, ["test-chat", "test-generate"])
}

fn app_with_models(
    endpoint: String,
    source: &str,
    models: [&str; 2],
) -> Result<(Application, NamedTempFile)> {
    let mut file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    file.write_all(source.as_bytes())?;
    let mut config = test_config();
    config.editor.idle_timeout = Duration::from_millis(20);
    config.editor.ai_completion.enabled = false;
    config.editor.ai_explain.enabled = true;
    config.editor.ai_explain.provider = Some("test-chat".into());
    config.editor.ai_generate.enabled = true;
    config.editor.ai_generate.provider = Some("test-generate".into());
    for (name, model) in ["test-chat", "test-generate"].into_iter().zip(models) {
        config.editor.ai.providers.insert(
            name.into(),
            AiProviderConfig {
                kind: AiProviderType::OpenaiCompatible,
                endpoint: endpoint.clone(),
                model: model.into(),
                context_size: 8192,
                proxy: AiProxy::None,
                ..Default::default()
            },
        );
    }
    let app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    Ok((app, file))
}

async fn request(requests: &mut UnboundedReceiver<Value>) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(5), requests.recv())
        .await
        .context("no request reached the selected AI provider")?
        .context("model server closed before receiving a request")
}

fn text(app: &Application) -> String {
    current_ref!(app.editor).1.text().to_string()
}

fn screen(app: &Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn wait_for_screen(app: &mut Application, expected: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            settle(app).await;
            if screen(app).contains(expected) {
                break;
            }
        }
    })
    .await
    .with_context(|| format!("missing {expected:?} in\n{}", screen(app)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panel_dispatches_selected_text_to_chat_and_cursor_to_generation() -> Result<()> {
    let (endpoint, mut requests, task) =
        server(0, vec![reply("answer-one"), reply("new_code")]).await;
    let source = "selected_word\nnext_line\n";
    let (mut app, _file) = app(endpoint, source)?;
    keys(&mut app, ":set-option ai-explain.enabled false<ret>").await?;
    assert!(!app.editor.config().ai_explain.enabled);
    keys(&mut app, "x:ai<ret><ret><space>").await?;
    assert!(!app.editor.config().ai_explain.enabled);
    keys(&mut app, "<ret>").await?;
    assert!(app.editor.config().ai_explain.enabled);
    keys(&mut app, ":ai<ret><ret>").await?;
    let explanation = request(&mut requests).await?;
    assert_eq!(explanation["model"], "test-chat");
    assert!(explanation["messages"]
        .to_string()
        .contains("selected_word"));
    wait_for_screen(&mut app, "answer-one").await?;
    keys(&mut app, "<esc>;:ai<ret><ret>add code<ret>").await?;
    let generation = request(&mut requests).await?;
    assert_eq!(generation["model"], "test-generate");
    assert!(generation["messages"].to_string().contains("add code"));
    wait_for_screen(&mut app, "new_code").await?;
    assert_eq!(text(&app), source);
    keys(&mut app, "<esc>").await?;
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_reopens_with_history_and_a_new_command_starts_a_fresh_selection() -> Result<()> {
    let (endpoint, mut requests, task) = server(
        0,
        vec![
            reply("first-answer"),
            reply("second-answer"),
            reply("fresh-answer"),
        ],
    )
    .await;
    let source = "selected_word\nother_word\n";
    let (mut app, _file) = app(endpoint, source)?;
    keys(&mut app, "x:ai-ask first question<ret>").await?;
    let first = request(&mut requests).await?;
    assert_eq!(first["model"], "test-chat");
    let context = first["messages"].to_string();
    assert!(
        context.contains("selected_word") && context.contains("first question"),
        "{context}"
    );
    wait_for_screen(&mut app, "first-answer").await?;
    keys(&mut app, "<esc>gj:ai-explain-show<ret>").await?;
    wait_for_screen(&mut app, "first-answer").await?;
    keys(&mut app, "follow up<ret>").await?;
    let follow = request(&mut requests).await?;
    let conversation = follow["messages"].as_array().unwrap();
    assert!(conversation
        .iter()
        .any(|message| message["role"] == "assistant" && message["content"] == "first-answer"));
    assert!(conversation.last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("follow up"));
    assert!(follow["messages"].to_string().contains("selected_word"));
    wait_for_screen(&mut app, "second-answer").await?;
    keys(&mut app, "<esc>gex:ai-explain next question<ret>").await?;
    let next = request(&mut requests).await?["messages"].to_string();
    assert!(
        next.contains("other_word") && next.contains("next question"),
        "{next}"
    );
    assert!(
        !next.contains("first-answer") && !next.contains("first question"),
        "{next}"
    );
    wait_for_screen(&mut app, "fresh-answer").await?;
    assert_eq!(text(&app), source);
    assert!(!current_ref!(app.editor).1.is_modified());
    keys(&mut app, "<esc>").await?;
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generate_command_previews_at_cursor_then_accepts_as_one_undoable_edit() -> Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply("generated()\n  second()")]).await;
    let source = "before\n左right\nafter\n";
    let (mut app, file) = app(endpoint, source)?;
    let version = current_ref!(app.editor).1.version();
    keys(&mut app, "jl:ai-generate add call<ret>").await?;
    let generation = request(&mut requests).await?;
    assert_eq!(generation["model"], "test-generate");
    let messages = generation["messages"].to_string();
    for context in ["add call", "左", "right"] {
        assert!(messages.contains(context), "{messages}");
    }
    wait_for_screen(&mut app, "generated()").await?;
    let rendered = screen(&app);
    let first = rendered
        .lines()
        .position(|line| line.contains("generated()"))
        .unwrap();
    let (view, doc) = current_ref!(app.editor);
    let inner = view.inner_area(doc);
    // The source character 左 is two cells wide; check inside each virtual line.
    for (x, y, symbol) in [(inner.x + 3, first, "e"), (inner.x + 2, first + 1, "s")] {
        let cell = app.test_screen().get(x, y as u16).unwrap();
        assert_eq!(cell.symbol.as_str(), symbol);
        assert_eq!(cell.fg, helix_view::graphics::Color::Gray);
    }
    assert!(
        rendered
            .lines()
            .nth(first + 1)
            .is_some_and(|line| line.contains("second()right")),
        "{rendered}"
    );
    assert_eq!(text(&app), source);
    assert_eq!(current_ref!(app.editor).1.version(), version);
    assert!(!current_ref!(app.editor).1.is_modified());
    keys(&mut app, "<S-right>").await?;
    assert_eq!(
        text(&app),
        "before\n左generated()\n  second()right\nafter\n"
    );
    keys(&mut app, "u").await?;
    assert_eq!(text(&app), source);
    assert_eq!(std::fs::read_to_string(file.path())?, source);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn insert_shortcut_opens_prompt_and_escape_discards_generated_text() -> Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply("discard_me")]).await;
    let source = "original\n";
    let (mut app, _file) = app(endpoint, source)?;
    keys(&mut app, r"i<C-\>add a line<ret>").await?;
    let generation = request(&mut requests).await?;
    assert!(generation["messages"].to_string().contains("add a line"));
    wait_for_screen(&mut app, "discard_me").await?;
    keys(&mut app, "<esc>").await?;
    assert!(!screen(&app).contains("discard_me"));
    assert_eq!(text(&app), source);
    keys(&mut app, "X<esc>").await?;
    assert_eq!(text(&app), "Xoriginal\n");
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shortcut_refines_preview_before_inserting_only_the_revised_code() -> Result<()> {
    let (endpoint, mut requests, task) =
        server(0, vec![reply("draft_call()"), reply("final_call()")]).await;
    let (mut app, _file) = app(endpoint, "original\n")?;
    keys(&mut app, r"<C-\>first requirement<ret>").await?;
    request(&mut requests).await?;
    wait_for_screen(&mut app, "draft_call()").await?;
    keys(&mut app, r"<C-\>use final name<ret>").await?;
    let messages = request(&mut requests).await?["messages"].to_string();
    for context in ["first requirement", "draft_call()", "use final name"] {
        assert!(messages.contains(context), "{messages}");
    }
    wait_for_screen(&mut app, "final_call()").await?;
    assert_eq!(text(&app), "original\n");
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "final_call()original\n");
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_while_request_is_in_flight_discards_late_generation() -> Result<()> {
    let (endpoint, mut requests, task) = server(600, vec![reply("stale_code")]).await;
    let (mut app, _file) = app(endpoint, "original\n")?;
    keys(&mut app, r"i<C-\>add code<ret>").await?;
    request(&mut requests).await?;
    keys(&mut app, "X").await?;
    task.await?;
    settle(&mut app).await;
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "Xoriginal\n");
    assert!(!screen(&app).contains("stale_code"));
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moving_away_and_back_discards_pending_generation_without_an_edit() -> Result<()> {
    let (endpoint, mut requests, task) = server(600, vec![reply("stale_code")]).await;
    let (mut app, _file) = app(endpoint, "original\n")?;
    let version = current_ref!(app.editor).1.version();
    keys(&mut app, ":ai-generate add code<ret>").await?;
    request(&mut requests).await?;
    keys(&mut app, "lh").await?;
    assert!(current_ref!(app.editor).1.ai_generate.is_none());
    task.await?;
    settle(&mut app).await;
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), "original\n");
    assert_eq!(current_ref!(app.editor).1.version(), version);
    assert!(!screen(&app).contains("stale_code"));
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_chat_restores_the_question_for_retry() -> Result<()> {
    let mut truncated = reply("incomplete");
    truncated["choices"][0]["finish_reason"] = json!("length");
    let (endpoint, mut requests, task) = server(0, vec![truncated, reply("complete-answer")]).await;
    let (mut app, _file) = app(endpoint, "selected_word\n")?;
    keys(&mut app, "x:ai-ask retry-me<ret>").await?;
    let first = request(&mut requests).await?;
    wait_for_screen(&mut app, "> retry-me").await?;
    keys(&mut app, "<ret>").await?;
    let retried = request(&mut requests).await?;
    assert_eq!(retried["messages"], first["messages"]);
    wait_for_screen(&mut app, "complete-answer").await?;
    assert_eq!(text(&app), "selected_word\n");
    keys(&mut app, "<esc>").await?;
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canceling_a_pending_question_restores_the_draft_and_discards_the_answer() -> Result<()> {
    let (endpoint, mut requests, task) = server(600, vec![reply("canceled-answer")]).await;
    let (mut app, _file) = app(endpoint, "selected_word\n")?;
    keys(&mut app, "x:ai-ask keep-this-question<ret>").await?;
    request(&mut requests).await?;
    keys(&mut app, "<C-c>").await?;
    task.await?;
    wait_for_screen(&mut app, "> keep-this-question").await?;
    assert!(!screen(&app).contains("canceled-answer"));
    let chat = app.editor.ai_chat.as_ref().unwrap();
    assert!(chat.pending.is_none() && chat.turns.is_empty());
    keys(&mut app, "<esc>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires HELIX_TEST_AI_ENDPOINT and HELIX_TEST_AI_MODEL"]
async fn live_model_explains_answers_and_generates_python_at_the_cursor() -> Result<()> {
    let endpoint = std::env::var("HELIX_TEST_AI_ENDPOINT")?;
    let model = std::env::var("HELIX_TEST_AI_MODEL")?;
    let source = "def add(a, b):\n    return a + b\n";
    let (mut app, _file) = app_with_models(endpoint, source, [&model; 2])?;
    keys(
        &mut app,
        ":set-language python<ret>%:ai-explain 请用一句中文解释函数的返回值<ret>",
    )
    .await?;
    let answer = live_answer(&mut app).await?;
    println!("Explanation: {answer}");
    assert!(answer.contains('加') || answer.contains('和'), "{answer}");
    keys(
        &mut app,
        "调用 add(2, 3) 的返回值是多少？请用阿拉伯数字回答。<ret>",
    )
    .await?;
    let follow_up = live_answer(&mut app).await?;
    println!("Follow-up: {follow_up}");
    assert!(follow_up.contains('5'), "{follow_up}");
    assert_eq!(text(&app), source);

    keys(&mut app, "<esc>:new<ret>:set-language python<ret>").await?;
    let original = text(&app);
    keys(&mut app, ":ai-generate Define sum_pair(left, right) in Python. Only insert a two-line function with exactly this signature and return left + right. No comments or type hints.<ret>").await?;
    tokio::time::timeout(Duration::from_secs(125), async {
        loop {
            settle(&mut app).await;
            let op = current_ref!(app.editor).1.ai_generate.as_ref();
            if op.is_none()
                || op.is_some_and(|generation| generation.annotation.is_some())
                || app.editor.is_err()
            {
                break;
            }
        }
    })
    .await?;
    assert_eq!(text(&app), original);
    let draft = current_ref!(app.editor)
        .1
        .ai_generate
        .as_ref()
        .and_then(|op| op.annotation.as_ref())
        .with_context(|| format!("no generated code: {:?}", app.editor.get_status()))?
        .text
        .to_string();
    println!("Generated Python:\n{draft}");
    // Check the requested Python expression without executing model-produced code.
    let lines: Vec<_> = draft.trim().lines().collect();
    assert_eq!(lines.len(), 2, "{draft}");
    assert_eq!(
        lines[0]
            .strip_prefix("def ")
            .context("missing Python function definition")?
            .split_whitespace()
            .collect::<String>(),
        "sum_pair(left,right):"
    );
    assert!(lines[1].starts_with([' ', '\t']), "{draft}");
    assert_eq!(
        lines[1]
            .trim_start()
            .strip_prefix("return ")
            .context("missing Python return statement")?
            .split_whitespace()
            .collect::<String>(),
        "left+right"
    );
    keys(&mut app, "<S-right>").await?;
    assert_eq!(text(&app), format!("{draft}{original}"));
    app.close().await;
    Ok(())
}

async fn live_answer(app: &mut Application) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(125), async {
        loop {
            settle(app).await;
            let chat = app.editor.ai_chat.as_ref().context("chat did not open")?;
            if chat.pending.is_none() {
                anyhow::ensure!(chat.notice.is_empty(), "{}", chat.notice);
                return chat
                    .turns
                    .last()
                    .map(|(_, answer)| answer.clone())
                    .context("missing answer");
            }
        }
    })
    .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_local_model_list_stages_applies_and_restores_automatic_choice() -> Result<()> {
    let mut config = test_config();
    config.editor.ai.enabled = false;
    config.editor.ai_completion.local_only = true;
    config.editor.ai_completion.provider = Some("compact".into());
    for (name, model, port, gpu_layers) in [
        ("!alpha", "Alpha-model", 18081, 99),
        ("!beta", "Beta-model", 18082, 99),
        ("compact", "Inline-only-model", 18083, 0),
    ] {
        config.editor.ai.providers.insert(
            name.into(),
            AiProviderConfig {
                kind: AiProviderType::LlamaCpp,
                endpoint: format!("http://127.0.0.1:{port}/v1/chat/completions"),
                model: model.into(),
                model_path: Some(format!("{model}.gguf").into()),
                gpu_layers,
                ..Default::default()
            },
        );
    }
    config
        .editor
        .ai
        .providers
        .insert("cloud".into(), AiProviderConfig::default_deepseek());
    let mut app = AppBuilder::new().with_config(config).build()?;
    keys(&mut app, ":ai<ret>s<end>m").await?;
    let list = screen(&app);
    for model in ["Alpha-model", "Beta-model", "!alpha", "!beta"] {
        assert!(list.contains(model), "{list}");
    }
    assert!(
        !list.contains("Inline-only-model") && !list.contains("deepseek-flash"),
        "{list}"
    );
    keys(&mut app, "jj<ret>").await?;
    assert!(app.editor.config().ai.local_generation_provider.is_none());
    assert!(screen(&app).contains("!beta"));
    keys(&mut app, "<tab><tab>m<home><esc><ret>").await?;
    assert_eq!(
        app.editor.config().ai.local_generation_provider.as_deref(),
        Some("!beta")
    );
    keys(&mut app, ":ai<ret>s<end>m<home><ret><esc>").await?;
    assert_eq!(
        app.editor.config().ai.local_generation_provider.as_deref(),
        Some("!beta")
    );
    keys(&mut app, ":ai<ret>s<end>m<home><ret><ret>").await?;
    assert!(app.editor.config().ai.local_generation_provider.is_none());
    assert_eq!(
        app.editor.config().ai_completion.provider.as_deref(),
        Some("compact")
    );
    app.close().await;
    Ok(())
}
