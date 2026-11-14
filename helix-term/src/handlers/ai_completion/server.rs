//! Application-owned llama.cpp process for a configured local GGUF model.

use std::{path::PathBuf, process::Stdio, time::Duration};

use anyhow::{bail, Context as _};
use helix_event::{runtime_local, status};
use helix_view::ai_completion::AiCompletionConfig;
use tokio::{
    process::{Child, Command},
    sync::watch,
    task::JoinHandle,
    time::{sleep, timeout, Instant},
};

#[derive(Clone)]
struct ServerState {
    config: AiCompletionConfig,
    ready: Option<Result<(), String>>,
}

runtime_local! {
    static STATE: parking_lot::Mutex<Option<watch::Receiver<ServerState>>> = parking_lot::Mutex::new(None);
}

/// The application owns this task, rather than a keystroke's cancelable request.
/// Aborting the task drops its kill-on-drop child even during model loading.
pub(crate) struct LocalServer {
    config: watch::Sender<AiCompletionConfig>,
    state: watch::Sender<ServerState>,
    task: Option<JoinHandle<()>>,
}

impl LocalServer {
    pub(crate) fn new(config: &AiCompletionConfig) -> Self {
        let (config_tx, config_rx) = watch::channel(config.clone());
        let (state_tx, state_rx) = watch::channel(ServerState {
            config: config.clone(),
            ready: None,
        });
        *STATE.lock() = Some(state_rx);
        let task = tokio::spawn(supervise(config_rx, state_tx.clone()));
        Self {
            config: config_tx,
            state: state_tx,
            task: Some(task),
        }
    }

    pub(crate) fn configure(&mut self, config: &AiCompletionConfig) {
        let failed = matches!(self.state.borrow().ready, Some(Err(_)));
        if same_server(&self.config.borrow(), config) && !failed {
            return;
        }
        self.state.send_replace(ServerState {
            config: config.clone(),
            ready: None,
        });
        self.config.send_replace(config.clone());
    }

    pub(crate) async fn stop(&mut self) {
        self.state.send_modify(|state| {
            state.ready = Some(Err("Local AI server stopped".into()));
        });
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

fn same_server(a: &AiCompletionConfig, b: &AiCompletionConfig) -> bool {
    a.enabled == b.enabled
        && a.model_path == b.model_path
        && a.server_path == b.server_path
        && a.endpoint == b.endpoint
        && a.model == b.model
        && a.api_key_env == b.api_key_env
        && a.context_size == b.context_size
        && a.gpu_layers == b.gpu_layers
        && a.startup_timeout_ms == b.startup_timeout_ms
}

pub(crate) async fn wait_ready(config: &AiCompletionConfig) -> anyhow::Result<()> {
    if config.model_path.is_none() {
        return Ok(());
    }
    let mut state = STATE
        .lock()
        .clone()
        .context("Local AI server is not initialized")?;
    loop {
        {
            let state = state.borrow_and_update();
            if !same_server(&state.config, config) || !config.enabled {
                bail!("AI completion configuration changed");
            }
            if let Some(result) = &state.ready {
                result.clone().map_err(anyhow::Error::msg)?;
                break;
            }
        }
        state.changed().await.context("Local AI server stopped")?;
    }
    // A reused server can be replaced without a child exit notification. Check
    // identity immediately before allowing a request containing source code.
    if !serves_model(&models_request(config)?, &config.model).await {
        bail!("Local AI endpoint no longer serves model {}", config.model);
    }
    Ok(())
}

async fn supervise(
    mut configs: watch::Receiver<AiCompletionConfig>,
    states: watch::Sender<ServerState>,
) {
    let mut child = None;
    loop {
        let config = configs.borrow_and_update().clone();
        let result = if config.enabled && config.model_path.is_some() {
            tokio::select! {
                result = prepare(&config, &mut child) => result,
                changed = configs.changed() => {
                    stop_child(&mut child).await;
                    if changed.is_err() { break; }
                    continue;
                }
            }
        } else {
            Ok(())
        };
        let ready = result.map_err(|error| format!("{error:#}"));
        if let Err(error) = &ready {
            report_error(error).await;
            stop_child(&mut child).await;
        }
        // configure() can publish a new pending state while prepare() finishes.
        // Never overwrite that newer state with the old model's readiness.
        states.send_if_modified(|state| {
            if same_server(&state.config, &config) {
                state.ready = Some(ready.clone());
                true
            } else {
                false
            }
        });

        let changed = if let Some(process) = child.as_mut() {
            tokio::select! {
                changed = configs.changed() => changed,
                exit = process.wait() => {
                    let error = format!("Local AI server exited ({exit:?}); reload the config to retry");
                    states.send_if_modified(|state| {
                        if same_server(&state.config, &config) {
                            state.ready = Some(Err(error.clone()));
                            true
                        } else {
                            false
                        }
                    });
                    report_error(&error).await;
                    configs.changed().await
                }
            }
        } else {
            configs.changed().await
        };
        stop_child(&mut child).await;
        if changed.is_err() {
            break;
        }
    }
}

async fn report_error(error: &str) {
    log::warn!("AI completion: {error}");
    status::report(status::StatusMessage {
        severity: status::Severity::Warning,
        message: format!("AI completion: {error}").into(),
    })
    .await;
}

async fn stop_child(child: &mut Option<Child>) {
    if let Some(mut process) = child.take() {
        let _ = process.kill().await;
    }
}

async fn prepare(config: &AiCompletionConfig, child: &mut Option<Child>) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(&config.endpoint).ok();
    let url = url.filter(|url| {
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/v1/chat/completions"
            && url.query().is_none()
            && url.fragment().is_none()
    }).context("Local model endpoint must be http://127.0.0.1:PORT/v1/chat/completions without credentials")?;
    let port = url.port_or_known_default().unwrap();
    let model_path = config
        .model_path
        .as_ref()
        .context("Local model path is unset")?;
    let model_path = helix_stdx::path::expand(model_path);
    if !model_path.is_file() {
        bail!("Local AI model not found: {}", model_path.display());
    }
    if config.model.trim().is_empty() {
        bail!("Local AI model alias must not be empty");
    }
    let request = models_request(config)?;

    // A refused TCP connection can take seconds on Windows. Binding is an
    // immediate availability check and also detects non-HTTP port occupants.
    match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => drop(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if serves_model(&request, &config.model).await {
                return Ok(());
            }
            bail!("Port {port} is in use but does not serve AI model {}; choose another endpoint port", config.model);
        }
        Err(error) => {
            return Err(error).context(format!("Could not bind local AI endpoint port {port}"))
        }
    }

