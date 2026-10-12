//! Bounded host-side bridge client. Addresses are routing hints; the configured
//! exact certificate and daemon identity must both match. No automatic retries.

use crate::{
    mesh_sessions::{self, ControlClient, SessionLimits},
    mesh_transport::{self, MeshTransport, PeerId},
};
use pu_core::mesh::{
    response::{Body, OutputFrame, Response},
    wire::{AgentTarget, Operation, OutputCursor, Request, WireError},
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    sync::{Semaphore, mpsc},
    time::timeout,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("host read capacity exhausted")]
    Capacity,
    #[error("invalid explicit mesh target or request")]
    Invalid,
    #[error("operation is not available through the read-only route")]
    Unsupported,
    #[error("selected worker is unreachable")]
    Offline,
    #[error("selected worker did not respond before the deadline")]
    Timeout,
    #[error("selected worker authentication or authorization failed")]
    Denied,
    #[error("selected worker response is incompatible")]
    Incompatible,
    #[error("selected worker closed the session")]
    Closed,
    #[error("selected worker refused the request")]
    Refused(Box<Response>),
}

/// Exact worker routing for one link. Addresses are hints only; the pinned
/// certificate and daemon identity authenticate every connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub receiver: PeerId,
    pub daemon: String,
    pub control: SocketAddr,
    pub bulk: SocketAddr,
}

