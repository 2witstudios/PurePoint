//! Daemon-side composition of owner-local mesh administration, the explicit
//! paired listener and host routing. Nothing listens until the owner both
//! initializes an identity and configures specific addresses.
//!
//! Every administrative mutation is serialized with listener replacement: the old
//! listener is stopped and drained before the change publishes, and the reply is
//! sent only after the new applied state is known. Closing bindings alone is not
//! an admission fence, so no handshake can reach a superseded listener.

use crate::{
    daemon_lifecycle::{
        DaemonLock,
        mesh_admin::{self, MeshAdminService},
        mesh_store::peer_id_from_name,
        owned_work::WorkOwner,
    },
    execution_state::{Caller, Limits, LocalAdmin},
    mesh_exec::{Executor, supervisor::Guardian},
    mesh_host::{self, HostClient},
    mesh_listener::BoundListeners,
    mesh_sessions::SessionLimits,
};
use pu_core::mesh::{
    admin::{MeshAdminRequest, MeshAdminResponse},
    local::{ListenerFailure, ListenerStatus, LocalRoute},
    response::Response,
};
use std::{collections::BTreeSet, sync::Arc};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

/// Receiver-approved runtimes. Remote callers name one of these IDs; they never
/// supply an executable, arguments or shell. The plain terminal is excluded.
pub const RUNTIME_IDS: [&str; 3] = ["claude", "codex", "opencode"];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Admin(#[from] mesh_admin::Error),
    #[error(transparent)]
    Host(#[from] mesh_host::Error),
    #[error("worker is not routed from this host")]
    UnknownWorker,
    #[error("mesh runtime is unavailable")]
    Unavailable,
}

/// Guardian executable for mesh runs: this daemon binary in guardian mode.
fn default_guardian() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| "pu-engine".into())
}

struct Running {
    stop: watch::Sender<bool>,
    task: JoinHandle<std::io::Result<()>>,
}

pub struct MeshRuntime {
    lock: Arc<DaemonLock>,
    engine: std::sync::OnceLock<std::sync::Weak<crate::engine::Engine>>,
    guardian: std::sync::Mutex<std::path::PathBuf>,
    executor: Mutex<Option<Arc<Executor>>>,
    service: Arc<MeshAdminService>,
    work: Arc<WorkOwner>,
    listener: Mutex<Option<Running>>,
    status: watch::Sender<ListenerStatus>,
    host: HostClient,
}

/// The local IPC socket already admits only the owning UID; that authenticated
/// ingress is the sole source of administrative capability.
fn local_admin() -> Result<LocalAdmin, Error> {
    let caller = Caller::authenticated_local("owner").map_err(|_| Error::Unavailable)?;
    LocalAdmin::authorized_by_local_policy(&caller).map_err(|_| Error::Unavailable)
}

impl MeshRuntime {
    pub fn new(lock: Arc<DaemonLock>, work: Arc<WorkOwner>) -> Result<Arc<Self>, Error> {
        let service = MeshAdminService::new(
            lock.clone(),
            Limits::default(),
            SessionLimits::default(),
            256,
            RUNTIME_IDS
                .iter()
                .map(|id| id.to_string())
                .collect::<BTreeSet<_>>(),
        )?;
        Ok(Arc::new(Self {
            lock,
            engine: std::sync::OnceLock::new(),
            guardian: std::sync::Mutex::new(default_guardian()),
            executor: Mutex::new(None),
            service,
            work,
            listener: Mutex::new(None),
            status: watch::channel(ListenerStatus::NotInitialized).0,
            host: HostClient::new(16)?,
        }))
    }

    /// Bound once by the IPC server; the runtime never extends engine lifetime.
    pub fn bind_engine(&self, engine: std::sync::Weak<crate::engine::Engine>) {
        let _ = self.engine.set(engine);
    }

