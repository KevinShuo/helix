use super::*;
use std::fs;
use tempfile::TempDir;

const SOURCE: &str =
    "def adjust(amount):\n    result = amount + 0\n    return result * 1\n\ndef sibling_marker():\n    return 17\n";

pub(super) fn project(endpoint: String) -> Result<(Application, TempDir)> {
    project_with_provider(endpoint, AiProviderType::OpenaiCompatible, 16384)
}

pub(super) fn project_with_provider(
    endpoint: String,
    kind: AiProviderType,
    context_size: u32,
) -> Result<(Application, TempDir)> {
    let dir = tempfile::tempdir()?;
    fs::create_dir(dir.path().join(".git"))?;
    fs::write(dir.path().join("maths.py"), SOURCE)?;
    fs::write(dir.path().join("README.md"), "# Fixture Arithmetic\n")?;
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"fixture-arithmetic\"\n",
    )?;
    fs::write(dir.path().join(".env"), "PRIVATE_FIXTURE_MARKER=1\n")?;
    fs::write(dir.path().join(".gitignore"), "ignored_fixture.py\n")?;
    fs::write(
        dir.path().join("ignored_fixture.py"),
        "IGNORED_FIXTURE_MARKER = 99\n",
    )?;
    let mut config = test_config();
    config.editor.idle_timeout = Duration::from_millis(20);
    config.editor.ai_completion.enabled = false;
    config.editor.ai_explain.enabled = true;
    config.editor.ai_explain.provider = Some("scope-mock".into());
    config.editor.ai.providers.insert(
        "scope-mock".into(),
        AiProviderConfig {
            kind,
            endpoint,
            model: if kind == AiProviderType::Deepseek {
                "deepseek-v4-flash"
            } else {
                "scope-test"
            }
            .into(),
            thinking: kind == AiProviderType::Deepseek,
            context_size,
            proxy: AiProxy::None,
            ..Default::default()
        },
    );
    let app = AppBuilder::new()
        .with_config(config)
        .with_file(dir.path().join("maths.py"), None)
        .build()?;
    Ok((app, dir))
}

fn snapshot(body: &Value) -> Result<Value> {
    assert_eq!(body["model"], "scope-test");
    let content = body["messages"][1]["content"]
        .as_str()
        .context("first user message is missing")?;
    let data = serde_json::Deserializer::from_str(content)
        .into_iter::<Value>()
        .next()
        .context("no snapshot precedes the question")??;
    Ok(data)
}

