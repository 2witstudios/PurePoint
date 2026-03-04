use pu_core::protocol::{Request, Response};
use tempfile::TempDir;

use crate::engine::Engine;

/// Shared test helper: initialise a project and spawn a root agent.
/// Returns (engine, agent_id, _tmp).
pub(crate) async fn init_and_spawn() -> (Engine, String, TempDir) {
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let pr = project_root.to_string_lossy().to_string();

    let engine = Engine::new();

    // Init
    engine
        .handle_request(Request::Init {
            project_root: pr.clone(),
        })
        .await;

    // Spawn with --root so no git worktree needed
    let resp = engine
        .handle_request(Request::Spawn {
            project_root: pr,
            prompt: "hello".into(),
            agent: "claude".into(),
            name: None,
            base: None,
            root: true,
            worktree: None,
        })
        .await;

    let agent_id = match resp {
        Response::SpawnResult { agent_id, .. } => agent_id,
        other => panic!("expected SpawnResult, got {other:?}"),
    };

    (engine, agent_id, tmp)
}
