use super::*;
use crate::daemon_lifecycle::mesh_exports::MeshExports;
use std::time::Duration;

#[test]
fn given_invalid_budget_should_reject_before_admission() {
    // given
    let mut limits = SessionLimits::default();
    // when / then
    assert!(limits.validate().is_ok());
    limits.control_per_peer = limits.control_total + 1;
    assert!(limits.validate().is_err());
    limits = SessionLimits::default();
    limits.send_deadline = std::time::Duration::ZERO;
    assert!(limits.validate().is_err());
}

use crate::{
    daemon_lifecycle::{DaemonLock, try_acquire_daemon_lock},
    execution_state::Limits as LedgerLimits,
    mesh_transport::{EnrolledPeers, Identity},
};
use pu_core::mesh::Capability;
use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};
use tokio::io::DuplexStream;

fn admin() -> LocalAdmin {
    LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap()).unwrap()
}
fn grants() -> Vec<ExportGrant> {
    vec![ExportGrant {
        export_id: "export".into(),
        capabilities: BTreeSet::from([Capability::Inspect, Capability::ReadOutput]),
        runtimes: BTreeSet::new(),
    }]
}
struct Fixture {
    _tmp: tempfile::TempDir,
    _lock: Arc<DaemonLock>,
    authority: Arc<SessionAuthority>,
    work: Arc<WorkOwner>,
    server: MeshTransport,
    clients: [MeshTransport; 2],
    identities: [Identity; 2],
    server_id: PeerId,
    generations: [u64; 2],
}
impl Fixture {
    async fn new(limits: SessionLimits, expiry: Option<i64>) -> Self {
        Self::with_anchor(limits, expiry, true).await
    }
    async fn with_anchor(limits: SessionLimits, expiry: Option<i64>, anchored: bool) -> Self {
        tokio::task::spawn_blocking(move || {
            let tmp = tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let lock = Arc::new(
                try_acquire_daemon_lock(&tmp.path().join("scratch.sock"))
                    .unwrap()
                    .unwrap(),
            );
            let work = WorkOwner::new(&lock);
            let mut store = MeshStore::initialize(lock.clone(), &admin()).unwrap();
            let ledger = Ledger::open(
                tmp.path().join("ledger.sqlite"),
                store.daemon_id(),
                LedgerLimits::default(),
            )
            .unwrap();
            let identities = [Identity::generate().unwrap(), Identity::generate().unwrap()];
            let generations = identities.each_ref().map(|id| {
                if anchored {
                    store
                        .enroll(&ledger, &admin(), id.certificate_der(), grants(), expiry)
                        .unwrap()
                        .generation
                } else {
                    ledger
                        .register_peer(&admin(), &peer_name(id.peer_id()))
                        .unwrap()
                        .generation
                }
            });
            let server = MeshTransport::new(
                store.identity(),
                EnrolledPeers::new(identities.iter().map(Identity::certificate_der)).unwrap(),
            )
            .unwrap();
            let server_id = store.identity().peer_id();
            let clients = identities.each_ref().map(|id| {
                MeshTransport::new(
                    id,
                    EnrolledPeers::new([store.identity().certificate_der()]).unwrap(),
                )
                .unwrap()
            });
            let exports = MeshExports::open(lock.clone(), store.daemon_id(), 16).unwrap();
            let authority = SessionAuthority::new(store, ledger, exports, limits).unwrap();
            Fixture {
                _tmp: tmp,
                _lock: lock,
                authority,
                work,
                server,
                clients,
                identities,
                server_id,
                generations,
            }
        })
        .await
        .unwrap()
    }
    async fn control(&self, peer: usize) -> (Session<DuplexStream>, ControlClient<DuplexStream>) {
        let (server, client) = tokio::io::duplex(4096);
        let (server, client) = tokio::join!(
            self.authority.accept(&self.server, server, Role::Control),
            ControlClient::connect(
                &self.clients[peer],
                client,
                self.server_id,
                self.authority.daemon.clone(),
                self.generations[peer],
                self.authority.limits.clone()
            )
        );
        (server.unwrap(), client.unwrap())
    }
    async fn bulk(
        &self,
        control: &ControlClient<DuplexStream>,
    ) -> (Session<DuplexStream>, BulkClient<DuplexStream>) {
        let (server, client) = tokio::io::duplex(4096);
        let (server, client) = tokio::join!(
            self.authority.accept(&self.server, server, Role::Bulk),
            control.connect_bulk(&self.clients[0], client)
        );
        (server.unwrap(), client.unwrap())
    }
    async fn hello(&self, peer: usize, role: Role, hello: Hello) -> Result<Session<DuplexStream>> {
        let (server, client) = tokio::io::duplex(4096);
        let client_task = async {
            let mut connection = self.clients[peer]
                .connect(client, role, self.server_id)
                .await
                .unwrap();
            connection
                .write_frame(&serde_json::to_vec(&hello).unwrap(), Duration::from_secs(1))
                .await
                .unwrap();
            let _ = connection.read_frame(Duration::from_secs(1)).await;
        };
        let (result, _) = tokio::join!(
            self.authority.accept(&self.server, server, role),
            client_task
        );
        result
    }
    fn inventory(&self) -> Request {
        Request {
            request_id: "request".into(),
            target_daemon: self.authority.daemon.clone(),
            operation: wire::Operation::Inventory {},
        }
    }
}

#[tokio::test]
async fn given_independent_bulk_tls_should_reject_cross_peer_and_stale_control_substitution() {
    // given: actual TLS over bounded duplex and canonical private SQLite policy
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, client) = f.control(0).await;
    let hello = Hello::Bulk {
        version: wire::BRIDGE_VERSION,
        target_daemon: f.authority.daemon.clone(),
        generation: f.generations[1],
        incarnation: client.welcome().incarnation.clone(),
        control_binding: client.welcome().control_binding.clone(),
    };
    // when / then: a valid second enrolled certificate cannot borrow a binding
    assert!(matches!(
        f.hello(1, Role::Bulk, hello).await,
        Err(Error::Denied)
    ));
    let old = client.welcome().clone();
    let (mut bulk, _observer) = f.bulk(&client).await;
    let (replacement, _new_client) = f.control(0).await;
    assert_ne!(replacement.welcome.control_binding, old.control_binding);
    assert!(control.binding.check().is_err());
    assert!(matches!(bulk.wait_bulk_close().await, Err(Error::Closed)));
    let stale = Hello::Bulk {
        version: 1,
        target_daemon: old.daemon_id,
        generation: old.generation,
        incarnation: old.incarnation,
        control_binding: old.control_binding,
    };
    assert!(matches!(
        f.hello(0, Role::Bulk, stale).await,
        Err(Error::Denied)
    ));
    control.close();
}

#[tokio::test]
async fn given_bad_role_version_target_generation_incarnation_should_reject_without_binding() {
    // given
    let f = Fixture::new(SessionLimits::default(), None).await;
    let base = Hello::Control {
        version: 1,
        target_daemon: f.authority.daemon.clone(),
        generation: f.generations[0],
    };
    // when / then
    let cases = [
        (Role::Bulk, base.clone()),
        (
            Role::Control,
            Hello::Control {
                version: 2,
                target_daemon: f.authority.daemon.clone(),
                generation: f.generations[0],
            },
        ),
        (
            Role::Control,
            Hello::Control {
                version: 1,
                target_daemon: "wrong-daemon".into(),
                generation: f.generations[0],
            },
        ),
        (
            Role::Control,
            Hello::Control {
                version: 1,
                target_daemon: f.authority.daemon.clone(),
                generation: f.generations[0] + 1,
            },
        ),
        (
            Role::Bulk,
            Hello::Bulk {
                version: 1,
                target_daemon: f.authority.daemon.clone(),
                generation: f.generations[0],
                incarnation: "old-boot".into(),
                control_binding: "old-control".into(),
            },
        ),
    ];
    for (role, hello) in cases {
        assert!(f.hello(0, role, hello).await.is_err());
    }
    let (control, _) = f.control(0).await;
    assert!(control.binding.check().is_ok());
}

