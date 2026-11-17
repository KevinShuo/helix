use super::{
    ai_doc::{keys, server, settle},
    helpers::*,
};
use helix_term::application::Application;
use helix_view::current_ref;
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};

const SOURCE: &str =
    "def add(a, b):\n    \"\"\"Return the sum of two numbers.\"\"\"\n    return a + b\n";
const CODE: &str = "from maths import add\n\ndef test_add():\n    assert add(2, 3) == 5\n    assert add(-2, 2) == 0\n";

fn reply(code: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"content":json!({"code":code,"assumptions":["整数相加遵循接口说明。"]}).to_string()}}]})
}

fn app(endpoint: String) -> anyhow::Result<(Application, tempfile::TempDir)> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.pytest.ini_options]\ntestpaths = ['tests']\n",
    )?;
    std::fs::write(dir.path().join("maths.py"), SOURCE)?;
    let mut config = test_config();
    config.editor.ai_test.enabled = true;
    config.editor.ai_test.max_tokens = 2048;
    config.editor.ai_test.python = std::env::var_os("HELIX_AI_TEST_PYTHON").map(PathBuf::from);
    config.editor.ai_completion.enabled = false;
    config.editor.ai_completion.endpoint = endpoint;
    config.editor.ai_completion.context_size = 16384;
    config.editor.idle_timeout = Duration::from_millis(50);
    let app = AppBuilder::new()
        .with_config(config)
        .with_file(dir.path().join("maths.py"), None)
        .build()?;
    Ok((app, dir))
}

