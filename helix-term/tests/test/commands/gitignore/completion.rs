use super::*;

async fn wait_for_menu(app: &mut Application) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.editor.last_completion.is_none() {
            app.editor.reset_idle_timer();
            run_event_loop_until_idle(app).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("completion menu did not open:\n{}", screen(app)))?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_path_replaces_only_current_segment_and_undoes() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let nested = root.path().join("frontend");
    fs::create_dir_all(nested.join("构建/cache"))?;
    let target = nested.join(".gitignore");
    let original = "/构旧/cache/\n";
    fs::write(&target, original)?;
    let mut app = AppBuilder::new()
        .with_file(&target, Some(helix_core::Position::new(0, 2)))
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><C-c>").await?;
    assert_eq!(doc!(app.editor).text().to_string(), original);
    keys(&mut app, "<C-x>").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "/构建/cache/\n");
    assert_eq!(fs::read_to_string(&target)?, original);
    keys(&mut app, "<esc>u").await?;
    assert_eq!(doc!(app.editor).text().to_string(), original);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forward_delete_dismisses_transactions_for_the_old_suffix() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("build"))?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join(".gitignore"), None)
        .with_input_text("/bu#[|/]#cache/\n")
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<del>").await?;
    assert!(app.editor.last_completion.is_none());
    assert_eq!(doc!(app.editor).text().to_string(), "/bucache/\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeat_mid_segment_completion_replaces_only_the_new_short_segment() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("build"))?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join(".gitignore"), None)
        .with_input_text("/bu#[|z]#zzz/cache/\n/b/keep/\n# sentinel\n/b/")
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret><esc>jghl.").await?;
    assert_eq!(
        doc!(app.editor).text().to_string(),
        "/build/cache/\n/build/keep/\n# sentinel\n/b/"
    );
    keys(&mut app, "2jghl.").await?;
    assert_eq!(
        doc!(app.editor).text().to_string(),
        "/build/cache/\n/build/keep/\n# sentinel\n/build/"
    );
    Ok(())
}

#[test]
fn lsp_can_open_first_and_git_candidates_still_follow_typing() -> anyhow::Result<()> {
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    #[cfg(unix)]
    runtime.max_blocking_threads(1);
    runtime
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(lsp_opens_before_git())
}