impl Target {
    fn validate(&self) -> Result<(), Error> {
        if pu_core::mesh::admin::valid_pair(self.control, self.bulk) {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }
}

/// The receiver allows one live control binding per peer: a new one fences the
/// previous binding and its bulk. The host therefore keeps exactly one link per
/// worker and carries every request and watch over it.
pub struct HostClient {
    /// Concurrent one-shot requests across all workers.
    admission: Arc<Semaphore>,
    /// Open watches; separate so long-lived observers never starve control.
    watches: Arc<Semaphore>,
    links: std::sync::Mutex<HashMap<PeerId, Arc<Link>>>,
    next_watch: std::sync::atomic::AtomicU64,
}

/// Concurrent watches per host across all workers. Each holds no buffering
/// beyond its bounded frame queue.
pub const MAX_WATCHES: usize = 64;

/// Frames buffered per local watcher before it is cut off as lagging; a cut-off
/// watcher resubscribes from its cursor and the worker reports the gap.
const WATCH_BUFFER: usize = 64;
/// Below the receiver's control read deadline.
const KEEPALIVE: Duration = Duration::from_secs(2);
/// A control session idle longer than this may already be closed remotely.
const STALE: Duration = Duration::from_secs(4);

/// Local watches per run, each with its own bounded queue. Frames fan out to
/// every watch of a run; a lagging watch is cut off rather than starved.
type Watchers =
    Arc<std::sync::Mutex<HashMap<AgentTarget, HashMap<u64, mpsc::Sender<OutputFrame>>>>>;

/// Reply deadline for operations that create worktrees or processes.
const SLOW_REPLY: Duration = Duration::from_secs(60);
/// Overall bound on one request, including any (re)connection handshakes.
const REQUEST_DEADLINE: Duration = Duration::from_secs(90);
/// Bound on subscribing or resubscribing a watch.
const SUBSCRIBE_DEADLINE: Duration = Duration::from_secs(20);
/// Resubscription attempts after a lost link before a watch reports Closed.
const RESUBSCRIBE_ATTEMPTS: usize = 5;

fn reply_deadline(operation: &Operation) -> Duration {
    match operation {
        Operation::Spawn { .. } | Operation::Stop { .. } => SLOW_REPLY,
        _ => SessionLimits::default().read_deadline,
    }
}

struct Conn {
    client: ControlClient<TcpStream>,
    reader: tokio::task::JoinHandle<()>,
    used: Instant,
    watchers: Watchers,
}

/// However a connection ends (error, replacement or a cancelled request that
/// owned it), its subscriptions end with it: closing the local queues makes
/// every watch resubscribe on the next connection and learn any gap.
impl Drop for Conn {
    fn drop(&mut self) {
        self.reader.abort();
        self.watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

/// A worker-issued operation window the host reuses for its mutations, valid
/// for one worker generation (a restart closes every window).
#[derive(Clone)]
struct CachedWindow {
    window: u64,
    expires_at: i64,
    generation: u64,
    reuse_until: Instant,
}

struct Link {
    target: Target,
    /// Generation of the worker reached by the current or last connection.
    generation: std::sync::atomic::AtomicU64,
    window: std::sync::Mutex<Option<CachedWindow>>,
    conn: tokio::sync::Mutex<Option<Conn>>,
    watchers: Watchers,
    keepalive: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Set when local trust or routing changed; the link must not be reused.
    retired: std::sync::atomic::AtomicBool,
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(task) = self
            .keepalive
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            task.abort();
        }
    }
}

impl Link {
    fn retired(&self) -> bool {
        self.retired.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn drop_conn(&self, slot: &mut Option<Conn>) {
        slot.take();
    }

    /// Fresh authenticated control + bulk pair. Discovery supplies only the
    /// current generation; nothing is replayed.
    async fn connect(&self, transport: &MeshTransport) -> Result<Conn, Error> {
        let limits = SessionLimits::default();
        let generation = ControlClient::discover(
            transport,
            connect(self.target.control).await?,
            self.target.receiver,
            self.target.daemon.clone(),
            limits.clone(),
        )
        .await
        .map_err(session_error)?;
        let client = ControlClient::connect(
            transport,
            connect(self.target.control).await?,
            self.target.receiver,
            self.target.daemon.clone(),
            generation.generation,
            limits,
        )
        .await
        .map_err(session_error)?;
        self.generation
            .store(generation.generation, std::sync::atomic::Ordering::SeqCst);
        let bulk = client
            .connect_bulk(transport, connect(self.target.bulk).await?)
            .await
            .map_err(session_error)?;
        Ok(Conn {
            client,
            reader: tokio::spawn(demultiplex(bulk, self.watchers.clone())),
            used: Instant::now(),
            watchers: self.watchers.clone(),
        })
    }

    /// The current connection, replacing one that is stale or whose bulk ended.
    async fn ensure<'a>(
        &self,
        slot: &'a mut Option<Conn>,
        transport: &MeshTransport,
    ) -> Result<&'a mut Conn, Error> {
        if self.retired() {
            return Err(Error::Closed);
        }
        if slot
            .as_ref()
            .is_some_and(|conn| conn.reader.is_finished() || conn.used.elapsed() > STALE)
        {
            self.drop_conn(slot);
        }
        if slot.is_none() {
            *slot = Some(self.connect(transport).await?);
        }
        slot.as_mut().ok_or(Error::Closed)
    }

    /// Connect if needed, then register a watch queue for `run` on that
    /// connection before any frame for it can arrive.
    async fn register(
        &self,
        transport: &MeshTransport,
        run: &AgentTarget,
        id: u64,
    ) -> Result<mpsc::Receiver<OutputFrame>, Error> {
        let mut slot = self.conn.lock().await;
        self.ensure(&mut slot, transport).await?;
        let (tx, rx) = mpsc::channel(WATCH_BUFFER);
        self.watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(run.clone())
            .or_default()
            .insert(id, tx);
        Ok(rx)
    }

