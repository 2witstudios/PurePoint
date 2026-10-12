//! Explicit paired LAN endpoints. No listener is created by constructing an
//! Engine or SessionAuthority; production startup consumes local configuration.

use crate::{
    daemon_lifecycle::mesh_exports::MeshExports,
    daemon_lifecycle::owned_work::WorkOwner,
    execution_state::{Caller, Ledger},
    mesh_sessions::{self, Accepted, SessionAuthority},
    mesh_transport::{MeshTransport, Role},
};
use pu_core::mesh::admin::ListenConfig;
use pu_core::mesh::{PeerPolicy, wire::Request};
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinSet,
};

/// Daemon-side bridge behavior behind an authenticated session. `binding` is the
/// receiver-issued control binding for the request's connection; the bulk
/// connection that names the same binding is handed to `serve_bulk`.
pub trait BridgeHandler: Send + Sync + 'static {
    fn dispatch(
        &self,
        binding: &str,
        ledger: &Ledger,
        exports: &MeshExports,
        caller: &Caller,
        policy: &PeerPolicy,
        request: &Request,
    ) -> mesh_sessions::Result<Vec<u8>>;

    /// A control session started or ended; bulk state for it may be created or
    /// must be released.
    fn control_opened(&self, _binding: &str) {}
    fn control_closed(&self, _binding: &str) {}

    /// Serve one authenticated bulk session until it closes. The default accepts
    /// no subscriptions and only detects disconnects.
    fn serve_bulk(
        self: Arc<Self>,
        mut session: mesh_sessions::Session<TcpStream>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move {
            let _ = session.wait_bulk_close().await;
        })
    }
}

/// Read-only metadata handler used before execution is enabled.
pub struct ReadOnly;

impl BridgeHandler for ReadOnly {
    fn dispatch(
        &self,
        _binding: &str,
        ledger: &Ledger,
        exports: &MeshExports,
        caller: &Caller,
        policy: &PeerPolicy,
        request: &Request,
    ) -> mesh_sessions::Result<Vec<u8>> {
        Ok(crate::mesh_read::dispatch(
            ledger, exports, caller, policy, request,
        )?)
    }
}

pub struct BoundListeners {
    control: TcpListener,
    bulk: TcpListener,
    #[cfg(test)]
    faults: std::sync::Mutex<std::collections::VecDeque<io::Error>>,
}

