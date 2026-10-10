use pu_core::paths;

use crate::error::CliError;
use crate::skill;

pub async fn run(socket: &std::path::Path, json: bool) -> Result<(), CliError> {
    crate::daemon_ctrl::ensure_daemon(socket).await?;
    let root = crate::commands::project_root_string()?;
    let response = crate::client::send_request(
        socket,
        &pu_core::protocol::Request::Init {
            project_root: root.clone(),
        },
    )
    .await?;
    let response = crate::output::check_response(response, json)?;
    if pu_core::paths::daemon_socket_path().is_ok_and(|default| default == socket) {
        skill::ensure_plugin_current();
    }
    write_agent_context(std::path::Path::new(&root));
    crate::output::print_response(&response, json)
}

fn write_agent_context(project_root: &std::path::Path) {
    let pu_dir = paths::pu_dir(project_root);
    let path = pu_dir.join("agent-context.md");
    if path.exists() {
        return;
    }
    // Write a stripped-down version of the skill content for non-Claude tools
    let content = skill::skill_content();
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("warning: failed to write agent-context.md: {e}");
    }
}