    /// One request on the link. The connection is taken out for the duration:
    /// if the caller is cancelled mid-request, or the reply does not correlate,
    /// it is dropped instead of leaving a half-read reply for the next request.
    /// Never retried: mutations are durable per operation window on the
    /// receiver, so the caller decides.
    async fn request(
        &self,
        transport: &MeshTransport,
        request: &Request,
        admission: Option<&Semaphore>,
    ) -> Result<Response, Error> {
        let mut slot = self.conn.lock().await;
        // Admission is per request actually sent, taken after the per-worker
        // lock so one stalled worker cannot hold the host's whole budget.
        let _admission = match admission {
            Some(admission) => Some(admission.try_acquire().map_err(|_| Error::Capacity)?),
            None => None,
        };
        self.ensure(&mut slot, transport).await?;
        let mut conn = slot.take().ok_or(Error::Closed)?;
        let result = conn
            .client
            .request_within::<Response>(request, reply_deadline(&request.operation))
            .await;
        conn.used = Instant::now();
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.drop_conn(&mut Some(conn));
                return Err(session_error(error));
            }
        };
        // The session client validates framing; this checks exact receiver,
        // request, kind and resource correlation as well.
        let decoded = response
            .encode()
            .ok()
            .and_then(|bytes| Response::decode(&bytes, request).ok());
        match decoded {
            Some(response) => {
                *slot = Some(conn);
                Ok(response)
            }
            None => {
                self.drop_conn(&mut Some(conn));
                Err(Error::Incompatible)
            }
        }
    }
}

impl Link {
    /// Windows are per principal and quota-limited on the worker, so the host
    /// reuses one until half its lifetime is spent instead of leaking a new
    /// window per command. A reused window is reported as issued; the worker
    /// still checks it on every mutation.
    async fn window(
        &self,
        transport: &MeshTransport,
        request: &Request,
        ttl_seconds: u32,
        admission: &Semaphore,
    ) -> Result<Response, Error> {
        let generation = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let cached = self
            .window
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|w| w.generation == generation && Instant::now() < w.reuse_until);
        if let Some(cached) = cached {
            return Ok(Response {
                request_id: request.request_id.clone(),
                daemon_id: request.target_daemon.clone(),
                body: Body::Window {
                    window: cached.window,
                    expires_at: cached.expires_at,
                },
            });
        }
        let response = self.request(transport, request, Some(admission)).await?;
        if let Body::Window { window, expires_at } = response.body {
            *self.window.lock().unwrap_or_else(|e| e.into_inner()) = Some(CachedWindow {
                window,
                expires_at,
                // Generation of the connection that issued it.
                generation: self.generation.load(std::sync::atomic::Ordering::SeqCst),
                reuse_until: Instant::now() + Duration::from_secs(u64::from(ttl_seconds) / 2),
            });
        }
        Ok(response)
    }
}