impl BoundListeners {
    /// Bind both explicit interfaces before exposing either to the service loop.
    /// A failed second bind drops the first; disabled configuration binds nothing.
    pub async fn bind(config: &ListenConfig) -> io::Result<Option<Self>> {
        if !config.validate() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid mesh listener addresses",
            ));
        }
        let (Some(control), Some(bulk)) = (config.bind_address, config.bulk_bind_address) else {
            return Ok(None);
        };
        let control = TcpListener::bind(control).await?;
        let bulk = TcpListener::bind(bulk).await?;
        Ok(Some(Self {
            control,
            bulk,
            #[cfg(test)]
            faults: Default::default(),
        }))
    }

    pub fn addresses(&self) -> io::Result<(SocketAddr, SocketAddr)> {
        Ok((self.control.local_addr()?, self.bulk.local_addr()?))
    }

    /// Every accepted TCP stream must first win an unauthenticated pre-auth
    /// slot (bounded total and per-source share, short handshake deadline)
    /// before a task is spawned; the session authority reserves its global and
    /// per-peer session permits only after the exact enrolled pin is proven,
    /// across listener reloads. Live tasks are therefore bounded by the pre-auth
    /// budget plus the session budgets, and unauthenticated traffic can slow,
    /// but never hold, an enrolled peer's admission.
    ///
    /// Transient `accept` failures (descriptor/buffer exhaustion, aborted or
    /// reset connections, interrupts) are logged and retried with bounded
    /// exponential backoff, so the endpoint survives them. Returns `Ok` on an
    /// explicit stop or work shutdown, and `Err` only when the listening socket
    /// itself is unusable (`EBADF`, `EINVAL`, `ENOTSOCK`, `EOPNOTSUPP`), where
    /// retrying cannot succeed. Stopping closes transports; accepted owned
    /// effects retain their barriers until actual completion. This does not
    /// stop local IPC or worker execution.
    pub async fn run<H: BridgeHandler>(
        self,
        authority: Arc<SessionAuthority>,
        transport: Arc<MeshTransport>,
        work: Arc<WorkOwner>,
        mut stop: watch::Receiver<bool>,
        handler: Arc<H>,
    ) -> io::Result<()> {
        let mut connections = JoinSet::new();
        let mut failures = 0u32;
        let outcome = loop {
            if *stop.borrow() || work.is_stopping() {
                break Ok(());
            }
            let incoming = tokio::select! {
                biased;
                _=stop.changed()=>break Ok(()),
                _=work.stopping()=>break Ok(()),
                _=connections.join_next(),if !connections.is_empty()=>continue,
                incoming=async {
                    #[cfg(test)]
                    if let Some(fault) = self.faults.lock().unwrap().pop_front() {
                        return Err(fault);
                    }
                    tokio::select! {
                        stream=self.control.accept()=>stream.map(|(io,from)|(io,from,Role::Control)),
                        stream=self.bulk.accept()=>stream.map(|(io,from)|(io,from,Role::Bulk)),
                    }
                }=>incoming,
            };
            let (stream, from, role) = match incoming {
                Ok(value) => {
                    failures = 0;
                    value
                }
                Err(error) if listener_unusable(&error) => break Err(error),
                Err(error) => {
                    failures = failures.saturating_add(1);
                    let delay = accept_backoff(failures);
                    tracing::warn!(%error, ?delay, "mesh listener accept failed; retrying");
                    tokio::select! {
                        biased;
                        _=stop.changed()=>break Ok(()),
                        _=work.stopping()=>break Ok(()),
                        _=tokio::time::sleep(delay)=>continue,
                    }
                }
            };
            // No queue: an exhausted budget or source share drops the stream
            // before any TLS work or task is spent on it.
            let Ok(preauth) = authority.preauthorize(Some(from.ip())) else {
                continue;
            };
            if stream.set_nodelay(true).is_err() {
                continue;
            }
            let authority = authority.clone();
            let transport = transport.clone();
            let work = work.clone();
            let handler = handler.clone();
            connections.spawn(async move {
                let mut session = match authority
                    .accept_admitted(&transport, stream, role, preauth)
                    .await
                {
                    Ok(Accepted::Session(session)) => session,
                    Ok(Accepted::Discovery) | Err(_) => return,
                };
                if role == Role::Bulk {
                    handler.serve_bulk(session).await;
                    return;
                }
                let binding = session.welcome().control_binding.clone();
                handler.control_opened(&binding);
                loop {
                    let effect = handler.clone();
                    let binding = binding.clone();
                    if session
                        .dispatch(&work, move |ledger, exports, caller, policy, request| {
                            effect.dispatch(&binding, ledger, exports, caller, policy, request)
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                handler.control_closed(&binding);
            });
        };
        connections.shutdown().await;
        outcome
    }
}

/// Errors that describe the listening socket itself rather than one incoming
/// connection or a temporary resource shortage.
fn listener_unusable(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EBADF | libc::EINVAL | libc::ENOTSOCK | libc::EOPNOTSUPP)
    )
}

