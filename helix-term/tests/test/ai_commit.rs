use super::helpers::*;
use helix_term::application::Application;
use helix_view::input::parse_macro;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn repository() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    for (key, value) in [
        ("user.name", "AI Commit Test"),
        ("user.email", "ai-commit@example.test"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("core.hooksPath", ".git/no-test-hooks"),
    ] {
        git(repo.path(), &["config", key, value]);
    }
    for name in ["a-choice.txt", "b-partial.txt", "z-unrelated.txt"] {
        fs::write(repo.path().join(name), "base\n").unwrap();
    }
    git(
        repo.path(),
        &[
            "add",
            "--",
            "a-choice.txt",
            "b-partial.txt",
            "z-unrelated.txt",
        ],
    );
    git(repo.path(), &["commit", "-m", "chore: initial fixture"]);
    repo
}

fn screen(app: &Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| {
            let mut text = String::new();
            let mut column = 0;
            while column < row.len() {
                let cell = &row[column];
                text.push_str(cell.symbol.as_str());
                // Wide graphemes cover the next cell; that cell is not a visible space.
                column += cell.width().max(1);
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    for key in parse_macro(input)? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(key.into());
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(key.into());
        app.handle_terminal_events(Ok(event)).await;
    }
    app.editor.reset_idle_timer();
    tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(app)).await?;
    Ok(())
}

async fn wait_for(app: &mut Application, text: &str) -> anyhow::Result<()> {
    wait_for_with_timeout(app, text, Duration::from_secs(8)).await
}

async fn wait_for_with_timeout(
    app: &mut Application,
    text: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while !screen(app).contains(text) && Instant::now() < deadline {
        keys(app, "").await?;
    }
    assert!(
        screen(app).contains(text),
        "Missing {text:?}:\n{}",
        screen(app)
    );
    Ok(())
}

async fn open(root: &Path, endpoint: Option<String>) -> anyhow::Result<Application> {
    let mut config = test_config();
    config.editor.ai_commit.enabled = endpoint.is_some();
    config.editor.ai_completion.enabled = false;
    config.editor.ai_completion.context_size = 8192;
    config.editor.idle_timeout = Duration::from_millis(30);
    if let Some(endpoint) = endpoint {
        config.editor.ai_completion.endpoint = endpoint;
    }
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(root.join("b-partial.txt"), None)
        .build()?;
    app.test_resize(140, 42).await?;
    keys(&mut app, ":ai-commit<ret>").await?;
    wait_for(&mut app, "diff (").await?;
    Ok(app)
}

async fn close(mut app: Application) -> anyhow::Result<()> {
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_switching_into_nested_repository_uses_its_own_workspace() -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("a-choice.txt"), "outer change\n")?;
    let nested = repo.path().join("nested");
    fs::create_dir(&nested)?;
    git(&nested, &["init", "-b", "nested-review"]);
    git(&nested, &["config", "user.name", "Nested Test"]);
    git(&nested, &["config", "user.email", "nested@example.test"]);
    fs::write(nested.join("inner.txt"), "nested change\n")?;
    let mut app = open(repo.path(), None).await?;
    keys(&mut app, "<C-w>").await?;
    app.editor.open(
        &nested.join("inner.txt"),
        helix_view::editor::Action::Replace,
    )?;
    keys(&mut app, ":ai-commit<ret>").await?;
    wait_for(&mut app, "nested-review").await?;
    assert_eq!(git(repo.path(), &["rev-list", "--count", "HEAD"]), "1");
    assert!(git(&nested, &["diff", "--cached"]).is_empty());
    close(app).await
}

