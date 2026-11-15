use super::{
    ai_doc::{keys, settle},
    helpers::*,
};
use helix_term::application::Application;
use helix_view::current_ref;
use std::time::Duration;

fn screen(app: &Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn test_gutter_blank_and_folded_rows_never_toggle_breakpoints() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test_cases.py");
    std::fs::write(
        &path,
        "def test_one():\n    assert True\n\ndef test_two():\n    assert True\n",
    )?;
    let mut config = test_config();
    config.editor.gutters = vec!["test".parse()?, "fold".parse()?, "line-numbers".parse()?].into();
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&path, None)
        .build()?;
    app.test_resize(100, 30).await?;
    assert!(
        screen(&app).lines().next().unwrap().starts_with('▶'),
        "{}",
        screen(&app)
    );
    click(&mut app, 2, 0).await?;
    assert!(app.editor.breakpoints.is_empty());
    assert!(app.editor.tests.request.is_none());
    keys(&mut app, "za").await?;
    assert_eq!(
        current_ref!(app.editor)
            .1
            .folds(current_ref!(app.editor).0.id)
            .len(),
        1
    );
    click(&mut app, 1, 0).await?; // blank row after the collapsed test
    assert!(app.editor.breakpoints.is_empty());
    assert!(app.editor.tests.request.is_none());
    keys(&mut app, "za").await?;
    // Unsaved buffers block execution but the click is still consumed as a test action.
    keys(&mut app, "i#<esc>").await?;
    click(&mut app, 3, 0).await?;
    assert!(app
        .editor
        .get_status()
        .is_some_and(|(s, _)| s.contains("保存")));
    assert!(app.editor.tests.request.is_none());
    keys(&mut app, ":q!<ret>").await?;
    app.close().await;
    Ok(())
}