/// Route bulk frames to the local watches of their run.
async fn demultiplex(mut bulk: mesh_sessions::BulkClient<TcpStream>, watchers: Watchers) {
    loop {
        let Ok(frame) = bulk.read::<OutputFrame>().await else {
            break;
        };
        // Re-validate the frame shape after the session's own framing.
        let Ok(frame) = serde_json::to_vec(&frame)
            .map_err(|_| ())
            .and_then(|bytes| OutputFrame::decode(&bytes).map_err(|_| ()))
        else {
            break;
        };
        let mut watchers = watchers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(watches) = watchers.get_mut(&frame.target) {
            // Lagging or gone: cut that watch off rather than drop its frames.
            watches.retain(|_, sender| sender.try_send(frame.clone()).is_ok());
        }
    }
    // Bulk ended: every watch must resubscribe and learn any gap.
    watchers.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

impl HostClient {
    pub fn new(max_concurrent: usize) -> Result<Self, Error> {
        if !(1..=64).contains(&max_concurrent) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            admission: Arc::new(Semaphore::new(max_concurrent)),
            watches: Arc::new(Semaphore::new(MAX_WATCHES)),
            links: std::sync::Mutex::new(HashMap::new()),
            next_watch: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Retire every link, ending open watches; called when local trust or
    /// routing changes so nothing keeps using a superseded trust set.
    pub fn reset(&self) {
        for (_, link) in self.links.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            link.retired
                .store(true, std::sync::atomic::Ordering::SeqCst);
            link.watchers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
    }

    fn link(&self, target: &Target) -> Arc<Link> {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(link) = links.get(&target.receiver)
            && link.target == *target
        {
            return link.clone();
        }
        let link = Arc::new(Link {
            target: target.clone(),
            conn: tokio::sync::Mutex::new(None),
            watchers: Arc::default(),
            keepalive: std::sync::Mutex::new(None),
            retired: std::sync::atomic::AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
            window: std::sync::Mutex::new(None),
        });
        if let Some(previous) = links.insert(target.receiver, link.clone()) {
            previous
                .retired
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        link
    }

    /// One request over the worker's link. Never retried automatically.
    pub async fn request(
        &self,
        transport: &MeshTransport,
        target: &Target,
        request: &Request,
    ) -> Result<Response, Error> {
        if matches!(
            request.operation,
            Operation::SubscribeOutput { .. } | Operation::UnsubscribeOutput { .. }
        ) {
            return Err(Error::Unsupported);
        }
        request.validate().map_err(|_| Error::Invalid)?;
        target.validate()?;
        if request.target_daemon != target.daemon {
            return Err(Error::Invalid);
        }
        let link = self.link(target);
        timeout(REQUEST_DEADLINE, async {
            if let Operation::IssueWindow { ttl_seconds } = request.operation {
                return link
                    .window(transport, request, ttl_seconds, &self.admission)
                    .await;
            }
            let response = link
                .request(transport, request, Some(&self.admission))
                .await?;
            if matches!(
                response.body,
                Body::Error {
                    code: pu_core::mesh::response::ErrorCode::ReceiptExpired
                }
            ) {
                // The cached window was closed under us; issue a fresh one next.
                *link.window.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Ok(response)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    /// Subscribe to one run's output over the worker's link. The watch holds a
    /// watch permit for its lifetime and resubscribes from its cursor after a
    /// lost link, so any evicted output is reported as a gap.
    pub async fn watch(
        &self,
        transport: Arc<MeshTransport>,
        target: &Target,
        request: &Request,
    ) -> Result<(Response, Watch), Error> {
        let Operation::SubscribeOutput { target: run, .. } = &request.operation else {
            return Err(Error::Unsupported);
        };
        request.validate().map_err(|_| Error::Invalid)?;
        target.validate()?;
        if request.target_daemon != target.daemon {
            return Err(Error::Invalid);
        }
        let admission = self
            .watches
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Capacity)?;
        let mut watch = Watch {
            link: self.link(target),
            transport,
            run: run.clone(),
            daemon: target.daemon.clone(),
            id: self
                .next_watch
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            rx: None,
            cursor: None,
            _admission: admission,
        };
        let response = watch.subscribe(request.clone()).await?;
        Ok((response, watch))
    }
}

/// A live output subscription. Frames are validated against the subscribed
/// run; gaps are surfaced, never filled.
pub struct Watch {
    link: Arc<Link>,
    transport: Arc<MeshTransport>,
    run: AgentTarget,
    daemon: String,
    id: u64,
    rx: Option<mpsc::Receiver<OutputFrame>>,
    cursor: Option<OutputCursor>,
    _admission: tokio::sync::OwnedSemaphorePermit,
}

impl Watch {
    async fn subscribe(&mut self, request: Request) -> Result<Response, Error> {
        let response = timeout(SUBSCRIBE_DEADLINE, async {
            let rx = self
                .link
                .register(&self.transport, &self.run, self.id)
                .await?;
            let response = self.link.request(&self.transport, &request, None).await?;
            Ok::<_, Error>((rx, response))
        })
        .await
        .map_err(|_| Error::Timeout)
        .and_then(|result| result);
        match response {
            Ok((rx, response)) => match &response.body {
                Body::Subscribed { cursor, .. } => {
                    // A resubscription after a different incarnation resets the
                    // cursor; within one incarnation keep the furthest offset.
                    if self
                        .cursor
                        .as_ref()
                        .is_none_or(|current| current.incarnation != cursor.incarnation)
                    {
                        self.cursor = Some(cursor.clone());
                    }
                    self.rx = Some(rx);
                    self.keepalive();
                    Ok(response)
                }
                _ => {
                    self.forget();
                    Err(Error::Refused(Box::new(response)))
                }
            },
            Err(error) => {
                self.forget();
                Err(error)
            }
        }
    }

    /// Keep the shared control binding alive while any watch is open.
    fn keepalive(&self) {
        let mut slot = self
            .link
            .keepalive
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let link = Arc::downgrade(&self.link);
        let transport = self.transport.clone();
        let daemon = self.daemon.clone();
        *slot = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(KEEPALIVE).await;
                let Some(link) = link.upgrade() else {
                    return;
                };
                if link.retired()
                    || link
                        .watchers
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .is_empty()
                {
                    return;
                }
                let idle = {
                    let slot = link.conn.lock().await;
                    slot.as_ref()
                        .is_some_and(|conn| conn.used.elapsed() >= KEEPALIVE)
                };
                if idle {
                    let ping = Request {
                        request_id: pu_core::id::session_id(),
                        target_daemon: daemon.clone(),
                        operation: Operation::Inventory {},
                    };
                    let _ = link.request(&transport, &ping, None).await;
                }
            }
        }));
    }

    /// Remove this watch; true if it was the run's last local watch.
    fn forget(&self) -> bool {
        let mut watchers = self.link.watchers.lock().unwrap_or_else(|e| e.into_inner());
        let Some(watches) = watchers.get_mut(&self.run) else {
            return true;
        };
        watches.remove(&self.id);
        if watches.is_empty() {
            watchers.remove(&self.run);
            return true;
        }
        false
    }

    /// Drop bytes this watch already delivered; another watch of the same run
    /// may have restarted the worker's subscription from an earlier offset.
    fn unseen(&self, mut frame: OutputFrame) -> Option<OutputFrame> {
        let Some(cursor) = &self.cursor else {
            return Some(frame);
        };
        if cursor.incarnation != frame.incarnation || frame.offset >= cursor.offset {
            return Some(frame);
        }
        let seen = usize::try_from(cursor.offset - frame.offset).ok()?;
        if seen >= frame.data.len() && !frame.end {
            return None;
        }
        frame.data.drain(..seen.min(frame.data.len()));
        frame.offset = cursor.offset;
        frame.gap = None;
        Some(frame)
    }

    pub async fn next(&mut self) -> Result<OutputFrame, Error> {
        let mut attempts = 0;
        loop {
            if self.link.retired() {
                return Err(Error::Closed);
            }
            if let Some(rx) = self.rx.as_mut()
                && let Some(frame) = rx.recv().await
            {
                if frame.target != self.run {
                    return Err(Error::Incompatible);
                }
                let Some(frame) = self.unseen(frame) else {
                    continue;
                };
                self.cursor = Some(OutputCursor {
                    incarnation: frame.incarnation.clone(),
                    offset: frame.offset + frame.data.len() as u64,
                });
                return Ok(frame);
            }
            // The link or this watch was cut: resubscribe from the cursor. The
            // worker reports evicted bytes as a gap; observation is never an effect.
            if attempts == RESUBSCRIBE_ATTEMPTS || self.link.retired() {
                return Err(Error::Closed);
            }
            attempts += 1;
            self.rx = None;
            let request = Request {
                request_id: pu_core::id::session_id(),
                target_daemon: self.daemon.clone(),
                operation: Operation::SubscribeOutput {
                    target: self.run.clone(),
                    cursor: self.cursor.clone(),
                },
            };
            if self.subscribe(request).await.is_err() {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // Other local watches of this run keep the worker's subscription.
        if !self.forget() || self.link.retired() {
            return;
        }
        let link = self.link.clone();
        let transport = self.transport.clone();
        let request = Request {
            request_id: pu_core::id::session_id(),
            target_daemon: self.daemon.clone(),
            operation: Operation::UnsubscribeOutput {
                target: self.run.clone(),
            },
        };
        // Best effort: release the receiver's subscription slot.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = timeout(
                    Duration::from_secs(5),
                    link.request(&transport, &request, None),
                )
                .await;
            });
        }
    }
}

