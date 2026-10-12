//! End-to-end remote execution between two real in-process daemons, using the
//! compiled `pu-engine` binary as the worker's run guardian. The worker project
//! is a real Git repository at an exact commit; its `claude` runtime is a test
//! script that echoes PTY input and leaves a detached, marked descendant so stop
//! must prove whole-tree termination.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pu_core::mesh::{
    Capability, ExportGrant,
    admin::{ListenConfig, MeshAdminRequest, MeshAdminResponse, PublicIdentity, WorkerRoute},
    local::LocalRoute,
    response::{Body, ErrorCode, InputOutcome, Lifecycle, ReceiptEvidence},
    wire::{AgentTarget, ControlGuard, Export, Mutation, Operation, Request as WireRequest},
};
use pu_core::protocol::{Request, Response};
use pu_engine::{engine::Engine, ipc_server::IpcServer};

const EXE: &str = env!("CARGO_BIN_EXE_pu-engine");

struct Daemon {
    _tmp: tempfile::TempDir,
    server: IpcServer,
}

impl Daemon {
    fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let server = IpcServer::bind(&tmp.path().join("daemon.sock"), Engine::new()).unwrap();
        server.mesh().set_guardian_executable(PathBuf::from(EXE));
        Self { _tmp: tmp, server }
    }

    async fn call(&self, request: Request) -> Response {
        self.server.engine().handle_request(request).await
    }

    async fn admin(&self, request: MeshAdminRequest) -> MeshAdminResponse {
        match self.call(Request::MeshAdmin { request }).await {
            Response::MeshAdmin { response } => response,
            other => panic!("admin request failed: {other:?}"),
        }
    }

    async fn identity(&self) -> PublicIdentity {
        match self.admin(MeshAdminRequest::IdentityShow {}).await {
            MeshAdminResponse::Identity { identity } => identity,
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

struct Fleet {
    host: Daemon,
    _worker: Daemon,
    worker_identity: PublicIdentity,
    project: tempfile::TempDir,
    marker: String,
    export: Export,
    commit: String,
}

impl Drop for Fleet {
    fn drop(&mut self) {
        // Never leak test processes, even on failure: kill only our marker.
        let _ = std::process::Command::new("pkill")
            .args(["-9", "-f", &self.marker])
            .status();
    }
}

fn free_pair() -> (SocketAddr, SocketAddr) {
    let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (a.local_addr().unwrap(), b.local_addr().unwrap())
}

async fn fleet(capabilities: BTreeSet<Capability>) -> Fleet {
    let host = Daemon::new();
    let worker = Daemon::new();
    for daemon in [&host, &worker] {
        daemon.admin(MeshAdminRequest::IdentityInitialize {}).await;
    }
    let host_identity = host.identity().await;
    let worker_identity = worker.identity().await;

    // A real repository and exact commit on the worker.
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "test"]);
    std::fs::write(root.join("README"), "worker project\n").unwrap();
    git(&root, &["add", "README"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    let commit = git(&root, &["rev-parse", "HEAD"]);
    if let Response::Error { code, message } = worker
        .call(Request::Init {
            project_root: root.to_string_lossy().into(),
        })
        .await
    {
        panic!("init failed: {code} {message}");
    }
    // The receiver's own runtime configuration: an echoing fake agent that also
    // leaves a detached descendant carrying a unique marker.
    let marker = format!("pu-mesh-exec-test-{}", uuid_like());
    let script = root.join(".pu").join("fake-agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n( exec perl -e 'use POSIX; exit 0 if fork; POSIX::setsid(); sleep 300' {marker} & )\necho READY\nexec cat\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        root.join(".pu").join("config.yaml"),
        format!(
            "agents:\n  claude:\n    name: claude\n    command: {}\n    launchArgs: []\n",
            script.display()
        ),
    )
    .unwrap();

    let export_id = match worker
        .admin(MeshAdminRequest::ExportRegister {
            project_root: root.to_string_lossy().into(),
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
    Fleet {
        host,
        _worker: worker,
        worker_identity,
        project,
        marker,
        export: Export {
            id: export_id,
            generation: 1,
        },
        commit,
    }
}

fn uuid_like() -> String {
    pu_core::id::session_id().replace('-', "")
}

impl Fleet {
    fn request(&self, operation: Operation) -> WireRequest {
        WireRequest {
            request_id: pu_core::id::session_id(),
            target_daemon: self.worker_identity.daemon_id.clone(),
            operation,
        }
    }

    async fn send_raw(&self, operation: Operation) -> Response {
        self.host
            .call(Request::MeshRoute {
                route: LocalRoute {
                    worker: self.worker_identity.fingerprint.clone(),
                    request: self.request(operation),
                },
            })
            .await
    }

    async fn send(&self, operation: Operation) -> Body {
        match self.send_raw(operation).await {
            Response::MeshRoute { response } => response.body,
            other => panic!("route failed: {other:?}"),
        }
    }

    /// Current guard from durable status; receipts are historical snapshots.
    async fn guard(&self, target: &AgentTarget) -> ControlGuard {
        let status = self.status(&target.agent_id).await;
        assert_eq!(status.target, *target);
        ControlGuard {
            target: status.target,
            epoch: status.epoch,
            revision: status.revision,
        }
    }

    async fn window(&self) -> u64 {
        match self.send(Operation::IssueWindow { ttl_seconds: 600 }).await {
            Body::Window { window, .. } => window,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn spawn_op(&self, window: u64, id: &str, commit: &str) -> Operation {
        Operation::Spawn {
            mutation: Mutation {
                window,
                id: id.into(),
            },
            export: self.export.clone(),
            runtime: "claude".into(),
            source_commit: commit.into(),
            prompt: "hello from host".into(),
        }
    }

    async fn status(&self, agent: &str) -> pu_core::mesh::response::AgentSummary {
        match self
            .send(Operation::Status {
                export: self.export.clone(),
                agent_id: Some(agent.into()),
                after_agent: None,
                limit: 1,
            })
            .await
        {
            Body::Status { mut agents, .. } => agents.remove(0),
            other => panic!("unexpected {other:?}"),
        }
    }

    async fn wait_lifecycle(&self, agent: &str, wanted: Lifecycle) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let status = self.status(agent).await;
            if status.lifecycle == wanted {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "lifecycle stayed {:?}, wanted {wanted:?}",
                status.lifecycle
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn marked_alive(&self) -> bool {
        std::process::Command::new("pgrep")
            .args(["-f", &self.marker])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false)
    }
}

fn evidence(body: Body) -> ReceiptEvidence {
    match body {
        Body::Receipt {
            evidence: Some(evidence),
        } => evidence,
        other => panic!("expected receipt, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_enrolled_worker_should_spawn_observe_input_and_stop_original_run_without_replay() {
    // given
    let fleet = fleet(BTreeSet::from([
        Capability::Inspect,
        Capability::ReadOutput,
        Capability::Spawn,
        Capability::Control,
        Capability::Input,
        Capability::Stop,
    ]))
    .await;
    let window = fleet.window().await;

    // when: spawn, then repeat the identical operation
    let spawned = evidence(
        fleet
            .send(fleet.spawn_op(window, "spawn-1", &fleet.commit))
            .await,
    );
    let duplicate = evidence(
        fleet
            .send(fleet.spawn_op(window, "spawn-1", &fleet.commit))
            .await,
    );

    // then: one durable worker-owned agent/run; the duplicate launched nothing
    assert_eq!(spawned.target, duplicate.target);
    fleet
        .wait_lifecycle(&spawned.target.agent_id, Lifecycle::Running)
        .await;
    match fleet
        .send(Operation::Status {
            export: fleet.export.clone(),
            agent_id: None,
            after_agent: None,
            limit: 64,
        })
        .await
    {
        Body::Status { agents, .. } => assert_eq!(agents.len(), 1),
        other => panic!("unexpected {other:?}"),
    }
    // A conflicting payload under the same operation identity is rejected.
    let other_commit = "0".repeat(40);
    assert!(matches!(
        fleet
            .send(fleet.spawn_op(window, "spawn-1", &other_commit))
            .await,
        Body::Error { .. }
    ));

    // when: observe output over the paired bulk connection
    let (subscribed, mut watch) = fleet
        .host
        .server
        .mesh()
        .watch(LocalRoute {
            worker: fleet.worker_identity.fingerprint.clone(),
            request: fleet.request(Operation::SubscribeOutput {
                target: spawned.target.clone(),
                cursor: None,
            }),
        })
        .await
        .unwrap();
    assert!(matches!(subscribed.body, Body::Subscribed { .. }));
    // The guarded runtime is ready once it prints READY on the PTY.
    let started = Instant::now();
    let mut early = Vec::new();
    while !String::from_utf8_lossy(&early).contains("READY") {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "agent never started"
        );
        if let Ok(Ok(frame)) = tokio::time::timeout(Duration::from_secs(3), watch.next()).await {
            early.extend_from_slice(&frame.data);
        }
    }

    // when: one writer, exact input acknowledgements
    let current = fleet.guard(&spawned.target).await;
    let writer = match fleet
        .send(Operation::OpenWriter {
            guard: current.clone(),
            previous: None,
        })
        .await
    {
        Body::Writer { writer, .. } => writer,
        other => panic!("unexpected {other:?}"),
    };
    let input = |sequence: u64, data: &[u8]| Operation::Input {
        guard: current.clone(),
        writer,
        sequence,
        data: data.to_vec(),
    };
    let first = fleet.send(input(1, b"ping-from-host\n")).await;
    match &first {
        Body::InputReceipt {
            evidence: Some(evidence),
        } => assert_eq!(evidence.outcome, InputOutcome::Written),
        other => panic!("unexpected {other:?}"),
    }
    // A repeated sequence returns the same evidence and writes nothing again.
    assert_eq!(fleet.send(input(1, b"ping-from-host\n")).await, first);
    assert_eq!(
        fleet.send(input(1, b"different-bytes\n")).await,
        Body::Error {
            code: ErrorCode::Conflict
        }
    );
    assert_eq!(
        fleet.send(input(3, b"skipped\n")).await,
        Body::Error {
            code: ErrorCode::SequenceGap
        }
    );

    // then: the echoed input arrives exactly once in the observed output
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&seen).contains("ping-from-host") {
        assert!(
            Instant::now() < deadline,
            "output: {:?}",
            String::from_utf8_lossy(&seen)
        );
        let frame = tokio::time::timeout(Duration::from_secs(5), watch.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.target, spawned.target);
        assert!(frame.gap.is_none());
        seen.extend_from_slice(&frame.data);
    }
    assert!(
        fleet.marked_alive(),
        "marked descendant should be running; output: {:?}",
        String::from_utf8_lossy(&seen)
    );

    // when: stop the original run
    let stop = Operation::Stop {
        guard: fleet.guard(&spawned.target).await,
        mutation: Mutation {
            window,
            id: "stop-1".into(),
        },
    };
    let stopped = evidence(fleet.send(stop.clone()).await);
    assert_eq!(stopped.target, spawned.target);

    // then: guardian-verified whole-tree termination releases the run
    fleet
        .wait_lifecycle(&spawned.target.agent_id, Lifecycle::Terminated)
        .await;
    assert!(!fleet.marked_alive(), "detached descendant survived stop");
    drop(watch);
    drop(fleet.project.path().to_path_buf());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_missing_source_or_ungranted_operation_should_fail_explicitly_without_launch() {
    // given
    let fleet = fleet(BTreeSet::from([Capability::Inspect, Capability::Spawn])).await;
    let window = fleet.window().await;
    // when / then: no commit fallback
    assert_eq!(
        fleet
            .send(fleet.spawn_op(window, "missing", &"f".repeat(40)))
            .await,
        Body::Error {
            code: ErrorCode::SourceMissing
        }
    );
    // an ungranted runtime is denied
    let mut ungranted = fleet.spawn_op(window, "codex", &fleet.commit);
    if let Operation::Spawn { runtime, .. } = &mut ungranted {
        *runtime = "codex".into();
    }
    match fleet.send_raw(ungranted).await {
        Response::Error { code, .. } => assert_eq!(code, "MESH_DENIED"),
        other => panic!("unexpected {other:?}"),
    }
    // nothing was admitted
    match fleet
        .send(Operation::Status {
            export: fleet.export.clone(),
            agent_id: None,
            after_agent: None,
            limit: 64,
        })
        .await
    {
        Body::Status { agents, .. } => assert!(agents.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
    // an ungranted operation is refused by the session before dispatch
    let suspend = Operation::Suspend {
        guard: ControlGuard {
            target: AgentTarget {
                export: fleet.export.clone(),
                agent_id: "agent".into(),
                run_id: "run".into(),
            },
            epoch: 1,
            revision: 1,
        },
        mutation: Mutation {
            window,
            id: "suspend".into(),
        },
    };
    match fleet.send_raw(suspend).await {
        Response::Error { code, .. } => assert_eq!(code, "MESH_DENIED"),
        other => panic!("unexpected {other:?}"),
    }
}
