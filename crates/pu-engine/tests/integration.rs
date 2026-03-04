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