// A whole-file add or commit -a would destroy the partial index or include unrelated edits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_disabled_manual_flow_preserves_partial_index_and_unrelated_edits(
) -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("b-partial.txt"), "staged part\n")?;
    git(repo.path(), &["add", "--", "b-partial.txt"]);
    fs::write(
        repo.path().join("b-partial.txt"),
        "staged part\nunstaged remainder\n",
    )?;
    fs::write(repo.path().join("a-choice.txt"), "chosen content\n")?;
    fs::write(repo.path().join("z-unrelated.txt"), "unrelated content\n")?;
    let mut app = open(repo.path(), None).await?;
    assert!(screen(&app).contains("整文件替换部分暂存"));
    keys(&mut app, "j<space>s").await?;
    wait_for(&mut app, "已暂存所选文件").await?;
    assert_eq!(
        git(repo.path(), &["diff", "--cached", "--name-only"]),
        "a-choice.txt\nb-partial.txt"
    );
    assert_eq!(git(repo.path(), &["show", ":b-partial.txt"]), "staged part");
    keys(
        &mut app,
        "efix(commit): 保留部分暂存<ret><ret>只提交明确选择的文件。<esc>l",
    )
    .await?;
    assert!(screen(&app).contains("English"));
    assert!(screen(&app).contains("fix(commit): 保留部分暂存"));
    assert!(screen(&app).contains("只提交明确选择的文件。"));
    keys(&mut app, "C").await?;
    wait_for(&mut app, "已提交").await?;
    assert_eq!(
        git(repo.path(), &["log", "-1", "--format=%B"]),
        "fix(commit): 保留部分暂存\n\n只提交明确选择的文件。"
    );
    assert_eq!(
        git(repo.path(), &["show", "HEAD:b-partial.txt"]),
        "staged part"
    );
    assert_eq!(
        git(repo.path(), &["show", "HEAD:a-choice.txt"]),
        "chosen content"
    );
    assert_eq!(git(repo.path(), &["show", "HEAD:z-unrelated.txt"]), "base");
    assert_eq!(
        git(repo.path(), &["diff", "--name-only"]),
        "b-partial.txt\nz-unrelated.txt"
    );
    assert!(git(repo.path(), &["diff", "--cached"]).is_empty());
    close(app).await
}

// Opening, Enter, selecting and cancellation must never implicitly stage or commit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_open_select_enter_and_cancel_are_read_only() -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("a-choice.txt"), "candidate\n")?;
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let before_index = fs::read(repo.path().join(".git/index"))?;
    let mut app = open(repo.path(), None).await?;
    keys(&mut app, "<space><ret><C-c><F11><F11>q").await?;
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    assert_eq!(fs::read(repo.path().join(".git/index"))?, before_index);
    assert_eq!(
        fs::read_to_string(repo.path().join("a-choice.txt"))?,
        "candidate\n"
    );
    assert!(!screen(&app).contains("AI 提交 |"));
    keys(&mut app, ":q!<ret>").await?;
    app.close().await;
    Ok(())
}

// A changed index must reject the old commit preview, retain the draft, and require review.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_stale_index_refuses_commit_and_retains_draft_for_explicit_review(
) -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("b-partial.txt"), "included\n")?;
    git(repo.path(), &["add", "--", "b-partial.txt"]);
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let mut app = open(repo.path(), None).await?;
    keys(
        &mut app,
        "efix: preserve reviewed draft<ret><ret>Manual body.<esc>",
    )
    .await?;
    fs::write(
        repo.path().join("z-unrelated.txt"),
        "external staged change\n",
    )?;
    git(repo.path(), &["add", "--", "z-unrelated.txt"]);
    keys(&mut app, "C").await?;
    wait_for(&mut app, "实际状态已重新读取").await?;
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    keys(&mut app, "r").await?;
    wait_for(&mut app, "已有暂存将全部提交").await?;
    assert!(screen(&app).contains("fix: preserve reviewed draft"));
    assert!(screen(&app).contains("Manual body."));
    keys(&mut app, "C").await?;
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    assert!(screen(&app).contains("保留手动文本"));
    keys(&mut app, "vC").await?;
    wait_for(&mut app, "已提交").await?;
    assert_eq!(
        git(repo.path(), &["log", "-1", "--format=%B"]),
        "fix: preserve reviewed draft\n\nManual body."
    );
    assert_eq!(
        git(
            repo.path(),
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        ),
        "b-partial.txt\nz-unrelated.txt"
    );
    close(app).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_unstage_selected_staged_row_preserves_disk_and_other_index_entries(
) -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("a-choice.txt"), "first staged\n")?;
    fs::write(repo.path().join("b-partial.txt"), "second staged\n")?;
    git(repo.path(), &["add", "--", "a-choice.txt", "b-partial.txt"]);
    fs::write(
        repo.path().join("a-choice.txt"),
        "first staged\nremaining disk edit\n",
    )?;
    let mut app = open(repo.path(), None).await?;
    keys(&mut app, "u").await?;
    wait_for(&mut app, "已取消该文件暂存").await?;
    assert_eq!(
        git(repo.path(), &["diff", "--cached", "--name-only"]),
        "b-partial.txt"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("a-choice.txt"))?,
        "first staged\nremaining disk edit\n"
    );
    assert_eq!(git(repo.path(), &["rev-list", "--count", "HEAD"]), "1");
    close(app).await
}

