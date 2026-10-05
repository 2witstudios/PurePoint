use std::path::Path;

use pu_core::protocol::{GridCommand, Request, Response};

use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use crate::{GridAction, TabAction};

pub async fn run(
    socket: &Path,
    workspace: Option<String>,
    action: GridAction,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::project_root_string()?;

    let (command, confirmation) = match grid_command(action, workspace) {
        GridRequest::Show { json } => {
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
            return Ok(());
        }
        GridRequest::Mutate {
            command,
            confirmation,
        } => (command, confirmation),
    };

    let resp = client::send_request(
        socket,
        &Request::GridCommand {
            project_root,
            command,
        },
    )
    .await?;
    output::check_response(resp, false)?;
    println!("{confirmation}");
    Ok(())
}

enum GridRequest {
    Show {
        json: bool,
    },
    Mutate {
        command: GridCommand,
        confirmation: &'static str,
    },
}

/// Map a `pu grid` subcommand to its wire command and a short confirmation.
/// `workspace_id` rides on every mutating command; `None` means the workspace on screen.
fn grid_command(action: GridAction, workspace_id: Option<String>) -> GridRequest {
    let (command, confirmation) = match action {
        GridAction::Show { json } => return GridRequest::Show { json },
        GridAction::Split { axis, leaf } => (
            GridCommand::Split {
                workspace_id,
                leaf_id: leaf,
                axis,
            },
            "Split pane",
        ),
        GridAction::Close { leaf } => (
            GridCommand::Close {
                workspace_id,
                leaf_id: leaf,
            },
            "Closed pane",
        ),
        GridAction::Focus { direction, leaf } => (
            GridCommand::Focus {
                workspace_id,
                leaf_id: leaf,
                direction,
            },
            "Focus moved",
        ),
        GridAction::Assign { agent_id, leaf } => (
            GridCommand::SetAgent {
                workspace_id,
                leaf_id: leaf,
                agent_id,
            },
            "Agent assigned",
        ),
        GridAction::Tab { action } => tab_command(action, workspace_id),
    };
    GridRequest::Mutate {
        command,
        confirmation,
    }
}

fn tab_command(action: TabAction, workspace_id: Option<String>) -> (GridCommand, &'static str) {
    match action {
        TabAction::New { leaf, agent } => (
            GridCommand::NewTab {
                workspace_id,
                leaf_id: leaf,
                agent_id: agent,
            },
            "Opened tab",
        ),
        TabAction::Select {
            index,
            next,
            prev: _,
            leaf,
        } => {
            let direction = match (index, next) {
                (Some(_), _) => None,
                (None, true) => Some("next".to_string()),
                (None, false) => Some("prev".to_string()),
            };
            (
                GridCommand::SelectTab {
                    workspace_id,
                    leaf_id: leaf,
                    index,
                    direction,
                },
                "Selected tab",
            )
        }
        TabAction::Close { leaf, tab } => (
            GridCommand::CloseTab {
                workspace_id,
                leaf_id: leaf,
                tab_id: tab,
            },
            "Closed tab",
        ),
        TabAction::Move { tab_id, to, index } => (
            GridCommand::MoveTab {
                workspace_id,
                tab_id,
                to_leaf: to,
                index,
            },
            "Moved tab",
        ),
        TabAction::Break { tab, axis } => (
            GridCommand::BreakTab {
                workspace_id,
                tab_id: tab,
                axis,
            },
            "Broke tab into new pane",
        ),
    }
}

fn print_ascii_grid(layout: &serde_json::Value) {
    print!("{}", render_grid(layout));
}

