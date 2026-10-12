use crate::{engine::Engine, ipc_server::IpcServer};
use pu_core::mesh::{
    Capability, ExportGrant,
    admin::{ListenConfig, MeshAdminRequest, MeshAdminResponse, PublicIdentity, WorkerRoute},
    local::{ListenerStatus, LocalRoute},
    response::Body,
    wire::{Operation, Request as WireRequest},
};
use pu_core::protocol::{Request, Response};
use std::{collections::BTreeSet, net::SocketAddr, os::unix::fs::PermissionsExt, sync::Arc};

pub(crate) struct Daemon {
    pub _tmp: tempfile::TempDir,
    pub server: IpcServer,
}

impl Daemon {
    pub(crate) fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let server = IpcServer::bind(&tmp.path().join("daemon.sock"), Engine::new()).unwrap();
        Self { _tmp: tmp, server }
    }

    pub(crate) fn engine(&self) -> &Arc<Engine> {
        self.server.engine()
    }

    pub(crate) async fn admin(&self, request: MeshAdminRequest) -> MeshAdminResponse {
        match self
            .engine()
            .handle_request(Request::MeshAdmin { request })
            .await
        {
            Response::MeshAdmin { response } => response,
            other => panic!("admin request failed: {other:?}"),
        }
    }

    pub(crate) async fn admin_error(&self, request: MeshAdminRequest) -> String {
        match self
            .engine()
            .handle_request(Request::MeshAdmin { request })
            .await
        {
            Response::Error { code, .. } => code,
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    pub(crate) async fn identity(&self) -> PublicIdentity {
        match self.admin(MeshAdminRequest::IdentityShow {}).await {
            MeshAdminResponse::Identity { identity } => identity,
            other => panic!("unexpected {other:?}"),
        }
    }

    pub(crate) async fn listener(&self) -> ListenerStatus {
        match self
            .engine()
            .handle_request(Request::MeshListenerStatus {})
            .await
        {
            Response::MeshListener { status } => status,
            other => panic!("unexpected {other:?}"),
        }
    }

    pub(crate) async fn route(&self, worker: &str, request: WireRequest) -> Response {
        self.engine()
            .handle_request(Request::MeshRoute {
                route: LocalRoute {
                    worker: worker.into(),
                    request,
                },
            })
            .await
    }
}

/// Reserve two distinct loopback ports. Explicit configuration rejects port 0.
pub(crate) fn free_pair() -> (SocketAddr, SocketAddr) {
    let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (a.local_addr().unwrap(), b.local_addr().unwrap())
}

pub(crate) struct Pair {
    pub host: Daemon,
    pub worker: Daemon,
    pub host_identity: PublicIdentity,
    pub worker_identity: PublicIdentity,
    pub export_id: String,
    pub _project: tempfile::TempDir,
    pub control: SocketAddr,
}

/// Owner-run setup on two independent daemons: explicit identity creation,
/// out-of-band fingerprint exchange, receiver grants and explicit listening.
pub(crate) async fn enrolled_pair(capabilities: BTreeSet<Capability>) -> Pair {
    let host = Daemon::new();
    let worker = Daemon::new();
    for daemon in [&host, &worker] {
        assert!(matches!(
            daemon.admin(MeshAdminRequest::IdentityInitialize {}).await,
            MeshAdminResponse::Identity { .. }
        ));
    }
    let host_identity = host.identity().await;
    let worker_identity = worker.identity().await;
    let project = tempfile::tempdir().unwrap();
    let export_id = match worker
        .admin(MeshAdminRequest::ExportRegister {
            project_root: project.path().to_str().unwrap().into(),
            label: "project".into(),
        })
        .await
    {
        MeshAdminResponse::Export { export } => export.id,
        other => panic!("unexpected {other:?}"),
    };
    worker
        .admin(MeshAdminRequest::EnrollManual {
            certificate_der: host_identity.certificate_der.clone(),
            expected_fingerprint: host_identity.fingerprint.clone(),
            grants: vec![ExportGrant {
                export_id: export_id.clone(),
                capabilities,
                runtimes: BTreeSet::from(["claude".into()]),
            }],
            expires_at: None,
        })
        .await;
    // The host pins the worker too, but grants it nothing on the host.
    host.admin(MeshAdminRequest::EnrollManual {
        certificate_der: worker_identity.certificate_der.clone(),
        expected_fingerprint: worker_identity.fingerprint.clone(),
        grants: vec![],
        expires_at: None,
    })
    .await;
    let (control, bulk) = free_pair();
    worker
        .admin(MeshAdminRequest::ConfigSet {
            config: ListenConfig {
                bind_address: Some(control),
                bulk_bind_address: Some(bulk),
            },
        })
        .await;
    host.admin(MeshAdminRequest::WorkerSet {
        route: WorkerRoute {
            fingerprint: worker_identity.fingerprint.clone(),
            daemon_id: worker_identity.daemon_id.clone(),
            label: "worker".into(),
            control_address: control,
            bulk_address: bulk,
        },
    })
    .await;
    Pair {
        host,
        worker,
        host_identity,
        worker_identity,
        export_id,
        _project: project,
        control,
    }
}

fn inventory(daemon: &str) -> WireRequest {
    WireRequest {
        request_id: pu_core::id::session_id(),
        target_daemon: daemon.into(),
        operation: Operation::Inventory {},
    }
}

#[tokio::test]
async fn given_uninitialized_daemon_should_open_no_listener_and_refuse_implicit_state() {
    // given
    let daemon = Daemon::new();
    // when
    daemon.engine().mesh().unwrap().start().await;
    // then
    assert_eq!(daemon.listener().await, ListenerStatus::NotInitialized);
    assert_eq!(
        daemon.admin_error(MeshAdminRequest::IdentityShow {}).await,
        "MESH_NOT_INITIALIZED"
    );
    assert_eq!(
        daemon
            .admin_error(MeshAdminRequest::ConfigSet {
                config: ListenConfig::default()
            })
            .await,
        "MESH_NOT_INITIALIZED"
    );
    assert_eq!(daemon.listener().await, ListenerStatus::NotInitialized);
}

#[tokio::test]
async fn given_enrolled_pair_should_route_pinned_inventory_and_report_applied_listener() {
    // given
    let pair = enrolled_pair(BTreeSet::from([Capability::Inspect])).await;
    assert!(matches!(
        pair.worker.listener().await,
        ListenerStatus::Listening { .. }
    ));
    assert_eq!(pair.host.listener().await, ListenerStatus::Disabled);
    // when
    let response = pair
        .host
        .route(
            &pair.worker_identity.fingerprint,
            inventory(&pair.worker_identity.daemon_id),
        )
        .await;
    // then
    match response {
        Response::MeshRoute { response } => match response.body {
            Body::Inventory { exports } => {
                assert_eq!(exports.len(), 1);
                assert_eq!(exports[0].export.id, pair.export_id);
            }
            other => panic!("unexpected body {other:?}"),
        },
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn given_unrouted_worker_or_wrong_daemon_should_fail_without_fallback() {
    // given
    let pair = enrolled_pair(BTreeSet::from([Capability::Inspect])).await;
    // when / then
    match pair
        .host
        .route(&"b".repeat(64), inventory(&pair.worker_identity.daemon_id))
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, "MESH_UNKNOWN_WORKER"),
        other => panic!("unexpected {other:?}"),
    }
    match pair
        .host
        .route(&pair.worker_identity.fingerprint, inventory("other-daemon"))
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, "MESH_INVALID_REQUEST"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn given_disable_should_stop_old_listener_before_reply() {
    // given
    let pair = enrolled_pair(BTreeSet::from([Capability::Inspect])).await;
    // when
    let response = pair
        .worker
        .admin(MeshAdminRequest::ConfigSet {
            config: ListenConfig::default(),
        })
        .await;
    // then: the old socket no longer accepts by the time the reply arrives
    assert!(matches!(response, MeshAdminResponse::Config { .. }));
    assert_eq!(pair.worker.listener().await, ListenerStatus::Disabled);
    assert!(tokio::net::TcpStream::connect(pair.control).await.is_err());
    match pair
        .host
        .route(
            &pair.worker_identity.fingerprint,
            inventory(&pair.worker_identity.daemon_id),
        )
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, "MESH_WORKER_OFFLINE"),
        other => panic!("unexpected {other:?}"),
    }
}