/// 5 ms doubling per consecutive failure, capped at 1 s; reset on success.
fn accept_backoff(failures: u32) -> Duration {
    const FIRST: Duration = Duration::from_millis(5);
    const MAX: Duration = Duration::from_secs(1);
    FIRST
        .saturating_mul(1u32 << failures.saturating_sub(1).min(16))
        .min(MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyInventory;
    impl BridgeHandler for EmptyInventory {
        fn dispatch(
            &self,
            _binding: &str,
            _ledger: &Ledger,
            _exports: &MeshExports,
            _caller: &Caller,
            _policy: &PeerPolicy,
            request: &Request,
        ) -> mesh_sessions::Result<Vec<u8>> {
            Ok(pu_core::mesh::response::Response {
                request_id: request.request_id.clone(),
                daemon_id: request.target_daemon.clone(),
                body: pu_core::mesh::response::Body::Inventory { exports: vec![] },
            }
            .encode()?)
        }
    }
    use crate::{
        daemon_lifecycle::{mesh_store::MeshStore, owned_work::WorkOwner, try_acquire_daemon_lock},
        execution_state::{Caller, Ledger, Limits, LocalAdmin},
        mesh_sessions::{ControlClient, SessionAuthority, SessionLimits},
        mesh_transport::{EnrolledPeers, Identity, MeshTransport},
    };
    use pu_core::mesh::admin::ListenConfig;
    use pu_core::mesh::{Capability, ExportGrant, wire};
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn given_explicit_listener_should_discover_serve_bound_control_bulk_and_stop_without_stopping_local_work()
     {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let lock = Arc::new(
            try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
                .unwrap()
                .unwrap(),
        );
        let work = WorkOwner::new(&lock);
        let admin =
            LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
                .unwrap();
        let mut store = MeshStore::initialize(lock.clone(), &admin).unwrap();
        let exports =
            crate::daemon_lifecycle::mesh_exports::MeshExports::open(lock, store.daemon_id(), 8)
                .unwrap();
        let ledger = Ledger::open(
            tmp.path().join("ledger.sqlite"),
            store.daemon_id(),
            Limits::default(),
        )
        .unwrap();
        let peer = Identity::generate().unwrap();
        store
            .enroll(
                &ledger,
                &admin,
                peer.certificate_der(),
                vec![ExportGrant {
                    export_id: "export".into(),
                    capabilities: BTreeSet::from([Capability::Inspect, Capability::ReadOutput]),
                    runtimes: BTreeSet::new(),
                }],
                None,
            )
            .unwrap();
        let pin = store.identity().peer_id();
        let client_transport = MeshTransport::new(
            &peer,
            EnrolledPeers::new([store.identity().certificate_der()]).unwrap(),
        )
        .unwrap();
        let transport = Arc::new(
            MeshTransport::new(
                store.identity(),
                EnrolledPeers::new([peer.certificate_der()]).unwrap(),
            )
            .unwrap(),
        );
        let limits = SessionLimits {
            control_total: 1,
            control_per_peer: 1,
            bulk_total: 1,
            bulk_per_peer: 1,
            ..SessionLimits::default()
        };
        let authority = SessionAuthority::new(store, ledger, exports, limits.clone()).unwrap();
        let pair = BoundListeners {
            control: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            bulk: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            faults: Default::default(),
        };
        let (control, bulk) = pair.addresses().unwrap();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let serve = tokio::spawn(pair.run(
            authority.clone(),
            transport,
            work.clone(),
            rx,
            Arc::new(EmptyInventory),
        ));
        let request = wire::Request {
            request_id: "query".into(),
            target_daemon: authority.daemon_id().into(),
            operation: wire::Operation::Inventory {},
        };
        let response = crate::mesh_host::HostClient::new(2)
            .unwrap()
            .request(
                &client_transport,
                &crate::mesh_host::Target {
                    receiver: pin,
                    daemon: authority.daemon_id().into(),
                    control,
                    bulk,
                },
                &request,
            )
            .await
            .unwrap();
        assert!(
            matches!(response.body,pu_core::mesh::response::Body::Inventory {exports} if exports.is_empty())
        );
        // The host link above closes asynchronously; its single control slot
        // (control_total = 1) frees once the server observes the close.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let discovery = loop {
            match ControlClient::discover(
                &client_transport,
                tokio::net::TcpStream::connect(control).await.unwrap(),
                pin,
                authority.daemon_id().into(),
                limits.clone(),
            )
            .await
            {
                Ok(discovery) => break discovery,
                Err(error) => {
                    assert!(std::time::Instant::now() < deadline, "{error:?}");
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }
        };
        let mut client = ControlClient::connect(
            &client_transport,
            tokio::net::TcpStream::connect(control).await.unwrap(),
            pin,
            authority.daemon_id().into(),
            discovery.generation,
            limits.clone(),
        )
        .await
        .unwrap();
        assert!(
            matches!(client.request::<pu_core::mesh::response::Response>(&request).await.unwrap().body,
            pu_core::mesh::response::Body::Inventory {exports} if exports.is_empty())
        );
        // Independent bulk remains admissible while the only control slot is held.
        let _bulk = client
            .connect_bulk(
                &client_transport,
                tokio::net::TcpStream::connect(bulk).await.unwrap(),
            )
            .await
            .unwrap();
        let denied = tokio::time::timeout(
            Duration::from_secs(2),
            ControlClient::discover(
                &client_transport,
                tokio::net::TcpStream::connect(control).await.unwrap(),
                pin,
                authority.daemon_id().into(),
                limits,
            ),
        )
        .await
        .unwrap();
        assert!(denied.is_err());
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), serve)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(client.request::<serde_json::Value>(&request).await.is_err());
        assert!(tokio::net::TcpStream::connect(control).await.is_err());
        assert!(tokio::net::TcpStream::connect(bulk).await.is_err());
        assert!(!work.is_stopping());
        work.spawn(async {}).unwrap().await.unwrap();
    }

    #[tokio::test]
    async fn given_disabled_or_partial_bind_should_leave_no_remote_endpoint() {
        assert!(
            BoundListeners::bind(&ListenConfig::default())
                .await
                .unwrap()
                .is_none()
        );
        let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control = reserved.local_addr().unwrap();
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bulk = occupied.local_addr().unwrap();
        drop(reserved);
        let config = ListenConfig {
            bind_address: Some(control),
            bulk_bind_address: Some(bulk),
        };
        assert!(BoundListeners::bind(&config).await.is_err());
        let freed = TcpListener::bind(control).await.unwrap();
        drop(freed);
        drop(occupied);
        let listeners = BoundListeners::bind(&config).await.unwrap().unwrap();
        assert_eq!(listeners.addresses().unwrap(), (control, bulk));
    }

    /// One enrolled host against a real paired listener over loopback.
    struct Served {
        _tmp: tempfile::TempDir,
        work: Arc<WorkOwner>,
        authority: Arc<SessionAuthority>,
        client: MeshTransport,
        pin: crate::mesh_transport::PeerId,
        control: SocketAddr,
        bulk: SocketAddr,
        stop: tokio::sync::watch::Sender<bool>,
        serve: tokio::task::JoinHandle<io::Result<()>>,
    }

    async fn serve(limits: SessionLimits, faults: Vec<io::Error>) -> Served {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let lock = Arc::new(
            try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
                .unwrap()
                .unwrap(),
        );
        let work = WorkOwner::new(&lock);
        let admin =
            LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
                .unwrap();
        let mut store = MeshStore::initialize(lock.clone(), &admin).unwrap();
        let exports =
            crate::daemon_lifecycle::mesh_exports::MeshExports::open(lock, store.daemon_id(), 8)
                .unwrap();
        let ledger = Ledger::open(
            tmp.path().join("ledger.sqlite"),
            store.daemon_id(),
            Limits::default(),
        )
        .unwrap();
        let peer = Identity::generate().unwrap();
        store
            .enroll(
                &ledger,
                &admin,
                peer.certificate_der(),
                vec![ExportGrant {
                    export_id: "export".into(),
                    capabilities: BTreeSet::from([Capability::Inspect]),
                    runtimes: BTreeSet::new(),
                }],
                None,
            )
            .unwrap();
        let pin = store.identity().peer_id();
        let client = MeshTransport::new(
            &peer,
            EnrolledPeers::new([store.identity().certificate_der()]).unwrap(),
        )
        .unwrap();
        let transport = Arc::new(
            MeshTransport::new(
                store.identity(),
                EnrolledPeers::new([peer.certificate_der()]).unwrap(),
            )
            .unwrap(),
        );
        let authority = SessionAuthority::new(store, ledger, exports, limits).unwrap();
        let pair = BoundListeners {
            control: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            bulk: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            faults: std::sync::Mutex::new(faults.into()),
        };
        let (control, bulk) = pair.addresses().unwrap();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let serve = tokio::spawn(pair.run(
            authority.clone(),
            transport,
            work.clone(),
            rx,
            Arc::new(EmptyInventory),
        ));
        Served {
            _tmp: tmp,
            work,
            authority,
            client,
            pin,
            control,
            bulk,
            stop,
            serve,
        }
    }

    impl Served {
        /// Discover, bind control and its bulk stream, retrying until `within`.
        async fn enroll_connect(
            &self,
            within: Duration,
        ) -> (
            ControlClient<tokio::net::TcpStream>,
            mesh_sessions::BulkClient<tokio::net::TcpStream>,
        ) {
            let deadline = std::time::Instant::now() + within;
            loop {
                let attempt = async {
                    let limits = self.authority.limits().clone();
                    let discovery = ControlClient::discover(
                        &self.client,
                        TcpStream::connect(self.control).await?,
                        self.pin,
                        self.authority.daemon_id().into(),
                        limits.clone(),
                    )
                    .await
                    .map_err(io::Error::other)?;
                    let client = ControlClient::connect(
                        &self.client,
                        TcpStream::connect(self.control).await?,
                        self.pin,
                        self.authority.daemon_id().into(),
                        discovery.generation,
                        limits,
                    )
                    .await
                    .map_err(io::Error::other)?;
                    let bulk = client
                        .connect_bulk(&self.client, TcpStream::connect(self.bulk).await?)
                        .await
                        .map_err(io::Error::other)?;
                    Ok::<_, io::Error>((client, bulk))
                };
                match tokio::time::timeout_at(deadline.into(), attempt).await {
                    Ok(Ok(connected)) => return connected,
                    Ok(Err(error)) if std::time::Instant::now() < deadline => {
                        let _ = error;
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    Ok(Err(error)) => panic!("enrolled peer locked out for {within:?}: {error}"),
                    Err(_) => panic!("enrolled peer locked out for {within:?}: timed out"),
                }
            }
        }

        fn inventory(&self) -> wire::Request {
            wire::Request {
                request_id: "query".into(),
                target_daemon: self.authority.daemon_id().into(),
                operation: wire::Operation::Inventory {},
            }
        }

        async fn shutdown(self) -> io::Result<()> {
            self.stop.send(true).unwrap();
            let result = tokio::time::timeout(Duration::from_secs(2), self.serve)
                .await
                .unwrap()
                .unwrap();
            assert!(!self.work.is_stopping());
            result
        }
    }

    fn single_session_limits() -> SessionLimits {
        SessionLimits {
            control_total: 1,
            control_per_peer: 1,
            bulk_total: 1,
            bulk_per_peer: 1,
            ..SessionLimits::default()
        }
    }

    #[tokio::test]
    async fn given_unauthenticated_flood_should_not_lock_out_enrolled_peer() {
        // given: far more idle TCP connections than session permits on BOTH
        // ports, none of which ever starts TLS (loopback is a single source, so
        // the per-source cap is lifted to isolate the session-permit property).
        let s = serve(
            SessionLimits {
                preauth_total: 32,
                preauth_per_source: 32,
                ..single_session_limits()
            },
            vec![],
        )
        .await;
        let mut flood = Vec::new();
        for _ in 0..12 {
            flood.push(TcpStream::connect(s.control).await.unwrap());
            flood.push(TcpStream::connect(s.bulk).await.unwrap());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        // when: the enrolled host connects while every flood stream is held,
        // well before the pre-auth handshake deadline could reclaim any of them
        let started = std::time::Instant::now();
        let (mut client, _bulk) = s.enroll_connect(Duration::from_millis(800)).await;
        // then
        assert!(started.elapsed() < Duration::from_millis(800));
        let response = client
            .request::<pu_core::mesh::response::Response>(&s.inventory())
            .await
            .unwrap();
        assert!(matches!(
            response.body,
            pu_core::mesh::response::Body::Inventory { .. }
        ));
        drop(flood);
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn given_saturated_preauth_budget_should_reclaim_idle_handshakes_within_deadline() {
        // given: idle connections exceed the whole pre-auth budget
        let s = serve(
            SessionLimits {
                preauth_total: 4,
                preauth_per_source: 4,
                handshake_deadline: Duration::from_millis(300),
                ..SessionLimits::default()
            },
            vec![],
        )
        .await;
        let mut flood = Vec::new();
        for _ in 0..8 {
            flood.push(TcpStream::connect(s.control).await.unwrap());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        // when / then: admission is slowed only within the pre-auth deadline
        let (_client, _bulk) = s.enroll_connect(Duration::from_secs(2)).await;
        // and the idle flood streams were closed by the receiver, not retained
        for mut stream in flood {
            let mut byte = [0u8; 1];
            let read = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::io::AsyncReadExt::read(&mut stream, &mut byte),
            )
            .await
            .unwrap();
            assert!(matches!(read, Ok(0) | Err(_)));
        }
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn given_transient_accept_error_should_keep_listening() {
        // given: resource exhaustion and per-connection accept failures
        let s = serve(
            SessionLimits::default(),
            vec![
                io::Error::from(io::ErrorKind::ConnectionAborted),
                io::Error::from_raw_os_error(libc::EMFILE),
                io::Error::from_raw_os_error(libc::ENFILE),
                io::Error::from(io::ErrorKind::Interrupted),
                io::Error::from_raw_os_error(libc::ENOBUFS),
            ],
        )
        .await;
        // when / then: the listener backs off and keeps serving
        let (mut client, _bulk) = s.enroll_connect(Duration::from_secs(3)).await;
        assert!(
            client
                .request::<pu_core::mesh::response::Response>(&s.inventory())
                .await
                .is_ok()
        );
        assert!(!s.serve.is_finished());
        // and an explicit stop still ends it cleanly
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn given_unrecoverable_listener_error_should_stop_with_error() {
        // given: the listening socket itself is no longer valid
        let s = serve(
            SessionLimits::default(),
            vec![io::Error::from_raw_os_error(libc::EBADF)],
        )
        .await;
        // when
        let result = tokio::time::timeout(Duration::from_secs(2), s.serve)
            .await
            .unwrap()
            .unwrap();
        // then
        assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EBADF));
    }
}