/// Render the workspace layout as text: per workspace, one line per pane in tree order,
/// listing its tabs by 1-based position and tab ID. `▸` marks the focused pane, `*` the active
/// tab, and `(active)` the workspace on screen — the target of commands without `--workspace`.
///
/// Handles the v3 document (tabs per leaf), v2 (one `agentId` per leaf, shown as a single
/// tab) and the pre-workspace `grid-layout.json` (the whole document is one tree).
fn render_grid(layout: &serde_json::Value) -> String {
    if layout.is_null() {
        return "No grid layout\n".to_string();
    }

    let workspaces = match layout.get("workspaces").and_then(|w| w.as_array()) {
        Some(list) => list.clone(),
        None => vec![layout.clone()],
    };

    if workspaces.is_empty() {
        return "No workspaces\n".to_string();
    }

    // The app records which workspace is on screen: commands without --workspace go there.
    let active = layout.get("activeWorkspaceId").and_then(|a| a.as_str());

    let mut out = String::new();
    for workspace in &workspaces {
        let label = workspace
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("workspace");
        let focused = workspace.get("focusedLeafId").and_then(|f| f.as_u64());
        let tree = workspace.get("tree").unwrap_or(workspace);
        let mut leaves = Vec::new();
        collect_leaves(tree, &mut leaves);

        out.push_str(label);
        if active == Some(label) {
            out.push_str("  (active)");
        }
        out.push('\n');
        if leaves.is_empty() {
            out.push_str("  (empty)\n");
        }
        for leaf in leaves {
            let leaf_id = leaf
                .get("leafId")
                .and_then(|l| l.as_u64())
                .map(|l| l.to_string())
                .unwrap_or_else(|| "?".to_string());
            let marker =
                if focused.is_some() && leaf.get("leafId").and_then(|l| l.as_u64()) == focused {
                    '▸'
                } else {
                    ' '
                };
            let tabs = render_tabs(leaf).join(" │ ");
            out.push_str(&format!("  {leaf_id} {marker} [{tabs}]\n"));
        }
    }
    out
}

/// One label per tab: `{position}{*?} {content} #{tab id}`. The position is what
/// `pu grid tab select N` takes; the ID is what `--tab` and `tab move` take.
fn render_tabs(leaf: &serde_json::Value) -> Vec<String> {
    let Some(tabs) = leaf.get("tabs").and_then(|t| t.as_array()) else {
        // v2/legacy leaf: a single implicit tab holding the leaf's agent.
        let content = leaf
            .get("agentId")
            .and_then(|a| a.as_str())
            .unwrap_or("(empty)");
        return vec![format!("1* {content}")];
    };

    let active = leaf.get("activeTabId").and_then(|a| a.as_u64());
    tabs.iter()
        .enumerate()
        .map(|(i, tab)| {
            let star = if active.is_some() && tab.get("id").and_then(|t| t.as_u64()) == active {
                "*"
            } else {
                ""
            };
            let id = tab
                .get("id")
                .and_then(|t| t.as_u64())
                .map(|id| format!(" #{id}"))
                .unwrap_or_default();
            format!("{}{star} {}{id}", i + 1, tab_content(tab))
        })
        .collect()
}

fn tab_content(tab: &serde_json::Value) -> String {
    match tab.get("kind").and_then(|k| k.as_str()) {
        Some("agent") => tab
            .get("agentId")
            .and_then(|a| a.as_str())
            .unwrap_or("(agent)")
            .to_string(),
        Some("file") => tab
            .get("path")
            .and_then(|p| p.as_str())
            .map(|p| {
                Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string())
            })
            .unwrap_or_else(|| "(file)".to_string()),
        _ => "(empty)".to_string(),
    }
}