#[tokio::test]
async fn given_stalled_bulk_should_preserve_control_latency_and_hold_revoke_barrier_through_bounded_write()
 {
    // given: bulk TLS flow control blocks against a 4KiB duplex
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, mut client) = f.control(0).await;
    let (mut bulk, _stalled) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    let frame = vec![b'x'; wire::MAX_BULK_FRAME];
    let send = tokio::spawn(async move {
        bulk.send_output(&subscription, &[0; 32 * 1024], &frame)
            .await
    });
    // Wait deterministically for shared authority ownership, not just scheduling.
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if f.authority.state.try_write().is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let start = std::time::Instant::now();
    let request = f.inventory();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| Ok(
            br#"{"ok":true}"#.to_vec()
        ))
    );
    assert!(dispatch.is_ok());
    assert_eq!(response.unwrap()["ok"], true);
    let latency = start.elapsed();
    eprintln!(
        "bounded-duplex stalled-bulk control roundtrip: {latency:?}; macOS {}",
        std::env::consts::ARCH
    );
    assert!(latency <= Duration::from_secs(1));
    let authority = f.authority.clone();
    let work = f.work.clone();
    let peer = f.identities[0].peer_id();
    let revoke = tokio::spawn(async move { authority.revoke(&work, admin(), peer).await });
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(
        !revoke.is_finished(),
        "revoke must await the held bounded send barrier"
    );
    assert!(matches!(
        send.await.unwrap(),
        Err(Error::Transport(crate::mesh_transport::Error::Timeout))
    ));
    assert!(revoke.await.unwrap().unwrap().revoked);
    assert!(matches!(
        control
            .dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
            .await,
        Err(Error::Closed)
    ));
}

#[tokio::test]
async fn given_live_authenticated_stream_should_recheck_expiry_and_generation_and_wake_idle_reads_on_revoke()
 {
    // given
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, mut client) = f.control(0).await;
    let (mut bulk, _observer) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    // when: canonical update durably advances generation and fences old sessions
    let updated = f
        .authority
        .update_policy(
            &f.work,
            admin(),
            f.identities[0].certificate_der().to_vec(),
            f.generations[0],
            grants(),
            Some(now()),
        )
        .await
        .unwrap();
    assert!(updated.generation > f.generations[0]);
    assert!(bulk.send_output(&subscription, b"x", b"x").await.is_err());
    let request = f.inventory();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
    );
    assert!(response.is_err());
    assert!(dispatch.is_err());
    let expired = Hello::Control {
        version: 1,
        target_daemon: f.authority.daemon.clone(),
        generation: updated.generation,
    };
    assert!(matches!(
        f.hello(0, Role::Control, expired).await,
        Err(Error::Denied)
    ));
    // independent live peer's idle control read is woken without holding authority
    let (mut idle, _client) = f.control(1).await;
    let work = f.work.clone();
    let waiter = tokio::spawn(async move {
        idle.dispatch(&work, |_, _exports, _, _, _| unreachable!())
            .await
    });
    tokio::task::yield_now().await;
    f.authority
        .revoke(&f.work, admin(), f.identities[1].peer_id())
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Closed)
    ));
}

#[tokio::test]
async fn given_cancelled_idle_control_read_should_close_binding_before_bulk_can_send() {
    // given
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, client) = f.control(0).await;
    let (mut bulk, _observer) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    // when: cancellation retains the Session value, not just a dropped task
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            control.dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
        )
        .await
        .is_err()
    );
    // then: a poisoned control stream must never continue lending authority
    assert!(matches!(
        bulk.send_output(&subscription, b"x", b"x").await,
        Err(Error::Closed)
    ));
}

#[tokio::test]
async fn given_cancelled_waiter_should_retain_blocking_effect_authority_and_admission_until_actual_completion()
 {
    // given: a real accepted request whose blocking effect has already started
    let limits = SessionLimits {
        control_total: 1,
        control_per_peer: 1,
        ..SessionLimits::default()
    };
    let f = Fixture::new(limits, None).await;
    let (mut control, mut client) = f.control(0).await;
    let request = f.inventory();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let work = f.work.clone();
    let dispatch = tokio::spawn(async move {
        control
            .dispatch(&work, move |ledger, _exports, caller, _, _| {
                ledger.validate_principal(caller).unwrap();
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(br#"{"ok":true}"#.to_vec())
            })
            .await
    });
    let client_request =
        tokio::spawn(async move { client.request::<serde_json::Value>(&request).await });
    entered_rx.await.unwrap();
    // when: transport waiter is cancelled while spawn_blocking cannot stop
    dispatch.abort();
    let _ = dispatch.await;
    assert_eq!(f.authority.control.available_permits(), 0);
    let authority = f.authority.clone();
    let work = f.work.clone();
    let peer = f.identities[0].peer_id();
    let revoke = tokio::spawn(async move { authority.revoke(&work, admin(), peer).await });
    tokio::time::sleep(Duration::from_millis(25)).await;
    let was_held = !revoke.is_finished() && f.authority.state.try_write().is_err();
    release_tx.send(()).unwrap(); // always release before asserting
    assert!(
        was_held,
        "actual blocking closure retains the authority guard"
    );
    assert!(revoke.await.unwrap().unwrap().revoked);
    f.work.wait_idle().await;
    assert_eq!(f.authority.control.available_permits(), 1);
    assert!(client_request.await.unwrap().is_err());
}

#[tokio::test]
async fn given_subscription_budgets_should_limit_concurrent_admission_and_release_on_drop() {
    // given
    let limits = SessionLimits {
        subscriptions_total: 2,
        subscriptions_per_peer: 1,
        ..SessionLimits::default()
    };
    let f = Fixture::new(limits, None).await;
    let (control, client) = f.control(0).await;
    let (mut bulk, _observer) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    // when / then
    assert!(matches!(
        bulk.subscribe("export".into()).await,
        Err(Error::Capacity)
    ));
    drop(subscription);
    drop(bulk);
    let (mut replacement_bulk, _observer) = f.bulk(&client).await;
    assert!(replacement_bulk.subscribe("export".into()).await.is_ok());
    drop(replacement_bulk);
    drop(control);
    assert_eq!(
        f.authority.bulk.available_permits(),
        f.authority.limits.bulk_total
    );
    assert_eq!(f.authority.registry.lock().unwrap().subscriptions, 0);
}

#[tokio::test]
async fn given_concurrent_tls_admissions_should_bound_total_and_peer_connections_and_release() {
    // given: all attempts authenticate through actual TLS before per-peer binding
    let limits = SessionLimits {
        control_total: 4,
        control_per_peer: 2,
        ..SessionLimits::default()
    };
    let f = Arc::new(Fixture::new(limits, None).await);
    let mut attempts = Vec::new();
    for _ in 0..16 {
        let f = f.clone();
        attempts.push(tokio::spawn(async move {
            let (server, client) = tokio::io::duplex(4096);
            tokio::join!(
                f.authority.accept(&f.server, server, Role::Control),
                ControlClient::connect(
                    &f.clients[0],
                    client,
                    f.server_id,
                    f.authority.daemon.clone(),
                    f.generations[0],
                    f.authority.limits.clone()
                )
            )
        }));
    }
    // when / then: successful results keep leases while all other attempts finish
    let mut sessions = Vec::new();
    for attempt in attempts {
        let (server, client) = attempt.await.unwrap();
        if let Ok(server) = server {
            sessions.push((server, client.unwrap()));
        }
    }
    // Concurrent replacement may close an earlier handshake before Welcome.
    assert!((1..=2).contains(&sessions.len()));
    assert_eq!(f.authority.control.available_permits(), 4 - sessions.len());
    assert_eq!(f.authority.registry.lock().unwrap().counts.len(), 1);
    sessions.clear();
    assert_eq!(f.authority.control.available_permits(), 4);
    assert_eq!(f.authority.registry.lock().unwrap().counts.len(), 0);
    // Sequential live values demonstrate the exact per-peer admission boundary.
    let first = f.control(0).await;
    let second = f.control(0).await;
    let (io, peer) = tokio::io::duplex(4096);
    let (server, _client) = tokio::join!(
        f.authority.accept(&f.server, io, Role::Control),
        ControlClient::connect(
            &f.clients[0],
            peer,
            f.server_id,
            f.authority.daemon.clone(),
            f.generations[0],
            f.authority.limits.clone()
        )
    );
    assert!(matches!(server, Err(Error::Capacity)));
    assert_eq!(f.authority.control.available_permits(), 2);
    drop(first);
    drop(second);
    assert_eq!(f.authority.control.available_permits(), 4);
}

#[tokio::test]
async fn given_expiry_after_authentication_should_deny_request_and_output_on_existing_streams() {
    // given: no policy update/revocation closes these already authenticated streams
    let f = Fixture::new(SessionLimits::default(), Some(now() + 2)).await;
    let (mut control, mut client) = f.control(0).await;
    let (mut bulk, _observer) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    // when / then: current receiver time is fenced in real SQLite on every use
    assert!(matches!(
        bulk.send_output(&subscription, b"x", b"x").await,
        Err(Error::Denied)
    ));
    let request = f.inventory();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
    );
    assert!(response.is_err());
    assert!(matches!(dispatch, Err(Error::Denied)));
}

