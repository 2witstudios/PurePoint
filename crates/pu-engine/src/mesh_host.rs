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
        if specific(self.control) && specific(self.bulk) && self.control.ip() == self.bulk.ip() {
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
}

/// Concurrent watches per host across all workers. Each holds no buffering
/// beyond its bounded frame queue.
pub const MAX_WATCHES: usize = 64;

/// Frames buffered per local watcher before it is cut off as lagging; a cut-off
/// watcher resubscribes from its cursor and the worker reports the gap.
const WATCH_BUFFER: usize = 256;
/// Below the receiver's control read deadline.
const KEEPALIVE: Duration = Duration::from_secs(2);
/// A control session idle longer than this may already be closed remotely.
const STALE: Duration = Duration::from_secs(4);

fn specific(address: SocketAddr) -> bool {
    let ip = match address.ip() {
        std::net::IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(address.ip(), Into::into),
        ip => ip,
    };
    address.port() != 0
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !matches!(ip, std::net::IpAddr::V4(ip) if ip.is_broadcast())
}

type Watchers = Arc<std::sync::Mutex<HashMap<AgentTarget, mpsc::Sender<OutputFrame>>>>;

struct Conn {
    client: ControlClient<TcpStream>,
    reader: tokio::task::JoinHandle<()>,
    used: Instant,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

struct Link {
    target: Target,
    conn: tokio::sync::Mutex<Option<Conn>>,
    watchers: Watchers,
    keepalive: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
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
        let mut bulk = client
            .connect_bulk(transport, connect(self.target.bulk).await?)
            .await
            .map_err(session_error)?;
        let watchers = self.watchers.clone();
        let reader = tokio::spawn(async move {
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
                if let Some(sender) = watchers.get(&frame.target)
                    && sender.try_send(frame.clone()).is_err()
                {
                    // Lagging or gone: cut it off rather than drop frames silently.
                    watchers.remove(&frame.target);
                }
            }
            // Bulk ended: every watcher must resubscribe and learn any gap.
            watchers.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });
        Ok(Conn {
            client,
            reader,
            used: Instant::now(),
        })
    }

    async fn request(
        &self,
        transport: &MeshTransport,
        request: &Request,
    ) -> Result<Response, Error> {
        let mut slot = self.conn.lock().await;
        if slot
            .as_ref()
            .is_some_and(|conn| conn.reader.is_finished() || conn.used.elapsed() > STALE)
        {
            *slot = None;
        }
        if slot.is_none() {
            *slot = Some(self.connect(transport).await?);
        }
        let conn = slot.as_mut().ok_or(Error::Closed)?;
        let result = conn.client.request::<Response>(request).await;
        conn.used = Instant::now();
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                // Never retried: the caller decides, and mutations are durable
                // per operation window on the receiver.
                *slot = None;
                return Err(session_error(error));
            }
        };
        // The session client validates framing; this checks exact receiver,
        // request, kind and resource correlation as well.
        Response::decode(
            &response.encode().map_err(|_| Error::Incompatible)?,
            request,
        )
        .map_err(|_| Error::Incompatible)
    }
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
        })
    }

    /// Drop every link; called when local trust or routing changes.
    pub fn reset(&self) {
        self.links.lock().unwrap_or_else(|e| e.into_inner()).clear();
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
        });
        links.insert(target.receiver, link.clone());
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
        let _admission = self.admission.try_acquire().map_err(|_| Error::Capacity)?;
        let link = self.link(target);
        // One overall deadline bounds handshakes and a spawn's worktree setup.
        timeout(Duration::from_secs(60), link.request(transport, request))
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Subscribe to one run's output over the worker's link. The watch holds an
    /// admission permit for its lifetime and resubscribes from its cursor after
    /// a lost link, so any evicted output is reported as a gap.
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
        let link = self.link(target);
        let mut watch = Watch {
            link,
            transport,
            run: run.clone(),
            daemon: target.daemon.clone(),
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
    rx: Option<mpsc::Receiver<OutputFrame>>,
    cursor: Option<OutputCursor>,
    _admission: tokio::sync::OwnedSemaphorePermit,
}

impl Watch {
    async fn subscribe(&mut self, request: Request) -> Result<Response, Error> {
        // Register before subscribing so no early frame is lost.
        let (tx, rx) = mpsc::channel(WATCH_BUFFER);
        self.link
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.run.clone(), tx);
        let response = timeout(
            Duration::from_secs(20),
            self.link.request(&self.transport, &request),
        )
        .await
        .map_err(|_| Error::Timeout)
        .and_then(|result| result);
        match response {
            Ok(response) => match &response.body {
                Body::Subscribed { cursor, .. } => {
                    self.cursor = Some(cursor.clone());
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
                if link
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
                    let _ = link.request(&transport, &ping).await;
                }
            }
        }));
    }

    fn forget(&self) {
        self.link
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.run);
    }

    pub async fn next(&mut self) -> Result<OutputFrame, Error> {
        for _ in 0..3 {
            if let Some(rx) = self.rx.as_mut()
                && let Some(frame) = rx.recv().await
            {
                if frame.target != self.run {
                    return Err(Error::Incompatible);
                }
                self.cursor = Some(OutputCursor {
                    incarnation: frame.incarnation.clone(),
                    offset: frame.offset + frame.data.len() as u64,
                });
                return Ok(frame);
            }
            // The link or this watcher was cut: resubscribe from the cursor. The
            // worker reports evicted bytes as a gap; observation is never an effect.
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
        Err(Error::Closed)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.forget();
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
                let _ = timeout(Duration::from_secs(5), link.request(&transport, &request)).await;
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
    async fn given_stalled_peer_should_bound_host_fanout_and_release_on_cancellation() {
        let (transport, _, pin) = transport_pair();
        let transport = Arc::new(transport);
        let host = Arc::new(HostClient::new(1).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bulk = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route = target(
            pin,
            listener.local_addr().unwrap(),
            bulk.local_addr().unwrap(),
        );
        let (h, t, r) = (host.clone(), transport.clone(), route.clone());
        let pending = tokio::spawn(async move { h.request(&t, &r, &inventory()).await });
        let (_stalled, _) = listener.accept().await.unwrap();
        assert!(matches!(
            host.request(&transport, &route, &inventory()).await,
            Err(Error::Capacity)
        ));
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        let (h, t, r) = (host.clone(), transport.clone(), route.clone());
        let next = tokio::spawn(async move { h.request(&t, &r, &inventory()).await });
        let (_next, _) = timeout(Duration::from_secs(1), listener.accept())
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
