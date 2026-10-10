use pu_core::protocol::{Request, Response};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn wire(socket: &Path, request: Request) -> Response {
    let stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    writer
        .write_all(format!("{}\n", serde_json::to_string(&request).unwrap()).as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

async fn cli(socket: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("--socket")
        .arg(socket)
        .args(args)
        .current_dir(cwd)
        .env_remove("PU_PROJECT_ROOT")
        .env_remove("PU_AGENT_ID")
        .output()
        .await
        .unwrap()
}

async fn json(socket: &Path, cwd: &Path, args: &[&str]) -> serde_json::Value {
    let output = cli(socket, cwd, args).await;
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn git(root: &Path, args: &[&str]) {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_cli_outside_projects_should_query_and_control_agents_globally_with_explicit_scope() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("test.sock");
    let registry = pu_core::paths::project_registry_path(&socket);
    let engine = pu_engine::engine::Engine::with_project_registry(registry.clone()).unwrap();
    let server = pu_engine::ipc_server::IpcServer::bind(&socket, engine).unwrap();
    let handle = tokio::spawn(async move {
        server.run().await.unwrap();
    });
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let first_root = first.canonicalize().unwrap();
    let first_s = first_root.to_str().unwrap();
    let second_root = second.canonicalize().unwrap();
    let second_s = second_root.to_str().unwrap();
    for root in [first_s, second_s] {
        let result = json(&socket, &outside, &["--project", root, "init", "--json"]).await;
        assert_eq!(result["created"], true);
    }
    // Explicit project takes precedence over a builder's inherited project.
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("--socket")
        .arg(&socket)
        .args(["--project", first_s, "status", "--json"])
        .env("PU_PROJECT_ROOT", "/nonexistent-inherited-project")
        .current_dir(&outside)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let projects = json(&socket, &outside, &["projects", "list", "--json"]).await;
    assert_eq!(projects["summary"]["projects"], 2);
    assert_eq!(projects["complete"], true);
    assert_eq!(projects["projects"][0]["project_root"], first_s);
    let inherited = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("--socket")
        .arg(&socket)
        .args(["agents", "list", "--json"])
        .env("PU_PROJECT_ROOT", "/unrelated-builder-project")
        .current_dir(&outside)
        .output()
        .await
        .unwrap();
    assert!(inherited.status.success());
    let inherited: serde_json::Value = serde_json::from_slice(&inherited.stdout).unwrap();
    assert!(inherited["project_root"].is_null());
    assert_eq!(inherited["summary"]["projects"], 2);
    git(&first, &["init", "-b", "main"]).await;
    git(
        &first,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    )
    .await;
    let root_agent = json(
        &socket,
        &outside,
        &[
            "--project",
            second_s,
            "spawn",
            "--root",
            "--agent",
            "terminal",
            "--command",
            "/bin/cat",
            "--json",
        ],
    )
    .await;
    let root_id = root_agent["agent_id"].as_str().unwrap();
    let worktree_agent = json(
        &socket,
        &outside,
        &[
            "--project",
            first_s,
            "spawn",
            "--agent",
            "terminal",
            "--command",
            "/bin/cat",
            "--name",
            "feature",
            "--json",
        ],
    )
    .await;
    let wt_id = worktree_agent["agent_id"].as_str().unwrap();
    let summary = json(&socket, &outside, &["status", "--global", "--json"]).await;
    assert_eq!(summary["summary"]["running"], 2);
    assert_eq!(summary["summary"]["worktrees"], 1);
    assert_eq!(summary["agents"], serde_json::json!([]));
    let agents = json(
        &socket,
        &outside,
        &["agents", "list", "--global", "--state", "running", "--json"],
    )
    .await;
    assert_eq!(agents["agents"].as_array().unwrap().len(), 2);
    assert!(
        agents["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == wt_id
                && a["project_root"] == first_s
                && !a["worktree_id"].is_null())
    );
    let worktrees = json(&socket, &outside, &["worktrees", "list", "--json"]).await;
    assert_eq!(worktrees["worktrees"][0]["project_root"], first_s);
    let status = json(&socket, &outside, &["status", "--agent", wt_id, "--json"]).await;
    assert_eq!(status["id"], wt_id);
    let inherited_target = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("--socket")
        .arg(&socket)
        .args(["status", "--agent", wt_id, "--json"])
        .env("PU_PROJECT_ROOT", "/unrelated-builder-project")
        .current_dir(&outside)
        .output()
        .await
        .unwrap();
    assert!(
        inherited_target.status.success(),
        "{}",
        String::from_utf8_lossy(&inherited_target.stderr)
    );
    let inherited_target: serde_json::Value =
        serde_json::from_slice(&inherited_target.stdout).unwrap();
    assert_eq!(inherited_target["id"], wt_id);

    // Scoped session actions must reject an agent belonging to another project.
    let rejected = cli(
        &socket,
        &outside,
        &["--project", first_s, "send", root_id, "wrong", "--json"],
    )
    .await;
    assert!(!rejected.status.success());
    let rejected_json: serde_json::Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(rejected_json["type"], "error");
    json(
        &socket,
        &outside,
        &["send", root_id, "hello-global", "--json"],
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let logs = json(&socket, &outside, &["logs", root_id, "--json"]).await;
            if logs["data"].as_str().unwrap().contains("hello-global") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    json(
        &socket,
        &outside,
        &[
            "--project",
            second_s,
            "trigger",
            "create",
            "global-routing",
            "--on",
            "agent_idle",
            "--inject",
            "continue",
            "--json",
        ],
    )
    .await;
    let assigned = json(
        &socket,
        &outside,
        &["trigger", "assign", root_id, "global-routing", "--json"],
    )
    .await;
    assert_eq!(assigned["agent_id"], root_id);
    let bench = json(&socket, &outside, &["bench", root_id, "--json"]).await;
    assert_eq!(bench["suspended"][0], root_id);
    let after = json(&socket, &outside, &["status", "--global", "--json"]).await;
    assert_eq!(after["summary"]["running"], 1);
    assert_eq!(after["summary"]["suspended"], 1);
    let scoped = json(
        &socket,
        &outside,
        &["--project", second_s, "agents", "list", "--json"],
    )
    .await;
    assert_eq!(scoped["agents"].as_array().unwrap().len(), 1);
    let resumed = json(&socket, &outside, &["play", root_id, "--json"]).await;
    assert_eq!(resumed["agent_id"], root_id);
    let resumed_agents = json(
        &socket,
        &outside,
        &["agents", "list", "--state", "running", "--json"],
    )
    .await;
    assert!(
        resumed_agents["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == root_id && a["project_root"] == second_s)
    );
    json(&socket, &outside, &["kill", "--agent", root_id, "--json"]).await;
    let killed = json(&socket, &outside, &["kill", "--agent", wt_id, "--json"]).await;
    assert_eq!(killed["killed"][0], wt_id);
    // Live standalone shells remain visible without a project manifest.
    let shell_id = match wire(
        &socket,
        Request::SpawnShell {
            cwd: outside.to_string_lossy().into_owned(),
        },
    )
    .await
    {
        Response::SpawnResult { agent_id, .. } => agent_id,
        other => panic!("unexpected response: {other:?}"),
    };
    let shells = json(
        &socket,
        &outside,
        &["agents", "list", "--state", "running", "--json"],
    )
    .await;
    assert!(
        shells["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == shell_id && a["project_root"].is_null())
    );
    assert_eq!(
        json(
            &socket,
            &outside,
            &["status", "--agent", &shell_id, "--json"]
        )
        .await["id"],
        shell_id
    );
    assert_eq!(
        json(&socket, &outside, &["kill", "--agent", &shell_id, "--json"]).await["killed"][0],
        shell_id
    );
    assert!(matches!(
        wire(&socket, Request::Shutdown).await,
        Response::ShuttingDown
    ));
    handle.await.unwrap();
    let engine = pu_engine::engine::Engine::with_project_registry(registry).unwrap();
    let server = pu_engine::ipc_server::IpcServer::bind(&socket, engine).unwrap();
    let handle = tokio::spawn(async move {
        server.run().await.unwrap();
    });
    let restored = json(&socket, &outside, &["status", "--global", "--json"]).await;
    assert_eq!(restored["summary"]["projects"], 2);
    assert_eq!(restored["summary"]["running"], 0);
    wire(&socket, Request::Shutdown).await;
    handle.await.unwrap();
}
