use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::agent::AgentEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeStatus {
    Active,
    Merging,
    Merged,
    Failed,
    Cleaned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub status: WorktreeStatus,
    pub agents: IndexMap<String, AgentEntry>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentStatus, WorktreeStatus};
    use serde_json;

    #[test]
    fn given_worktree_status_should_round_trip_json() {
        let statuses = vec![
            WorktreeStatus::Active,
            WorktreeStatus::Merging,
            WorktreeStatus::Merged,
            WorktreeStatus::Failed,
            WorktreeStatus::Cleaned,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: WorktreeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn given_worktree_entry_should_round_trip_json() {
        let mut agents = IndexMap::new();
        agents.insert(
            "ag-1".to_string(),
            AgentEntry {
                id: "ag-1".into(),
                name: "claude".into(),
                agent_type: "claude".into(),
                status: AgentStatus::Running,
                prompt: Some("test".into()),
                started_at: chrono::Utc::now(),
                completed_at: None,
                exit_code: None,
                error: None,
                pid: None,
                session_id: None,
                suspended_at: None,
                suspended: false,
                command: None,
                plan_mode: false,
                trigger_seq_index: None,
                trigger_state: None,
                trigger_total: None,
                gate_attempts: None,
                no_trigger: false,
                trigger_name: None,
            },
        );
        let entry = WorktreeEntry {
            id: "wt-abc".into(),
            name: "fix-auth".into(),
            path: "/tmp/wt".into(),
            branch: "pu/fix-auth".into(),
            base_branch: Some("main".into()),
            status: WorktreeStatus::Active,
            agents,
            created_at: chrono::Utc::now(),
            merged_at: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: WorktreeEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "wt-abc");
        assert_eq!(parsed.branch, "pu/fix-auth");
        assert!(parsed.agents.contains_key("ag-1"));
    }
}
