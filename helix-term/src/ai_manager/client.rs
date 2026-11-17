//! Editor-side registration and cancelable, operation-scoped model leases.
use super::protocol::{self, Discovery, Envelope, Request, Response, VERSION};
use anyhow::{ensure, Context, Result};
use helix_event::runtime_local;
use helix_view::ai::AiProviderConfig;
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::watch,
    time::{sleep, timeout, Instant},
};

runtime_local! {
    static CLIENTS: parking_lot::Mutex<BTreeMap<String, Weak<Client>>> = parking_lot::Mutex::new(BTreeMap::new());
}

struct Client {
    root: PathBuf,
    providers: watch::Sender<Vec<AiProviderConfig>>,
    ready: watch::Sender<Option<std::result::Result<(), String>>>,
}

#[cfg(any(feature = "integration", not(any(windows, target_os = "linux"))))]
pub(crate) use crate::handlers::ai_completion::server::LocalServers;

/// A registration, not the owner of the model process.
#[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
pub(crate) struct LocalServers {
    client: Arc<Client>,
    worker: tokio::task::JoinHandle<()>,
}

#[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
impl LocalServers {
    pub(crate) fn new(providers: &[AiProviderConfig]) -> Self {
        let (providers, changes) = watch::channel(providers.to_vec());
        let (ready, _) = watch::channel(None);
        let client = Arc::new(Client {
            root: helix_loader::cache_dir().join("ai-manager"),
            providers,
            ready,
        });
        let worker = tokio::spawn(registrar(client.clone(), changes));
        let pool = Self { client, worker };
        pool.register();
        pool
    }

    fn register(&self) {
        let weak = Arc::downgrade(&self.client);
        let mut clients = CLIENTS.lock();
        clients.retain(|_, old| old.strong_count() != 0 && !old.ptr_eq(&weak));
        for provider in self
            .client
            .providers
            .borrow()
            .iter()
            .filter(|p| p.model_path.is_some())
        {
            clients.insert(provider.endpoint.clone(), weak.clone());
        }
    }

    pub(crate) fn configure(&mut self, providers: &[AiProviderConfig]) {
        self.client.ready.send_replace(None);
        self.client.providers.send_replace(providers.to_vec());
        self.register();
    }

    pub(crate) async fn stop(&mut self) {
        self.client.providers.send_replace(Vec::new());
        self.client
            .ready
            .send_replace(Some(Err("AI model registration stopped".into())));
        self.worker.abort();
        let _ = (&mut self.worker).await;
        self.register();
    }
}

#[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
impl Drop for LocalServers {
    fn drop(&mut self) {
        self.client.providers.send_replace(Vec::new());
        self.worker.abort();
        self.register();
    }
}

fn lookup(config: &AiProviderConfig) -> Option<Arc<Client>> {
    if config.model_path.is_none() {
        return None;
    }
    CLIENTS.lock().get(&config.endpoint).and_then(Weak::upgrade)
}

pub(crate) fn is_registered(config: &AiProviderConfig) -> bool {
    lookup(config).is_some()
}

fn normalized(config: &AiProviderConfig) -> AiProviderConfig {
    let mut config = config.clone();
    if let Some(path) = config.model_path.as_mut() {
        *path = helix_stdx::path::canonicalize(helix_stdx::path::expand(&*path));
    }
    let expanded = helix_stdx::path::expand(&config.server_path);
    // A bare executable name still uses PATH; relative file paths must be
    // resolved in the requesting editor, not the manager's original directory.
    if expanded.components().count() > 1 {
        config.server_path = helix_stdx::path::canonicalize(expanded);
    }
    config
}