#[tokio::test]
async fn given_real_tcp_backpressure_should_keep_control_responsive_and_bound_bulk_write() {
    use std::os::fd::AsRawFd;
    use tokio::net::{TcpListener, TcpSocket};
    // given: scratch loopback only, bounded kernel buffers, independent TLS ALPN
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, mut client) = f.control(0).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(1024).unwrap();
    let (accepted, connected) = tokio::join!(
        listener.accept(),
        socket.connect(listener.local_addr().unwrap())
    );
    let (server_io, _) = accepted.unwrap();
    let client_io = connected.unwrap();
    let size: libc::c_int = 1024;
    // Safe: scratch socket and pointer to live c_int; no user socket touched.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                server_io.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&size as *const libc::c_int).cast(),
                std::mem::size_of_val(&size) as libc::socklen_t,
            )
        },
        0
    );
    let (server, observer) = tokio::join!(
        f.authority.accept(&f.server, server_io, Role::Bulk),
        client.connect_bulk(&f.clients[0], client_io)
    );
    let mut bulk = server.unwrap();
    let _stalled = observer.unwrap();
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    let send = tokio::spawn(async move {
        // A finite series also covers platforms that initially buffer one frame.
        let frame = vec![b'x'; wire::MAX_BULK_FRAME];
        for _ in 0..16 {
            bulk.send_output(&subscription, &[0; 32 * 1024], &frame)
                .await?;
        }
        Ok(())
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !send.is_finished(),
        "real TCP receiver must impose backpressure"
    );
    let request = f.inventory();
    let start = std::time::Instant::now();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| Ok(
            br#"{"ok":true}"#.to_vec()
        ))
    );
    assert!(dispatch.is_ok());
    assert_eq!(response.unwrap()["ok"], true);
    let measured = start.elapsed();
    eprintln!(
        "real loopback TCP stalled-bulk control roundtrip: {measured:?}; macOS {}",
        std::env::consts::ARCH
    );
    assert!(measured <= Duration::from_secs(1));
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), send)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Transport(crate::mesh_transport::Error::Timeout))
    ));
}

#[tokio::test]
async fn given_output_producer_should_lookup_and_copy_under_retained_authority() {
    // given
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (_control, client) = f.control(0).await;
    let (mut bulk, mut observer) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    let authority = f.authority.clone();
    // when: parent resource lookup is performed inside the actual send producer
    let producer = bulk.send_output_with(&subscription, move |ledger, _exports, caller| {
        assert!(authority.state.try_write().is_err());
        ledger.validate_principal(caller).unwrap();
        // Parent must additionally check exact export generation and agent/run.
        Ok((1, br#"{"data":"x"}"#.to_vec()))
    });
    let (sent, observed) = tokio::join!(producer, observer.read::<serde_json::Value>());
    // then
    assert!(sent.is_ok());
    assert_eq!(observed.unwrap()["data"], "x");
}

#[tokio::test]
async fn given_unanchored_or_mismatched_canonical_policy_should_deny_authenticated_connections_and_existing_streams()
 {
    // given: enrolled TLS pin alone is not a grant or durable policy anchor
    let unanchored = Fixture::with_anchor(SessionLimits::default(), None, false).await;
    let hello = Hello::Control {
        version: 1,
        target_daemon: unanchored.authority.daemon.clone(),
        generation: unanchored.generations[0],
    };
    assert!(matches!(
        unanchored.hello(0, Role::Control, hello).await,
        Err(Error::Denied)
    ));
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, mut client) = f.control(0).await;
    let (mut bulk, _observer) = f.bulk(&client).await;
    let subscription = bulk.subscribe("export".into()).await.unwrap();
    // when: simulate a torn policy publication without updating the SQL anchor
    let path = f._tmp.path().join(format!(
        "scratch.sock.mesh-peer-{}.json",
        peer_name(f.identities[0].peer_id())
    ));
    tokio::task::spawn_blocking(move || {
        let mut policy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        policy["exports"] = serde_json::json!([]);
        std::fs::write(path, serde_json::to_vec(&policy).unwrap()).unwrap();
    })
    .await
    .unwrap();
    // then: valid TLS and generation still fail on every request/send
    assert!(matches!(
        bulk.send_output(&subscription, b"x", b"x").await,
        Err(Error::Denied)
    ));
    let request = f.inventory();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
    );
    assert!(response.is_err());
    assert!(matches!(dispatch, Err(Error::Denied)));
}

#[tokio::test]
async fn given_cancelled_admin_waiter_should_complete_durable_revocation_before_releasing_exclusive_guard()
 {
    // given
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (control, _client) = f.control(0).await;
    let peer = f.identities[0].peer_id();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let authority = f.authority.clone();
    let work = f.work.clone();
    let waiter = tokio::spawn(async move {
        authority
            .administer(&work, admin(), peer, move |_, ledger, admin| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                ledger
                    .revoke_peer(admin, &peer_name(peer))
                    .map_err(|_| Error::Operation)
            })
            .await
    });
    entered_rx.await.unwrap();
    // when
    waiter.abort();
    let _ = waiter.await;
    let exclusive_was_retained = f.authority.state.try_read().is_err();
    assert!(
        control.binding.check().is_ok(),
        "not closed before durable change completes"
    );
    release_tx.send(()).unwrap();
    assert!(exclusive_was_retained);
    f.work.wait_idle().await;
    assert!(matches!(control.binding.check(), Err(Error::Closed)));
    let guard = f.authority.state.clone().read_owned().await;
    let revoked =
        tokio::task::spawn_blocking(move || guard.ledger.peer(&peer_name(peer)).unwrap().revoked)
            .await
            .unwrap();
    assert!(revoked);
}