async fn lsp_opens_before_git() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let executable = compile_fixture("diagnostics_lsp", root.path())?;
    let log = root.path().join("requests.jsonl");
    let overrides = format!(
        r#"
[language-server.completion-fixture]
command = {}
args = [{}, "completion"]
[[language]]
name = "git-ignore"
language-servers = ["completion-fixture"]
"#,
        toml::Value::String(executable.to_string_lossy().into_owned()),
        toml::Value::String(log.to_string_lossy().into_owned())
    );
    let file = root.path().join(".gitignore");
    fs::write(&file, "/generated/pr\n")?;
    let mut config = test_config();
    config.editor.auto_completion = false;
    config.editor.lsp.enable = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_lang_loader(test_syntax_loader(Some(overrides)))
        .with_file(&file, None)
        .build()?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("textDocument/didOpen")
        {
            app.editor.reset_idle_timer();
            run_event_loop_until_idle(&mut app).await;
        }
    })
    .await?;
    // Hold the Unix blocking worker so the first disk request is definitely pending.
    // Windows uses blocking workers for stdio pipes too. There, the absent generated
    // directory ensures only the LSP contributes items to the initial popup.
    #[cfg(unix)]
    let release = {
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let (started, waiting) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let _ = started.send(());
            let _ = gate.recv();
        });
        waiting.await?;
        release
    };
    keys(&mut app, "A<C-x>").await?;
    wait_for_menu(&mut app).await?;
    assert!(screen(&app).contains("prefix-lsp"));
    assert!(!screen(&app).contains("prefix-local"));
    fs::create_dir(root.path().join("generated"))?;
    fs::write(root.path().join("generated/prefix-local"), "")?;
    keys(&mut app, "ef").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "/generated/pref\n");
    #[cfg(unix)]
    drop(release);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !screen(&app).contains("prefix-local") {
            app.editor.reset_idle_timer();
            run_event_loop_until_idle(&mut app).await;
        }
    })
    .await?;
    assert!(screen(&app).contains("prefix-lsp"));
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_language_paths_keep_templates_in_unsaved_draft() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    fs::create_dir(root.path().join("target-local-only"))?;
    let target = root.path().join(".gitignore");
    let mut config = test_config();
    config.editor.auto_completion = false;
    let loader = test_syntax_loader(Some(
        "[[language]]\nname = 'git-ignore'\npath-completion = false\n".into(),
    ));
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_lang_loader(loader)
        .with_file(&target, None)
        .build()?;
    keys(&mut app, "i/tar").await?;
    assert!(app.editor.last_completion.is_none());
    assert!(!doc!(app.editor).path_completion_enabled());
    keys(&mut app, "<C-x>").await?;
    wait_for_menu(&mut app).await?;
    assert!(!screen(&app).contains("target-local-only"));
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string().trim_end(), "/target/");
    assert!(!target.exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_line_manually_offers_local_project_template() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join(".gitignore"), None)
        .build()?;
    keys(&mut app, "i").await?;
    assert!(app.editor.last_completion.is_none());
    keys(&mut app, "<C-x>").await?;
    wait_for_menu(&mut app).await?;
    assert!(screen(&app).contains("/target/"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_trigger_refreshes_an_open_rule_menu() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("cache-first"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join(".gitignore"), None)
        .with_input_text("/ca#[|\n]#")
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    wait_for_menu(&mut app).await?;
    assert!(screen(&app).contains("cache-first"));
    fs::write(root.path().join("cache-second"), "")?;
    keys(&mut app, "<C-x>").await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !screen(&app).contains("cache-second") {
            app.editor.reset_idle_timer();
            run_event_loop_until_idle(&mut app).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_after_limited_empty_menu_can_reach_other_candidates_and_undo() -> anyhow::Result<()>
{
    complete_hidden_candidate("output").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prefix_match_outside_limit_outranks_visible_fuzzy_matches() -> anyhow::Result<()> {
    // Every original candidate remains a fuzzy match all the way through "zztr".
    complete_hidden_candidate("a-z-z-t-r-").await
}

async fn complete_hidden_candidate(initial_prefix: &str) -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    for n in 0..64 {
        fs::write(root.path().join(format!("{initial_prefix}{n:02}")), "")?;
    }
    fs::write(root.path().join("zztrace-output"), "")?;
    let target = root.path().join(".gitignore");
    let mut app = AppBuilder::new()
        .with_file(&target, None)
        .with_input_text("#[|\n]#")
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    wait_for_menu(&mut app).await?;
    assert!(!screen(&app).contains("zztrace-output"));
    keys(&mut app, "zztr").await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !screen(&app).contains("zztrace-output") {
            app.editor.reset_idle_timer();
            run_event_loop_until_idle(&mut app).await;
        }
    })
    .await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "zztrace-output\n");
    keys(&mut app, "<esc>u").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "zztr\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extension_punctuation_triggers_completion_automatically() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    #[cfg(feature = "git")]
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root.path())
        .status()?
        .success());
    fs::write(root.path().join("first.hxtrace"), "")?;
    fs::write(root.path().join("second.hxtrace"), "")?;
    let mut config = test_config();
    config.editor.completion_trigger_len = 12;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(root.path().join(".gitignore"), None)
        .build()?;
    keys(&mut app, "i*.").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string().trim_end(), "*.hxtrace");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn comments_keep_automatic_path_completion_quiet() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("folder"))?;
    fs::write(root.path().join("folder/local-output"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join(".gitignore"), None)
        .build()?;
    keys(&mut app, "i# ./folder/").await?;
    assert!(app.editor.last_completion.is_none(), "{}", screen(&app));
    keys(&mut app, "<C-x>").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(
        doc!(app.editor).text().to_string().trim_end(),
        "# ./folder/local-output"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codeowners_shares_grammar_but_keeps_ordinary_path_completion() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("folder"))?;
    fs::write(root.path().join("folder/owner"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("CODEOWNERS"), None)
        .build()?;
    keys(&mut app, "i# ./folder/").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(
        doc!(app.editor).text().to_string().trim_end(),
        "# ./folder/owner"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn matching_multicursor_contexts_apply_one_completion_everywhere() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("shared"))?;
    let target = root.path().join(".gitignore");
    let mut app = AppBuilder::new()
        .with_file(&target, None)
        .with_input_text("/sh#[|\n]#/sh#(|\n)#")
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "/shared/\n/shared/\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mismatched_multicursor_contexts_skip_completion() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("shared"))?;
    let target = root.path().join(".gitignore");
    let mut app = AppBuilder::new()
        .with_file(&target, None)
        .with_input_text("/sh#[|\n]#!/sh#(|\n)#")
        .build()?;
    keys(&mut app, "i<C-x>").await?;
    assert!(app.editor.last_completion.is_none(), "{}", screen(&app));
    assert_eq!(doc!(app.editor).text().to_string(), "/sh\n!/sh\n");
    Ok(())
}

#[cfg(feature = "git")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exception_completion_uses_unsaved_rules_and_retains_typed_prefix() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root.path())
        .status()?
        .success());
    fs::write(root.path().join(".env.example"), "")?;
    fs::write(root.path().join(".env.aaa"), "")?;
    let target = root.path().join(".gitignore");
    fs::write(&target, "# saved\n")?;
    let mut app = AppBuilder::new()
        .with_file(&target, None)
        .with_input_text(".env*\n!.env.aaa\n#[|\n]#")
        .build()?;
    keys(&mut app, "i!").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, ".env").await?;
    wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret>").await?;
    assert_eq!(
        doc!(app.editor).text().to_string(),
        ".env*\n!.env.aaa\n!.env.example\n"
    );
    assert_eq!(fs::read_to_string(&target)?, "# saved\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switching_documents_discards_pending_menu() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("project-output"))?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join(".gitignore"), None)
        .build()?;
    // Send all keys before pumping the asynchronous completion callbacks.
    keys(&mut app, "i/proj<C-x><esc>:new<ret>i").await?;
    assert!(doc!(app.editor).path().is_none());
    assert!(app.editor.last_completion.is_none(), "{}", screen(&app));
    Ok(())
}
