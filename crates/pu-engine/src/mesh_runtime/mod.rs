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
    mesh_host::{self, HostReader},
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

struct Running {
    stop: watch::Sender<bool>,
    task: JoinHandle<std::io::Result<()>>,
}

pub struct MeshRuntime {
    service: Arc<MeshAdminService>,
    work: Arc<WorkOwner>,
    listener: Mutex<Option<Running>>,
    status: watch::Sender<ListenerStatus>,
    host: HostReader,
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
            lock,
            Limits::default(),
            SessionLimits::default(),
            256,
            RUNTIME_IDS
                .iter()
                .map(|id| id.to_string())
                .collect::<BTreeSet<_>>(),
        )?;
        Ok(Arc::new(Self {
            service,
            work,
            listener: Mutex::new(None),
            status: watch::channel(ListenerStatus::NotInitialized).0,
            host: HostReader::new(16)?,
        }))
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
        let pin = peer_id_from_name(&worker.fingerprint).map_err(|_| Error::UnknownWorker)?;
        Ok(self
            .host
            .read(&transport, pin, worker.control_address, &route.request)
            .await?)
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
        let (stop_tx, stop_rx) = watch::channel(false);
        let status = self.status.clone();
        let serve = bound.run(
            snapshot.authority,
            Arc::new(snapshot.transport),
            self.work.clone(),
            stop_rx,
            Arc::new(crate::mesh_listener::ReadOnly),
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
