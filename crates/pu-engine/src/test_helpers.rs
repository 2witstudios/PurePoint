use pu_core::protocol::{Request, Response};
use tempfile::TempDir;

use crate::engine::Engine;

/// Shared test helper: initialise a project and spawn a root agent.
/// Returns (engine, agent_id, _tmp).
pub(crate) async fn init_and_spawn() -> (Engine, String, TempDir) {
    init_and_spawn_command(None).await
}

/// Use a controlled PTY command when a test requires deterministic output.
pub(crate) async fn init_and_spawn_command(command: Option<&str>) -> (Engine, String, TempDir) {
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
            prompt: String::new(),
            agent: "terminal".into(),
            name: None,
            base: None,
            root: true,
            worktree: None,
            command: command.map(str::to_owned),
            no_auto: false,
            extra_args: vec![],
            plan_mode: false,
            no_trigger: false,
            trigger: None,
            placement: None,
        })
        .await;

    let agent_id = match resp {
        Response::SpawnResult { agent_id, .. } => agent_id,
        other => panic!("expected SpawnResult, got {other:?}"),
    };

    (engine, agent_id, tmp)
}

/// A PTY program that prints nothing of its own and echoes what it is sent:
/// `cat` on Unix; `findstr` on Windows, where the console's line editing
/// echoes typed input (ConPTY has no `cat`).
#[cfg(unix)]
pub(crate) const ECHO_COMMAND: &str = "/bin/cat";
#[cfg(windows)]
pub(crate) const ECHO_COMMAND: &str = "findstr.exe x*";

/// Type `marker` and Enter into an [`ECHO_COMMAND`] session and wait until
/// the terminal echoes it: the terminal's startup paint (a ConPTY clears the
/// screen first) is in the buffer by then. (conhost may still retitle the
/// window afterwards, which draws nothing.)
pub(crate) async fn wait_for_echo(engine: &Engine, agent_id: &str, marker: &[u8]) {
    let mut line = marker.to_vec();
    // The console's line editing ends a line on Enter (`\r`); a tty on `\n`.
    line.push(if cfg!(windows) { b'\r' } else { b'\n' });
    assert!(matches!(
        engine
            .handle_request(Request::Input {
                agent_id: agent_id.into(),
                data: line,
                submit: false,
                cancel_on_disconnect: false,
            })
            .await,
        Response::Ok
    ));
    let buffer = engine.get_attach_handles(agent_id).await.unwrap().0;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while !buffer
            .read_from(0)
            .0
            .windows(marker.len())
            .any(|w| w == marker)
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "echo session never echoed its marker: {:?}",
            String::from_utf8_lossy(&buffer.read_from(0).0)
        )
    });
}
