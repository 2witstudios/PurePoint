use pu_core::read::Runtime;
use pu_engine::read_source::{Binding, save_binding};
use pu_engine::{engine::Engine, ipc_server::IpcServer};
use std::path::Path;

async fn cli(socket: &Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("--socket")
        .arg(socket)
        .args(args)
        .env_remove("PU_PROJECT_ROOT")
        .output()
        .await
        .unwrap()
}
async fn json(socket: &Path, args: &[&str]) -> serde_json::Value {
    let output = cli(socket, args).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_native_sources_should_read_all_runtimes_through_real_cli_and_ipc_after_restart() {
    // given
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let root_s = root.to_str().unwrap();
    let socket = root.join("test.sock");
    let registry = pu_core::paths::project_registry_path(&socket);
    let engine = Engine::with_project_registry(registry.clone()).unwrap();
    let server = IpcServer::bind(&socket, engine).unwrap();
    let task = tokio::spawn(async move {
        server.run().await.unwrap();
    });
    json(&socket, &["--project", root_s, "init", "--json"]).await;
    for (runtime, kind, id, data) in [
        (Runtime::Claude,"claude","claude1",serde_json::json!({"type":"assistant","uuid":"a","sessionId":"claude1","message":{"content":[{"type":"text","text":"Claude result"}]}}).to_string()),
        (Runtime::Codex,"codex","codex1",format!("{}\n{}",serde_json::json!({"type":"session_meta","payload":{"id":"codex1","cwd":root_s,"source":"cli"}}),serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Codex result"}]}}))),
        (Runtime::Pi,"pi","pi1",format!("{}\n{}",serde_json::json!({"type":"session","id":"pi1","cwd":root_s}),serde_json::json!({"type":"message","id":"a","parentId":null,"message":{"role":"assistant","content":[{"type":"text","text":"Pi result"}]}}))),
    ] {
        let source = root.join(format!("{id}.jsonl")); std::fs::write(&source, format!("{data}\n")).unwrap();
        pu_core::manifest::update_manifest(&root, |mut m| {
            m.agents.insert(format!("ag-{kind}"), serde_json::from_value(serde_json::json!({"id":format!("ag-{kind}"),"name":kind,"agentType":kind,"sessionId":id,"status":"running","startedAt":"2026-10-10T00:00:00Z","suspended":true})).unwrap()); m
        }).unwrap();
        save_binding(&root,&format!("ag-{kind}"),&Binding {version:1,runtime,session_id:id.into(),path:source,cwd:root_s.into()}).unwrap();
    }
    let storage = root.join("storage");
    for dir in ["session/project", "message/ses_exact", "part/msg1"] {
        std::fs::create_dir_all(storage.join(dir)).unwrap();
    }
    let source = storage.join("session/project/ses_exact.json");
    std::fs::write(
        &source,
        serde_json::json!({"id":"ses_exact","directory":root_s}).to_string(),
    )
    .unwrap();
    std::fs::write(storage.join("message/ses_exact/msg1.json"),serde_json::json!({"id":"msg1","sessionID":"ses_exact","role":"assistant","time":{"created":1}}).to_string()).unwrap();
    std::fs::write(storage.join("part/msg1/part1.json"),serde_json::json!({"id":"part1","sessionID":"ses_exact","messageID":"msg1","type":"text","text":"OpenCode result"}).to_string()).unwrap();
    pu_core::manifest::update_manifest(&root,|mut m| { m.agents.insert("ag-opencode".into(),serde_json::from_value(serde_json::json!({"id":"ag-opencode","name":"opencode","agentType":"opencode","sessionId":"ses_exact","status":"running","startedAt":"2026-10-10T00:00:00Z","suspended":true})).unwrap());m }).unwrap();
    save_binding(
        &root,
        "ag-opencode",
        &Binding {
            version: 1,
            runtime: Runtime::Opencode,
            session_id: "ses_exact".into(),
            path: source,
            cwd: root_s.into(),
        },
    )
    .unwrap();
    // when: replace the daemon, retaining only persisted state
    task.abort();
    let _ = task.await;
    std::fs::remove_file(&socket).unwrap();
    let server =
        IpcServer::bind(&socket, Engine::with_project_registry(registry).unwrap()).unwrap();
    let task = tokio::spawn(async move {
        server.run().await.unwrap();
    });
    // then: targeted reads resolve ownership globally, without any live AI process
    for (kind, text) in [
        ("claude", "Claude result"),
        ("codex", "Codex result"),
        ("opencode", "OpenCode result"),
        ("pi", "Pi result"),
    ] {
        let agent = format!("ag-{kind}");
        let report = json(&socket, &["read", &agent, "--json"]).await;
        assert_eq!(report["source_kind"], "transcript");
        assert_eq!(report["records"][0]["text"], text);
        let cursor = report["next_cursor"].as_str().unwrap();
        let next = json(&socket, &["read", &agent, "--since", cursor, "--json"]).await;
        assert_eq!(next["records"].as_array().unwrap().len(), 0);
        let bad = cli(&socket, &["read", &agent, "--since", "invalid", "--json"]).await;
        assert!(!bad.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bad.stdout).unwrap()["code"],
            "READ_INVALID_CURSOR"
        );
    }
    let other = root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    json(
        &socket,
        &["--project", other.to_str().unwrap(), "init", "--json"],
    )
    .await;
    let scoped = cli(
        &socket,
        &[
            "--project",
            other.to_str().unwrap(),
            "read",
            "ag-claude",
            "--json",
        ],
    )
    .await;
    assert!(!scoped.status.success());
    pu_core::manifest::update_manifest(&root, |mut m| { m.agents.insert("ag-custom".into(),serde_json::from_value(serde_json::json!({"id":"ag-custom","name":"custom","agentType":"custom","status":"running","startedAt":"2026-10-10T00:00:00Z","suspended":true})).unwrap());m }).unwrap();
    let unsupported = cli(&socket, &["read", "ag-custom", "--json"]).await;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unsupported.stdout).unwrap()["code"],
        "READ_UNSUPPORTED_RUNTIME"
    );
    let terminal = json(
        &socket,
        &[
            "--project",
            root_s,
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
    let id = terminal["agent_id"].as_str().unwrap();
    json(&socket, &["send", id, "hello terminal", "--json"]).await;
    let report = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let r = json(&socket, &["read", id, "--json"]).await;
            if r["records"].as_array().unwrap().iter().any(|x| {
                x["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("hello terminal"))
            }) {
                break r;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(report["source_kind"], "terminal_text");
    let human = cli(&socket, &["read", id]).await;
    assert!(String::from_utf8_lossy(&human.stdout).starts_with("Terminal text"));
    let raw = cli(&socket, &["logs", id, "--json"]).await;
    assert!(raw.status.success());
    assert!(String::from_utf8_lossy(&raw.stderr).contains("deprecated"));
    json(&socket, &["kill", "--agent", id, "--json"]).await;
    task.abort();
    let _ = task.await;
}
