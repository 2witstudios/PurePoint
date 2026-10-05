use serde::{Deserialize, Serialize};

/// Pane-grid commands. The daemon never models layout: these are validated by serde and
/// rebroadcast to the subscribed app, which owns the pane tree (`.pu/workspaces.json`).
///
/// Leaf ids identify panes; tab ids are stable per-workspace surface ids. Both are only unique
/// within one workspace, so every mutating command can name its `workspace_id`; `None` means
/// the workspace on screen in the app. `None` leaf ids mean "the focused pane", `None` tab ids
/// mean "the target pane's active tab".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GridCommand {
    Split {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default)]
        leaf_id: Option<u32>,
        #[serde(default = "default_axis")]
        axis: String,
    },
    Close {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default)]
        leaf_id: Option<u32>,
    },
    Focus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default)]
        leaf_id: Option<u32>,
        #[serde(default)]
        direction: Option<String>,
    },
    /// Set the content of the pane's active tab to an agent.
    SetAgent {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leaf_id: Option<u32>,
        agent_id: String,
    },
    GetLayout,
    /// Open a tab after the pane's active tab — showing `agent_id`, or empty if `None`.
    NewTab {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leaf_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
    },
    /// Select a tab by 1-based `index` or by `direction` ("next" | "prev").
    SelectTab {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leaf_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<String>,
    },
    CloseTab {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leaf_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab_id: Option<u32>,
    },
    /// Move a tab to `to_leaf` at 1-based `index` (append if `None`).
    MoveTab {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        tab_id: u32,
        to_leaf: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    /// Move a tab into a new pane split off its current pane.
    BreakTab {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab_id: Option<u32>,
        #[serde(default = "default_axis")]
        axis: String,
    },
}

fn default_axis() -> String {
    "v".to_string()
}
