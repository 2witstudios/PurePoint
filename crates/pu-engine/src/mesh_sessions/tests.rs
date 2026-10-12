use super::*;
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
            let authority = SessionAuthority::new(store, ledger, limits).unwrap();
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
        control.dispatch(&f.work, |_, _, _, _| Ok(br#"{"ok":true}"#.to_vec()))
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
        control.dispatch(&f.work, |_, _, _, _| unreachable!()).await,
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
        control.dispatch(&f.work, |_, _, _, _| unreachable!())
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
    let waiter =
        tokio::spawn(async move { idle.dispatch(&work, |_, _, _, _| unreachable!()).await });
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
            control.dispatch(&f.work, |_, _, _, _| unreachable!())
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
            .dispatch(&work, move |ledger, caller, _, _| {
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
        control.dispatch(&f.work, |_, _, _, _| unreachable!())
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
        control.dispatch(&f.work, |_, _, _, _| Ok(br#"{"ok":true}"#.to_vec()))
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
    let producer = bulk.send_output_with(&subscription, move |ledger, caller| {
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
        control.dispatch(&f.work, |_, _, _, _| unreachable!())
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
            .dispatch(&f.work, |_, _, _, _| unreachable!())
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
        control.dispatch(&f.work, |ledger, caller, _, request| {
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
        control.dispatch(&f.work, |_, _, _, _| unreachable!())
    );
    assert!(response.is_err());
    assert!(matches!(dispatch, Err(Error::Denied)));
}
