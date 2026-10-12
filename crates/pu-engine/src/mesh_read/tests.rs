use super::*;
use crate::{
    daemon_lifecycle::{mesh_exports::MeshExports, try_acquire_daemon_lock},
    execution_state::*,
};
use pu_core::mesh::{
    Capability, ExportGrant, PeerPolicy,
    response::{Body, ErrorCode, Response},
    wire::{self, Operation, Request},
};
use std::{collections::BTreeSet, os::unix::fs::PermissionsExt, sync::Arc};

struct Fixture {
    _tmp: tempfile::TempDir,
    ledger: Ledger,
    exports: MeshExports,
    admin: LocalAdmin,
    caller: Caller,
    policy: PeerPolicy,
    intent: LaunchIntent,
    receipt: LaunchReceipt,
    key: OperationKey,
}

#[test]
fn given_paged_status_should_honor_limit_and_resume_after_last_immutable_agent_id() {
    let f = Fixture::new();
    for i in [2, 3] {
        let key = OperationKey {
            window: f.key.window,
            id: OperationId(format!("op-{i}")),
            digest: PayloadDigest([i; 32]),
        };
        f.ledger
            .admit_launch(&f.caller, &key, &f.intent, None, 101)
            .unwrap();
    }
    let mut after = None;
    let mut ids = Vec::new();
    for page in 0..3 {
        let request = f.request(Operation::Status {
            export: f.export(),
            agent_id: None,
            after_agent: after.clone(),
            limit: 1,
        });
        let Body::Status { agents, next_agent } = f.call(&request).body else {
            panic!("status reply expected")
        };
        assert_eq!(agents.len(), 1);
        if let Some(previous) = after.as_ref() {
            assert!(&agents[0].target.agent_id > previous);
        }
        ids.push(agents[0].target.agent_id.clone());
        assert_eq!(next_agent.is_some(), page < 2);
        after = next_agent;
    }
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let lock = Arc::new(
            try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
                .unwrap()
                .unwrap(),
        );
        let admin =
            LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
                .unwrap();
        let mut exports = MeshExports::open(lock, "worker", 8).unwrap();
        let export = exports
            .register(&admin, tmp.path(), "Private project")
            .unwrap();
        let ledger = Ledger::in_memory("worker", Limits::default()).unwrap();
        ledger.register_peer(&admin, "host").unwrap();
        let peer = ledger
            .update_peer_authorization(&admin, "host", 1, PayloadDigest([9; 32]))
            .unwrap();
        let caller = Caller::authenticated_peer("host", peer.generation).unwrap();
        let policy = PeerPolicy {
            peer_id: "host".into(),
            generation: peer.generation,
            revoked: false,
            expires_at: None,
            exports: vec![ExportGrant {
                export_id: export.id.clone(),
                capabilities: BTreeSet::from([Capability::Inspect]),
                runtimes: BTreeSet::from(["codex".into()]),
            }],
        };
        let intent = LaunchIntent {
            project: tmp.path().canonicalize().unwrap().to_str().unwrap().into(),
            export: Some(export.id),
            runtime: "codex".into(),
            source_commit: "a".repeat(40),
            worktree: WorktreeIntent {
                id: "worktree".into(),
                path: tmp.path().join("private-worktree").to_str().unwrap().into(),
                branch: "secret-branch".into(),
            },
        };
        let window = ledger.issue_window(&caller, 100, 100).unwrap();
        let key = OperationKey {
            window: window.id,
            id: OperationId("operation".into()),
            digest: PayloadDigest([4; 32]),
        };
        let receipt = ledger
            .admit_launch(&caller, &key, &intent, None, 100)
            .unwrap()
            .value;
        Self {
            _tmp: tmp,
            ledger,
            exports,
            admin,
            caller,
            policy,
            intent,
            receipt,
            key,
        }
    }
    fn export(&self) -> wire::Export {
        wire::Export {
            id: self.intent.export.clone().unwrap(),
            generation: 1,
        }
    }
    fn target(&self) -> wire::AgentTarget {
        wire::AgentTarget {
            export: self.export(),
            agent_id: self.receipt.agent_id.0.clone(),
            run_id: self.receipt.run_id.0.clone(),
        }
    }
    fn request(&self, operation: Operation) -> Request {
        Request {
            request_id: "request".into(),
            target_daemon: "worker".into(),
            operation,
        }
    }
    fn call(&self, request: &Request) -> Response {
        let bytes = dispatch(
            &self.ledger,
            &self.exports,
            &self.caller,
            &self.policy,
            request,
        )
        .unwrap();
        Response::decode(&bytes, request).unwrap()
    }
}