    /// Embedding/test override of the guardian binary (default: this daemon).
    pub fn set_guardian_executable(&self, path: std::path::PathBuf) {
        *self.guardian.lock().unwrap_or_else(|e| e.into_inner()) = path;
    }

    /// One executor per loaded authority. Its creation reconciles runs left
    /// Unknown by a previous daemon from their verified supervisor records.
    async fn executor(
        self: &Arc<Self>,
        authority: Arc<crate::mesh_sessions::SessionAuthority>,
    ) -> Option<Arc<Executor>> {
        let mut slot = self.executor.lock().await;
        if let Some(executor) = slot.as_ref() {
            return Some(executor.clone());
        }
        let engine = self.engine.get()?.clone();
        let outcomes = match self.lock.mesh_outcome_dir() {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(%error, "mesh run outcome directory unavailable");
                return None;
            }
        };
        let guardian = self
            .guardian
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let executor = Executor::new(
            engine,
            authority,
            self.work.clone(),
            Arc::new(Guardian::new(guardian, outcomes)),
        );
        executor.bind_self();
        executor.reconcile().await;
        *slot = Some(executor.clone());
        Some(executor)
    }

    pub fn service(&self) -> &Arc<MeshAdminService> {
        &self.service
    }

    pub fn status(&self) -> ListenerStatus {
        self.status.borrow().clone()
    }

    pub fn subscribe_status(&self) -> watch::Receiver<ListenerStatus> {
        self.status.subscribe()
    }

    /// Apply persisted configuration at daemon start. Missing identity is a
    /// normal disabled state, never an implicit initialization.
    pub async fn start(self: &Arc<Self>) {
        // Reconcile even when no listener is configured: runs outlive listeners.
        if let Ok(admin) = local_admin()
            && let Ok(authority) = self.service.current_authority(&self.work, admin).await
        {
            self.executor(authority).await;
        }
        let mut slot = self.listener.lock().await;
        self.apply(&mut slot).await;
    }

    pub async fn admin(
        self: &Arc<Self>,
        request: MeshAdminRequest,
    ) -> Result<MeshAdminResponse, Error> {
        let admin = local_admin()?;
        if !mutates(&request) {
            return Ok(self.service.handle(&self.work, admin, request).await?);
        }
        let mut slot = self.listener.lock().await;
        stop(&mut slot).await;
        self.status.send_replace(ListenerStatus::Stopped);
        // Host links were authenticated under the previous trust/routing state.
        self.host.reset();
        let result = self.service.handle(&self.work, admin, request).await;
        // Re-apply even after failure: a failed publication can leave durable
        // intent whose acknowledgement is uncertain, and the old listener is gone.
        self.apply(&mut slot).await;
        Ok(result?)
    }

    /// Forward one request to an explicitly routed, pinned worker. Only read
    /// operations are accepted on this one-shot path.
    pub async fn route(self: &Arc<Self>, route: LocalRoute) -> Result<Response, Error> {
        if !route.validate() {
            return Err(mesh_host::Error::Invalid.into());
        }
        let (target, transport) = self.routed(&route).await?;
        Ok(self
            .host
            .request(&transport, &target, &route.request)
            .await?)
    }

    /// Subscribe to a routed worker run's output over that worker's link.
    pub async fn watch(
        self: &Arc<Self>,
        route: LocalRoute,
    ) -> Result<(Response, mesh_host::Watch), Error> {
        if !route.validate() {
            return Err(mesh_host::Error::Invalid.into());
        }
        let (target, transport) = self.routed(&route).await?;
        Ok(self
            .host
            .watch(Arc::new(transport), &target, &route.request)
            .await?)
    }

    /// The route's worker from this host's own directory plus a transport whose
    /// trust set is the current active enrolled certificates.
    async fn routed(
        self: &Arc<Self>,
        route: &LocalRoute,
    ) -> Result<(mesh_host::Target, crate::mesh_transport::MeshTransport), Error> {
        let admin = local_admin()?;
        let workers = match self
            .service
            .handle(&self.work, admin.clone(), MeshAdminRequest::WorkerList {})
            .await?
        {
            MeshAdminResponse::Workers { workers } => workers,
            _ => return Err(Error::Unavailable),
        };
        let worker = workers
            .into_iter()
            .find(|worker| worker.fingerprint == route.worker)
            .ok_or(Error::UnknownWorker)?;
        if worker.daemon_id != route.request.target_daemon {
            return Err(mesh_host::Error::Invalid.into());
        }
        let authority = self
            .service
            .current_authority(&self.work, admin.clone())
            .await?;
        let transport = authority
            .transport(&self.work, admin, now())
            .await
            .map_err(|_| Error::Unavailable)?;
        let target = mesh_host::Target {
            receiver: peer_id_from_name(&worker.fingerprint).map_err(|_| Error::UnknownWorker)?,
            daemon: worker.daemon_id,
            control: worker.control_address,
            bulk: worker.bulk_address,
        };
        Ok((target, transport))
    }

    async fn apply(self: &Arc<Self>, slot: &mut Option<Running>) {
        stop(slot).await;
        let admin = match local_admin() {
            Ok(admin) => admin,
            Err(_) => {
                self.fail(ListenerFailure::Service);
                return;
            }
        };
        let snapshot = match self.service.listener_snapshot(&self.work, admin).await {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                self.status.send_replace(ListenerStatus::Disabled);
                return;
            }
            Err(mesh_admin::Error::NotInitialized) => {
                self.status.send_replace(ListenerStatus::NotInitialized);
                return;
            }
            Err(mesh_admin::Error::NoActivePeers) => {
                self.status.send_replace(ListenerStatus::NoActivePeers);
                return;
            }
            Err(_) => {
                self.fail(ListenerFailure::State);
                return;
            }
        };
        let bound = match BoundListeners::bind(&snapshot.config).await {
            Ok(Some(bound)) => bound,
            Ok(None) => {
                self.status.send_replace(ListenerStatus::Disabled);
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "mesh listener bind failed");
                self.fail(ListenerFailure::Bind);
                return;
            }
        };
        let Some(executor) = self.executor(snapshot.authority.clone()).await else {
            self.fail(ListenerFailure::Service);
            return;
        };
        let (stop_tx, stop_rx) = watch::channel(false);
        let status = self.status.clone();
        let serve = bound.run(
            snapshot.authority,
            Arc::new(snapshot.transport),
            self.work.clone(),
            stop_rx,
            executor,
        );
        self.status.send_replace(ListenerStatus::Listening {
            config: snapshot.config,
            revision: snapshot.revision,
        });
        let task = tokio::spawn(async move {
            let result = serve.await;
            // A listener that ends on its own (accept failure) must not leave a
            // stale Listening status; a requested stop returns Ok.
            if let Err(error) = &result {
                tracing::warn!(%error, "mesh listener stopped");
                status.send_replace(ListenerStatus::Failed {
                    reason: ListenerFailure::Service,
                });
            }
            result
        });
        *slot = Some(Running {
            stop: stop_tx,
            task,
        });
    }

    fn fail(&self, reason: ListenerFailure) {
        self.status.send_replace(ListenerStatus::Failed { reason });
    }
}

async fn stop(slot: &mut Option<Running>) {
    if let Some(running) = slot.take() {
        let _ = running.stop.send(true);
        // Awaiting the task drains its accepted connections before any change
        // publishes; accepted owned effects still retain their own barriers.
        let _ = running.task.await;
    }
}

fn mutates(request: &MeshAdminRequest) -> bool {
    !matches!(
        request,
        MeshAdminRequest::IdentityShow {}
            | MeshAdminRequest::PeerList { .. }
            | MeshAdminRequest::ExportList {}
            | MeshAdminRequest::ConfigShow {}
            | MeshAdminRequest::WorkerList {}
    )
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests;
