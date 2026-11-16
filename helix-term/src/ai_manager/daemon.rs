//! The helper owns model children; editor connections own only cancelable leases.
use super::{
    process,
    protocol::{self, Discovery, Envelope, Request, Response, VERSION},
};
use crate::handlers::ai_completion::server::{self, LocalServer};
use anyhow::{ensure, Context, Result};
use helix_view::ai::{AiProviderConfig, AiProviderType};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task::JoinSet,
    time::{timeout, Instant},
};

const IDLE: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Scheduler {
    current: Option<(AiProviderConfig, LocalServer)>,
}

impl Scheduler {
    async fn stop(&mut self) {
        if let Some((_, mut server)) = self.current.take() {
            server.stop().await;
        }
    }

    async fn load(&mut self, config: &AiProviderConfig) -> Result<()> {
        if let Some((loaded, running)) = &self.current {
            if server::same_server(loaded, config)
                && server::wait_managed_ready(loaded).await.is_ok()
            {
                return Ok(());
            }
            if !running.is_owned() {
                let url = server::local_url(loaded)?;
                // A borrowed process may have belonged to an older hx which
                // has since exited. Only discard it after proving its port is
                // free; never terminate or replace a live unknown owner.
                match std::net::TcpListener::bind((
                    "127.0.0.1",
                    url.port_or_known_default().unwrap(),
                )) {
                    Ok(listener) => drop(listener),
                    Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                        anyhow::bail!("Local AI server is externally owned; the shared manager cannot unload it");
                    }
                    Err(error) => {
                        return Err(error)
                            .context("Could not verify external AI endpoint ownership")
                    }
                }
            }
        }
        self.stop().await;
        self.current = Some((config.clone(), LocalServer::new(config)));
        let result = server::wait_managed_ready(config).await;
        if result.is_err() {
            self.stop().await;
        }
        result
    }

    async fn restore(&mut self, resident: Option<&AiProviderConfig>) -> Result<()> {
        if let Some(resident) = resident {
            self.load(resident).await?;
        }
        Ok(())
    }
}

struct Connection(Arc<AtomicUsize>);

impl Connection {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Self(count.clone())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn validate(config: &AiProviderConfig) -> Result<()> {
    config.validate()?;
    ensure!(
        config.kind == AiProviderType::LlamaCpp && config.model_path.is_some(),
        "Shared AI manager requires a managed llama-cpp provider"
    );
    server::local_url(config)?;
    Ok(())
}

async fn reply(stream: &mut TcpStream, result: Result<()>) -> Result<()> {
    let response = match result {
        Ok(()) => Response::Ok,
        Err(error) => Response::Error {
            message: format!("{error:#}"),
        },
    };
    timeout(Duration::from_secs(2), protocol::write(stream, &response)).await??;
    Ok(())
}

async fn handle(
    mut stream: TcpStream,
    token: Arc<String>,
    scheduler: Arc<Mutex<Scheduler>>,
    connections: Arc<AtomicUsize>,
) -> Result<()> {
    let envelope: Envelope = timeout(Duration::from_secs(5), protocol::read(&mut stream)).await??;
    if envelope.version != VERSION || envelope.token != *token {
        reply(
            &mut stream,
            Err(anyhow::anyhow!(
                "AI manager authentication or protocol version mismatch"
            )),
        )
        .await?;
        return Ok(());
    }
    let connection = Connection::new(&connections);
    match envelope.request {
        Request::Register { resident } => {
            if let Some(resident) = &resident {
                if let Err(error) = validate(resident) {
                    return reply(&mut stream, Err(error)).await;
                }
            }
            reply(&mut stream, Ok(())).await?;
            if let Some(resident) = resident {
                let mut state = tokio::select! {
                    guard = scheduler.lock() => guard,
                    _ = stream.read_u8() => return Ok(()),
                };
                if state.current.is_none() {
                    tokio::select! {
                        result = state.load(&resident) => {
                            if let Err(error) = result { log::warn!("AI model warmup: {error:#}"); }
                        }
                        _ = stream.read_u8() => { state.stop().await; return Ok(()); }
                    }
                }
            }
            let _ = stream.read_u8().await;
        }
        Request::Acquire { mut model, restore } => {
            let validated = validate(&model).and_then(|_| {
                if let Some(resident) = &restore {
                    validate(resident)?;
                }
                Ok(())
            });
            if let Err(error) = validated {
                return reply(&mut stream, Err(error)).await;
            }
            model.on_demand = restore.is_some();
            let mut state = tokio::select! {
                guard = scheduler.lock() => guard,
                _ = stream.read_u8() => return Ok(()),
            };
            // Check once more after entering the queue: an already disconnected
            // request must not unload another editor's warm model.
            let mut byte = [0];
            match stream.try_read(&mut byte) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                _ => return Ok(()),
            }
            let loaded = tokio::select! {
                result = state.load(&model) => Some(result),
                _ = stream.read_u8() => None,
            };
            let Some(loaded) = loaded else {
                drop(connection);
                state.stop().await;
                state.restore(restore.as_ref()).await?;
                return Ok(());
            };
            if let Err(error) = loaded {
                let _ = reply(&mut stream, Err(error)).await;
                drop(connection);
                // Preserve an external server, including when it blocked a switch.
                state.restore(restore.as_ref()).await?;
                return Ok(());
            }
            if reply(&mut stream, Ok(())).await.is_err() {
                drop(connection);
                state.restore(restore.as_ref()).await?;
                return Ok(());
            }
            // The HTTP request runs in the editor while this connection holds
            // the exclusive model lease. EOF is cancellation/crash notification.
            let released = stream.read_u8().await;
            // Restoration is background cleanup, not a live client. The last
            // editor's disconnect must still start the 10-second idle timer.
            drop(connection);
            let result = state.restore(restore.as_ref()).await;
            if matches!(released, Ok(1)) {
                reply(&mut stream, result).await?;
            } else if let Err(error) = result {
                log::warn!("AI model restoration after disconnect: {error:#}");
            }
        }
    }
    Ok(())
}