    let log_dir = helix_loader::cache_dir().join("ai-completion");
    tokio::fs::create_dir_all(&log_dir)
        .await
        .context("Could not create local AI log directory")?;
    let log_path = log_dir.join(format!("server-{}-{port}.log", std::process::id()));
    let log = std::fs::File::create(&log_path).context("Could not open local AI server log")?;
    let server_path: PathBuf = helix_stdx::path::expand(&config.server_path).into_owned();
    let mut command = Command::new(&server_path);
    command
        .arg("--model")
        .arg(&*model_path)
        .arg("--alias")
        .arg(&config.model)
        .args(["--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .arg("--ctx-size")
        .arg(config.context_size.clamp(512, 131_072).to_string())
        .arg("--n-gpu-layers")
        .arg(config.gpu_layers.min(999).to_string())
        .args([
            "--parallel",
            "1",
            "--flash-attn",
            "on",
            "--cache-type-k",
            "f16",
            "--cache-type-v",
            "f16",
            "--jinja",
        ])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    *child = Some(command.spawn().with_context(|| {
        format!(
            "Could not start {}; set server-path to the llama-server executable",
            server_path.display()
        )
    })?);
    log::info!(
        "Loading local AI model {}; server log: {}",
        model_path.display(),
        log_path.display()
    );

    let deadline =
        Instant::now() + Duration::from_millis(config.startup_timeout_ms.clamp(100, 600_000));
    let loading = async {
        loop {
            if let Some(exit) = child.as_mut().unwrap().try_wait()? {
                bail!(
                    "Local AI server exited ({exit}); see {}",
                    log_path.display()
                );
            }
            if serves_model(&request, &config.model).await {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    };
    timeout(deadline.saturating_duration_since(Instant::now()), loading)
        .await
        .with_context(|| {
            format!(
                "Local AI model startup timed out; see {}",
                log_path.display()
            )
        })?
}

fn models_request(config: &AiCompletionConfig) -> anyhow::Result<reqwest::RequestBuilder> {
    let url = reqwest::Url::parse(&config.endpoint)?.join("/v1/models")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(500))
        .build()?;
    let mut request = client.get(url);
    if let Some(name) = config
        .api_key_env
        .as_deref()
        .filter(|name| !name.is_empty())
    {
        let token = std::env::var(name)
            .ok()
            .filter(|token| !token.trim().is_empty())
            .context("AI API key environment variable is unset or empty")?;
        request = request.bearer_auth(token);
    }
    Ok(request)
}

async fn serves_model(request: &reqwest::RequestBuilder, model: &str) -> bool {
    let Some(request) = request.try_clone() else {
        return false;
    };
    let Ok(response) = request.send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(body) = response.json::<serde_json::Value>().await else {
        return false;
    };
    body["data"].as_array().is_some_and(|models| {
        models
            .iter()
            .any(|entry| entry["id"].as_str() == Some(model))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn local_config(model: &std::path::Path) -> AiCompletionConfig {
        AiCompletionConfig {
            enabled: true,
            model: "local-code-model".into(),
            model_path: Some(model.to_owned()),
            server_path: "missing-llama-server".into(),
            ..AiCompletionConfig::default()
        }
    }

    async fn model_list_server(
        model: &str,
    ) -> (
        String,
        tokio::task::JoinHandle<()>,
        std::sync::Arc<parking_lot::RwLock<String>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let served_model = std::sync::Arc::new(parking_lot::RwLock::new(model.to_owned()));
        let current_model = served_model.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let body = serde_json::json!({"data": [{"id": *current_model.read()}]}).to_string();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0; 1024];
                    loop {
                        let n = socket.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..n]);
                        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                            break;
                        }
                    }
                    assert!(request.starts_with(b"GET /v1/models "));
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        (endpoint, server, served_model)
    }

    #[tokio::test]
    async fn local_model_reuses_a_matching_external_server_without_owning_it() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let (endpoint, external, _) = model_list_server("local-code-model").await;
        let mut config = local_config(model.path());
        config.endpoint = endpoint.clone();
        let mut child = None;
        prepare(&config, &mut child).await.unwrap();
        assert!(
            child.is_none(),
            "an existing process must never become owned"
        );
        drop(child);
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(endpoint.replace("chat/completions", "models"))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "the external server must remain alive"
        );
        external.abort();
    }

    #[tokio::test]
    async fn local_model_refuses_a_port_serving_another_model() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let (endpoint, external, _) = model_list_server("other-model").await;
        let mut config = local_config(model.path());
        config.endpoint = endpoint.clone();
        let mut child = None;
        let error = prepare(&config, &mut child).await.unwrap_err().to_string();
        assert!(error.contains("does not serve"), "{error}");
        assert!(child.is_none());
        assert!(tokio::net::TcpStream::connect(
            reqwest::Url::parse(&endpoint)
                .unwrap()
                .socket_addrs(|| None)
                .unwrap()[0]
        )
        .await
        .is_ok());
        external.abort();
    }

    #[tokio::test]
    async fn local_model_rejects_non_loopback_or_non_chat_endpoints() {
        let model = tempfile::NamedTempFile::new().unwrap();
        for endpoint in [
            "http://example.com:19240/v1/chat/completions",
            "http://127.0.0.1:19240/other",
            "https://127.0.0.1:19240/v1/chat/completions",
            "http://user:password@127.0.0.1:19240/v1/chat/completions",
        ] {
            let mut config = local_config(model.path());
            config.endpoint = endpoint.into();
            let error = prepare(&config, &mut None).await.unwrap_err().to_string();
            assert!(
                error.contains("127.0.0.1") && error.contains("endpoint"),
                "{error}"
            );
            assert!(!error.contains("password"));
        }
    }

    #[tokio::test]
    async fn local_model_reports_a_missing_model_before_starting_any_process() {
        let directory = tempfile::tempdir().unwrap();
        let config = local_config(&directory.path().join("missing.gguf"));
        let mut child = None;
        let error = prepare(&config, &mut child).await.unwrap_err().to_string();
        assert!(error.contains("model not found"), "{error}");
        assert!(child.is_none());
    }

    #[tokio::test]
    async fn local_model_reports_a_missing_runtime_promptly() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = local_config(model.path());
        config.endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        drop(listener);
        let error = tokio::time::timeout(Duration::from_secs(5), prepare(&config, &mut None))
            .await
            .unwrap()
            .unwrap_err()
            .to_string();
        assert!(error.contains("server-path"), "{error}");
    }

    #[tokio::test]
    async fn local_model_detects_early_process_exit_without_waiting_for_timeout() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = local_config(model.path());
        config.endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        // The Rust test executable rejects llama.cpp arguments and exits immediately.
        config.server_path = std::env::current_exe().unwrap();
        drop(listener);
        let mut child = None;
        let error = tokio::time::timeout(Duration::from_secs(5), prepare(&config, &mut child))
            .await
            .unwrap()
            .unwrap_err()
            .to_string();
        assert!(error.contains("exited"), "{error}");
    }

    #[tokio::test]
    async fn local_model_can_retry_failed_startup_on_config_reload() {
        let _messages = status::setup();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model with spaces.gguf");
        let (endpoint, external, served_model) = model_list_server("local-code-model").await;
        let mut config = local_config(&path);
        config.endpoint = endpoint;
        let mut server = LocalServer::new(&config);
        let error = wait_ready(&config).await.unwrap_err().to_string();
        assert!(error.contains("model not found"), "{error}");
        std::fs::write(&path, b"model fixture").unwrap();
        server.configure(&config);
        timeout(Duration::from_secs(5), wait_ready(&config))
            .await
            .unwrap()
            .unwrap();
        // Unrelated completion tuning must keep the loaded provider available.
        config.debounce_ms += 1;
        config.max_tokens += 1;
        server.configure(&config);
        wait_ready(&config).await.unwrap();
        *served_model.write() = "different-model".into();
        assert!(
            wait_ready(&config).await.is_err(),
            "a replaced external model must be revalidated before sending source code"
        );
        server.stop().await;
        external.abort();
    }
}
