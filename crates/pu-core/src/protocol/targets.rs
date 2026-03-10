use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillTarget {
    Agent(String),
    Worktree(String),
    All,
    AllWorktrees,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspendTarget {
    Agent(String),
    All,
}