#[tokio::test]
async fn given_control_tls_disconnect_should_wake_bulk_without_releasing_worker_execution() {
    // given: synthetic ledger run only; no processes/PTYs launched or signalled
    use crate::execution_state::{
        LaunchIntent, Lifecycle, OperationId, OperationKey, PayloadDigest, SupervisionIdentity,
        WorktreeIntent,
    };
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, client) = f.control(0).await;
    let peer = control.binding.peer;
    let generation = control.binding.generation;
    let recorded = f
        .authority
        .administer(&f.work, admin(), peer, move |_, ledger, _| {
            let caller = Caller::authenticated_peer(peer_name(peer), generation).unwrap();
            let key = OperationKey {
                window: ledger.issue_window(&caller, now(), 60).unwrap().id,
                id: OperationId::new("launch").unwrap(),
                digest: PayloadDigest([7; 32]),
            };
            let intent = LaunchIntent {
                project: "project".into(),
                export: Some("export".into()),
                runtime: "codex".into(),
                source_commit: "a".repeat(40),
                worktree: WorktreeIntent {
                    id: "worktree".into(),
                    path: "/scratch/worktree".into(),
                    branch: "pu/scratch".into(),
                },
            };
            let receipt = ledger
                .admit_launch(&caller, &key, &intent, None, now())
                .unwrap()
                .value;
            ledger.begin_start(&caller, &receipt.guard).unwrap();
            ledger
                .record_running(
                    &receipt.run_id,
                    &SupervisionIdentity {
                        mechanism: "synthetic-test".into(),
                        token: "no-real-process".into(),
                    },
                )
                .unwrap();
            Ok(receipt)
        })
        .await
        .unwrap();
    // Admin always fences existing streams, so establish a fresh control.
    control.close();
    drop(client);
    let (mut control, client) = f.control(0).await;
    let (mut bulk, _observer) = f.bulk(&client).await;
    drop(client);
    // when / then: actual TLS EOF closes binding and wakes the idle bulk read
    assert!(
        control
            .dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
            .await
            .is_err()
    );
    assert!(matches!(bulk.wait_bulk_close().await, Err(Error::Closed)));
    f.authority.revoke(&f.work, admin(), peer).await.unwrap();
    let state = f.authority.state.clone().read_owned().await;
    tokio::task::spawn_blocking(move || {
        let run = state.ledger.run(&recorded.run_id).unwrap();
        let agent = state.ledger.agent(&recorded.guard.agent_id).unwrap();
        assert_eq!(run.lifecycle, Lifecycle::Running);
        assert!(run.lifecycle.holds_capacity());
        assert_eq!(agent.controller, None);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn given_cancelled_handshake_validation_should_retain_role_permit_in_actual_blocking_closure()
{
    // given: TLS completed; real store/SQLite validation is gated in its owner
    let f = Arc::new(
        Fixture::new(
            SessionLimits {
                control_total: 1,
                control_per_peer: 1,
                ..SessionLimits::default()
            },
            None,
        )
        .await,
    );
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    *f.authority.validation_gate.lock().unwrap() = Some(ValidationGate {
        entered: entered_tx,
        release: release_rx,
    });
    let (server_io, client_io) = tokio::io::duplex(4096);
    let server_f = f.clone();
    let server = tokio::spawn(async move {
        server_f
            .authority
            .accept(&server_f.server, server_io, Role::Control)
            .await
    });
    let client_f = f.clone();
    let client = tokio::spawn(async move {
        ControlClient::connect(
            &client_f.clients[0],
            client_io,
            client_f.server_id,
            client_f.authority.daemon.clone(),
            client_f.generations[0],
            client_f.authority.limits.clone(),
        )
        .await
    });
    entered_rx.await.unwrap();
    // when: cancelling the TLS waiter cannot cancel already-running blocking work
    server.abort();
    let _ = server.await;
    let was_retained = f.authority.control.available_permits() == 0;
    let mut rejected = 0;
    for _ in 0..32 {
        let (io, _peer) = tokio::io::duplex(4096);
        if matches!(
            tokio::time::timeout(
                Duration::from_millis(20),
                f.authority.accept(&f.server, io, Role::Control)
            )
            .await,
            Ok(Err(Error::Capacity))
        ) {
            rejected += 1;
        }
    }
    // No new handshakes or blocking checks were admitted beyond that role permit.
    assert_eq!(
        f.authority.bulk.available_permits(),
        f.authority.limits.bulk_total
    );
    release_tx.send(()).unwrap();
    assert!(was_retained);
    assert_eq!(rejected, 32);
    assert!(client.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.authority.control.available_permits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.authority.control.available_permits(), 1);
    assert_eq!(f.authority.registry.lock().unwrap().counts.len(), 0);
}

#[tokio::test]
async fn given_inspect_only_grant_should_allow_input_receipt_reconciliation_without_input_authority()
 {
    // given: actual TLS with Inspect and ReadOutput, no Control/Input grant
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (mut control, mut client) = f.control(0).await;
    let request = Request {
        request_id: "reconcile".into(),
        target_daemon: f.authority.daemon.clone(),
        operation: wire::Operation::InputReceipt {
            target: wire::AgentTarget {
                export: wire::Export {
                    id: "export".into(),
                    generation: 1,
                },
                agent_id: "agent".into(),
                run_id: "run".into(),
            },
            writer: 1,
            sequence: 1,
        },
    };
    // when: inspect-only reconciliation crosses the guarded typed dispatcher
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |ledger, _exports, caller, _, request| {
            ledger.validate_principal(caller).unwrap();
            assert!(matches!(
                request.operation,
                wire::Operation::InputReceipt { .. }
            ));
            // Mapping test only: no acknowledgement or actual writer is asserted.
            // Parent must check exact writer/agent/run/export and stable ownership.
            Ok(br#"{"receipt":"unknown"}"#.to_vec())
        })
    );
    // then: reconciliation requires no Input permission and does not reopen/replay
    assert!(dispatch.is_ok());
    assert_eq!(response.unwrap()["receipt"], "unknown");
    let mut output_only = grants();
    output_only[0].capabilities.remove(&Capability::Inspect);
    let updated = f
        .authority
        .update_policy(
            &f.work,
            admin(),
            f.identities[0].certificate_der().to_vec(),
            f.generations[0],
            output_only,
            None,
        )
        .await
        .unwrap();
    let (server_io, client_io) = tokio::io::duplex(4096);
    let (server, client) = tokio::join!(
        f.authority.accept(&f.server, server_io, Role::Control),
        ControlClient::connect(
            &f.clients[0],
            client_io,
            f.server_id,
            f.authority.daemon.clone(),
            updated.generation,
            f.authority.limits.clone()
        )
    );
    let mut control = server.unwrap();
    let mut client = client.unwrap();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| unreachable!())
    );
    assert!(response.is_err());
    assert!(matches!(dispatch, Err(Error::Denied)));
}

