//! Opt-in authenticated session authority, with no listener or local IPC proxy.
//!
//! This owns the store and ledger: all administration must pass its exclusive
//! barrier. Dispatch closures retain shared authority in the actual blocking
//! operation, never in a cancellable waiter. Receiver resource/run/export lookup
//! and per-agent dispatch serialization remain the engine's responsibility.
use crate::{
    daemon_lifecycle::{
        mesh_store::{MeshStore, peer_name},
        owned_work::WorkOwner,
    },
    execution_state::{Caller, Ledger, LocalAdmin, PeerRecord},
    mesh_transport::{AuthenticatedConnection, MeshTransport, PeerId, Role},
};
use pu_core::mesh::{
    Action, ExportGrant, PeerPolicy,
    wire::{self, Hello, Request, Welcome},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{OwnedSemaphorePermit, RwLock, Semaphore, watch},
};

mod limits;
pub use limits::SessionLimits;
mod client;
pub use client::{BulkClient, ControlClient};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session authority denied")]
    Denied,
    #[error("session capacity exhausted")]
    Capacity,
    #[error("invalid session limits")]
    Limits,
    #[error("session closed")]
    Closed,
    #[error("session frame exceeds budget")]
    Frame,
    #[error("session operation failed")]
    Operation,
    #[error(transparent)]
    Transport(#[from] crate::mesh_transport::Error),
    #[error(transparent)]
    Wire(#[from] wire::WireError),
}
pub type Result<T> = std::result::Result<T, Error>;

struct AuthorityState {
    store: MeshStore,
    ledger: Ledger,
}
#[cfg(test)]
struct ValidationGate {
    entered: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
#[derive(Default)]
struct Counts {
    control: usize,
    bulk: usize,
    subscriptions: usize,
}
#[derive(Default)]
struct Registry {
    counts: HashMap<PeerId, Counts>,
    controls: HashMap<PeerId, Weak<Binding>>,
    subscriptions: usize,
}
struct Binding {
    peer: PeerId,
    generation: u64,
    id: String,
    closed: watch::Sender<bool>,
}
impl Binding {
    fn check(&self) -> Result<()> {
        if *self.closed.borrow() {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }
    fn close(&self) {
        self.closed.send_replace(true);
    }
    async fn closing(&self) {
        let mut rx = self.closed.subscribe();
        loop {
            if *rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}

/// No store/ledger clones or authorization snapshots escape this authority.
/// Construct in spawn_blocking after explicit local setup, never on ingress.
pub struct SessionAuthority {
    state: Arc<RwLock<AuthorityState>>,
    registry: Mutex<Registry>,
    control: Arc<Semaphore>,
    bulk: Arc<Semaphore>,
    limits: SessionLimits,
    daemon: String,
    incarnation: String,
    #[cfg(test)]
    validation_gate: Mutex<Option<ValidationGate>>,
}
impl SessionAuthority {
    pub fn new(store: MeshStore, ledger: Ledger, limits: SessionLimits) -> Result<Arc<Self>> {
        limits.validate()?;
        if store.daemon_id() != ledger.owner_daemon() {
            return Err(Error::Denied);
        }
        Ok(Arc::new(Self {
            daemon: store.daemon_id().into(),
            incarnation: pu_core::id::session_id(),
            state: Arc::new(RwLock::new(AuthorityState { store, ledger })),
            registry: Mutex::new(Registry::default()),
            control: Arc::new(Semaphore::new(limits.control_total)),
            bulk: Arc::new(Semaphore::new(limits.bulk_total)),
            limits,
            #[cfg(test)]
            validation_gate: Mutex::new(None),
        }))
    }
    pub fn daemon_id(&self) -> &str {
        &self.daemon
    }
    pub fn incarnation(&self) -> &str {
        &self.incarnation
    }
    pub fn limits(&self) -> &SessionLimits {
        &self.limits
    }

    fn admit(
        self: &Arc<Self>,
        peer: PeerId,
        role: Role,
        total: OwnedSemaphorePermit,
    ) -> Result<Lease> {
        let mut registry = self.registry.lock().map_err(|_| Error::Operation)?;
        // No await while registry is locked. Authority is always acquired first
        // where both locks are needed; Drop never acquires authority.
        let counts = registry.counts.entry(peer).or_default();
        let (count, limit) = match role {
            Role::Control => (&mut counts.control, self.limits.control_per_peer),
            Role::Bulk => (&mut counts.bulk, self.limits.bulk_per_peer),
        };
        if *count >= limit {
            return Err(Error::Capacity);
        }
        *count += 1;
        Ok(Lease {
            authority: self.clone(),
            peer,
            role,
            _total: total,
        })
    }

    /// Total role permits are reserved BEFORE TLS negotiation, with no queue.
    /// Only an independent mTLS connection can establish a bulk session.
    pub async fn accept<S>(
        self: &Arc<Self>,
        transport: &MeshTransport,
        io: S,
        role: Role,
    ) -> Result<Session<S>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let pool = match role {
            Role::Control => &self.control,
            Role::Bulk => &self.bulk,
        };
        let total = pool
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Capacity)?;
        let connection = transport.accept(io, role).await?;
        let lease = self.admit(connection.peer_id(), role, total)?;
        self.bind(connection, lease).await
    }
    async fn bind<S>(
        self: &Arc<Self>,
        mut connection: AuthenticatedConnection<S>,
        lease: Lease,
    ) -> Result<Session<S>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let hello = Hello::decode(&connection.read_frame(self.limits.read_deadline).await?)?;
        let (generation, target, prior) = match hello {
            Hello::Control {
                generation,
                target_daemon,
                ..
            } if connection.role() == Role::Control => (generation, target_daemon, None),
            Hello::Bulk {
                generation,
                target_daemon,
                incarnation,
                control_binding,
                ..
            } if connection.role() == Role::Bulk && incarnation == self.incarnation => {
                (generation, target_daemon, Some(control_binding))
            }
            _ => return Err(Error::Denied),
        };
        if target != self.daemon {
            return Err(Error::Denied);
        }
        let guard = self.state.clone().read_owned().await;
        // Admission travels with the actual validation, including a blocking
        // closure already queued/running after its transport waiter is cancelled.
        #[cfg(test)]
        let gate = self.validation_gate.lock().unwrap().take();
        let (mut connection, guard, lease) = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.entered.send(()).ok();
                gate.release.recv().ok();
            }
            guard
                .store
                .connection_policy(&guard.ledger, &connection, generation, now())
                .map_err(|_| Error::Denied)?;
            Ok::<_, Error>((connection, guard, lease))
        })
        .await
        .map_err(|_| Error::Operation)??;
        let binding = {
            let mut registry = self.registry.lock().map_err(|_| Error::Operation)?;
            if let Some(id) = prior {
                let binding = registry
                    .controls
                    .get(&connection.peer_id())
                    .and_then(Weak::upgrade)
                    .ok_or(Error::Denied)?;
                binding.check()?;
                if binding.id != id
                    || binding.peer != connection.peer_id()
                    || binding.generation != generation
                {
                    return Err(Error::Denied);
                }
                binding
            } else {
                let binding = Arc::new(Binding {
                    peer: connection.peer_id(),
                    generation,
                    id: pu_core::id::session_id(),
                    closed: watch::channel(false).0,
                });
                if let Some(old) = registry
                    .controls
                    .insert(binding.peer, Arc::downgrade(&binding))
                    .and_then(|b| b.upgrade())
                {
                    old.close();
                }
                binding
            }
        };
        let welcome = Welcome {
            version: wire::BRIDGE_VERSION,
            daemon_id: self.daemon.clone(),
            generation,
            incarnation: self.incarnation.clone(),
            control_binding: binding.id.clone(),
        };
        let mut session = Session {
            connection: None,
            authority: self.clone(),
            role: lease.role,
            lease: Some(Arc::new(lease)),
            binding,
            welcome,
        };
        let bytes = serde_json::to_vec(&session.welcome).map_err(|_| Error::Operation)?;
        let result = tokio::select! {
            _ = session.binding.closing() => Err(Error::Closed),
            result = connection.write_frame(&bytes, self.limits.send_deadline) => result.map_err(Error::from),
        };
        drop(guard);
        result?;
        session.connection = Some(connection);
        Ok(session)
    }

    /// Trusted Rust embedding hook; never invoke from a remote admin request.
    /// Admin closures execute on a blocking owner retaining the EXCLUSIVE guard.
    /// Dropping this async waiter cannot cancel a started durable change or wake
    /// streams before that change has actually finished. No network wait in SQL.
    pub async fn administer<T, F>(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
        peer: PeerId,
        change: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut MeshStore, &Ledger, &LocalAdmin) -> Result<T> + Send + 'static,
    {
        let authority = self.clone();
        let blocking_work = work.clone();
        work.spawn(async move {
            let guard = authority.state.clone().write_owned().await;
            tokio::task::spawn_blocking(move || {
                let _work_owner = blocking_work;
                let mut guard = guard;
                let closing = AdminFence { authority, peer };
                let AuthorityState { store, ledger } = &mut *guard;
                let result = change(store, ledger, &admin);
                // Even a failed/torn policy change closes sessions. Reconnect
                // checks canonical bytes and durable anchor again.
                drop(closing);
                result
            })
            .await
            .map_err(|_| Error::Operation)?
        })
        .map_err(|_| Error::Closed)?
        .await
        .map_err(|_| Error::Operation)?
    }
    /// Durably revoke a peer under exclusive authority and wake its sessions.
    /// Running worker execution/reservations are preserved by the ledger fence.
    pub async fn revoke(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
        peer: PeerId,
    ) -> Result<PeerRecord> {
        self.administer(work, admin, peer, move |_, ledger, admin| {
            ledger
                .revoke_peer(admin, &peer_name(peer))
                .map_err(|_| Error::Operation)
        })
        .await
    }
    /// Explicit local policy publication, with durable generation/anchor fencing.
    pub async fn update_policy(
        self: &Arc<Self>,
        work: &Arc<WorkOwner>,
        admin: LocalAdmin,
        certificate: Vec<u8>,
        generation: u64,
        exports: Vec<ExportGrant>,
        expires_at: Option<i64>,
    ) -> Result<PeerRecord> {
        let peer = PeerId::from_certificate_der(&certificate)?;
        self.administer(work, admin, peer, move |store, ledger, admin| {
            store
                .update_policy(ledger, admin, &certificate, generation, exports, expires_at)
                .map_err(|_| Error::Operation)
        })
        .await
    }
    fn close_peer(&self, peer: PeerId) {
        if let Ok(mut registry) = self.registry.lock()
            && let Some(binding) = registry.controls.remove(&peer).and_then(|b| b.upgrade())
        {
            binding.close();
        }
    }
}
struct AdminFence {
    authority: Arc<SessionAuthority>,
    peer: PeerId,
}
impl Drop for AdminFence {
    fn drop(&mut self) {
        self.authority.close_peer(self.peer);
    }
}
struct CancelFence {
    binding: Arc<Binding>,
    armed: bool,
}
impl CancelFence {
    fn new(binding: Arc<Binding>) -> Self {
        Self {
            binding,
            armed: true,
        }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for CancelFence {
    fn drop(&mut self) {
        if self.armed {
            self.binding.close();
        }
    }
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
struct Lease {
    authority: Arc<SessionAuthority>,
    peer: PeerId,
    role: Role,
    _total: OwnedSemaphorePermit,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.authority.registry.lock()
            && let Some(counts) = registry.counts.get_mut(&self.peer)
        {
            match self.role {
                Role::Control => counts.control -= 1,
                Role::Bulk => counts.bulk -= 1,
            }
            if counts.control + counts.bulk + counts.subscriptions == 0 {
                registry.counts.remove(&self.peer);
                registry.controls.remove(&self.peer);
            }
        }
    }
}

/// Owns one live mTLS stream and its bounded admission. No raw stream escapes.
/// Dropping/closing control invalidates all its bulk streams and wakes readers.
pub struct Session<S> {
    connection: Option<AuthenticatedConnection<S>>,
    authority: Arc<SessionAuthority>,
    role: Role,
    lease: Option<Arc<Lease>>,
    binding: Arc<Binding>,
    welcome: Welcome,
}
impl<S> Drop for Session<S> {
    fn drop(&mut self) {
        if self.role == Role::Control {
            self.binding.close();
        }
    }
}
impl<S> Session<S> {
    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }
    pub fn close(&mut self) {
        self.connection.take();
        self.lease.take();
        if self.role == Role::Control {
            self.binding.close();
        }
    }
    pub fn role(&self) -> Role {
        self.role
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Session<S> {
    /// Idle reads hold NO authority. A frame is untrusted until dispatch below;
    /// callers cannot obtain a purportedly authorized Request snapshot.
    async fn read_request(&mut self) -> Result<Request> {
        if let Err(error) = self.binding.check() {
            self.close();
            return Err(error);
        }
        if self.role() != Role::Control {
            return Err(Error::Denied);
        }
        let connection = self.connection.as_mut().ok_or(Error::Closed)?;
        let result = tokio::select! {
            _ = self.binding.closing() => Err(Error::Closed),
            bytes = connection.read_frame(self.authority.limits.read_deadline) => {
                let bytes = match bytes { Ok(bytes) => bytes, Err(error) => { self.close(); return Err(error.into()); } };
                if bytes.len() > self.authority.limits.control_frame { Err(Error::Frame) } else { Ok(Request::decode(&bytes)?) }
            }
        };
        if result.is_err() {
            self.close();
        }
        result
    }

    /// Trusted Rust dispatcher embedding hook.
    /// One owned effect at a time per connection. Closure performs exact receiver
    /// resource/generation/run checks and synchronous ledger work, then returns a
    /// bounded response. Do NOT wait for network/PTY inside ledger transactions.
    /// Per-agent epochs/PTY dispatch require the parent's stronger boundary.
    pub async fn dispatch<F>(&mut self, work: &Arc<WorkOwner>, effect: F) -> Result<()>
    where
        F: FnOnce(&Ledger, &Caller, &PeerPolicy, &Request) -> Result<Vec<u8>> + Send + 'static,
    {
        let mut fence = CancelFence::new(self.binding.clone());
        let request = match self.read_request().await {
            Ok(request) => request,
            Err(error) => {
                self.close();
                return Err(error);
            }
        };
        if request.target_daemon != self.welcome.daemon_id {
            self.close();
            return Err(Error::Denied);
        }
        let connection = self.connection.take().ok_or(Error::Closed)?;
        let authority = self.authority.clone();
        let binding = self.binding.clone();
        let lease = self.lease.clone().ok_or(Error::Closed)?;
        let blocking_work = work.clone();
        let handle = work
            .spawn(async move {
                let send_deadline = authority.limits.send_deadline;
                let _lease = lease;
                let sending_binding = binding.clone();
                let guard = authority.state.clone().read_owned().await;
                let (mut connection, guard, response) = tokio::task::spawn_blocking(move || {
                    let _work_owner = blocking_work;
                    binding.check()?;
                    let policy = guard
                        .store
                        .connection_policy(&guard.ledger, &connection, binding.generation, now())
                        .map_err(|_| Error::Denied)?;
                    let caller = authorize_request(&guard, &connection, &binding, &request)?;
                    let response = effect(&guard.ledger, &caller, &policy, &request)?;
                    // Time may have advanced during the effect. Recheck before send.
                    binding.check()?;
                    guard
                        .store
                        .connection_policy(&guard.ledger, &connection, binding.generation, now())
                        .map_err(|_| Error::Denied)?;
                    if response.is_empty() || response.len() > authority.limits.control_frame {
                        return Err(Error::Frame);
                    }
                    Ok::<_, Error>((connection, guard, response))
                })
                .await
                .map_err(|_| Error::Operation)??;
                tokio::select! {
                    _ = sending_binding.closing() => return Err(Error::Closed),
                    result = connection.write_frame(&response, send_deadline) => result?,
                }
                drop(guard);
                Ok(connection)
            })
            .map_err(|_| Error::Closed)?;
        let result = handle.await.map_err(|_| Error::Operation)?;
        match result {
            Ok(connection) => {
                self.connection = Some(connection);
                fence.disarm();
                Ok(())
            }
            Err(error) => {
                self.close();
                Err(error)
            }
        }
    }

    /// Fresh live authorization and retained barrier through the bounded send.
    /// Subscription is a resource permit only: it never caches authorization.
    pub async fn send_output(
        &mut self,
        subscription: &Subscription,
        raw_chunk: &[u8],
        encoded_frame: &[u8],
    ) -> Result<()> {
        if raw_chunk.len() > self.authority.limits.output_chunk
            || encoded_frame.is_empty()
            || encoded_frame.len() > self.authority.limits.bulk_frame
        {
            return Err(Error::Frame);
        }
        let raw_len = raw_chunk.len();
        let frame = encoded_frame.to_vec();
        self.send_output_with(subscription, move |_, _| Ok((raw_len, frame)))
            .await
    }

    /// Parent integration path: exact export generation/run ownership lookup and
    /// bounded output copy happen INSIDE the retained shared barrier, on a blocking
    /// owner. Return (raw byte count, encoded frame). No authority snapshot escapes.
    /// Producer must bound its own allocation before returning; no PTY/network
    /// wait is allowed inside a ledger transaction. This is observation only.
    pub async fn send_output_with<F>(
        &mut self,
        subscription: &Subscription,
        producer: F,
    ) -> Result<()>
    where
        F: FnOnce(&Ledger, &Caller) -> Result<(usize, Vec<u8>)> + Send + 'static,
    {
        if self.role() != Role::Bulk || !Arc::ptr_eq(&self.binding, &subscription.binding) {
            return Err(Error::Denied);
        }
        let authority = self.authority.clone();
        let connection = self.connection.take().ok_or(Error::Closed)?;
        let binding = self.binding.clone();
        let export = subscription.export.clone();
        let lease = self.lease.clone().ok_or(Error::Closed)?;
        let guard = authority.state.clone().read_owned().await;
        let limits = authority.limits.clone();
        let checked = tokio::task::spawn_blocking(move || {
            binding.check()?;
            let caller = guard
                .store
                .authorize_connection(
                    &guard.ledger,
                    &connection,
                    binding.generation,
                    &export,
                    &Action::ReadOutput,
                    now(),
                )
                .map_err(|_| Error::Denied)?;
            let (raw_len, frame) = producer(&guard.ledger, &caller)?;
            if raw_len > limits.output_chunk || frame.is_empty() || frame.len() > limits.bulk_frame
            {
                return Err(Error::Frame);
            }
            binding.check()?;
            guard
                .store
                .authorize_connection(
                    &guard.ledger,
                    &connection,
                    binding.generation,
                    &export,
                    &Action::ReadOutput,
                    now(),
                )
                .map_err(|_| Error::Denied)?;
            Ok::<_, Error>((connection, guard, lease, frame))
        })
        .await
        .map_err(|_| Error::Operation)?;
        let (mut connection, guard, _lease, frame) = match checked {
            Ok(value) => value,
            Err(error) => {
                self.close();
                return Err(error);
            }
        };
        let result = tokio::select! {
            _ = self.binding.closing() => Err(Error::Closed),
            result = connection.write_frame(&frame, authority.limits.send_deadline) => result.map_err(Error::from),
        };
        drop(guard);
        match result {
            Ok(()) => {
                self.connection = Some(connection);
                Ok(())
            }
            Err(error) => {
                self.close();
                Err(error)
            }
        }
    }
    pub async fn subscribe(&mut self, export: String) -> Result<Subscription> {
        if self.role() != Role::Bulk {
            return Err(Error::Denied);
        }
        let connection = self.connection.take().ok_or(Error::Closed)?;
        let binding = self.binding.clone();
        let authority = self.authority.clone();
        let lease = self.lease.clone().ok_or(Error::Closed)?;
        let guard = authority.state.clone().read_owned().await;
        let result = tokio::task::spawn_blocking(move || {
            let _lease = lease;
            binding.check()?;
            guard
                .store
                .authorize_connection(
                    &guard.ledger,
                    &connection,
                    binding.generation,
                    &export,
                    &Action::ReadOutput,
                    now(),
                )
                .map_err(|_| Error::Denied)?;
            let mut registry = authority.registry.lock().map_err(|_| Error::Operation)?;
            if registry.subscriptions >= authority.limits.subscriptions_total
                || registry
                    .counts
                    .get(&binding.peer)
                    .map_or(0, |c| c.subscriptions)
                    >= authority.limits.subscriptions_per_peer
            {
                return Err(Error::Capacity);
            }
            registry.subscriptions += 1;
            registry
                .counts
                .entry(binding.peer)
                .or_default()
                .subscriptions += 1;
            drop(registry);
            let subscription = Subscription {
                authority,
                binding,
                export,
            };
            drop(guard);
            Ok::<_, Error>((connection, subscription))
        })
        .await
        .map_err(|_| Error::Operation)?;
        match result {
            Ok((connection, subscription)) => {
                self.connection = Some(connection);
                Ok(subscription)
            }
            Err(error) => {
                self.close();
                Err(error)
            }
        }
    }
    /// Bulk input is not a dispatch surface. This bounded read only detects idle
    /// disconnects; it is woken on control close/revocation and rejects all data.
    pub async fn wait_bulk_close(&mut self) -> Result<()> {
        if self.role() != Role::Bulk {
            return Err(Error::Denied);
        }
        let connection = self.connection.as_mut().ok_or(Error::Closed)?;
        tokio::select! {
            _ = self.binding.closing() => {},
            _ = connection.read_frame(self.authority.limits.read_deadline) => {},
        }
        self.close();
        Err(Error::Closed)
    }
}

/// Bounded observation lease, never permission for a future send.
pub struct Subscription {
    authority: Arc<SessionAuthority>,
    binding: Arc<Binding>,
    export: String,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.authority.registry.lock() {
            registry.subscriptions -= 1;
            if let Some(counts) = registry.counts.get_mut(&self.binding.peer) {
                counts.subscriptions -= 1;
                if counts.control + counts.bulk + counts.subscriptions == 0 {
                    registry.counts.remove(&self.binding.peer);
                    registry.controls.remove(&self.binding.peer);
                }
            }
        }
    }
}
fn authorize_request<S>(
    state: &AuthorityState,
    connection: &AuthenticatedConnection<S>,
    binding: &Binding,
    request: &Request,
) -> Result<Caller> {
    use wire::Operation::*;
    let selection = match &request.operation {
        Inventory {} | IssueWindow { .. } | CloseWindow { .. } => None,
        Status { export, .. } | Receipt { export, .. } => Some((&export.id, Action::Inspect)),
        Spawn {
            export, runtime, ..
        } => Some((&export.id, Action::Spawn { runtime })),
        AcquireControl { guard, .. } | OpenWriter { guard, .. } => {
            Some((&guard.target.export.id, Action::AcquireControl))
        }
        Stop { guard, .. } => Some((&guard.target.export.id, Action::Stop)),
        Suspend { guard, .. } => Some((&guard.target.export.id, Action::Suspend)),
        Resume { guard, runtime, .. } => {
            Some((&guard.target.export.id, Action::Resume { runtime }))
        }
        Input { guard, .. } => Some((&guard.target.export.id, Action::Input)),
        Resize { guard, .. } => Some((&guard.target.export.id, Action::Resize)),
        InputReceipt { target, .. } => Some((&target.export.id, Action::Inspect)),
        SubscribeOutput { target, .. } | ReadConversation { target, .. } => {
            Some((&target.export.id, Action::ReadOutput))
        }
    };
    if let Some((export, action)) = selection {
        state
            .store
            .authorize_connection(
                &state.ledger,
                connection,
                binding.generation,
                export,
                &action,
                now(),
            )
            .map_err(|_| Error::Denied)
    } else {
        state
            .store
            .connection_policy(&state.ledger, connection, binding.generation, now())
            .map_err(|_| Error::Denied)?;
        let caller =
            Caller::authenticated_peer(peer_name(connection.peer_id()), binding.generation)
                .map_err(|_| Error::Denied)?;
        state
            .ledger
            .validate_principal(&caller)
            .map_err(|_| Error::Denied)?;
        Ok(caller)
    }
}
#[cfg(test)]
mod tests;