async fn peer_generation(daemon: &Daemon, fingerprint: &str) -> u64 {
    match daemon
        .admin(MeshAdminRequest::PeerList {
            after: None,
            limit: 8,
        })
        .await
    {
        MeshAdminResponse::Peers { peers, .. } => {
            peers
                .into_iter()
                .find(|peer| peer.fingerprint == fingerprint)
                .unwrap()
                .generation
        }
        other => panic!("unexpected {other:?}"),
    }
}

async fn revoke(daemon: &Daemon, fingerprint: &str) {
    let generation = peer_generation(daemon, fingerprint).await;
    daemon
        .admin(MeshAdminRequest::PeerChange {
            fingerprint: fingerprint.into(),
            expected_generation: generation,
            change: pu_core::mesh::admin::PeerChange::Revoke,
        })
        .await;
}

fn route_code(response: Response) -> String {
    match response {
        Response::Error { code, .. } => code,
        other => panic!("expected rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn given_last_peer_revoked_should_close_listener_and_reveal_nothing() {
    // given
    let pair = enrolled_pair(BTreeSet::from([Capability::Inspect])).await;
    // when
    revoke(&pair.worker, &pair.host_identity.fingerprint).await;
    // then
    assert_eq!(pair.worker.listener().await, ListenerStatus::NoActivePeers);
    assert!(tokio::net::TcpStream::connect(pair.control).await.is_err());
    let response = pair
        .host
        .route(
            &pair.worker_identity.fingerprint,
            inventory(&pair.worker_identity.daemon_id),
        )
        .await;
    assert_eq!(route_code(response), "MESH_WORKER_OFFLINE");
}

#[tokio::test]
async fn given_one_of_several_peers_revoked_should_deny_it_at_authentication() {
    // given
    let pair = enrolled_pair(BTreeSet::from([Capability::Inspect])).await;
    let other = crate::mesh_transport::Identity::generate().unwrap();
    pair.worker
        .admin(MeshAdminRequest::EnrollManual {
            certificate_der: other.certificate_der().to_vec(),
            expected_fingerprint: crate::daemon_lifecycle::mesh_store::peer_name(other.peer_id()),
            grants: vec![],
            expires_at: None,
        })
        .await;
    // when
    revoke(&pair.worker, &pair.host_identity.fingerprint).await;
    // then
    assert!(matches!(
        pair.worker.listener().await,
        ListenerStatus::Listening { .. }
    ));
    let response = pair
        .host
        .route(
            &pair.worker_identity.fingerprint,
            inventory(&pair.worker_identity.daemon_id),
        )
        .await;
    assert_eq!(route_code(response), "MESH_DENIED");
}

#[tokio::test]
async fn given_worker_route_for_unenrolled_peer_should_reject() {
    // given
    let daemon = Daemon::new();
    daemon.admin(MeshAdminRequest::IdentityInitialize {}).await;
    // when / then
    assert_eq!(
        daemon
            .admin_error(MeshAdminRequest::WorkerSet {
                route: WorkerRoute {
                    fingerprint: "c".repeat(64),
                    daemon_id: "worker".into(),
                    label: "ghost".into(),
                    control_address: "127.0.0.1:9001".parse().unwrap(),
                    bulk_address: "127.0.0.1:9002".parse().unwrap(),
                },
            })
            .await,
        "MESH_INVALID_REQUEST"
    );
}
