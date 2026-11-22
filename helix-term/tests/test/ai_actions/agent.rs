use super::{
    scopes::{project, project_with_provider},
    *,
};
use crate::test::ai_doc::server_with_status;
use std::fs;

pub(super) fn call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{
        "role":"assistant","content":null,
        "tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]
    }}]})
}

pub(super) async fn next_request(
    app: &mut Application,
    requests: &mut UnboundedReceiver<Value>,
) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            settle(app).await;
            match requests.try_recv() {
                Ok(message) => return Ok(message),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => continue,
                Err(e) => return Err(anyhow::Error::new(e)),
            }
        }
    })
    .await
    .with_context(|| {
        format!(
            "missing model request; status={:?}\n{}",
            app.editor.get_status(),
            screen(app)
        )
    })?
}

fn assert_tools(request: &Value) {
    let names: Vec<_> = request["tools"]
        .as_array()
        .expect("project request needs tool schemas")
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names.len(),
        6,
        "unexpected workspace tool offered: {names:?}"
    );
    for name in [
        "read_file",
        "search_text",
        "find_files",
        "file_structure",
        "find_definition",
        "find_references",
    ] {
        assert!(names.contains(&name), "{names:?}");
    }
}

pub(super) fn tool_result<'a>(request: &'a Value, id: &str, name: &str) -> &'a str {
    let messages = request["messages"].as_array().unwrap();
    let sent = messages
        .iter()
        .position(|m| {
            m["role"] == "assistant"
                && m["tool_calls"].as_array().is_some_and(|calls| {
                    calls
                        .iter()
                        .any(|c| c["id"] == id && c["function"]["name"] == name)
                })
        })
        .expect("assistant's requested tool call is missing");
    let result = messages
        .iter()
        .position(|m| m["role"] == "tool" && m["tool_call_id"] == id)
        .expect("tool result missing matching call ID");
    assert!(sent < result, "{messages:?}");
    messages[result]["content"]
        .as_str()
        .expect("tool result must be readable text")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_tools_resolve_both_definitions_then_read_the_unsaved_sibling_buffer() -> Result<()>
{
    let disk = "# ledger\ndef tally(amount):\n    return amount + 1\n";
    let (endpoint, mut rx, service) = server(
        0,
        vec![
            call(
                "lookup-1",
                "find_definition",
                json!({"symbol":"tally","path":"ledger.py","line":2}),
            ),
            call(
                "read-2",
                "read_file",
                json!({"path":"ledger.py","start_line":3,"end_line":3}),
            ),
            reply("UNSAVED_PROOF is returned at `ledger.py:3`."),
        ],
    )
    .await;
    let (mut app, dir) = project(endpoint)?;
    let ledger = dir.path().join("ledger.py");
    fs::write(&ledger, disk)?;
    fs::write(
        dir.path().join("archive.py"),
        "def tally(amount):\n    return amount * 2\n",
    )?;
    app.editor
        .open(&ledger, helix_view::editor::Action::Replace)?;
    keys(&mut app, "ggjjA + UNSAVED_PROOF<esc>").await?;
    let edited = text(&app);
    assert!(edited.contains("return amount + 1 + UNSAVED_PROOF"));
    let ledger_id = current_ref!(app.editor).1.id();
    let version = current_ref!(app.editor).1.version();
    keys(
        &mut app,
        ":buffer-previous<ret>:ai-explain project where is tally defined?<ret>",
    )
    .await?;
    assert_ne!(current_ref!(app.editor).1.id(), ledger_id);
    let start = next_request(&mut app, &mut rx).await?;
    assert_tools(&start);
    let definitions = next_request(&mut app, &mut rx).await?;
    let definitions = tool_result(&definitions, "lookup-1", "find_definition");
    assert!(
        definitions.contains("文本") && definitions.contains("候选"),
        "{definitions}"
    );
    assert!(
        definitions.contains("ledger.py:2") && definitions.contains("archive.py:1"),
        "{definitions}"
    );
    let read = next_request(&mut app, &mut rx).await?;
    let line = tool_result(&read, "read-2", "read_file");
    assert!(
        line.contains("ledger.py:3") && line.contains("return amount + 1 + UNSAVED_PROOF"),
        "{line}"
    );
    assert!(!line.contains("def tally("), "{line}");
    wait_for_turns(&mut app, 1).await?;
    let chat = app.editor.ai_chat.as_ref().unwrap();
    assert!(chat
        .locations
        .iter()
        .any(|(p, n)| p.ends_with("ledger.py") && *n == 3));
    keys(&mut app, "<A-t>").await?;
    let trace = screen(&app);
    assert!(trace.contains("查阅记录"), "{trace}");
    assert!(
        trace.contains("tally") || trace.contains("ledger.py"),
        "{trace}"
    );
    keys(&mut app, "<esc><A-g><ret>").await?;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(doc.id(), ledger_id);
    assert_eq!(doc.version(), version);
    assert_eq!(doc.text().to_string(), edited);
    let rope = doc.text().slice(..);
    assert_eq!(
        rope.char_to_line(doc.selection(view.id).primary().cursor(rope)),
        2
    );
    assert_eq!(fs::read_to_string(&ledger)?, disk);
    app.close().await;
    service.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alt_e_enables_project_tools_for_selection_followups_and_preserves_the_original_snapshot(
) -> Result<()> {
    let (endpoint, mut rx, service) = server(
        0,
        vec![
            reply("ordinary-answer"),
            call(
                "extra-read",
                "read_file",
                json!({"path":"README.md", "start_line":1, "end_line":1}),
            ),
            reply("extended-answer"),
            reply("local-followup"),
        ],
    )
    .await;
    let (mut app, _dir) = project(endpoint)?;
    keys(&mut app, "jx:ai-explain selection original question<ret>").await?;
    let baseline = next_request(&mut app, &mut rx).await?;
    assert!(baseline.get("tools").is_none());
    wait_for_screen(&mut app, "ordinary-answer").await?;
    let frozen = app.editor.ai_chat.as_ref().unwrap().source.clone();
    keys(&mut app, "<esc>").await?;
    let other_project = tempfile::tempdir()?;
    fs::create_dir(other_project.path().join(".git"))?;
    let other = other_project.path().join("README.md");
    fs::write(&other, "# UNRELATED_PROJECT_MARKER\n")?;
    app.editor
        .open(&other, helix_view::editor::Action::Replace)?;
    keys(&mut app, ":ai-explain-show<ret><A-e>").await?;
    let expanded = next_request(&mut app, &mut rx).await?;
    assert_tools(&expanded);
    let messages = expanded["messages"].as_array().unwrap();
    assert!(messages.iter().any(|m| m["role"] == "user"
        && m["content"]
            .as_str()
            .is_some_and(|s| s.contains("original question"))));
    let continuation = next_request(&mut app, &mut rx).await?;
    assert!(tool_result(&continuation, "extra-read", "read_file").contains("Fixture Arithmetic"));
    assert!(!continuation["messages"]
        .to_string()
        .contains("UNRELATED_PROJECT_MARKER"));
    wait_for_screen(&mut app, "extended-answer").await?;
    keys(&mut app, "just the selected line?<ret>").await?;
    let local = next_request(&mut app, &mut rx).await?;
    assert_tools(&local);
    assert!(local["messages"]
        .to_string()
        .contains("just the selected line?"));
    wait_for_screen(&mut app, "local-followup").await?;
    assert_eq!(app.editor.ai_chat.as_ref().unwrap().source, frozen);
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    service.await?;
    Ok(())
}

async fn assert_plain_fallback(kind: AiProviderType, status: u16) -> Result<()> {
    let error = json!({"error":{"message":"Unrecognized request argument: tools (function calling is not supported)"}});
    let (endpoint, mut rx, service) =
        server_with_status(0, vec![(status, error), (200, reply("compat-answer"))]).await;
    let (mut app, _dir) = project_with_provider(endpoint, kind, 8192)?;
    keys(&mut app, ":ai-explain project fixture question<ret>").await?;
    assert_tools(&next_request(&mut app, &mut rx).await?);
    let plain = next_request(&mut app, &mut rx).await?;
    assert!(
        plain.get("tools").is_none() && plain.get("tool_choice").is_none(),
        "{plain}"
    );
    assert!(plain["messages"].to_string().contains("fixture question"));
    assert!(!plain["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool"));
    wait_for_screen(&mut app, "compat-answer").await?;
    assert_eq!(app.editor.ai_chat.as_ref().unwrap().turns.len(), 1);
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    service.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_model_rejecting_tools_falls_back_to_a_plain_project_answer() -> Result<()> {
    assert_plain_fallback(AiProviderType::OpenaiCompatible, 400).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deepseek_rejecting_tools_falls_back_to_a_plain_project_answer() -> Result<()> {
    assert_plain_fallback(AiProviderType::Deepseek, 422).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_authentication_error_does_not_retry_without_tools() -> Result<()> {
    let error = json!({"error":{"message":"Unauthorized to access tools using these credentials"}});
    let (endpoint, mut rx, service) =
        server_with_status(0, vec![(401, error), (200, reply("wrong-retry"))]).await;
    let (mut app, _dir) = project(endpoint)?;
    keys(&mut app, ":ai-explain project provider-error<ret>").await?;
    assert_tools(&next_request(&mut app, &mut rx).await?);
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.editor.ai_chat.as_ref().unwrap().pending.is_some() {
            settle(&mut app).await;
        }
    })
    .await?;
    assert!(rx.try_recv().is_err());
    let chat = app.editor.ai_chat.as_ref().unwrap();
    assert!(chat.turns.is_empty());
    assert_eq!(chat.draft, "provider-error");
    assert!(!chat.notice.is_empty());
    app.close().await;
    service.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_tool_response_does_not_retry_without_tools() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/v1/chat/completions", listener.local_addr()?);
    let (mut app, _dir) = project(endpoint)?;
    let connection = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).await? > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8192\r\nConnection: close\r\n\r\n{")
            .await?;
        socket.shutdown().await?;
        Ok::<_, std::io::Error>(listener)
    });
    keys(&mut app, ":ai-explain project interrupted-question<ret>").await?;
    let listener = tokio::time::timeout(Duration::from_secs(2), connection).await???;
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.editor.ai_chat.as_ref().unwrap().pending.is_some() {
            settle(&mut app).await;
        }
    })
    .await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(120), listener.accept())
            .await
            .is_err()
    );
    let chat = app.editor.ai_chat.as_ref().unwrap();
    assert!(chat.turns.is_empty());
    assert_eq!(chat.draft, "interrupted-question");
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canceling_from_the_trace_drops_late_tool_calls_and_keeps_the_question() -> Result<()> {
    let (endpoint, mut rx, service) = server(
        450,
        vec![
            call("first-read", "read_file", json!({"path":"README.md"})),
            call("late-read", "read_file", json!({"path":"maths.py"})),
            reply("late-answer"),
        ],
    )
    .await;
    let (mut app, _dir) = project(endpoint)?;
    keys(&mut app, ":ai-explain project cancel-this-question<ret>").await?;
    assert_tools(&next_request(&mut app, &mut rx).await?);
    let after_read = next_request(&mut app, &mut rx).await?;
    assert!(tool_result(&after_read, "first-read", "read_file").contains("Fixture Arithmetic"));
    keys(&mut app, "<A-t><C-c>").await?;
    tokio::time::sleep(Duration::from_millis(650)).await;
    settle(&mut app).await;
    let chat = app.editor.ai_chat.as_ref().unwrap();
    assert!(chat.pending.is_none() && chat.turns.is_empty());
    assert!(chat.agent_progress.is_empty());
    assert_eq!(chat.draft, "cancel-this-question");
    assert!(
        rx.try_recv().is_err(),
        "the late response started another tool round"
    );
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    service.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deepseek_thinking_resends_each_tool_round_with_its_reasoning() -> Result<()> {
    let mut first = call("ds-find", "find_files", json!({"query":"README"}));
    first["choices"][0]["message"]["reasoning_content"] = json!("reasoning for the first tool");
    let mut second = call(
        "ds-read",
        "read_file",
        json!({"path":"README.md","start_line":1,"end_line":1}),
    );
    second["choices"][0]["message"]["reasoning_content"] = json!("reasoning for the second tool");
    let (endpoint, mut rx, service) = server(0, vec![first, second, reply("ds-answer")]).await;
    let (mut app, _dir) = project_with_provider(endpoint, AiProviderType::Deepseek, 16384)?;
    keys(
        &mut app,
        ":ai-explain project locate the fixture README<ret>",
    )
    .await?;
    let initial = next_request(&mut app, &mut rx).await?;
    assert_eq!(initial["model"], "deepseek-v4-flash");
    assert_eq!(initial["thinking"]["type"], "enabled");
    assert_tools(&initial);
    let follow = next_request(&mut app, &mut rx).await?;
    assert!(tool_result(&follow, "ds-find", "find_files").contains("README.md"));
    let final_request = next_request(&mut app, &mut rx).await?;
    assert!(tool_result(&final_request, "ds-read", "read_file").contains("Fixture Arithmetic"));
    let reasons: Vec<_> = final_request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "assistant")
        .map(|m| m["reasoning_content"].as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            Some("reasoning for the first tool"),
            Some("reasoning for the second tool")
        ]
    );
    wait_for_screen(&mut app, "ds-answer").await?;
    assert!(!screen(&app).contains("reasoning for the"));
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    service.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn llama_cpp_preserves_its_model_and_chat_template_settings_through_a_tool_call() -> Result<()>
{
    let (endpoint, mut rx, service) = server(
        0,
        vec![
            call(
                "llama-read",
                "read_file",
                json!({"path":"README.md","start_line":1,"end_line":1}),
            ),
            reply("llama-answer"),
        ],
    )
    .await;
    let (mut app, _dir) = project_with_provider(endpoint, AiProviderType::LlamaCpp, 16384)?;
    assert!(app.editor.config().ai.providers["scope-mock"]
        .model_path
        .is_none());
    keys(&mut app, ":ai-explain project read the README<ret>").await?;
    let initial = next_request(&mut app, &mut rx).await?;
    let continuation = next_request(&mut app, &mut rx).await?;
    for body in [&initial, &continuation] {
        assert_tools(body);
        assert_eq!(body["model"], "scope-test");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["enable_thinking"], false);
        assert!(body.get("thinking").is_none());
    }
    let evidence = tool_result(&continuation, "llama-read", "read_file");
    assert!(
        evidence.contains("README.md:1") && evidence.contains("Fixture Arithmetic"),
        "{evidence}"
    );
    wait_for_screen(&mut app, "llama-answer").await?;
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    service.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn small_local_context_keeps_room_for_the_answer_without_tool_schemas() -> Result<()> {
    let (endpoint, mut rx, service) = server(0, vec![reply("small-context-answer")]).await;
    let context = 4096;
    let (mut app, dir) =
        project_with_provider(endpoint, AiProviderType::OpenaiCompatible, context)?;
    fs::write(
        dir.path().join("history.py"),
        "long_history_row = 1325\n".repeat(600),
    )?;
    keys(
        &mut app,
        ":ai-explain project what is long_history_row?<ret>",
    )
    .await?;
    let req = next_request(&mut app, &mut rx).await?;
    assert!(
        req.get("tools").is_none(),
        "the tool schemas leave insufficient room in this context"
    );
    let bytes: usize = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap().len() + 16)
        .sum();
    let output = req["max_tokens"].as_u64().unwrap() as usize;
    assert!(
        output > 0 && bytes + output + 128 <= context as usize,
        "input={bytes} output={output}"
    );
    wait_for_screen(&mut app, "small-context-answer").await?;
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    service.await?;
    Ok(())
}