#[tokio::test]
async fn given_control_endpoint_discovery_should_return_only_current_generation_without_creating_or_replacing_binding()
 {
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (control, mut client) = f.control(0).await;
    let original = control.welcome().clone();
    let (server_io, client_io) = tokio::io::duplex(4096);
    let (accepted, discovered) = tokio::join!(
        f.authority.accept_incoming(&f.server, server_io),
        ControlClient::discover(
            &f.clients[0],
            client_io,
            f.server_id,
            f.authority.daemon.clone(),
            f.authority.limits.clone()
        )
    );
    assert!(matches!(accepted, Ok(Accepted::Discovery)));
    let discovered = discovered.unwrap();
    assert_eq!(discovered.generation, f.generations[0]);
    assert_eq!(discovered.daemon_id, f.authority.daemon);
    assert_eq!(control.welcome(), &original);
    assert!(control.binding.check().is_ok());
    assert_eq!(f.authority.control.available_permits(), 7);
    assert_eq!(f.authority.bulk.available_permits(), 8);
    let request = f.inventory();
    let mut control = control;
    let (response, dispatched) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, |_, _exports, _, _, _| Ok(
            br#"{"ok":true}"#.to_vec()
        ))
    );
    assert!(dispatched.is_ok());
    assert_eq!(response.unwrap()["ok"], true);
}

impl Fixture {
    async fn reopen(self) -> Self {
        self.work.wait_idle().await;
        tokio::task::spawn_blocking(move || {
            let Fixture {
                _tmp,
                _lock,
                authority,
                work,
                server,
                clients,
                identities,
                server_id,
                ..
            } = self;
            let authority = Arc::try_unwrap(authority)
                .ok()
                .expect("no live old sessions or operations");
            let limits = authority.limits;
            let AuthorityState {
                store,
                ledger,
                exports,
            } = Arc::try_unwrap(authority.state).ok().unwrap().into_inner();
            drop(ledger);
            let ledger = Ledger::open(
                _tmp.path().join("ledger.sqlite"),
                store.daemon_id(),
                LedgerLimits::default(),
            )
            .unwrap();
            let generations = identities
                .each_ref()
                .map(|id| ledger.peer(&peer_name(id.peer_id())).unwrap().generation);
            let authority = SessionAuthority::new(store, ledger, exports, limits).unwrap();
            Self {
                _tmp,
                _lock,
                authority,
                work,
                server,
                clients,
                identities,
                server_id,
                generations,
            }
        })
        .await
        .unwrap()
    }
    async fn discover(
        &self,
        peer: usize,
        target: String,
    ) -> (Result<Accepted<DuplexStream>>, Result<GenerationDiscovery>) {
        let (server, client) = tokio::io::duplex(4096);
        tokio::join!(
            self.authority.accept_incoming(&self.server, server),
            ControlClient::discover(
                &self.clients[peer],
                client,
                self.server_id,
                target,
                self.authority.limits.clone()
            )
        )
    }
}

#[tokio::test]
async fn given_sqlite_restart_should_discover_fresh_generation_and_read_receipts_without_restoring_old_bindings_windows_or_writers()
 {
    use crate::execution_state::{
        InputOutcome, LaunchIntent, Lifecycle, OperationId, OperationKey, PayloadDigest,
        SupervisionIdentity, WorktreeIntent,
    };
    let f = Fixture::new(SessionLimits::default(), None).await;
    let peer = f.identities[0].peer_id();
    let generation = f.generations[0];
    // Only synthetic ledger records; no process/PTY or bytes are ever dispatched.
    let (key, intent, launch, writer) = f
        .authority
        .administer(&f.work, admin(), peer, move |_, ledger, _| {
            let caller = Caller::authenticated_peer(peer_name(peer), generation).unwrap();
            let key = OperationKey {
                window: ledger.issue_window(&caller, now(), 60).unwrap().id,
                id: OperationId::new("original-operation").unwrap(),
                digest: PayloadDigest([7; 32]),
            };
            let intent = LaunchIntent {
                project: "project".into(),
                export: Some("export".into()),
                runtime: "codex".into(),
                source_commit: "a".repeat(40),
                worktree: WorktreeIntent {
                    id: "worktree".into(),
                    path: "/scratch/worktree".into(),
                    branch: "pu/scratch".into(),
                },
            };
            let launch = ledger
                .admit_launch(&caller, &key, &intent, None, now())
                .unwrap()
                .value;
            ledger.begin_start(&caller, &launch.guard).unwrap();
            let agent = ledger
                .record_running(
                    &launch.run_id,
                    &SupervisionIdentity {
                        mechanism: "synthetic-test".into(),
                        token: "no-process".into(),
                    },
                )
                .unwrap();
            let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
            ledger
                .accept_input(
                    &caller,
                    &agent.guard(),
                    writer.id,
                    1,
                    PayloadDigest([8; 32]),
                    1,
                )
                .unwrap();
            Ok((key, intent, launch, writer))
        })
        .await
        .unwrap();
    let (control, client) = f.control(0).await;
    let (bulk, observer) = f.bulk(&client).await;
    let old = control.welcome().clone();
    drop((control, client, bulk, observer));
    let f = f.reopen().await;
    assert!(f.generations[0] > old.generation);
    assert!(matches!(
        f.hello(
            0,
            Role::Control,
            Hello::Control {
                version: 1,
                target_daemon: f.authority.daemon.clone(),
                generation: old.generation
            }
        )
        .await,
        Err(Error::Denied)
    ));
    let (accepted, discovered) = f.discover(0, f.authority.daemon.clone()).await;
    assert!(matches!(accepted, Ok(Accepted::Discovery)));
    let discovered = discovered.unwrap();
    assert_eq!(discovered.generation, f.generations[0]);
    assert_eq!(f.authority.registry.lock().unwrap().controls.len(), 0);
    assert_eq!(f.authority.registry.lock().unwrap().counts.len(), 0);
    assert!(matches!(
        f.hello(
            0,
            Role::Bulk,
            Hello::Bulk {
                version: 1,
                target_daemon: old.daemon_id.clone(),
                generation: discovered.generation,
                incarnation: old.incarnation.clone(),
                control_binding: old.control_binding.clone()
            }
        )
        .await,
        Err(Error::Denied)
    ));
    // Fresh strict Control+Bulk on separate new streams, no automatic mutation.
    let (server, io) = tokio::io::duplex(4096);
    let (control, client) = tokio::join!(
        f.authority.accept_incoming(&f.server, server),
        ControlClient::connect(
            &f.clients[0],
            io,
            f.server_id,
            discovered.daemon_id,
            discovered.generation,
            f.authority.limits.clone()
        )
    );
    let mut control = match control.unwrap() {
        Accepted::Session(session) => session,
        Accepted::Discovery => panic!("strict Control must create session"),
    };
    let mut client = client.unwrap();
    assert_ne!(control.welcome.incarnation, old.incarnation);
    assert_ne!(control.welcome.control_binding, old.control_binding);
    let (bulk, observer) = f.bulk(&client).await;
    drop((bulk, observer));
    let request = Request {
        request_id: "inspect-old-input".into(),
        target_daemon: f.authority.daemon.clone(),
        operation: wire::Operation::InputReceipt {
            target: wire::AgentTarget {
                export: wire::Export {
                    id: "export".into(),
                    generation: 1,
                },
                agent_id: writer.agent_id.0.clone(),
                run_id: writer.run_id.0.clone(),
            },
            writer: writer.id.0,
            sequence: 1,
        },
    };
    let writer_id = writer.id;
    let old_key = key.clone();
    let old_intent = intent.clone();
    let old_guard = launch.guard.clone();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, move |ledger, _exports, caller, _, request| {
            let wire::Operation::InputReceipt {
                target,
                writer,
                sequence,
            } = &request.operation
            else {
                panic!()
            };
            let stored = ledger.writer(writer_id).unwrap();
            assert_eq!(*writer, writer_id.0);
            assert_eq!(target.agent_id, stored.agent_id.0);
            assert_eq!(target.run_id, stored.run_id.0);
            match (&stored.principal, caller.principal()) {
                (
                    crate::execution_state::Principal::Peer { id: stored, .. },
                    crate::execution_state::Principal::Peer { id: current, .. },
                ) => assert_eq!(stored, current),
                _ => panic!("receipt is owned by the current stable TLS peer"),
            }
            let run = ledger.run(&stored.run_id).unwrap();
            assert_eq!(run.intent.project, "project");
            assert_eq!(
                run.intent.export.as_deref(),
                Some(target.export.id.as_str())
            );
            assert_eq!(run.lifecycle, Lifecycle::Unknown);
            assert!(!stored.live);
            assert_eq!(
                ledger
                    .input_receipt(writer_id, *sequence)
                    .unwrap()
                    .unwrap()
                    .outcome,
                InputOutcome::Unknown
            );
            let fresh_on_closed_window = OperationKey {
                window: old_key.window,
                id: OperationId::new("new-operation").unwrap(),
                digest: PayloadDigest([9; 32]),
            };
            assert!(
                ledger
                    .admit_launch(caller, &fresh_on_closed_window, &old_intent, None, now())
                    .is_err()
            );
            assert!(ledger.validate_dispatch(caller, &old_guard).is_err());
            assert!(
                ledger
                    .accept_input(caller, &old_guard, writer_id, 2, PayloadDigest([9; 32]), 1)
                    .is_err()
            );
            Ok(br#"{"outcome":"unknown"}"#.to_vec())
        })
    );
    assert!(dispatch.is_ok());
    assert_eq!(response.unwrap()["outcome"], "unknown");
    let request = Request {
        request_id: "inspect-old-operation".into(),
        target_daemon: f.authority.daemon.clone(),
        operation: wire::Operation::Receipt {
            export: wire::Export {
                id: "export".into(),
                generation: 1,
            },
            mutation: wire::Mutation {
                window: key.window.0,
                id: key.id.0.clone(),
            },
            payload_digest: key.digest.0,
        },
    };
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, move |ledger, _exports, caller, _, _| {
            assert!(ledger.operation_receipt(caller, &key).unwrap().is_some());
            assert_eq!(
                ledger.run(&launch.run_id).unwrap().lifecycle,
                Lifecycle::Unknown
            );
            Ok(br#"{"found":true}"#.to_vec())
        })
    );
    assert!(dispatch.is_ok());
    assert_eq!(response.unwrap()["found"], true);
}