async fn generate(
    app: &mut Application,
    requests: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> anyhow::Result<Value> {
    keys(app, ":ai-test<ret>").await?;
    let request = tokio::time::timeout(Duration::from_secs(3), requests.recv())
        .await
        .map_err(|e| anyhow::anyhow!("{e}; status={:?}", app.editor.get_status()))?
        .expect("generation request");
    settle(app).await;
    Ok(request)
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_test_large_preview_reaches_tail_and_home_without_editing_source() -> anyhow::Result<()>
{
    use helix_view::ai::{AiProviderConfig, AiProviderType, AiProxy};
    let code = format!("{CODE}{}# TAIL_MARKER\n", "\n".repeat(70_000));
    let (endpoint, mut requests, task) = server(0, vec![reply(&code)]).await;
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.pytest.ini_options]\n",
    )?;
    let source = dir.path().join("maths.py");
    std::fs::write(&source, SOURCE)?;
    let mut config = test_config();
    config.editor.ai_completion.enabled = false;
    config.editor.ai_test.enabled = true;
    config.editor.ai_test.provider = Some("large-test".into());
    config.editor.ai.providers.insert(
        "large-test".into(),
        AiProviderConfig {
            kind: AiProviderType::Deepseek,
            model: "deepseek-flash".into(),
            endpoint,
            proxy: AiProxy::None,
            ..Default::default()
        },
    );
    config.editor.idle_timeout = Duration::from_millis(50);
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&source, None)
        .build()?;
    app.test_resize(48, 22).await?;
    generate(&mut app, &mut requests).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while app
        .editor
        .ai_test
        .as_ref()
        .is_some_and(|op| op.changes.is_none())
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
        settle(&mut app).await;
    }
    assert!(
        app.editor
            .ai_test
            .as_ref()
            .is_some_and(|op| op.changes.is_some()),
        "large result did not reach preview: {:?}",
        app.editor.get_status()
    );
    let screen = |app: &Application| {
        app.test_screen()
            .content
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect::<String>()
    };
    keys(&mut app, "<end>").await?;
    assert!(screen(&app).contains("TAIL_MARKER"));
    app.test_resize(20, 22).await?;
    keys(&mut app, "<C-end>").await?;
    assert!(screen(&app).contains("TAIL_MARKER"));
    keys(&mut app, "<home>").await?;
    assert!(screen(&app).contains("add"));
    assert!(!screen(&app).contains("TAIL_MARKER"));
    assert_eq!(current_ref!(app.editor).1.text().to_string(), SOURCE);
    assert_eq!(std::fs::read_to_string(&source)?, SOURCE);
    keys(&mut app, "<esc>").await?;
    app.close().await;
    task.await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_test_previews_and_applies_to_undoable_buffers_without_touching_source(
) -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(CODE)]).await;
    let (mut app, _dir) = app(endpoint)?;
    let request = generate(&mut app, &mut requests).await?;
    assert!(request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("Return the sum"));
    assert_eq!(request["max_tokens"], 2048);
    let path = app
        .editor
        .ai_test
        .as_ref()
        .unwrap()
        .project
        .test_file
        .clone();
    assert!(!path.exists());
    assert_eq!(current_ref!(app.editor).1.text().to_string(), SOURCE);
    keys(&mut app, "<ret>").await?;
    assert!(app.editor.ai_test.as_ref().unwrap().applied);
    assert!(!path.exists(), "acceptance must not write without save");
    let generated = app.editor.document_by_path(&path).unwrap();
    assert!(generated.is_modified());
    assert!(generated
        .text()
        .to_string()
        .contains("assert add(2, 3) == 5"));
    assert_eq!(current_ref!(app.editor).1.text().to_string(), SOURCE);
    keys(&mut app, ":ai-test-run<ret>").await?;
    assert!(
        app.editor.get_status().unwrap().0.contains("文件已变化")
            || app.editor.get_status().unwrap().0.contains("保存")
    );
    app.editor
        .open(&path, helix_view::editor::Action::Replace)?;
    keys(&mut app, "u").await?;
    assert!(!current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .contains("assert add"));
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_test_cancel_and_stale_dependency_never_apply_results() -> anyhow::Result<()> {
    let (endpoint, mut requests, task) = server(0, vec![reply(CODE), reply(CODE)]).await;
    let (mut app, dir) = app(endpoint)?;
    generate(&mut app, &mut requests).await?;
    std::fs::write(dir.path().join("pyproject.toml"), "# externally changed\n")?;
    keys(&mut app, "<ret>").await?;
    let path = app
        .editor
        .ai_test
        .as_ref()
        .unwrap()
        .project
        .test_file
        .clone();
    assert!(app.editor.document_by_path(&path).is_none());
    keys(&mut app, ":ai-test-cancel<ret>").await?;
    assert!(app.editor.ai_test.is_none());
    generate(&mut app, &mut requests).await?;
    keys(&mut app, "<esc>").await?;
    assert!(app.editor.ai_test.is_none());
    assert_eq!(current_ref!(app.editor).1.text().to_string(), SOURCE);
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_test_cmake_and_test_changes_are_accepted_together() -> anyhow::Result<()> {
    let code = "#include <gtest/gtest.h>\nextern \"C\" int add(int a, int b);\nTEST(__HELIX_TEST_SUITE__, Adds) { EXPECT_EQ(add(2,3), 5); }\n";
    let (endpoint, mut requests, task) = server(0, vec![reply(code)]).await;
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.20)\nproject(demo C)\nadd_library(maths maths.c)\n",
    )?;
    std::fs::write(
        dir.path().join("maths.c"),
        "int add(int a, int b) { return a + b; }\n",
    )?;
    let mut config = test_config();
    config.editor.ai_test.enabled = true;
    config.editor.ai_completion.enabled = false;
    config.editor.ai_completion.endpoint = endpoint;
    config.editor.ai_completion.context_size = 16384;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(dir.path().join("maths.c"), None)
        .build()?;
    keys(&mut app, ":ai-test<ret>").await?;
    tokio::time::timeout(Duration::from_secs(3), requests.recv())
        .await
        .map_err(|e| anyhow::anyhow!("{e}; status={:?}", app.editor.get_status()))?
        .unwrap();
    settle(&mut app).await;
    let op = app.editor.ai_test.as_ref().unwrap();
    assert!(op
        .changes
        .as_ref()
        .is_some_and(|changes| changes.len() == 4));
    let paths = op
        .changes
        .as_ref()
        .unwrap()
        .iter()
        .map(|c| c.before.path.clone())
        .collect::<Vec<_>>();
    keys(&mut app, "<ret>").await?;
    for path in paths {
        assert!(app.editor.document_by_path(path).unwrap().is_modified());
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CMakeLists.txt"))?
            .matches("add_library")
            .count(),
        1
    );
    assert!(!dir.path().join("tests").exists());
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_AI_TEST_PYTHON with pytest installed"]
async fn ai_test_real_pytest_pass_runs_saved_changes() -> anyhow::Result<()> {
    assert!(std::env::var_os("HELIX_AI_TEST_PYTHON").is_some());
    let (endpoint, mut requests, task) = server(0, vec![reply(CODE)]).await;
    let (mut app, _dir) = app(endpoint)?;
    generate(&mut app, &mut requests).await?;
    keys(&mut app, "<ret>:wa!<ret>").await?;
    settle(&mut app).await;
    keys(&mut app, ":ai-test-run<ret>").await?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    settle(&mut app).await;
    let op = app.editor.ai_test.as_ref().unwrap();
    assert!(
        op.report.as_ref().is_some_and(|r| r.success),
        "{:?}",
        op.report
    );
    assert_eq!(op.repairs, 0);
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_AI_TEST_PYTHON with pytest installed"]
async fn ai_test_import_repair_is_previewed_once_and_preserves_assertions() -> anyhow::Result<()> {
    assert!(std::env::var_os("HELIX_AI_TEST_PYTHON").is_some());
    let (endpoint, mut requests, task) = server(
        0,
        vec![
            reply(&CODE.replace("from maths", "from missing_module")),
            reply(CODE),
        ],
    )
    .await;
    let (mut app, _dir) = app(endpoint)?;
    generate(&mut app, &mut requests).await?;
    keys(&mut app, "<ret>:wa!<ret>").await?;
    settle(&mut app).await;
    keys(&mut app, ":ai-test-run<ret>").await?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        settle(&mut app).await;
        if app
            .editor
            .ai_test
            .as_ref()
            .is_some_and(|op| op.repairs == 1 && op.changes.is_some())
        {
            break;
        }
    }
    let op = app.editor.ai_test.as_ref().expect("repair operation");
    assert_eq!(op.repairs, 1);
    assert!(!op.applied, "repair must return to preview");
    let path = op.project.test_file.clone();
    assert!(
        std::fs::read_to_string(&path)?.contains("missing_module"),
        "unaccepted repair cannot change disk"
    );
    assert!(requests.try_recv().is_ok(), "exactly one repair request");
    keys(&mut app, "<ret>:wa!<ret>").await?;
    settle(&mut app).await;
    keys(&mut app, ":ai-test-run<ret>").await?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        settle(&mut app).await;
        if app
            .editor
            .ai_test
            .as_ref()
            .is_some_and(|op| !op.running && op.report.as_ref().is_some_and(|r| r.success))
        {
            break;
        }
    }
    let op = app.editor.ai_test.as_ref().unwrap();
    assert!(
        op.report.as_ref().is_some_and(|r| r.success),
        "{:?}",
        op.report
    );
    assert_eq!(op.repairs, 1);
    app.close().await;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_AI_TEST_PYTHON with pytest installed"]
