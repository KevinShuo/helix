//! Cancellable, application-owned model switching for explicit local requests.
use super::{report_error, wait_ready, AiProviderConfig, LocalServer};
use anyhow::{ensure, Context, Result};
use helix_event::runtime_local;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
};
use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
};

runtime_local! {
    static POOLS: parking_lot::Mutex<BTreeMap<String, Weak<Shared>>> = parking_lot::Mutex::new(BTreeMap::new());
}
static NEXT_LEASE: AtomicU64 = AtomicU64::new(1);

struct Models {
    servers: BTreeMap<String, LocalServer>,
    temporary: Option<u64>,
    epoch: u64,
}

impl Models {
    fn start_residents(providers: &[AiProviderConfig], epoch: u64) -> Self {
        Self {
            servers: providers
                .iter()
                .filter(|p| p.model_path.is_some() && !p.on_demand)
                .map(|p| (p.endpoint.clone(), LocalServer::new(p)))
                .collect(),
            temporary: None,
            epoch,
        }
    }

    async fn stop(&mut self) {
        for server in self.servers.values_mut() {
            server.stop().await;
        }
        self.servers.clear();
    }
}

struct Shared {
    models: Mutex<Models>,
    configured: parking_lot::RwLock<Vec<AiProviderConfig>>,
    epoch: watch::Sender<u64>,
    stopped: AtomicBool,
}

impl Shared {
    fn snapshot(&self) -> (u64, Vec<AiProviderConfig>) {
        let providers = self.configured.read();
        (*self.epoch.borrow(), providers.clone())
    }

    async fn apply_latest(&self, models: &mut Models) {
        if models.epoch == *self.epoch.borrow() {
            return;
        }
        let (epoch, providers) = self.snapshot();
        let residents: Vec<_> = providers
            .iter()
            .filter(|p| p.model_path.is_some() && !p.on_demand)
            .collect();
        if models.temporary.is_none()
            && models.servers.len() == residents.len()
            && residents.iter().all(|p| {
                models
                    .servers
                    .get(&p.endpoint)
                    .is_some_and(|server| super::same_server(&server._config.borrow(), p))
            })
        {
            for provider in residents {
                models
                    .servers
                    .get_mut(&provider.endpoint)
                    .unwrap()
                    .configure(provider);
            }
            models.epoch = epoch;
            return;
        }
        models.stop().await;
        models.temporary = None;
        let (epoch, providers) = self.snapshot();
        if !self.stopped.load(Ordering::Acquire) {
            *models = Models::start_residents(&providers, epoch);
        }
    }

    async fn restore(&self, id: u64) -> Result<()> {
        let residents = {
            let mut models = self.models.lock().await;
            if models.temporary != Some(id) {
                return Ok(());
            }
            models.stop().await;
            models.temporary = None;
            let (epoch, providers) = self.snapshot();
            if self.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            *models = Models::start_residents(&providers, epoch);
            providers
                .into_iter()
                .filter(|p| p.model_path.is_some() && !p.on_demand)
                .collect::<Vec<_>>()
        };
        // Keep automatic requests suspended until the ordinary model is ready.
        for model in residents {
            wait_ready(&model).await?;
        }
        Ok(())
    }

    async fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.epoch.send_modify(|epoch| *epoch += 1);
        let mut models = self.models.lock().await;
        models.stop().await;
        models.temporary = None;
    }
}

pub(crate) struct LocalServers {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl LocalServers {
    pub(crate) fn new(providers: &[AiProviderConfig]) -> Self {
        let (epoch, mut changes) = watch::channel(0);
        let shared = Arc::new(Shared {
            models: Mutex::new(Models::start_residents(providers, 0)),
            configured: parking_lot::RwLock::new(providers.to_vec()),
            epoch,
            stopped: AtomicBool::new(false),
        });
        let handle = shared.clone();
        let worker = tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                if handle.stopped.load(Ordering::Acquire) {
                    break;
                }
                let mut models = handle.models.lock().await;
                handle.apply_latest(&mut models).await;
            }
        });
        let pool = Self {
            shared,
            worker: Some(worker),
        };
        pool.register(providers);
        pool
    }

    fn register(&self, providers: &[AiProviderConfig]) {
        let weak = Arc::downgrade(&self.shared);
        let mut pools = POOLS.lock();
        pools.retain(|_, existing| existing.strong_count() != 0 && !existing.ptr_eq(&weak));
        for provider in providers {
            pools.insert(provider.endpoint.clone(), weak.clone());
        }
    }

    pub(crate) fn configure(&mut self, providers: &[AiProviderConfig]) {
        self.register(providers);
        let mut configured = self.shared.configured.write();
        // Config reload also retries failed resident startup, as before.
        *configured = providers.to_vec();
        self.shared.epoch.send_modify(|epoch| *epoch += 1);
    }

    pub(crate) async fn stop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.epoch.send_modify(|epoch| *epoch += 1);
        if let Some(worker) = self.worker.take() {
            worker.abort();
            let _ = worker.await;
        }
        self.shared.shutdown().await;
        self.register(&[]);
    }

    #[cfg(test)]
    pub(super) async fn resident_count(&self) -> usize {
        self.shared.models.lock().await.servers.len()
    }
}