async fn discover(root: &Path) -> Result<(TcpStream, Discovery)> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut spawned = false;
    loop {
        if manager_is_running(root)? {
            if let Ok(bytes) = tokio::fs::read(root.join("manager.json")).await {
                if bytes.len() <= 4096 {
                    if let Ok(info) = serde_json::from_slice::<Discovery>(&bytes) {
                        if let Ok(Ok(stream)) = timeout(
                            Duration::from_millis(250),
                            TcpStream::connect(("127.0.0.1", info.port)),
                        )
                        .await
                        {
                            ensure!(info.version == VERSION, "Shared AI manager protocol changed; close old editors before upgrading");
                            return Ok((stream, info));
                        }
                    }
                }
            }
        }
        if !spawned {
            let mut command = tokio::process::Command::new(std::env::current_exe()?);
            command
                .arg("--internal-ai-manager")
                .arg(root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(false);
            #[cfg(windows)]
            command.creation_flags(0x0800_0200); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
            #[cfg(unix)]
            command.process_group(0);
            let mut child = command
                .spawn()
                .context("Could not start shared AI manager")?;
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            spawned = true;
        }
        ensure!(
            Instant::now() < deadline,
            "Shared AI manager did not start; see {}",
            root.join("manager.log").display()
        );
        sleep(Duration::from_millis(50)).await;
    }
}

fn manager_is_running(root: &Path) -> Result<bool> {
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("manager.lock"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("Could not inspect shared AI manager lock"),
    };
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(error) => Err(error.into()),
    }
}

async fn connect(root: &Path, request: Request, budget: Duration) -> Result<TcpStream> {
    let (mut stream, discovery) = discover(root).await?;
    protocol::write(
        &mut stream,
        &Envelope {
            version: VERSION,
            token: discovery.token,
            request,
        },
    )
    .await?;
    timeout(budget, protocol::read::<Response>(&mut stream))
        .await
        .context("Shared AI model wait timed out")??
        .result()?;
    Ok(stream)
}

#[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
async fn registrar(client: Arc<Client>, mut changes: watch::Receiver<Vec<AiProviderConfig>>) {
    loop {
        let providers = changes.borrow_and_update().clone();
        if !providers.iter().any(|p| p.model_path.is_some()) {
            if changes.changed().await.is_err() {
                break;
            }
            continue;
        }
        let resident = providers
            .iter()
            .find(|p| p.model_path.is_some() && !p.on_demand)
            .map(normalized);
        let result = tokio::select! {
            result = connect(&client.root, Request::Register { resident }, Duration::from_secs(5)) => result,
            _ = changes.changed() => continue,
        };
        match result {
            Ok(mut connection) => {
                client.ready.send_replace(Some(Ok(())));
                tokio::select! {
                    _ = connection.read_u8() => {},
                    _ = changes.changed() => {},
                }
                client.ready.send_replace(None);
            }
            Err(error) => {
                let error = format!("{error:#}");
                log::warn!("Shared AI manager: {error}");
                client.ready.send_replace(Some(Err(error)));
                tokio::select! { _ = sleep(Duration::from_secs(2)) => {}, _ = changes.changed() => {} }
            }
        }
    }
}

