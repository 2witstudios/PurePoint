//! Explicit owner-local mesh administration. No listener, invitation exchange,
//! bridge proxy, worktree creation or native execution is enabled by this module.
//! Configuration responses describe durable intent, never a successful socket bind.
mod persistence;
use super::{
    DaemonLock,
    mesh_exports::{ExportRecord, MeshExports},
    mesh_store::{MeshStore, peer_id_from_name, peer_name},
    owned_work::WorkOwner,
};
use crate::execution_state::{Ledger, Limits, LocalAdmin, PeerRecord};
use crate::mesh_sessions::{Error as SessionError, SessionAuthority, SessionLimits};
use crate::mesh_transport::{MeshTransport, PeerId};
use persistence::*;
use pu_core::mesh::admin::{
    ExportStatus, ListenConfig, MeshAdminRequest, MeshAdminResponse, PeerChange, PeerStatus,
    PublicIdentity,
};
use pu_core::mesh::{ExportGrant, PeerPolicy, valid_identifier};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mesh state is not initialized; explicit local identity initialization required")]
    NotInitialized,
    #[error("retained mesh state exists; refusing identity replacement or automated migration")]
    AlreadyInitialized,
    #[error("invalid mesh administration request")]
    InvalidRequest,
    #[error("certificate does not match independently supplied fingerprint")]
    FingerprintMismatch,
    #[error("mesh private state rejected: {0}; explicit local recovery required")]
    State(&'static str),
    #[error("mesh administration work is unavailable")]
    Unavailable,
    #[error("mesh authority rejected administration")]
    Authority,
}
pub type Result<T> = std::result::Result<T, Error>;

struct State {
    authority: Arc<SessionAuthority>,
    config: ConfigRecord,
    public_identity: PublicIdentity,
}

/// Root supplies the retained singleton and receiver-approved runtime IDs. Creation
/// is inert: existing state loads only on explicit local requests or listener setup.
pub struct MeshAdminService {
    lock: Arc<DaemonLock>,
    limits: Limits,
    sessions: SessionLimits,
    export_limit: usize,
    runtime_ids: BTreeSet<String>,
    state: Mutex<Option<State>>,
    revisions: watch::Sender<u64>,
}

/// Transport setup only, never an authorization snapshot. Parent must apply both
/// listeners atomically and report bind failures; sessions/events recheck authority.
pub struct ListenerSnapshot {
    pub authority: Arc<SessionAuthority>,
    pub transport: MeshTransport,
    pub config: ListenConfig,
    pub revision: u64,
}

