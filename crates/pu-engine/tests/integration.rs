use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn start_server(sock: &std::path::Path) -> tokio::task::JoinHandle<()> {
    let engine = pu_engine::engine::Engine::new();
    let server = pu_engine::ipc_server::IpcServer::bind(sock, engine).unwrap();
    tokio::spawn(async move {
        server.run().await.ok();
    })
}

async fn send(sock: &std::path::Path, req: &Request) -> Response {
    let stream = UnixStream::connect(sock).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let json = serde_json::to_string(req).unwrap();
    writer
        .write_all(format!("{json}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn given_full_lifecycle_should_init_status_and_shutdown() {
    let tmp = TempDir::new().unwrap();
    let sock = tmp.path().join("daemon.sock");
    let project = tmp.path().join("myproject");
    std::fs::create_dir_all(&project).unwrap();

    let handle = start_server(&sock).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Health check
    let resp = send(&sock, &Request::Health).await;
    match resp {
        Response::HealthReport {
            protocol_version,
            pid,
            ..
        } => {
            assert_eq!(protocol_version, PROTOCOL_VERSION);
            assert!(pid > 0);
        }
        other => panic!("expected HealthReport, got {other:?}"),
    }

    // Init
    let resp = send(
        &sock,
        &Request::Init {
            project_root: project.to_string_lossy().into(),
        },
    )
    .await;
    assert!(matches!(resp, Response::InitResult { created: true }));

    // Init again (should say already initialized)
    let resp = send(
        &sock,
        &Request::Init {
            project_root: project.to_string_lossy().into(),
        },
    )
    .await;
    assert!(matches!(resp, Response::InitResult { created: false }));

    // Manifest exists on disk
    assert!(project.join(".pu/manifest.json").exists());
    assert!(project.join(".pu/config.yaml").exists());

    // Status (empty)
    let resp = send(
        &sock,
        &Request::Status {
            project_root: project.to_string_lossy().into(),
            agent_id: None,
        },
    )
    .await;
    match resp {
        Response::StatusReport {
            worktrees, agents, ..
        } => {
            assert!(worktrees.is_empty());
            assert!(agents.is_empty());
        }
        other => panic!("expected StatusReport, got {other:?}"),
    }

    // Shutdown
    let resp = send(&sock, &Request::Shutdown).await;
    assert!(matches!(resp, Response::ShuttingDown));

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(handle.is_finished());
}

#[tokio::test(flavor = "current_thread")]
async fn given_manifest_should_be_readable_by_macos_app() {
    let tmp = TempDir::new().unwrap();
    let sock = tmp.path().join("daemon.sock");
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let handle = start_server(&sock).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Init to create manifest
    send(
        &sock,
        &Request::Init {
            project_root: project.to_string_lossy().into(),
        },
    )
    .await;

    // Read the manifest as raw JSON and verify camelCase format
    let content = std::fs::read_to_string(project.join(".pu/manifest.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert!(v.get("projectRoot").is_some());
    assert!(v.get("createdAt").is_some());
    assert!(v.get("updatedAt").is_some());
    assert!(v.get("worktrees").is_some());
    assert!(v.get("version").is_some());

    // No snake_case keys
    assert!(v.get("project_root").is_none());
    assert!(v.get("created_at").is_none());

    send(&sock, &Request::Shutdown).await;
    handle.await.ok();
}

/// Test harness that handles server setup, project init, and shutdown.
struct TestHarness {
    _tmp: TempDir,
    sock: std::path::PathBuf,
    project: std::path::PathBuf,
    handle: tokio::task::JoinHandle<()>,
}

impl TestHarness {
    /// Start a server and initialise a project.
    async fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("daemon.sock");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let handle = start_server(&sock).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        send(
            &sock,
            &Request::Init {
                project_root: project.to_string_lossy().into(),
            },
        )
        .await;

        Self {
            _tmp: tmp,
            sock,
            project,
            handle,
        }
    }

    /// Start a server without initialising a project.
    async fn new_bare() -> Self {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("daemon.sock");
        let project = tmp.path().join("project");

        let handle = start_server(&sock).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        Self {
            _tmp: tmp,
            sock,
            project,
            handle,
        }
    }

    fn project_root(&self) -> String {
        self.project.to_string_lossy().into()
    }

    async fn send(&self, req: &Request) -> Response {
        send(&self.sock, req).await
    }

    async fn shutdown(self) {
        send(&self.sock, &Request::Shutdown).await;
        self.handle.await.ok();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_kill_all_on_empty_project_should_return_empty_killed() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::Kill {
            project_root: h.project_root(),
            target: pu_core::protocol::KillTarget::All,
        })
        .await;
    match resp {
        Response::KillResult { killed, .. } => {
            assert!(killed.is_empty());
        }
        other => panic!("expected KillResult, got {other:?}"),
    }

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_kill_nonexistent_agent_should_return_kill_result() {
    let h = TestHarness::new().await;

    // Engine returns KillResult even for nonexistent agents (best-effort kill)
    let resp = h
        .send(&Request::Kill {
            project_root: h.project_root(),
            target: pu_core::protocol::KillTarget::Agent("ag-nonexistent".into()),
        })
        .await;
    match resp {
        Response::KillResult { killed, .. } => {
            assert_eq!(killed, vec!["ag-nonexistent"]);
        }
        other => panic!("expected KillResult, got {other:?}"),
    }

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_grid_get_layout_should_return_layout() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::GridCommand {
            project_root: h.project_root(),
            command: pu_core::protocol::GridCommand::GetLayout,
        })
        .await;
    assert!(
        matches!(resp, Response::GridLayout { .. }),
        "expected GridLayout, got {resp:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_grid_split_should_succeed() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::GridCommand {
            project_root: h.project_root(),
            command: pu_core::protocol::GridCommand::Split {
                leaf_id: None,
                axis: "v".into(),
            },
        })
        .await;
    assert!(
        !matches!(resp, Response::Error { .. }),
        "expected success for grid split, got {resp:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_logs_for_nonexistent_agent_should_return_error() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::Logs {
            agent_id: "ag-nonexistent".into(),
            tail: 100,
        })
        .await;
    assert!(
        matches!(resp, Response::Error { .. }),
        "expected Error for nonexistent agent logs, got {resp:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_rename_nonexistent_agent_should_return_error() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::Rename {
            project_root: h.project_root(),
            agent_id: "ag-nonexistent".into(),
            name: "new-name".into(),
        })
        .await;
    assert!(
        matches!(resp, Response::Error { .. }),
        "expected Error for nonexistent agent rename, got {resp:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_status_for_nonexistent_agent_should_return_error() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::Status {
            project_root: h.project_root(),
            agent_id: Some("ag-nonexistent".into()),
        })
        .await;
    assert!(
        matches!(resp, Response::Error { .. }),
        "expected Error for nonexistent agent status, got {resp:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_delete_nonexistent_worktree_should_return_error() {
    let h = TestHarness::new().await;

    let resp = h
        .send(&Request::DeleteWorktree {
            project_root: h.project_root(),
            worktree_id: "wt-nonexistent".into(),
        })
        .await;
    assert!(
        matches!(resp, Response::Error { .. }),
        "expected Error for nonexistent worktree delete, got {resp:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn given_uninitialised_project_kill_should_return_error() {
    let h = TestHarness::new_bare().await;

    let resp = h
        .send(&Request::Kill {
            project_root: "/nonexistent/project".into(),
            target: pu_core::protocol::KillTarget::All,
        })
        .await;
    assert!(
        matches!(resp, Response::Error { .. }),
        "expected Error for uninitialised project, got {resp:?}"
    );

    h.shutdown().await;
}