fn assert_scope(body: &Value, scope: &str) -> Result<()> {
    let data = snapshot(body)?;
    assert_eq!(data["scope"], scope, "{data}");
    let source = data["selected_code"].as_str().context("missing source")?;
    match scope {
        "selection" => assert_eq!(source.trim_end(), "    result = amount + 0"),
        "function" => {
            assert!(
                source.contains("def adjust(amount):") && source.contains("return result * 1"),
                "{source}"
            );
            assert!(!source.contains("sibling_marker"), "{source}");
        }
        "file" => {
            for line in SOURCE.lines().filter(|line| !line.is_empty()) {
                assert!(source.contains(line.trim()), "missing {line:?} in {source}");
            }
        }
        "project" => {
            let contents = body["messages"].to_string();
            assert!(
                contents.contains("maths.py") && contents.contains("README.md"),
                "{contents}"
            );
            assert!(
                !contents.contains("PRIVATE_FIXTURE_MARKER")
                    && !contents.contains("IGNORED_FIXTURE_MARKER"),
                "{contents}"
            );
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explain_all_four_scopes_retain_their_source_and_history_for_followups() -> Result<()> {
    let scopes = ["selection", "function", "file", "project"];
    let replies = scopes
        .iter()
        .flat_map(|_| [reply("scope-answer"), reply("followup-answer")])
        .collect();
    let (endpoint, mut requests, task) = server(0, replies).await;
    let (mut app, dir) = project(endpoint)?;
    for scope in scopes {
        let select = if scope == "selection" { "x" } else { "" };
        keys(&mut app, &format!("ggj{select}:ai-explain {scope}<ret>")).await?;
        let initial = request(&mut requests).await?;
        assert_scope(&initial, scope)?;
        wait_for_screen(&mut app, "scope-answer").await?;
        keys(&mut app, "why again?<ret>").await?;
        let followup = request(&mut requests).await?;
        assert_scope(&followup, scope)?;
        if scope != "project" {
            assert_eq!(
                snapshot(&followup)?["selected_code"],
                snapshot(&initial)?["selected_code"]
            );
        }
        let messages = followup["messages"].as_array().unwrap();
        assert!(messages.iter().any(|m| m["role"] == "assistant"
            && m["content"]
                .as_str()
                .is_some_and(|s| s.starts_with("scope-answer"))));
        assert!(messages.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("why again?"));
        wait_for_screen(&mut app, "followup-answer").await?;
        keys(&mut app, "<esc>").await?;
        assert_eq!(text(&app), SOURCE);
        assert!(!current_ref!(app.editor).1.is_modified());
    }
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

fn suggestions() -> Value {
    reply(
        &json!({"suggestions": [
            {"line": 2, "title": "DropZero", "reason": "Zero leaves an integer unchanged.",
             "before": "    result = amount + 0", "after": "    result = amount"},
            {"line": 3, "title": "DropOne", "reason": "Multiplication by one preserves the result.",
             "before": "    return result * 1", "after": "    return result"},
            {"line": 2, "title": "DiscussTypes", "reason": "Consider the supported operand types."},
            {"line": 2, "title": "FourthSuggestion", "reason": "Exceeds the three-suggestion limit."}
        ]})
        .to_string(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optimize_all_three_scopes_support_followups_without_editing() -> Result<()> {
    let scopes = ["selection", "function", "file"];
    let replies = scopes
        .iter()
        .flat_map(|_| [suggestions(), reply("scope-followup")])
        .collect();
    let (endpoint, mut requests, task) = server(0, replies).await;
    let (mut app, dir) = project(endpoint)?;
    let version = current_ref!(app.editor).1.version();
    keys(&mut app, ":ai-optimize project<ret>").await?;
    assert!(requests.try_recv().is_err());
    assert!(app.editor.ai_chat.is_none());
    for scope in scopes {
        let select = if scope == "selection" { "x" } else { "" };
        keys(&mut app, &format!("ggj{select}:ai-optimize {scope}<ret>")).await?;
        let initial = request(&mut requests).await?;
        assert_scope(&initial, scope)?;
        wait_for_screen(&mut app, "DropZero").await?;
        keys(&mut app, "what types?<ret>").await?;
        let followup = request(&mut requests).await?;
        assert_scope(&followup, scope)?;
        let history = followup["messages"].as_array().unwrap();
        assert!(history.iter().any(|m| m["role"] == "assistant"
            && m["content"]
                .as_str()
                .is_some_and(|s| s.contains("DropZero"))));
        assert!(history.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("what types?"));
        wait_for_screen(&mut app, "scope-followup").await?;
        keys(&mut app, "<esc>").await?;
        assert_eq!(text(&app), SOURCE);
        assert_eq!(current_ref!(app.editor).1.version(), version);
    }
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optimize_preview_cancels_or_accepts_only_one_suggestion_in_one_undo() -> Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![suggestions()]).await;
    let (mut app, dir) = project(endpoint)?;
    let version = current_ref!(app.editor).1.version();
    keys(&mut app, ":ai-optimize file<ret>").await?;
    request(&mut requests).await?;
    wait_for_screen(&mut app, "DropZero").await?;
    assert_eq!(app.editor.ai_chat.as_ref().unwrap().suggestions.len(), 3);
    keys(&mut app, "<esc>:ai-optimize-preview 3<ret>").await?;
    assert!(app
        .editor
        .get_status()
        .is_some_and(|(message, _)| message.contains("供讨论")));
    assert_eq!(text(&app), SOURCE);
    keys(&mut app, ":ai-optimize-preview 2<ret>").await?;
    let preview = screen(&app);
    assert!(
        preview.contains("DropOne") && preview.contains("return result * 1"),
        "{preview}"
    );
    assert_eq!(text(&app), SOURCE);
    assert_eq!(current_ref!(app.editor).1.version(), version);
    keys(&mut app, "<esc>").await?;
    wait_for_screen(&mut app, "DropZero").await?;
    assert_eq!(text(&app), SOURCE);
    keys(&mut app, "<esc>:ai-optimize-preview 2<ret><ret>").await?;
    assert_eq!(
        text(&app),
        SOURCE.replace("return result * 1", "return result")
    );
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    keys(&mut app, "<esc>u").await?;
    assert_eq!(text(&app), SOURCE);
    assert!(!current_ref!(app.editor).1.is_modified());
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optimize_rejects_another_buffer_and_an_edited_snapshot() -> Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![suggestions()]).await;
    let (mut app, dir) = project(endpoint)?;
    keys(&mut app, ":ai-optimize function<ret>").await?;
    request(&mut requests).await?;
    wait_for_screen(&mut app, "DropZero").await?;
    keys(&mut app, "<esc>:new<ret>").await?;
    let blank = text(&app);
    keys(&mut app, ":ai-optimize-preview 2<ret><ret>").await?;
    assert_eq!(text(&app), blank);
    assert!(!current_ref!(app.editor).1.is_modified());
    keys(&mut app, ":buffer-previous<ret>").await?;
    assert_eq!(text(&app), SOURCE);
    keys(&mut app, "ggiinvalidation<esc>").await?;
    let changed = text(&app);
    assert_ne!(changed, SOURCE);
    keys(&mut app, ":ai-optimize-preview 2<ret><ret>").await?;
    assert_eq!(text(&app), changed);
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optimize_rechecks_version_at_confirmation_after_a_formatter_edit() -> Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![suggestions()]).await;
    let (mut app, dir) = project(endpoint)?;
    keys(&mut app, ":ai-optimize file<ret>").await?;
    request(&mut requests).await?;
    wait_for_screen(&mut app, "DropZero").await?;
    keys(&mut app, "<esc>:ai-optimize-preview 2<ret>").await?;
    assert!(screen(&app).contains("DropOne"));
    {
        let (view, doc) = helix_view::current!(app.editor);
        let tx = helix_core::Transaction::change(
            doc.text(),
            [(0, 0, Some("# formatter\n".into()))].into_iter(),
        );
        assert!(doc.apply(&tx, view.id));
        doc.append_changes_to_history(view);
    }
    keys(&mut app, "<ret>").await?;
    assert_eq!(text(&app), format!("# formatter\n{SOURCE}"));
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_panel_cycles_explain_and_optimize_scopes_and_opens_one_preview() -> Result<()> {
    let (endpoint, mut requests, task) =
        server(0, vec![reply("function-answer"), suggestions()]).await;
    let (mut app, _dir) = project(endpoint)?;
    app.test_resize(84, 28).await?;
    keys(&mut app, "jx:ai<ret>").await?;
    let dashboard = screen(&app);
    for label in ["[选区]", "工程根", "scope-test"] {
        assert!(dashboard.contains(label), "{dashboard}");
    }
    keys(&mut app, "<tab><ret>").await?;
    assert_scope(&request(&mut requests).await?, "function")?;
    wait_for_screen(&mut app, "function-answer").await?;
    keys(&mut app, "<esc>ggjx:ai<ret>j<tab><tab><ret>").await?;
    assert_scope(&request(&mut requests).await?, "file")?;
    wait_for_screen(&mut app, "DropZero").await?;
    keys(&mut app, "<A-p><down><ret>").await?;
    let preview = screen(&app);
    assert!(
        preview.contains("DropOne") && preview.contains("return result * 1"),
        "{preview}"
    );
    assert_eq!(text(&app), SOURCE);
    keys(&mut app, "<esc>").await?;
    assert_eq!(text(&app), SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_explanation_opens_a_cited_line_from_the_retrieved_source() -> Result<()> {
    let answer =
        "See `dispatch.py:3` for ROUTE_ENTRY, and `missing.py:99` for an unsupported reference.";
    let (endpoint, mut requests, task) = server(0, vec![reply(answer)]).await;
    let (mut app, dir) = project(endpoint)?;
    fs::write(
        dir.path().join("dispatch.py"),
        "# dispatch\ndef route():\n    return ROUTE_ENTRY\n",
    )?;
    keys(
        &mut app,
        "ggO# UNSAVED_BUFFER_MARKER<esc>:ai-explain project where is ROUTE_ENTRY?<ret>",
    )
    .await?;
    let body = request(&mut requests).await?;
    assert!(snapshot(&body)?["selected_code"]
        .as_str()
        .unwrap()
        .contains("UNSAVED_BUFFER_MARKER"));
    let evidence = body["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(
        evidence.contains("dispatch.py") && evidence.contains("return ROUTE_ENTRY"),
        "{evidence}"
    );
    wait_for_turns(&mut app, 1).await?;
    keys(&mut app, "<A-g>").await?;
    let choices = screen(&app);
    assert!(
        choices.contains("dispatch.py:3") && !choices.contains("missing.py:99"),
        "{choices}"
    );
    keys(&mut app, "<ret>").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(doc.path().is_some_and(|p| p.ends_with("dispatch.py")));
    let rope = doc.text().slice(..);
    assert_eq!(
        rope.char_to_line(doc.selection(view.id).primary().cursor(rope)),
        2
    );
    assert!(!doc.is_modified());
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explain_without_arguments_distinguishes_a_cursor_from_one_selected_character() -> Result<()>
{
    let (endpoint, mut requests, task) =
        server(0, vec![reply("auto-function"), reply("auto-selection")]).await;
    let (mut app, _dir) = project(endpoint)?;
    keys(&mut app, ":new<ret>:ai-explain<ret>").await?;
    assert!(app.editor.ai_chat.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(120), requests.recv())
            .await
            .is_err()
    );
    keys(&mut app, ":buffer-previous<ret>ggj:ai-explain<ret>").await?;
    assert_scope(&request(&mut requests).await?, "function")?;
    wait_for_screen(&mut app, "auto-function").await?;
    keys(&mut app, "<esc>ggv:ai-explain<ret>").await?;
    let data = snapshot(&request(&mut requests).await?)?;
    assert_eq!(data["scope"], "selection", "{data}");
    assert_eq!(data["selected_code"], "d");
    wait_for_screen(&mut app, "auto-selection").await?;
    assert_eq!(text(&app), SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explain_file_reads_unsaved_buffer_and_freezes_it_for_followup() -> Result<()> {
    let (endpoint, mut requests, task) =
        server(0, vec![reply("unsaved-answer"), reply("original-snapshot")]).await;
    let (mut app, dir) = project(endpoint)?;
    keys(
        &mut app,
        "ggO# FIRST_UNSAVED_CHANGE<esc>:ai-explain file<ret>",
    )
    .await?;
    let first = snapshot(&request(&mut requests).await?)?;
    let before = text(&app);
    assert_eq!(first["scope"], "file");
    assert!(before.contains("FIRST_UNSAVED_CHANGE"));
    assert_eq!(first["selected_code"], before);
    wait_for_screen(&mut app, "unsaved-answer").await?;
    keys(
        &mut app,
        "<esc>ggO# SECOND_UNSAVED_CHANGE<esc>:ai-explain-show<ret>what changed?<ret>",
    )
    .await?;
    let followup = snapshot(&request(&mut requests).await?)?;
    assert_eq!(followup["selected_code"], before);
    assert!(text(&app).contains("SECOND_UNSAVED_CHANGE"));
    wait_for_screen(&mut app, "original-snapshot").await?;
    assert_eq!(fs::read_to_string(dir.path().join("maths.py"))?, SOURCE);
    app.close().await;
    task.await?;
    Ok(())
}