#[test]
fn given_retained_receipt_should_require_exact_live_export_and_preserve_snapshot_privacy() {
    let mut f = Fixture::new();
    let request = f.request(Operation::Receipt {
        export: f.export(),
        mutation: wire::Mutation {
            window: f.key.window.0,
            id: f.key.id.0.clone(),
        },
        payload_digest: f.key.digest.0,
    });
    let reply = f.call(&request);
    assert!(matches!(reply.body, Body::Receipt { evidence: Some(_) }));
    let json = String::from_utf8(reply.encode().unwrap()).unwrap();
    assert!(!json.contains(&f.intent.project));
    assert!(!json.contains("secret-branch"));
    let wrong = f.request(Operation::Receipt {
        export: wire::Export {
            id: "other-export".into(),
            generation: 1,
        },
        mutation: wire::Mutation {
            window: f.key.window.0,
            id: f.key.id.0.clone(),
        },
        payload_digest: f.key.digest.0,
    });
    assert!(matches!(f.call(&wrong).body, Body::Error { .. }));
    let other = f
        .exports
        .register(&f.admin, f._tmp.path(), "Same root, different export")
        .unwrap();
    f.policy.exports.push(ExportGrant {
        export_id: other.id.clone(),
        capabilities: BTreeSet::from([Capability::Inspect]),
        runtimes: BTreeSet::new(),
    });
    let mut cross_export = request.clone();
    if let Operation::Receipt { export, .. } = &mut cross_export.operation {
        export.id = other.id;
    }
    assert!(matches!(
        f.call(&cross_export).body,
        Body::Error {
            code: ErrorCode::Unavailable
        }
    ));
    f.exports.revoke(&f.admin, &f.export().id).unwrap();
    assert!(matches!(
        f.call(&request).body,
        Body::Error {
            code: ErrorCode::Unavailable
        }
    ));
}

#[test]
fn given_status_should_show_current_durable_run_only_and_deny_changed_project_or_generation() {
    let mut f = Fixture::new();
    let request:Request=serde_json::from_value(serde_json::json!({"request_id":"status","target_daemon":"worker","operation":{"type":"status","export":f.export(),"agent_id":f.receipt.agent_id.0}})).unwrap();
    let response = f.call(&request);
    assert!(
        matches!(response.body,Body::Status{agents,..} if agents.len()==1 && agents[0].target==f.target() && agents[0].lifecycle==pu_core::mesh::response::Lifecycle::Accepted)
    );
    let mut stale = request.clone();
    if let Operation::Status { export, .. } = &mut stale.operation {
        export.generation = 2;
    }
    assert!(matches!(
        f.call(&stale).body,
        Body::Error {
            code: ErrorCode::Unavailable
        }
    ));
    let other_project = f._tmp.path().join("other-project");
    std::fs::create_dir(&other_project).unwrap();
    let other = f
        .exports
        .register(&f.admin, &other_project, "Other")
        .unwrap();
    f.policy.exports.push(ExportGrant {
        export_id: other.id.clone(),
        capabilities: BTreeSet::from([Capability::Inspect]),
        runtimes: BTreeSet::new(),
    });
    if let Operation::Status { export, .. } = &mut stale.operation {
        export.generation = 1;
        export.id = other.id;
    }
    assert!(matches!(
        f.call(&stale).body,
        Body::Error {
            code: ErrorCode::Unavailable
        }
    ));
}

#[test]
fn given_input_receipt_should_reject_other_principal_run_or_export_and_allow_same_peer_after_fence()
{
    let mut f = Fixture::new();
    let agent = f.ledger.begin_start(&f.caller, &f.receipt.guard).unwrap();
    f.ledger
        .record_running(
            &agent.current_run,
            &SupervisionIdentity {
                mechanism: "test".into(),
                token: "exact-run-test-evidence".into(),
            },
        )
        .unwrap();
    let guard = f.ledger.agent(&agent.agent_id).unwrap().guard();
    let writer = f.ledger.open_writer(&f.caller, &guard, None).unwrap();
    f.ledger
        .accept_input(&f.caller, &guard, writer.id, 1, PayloadDigest([2; 32]), 5)
        .unwrap();
    f.ledger
        .finish_input(writer.id, 1, InputOutcome::Unknown)
        .unwrap();
    let request = f.request(Operation::InputReceipt {
        target: f.target(),
        writer: writer.id.0,
        sequence: 1,
    });
    assert!(matches!(
        f.call(&request).body,
        Body::InputReceipt { evidence: Some(_) }
    ));
    let next = f
        .ledger
        .advance_peer_generation(&f.admin, "host", f.policy.generation)
        .unwrap();
    f.caller = Caller::authenticated_peer("host", next.generation).unwrap();
    f.policy.generation = next.generation;
    assert!(matches!(
        f.call(&request).body,
        Body::InputReceipt { evidence: Some(_) }
    ));
    let mut wrong = request.clone();
    if let Operation::InputReceipt { target, .. } = &mut wrong.operation {
        target.run_id = "other-run".into();
    }
    assert!(matches!(f.call(&wrong).body, Body::Error { .. }));
    f.ledger.register_peer(&f.admin, "other").unwrap();
    let peer = f
        .ledger
        .update_peer_authorization(&f.admin, "other", 1, PayloadDigest([9; 32]))
        .unwrap();
    f.caller = Caller::authenticated_peer("other", peer.generation).unwrap();
    f.policy.peer_id = "other".into();
    f.policy.generation = peer.generation;
    assert!(matches!(
        f.call(&request).body,
        Body::Error {
            code: ErrorCode::Denied
        }
    ));
}