fn collect_leaves<'a>(node: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
    match node.get("type").and_then(|t| t.as_str()) {
        Some("leaf") => out.push(node),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_v3_layout_should_list_each_pane_with_its_tabs() {
        // given
        let layout = json!({"version":3,"workspaces":[{"id":"ws-ag-x","worktreeId":null,
            "focusedLeafId":0,"nextLeafId":3,"nextSurfaceId":5,
            "tree":{"type":"split","axis":"vertical","ratio":0.5,
              "first":{"type":"leaf","leafId":0,"activeTabId":1,"tabs":[
                {"id":0,"kind":"agent","agentId":"ag-a"},
                {"id":1,"kind":"file","path":"/abs/README.md"},
                {"id":2,"kind":"empty"}]},
              "second":{"type":"leaf","leafId":1,"activeTabId":3,"tabs":[
                {"id":3,"kind":"agent","agentId":"ag-b"}]}}}]});

        // when
        let rendered = render_grid(&layout);

        // then
        assert_eq!(
            rendered,
            "ws-ag-x\n  0 ▸ [1 ag-a #0 │ 2* README.md #1 │ 3 (empty) #2]\n  1   [1* ag-b #3]\n"
        );
    }

    #[test]
    fn given_active_workspace_should_mark_it() {
        let layout = json!({"version":3,"activeWorkspaceId":"ws-b","workspaces":[
            {"id":"ws-a","focusedLeafId":0,"tree":{"type":"leaf","leafId":0,"activeTabId":0,
              "tabs":[{"id":0,"kind":"agent","agentId":"ag-a"}]}},
            {"id":"ws-b","focusedLeafId":0,"tree":{"type":"leaf","leafId":0,"activeTabId":0,
              "tabs":[{"id":0,"kind":"agent","agentId":"ag-b"}]}}]});

        assert_eq!(
            render_grid(&layout),
            "ws-a\n  0 ▸ [1* ag-a #0]\nws-b  (active)\n  0 ▸ [1* ag-b #0]\n"
        );
    }

    #[test]
    fn given_v2_layout_should_show_each_leaf_agent_as_a_single_active_tab() {
        // given
        let layout = json!({"version":2,"workspaces":[{"id":"ws-1","focusedLeafId":1,
            "tree":{"type":"split","axis":"horizontal","ratio":0.5,
              "first":{"type":"leaf","leafId":0,"agentId":"ag-a"},
              "second":{"type":"leaf","leafId":1}}}]});

        // when
        let rendered = render_grid(&layout);

        // then
        assert_eq!(rendered, "ws-1\n  0   [1* ag-a]\n  1 ▸ [1* (empty)]\n");
    }

    #[test]
    fn given_pre_workspace_layout_should_render_document_as_one_tree() {
        let layout =
            json!({"ownerAgentId":"ag-a","tree":{"type":"leaf","leafId":0,"agentId":"ag-a"}});
        assert_eq!(render_grid(&layout), "workspace\n  0   [1* ag-a]\n");
    }

    #[test]
    fn given_null_layout_should_report_no_layout() {
        assert_eq!(render_grid(&serde_json::Value::Null), "No grid layout\n");
    }

    #[test]
    fn given_document_without_workspaces_should_report_none() {
        let layout = json!({"version":3,"workspaces":[]});
        assert_eq!(render_grid(&layout), "No workspaces\n");
    }

    #[test]
    fn given_select_by_direction_should_send_direction_without_index() {
        // given
        let action = TabAction::Select {
            index: None,
            next: false,
            prev: true,
            leaf: Some(2),
        };

        // when
        let (command, _) = tab_command(action, None);

        // then
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"action":"select_tab","leaf_id":2,"direction":"prev"}"#
        );
    }

    #[test]
    fn given_select_by_index_should_send_index_without_direction() {
        let action = TabAction::Select {
            index: Some(3),
            next: false,
            prev: false,
            leaf: None,
        };
        let (command, _) = tab_command(action, None);
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"action":"select_tab","index":3}"#
        );
    }

    #[test]
    fn given_workspace_should_send_it_with_the_command() {
        let action = TabAction::Close {
            leaf: None,
            tab: Some(4),
        };
        let (command, _) = tab_command(action, Some("ws-ag-b".to_string()));
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"action":"close_tab","workspace_id":"ws-ag-b","tab_id":4}"#
        );
    }
}