fn lock(root: &Path) -> Result<Option<File>> {
    std::fs::create_dir_all(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("manager.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) async fn run(root: &Path) -> Result<()> {
    let Some(_lock) = lock(root).context("Could not lock shared AI manager")? else {
        return Ok(());
    };
    process::initialize()?;
    crate::logging::init_file(log::LevelFilter::Warn, &root.join("manager.log"))?;
    // LocalServer publishes failures on the application's status channel. The
    // helper has no TUI, so drain it while retaining the ordinary log output.
    let mut messages = helix_event::status::setup();
    tokio::spawn(async move { while messages.recv().await.is_some() {} });
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let info = Discovery {
        version: VERSION,
        port: listener.local_addr()?.port(),
        token: protocol::random_token()?,
        pid: std::process::id(),
    };
    let mut metadata = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(metadata.as_file_mut(), &info)?;
    metadata
        .persist(root.join("manager.json"))
        .map_err(|error| error.error)?;
    let token = Arc::new(info.token);
    let scheduler = Arc::new(Mutex::new(Scheduler::default()));
    let connections = Arc::new(AtomicUsize::new(0));
    let mut handlers = JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut idle_since = Instant::now();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let token = token.clone();
                let scheduler = scheduler.clone();
                let connections = connections.clone();
                handlers.spawn(async move {
                    if let Err(error) = handle(stream, token, scheduler, connections).await {
                        log::debug!("AI manager connection ended: {error:#}");
                    }
                });
            }
            _ = tick.tick() => {
                if connections.load(Ordering::Acquire) != 0 { idle_since = Instant::now(); }
                else if idle_since.elapsed() >= IDLE { break; }
            }
            Some(result) = handlers.join_next(), if !handlers.is_empty() => {
                if let Err(error) = result { log::warn!("AI manager connection task failed: {error}"); }
            }
        }
    }
    drop(listener);
    handlers.abort_all();
    while handlers.join_next().await.is_some() {}
    scheduler.lock().await.stop().await;
    let _ = std::fs::remove_file(root.join("manager.json"));
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    pub async fn manager(root: &Path) -> (File, tokio::task::JoinHandle<()>) {
        let lock = lock(root).unwrap().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let info = Discovery {
            version: VERSION,
            port: listener.local_addr().unwrap().port(),
            token: protocol::random_token().unwrap(),
            pid: std::process::id(),
        };
        std::fs::write(
            root.join("manager.json"),
            serde_json::to_vec(&info).unwrap(),
        )
        .unwrap();
        let token = Arc::new(info.token);
        let scheduler = Arc::new(Mutex::new(Scheduler::default()));
        let connections = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        handlers.spawn(handle(stream, token.clone(), scheduler.clone(), connections.clone()));
                    }
                    Some(_) = handlers.join_next(), if !handlers.is_empty() => {}
                }
            }
        });
        (lock, task)
    }

    #[tokio::test]
    async fn shared_ai_lease_does_not_expire_during_a_multi_request_operation() {
        let _messages = helix_event::status::setup();
        let dir = tempfile::tempdir().unwrap();
        let (baseline, _) = server::lease_tests::models(dir.path(), 0);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let state = Arc::new(Mutex::new(Scheduler::default()));
        let token = protocol::random_token().unwrap();
        let handler = tokio::spawn(handle(
            stream,
            Arc::new(token.clone()),
            state.clone(),
            Arc::new(AtomicUsize::new(0)),
        ));
        protocol::write(
            &mut client,
            &Envelope {
                version: VERSION,
                token,
                request: Request::Acquire {
                    model: baseline,
                    restore: None,
                },
            },
        )
        .await
        .unwrap();
        protocol::read::<Response>(&mut client)
            .await
            .unwrap()
            .result()
            .unwrap();
        tokio::task::yield_now().await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(181)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        let expired = handler.is_finished();
        tokio::time::resume();
        if !expired {
            client.write_u8(1).await.unwrap();
            protocol::read::<Response>(&mut client)
                .await
                .unwrap()
                .result()
                .unwrap();
        }
        handler.await.unwrap().unwrap();
        state.lock().await.stop().await;
        assert!(
            !expired,
            "a connected multi-request operation must not lose its model after 180 seconds"
        );
    }
}
