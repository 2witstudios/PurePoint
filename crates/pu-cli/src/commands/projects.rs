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
            let root = resolve(path, Default::Init)?;
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
            let project_root = resolve(path, Default::Explicit)?;
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

/// Where a command without a path argument looks for its project.
enum Default {
    /// As `pu init`: --project, then PU_PROJECT_ROOT (set for agents), then cwd.
    Init,
    /// --project, then cwd. Removal ignores an inherited PU_PROJECT_ROOT, so it
    /// never removes a project for every device that the caller did not name.
    Explicit,
}

/// A path argument wins over --project and the current directory. On another
/// machine the root must be absolute: a local relative path means nothing there.
fn resolve(path: Option<String>, default: Default) -> Result<String, CliError> {
    let path = match (path, default) {
        (Some(path), _) => path,
        (None, Default::Init) => crate::commands::project_root_string()?,
        (None, Default::Explicit) => match crate::commands::explicit_project() {
            Some(root) => root,
            None => match crate::machine_target::remote_machine() {
                Some(machine) => return Err(crate::commands::remote_project_required(machine)),
                None => crate::commands::cwd_string()?,
            },
        },
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