pub(crate) async fn with_model<T>(
    config: &AiProviderConfig,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    let client =
        lookup(config).context("Shared AI provider is not registered; reload configuration")?;
    let mut ready = client.ready.subscribe();
    let mut changes = client.providers.subscribe();
    let providers = changes.borrow_and_update().clone();
    ensure!(providers.contains(config), "AI model configuration changed");
    let restore = if config.on_demand {
        providers
            .iter()
            .find(|p| p.model_path.is_some() && !p.on_demand)
            .map(normalized)
    } else {
        None
    };
    // Explicit-only configurations still need an exclusive temporary request,
    // even if there is no ordinary completion model to restore.
    let budget = Duration::from_millis(config.startup_timeout_ms.clamp(100, 600_000));
    let registration = timeout(Duration::from_secs(20), async {
        loop {
            if let Some(result) = ready.borrow_and_update().clone() {
                return result.map_err(anyhow::Error::msg);
            }
            ready
                .changed()
                .await
                .context("AI model registration stopped")?;
        }
    });
    tokio::select! {
        result = registration => result.context("Shared AI manager connection timed out")??,
        _ = changes.changed() => anyhow::bail!("AI model configuration changed"),
    }
    let acquisition = connect(
        &client.root,
        Request::Acquire {
            model: normalized(config),
            restore,
        },
        budget,
    );
    let mut lease = tokio::select! {
        result = acquisition => result?,
        _ = changes.changed() => anyhow::bail!("AI model configuration changed"),
    };
    ensure!(
        client.providers.borrow().contains(config),
        "AI model configuration changed"
    );
    let result = tokio::select! {
        result = operation => result,
        _ = lease.read_u8() => Err(anyhow::anyhow!("Shared AI manager disconnected during inference; retry the request")),
        _ = changes.changed() => Err(anyhow::anyhow!("AI model configuration changed")),
    };
    let restored = timeout(budget, async {
        lease.write_u8(1).await?;
        protocol::read::<Response>(&mut lease).await?.result()
    })
    .await
    .context("Shared AI model restoration timed out")?;
    match result {
        Ok(value) => restored.map(|_| value),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shared_ai_discovery_does_not_trust_a_stale_port_reused_by_another_service() {
        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stale = Discovery {
            version: VERSION,
            port: listener.local_addr().unwrap().port(),
            token: "stale".into(),
            pid: 1,
        };
        std::fs::write(
            dir.path().join("manager.json"),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        let result = timeout(Duration::from_millis(400), discover(dir.path())).await;
        assert!(
            result.is_err(),
            "a recycled TCP port must not be returned as a live AI manager"
        );
    }

    #[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
    fn registration(root: &Path, providers: &[AiProviderConfig]) -> LocalServers {
        let (providers, changes) = watch::channel(providers.to_vec());
        let (ready, _) = watch::channel(None);
        let client = Arc::new(Client {
            root: root.into(),
            providers,
            ready,
        });
        let worker = tokio::spawn(registrar(client.clone(), changes));
        let pool = LocalServers { client, worker };
        pool.register();
        pool
    }

    #[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
    #[tokio::test]
    async fn shared_ai_production_registration_reuses_one_model_after_first_editor_stops() {
        let _messages = helix_event::status::setup();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manager");
        let (_lock, manager) = super::super::daemon::tests::manager(&root).await;
        let (baseline, _) =
            crate::handlers::ai_completion::server::lease_tests::models(dir.path(), 0);
        let mut first = registration(&root, &[baseline.clone()]);
        let mut second = registration(&root, &[baseline.clone()]);
        assert_eq!(with_model(&baseline, async { Ok(42) }).await.unwrap(), 42);
        first.stop().await;
        assert_eq!(with_model(&baseline, async { Ok(43) }).await.unwrap(), 43);
        let starts = std::fs::read_to_string(
            baseline
                .model_path
                .as_ref()
                .unwrap()
                .with_extension("started"),
        )
        .unwrap();
        assert_eq!(starts.lines().count(), 1);
        second.stop().await;
        manager.abort();
        let _ = manager.await;
    }

    #[cfg(all(not(feature = "integration"), any(windows, target_os = "linux")))]
    #[tokio::test]
    async fn shared_ai_configuration_reload_cancels_active_inference_and_releases_lease() {
        let _messages = helix_event::status::setup();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manager");
        let (_lock, manager) = super::super::daemon::tests::manager(&root).await;
        let (baseline, _) =
            crate::handlers::ai_completion::server::lease_tests::models(dir.path(), 0);
        let mut pool = registration(&root, &[baseline.clone()]);
        let (started, active) = tokio::sync::oneshot::channel();
        let config = baseline.clone();
        let mut request = tokio::spawn(async move {
            with_model(&config, async {
                let _ = started.send(());
                std::future::pending::<Result<()>>().await
            })
            .await
        });
        active.await.unwrap();
        let mut updated = baseline.clone();
        updated.context_size += 512;
        pool.configure(&[updated.clone()]);
        let cancelled = timeout(Duration::from_secs(2), &mut request).await;
        assert!(
            cancelled.is_ok(),
            "configuration reload must cancel a lease holding the previous model"
        );
        assert!(cancelled.unwrap().unwrap().is_err());
        with_model(&updated, async { Ok(()) }).await.unwrap();
        pool.stop().await;
        manager.abort();
        let _ = manager.await;
    }
}
