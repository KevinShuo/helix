use super::*;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

fn stub() -> &'static Path {
    static BUILD: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &BUILD
        .get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("server.rs");
            std::fs::write(
                &source,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/ai-model-server.rs"
                )),
            )
            .unwrap();
            let exe = dir.path().join(if cfg!(windows) {
                "server.exe"
            } else {
                "server"
            });
            let mut command = std::process::Command::new("rustc");
            command
                .arg("--edition=2021")
                .arg(&source)
                .arg("-o")
                .arg(&exe);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            (dir, exe)
        })
        .1
}

fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn local_runtime_arguments_reach_child_and_reload_restarts_only_when_required() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (mut config, _) = models(dir.path(), 0);
    config.runtime = toml::from_str(
        "cache-type-k = 'q8_0'\ncache-type-v = 'q8_0'\nspeculative-decoding = 'mtp'\ndraft-tokens = 2\nthreads = 8\nthreads-batch = 12\nbatch-size = 2048\nubatch-size = 256\n",
    ).unwrap();
    config.runtime.backend_sampling = true;
    config.runtime.load_mode = helix_view::ai::AiModelLoadMode::Dio;
    let path = config.model_path.as_ref().unwrap().clone();
    let mut server = LocalServer::new(&config);
    ready(&config).await;
    let args = std::fs::read_to_string(path.with_extension("args")).unwrap();
    let pairs: Vec<_> = args.lines().collect();
    // Explicitly stop before asserting, including on the pre-implementation run.
    server.stop().await;
    assert!(
        pairs.contains(&"--backend-sampling"),
        "GPU sampling flag must reach the process: {args}"
    );
    for (key, expected) in [
        ("--cache-type-k", "q8_0"),
        ("--cache-type-v", "q8_0"),
        ("--spec-type", "draft-mtp"),
        ("--spec-draft-n-max", "2"),
        ("--threads", "8"),
        ("--threads-batch", "12"),
        ("--batch-size", "2048"),
        ("--ubatch-size", "256"),
        ("--load-mode", "dio"),
    ] {
        assert!(
            pairs.windows(2).any(|pair| pair == [key, expected]),
            "{key} {expected}: {args}"
        );
    }

    let mut server = LocalServer::new(&config);
    ready(&config).await;
    let starts = || {
        std::fs::read_to_string(path.with_extension("started"))
            .unwrap()
            .lines()
            .count()
    };
    assert_eq!(starts(), 2);
    config.thinking = true;
    config.thinking_budget = Some(2048);
    server.configure(&config);
    ready(&config).await;
    assert_eq!(starts(), 2, "request tuning must keep the process");
    config.runtime.threads = Some(4);
    server.configure(&config);
    ready(&config).await;
    assert_eq!(starts(), 3, "runtime tuning must replace the process");
    server.stop().await;
}

#[tokio::test]
async fn local_runtime_defaults_keep_legacy_cache_and_do_not_require_speculative_support() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (config, _) = models(dir.path(), 0);
    let mut server = LocalServer::new(&config);
    ready(&config).await;
    let args = std::fs::read_to_string(config.model_path.as_ref().unwrap().with_extension("args"))
        .unwrap();
    server.stop().await;
    assert!(args.contains("--cache-type-k\nf16\n--cache-type-v\nf16"));
    assert!(
        !args.contains("--spec-type"),
        "legacy runtimes need no new flags"
    );
    assert!(!args.contains("--threads"));
    assert!(!args.contains("--backend-sampling"));
    assert!(!args.contains("--load-mode"));
}

pub(crate) fn models(dir: &Path, delay: u64) -> (AiProviderConfig, AiProviderConfig) {
    let baseline_port = port();
    let mut temporary_port = port();
    while baseline_port == temporary_port {
        temporary_port = port();
    }
    let make = |name: &str, port: u16, peer: u16, delay: u64| {
        let path = dir.join(format!("{name}.gguf"));
        std::fs::write(&path, format!("peer={peer}\ndelay={delay}\n")).unwrap();
        AiProviderConfig {
            kind: helix_view::ai::AiProviderType::LlamaCpp,
            model: name.into(),
            model_path: Some(path),
            server_path: stub().to_owned(),
            endpoint: format!("http://127.0.0.1:{port}/v1/chat/completions"),
            startup_timeout_ms: 10_000,
            ..Default::default()
        }
    };
    let baseline = make("baseline-9b", baseline_port, 0, 0);
    let mut temporary = make("temporary-27b", temporary_port, baseline_port, delay);
    temporary.on_demand = true;
    (baseline, temporary)
}