#[tokio::test]
async fn given_discovery_should_deny_wrong_target_pin_role_version_and_inactive_or_damaged_policy()
{
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (accepted, client) = f.discover(0, "wrong-daemon".into()).await;
    assert!(matches!(accepted, Err(Error::Denied)));
    assert!(client.is_err());
    let (server, client) = tokio::io::duplex(4096);
    let (accepted, client) = tokio::join!(
        f.authority.accept_incoming(&f.server, server),
        ControlClient::discover(
            &f.clients[0],
            client,
            f.identities[1].peer_id(),
            f.authority.daemon.clone(),
            f.authority.limits.clone()
        )
    );
    assert!(accepted.is_err());
    assert!(matches!(
        client,
        Err(Error::Transport(crate::mesh_transport::Error::Pin))
    ));
    assert!(matches!(
        f.hello(
            0,
            Role::Bulk,
            Hello::Discover {
                version: 1,
                target_daemon: f.authority.daemon.clone()
            }
        )
        .await,
        Err(Error::Denied)
    ));
    // Strict session API never silently upgrades discovery into a control session.
    assert!(matches!(
        f.hello(
            0,
            Role::Control,
            Hello::Discover {
                version: 1,
                target_daemon: f.authority.daemon.clone()
            }
        )
        .await,
        Err(Error::Denied)
    ));
    let unanchored = Fixture::with_anchor(SessionLimits::default(), None, false).await;
    assert!(matches!(
        unanchored
            .discover(0, unanchored.authority.daemon.clone())
            .await
            .0,
        Err(Error::Denied)
    ));
    let expired = Fixture::new(SessionLimits::default(), Some(now())).await;
    assert!(matches!(
        expired
            .discover(0, expired.authority.daemon.clone())
            .await
            .0,
        Err(Error::Denied)
    ));
    f.authority
        .revoke(&f.work, admin(), f.identities[0].peer_id())
        .await
        .unwrap();
    assert!(matches!(
        f.discover(0, f.authority.daemon.clone()).await.0,
        Err(Error::Denied)
    ));
    let path = f._tmp.path().join(format!(
        "scratch.sock.mesh-peer-{}.json",
        peer_name(f.identities[1].peer_id())
    ));
    tokio::task::spawn_blocking(move || std::fs::write(path, b"{}").unwrap())
        .await
        .unwrap();
    assert!(matches!(
        f.discover(1, f.authority.daemon.clone()).await.0,
        Err(Error::Denied)
    ));
}

#[tokio::test]
async fn given_cancelled_discovery_validation_should_retain_transient_admission_without_creating_bindings()
 {
    let f = Arc::new(
        Fixture::new(
            SessionLimits {
                control_total: 1,
                control_per_peer: 1,
                ..SessionLimits::default()
            },
            None,
        )
        .await,
    );
    let (entered, began) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    *f.authority.validation_gate.lock().unwrap() = Some(ValidationGate {
        entered,
        release: wait,
    });
    let (server, client) = tokio::io::duplex(4096);
    let task_f = f.clone();
    let accepted = tokio::spawn(async move {
        task_f
            .authority
            .accept_incoming(&task_f.server, server)
            .await
    });
    let task_f = f.clone();
    let client = tokio::spawn(async move {
        ControlClient::discover(
            &task_f.clients[0],
            client,
            task_f.server_id,
            task_f.authority.daemon.clone(),
            task_f.authority.limits.clone(),
        )
        .await
    });
    began.await.unwrap();
    accepted.abort();
    let _ = accepted.await;
    let retained = f.authority.control.available_permits() == 0;
    let (io, _peer) = tokio::io::duplex(4096);
    let rejected = matches!(
        f.authority.accept_incoming(&f.server, io).await,
        Err(Error::Capacity)
    );
    release.send(()).unwrap();
    assert!(retained && rejected);
    assert!(client.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.authority.control.available_permits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.authority.registry.lock().unwrap().controls.len(), 0);
    assert_eq!(f.authority.registry.lock().unwrap().counts.len(), 0);
}