#[test]
fn given_inventory_should_filter_revoked_ungranted_and_stale_resources_and_gate_mutations() {
    let mut f = Fixture::new();
    let request = f.request(Operation::Inventory {});
    assert!(matches!(f.call(&request).body,Body::Inventory{exports} if exports.len()==1));
    f.policy.exports[0].capabilities.clear();
    assert!(matches!(f.call(&request).body,Body::Inventory{exports} if exports.is_empty()));
    f.policy.exports[0].capabilities.insert(Capability::Inspect);
    f.exports.revoke(&f.admin, &f.export().id).unwrap();
    assert!(matches!(f.call(&request).body,Body::Inventory{exports} if exports.is_empty()));
    let mutation = f.request(Operation::IssueWindow { ttl_seconds: 60 });
    assert!(matches!(
        f.call(&mutation).body,
        Body::Error {
            code: ErrorCode::ExecutionDisabled
        }
    ));
}

#[tokio::test]
async fn given_actual_tls_peer_should_filter_inventory_and_deny_dispatch_after_revocation() {
    use crate::{
        daemon_lifecycle::{mesh_store::MeshStore, owned_work::WorkOwner},
        mesh_sessions::{ControlClient, SessionAuthority, SessionLimits},
        mesh_transport::{EnrolledPeers, Identity, MeshTransport, Role},
    };
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
    let ledger = Ledger::open(
        tmp.path().join("ledger.sqlite"),
        store.daemon_id(),
        Limits::default(),
    )
    .unwrap();
    let mut exports = MeshExports::open(lock, store.daemon_id(), 8).unwrap();
    let exported = exports
        .register(&admin, tmp.path(), "Receiver project")
        .unwrap();
    let hidden = exports
        .register(&admin, tmp.path(), "Unshared project")
        .unwrap();
    let client_identity = Identity::generate().unwrap();
    let peer = store
        .enroll(
            &ledger,
            &admin,
            client_identity.certificate_der(),
            vec![ExportGrant {
                export_id: exported.id.clone(),
                capabilities: BTreeSet::from([Capability::Inspect]),
                runtimes: BTreeSet::from(["codex".into()]),
            }],
            None,
        )
        .unwrap();
    let server_pin = store.identity().peer_id();
    let server = MeshTransport::new(
        store.identity(),
        EnrolledPeers::new([client_identity.certificate_der()]).unwrap(),
    )
    .unwrap();
    let client = MeshTransport::new(
        &client_identity,
        EnrolledPeers::new([store.identity().certificate_der()]).unwrap(),
    )
    .unwrap();
    let authority = SessionAuthority::new(store, ledger, SessionLimits::default()).unwrap();
    let exports = Arc::new(exports);
    let (rx, tx) = tokio::io::duplex(4096);
    let (session, client) = tokio::join!(
        authority.accept(&server, rx, Role::Control),
        ControlClient::connect(
            &client,
            tx,
            server_pin,
            authority.daemon_id().into(),
            peer.generation,
            SessionLimits::default()
        )
    );
    let mut session = session.unwrap();
    let mut client = client.unwrap();
    let request = Request {
        request_id: "tls-inventory".into(),
        target_daemon: authority.daemon_id().into(),
        operation: Operation::Inventory {},
    };
    let (sent, reply) = tokio::join!(
        session.dispatch(&work, move |ledger, caller, policy, request| dispatch(
            ledger, &exports, caller, policy, request
        )
        .map_err(crate::mesh_sessions::Error::from)),
        client.request::<Response>(&request)
    );
    sent.unwrap();
    let reply = reply.unwrap();
    let validated = Response::decode(&reply.encode().unwrap(), &request).unwrap();
    assert!(
        matches!(validated.body,Body::Inventory{exports} if exports.len()==1 && exports[0].export.id==exported.id && exports[0].export.id!=hidden.id)
    );
    authority
        .revoke(&work, admin, client_identity.peer_id())
        .await
        .unwrap();
    assert!(
        session
            .dispatch(&work, |_, _, _, _| panic!("revoked peer reached dispatch"))
            .await
            .is_err()
    );
}