impl MeshAdminService {
    pub fn new(
        lock: Arc<DaemonLock>,
        limits: Limits,
        sessions: SessionLimits,
        export_limit: usize,
        runtime_ids: BTreeSet<String>,
    ) -> Result<Arc<Self>> {
        if limits.peers > 256
            || export_limit == 0
            || export_limit > 1024
            || runtime_ids.len() > 32
            || runtime_ids.iter().any(|id| !valid_identifier(id))
            || sessions.validate().is_err()
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Arc::new(Self {
            lock,
            limits,
            sessions,
            export_limit,
            runtime_ids,
            state: Mutex::new(None),
            revisions: watch::channel(0).0,
        }))
    }
    /// Rebuild transport/listener setup after every change notification, even when
    /// failed publication leaves the previous numeric revision. Never cache grants.
    pub fn subscribe_revision(&self) -> watch::Receiver<u64> {
        self.revisions.subscribe()
    }
    fn notify(&self, revision: u64) {
        self.revisions.send_modify(|current| *current = revision);
    }

    fn load(&self, admin: &LocalAdmin, initialize: bool) -> Result<State> {
        self.lock
            .verify_lock()
            .map_err(|_| Error::State("private directory or singleton lock"))?;
        let store = if initialize {
            require_fresh(&self.lock)?;
            MeshStore::initialize(self.lock.clone(), admin)
                .map_err(|_| Error::State("identity initialization"))?
        } else {
            if self
                .lock
                .directory
                .entry(&name(&self.lock, ".mesh-identity.json"))
                .map_err(|_| Error::State("identity"))?
                .is_none()
            {
                return Err(Error::NotInitialized);
            }
            require_file(&self.lock, ".mesh-ledger.sqlite")?;
            require_file(&self.lock, ".mesh-listen.json")?;
            MeshStore::open(self.lock.clone()).map_err(|_| Error::State("existing identity"))?
        };
        let config = if initialize {
            let config = ConfigRecord {
                schema: 1,
                owner_daemon: store.daemon_id().into(),
                revision: 1,
                config: ListenConfig::default(),
            };
            write_config(&self.lock, &config, true)?;
            config
        } else {
            read_config(&self.lock, store.daemon_id())?
        };
        let path = self
            .lock
            .directory
            .checked_path_for(&name(&self.lock, ".mesh-ledger.sqlite"))
            .map_err(|_| Error::State("ledger path"))?;
        let ledger = Ledger::open(path, store.daemon_id(), self.limits.clone())
            .map_err(|_| Error::State("existing execution ledger"))?;
        let exports = MeshExports::open(self.lock.clone(), store.daemon_id(), self.export_limit)
            .map_err(|_| Error::State("export records"))?;
        self.lock
            .verify_lock()
            .map_err(|_| Error::State("private directory or singleton lock"))?;
        let public_identity = PublicIdentity {
            daemon_id: store.daemon_id().into(),
            certificate_der: store.identity().certificate_der().to_vec(),
            fingerprint: peer_name(store.identity().peer_id()),
        };
        let authority = SessionAuthority::new(store, ledger, exports, self.sessions.clone())
            .map_err(|_| Error::Authority)?;
        Ok(State {
            authority,
            config,
            public_identity,
        })
    }
    async fn ensure_loaded(
        self: &Arc<Self>,
        state: &mut Option<State>,
        admin: &LocalAdmin,
        initialize: bool,
    ) -> Result<()> {
        if state.is_some() && initialize {
            return Err(Error::AlreadyInitialized);
        }
        if state.is_none() {
            let service = self.clone();
            let admin = admin.clone();
            let loaded = tokio::task::spawn_blocking(move || service.load(&admin, initialize))
                .await
                .map_err(|_| Error::Unavailable)??;
            self.notify(loaded.config.revision);
            *state = Some(loaded);
        }
        Ok(())
    }
    async fn check_config(self: &Arc<Self>, state: &State) -> Result<()> {
        let service = self.clone();
        let expected = state.config.clone();
        let identity = state.public_identity.clone();
        tokio::task::spawn_blocking(move || {
            require_file(&service.lock, ".mesh-identity.json")?;
            let persisted = MeshStore::open(service.lock.clone())
                .map_err(|_| Error::State("existing identity"))?;
            if persisted.daemon_id() != identity.daemon_id
                || persisted.identity().certificate_der() != identity.certificate_der
            {
                return Err(Error::State("existing identity was replaced"));
            }
            require_file(&service.lock, ".mesh-ledger.sqlite")?;
            let fresh = read_config(&service.lock, &expected.owner_daemon)?;
            if fresh != expected {
                return Err(Error::State(
                    "configuration changed outside local administration",
                ));
            }
            Ok(())
        })
        .await
        .map_err(|_| Error::Unavailable)?
    }
    /// Local ingress must derive LocalAdmin from authenticated owner UID/policy.
    /// WorkOwner retains accepted work and its lock through cancellation/drain.
    pub async fn handle(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
        request: MeshAdminRequest,
    ) -> Result<MeshAdminResponse> {
        if !request.validate() {
            return Err(Error::InvalidRequest);
        }
        let service = self.clone();
        let work_owned = work.clone();
        work.spawn(async move {
            let result = service.handle_owned(&work_owned, admin, request).await;
            if result.is_err() {
                let revision = *service.revisions.borrow();
                service.notify(revision);
            }
            result
        })
        .map_err(|_| Error::Unavailable)?
        .await
        .map_err(|_| Error::Unavailable)?
    }
    async fn handle_owned(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
        request: MeshAdminRequest,
    ) -> Result<MeshAdminResponse> {
        let initialize = matches!(request, MeshAdminRequest::IdentityInitialize {});
        let mut slot = self.state.lock().await;
        self.ensure_loaded(&mut slot, &admin, initialize).await?;
        let state = slot.as_mut().ok_or(Error::NotInitialized)?;
        self.check_config(state).await?;
        let authority = state.authority.clone();
        let is_config_set = matches!(&request, MeshAdminRequest::ConfigSet { .. });
        let mut change_config = None;
        let changes = matches!(
            request,
            MeshAdminRequest::EnrollManual { .. }
                | MeshAdminRequest::PolicyUpdate { .. }
                | MeshAdminRequest::PeerChange { .. }
                | MeshAdminRequest::ExportRegister { .. }
                | MeshAdminRequest::ExportRevoke { .. }
                | MeshAdminRequest::ConfigSet { .. }
        );
        let response = match request {
            MeshAdminRequest::IdentityInitialize {} | MeshAdminRequest::IdentityShow {} => {
                let identity = authority
                    .identity(work, admin.clone())
                    .await
                    .map_err(|_| Error::Authority)?;
                MeshAdminResponse::Identity {
                    identity: PublicIdentity {
                        daemon_id: identity.daemon_id,
                        certificate_der: identity.certificate_der,
                        fingerprint: peer_name(identity.peer_id),
                    },
                }
            }
            MeshAdminRequest::ConfigShow {} => MeshAdminResponse::Config {
                config: state.config.config.clone(),
                revision: state.config.revision,
            },
            MeshAdminRequest::ConfigSet { config } => {
                change_config = Some(config);
                MeshAdminResponse::Done {}
            }
            MeshAdminRequest::ExportList {} => {
                let exports = authority
                    .inspect_admin(work, admin.clone(), |_, _, exports, admin| {
                        exports.list(admin).map_err(|_| SessionError::Operation)
                    })
                    .await
                    .map_err(|_| Error::State("export catalog"))?;
                MeshAdminResponse::Exports {
                    exports: exports.into_iter().map(export_status).collect(),
                }
            }
            MeshAdminRequest::ExportRegister {
                project_root,
                label,
            } => MeshAdminResponse::Export {
                export: export_status(
                    authority
                        .register_export(work, admin.clone(), PathBuf::from(project_root), label)
                        .await
                        .map_err(|_| Error::Authority)?,
                ),
            },
            MeshAdminRequest::ExportRevoke { export_id } => {
                authority
                    .revoke_export(work, admin.clone(), export_id)
                    .await
                    .map_err(|_| Error::Authority)?;
                MeshAdminResponse::Done {}
            }
            MeshAdminRequest::PeerList { after, limit } => {
                let (peers, next_after) = authority
                    .inspect_admin(work, admin.clone(), move |store, ledger, _, admin| {
                        let mut peers = Vec::new();
                        let mut cursor = after;
                        let mut bytes = 0usize;
                        // Keep retained policy DTOs below the local IPC response budget;
                        // never allocate a 256-policy vector before applying that bound.
                        for _ in 0..limit {
                            let page = store
                                .list_admin_peers(ledger, admin, cursor.as_deref(), 1)
                                .map_err(|_| SessionError::Operation)?;
                            let Some((record, policy, _certificate)) = page.into_iter().next()
                            else {
                                return Ok((peers, None));
                            };
                            let peer = peer_status(record, policy);
                            if cursor
                                .as_ref()
                                .is_some_and(|last| peer.fingerprint <= *last)
                            {
                                return Err(SessionError::Operation);
                            }
                            let size = serde_json::to_vec(&peer)
                                .map_err(|_| SessionError::Operation)?
                                .len();
                            if bytes + size > 512 * 1024 {
                                if peers.is_empty() {
                                    return Err(SessionError::Frame);
                                }
                                return Ok((peers, cursor));
                            }
                            bytes += size;
                            cursor = Some(peer.fingerprint.clone());
                            peers.push(peer);
                        }
                        Ok((peers, cursor))
                    })
                    .await
                    .map_err(|_| Error::State("canonical peer policies and ledger anchors"))?;
                MeshAdminResponse::Peers { peers, next_after }
            }
            MeshAdminRequest::EnrollManual {
                certificate_der,
                expected_fingerprint,
                grants,
                expires_at,
            } => {
                let peer = checked_peer(&certificate_der, &expected_fingerprint)?;
                self.check_grants(work, &admin, &authority, &grants).await?;
                let status = authority
                    .administer(work, admin.clone(), peer, move |store, ledger, admin| {
                        let record = store
                            .enroll(ledger, admin, &certificate_der, grants, expires_at)
                            .map_err(|_| SessionError::Operation)?;
                        let (_, policy, _) = store
                            .admin_peer(ledger, admin, &record.id)
                            .map_err(|_| SessionError::Operation)?;
                        Ok(peer_status(record, policy))
                    })
                    .await
                    .map_err(|_| Error::Authority)?;
                MeshAdminResponse::Peer { peer: status }
            }
            MeshAdminRequest::PolicyUpdate {
                certificate_der,
                expected_fingerprint,
                expected_generation,
                grants,
                expires_at,
            } => {
                let peer = checked_peer(&certificate_der, &expected_fingerprint)?;
                self.check_grants(work, &admin, &authority, &grants).await?;
                let status = authority
                    .administer(work, admin.clone(), peer, move |store, ledger, admin| {
                        let record = store
                            .update_policy(
                                ledger,
                                admin,
                                &certificate_der,
                                expected_generation,
                                grants,
                                expires_at,
                            )
                            .map_err(|_| SessionError::Operation)?;
                        let (_, policy, _) = store
                            .admin_peer(ledger, admin, &record.id)
                            .map_err(|_| SessionError::Operation)?;
                        Ok(peer_status(record, policy))
                    })
                    .await
                    .map_err(|_| Error::Authority)?;
                MeshAdminResponse::Peer { peer: status }
            }
            MeshAdminRequest::PeerChange {
                fingerprint,
                expected_generation,
                change,
            } => {
                let peer = peer_id_from_name(&fingerprint).map_err(|_| Error::InvalidRequest)?;
                authority
                    .administer(work, admin.clone(), peer, move |store, ledger, admin| {
                        let record = ledger
                            .peer(&fingerprint)
                            .map_err(|_| SessionError::Operation)?;
                        if record.generation != expected_generation {
                            return Err(SessionError::Denied);
                        }
                        // Revoke must still work when public policy bytes are damaged.
                        // Regrant/advance require checked policy, so cannot repair trust
                        // or silently re-enable unanchored records.
                        if change != PeerChange::Revoke {
                            store
                                .admin_peer(ledger, admin, &fingerprint)
                                .map_err(|_| SessionError::Operation)?;
                        }
                        match change {
                            PeerChange::Revoke => ledger.revoke_peer(admin, &fingerprint),
                            PeerChange::Regrant => {
                                ledger.regrant_peer(admin, &fingerprint, expected_generation)
                            }
                            PeerChange::AdvanceGeneration => ledger.advance_peer_generation(
                                admin,
                                &fingerprint,
                                expected_generation,
                            ),
                        }
                        .map_err(|_| SessionError::Operation)?;
                        Ok(())
                    })
                    .await
                    .map_err(|_| Error::Authority)?;
                MeshAdminResponse::Done {}
            }
        };
        if changes {
            let mut next = state.config.clone();
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or(Error::State("configuration revision exhausted"))?;
            if let Some(config) = change_config {
                next.config = config;
            }
            let lock = self.lock.clone();
            let persisted = next.clone();
            authority
                .administer_exports(work, admin, move |_, _| {
                    write_config(&lock, &persisted, false).map_err(|_| SessionError::Operation)
                })
                .await
                .map_err(|_| Error::State("configuration publication"))?;
            state.config = next;
            self.notify(state.config.revision);
        }
        if is_config_set {
            return Ok(MeshAdminResponse::Config {
                config: state.config.config.clone(),
                revision: state.config.revision,
            });
        }
        Ok(response)
    }
    async fn check_grants(
        &self,
        work: &Arc<WorkOwner>,
        admin: &LocalAdmin,
        authority: &Arc<SessionAuthority>,
        grants: &[ExportGrant],
    ) -> Result<()> {
        if grants.iter().any(|grant| {
            grant
                .runtimes
                .iter()
                .any(|runtime| !self.runtime_ids.contains(runtime))
        }) {
            return Err(Error::InvalidRequest);
        }
        let grants = grants.to_vec();
        authority
            .inspect_admin(work, admin.clone(), move |_, _, exports, admin| {
                let records = exports.list(admin).map_err(|_| SessionError::Operation)?;
                for grant in grants {
                    let record = records
                        .iter()
                        .find(|record| record.id == grant.export_id && !record.revoked)
                        .ok_or(SessionError::Denied)?;
                    exports
                        .resolve(&record.id, record.generation)
                        .map_err(|_| SessionError::Denied)?;
                }
                Ok(())
            })
            .await
            .map_err(|_| Error::Authority)
    }
    /// Trusted local embedding handle, not a detached policy or grant snapshot.
    /// Callers still use the authority's guarded hooks for every effect/observation.
    pub async fn current_authority(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
    ) -> Result<Arc<SessionAuthority>> {
        let service = self.clone();
        work.spawn(async move {
            let mut slot = service.state.lock().await;
            service.ensure_loaded(&mut slot, &admin, false).await?;
            let state = slot.as_ref().ok_or(Error::NotInitialized)?;
            service.check_config(state).await?;
            Ok(state.authority.clone())
        })
        .map_err(|_| Error::Unavailable)?
        .await
        .map_err(|_| Error::Unavailable)?
    }

    /// None is disabled. No missing-state initialization, key replacement or bind
    /// occurs here. Root must react to errors by closing affected listener service.
    pub async fn listener_snapshot(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
    ) -> Result<Option<ListenerSnapshot>> {
        let service = self.clone();
        let owned_work = work.clone();
        work.spawn(async move {
            let mut slot = service.state.lock().await;
            service.ensure_loaded(&mut slot, &admin, false).await?;
            let state = slot.as_ref().ok_or(Error::NotInitialized)?;
            service.check_config(state).await?;
            if state.config.config.bind_address.is_none() {
                return Ok(None);
            }
            let transport = state
                .authority
                .transport(&owned_work, admin, now())
                .await
                .map_err(|_| Error::State("checked transport certificate set"))?;
            Ok(Some(ListenerSnapshot {
                authority: state.authority.clone(),
                transport,
                config: state.config.config.clone(),
                revision: state.config.revision,
            }))
        })
        .map_err(|_| Error::Unavailable)?
        .await
        .map_err(|_| Error::Unavailable)?
    }
}
fn checked_peer(certificate: &[u8], pin: &str) -> Result<PeerId> {
    let peer = PeerId::from_certificate_der(certificate).map_err(|_| Error::InvalidRequest)?;
    if peer_name(peer) != pin {
        return Err(Error::FingerprintMismatch);
    }
    Ok(peer)
}
fn peer_status(record: PeerRecord, policy: PeerPolicy) -> PeerStatus {
    PeerStatus {
        fingerprint: record.id,
        generation: record.generation,
        revoked: record.revoked,
        authorization_revision: record.authorization_revision,
        policy,
    }
}
fn export_status(record: ExportRecord) -> ExportStatus {
    ExportStatus {
        id: record.id,
        generation: record.generation,
        label: record.label,
        revoked: record.revoked,
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}
#[cfg(test)]
mod tests;
