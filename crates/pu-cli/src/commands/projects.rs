use std::path::Path;

use pu_core::protocol::{InventoryKind, Request};

use crate::error::CliError;
use crate::{ProjectsAction, client, daemon_ctrl, output};

pub async fn run(socket: &Path, action: ProjectsAction) -> Result<(), CliError> {
    match action {
        ProjectsAction::List { json, .. } => {
            crate::commands::inventory::run(socket, InventoryKind::Projects, None, json).await
        }
        ProjectsAction::Add { path, json } => {
            let root = resolve(path)?;
            if crate::machine_target::remote_machine().is_none() {
                // Locally, adding is `pu init`: it also writes the agent context file.
                return crate::commands::init::run_at(socket, root, json).await;
            }
            send(socket, Request::Init { project_root: root }, json).await
        }
        ProjectsAction::Remove {
            path,
            suspend_agents,
            json,
        } => {
            let project_root = resolve(path)?;
            send(
                socket,
                Request::RemoveProject {
                    project_root,
                    suspend_agents,
                },
                json,
            )
            .await
        }
    }
}

async fn send(socket: &Path, request: Request, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let response = client::send_request(socket, &request).await?;
    let response = output::check_response(response, json)?;
    output::print_response(&response, json)
}

/// A path argument wins over --project and the current directory. On another
/// machine it must be absolute: a local relative path means nothing there.
fn resolve(path: Option<String>) -> Result<String, CliError> {
    let Some(path) = path else {
        return crate::commands::project_root_string();
    };
    if crate::machine_target::remote_machine().is_some() {
        if !Path::new(&path).is_absolute() {
            return Err(CliError::Other(format!(
                "{path} must be an absolute path on the machine"
            )));
        }
        return Ok(path);
    }
    Ok(std::path::absolute(&path)?.to_string_lossy().into_owned())
}