impl Drop for LocalServers {
    fn drop(&mut self) {
        self.register(&[]);
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.epoch.send_modify(|epoch| *epoch += 1);
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let handle = self.shared.clone();
            runtime.spawn(async move {
                handle.shutdown().await;
            });
        }
    }
}

struct Lease {
    shared: Arc<Shared>,
    id: u64,
    armed: bool,
}

impl Lease {
    async fn acquire(model: &AiProviderConfig) -> Result<Self> {
        let shared = POOLS
            .lock()
            .get(&model.endpoint)
            .and_then(Weak::upgrade)
            .context("On-demand local AI provider is not configured; reload configuration")?;
        let mut changes = shared.epoch.subscribe();
        let (epoch, configured) = shared.snapshot();
        ensure!(
            !shared.stopped.load(Ordering::Acquire) && configured.contains(model),
            "AI model configuration changed"
        );
        let lease = Self {
            shared,
            id: NEXT_LEASE.fetch_add(1, Ordering::Relaxed),
            armed: true,
        };
        {
            let mut models = lease.shared.models.lock().await;
            ensure!(
                !lease.shared.stopped.load(Ordering::Acquire)
                    && epoch == *lease.shared.epoch.borrow(),
                "AI model configuration changed"
            );
            ensure!(
                models.temporary.is_none(),
                "Another temporary AI model is already active"
            );
            let url = super::local_url(model)?;
            // Do this before unloading residents. An external temporary server
            // cannot be released by our lease and must never be reused.
            let listener = std::net::TcpListener::bind(("127.0.0.1", url.port_or_known_default().unwrap()))
                .context("On-demand local AI endpoint is externally owned or in use; close its server before switching models")?;
            drop(listener);
            // Reused external processes cannot be unloaded safely by this editor.
            for server in models.servers.values() {
                let resident = server._config.borrow().clone();
                tokio::select! {
                    result = wait_ready(&resident) => result?,
                    _ = changes.changed() => anyhow::bail!("AI model configuration changed"),
                }
                ensure!(server.state.borrow().owned, "Local AI server is externally owned; close its owning editor/server before switching models");
            }
            models.temporary = Some(lease.id);
            models.stop().await;
            ensure!(
                !lease.shared.stopped.load(Ordering::Acquire)
                    && epoch == *lease.shared.epoch.borrow(),
                "AI model configuration changed"
            );
            models
                .servers
                .insert(model.endpoint.clone(), LocalServer::new(model));
            models.epoch = epoch;
        }
        wait_ready(model).await?;
        ensure!(
            lease
                .shared
                .models
                .lock()
                .await
                .servers
                .get(&model.endpoint)
                .is_some_and(|server| server.state.borrow().owned),
            "On-demand local AI server must be owned by this editor"
        );
        ensure!(
            epoch == *lease.shared.epoch.borrow() && !lease.shared.stopped.load(Ordering::Acquire),
            "AI model configuration changed"
        );
        Ok(lease)
    }

    async fn finish(mut self) {
        let shared = self.shared.clone();
        let id = self.id;
        self.armed = false;
        // Restoration must survive cancellation of the caller awaiting it.
        let _ = tokio::spawn(async move {
            if let Err(error) = shared.restore(id).await {
                report_error(&error.to_string()).await;
            }
        })
        .await;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.armed || self.shared.stopped.load(Ordering::Acquire) {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let shared = self.shared.clone();
            let id = self.id;
            runtime.spawn(async move {
                if let Err(error) = shared.restore(id).await {
                    report_error(&error.to_string()).await;
                }
            });
        }
    }
}

pub(crate) async fn with_model<T>(
    model: &AiProviderConfig,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    if !model.on_demand {
        return operation.await;
    }
    let lease = Lease::acquire(model).await?;
    let result = operation.await;
    lease.finish().await;
    result
}
