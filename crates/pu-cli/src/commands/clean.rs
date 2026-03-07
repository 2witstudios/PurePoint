use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, Response};
use std::path::Path;

pub async fn run(
    socket: &Path,
    worktree: Option<String>,
    all: bool,
    json: bool,
) -> Result<(), CliError> {
    if !all && worktree.is_none() {
        return Err(CliError::Other(
            "clean target required — use --worktree <id> or --all".into(),
        ));
    }

    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = crate::commands::cwd_string()?;

    if all {
        // Get status to find all worktree IDs
        let status_resp = client::send_request(
            socket,
            &Request::Status {
                project_root: project_root.clone(),
                agent_id: None,
            },
        )
        .await?;
        let status_resp = output::check_response(status_resp, json)?;

        let wt_ids: Vec<String> = match &status_resp {
            Response::StatusReport { worktrees, .. } => {
                worktrees.iter().map(|wt| wt.id.clone()).collect()
            }
            _ => vec![],
        };

        if wt_ids.is_empty() {
            if json {
                println!("[]");
            } else {
                println!("No worktrees to clean");
            }
            return Ok(());
        }

        for wt_id in &wt_ids {
            let resp = client::send_request(
                socket,
                &Request::DeleteWorktree {
                    project_root: project_root.clone(),
                    worktree_id: wt_id.clone(),
                },
            )
            .await?;
            let resp = output::check_response(resp, json)?;
            output::print_response(&resp, json);
        }
    } else if let Some(wt_id) = worktree {
        let resp = client::send_request(
            socket,
            &Request::DeleteWorktree {
                project_root,
                worktree_id: wt_id,
            },
        )
        .await?;
        let resp = output::check_response(resp, json)?;
        output::print_response(&resp, json);
    }

    Ok(())
}