async fn ai_test_failed_repair_keeps_original_test_diagnostics() -> anyhow::Result<()> {
    assert!(std::env::var_os("HELIX_AI_TEST_PYTHON").is_some());
    let (endpoint, mut requests, task) = server(
        0,
        vec![
            reply(&CODE.replace("from maths", "from missing_module")),
            reply("def test_add():\n    pass\n"),
        ],
    )
    .await;
    let (mut app, _dir) = app(endpoint)?;
    generate(&mut app, &mut requests).await?;
    keys(&mut app, "<ret>:wa!<ret>").await?;
    settle(&mut app).await;
    let path = app
        .editor
        .ai_test
        .as_ref()
        .unwrap()
        .project
        .test_file
        .clone();
    keys(&mut app, ":ai-test-run<ret>").await?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        settle(&mut app).await;
        if app
            .editor
            .ai_test
            .as_ref()
            .is_none_or(|op| op.repairs == 1 && op.applied)
        {
            break;
        }
    }
    let op = app
        .editor
        .ai_test
        .as_ref()
        .expect("failed repair must preserve the run report");
    assert!(op.applied && !op.running && op.repairs == 1);
    let report = op.report.as_ref().expect("original run report");
    assert!(
        !report.success && report.log.contains("ModuleNotFoundError"),
        "{report:?}"
    );
    assert!(report.log.contains("自动修复失败"), "{report:?}");
    assert!(std::fs::read_to_string(path)?.contains("missing_module"));
    assert!(requests.try_recv().is_ok());
    app.close().await;
    task.abort();
    Ok(())
}
