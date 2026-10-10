use crate::types::WorktreeStatus;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryKind {
    Summary,
    Projects,
    Agents,
    Worktrees,
}

/// Observed process state, distinct from the persisted resumable agent status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentQueryState {
    Running,
    Suspended,
    Broken,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryCounts {
    pub projects: usize,
    pub worktrees: usize,
    pub agents: usize,
    pub running: usize,
    pub suspended: usize,
    pub broken: usize,
    pub unknown: usize,
    pub running_ai_agents: usize,
    pub running_terminals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInventoryEntry {
    pub project_root: String,
    pub name: String,
    pub available: bool,
    pub error: Option<String>,
    pub counts: InventoryCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInventoryEntry {
    pub project_root: Option<String>,
    pub cwd: String,
    pub worktree_id: Option<String>,
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub state: AgentQueryState,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub idle_seconds: Option<u64>,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInventoryEntry {
    pub project_root: String,
    pub id: String,
    pub name: String,
    pub path: String,
    pub branch: String,
    pub status: WorktreeStatus,
    pub agent_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReport {
    pub observed_at: DateTime<Utc>,
    /// None denotes the entire selected daemon's registered project inventory.
    pub project_root: Option<String>,
    pub complete: bool,
    /// Counts are for the whole scope, before the optional agent-state filter.
    pub summary: InventoryCounts,
    pub projects: Vec<ProjectInventoryEntry>,
    pub agents: Vec<AgentInventoryEntry>,
    pub worktrees: Vec<WorktreeInventoryEntry>,
}