pub(crate) fn reachable(model: &AiProviderConfig) -> bool {
    let url = reqwest::Url::parse(&model.endpoint).unwrap();
    std::net::TcpStream::connect_timeout(
        &url.socket_addrs(|| None).unwrap()[0],
        Duration::from_millis(100),
    )
    .is_ok()
}

pub(crate) async fn ready(model: &AiProviderConfig) {
    timeout(Duration::from_secs(15), async {
        loop {
            if wait_ready(model).await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn local_commit_lease_loads_on_demand_and_restores_after_success_and_error() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    assert!(!reachable(&temporary), "27B must not load at startup");
    for fail in [false, true] {
        let result = with_model(&temporary, async {
            assert!(!reachable(&baseline), "9B must exit before 27B starts");
            assert!(reachable(&temporary));
            if fail {
                anyhow::bail!("provider failure");
            }
            Ok(42)
        })
        .await;
        assert_eq!(result.is_err(), fail);
        ready(&baseline).await;
        assert!(!reachable(&temporary), "temporary model must be released");
    }
    pool.stop().await;
    assert!(!reachable(&baseline));
}

#[tokio::test]
async fn local_commit_lease_cancellation_during_loading_and_inference_restores_baseline() {
    let _messages = status::setup();
    for delay in [0, 3000] {
        let dir = tempfile::tempdir().unwrap();
        let (baseline, temporary) = models(dir.path(), delay);
        let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
        ready(&baseline).await;
        let selected = temporary.clone();
        let task = tokio::spawn(async move {
            with_model(&selected, std::future::pending::<anyhow::Result<()>>()).await
        });
        timeout(Duration::from_secs(10), async {
            loop {
                if temporary
                    .model_path
                    .as_ref()
                    .unwrap()
                    .with_extension("started")
                    .exists()
                {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        if delay == 0 {
            ready(&temporary).await;
        }
        task.abort();
        let _ = task.await;
        ready(&baseline).await;
        assert!(!reachable(&temporary));
        pool.stop().await;
    }
}

#[tokio::test]
async fn local_commit_lease_reload_and_shutdown_do_not_restore_an_obsolete_model() {
    let _messages = status::setup();
    for shutdown in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (baseline, temporary) = models(dir.path(), 0);
        let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
        ready(&baseline).await;
        let selected = temporary.clone();
        let task = tokio::spawn(async move {
            with_model(&selected, std::future::pending::<anyhow::Result<()>>()).await
        });
        ready(&temporary).await;
        if shutdown {
            pool.stop().await;
        } else {
            pool.configure(&[]);
        }
        timeout(Duration::from_secs(5), async {
            while reachable(&temporary) {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        sleep(Duration::from_millis(100)).await;
        assert!(!reachable(&baseline));
        assert!(!reachable(&temporary));
        pool.stop().await;
    }
}

#[tokio::test]
async fn local_commit_lease_rejects_external_owners_and_concurrent_leases() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    let mut external = LocalServer::new(&baseline);
    ready(&baseline).await;
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let error = with_model(&temporary, async { Ok(()) })
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("externally owned"), "{error}");
    assert!(
        reachable(&baseline),
        "an externally owned model must never be killed"
    );
    pool.stop().await;
    assert!(reachable(&baseline));
    external.stop().await;

    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let selected = temporary.clone();
    let task = tokio::spawn(async move {
        with_model(&selected, std::future::pending::<anyhow::Result<()>>()).await
    });
    ready(&temporary).await;
    assert!(with_model(&temporary, async { Ok(()) })
        .await
        .unwrap_err()
        .to_string()
        .contains("already active"));
    assert!(
        reachable(&temporary),
        "a rejected second lease must preserve the first"
    );
    task.abort();
    let _ = task.await;
    ready(&baseline).await;
    pool.stop().await;
}

#[tokio::test]
async fn local_commit_lease_restores_after_temporary_startup_failure() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    std::fs::remove_file(temporary.model_path.as_ref().unwrap()).unwrap();
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let error = with_model::<()>(&temporary, async { anyhow::bail!("must not run") })
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("model not found"), "{error}");
    ready(&baseline).await;
    assert!(!reachable(&temporary));
    pool.stop().await;
}

#[tokio::test]
async fn local_commit_lease_restoration_survives_caller_cancellation() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    std::fs::write(baseline.model_path.as_ref().unwrap(), "delay=1500\n").unwrap();
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let selected = temporary.clone();
    let task = tokio::spawn(async move { with_model(&selected, async { Ok(()) }).await });
    let events = baseline
        .model_path
        .as_ref()
        .unwrap()
        .with_extension("started");
    timeout(Duration::from_secs(10), async {
        loop {
            if std::fs::read_to_string(&events).unwrap().lines().count() == 2 {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    ready(&baseline).await;
    assert!(!reachable(&temporary));
    pool.stop().await;
}

#[tokio::test]
async fn local_commit_lease_rejects_external_temporary_without_unloading_residents() {
    let _messages = status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    std::fs::write(temporary.model_path.as_ref().unwrap(), "delay=0\n").unwrap();
    let mut outside = temporary.clone();
    outside.on_demand = false;
    let mut external = LocalServer::new(&outside);
    ready(&outside).await;
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let error = with_model::<()>(&temporary, async { anyhow::bail!("must not run") })
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("externally owned"), "{error}");
    ready(&baseline).await;
    assert!(reachable(&outside));
    assert_eq!(
        std::fs::read_to_string(
            baseline
                .model_path
                .as_ref()
                .unwrap()
                .with_extension("started")
        )
        .unwrap()
        .lines()
        .count(),
        1,
        "resident must not be unloaded or restarted when external 27B prevents switching"
    );
    pool.stop().await;
    assert!(reachable(&outside));
    external.stop().await;
}

#[tokio::test]
#[ignore = "requires explicit live GPU model paths and no external model processes"]
async fn local_generation_live_gpu_switches_to_27b_and_restores_9b() {
    use helix_view::ai_commit::AiCommitConfig;
    let _messages = status::setup();
    let baseline = AiProviderConfig {
        kind: helix_view::ai::AiProviderType::LlamaCpp,
        model: "live-9b".into(),
        model_path: Some(
            std::env::var("HELIX_AI_LIVE_9B_PATH")
                .expect("9B path")
                .into(),
        ),
        server_path: std::env::var("HELIX_AI_LIVE_SERVER")
            .expect("server path")
            .into(),
        endpoint: format!("http://127.0.0.1:{}/v1/chat/completions", port()),
        startup_timeout_ms: 180000,
        ..Default::default()
    };
    let mut temporary = baseline.clone();
    temporary.model = "live-27b".into();
    temporary.model_path = Some(
        std::env::var("HELIX_AI_LIVE_27B_PATH")
            .expect("27B path")
            .into(),
    );
    loop {
        temporary.endpoint = format!("http://127.0.0.1:{}/v1/chat/completions", port());
        if temporary.endpoint != baseline.endpoint {
            break;
        }
    }
    temporary.context_size = 16384;
    temporary.thinking = true;
    temporary.thinking_budget = Some(2048);
    temporary.reasoning_effort = Some("medium".into());
    temporary.on_demand = true;
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    timeout(Duration::from_secs(180), wait_ready(&baseline))
        .await
        .unwrap()
        .unwrap();
    assert!(!reachable(&temporary));
    let input=crate::handlers::ai_commit::AiFileInput {
        id:1,path:"core/stats.py".into(),staged:true,partial:false,eligible:true,
        diff:"@@ -1,2 +1,4 @@\n def mean(values):\n+    if not values:\n+        return 0.0\n     return sum(values) / len(values)\n".into(),
    };
    let config = AiCommitConfig {
        enabled: true,
        max_tokens: 1024,
        timeout_ms: 120000,
        ..Default::default()
    };
    let start = Instant::now();
    let result = crate::handlers::ai_commit::generate_message(
        &temporary,
        &config,
        &[input],
        "说明修复mean对空输入除零，未运行测试",
    )
    .await;
    let restored = wait_ready(&baseline).await.is_ok() && !reachable(&temporary);
    pool.stop().await;
    let message = result.unwrap().text;
    assert!(
        restored,
        "9B must be restored and 27B released before returning"
    );
    std::fs::write(std::env::var("HELIX_AI_LIVE_SMOKE_OUTPUT").expect("output path"),serde_json::json!({"restored_9b":restored,"released_27b":!reachable(&temporary),"elapsed_ms":start.elapsed().as_millis(),"message":message,"thinking_budget":2048,"reasoning_effort":"medium"}).to_string()).unwrap();
    println!("LIVE 9B -> 27B bounded generation -> 9B: passed");
}

#[tokio::test]
#[ignore = "requires explicit installed config path and exclusive local GPU access"]
async fn local_generation_live_installed_runtime_restores_completion_after_commit() {
    use helix_view::ai::AiFeature;
    let _messages = status::setup();
    let text = std::fs::read_to_string(std::env::var("HELIX_AI_LIVE_CONFIG").expect("config path"))
        .unwrap();
    let raw: crate::config::ConfigRaw = toml::from_str(&text).unwrap();
    let config: helix_view::editor::Config = raw.editor.unwrap().try_into().unwrap();
    let baseline = config.ai_provider(AiFeature::Completion).unwrap();
    let commit = config.ai_provider(AiFeature::Commit).unwrap();
    assert!(commit.on_demand);
    let mut pool = LocalServers::new(&config.active_ai_providers().unwrap());
    timeout(Duration::from_secs(180), wait_ready(&baseline))
        .await
        .unwrap()
        .unwrap();
    async fn probe_completion(
        config: &helix_view::ai_completion::AiCompletionConfig,
        model: &AiProviderConfig,
    ) -> anyhow::Result<String> {
        super::super::client::complete(config, model, &super::super::client::CompletionContext {
            filename: "synthetic/add.py".into(), language: "python".into(), indent_unit: "    ",
            prefix: "def add(a: int, b: int) -> int:\n    \"\"\"Return the sum of a and b.\"\"\"\n    return a + ".into(),
            suffix: "\n".into(), supporting: String::new(), members: None,
        }).await?.ok_or_else(|| anyhow::anyhow!("9B did not offer the contract-defined continuation"))
    }
    let completion_before = probe_completion(&config.ai_completion, &baseline).await;
    let input = crate::handlers::ai_commit::AiFileInput {
        id: 1, path: "synthetic/stats.py".into(), staged: true, partial: false, eligible: true,
        diff: "@@ -1,2 +1,4 @@\n def mean(values):\n+    if not values:\n+        return 0.0\n     return sum(values) / len(values)\n".into(),
    };
    let started = Instant::now();
    let message = crate::handlers::ai_commit::generate_message(
        &commit,
        &config.ai_commit,
        &[input],
        "说明修复mean空输入除零；未运行测试",
    )
    .await;
    let after_commit = wait_ready(&baseline).await.is_ok() && !reachable(&commit);
    let completion_after = probe_completion(&config.ai_completion, &baseline).await;
    pool.stop().await;
    let message = message.unwrap().text;
    assert!(after_commit, "generation must restore the resident model");
    assert!(!message.trim().is_empty());
    let completion_before = completion_before.unwrap();
    let completion_after = completion_after.unwrap();
    assert_eq!(completion_before.trim(), "b");
    assert_eq!(completion_after.trim(), "b");
    let evidence = serde_json::json!({
        "elapsed_seconds": started.elapsed().as_secs_f64(),
        "restored_after_commit": after_commit,
        "completion_before": completion_before, "completion_after": completion_after,
        "commit": {"model": commit.model, "context": commit.context_size, "gpu_layers": commit.gpu_layers, "runtime": commit.runtime, "text": message},
    });
    std::fs::write(
        std::env::var("HELIX_AI_LIVE_SMOKE_OUTPUT").expect("output path"),
        evidence.to_string(),
    )
    .unwrap();
}
