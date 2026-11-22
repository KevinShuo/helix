use super::{
    agent::{call, next_request, tool_result},
    *,
};
use crate::test::{function_hints::fixture_config, helpers::test_syntax_loader};
use std::fs;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_uses_language_server_definitions_and_references_at_source_positions() -> Result<()>
{
    let project = tempfile::tempdir()?;
    let fixture = tempfile::tempdir()?;
    let (overrides, log) = fixture_config(fixture.path())?;
    fs::write(
        project.path().join("pyproject.toml"),
        "[project]\nname = 'symbols'\n",
    )?;
    let source = project.path().join("entry.py");
    fs::write(&source, "def hello():\n    return 1\n\nhello()\nhello()\n")?;
    let target = project.path().join("library.py");
    fs::write(
        &target,
        "# library\ndef actual_definition():\n    return 1\n",
    )?;
    let at = |line| json!({"line":line,"character":0});
    fs::write(
        log.with_extension("definition"),
        json!([{
            "targetUri":helix_stdx::Url::from_file_path(target).unwrap(),
            "targetRange":{"start":at(0),"end":at(2)},
            "targetSelectionRange":{"start":at(1),"end":at(1)}
        }])
        .to_string(),
    )?;

    let (endpoint, mut requests, service) = server(
        0,
        vec![
            call(
                "def",
                "find_definition",
                json!({"path":"entry.py","line":4,"column":2,"symbol":"hello"}),
            ),
            call(
                "refs",
                "find_references",
                json!({"path":"entry.py","line":1,"column":5,"symbol":"hello"}),
            ),
            reply("Read `library.py:2`."),
        ],
    )
    .await;
    let mut config = test_config();
    config.editor.lsp.enable = true;
    config.editor.idle_timeout = Duration::from_millis(20);
    config.editor.ai_completion.enabled = false;
    config.editor.ai_explain.enabled = true;
    config.editor.ai_explain.provider = Some("fixture".into());
    config.editor.ai.providers.insert(
        "fixture".into(),
        AiProviderConfig {
            kind: AiProviderType::OpenaiCompatible,
            endpoint,
            model: "fixture".into(),
            context_size: 16384,
            proxy: AiProxy::None,
            ..Default::default()
        },
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_lang_loader(test_syntax_loader(Some(overrides)))
        .with_file(&source, None)
        .build()?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while current_ref!(app.editor)
            .1
            .language_servers()
            .next()
            .is_none()
        {
            settle(&mut app).await;
        }
    })
    .await
    .context("fixture language server did not initialize")?;
    keys(&mut app, ":ai-explain project trace hello<ret>").await?;
    next_request(&mut app, &mut requests).await?;
    let definition = next_request(&mut app, &mut requests).await?;
    let definition = tool_result(&definition, "def", "find_definition");
    assert!(
        definition.contains("语言服务器")
            && definition.contains("library.py:2: def actual_definition"),
        "{definition}"
    );
    let refs = next_request(&mut app, &mut requests).await?;
    let refs = tool_result(&refs, "refs", "find_references");
    assert!(
        refs.contains("语言服务器")
            && refs.contains("entry.py:4: hello()")
            && refs.contains("entry.py:5: hello()"),
        "{refs}"
    );
    assert_eq!(refs.matches("entry.py:4:").count(), 1);
    wait_for_turns(&mut app, 1).await?;
    app.close().await;
    service.await?;
    Ok(())
}