#[tokio::test]
async fn given_discovery_tls_frames_should_reject_bad_version_and_typed_client_response_substitution()
 {
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (server, client) = tokio::io::duplex(4096);
    let malicious = async {
        let mut client = f.clients[0]
            .connect(client, Role::Control, f.server_id)
            .await
            .unwrap();
        client
            .write_frame(
                br#"{"role":"discover","version":2,"target_daemon":"worker"}"#,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        let _ = client.read_frame(Duration::from_secs(1)).await;
    };
    let (accepted, _) = tokio::join!(f.authority.accept_incoming(&f.server, server), malicious);
    assert!(matches!(
        accepted,
        Err(Error::Wire(wire::WireError::Version))
    ));
    for response in [
        serde_json::to_vec(&serde_json::json!({"version":1,"daemon_id":"other-receiver","generation":f.generations[0]})).unwrap(),
        serde_json::to_vec(&serde_json::json!({"version":2,"daemon_id":f.authority.daemon,"generation":f.generations[0]})).unwrap(),
        serde_json::to_vec(&serde_json::json!({"version":1,"daemon_id":f.authority.daemon,"generation":0})).unwrap(),
        vec![b' '; 2049],
    ] {
        let (server, client) = tokio::io::duplex(4096);
        let substituted = async {
            let mut server = f.server.accept(server, Role::Control).await.unwrap();
            assert!(matches!(Hello::decode(&server.read_frame(Duration::from_secs(1)).await.unwrap()).unwrap(), Hello::Discover { .. }));
            server.write_frame(&response, Duration::from_secs(1)).await.unwrap();
        };
        let (_, client) = tokio::join!(substituted, ControlClient::discover(&f.clients[0], client, f.server_id, f.authority.daemon.clone(), f.authority.limits.clone()));
        assert!(client.is_err());
    }
    assert_eq!(f.authority.registry.lock().unwrap().controls.len(), 0);
}

#[tokio::test]
async fn given_mismatched_export_owner_should_reject_authority_composition() {
    let f = Fixture::new(SessionLimits::default(), None).await;
    let lock = f._lock.clone();
    tokio::task::spawn_blocking(move || {
        use crate::daemon_lifecycle::mesh_exports::MeshExports;
        let store = MeshStore::open(lock.clone()).unwrap();
        let ledger = Ledger::in_memory(store.daemon_id(), LedgerLimits::default()).unwrap();
        let exports = MeshExports::open(lock, "different-owner", 8).unwrap();
        assert!(matches!(
            SessionAuthority::new(store, ledger, exports, SessionLimits::default()),
            Err(Error::Denied)
        ));
    })
    .await
    .unwrap();
}

#[test]
fn given_peer_name_selector_should_parse_exact_lowercase_pin_without_proving_transport_identity() {
    use crate::daemon_lifecycle::mesh_store::peer_id_from_name;
    let identity = Identity::generate().unwrap();
    let name = peer_name(identity.peer_id());
    assert_eq!(peer_id_from_name(&name).unwrap(), identity.peer_id());
    assert_eq!(
        PeerId::from_fingerprint(*identity.peer_id().fingerprint()),
        identity.peer_id()
    );
    for name in [
        "".into(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        format!("../{}", "a".repeat(61)),
    ] {
        assert!(peer_id_from_name(&name).is_err());
    }
    // These values are expected pins only; there is no AuthenticatedConnection
    // constructor, certificate possession or Caller authority in this API.
}

#[tokio::test]
async fn given_export_mutation_should_close_all_peers_even_on_error_under_exclusive_authority() {
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (control0, client0) = f.control(0).await;
    let (control1, _client1) = f.control(1).await;
    let (mut bulk0, _observer0) = f.bulk(&client0).await;
    let project = f._tmp.path().join("export-project");
    tokio::task::spawn_blocking({
        let project = project.clone();
        move || std::fs::create_dir(project).unwrap()
    })
    .await
    .unwrap();
    let exported = f
        .authority
        .register_export(&f.work, admin(), project, "project".into())
        .await
        .unwrap();
    assert!(matches!(control0.binding.check(), Err(Error::Closed)));
    assert!(matches!(control1.binding.check(), Err(Error::Closed)));
    assert!(matches!(bulk0.wait_bulk_close().await, Err(Error::Closed)));
    drop((control0, control1, bulk0));
    let (control0, _client0) = f.control(0).await;
    let (control1, _client1) = f.control(1).await;
    let authority = f.authority.clone();
    let id = exported.id.clone();
    let failed: Result<()> = f
        .authority
        .administer_exports(&f.work, admin(), move |exports, admin| {
            assert!(authority.state.try_read().is_err());
            exports.revoke(admin, &id).unwrap();
            Err(Error::Operation) // durable tombstone still fences after later error
        })
        .await;
    assert!(failed.is_err());
    assert!(control0.binding.check().is_err() && control1.binding.check().is_err());
    let id = exported.id;
    let result = f
        .authority
        .inspect_admin(&f.work, admin(), move |_, _, exports, _| {
            Ok(exports.resolve(&id, 1).is_err())
        })
        .await
        .unwrap();
    assert!(result);
}

impl Fixture {
    async fn issued_export(&mut self) -> ExportRecord {
        let path = self._tmp.path().join("issued-project");
        tokio::task::spawn_blocking({
            let path = path.clone();
            move || std::fs::create_dir(path).unwrap()
        })
        .await
        .unwrap();
        let record = self
            .authority
            .register_export(&self.work, admin(), path, "issued".into())
            .await
            .unwrap();
        for peer in 0..2 {
            let mut policy = grants();
            policy[0].export_id = record.id.clone();
            self.generations[peer] = self
                .authority
                .update_policy(
                    &self.work,
                    admin(),
                    self.identities[peer].certificate_der().to_vec(),
                    self.generations[peer],
                    policy,
                    None,
                )
                .await
                .unwrap()
                .generation;
        }
        record
    }
}

#[tokio::test]
async fn given_borrowed_exports_should_resolve_inside_dispatch_and_hold_revocation_through_bounded_bulk_send()
 {
    let mut f = Fixture::new(SessionLimits::default(), None).await;
    let export = f.issued_export().await;
    let (mut control, mut client) = f.control(0).await;
    let request = Request {
        request_id: "status".into(),
        target_daemon: f.authority.daemon.clone(),
        operation: wire::Operation::Status {
            export: wire::Export {
                id: export.id.clone(),
                generation: export.generation,
            },
            agent_id: None,
            after_agent: None,
            limit: 64,
        },
    };
    let authority = f.authority.clone();
    let (response, dispatch) = tokio::join!(
        client.request::<serde_json::Value>(&request),
        control.dispatch(&f.work, move |ledger, exports, caller, _, request| {
            assert!(authority.state.try_write().is_err());
            ledger.validate_principal(caller).unwrap();
            let wire::Operation::Status { export, .. } = &request.operation else {
                panic!()
            };
            let resolved = exports.resolve(&export.id, export.generation).unwrap();
            exports.revalidate(&resolved).unwrap();
            Ok(br#"{"checked":true}"#.to_vec())
        })
    );
    assert!(dispatch.is_ok());
    assert_eq!(response.unwrap()["checked"], true);
    let (mut bulk, _stalled) = f.bulk(&client).await;
    let subscription = bulk.subscribe(export.id.clone()).await.unwrap();
    let id = export.id.clone();
    let send = tokio::spawn(async move {
        bulk.send_output_with(&subscription, move |ledger, exports, caller| {
            ledger.validate_principal(caller).unwrap();
            let resolved = exports.resolve(&id, 1).unwrap();
            exports.revalidate(&resolved).unwrap();
            Ok((32 * 1024, vec![b'x'; wire::MAX_BULK_FRAME]))
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.authority.state.try_write().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let authority = f.authority.clone();
    let work = f.work.clone();
    let id = export.id;
    let revoke = tokio::spawn(async move { authority.revoke_export(&work, admin(), id).await });
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(!revoke.is_finished());
    assert!(matches!(
        send.await.unwrap(),
        Err(Error::Transport(crate::mesh_transport::Error::Timeout))
    ));
    assert!(revoke.await.unwrap().is_ok());
    assert!(matches!(control.binding.check(), Err(Error::Closed)));
}

#[tokio::test]
async fn given_cancelled_export_admin_should_retain_exclusive_guard_and_wake_all_bindings_after_durable_change()
 {
    let mut f = Fixture::new(SessionLimits::default(), None).await;
    let export = f.issued_export().await;
    let (control0, _client0) = f.control(0).await;
    let (control1, _client1) = f.control(1).await;
    let (entered, began) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let authority = f.authority.clone();
    let work = f.work.clone();
    let id = export.id.clone();
    let task = tokio::spawn(async move {
        authority
            .administer_exports(&work, admin(), move |exports, admin| {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                exports.revoke(admin, &id).map_err(|_| Error::Operation)
            })
            .await
    });
    began.await.unwrap();
    task.abort();
    let _ = task.await;
    let held = f.authority.state.try_read().is_err();
    let live_until_change = control0.binding.check().is_ok() && control1.binding.check().is_ok();
    release.send(()).unwrap();
    assert!(held && live_until_change);
    f.work.wait_idle().await;
    assert!(control0.binding.check().is_err() && control1.binding.check().is_err());
    let id = export.id;
    assert!(
        f.authority
            .inspect_admin(&f.work, admin(), move |_, _, exports, _| Ok(exports
                .resolve(&id, 1)
                .is_err()))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn given_guarded_admin_transport_snapshot_should_validate_exact_pins_but_never_cache_session_authority()
 {
    let mut f = Fixture::new(SessionLimits::default(), None).await;
    let identity = f.authority.identity(&f.work, admin()).await.unwrap();
    assert_eq!(identity.daemon_id, f.authority.daemon);
    assert_eq!(identity.peer_id, f.server_id);
    assert_eq!(
        PeerId::from_certificate_der(&identity.certificate_der).unwrap(),
        f.server_id
    );
    let snapshot = f
        .authority
        .transport(&f.work, admin(), now())
        .await
        .unwrap();
    let peer = f.identities[0].peer_id();
    let revoked = f.authority.revoke(&f.work, admin(), peer).await.unwrap();
    f.generations[0] = revoked.generation;
    let (server, client) = tokio::io::duplex(4096);
    let (accepted, client) = tokio::join!(
        f.authority.accept_incoming(&snapshot, server),
        ControlClient::connect(
            &f.clients[0],
            client,
            f.server_id,
            f.authority.daemon.clone(),
            revoked.generation,
            f.authority.limits.clone()
        )
    );
    assert!(matches!(accepted, Err(Error::Denied)));
    assert!(client.is_err());
    let name = peer_name(peer);
    let displayed = f
        .authority
        .inspect_admin(&f.work, admin(), move |store, ledger, _, admin| {
            store
                .admin_peer(ledger, admin, &name)
                .map_err(|_| Error::Operation)
        })
        .await
        .unwrap();
    assert!(displayed.0.revoked && displayed.1.revoked);
    assert_eq!(PeerId::from_certificate_der(&displayed.2).unwrap(), peer);
    let current_transport = f
        .authority
        .transport(&f.work, admin(), now())
        .await
        .unwrap();
    let (server, client) = tokio::io::duplex(4096);
    let (accepted, _client) = tokio::join!(
        current_transport.accept(server, Role::Control),
        f.clients[0].connect(client, Role::Control, f.server_id)
    );
    assert!(
        accepted.is_err(),
        "new TLS setup includes only active checked pins"
    );
    let expired = f
        .authority
        .update_policy(
            &f.work,
            admin(),
            f.identities[1].certificate_der().to_vec(),
            f.generations[1],
            grants(),
            Some(now()),
        )
        .await
        .unwrap();
    f.generations[1] = expired.generation;
    let name = peer_name(f.identities[1].peer_id());
    let displayed = f
        .authority
        .inspect_admin(&f.work, admin(), move |store, ledger, _, admin| {
            store
                .admin_peer(ledger, admin, &name)
                .map_err(|_| Error::Operation)
        })
        .await
        .unwrap();
    assert!(displayed.1.expires_at.is_some());
    assert!(matches!(
        f.authority.transport(&f.work, admin(), now()).await,
        Err(Error::Denied)
    ));
    let listed = f
        .authority
        .inspect_admin(&f.work, admin(), |store, ledger, _, admin| {
            store
                .list_admin_peers(ledger, admin, None, 2)
                .map_err(|_| Error::Operation)
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
}

#[tokio::test]
async fn given_damaged_policy_should_deny_admin_snapshots_but_allow_ledger_based_local_revoke() {
    use crate::daemon_lifecycle::mesh_store::peer_id_from_name;
    let f = Fixture::new(SessionLimits::default(), None).await;
    let (control, _client) = f.control(0).await;
    let name = peer_name(f.identities[0].peer_id());
    let path = f
        ._tmp
        .path()
        .join(format!("scratch.sock.mesh-peer-{name}.json"));
    tokio::task::spawn_blocking(move || std::fs::write(path, b"{}").unwrap())
        .await
        .unwrap();
    assert!(
        f.authority
            .transport(&f.work, admin(), now())
            .await
            .is_err()
    );
    assert!(
        f.authority
            .inspect_admin(&f.work, admin(), |store, ledger, _, admin| store
                .list_admin_peers(ledger, admin, None, 2)
                .map_err(|_| Error::Operation))
            .await
            .is_err()
    );
    let selected = peer_id_from_name(&name).unwrap();
    let generation = f.generations[0];
    let revoked = f
        .authority
        .administer(&f.work, admin(), selected, move |_, ledger, admin| {
            assert_eq!(ledger.peer(&name).unwrap().generation, generation);
            ledger
                .revoke_peer(admin, &name)
                .map_err(|_| Error::Operation)
        })
        .await
        .unwrap();
    assert!(revoked.revoked);
    assert!(control.binding.check().is_err());
    // A valid certificate enrollment without an anchor also denies setup/list.
    let unanchored = Fixture::with_anchor(SessionLimits::default(), None, false).await;
    assert!(
        unanchored
            .authority
            .transport(&unanchored.work, admin(), now())
            .await
            .is_err()
    );
    assert!(
        unanchored
            .authority
            .inspect_admin(&unanchored.work, admin(), |store, ledger, _, admin| store
                .list_admin_peers(ledger, admin, None, 2)
                .map_err(|_| Error::Operation))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn given_transport_catalog_overflow_should_reject_before_unbounded_policy_or_certificate_reads()
 {
    let (authority, work, _tmp, _lock) = tokio::task::spawn_blocking(|| {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let lock = Arc::new(
            try_acquire_daemon_lock(&tmp.path().join("scratch.sock"))
                .unwrap()
                .unwrap(),
        );
        let store = MeshStore::initialize(lock.clone(), &admin()).unwrap();
        let ledger = Ledger::open(
            tmp.path().join("ledger.sqlite"),
            store.daemon_id(),
            LedgerLimits {
                peers: 257,
                ..LedgerLimits::default()
            },
        )
        .unwrap();
        for index in 0..257 {
            ledger
                .register_peer(&admin(), &format!("{index:064x}"))
                .unwrap();
        }
        let exports = MeshExports::open(lock.clone(), store.daemon_id(), 8).unwrap();
        let work = WorkOwner::new(&lock);
        let authority =
            SessionAuthority::new(store, ledger, exports, SessionLimits::default()).unwrap();
        (authority, work, tmp, lock)
    })
    .await
    .unwrap();
    assert!(
        authority
            .inspect_admin(&work, admin(), |store, ledger, _, admin| Ok(matches!(
                store.active_peer_certificates(ledger, admin, now()),
                Err(crate::daemon_lifecycle::mesh_store::Error::Capacity)
            )))
            .await
            .unwrap()
    );
    assert!(authority.transport(&work, admin(), now()).await.is_err());
}