// Bracketed paste belongs to the focused commit workspace, never the underlying source buffer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_pasted_multiline_message_does_not_modify_underlying_source() -> anyhow::Result<()>
{
    let repo = repository();
    fs::write(repo.path().join("b-partial.txt"), "included source\n")?;
    git(repo.path(), &["add", "--", "b-partial.txt"]);
    let mut app = open(repo.path(), None).await?;
    let original_source = helix_view::current_ref!(app.editor).1.text().to_string();
    keys(&mut app, "e").await?;
    #[cfg(windows)]
    let event = crossterm::event::Event::Paste("fix(ai): 中文粘贴\r\n\r\n保留完整正文。".into());
    #[cfg(not(windows))]
    let event = termina::event::Event::Paste("fix(ai): 中文粘贴\r\n\r\n保留完整正文。".into());
    app.handle_terminal_events(Ok(event)).await;
    keys(&mut app, "").await?;
    assert_eq!(
        helix_view::current_ref!(app.editor).1.text().to_string(),
        original_source,
        "message paste must not reach source buffer"
    );
    assert!(
        screen(&app).contains("fix(ai): 中文粘贴"),
        "{}",
        screen(&app)
    );
    assert!(screen(&app).contains("保留完整正文。"), "{}", screen(&app));
    keys(&mut app, "<esc>").await?;
    #[cfg(windows)]
    let event = crossterm::event::Event::Paste("unwanted source edit".into());
    #[cfg(not(windows))]
    let event = termina::event::Event::Paste("unwanted source edit".into());
    app.handle_terminal_events(Ok(event)).await;
    keys(&mut app, "").await?;
    assert_eq!(
        helix_view::current_ref!(app.editor).1.text().to_string(),
        original_source
    );
    assert!(!helix_view::current_ref!(app.editor).1.is_modified());
    keys(&mut app, "C").await?;
    wait_for(&mut app, "已提交").await?;
    assert_eq!(
        git(repo.path(), &["log", "-1", "--format=%B"]),
        "fix(ai): 中文粘贴\n\n保留完整正文。"
    );
    close(app).await
}

enum Reply {
    Summary,
    Recommend,
    Chinese,
    English,
}

// Only the external HTTP service is doubled; commands, jobs, UI and Git remain real.
async fn server(
    replies: Vec<Reply>,
    delay_ms: u64,
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
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
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
                        .find_map(|line| line.strip_prefix("content-length:"))
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
            let request: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
            let data: Value =
                serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
            let files = data["files"].as_array().unwrap();
            let content = match reply {
                Reply::Summary => {
                    json!({"summaries":files.iter().map(|file| json!({"id":file["id"],"summary":"调整文件内容"})).collect::<Vec<_>>()})
                }
                Reply::Recommend => {
                    json!({"selected":files.iter().filter(|file| file["staged"] == false && file["partial"] == false && file["manual_review"] == false).map(|file| file["id"].clone()).collect::<Vec<_>>(),"reasons":files.iter().map(|file| json!({"id":file["id"],"reason":"检查本次提交范围"})).collect::<Vec<_>>()})
                }
                Reply::Chinese => {
                    json!({"type":"fix","scope":"fixture","subject":"保留暂存版本","body":"仅描述已暂存的改动。"})
                }
                Reply::English => {
                    json!({"type":"fix","scope":"fixture","subject":"preserve staged version","body":"Describe staged changes only."})
                }
            };
            let _ = tx.send(request);
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let response = json!({"choices":[{"finish_reason":"stop","message":{"content":content.to_string()}}]}).to_string();
            let _ = socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await;
        }
    });
    (endpoint, rx, task)
}

