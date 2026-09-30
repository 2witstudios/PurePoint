use std::path::Path;

use pu_core::protocol::{GridCommand, Request, Response};

use crate::GridAction;
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(socket: &Path, action: GridAction) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::project_root_string()?;

    match action {
        GridAction::Show { json } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::GetLayout,
                },
            )
            .await?;
            let resp = output::check_response(resp, json)?;

            match resp {
                Response::GridLayout { layout } => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&layout)?);
                    } else {
                        print_ascii_grid(&layout);
                    }
                }
                _ => {
                    println!("No grid layout");
                }
            }
        }

        GridAction::Split { axis, leaf } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::Split {
                        leaf_id: leaf,
                        axis,
                    },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Split pane");
        }

        GridAction::Close { leaf } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::Close { leaf_id: leaf },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Closed pane");
        }

        GridAction::Focus { direction, leaf } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::Focus {
                        leaf_id: leaf,
                        direction,
                    },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Focus moved");
        }

        GridAction::Assign { agent_id, leaf } => {
            let leaf_id = leaf.unwrap_or(0);
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::SetAgent { leaf_id, agent_id },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Agent assigned");
        }
    }

    Ok(())
}

/// Render the workspace layout as ASCII boxes — one box per workspace, since a workspace
/// is the unit that owns a pane tree.
fn print_ascii_grid(layout: &serde_json::Value) {
    if layout.is_null() {
        println!("No grid layout");
        return;
    }

    let workspaces = match layout.get("workspaces").and_then(|w| w.as_array()) {
        Some(list) => list.clone(),
        // Pre-workspace `grid-layout.json`: the whole document is one tree.
        None => vec![layout.clone()],
    };

    if workspaces.is_empty() {
        println!("No workspaces");
        return;
    }

    for workspace in &workspaces {
        let tree = workspace.get("tree").unwrap_or(workspace);
        let mut leaves = Vec::new();
        collect_leaves(tree, &mut leaves);

        let label = workspace
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("workspace");

        let max_width = 28;
        let border_h = "─".repeat(max_width);
        println!("{label}");
        println!("┌{border_h}┐");
        if leaves.is_empty() {
            println!("│{:^max_width$}│", "(empty)");
        }
        for leaf in &leaves {
            let agent = leaf.as_deref().unwrap_or("(empty)");
            println!("│{agent:^max_width$}│");
        }
        println!("└{border_h}┘");
    }
}

fn collect_leaves(node: &serde_json::Value, out: &mut Vec<Option<String>>) {
    match node.get("type").and_then(|t| t.as_str()) {
        Some("leaf") => {
            let agent_id = node
                .get("agentId")
                .and_then(|a| a.as_str())
                .map(String::from);
            out.push(agent_id);
        }
        Some("split") => {
            if let Some(first) = node.get("first") {
                collect_leaves(first, out);
            }
            if let Some(second) = node.get("second") {
                collect_leaves(second, out);
            }
        }
        _ => {}
    }
}