async fn click(app: &mut Application, row: u16, column: u16) -> anyhow::Result<()> {
    #[cfg(windows)]
    let event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    #[cfg(not(windows))]
    let event = termina::event::Event::Mouse(termina::event::MouseEvent {
        kind: termina::event::MouseEventKind::Down(termina::event::MouseButton::Left),
        column,
        row,
        modifiers: termina::event::KeyModifiers::NONE,
    });
    app.handle_terminal_events(Ok(event)).await;
    settle(app).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_panel_docks_releases_focus_reopens_and_navigates_failure() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test_cases.py");
    std::fs::write(
        &path,
        "def test_one():\n    assert False\n\ndef test_two():\n    assert True\n",
    )?;
    let mut app = AppBuilder::new().with_file(&path, None).build()?;
    app.test_resize(100, 30).await?;
    let canonical = helix_stdx::path::canonicalize(path.clone());
    app.editor.tests.cases.push(serde_json::from_value(serde_json::json!({
        "id":"test_cases.py::test_one", "name":"test_one", "path":canonical,
        "line":0, "outcome":"failed", "details":"assert False", "failure_path":canonical, "failure_line":1
    }))?);
    keys(&mut app, ":test-show<ret>").await?;
    assert!(screen(&app).contains("Tests"), "{}", screen(&app));
    assert!(current_ref!(app.editor).0.area.height < 29);
    keys(&mut app, "<ret>").await?; // group expands/collapses
    keys(&mut app, "<right>j<ret>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().char_to_line(
            current_ref!(app.editor)
                .1
                .selection(current_ref!(app.editor).0.id)
                .primary()
                .cursor(current_ref!(app.editor).1.text().slice(..))
        ),
        1
    );
    keys(&mut app, "i#<esc>").await?;
    assert!(current_ref!(app.editor).1.is_modified());
    assert!(app.editor.tests.stale || app.editor.tests.request.is_none());
    keys(&mut app, ":test-show<ret>q").await?;
    assert_eq!(current_ref!(app.editor).0.area.height, 29);
    keys(&mut app, ":test-show<ret>").await?;
    assert!(screen(&app).contains("test_one"));
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn declaration_cache_tracks_reopened_documents_and_language_changes() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test_cases.py");
    let canonical = helix_stdx::path::canonicalize(&path);
    std::fs::write(&path, "def test_old(): pass\n")?;
    let mut app = AppBuilder::new().with_file(&path, None).build()?;
    app.test_resize(100, 30).await?;
    assert_eq!(
        app.editor.tests.declarations[&canonical].declarations[0].name,
        "test_old"
    );
    keys(&mut app, ":buffer-close<ret>").await?;
    std::fs::write(&path, "def test_new(): pass\n")?;
    keys(&mut app, &format!(":open {}<ret>", path.display())).await?;
    assert_eq!(
        app.editor.tests.declarations[&canonical].declarations[0].name,
        "test_new"
    );
    keys(&mut app, ":set-language text<ret>").await?;
    assert!(app.editor.tests.declarations[&canonical]
        .declarations
        .is_empty());
    keys(&mut app, ":set-language python<ret>").await?;
    assert_eq!(
        app.editor.tests.declarations[&canonical].declarations.len(),
        1
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_AI_TEST_PYTHON with pytest"]
async fn real_pytest_panel_failed_only_rerun_preserves_context() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test_cases.py");
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.pytest.ini_options]\n",
    )?;
    std::fs::write(
        &path,
        "def test_pass():\n    assert True\ndef test_bad():\n    assert False\ndef test_unexecuted():\n    assert True\n",
    )?;
    std::fs::write(
        dir.path().join("pytest.ini"),
        "[pytest]\naddopts = --maxfail=1\n",
    )?;
    let mut config = test_config();
    config.editor.test.python = Some(
        std::env::var_os("HELIX_AI_TEST_PYTHON")
            .expect("python path")
            .into(),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&path, None)
        .build()?;
    keys(&mut app, ":test-file<ret>").await?;
    for _ in 0..100 {
        settle(&mut app).await;
        if !app.editor.tests.running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!app.editor.tests.running);
    assert_eq!(
        app.editor.tests.cases.len(),
        3,
        "{:?}",
        app.editor.tests.phase
    );
    assert_eq!(app.editor.tests.failed_ids(), ["test_cases.py::test_bad"]);
    let source = dir.path().join("other.cpp");
    std::fs::write(&source, "int x;\n")?;
    keys(&mut app, "q").await?;
    keys(&mut app, &format!(":open {}<ret>", source.display())).await?;
    keys(&mut app, ":test-rerun-failed<ret>").await?;
    for _ in 0..100 {
        settle(&mut app).await;
        if !app.editor.tests.running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(app.editor.tests.cases.len(), 1);
    assert_eq!(app.editor.tests.cases[0].id, "test_cases.py::test_bad");
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

async fn wait_run(app: &mut Application) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        settle(app).await;
        if !app.editor.tests.running {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{}",
            app.editor.tests.phase
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires HELIX_AI_TEST_PYTHON with pytest"]
async fn real_pytest_cancel_then_new_run_ignores_late_updates() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test_cases.py");
    std::fs::write(
        &path,
        "import time\ndef test_slow():\n    time.sleep(30)\ndef test_fast():\n    assert True\n",
    )?;
    let mut config = test_config();
    config.editor.test.python = Some(
        std::env::var_os("HELIX_AI_TEST_PYTHON")
            .expect("python path")
            .into(),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&path, None)
        .build()?;
    keys(&mut app, "2G:test<ret>").await?;
    for _ in 0..100 {
        settle(&mut app).await;
        if app
            .editor
            .tests
            .cases
            .iter()
            .any(|c| c.outcome == helix_view::test::Outcome::Running)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(app.editor.tests.running);
    keys(&mut app, "s").await?;
    assert!(!app.editor.tests.running);
    assert!(app
        .editor
        .tests
        .cases
        .iter()
        .all(|c| c.outcome == helix_view::test::Outcome::Cancelled));
    keys(&mut app, "q4G:test<ret>").await?;
    wait_run(&mut app).await;
    assert_eq!(
        app.editor.tests.cases.len(),
        1,
        "{}",
        app.editor.tests.phase
    );
    assert_eq!(app.editor.tests.cases[0].id, "test_cases.py::test_fast");
    assert_eq!(
        app.editor.tests.cases[0].outcome,
        helix_view::test::Outcome::Passed
    );
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires CMake, compiler and installed GoogleTest/vcpkg"]
async fn real_gtest_single_file_parameterized_skip_failed_rerun_and_build_error(
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test_cases.cpp");
    std::fs::write(dir.path().join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.20)\nproject(workbench C CXX)\nenable_testing()\nfind_package(GTest CONFIG REQUIRED)\nadd_library(maths maths.c)\nadd_executable(cases test_cases.cpp)\ntarget_link_libraries(cases maths GTest::gtest_main)\nadd_executable(other other.cpp)\ntarget_link_libraries(other GTest::gtest_main)\nadd_test(NAME setup COMMAND ${CMAKE_COMMAND} -E true)\nset_tests_properties(setup PROPERTIES FIXTURES_SETUP Ready)\ninclude(GoogleTest)\ngtest_discover_tests(cases TEST_PREFIX prefix_ PROPERTIES FIXTURES_REQUIRED Ready)\ngtest_discover_tests(other TEST_PREFIX other_)\n")?;
    std::fs::write(
        dir.path().join("other.cpp"),
        "#include <gtest/gtest.h>\nTEST(Suite, Pass) { FAIL(); }\n",
    )?;
    std::fs::write(
        dir.path().join("maths.c"),
        "int add(int a, int b) { return a+b; }\n",
    )?;
    std::fs::write(&path, "#include <gtest/gtest.h>\nextern \"C\" int add(int,int);\nTEST(Suite, Pass) { EXPECT_EQ(add(1,2),3); }\nTEST(Suite, Bad) { EXPECT_EQ(add(1,2),4); }\nTEST(Suite, Skip) { GTEST_SKIP(); }\nclass Param : public testing::TestWithParam<int> {};\nTEST_P(Param, Works) { EXPECT_GT(GetParam(),0); }\nINSTANTIATE_TEST_SUITE_P(Values, Param, testing::Values(1,2));\n")?;
    let mut cache = serde_json::json!({});
    if let Some(root) = std::env::var_os("VCPKG_ROOT") {
        cache["CMAKE_TOOLCHAIN_FILE"] = std::path::PathBuf::from(root)
            .join("scripts/buildsystems/vcpkg.cmake")
            .to_string_lossy()
            .into_owned()
            .into();
    }
    std::fs::write(
        dir.path().join("CMakePresets.json"),
        serde_json::to_vec(
            &serde_json::json!({"version":3,"configurePresets":[{"name":"local","binaryDir":"out/build","cacheVariables":cache}]}),
        )?,
    )?;
    let mut config = test_config();
    config.editor.test.cmake_preset = Some("local".into());
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&path, None)
        .build()?;
    // A declaration click must not run the neighbouring failing case.
    keys(&mut app, "3G:test<ret>").await?;
    wait_run(&mut app).await;
    assert_eq!(
        app.editor.tests.cases.len(),
        1,
        "{}\n{}",
        app.editor.tests.phase,
        app.editor.tests.log
    );
    assert_eq!(app.editor.tests.cases[0].id, "prefix_Suite.Pass");
    assert!(dir
        .path()
        .join("out/build/.cmake/api/v1/query/client-helix-tests/codemodel-v2")
        .is_file());
    assert_eq!(
        app.editor.tests.cases[0].outcome,
        helix_view::test::Outcome::Passed
    );
    keys(&mut app, "<esc>:test-file<ret>").await?;
    wait_run(&mut app).await;
    assert_eq!(
        app.editor.tests.cases.len(),
        5,
        "{}\n{}",
        app.editor.tests.phase,
        app.editor.tests.log
    );
    assert_eq!(app.editor.tests.failed_ids(), ["prefix_Suite.Bad"]);
    assert_eq!(
        app.editor
            .tests
            .cases
            .iter()
            .filter(|c| c.outcome == helix_view::test::Outcome::Skipped)
            .count(),
        1
    );
    keys(&mut app, "f").await?;
    wait_run(&mut app).await;
    assert_eq!(app.editor.tests.cases.len(), 1);
    assert_eq!(app.editor.tests.cases[0].id, "prefix_Suite.Bad");
    std::fs::write(dir.path().join("maths.c"), "this is not valid C;\n")?;
    keys(&mut app, "<esc>:test-project<ret>").await?;
    wait_run(&mut app).await;
    assert!(
        app.editor.tests.phase.contains("Build"),
        "{}",
        app.editor.tests.phase
    );
    assert!(app.editor.tests.cases.is_empty());
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;
    Ok(())
}