async fn request(
    requests: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> anyhow::Result<Value> {
    tokio::time::timeout(Duration::from_secs(3), requests.recv())
        .await?
        .ok_or_else(|| anyhow::anyhow!("model server closed before request"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_recommendation_requires_explicit_stage_before_generating_message(
) -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("a-choice.txt"), "chosen content\n")?;
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let before_index = fs::read(repo.path().join(".git/index"))?;
    let (endpoint, mut requests, task) = server(
        vec![
            Reply::Summary,
            Reply::Recommend,
            Reply::Summary,
            Reply::Chinese,
        ],
        0,
    )
    .await;
    let mut app = open(repo.path(), Some(endpoint)).await?;
    wait_for(&mut app, "推荐完成").await?;
    request(&mut requests).await?;
    request(&mut requests).await?;
    assert!(screen(&app).contains("[x] a-choice.txt"));
    assert_eq!(fs::read(repo.path().join(".git/index"))?, before_index);
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    keys(&mut app, "s").await?;
    wait_for(&mut app, "fix(fixture): 保留暂存版本").await?;
    request(&mut requests).await?;
    request(&mut requests).await?;
    assert_eq!(
        git(repo.path(), &["diff", "--cached", "--name-only"]),
        "a-choice.txt"
    );
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    keys(&mut app, "C").await?;
    wait_for(&mut app, "已提交").await?;
    assert_eq!(
        git(repo.path(), &["log", "-1", "--format=%s"]),
        "fix(fixture): 保留暂存版本"
    );
    assert_eq!(
        git(
            repo.path(),
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        ),
        "a-choice.txt"
    );
    close(app).await?;
    task.abort();
    Ok(())
}

// Changing staged scope must preserve manual text and wait for explicit regeneration or review.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_staging_preserves_manual_draft_until_explicit_review() -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("a-choice.txt"), "chosen content\n")?;
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let (endpoint, mut requests, task) = server(
        vec![
            Reply::Summary,
            Reply::Recommend,
            Reply::Summary,
            Reply::Chinese,
        ],
        0,
    )
    .await;
    let mut app = open(repo.path(), Some(endpoint)).await?;
    wait_for(&mut app, "推荐完成").await?;
    request(&mut requests).await?;
    request(&mut requests).await?;
    keys(
        &mut app,
        "efix: preserve manual intent<ret><ret>Keep this manually written body.<esc>s",
    )
    .await?;
    wait_for(&mut app, "已有暂存 1").await?;
    assert!(
        screen(&app).contains("fix: preserve manual intent"),
        "{}",
        screen(&app)
    );
    assert!(
        screen(&app).contains("Keep this manually written body."),
        "{}",
        screen(&app)
    );
    assert!(
        !app.editor.ai_commit_running,
        "staging must not regenerate an existing manual draft"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(300), requests.recv())
            .await
            .is_err(),
        "staging sent an unsolicited request that could replace the manual draft"
    );
    assert!(screen(&app).contains("需生成或手动检查"));
    keys(&mut app, "C").await?;
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    keys(&mut app, "vC").await?;
    wait_for(&mut app, "已提交").await?;
    assert_eq!(
        git(repo.path(), &["log", "-1", "--format=%B"]),
        "fix: preserve manual intent\n\nKeep this manually written body."
    );
    close(app).await?;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_native_generation_uses_index_and_language_switch_retains_chinese_draft(
) -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("b-partial.txt"), "STAGED_VERSION\n")?;
    git(repo.path(), &["add", "--", "b-partial.txt"]);
    fs::write(
        repo.path().join("b-partial.txt"),
        "STAGED_VERSION\nUNSTAGED_ONLY\n",
    )?;
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let (endpoint, mut requests, task) = server(
        vec![
            Reply::Summary,
            Reply::Recommend,
            Reply::Summary,
            Reply::Chinese,
            Reply::Summary,
            Reply::English,
        ],
        0,
    )
    .await;
    let mut app = open(repo.path(), Some(endpoint)).await?;
    wait_for(&mut app, "推荐完成").await?;
    request(&mut requests).await?;
    request(&mut requests).await?;
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    assert_eq!(
        git(repo.path(), &["show", ":b-partial.txt"]),
        "STAGED_VERSION"
    );
    keys(&mut app, "g").await?;
    wait_for(&mut app, "fix(fixture): 保留暂存版本").await?;
    let chinese_summary = request(&mut requests).await?;
    assert!(chinese_summary["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("STAGED_VERSION"));
    assert!(!chinese_summary["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("UNSTAGED_ONLY"));
    request(&mut requests).await?;
    keys(&mut app, "l").await?;
    assert!(screen(&app).contains("English"));
    assert!(screen(&app).contains("fix(fixture): 保留暂存版本"));
    assert!(
        requests.try_recv().is_err(),
        "switching language must require explicit generation"
    );
    keys(&mut app, "g").await?;
    wait_for(&mut app, "fix(fixture): preserve staged version").await?;
    let english_summary = request(&mut requests).await?;
    assert!(english_summary["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("in English"));
    assert!(!english_summary["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("UNSTAGED_ONLY"));
    request(&mut requests).await?;
    keys(&mut app, "C").await?;
    wait_for(&mut app, "已提交").await?;
    assert_eq!(
        git(repo.path(), &["log", "-1", "--format=%B"]),
        "fix(fixture): preserve staged version\n\nDescribe staged changes only."
    );
    assert_eq!(
        git(repo.path(), &["show", "HEAD:b-partial.txt"]),
        "STAGED_VERSION"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("b-partial.txt"))?,
        "STAGED_VERSION\nUNSTAGED_ONLY\n"
    );
    close(app).await?;
    task.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_commit_cancel_discards_delayed_model_result_without_git_writes() -> anyhow::Result<()> {
    let repo = repository();
    fs::write(repo.path().join("a-choice.txt"), "candidate\n")?;
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let before_index = fs::read(repo.path().join(".git/index"))?;
    let (endpoint, mut requests, task) = server(vec![Reply::Summary, Reply::Recommend], 300).await;
    let mut app = open(repo.path(), Some(endpoint)).await?;
    request(&mut requests).await?;
    keys(&mut app, "<C-c>efix: keep manual draft<esc>l").await?;
    tokio::time::sleep(Duration::from_millis(350)).await;
    keys(&mut app, "").await?;
    assert!(screen(&app).contains("fix: keep manual draft"));
    assert!(screen(&app).contains("English"));
    assert!(!app.editor.ai_commit_running);
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    assert_eq!(fs::read(repo.path().join(".git/index"))?, before_index);
    close(app).await?;
    task.abort();
    Ok(())
}

/// Requires the explicitly configured local model service; excluded from ordinary regression runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires local Qwen3.5-9B-Q6_K at 127.0.0.1:19240"]
async fn ai_commit_real_local_model_recommends_stages_and_commits_chinese_message(
) -> anyhow::Result<()> {
    let repo = repository();
    let source = "def add(a, b):\n    return a + b\n";
    fs::write(repo.path().join("tiny.py"), source)?;
    let before_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let before_index = fs::read(repo.path().join(".git/index"))?;
    let mut config = test_config();
    config.editor.ai_completion.enabled = false;
    config.editor.ai_completion.endpoint = "http://127.0.0.1:19240/v1/chat/completions".into();
    config.editor.ai_completion.model = "Qwen3.5-9B-Q6_K".into();
    config.editor.ai_completion.context_size = 4096;
    config.editor.ai_commit.enabled = true;
    config.editor.ai_commit.timeout_ms = 60_000;
    config.editor.idle_timeout = Duration::from_millis(30);
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(repo.path().join("b-partial.txt"), None)
        .build()?;
    app.test_resize(140, 42).await?;
    keys(&mut app, ":ai-commit<ret>").await?;
    wait_for_with_timeout(&mut app, "推荐完成", Duration::from_secs(60)).await?;
    assert!(screen(&app).contains("[x] tiny.py"), "{}", screen(&app));
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    assert_eq!(fs::read(repo.path().join(".git/index"))?, before_index);
    keys(&mut app, "s").await?;
    wait_for_with_timeout(&mut app, "信息已生成", Duration::from_secs(60)).await?;
    assert_eq!(
        git(repo.path(), &["diff", "--cached", "--name-only"]),
        "tiny.py"
    );
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), before_head);
    keys(&mut app, "C").await?;
    wait_for(&mut app, "已提交").await?;
    let message = git(repo.path(), &["log", "-1", "--format=%B"]);
    assert!(message.lines().next().unwrap().contains(": "), "{message}");
    assert!(
        message
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
        "expected Chinese message: {message}"
    );
    assert_eq!(git(repo.path(), &["show", "HEAD:tiny.py"]), source.trim());
    assert!(git(repo.path(), &["status", "--porcelain"]).is_empty());
    close(app).await
}