async fn connect(address: SocketAddr) -> Result<TcpStream, Error> {
    let stream = timeout(Duration::from_secs(5), TcpStream::connect(address))
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Offline)?;
    stream.set_nodelay(true).map_err(|_| Error::Offline)?;
    Ok(stream)
}
fn session_error(error: mesh_sessions::Error) -> Error {
    match error {
        mesh_sessions::Error::Denied => Error::Denied,
        mesh_sessions::Error::Capacity => Error::Capacity,
        mesh_sessions::Error::Closed => Error::Closed,
        mesh_sessions::Error::Transport(mesh_transport::Error::Timeout) => Error::Timeout,
        mesh_sessions::Error::Wire(WireError::Version) => Error::Incompatible,
        mesh_sessions::Error::Transport(_) => Error::Denied,
        _ => Error::Incompatible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_transport::{EnrolledPeers, Identity};
    use pu_core::mesh::wire::{Operation, Request};
    use std::sync::Arc;

    fn transport_pair() -> (MeshTransport, MeshTransport, PeerId) {
        let client = Identity::generate().unwrap();
        let server = Identity::generate().unwrap();
        let pin = server.peer_id();
        (
            MeshTransport::new(
                &client,
                EnrolledPeers::new([server.certificate_der()]).unwrap(),
            )
            .unwrap(),
            MeshTransport::new(
                &server,
                EnrolledPeers::new([client.certificate_der()]).unwrap(),
            )
            .unwrap(),
            pin,
        )
    }

    fn target(receiver: PeerId, control: SocketAddr, bulk: SocketAddr) -> Target {
        Target {
            receiver,
            daemon: "worker".into(),
            control,
            bulk,
        }
    }

    fn inventory() -> Request {
        Request {
            request_id: "read".into(),
            target_daemon: "worker".into(),
            operation: Operation::Inventory {},
        }
    }

    #[tokio::test]
    async fn given_stalled_workers_should_bound_host_fanout_and_release_on_cancellation() {
        // given: a host budget of one request and two stalled workers
        let client = Identity::generate().unwrap();
        let first = Identity::generate().unwrap();
        let second = Identity::generate().unwrap();
        let transport = Arc::new(
            MeshTransport::new(
                &client,
                EnrolledPeers::new([first.certificate_der(), second.certificate_der()]).unwrap(),
            )
            .unwrap(),
        );
        let host = Arc::new(HostClient::new(1).unwrap());
        let listen = || async { tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap() };
        let (control_a, bulk_a, control_b, bulk_b) = (
            listen().await,
            listen().await,
            listen().await,
            listen().await,
        );
        let route_a = target(
            first.peer_id(),
            control_a.local_addr().unwrap(),
            bulk_a.local_addr().unwrap(),
        );
        let route_b = target(
            second.peer_id(),
            control_b.local_addr().unwrap(),
            bulk_b.local_addr().unwrap(),
        );
        // when: one request stalls on the first worker
        let (h, t, r) = (host.clone(), transport.clone(), route_a.clone());
        let pending = tokio::spawn(async move { h.request(&t, &r, &inventory()).await });
        let (_stalled, _) = control_a.accept().await.unwrap();
        // then: a request to another worker is refused, not queued
        assert!(matches!(
            host.request(&transport, &route_b, &inventory()).await,
            Err(Error::Capacity)
        ));
        assert!(
            timeout(Duration::from_millis(50), control_b.accept())
                .await
                .is_err()
        );
        // when: the stalled request is cancelled, its permit is released
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        let (h, t, r) = (host.clone(), transport.clone(), route_b.clone());
        let next = tokio::spawn(async move { h.request(&t, &r, &inventory()).await });
        let (_next, _) = timeout(Duration::from_secs(1), control_b.accept())
            .await
            .unwrap()
            .unwrap();
        next.abort();
        assert!(next.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn given_authenticated_mismatched_reply_should_refuse_without_replaying_request() {
        use pu_core::mesh::{response::Body, wire};
        let (transport, server, pin) = transport_pair();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bulk_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route = target(
            pin,
            listener.local_addr().unwrap(),
            bulk_listener.local_addr().unwrap(),
        );
        let serve = tokio::spawn(async move {
            let (io, _) = listener.accept().await.unwrap();
            let mut discovery = server
                .accept(io, mesh_transport::Role::Control)
                .await
                .unwrap();
            assert!(matches!(
                wire::Hello::decode(&discovery.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap(),
                wire::Hello::Discover { .. }
            ));
            discovery
                .write_frame(
                    &serde_json::to_vec(&wire::GenerationDiscovery {
                        version: wire::BRIDGE_VERSION,
                        daemon_id: "worker".into(),
                        generation: 2,
                    })
                    .unwrap(),
                    Duration::from_secs(1),
                )
                .await
                .unwrap();
            drop(discovery);
            let (io, _) = listener.accept().await.unwrap();
            let mut control = server
                .accept(io, mesh_transport::Role::Control)
                .await
                .unwrap();
            assert!(matches!(
                wire::Hello::decode(&control.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap(),
                wire::Hello::Control { generation: 2, .. }
            ));
            let welcome = serde_json::to_vec(&wire::Welcome {
                version: wire::BRIDGE_VERSION,
                daemon_id: "worker".into(),
                generation: 2,
                incarnation: "incarnation".into(),
                control_binding: "binding".into(),
            })
            .unwrap();
            control
                .write_frame(&welcome, Duration::from_secs(1))
                .await
                .unwrap();
            let (io, _) = bulk_listener.accept().await.unwrap();
            let mut bulk = server.accept(io, mesh_transport::Role::Bulk).await.unwrap();
            assert!(matches!(
                wire::Hello::decode(&bulk.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap(),
                wire::Hello::Bulk { .. }
            ));
            bulk.write_frame(&welcome, Duration::from_secs(1))
                .await
                .unwrap();
            let request =
                Request::decode(&control.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap();
            assert_eq!(request, inventory());
            let reply = Response {
                request_id: "someone-else".into(),
                daemon_id: "worker".into(),
                body: Body::Inventory { exports: vec![] },
            };
            control
                .write_frame(&reply.encode().unwrap(), Duration::from_secs(1))
                .await
                .unwrap();
            assert!(
                timeout(Duration::from_millis(30), listener.accept())
                    .await
                    .is_err()
            );
        });
        assert!(matches!(
            HostClient::new(1)
                .unwrap()
                .request(&transport, &route, &inventory())
                .await,
            Err(Error::Incompatible)
        ));
        serve.await.unwrap();
    }
    #[tokio::test]
    async fn given_output_subscription_on_one_shot_route_should_refuse_before_contacting() {
        let identity = Identity::generate().unwrap();
        let receiver = Identity::generate().unwrap();
        let transport = MeshTransport::new(
            &identity,
            EnrolledPeers::new([receiver.certificate_der()]).unwrap(),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let request = Request {
            request_id: "operation".into(),
            target_daemon: "worker".into(),
            operation: Operation::SubscribeOutput {
                target: pu_core::mesh::wire::AgentTarget {
                    export: pu_core::mesh::wire::Export {
                        id: "export".into(),
                        generation: 1,
                    },
                    agent_id: "agent".into(),
                    run_id: "run".into(),
                },
                cursor: None,
            },
        };
        assert!(matches!(
            HostClient::new(2)
                .unwrap()
                .request(
                    &transport,
                    &target(
                        receiver.peer_id(),
                        listener.local_addr().unwrap(),
                        "127.0.0.1:9".parse().unwrap()
                    ),
                    &request
                )
                .await,
            Err(Error::Unsupported)
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }
}
