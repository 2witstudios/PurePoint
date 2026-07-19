use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::agent::AgentEntry;
use super::worktree::WorktreeEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub project_root: String,
    pub worktrees: IndexMap<String, WorktreeEntry>,
    #[serde(default)]
    pub agents: IndexMap<String, AgentEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Manifest {
    pub fn new(project_root: String) -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            project_root,
            worktrees: IndexMap::new(),
            agents: IndexMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn find_agent(&self, agent_id: &str) -> Option<AgentLocation<'_>> {
        if let Some(agent) = self.agents.get(agent_id) {
            return Some(AgentLocation::Root(agent));
        }
        for wt in self.worktrees.values() {
            if let Some(agent) = wt.agents.get(agent_id) {
                return Some(AgentLocation::Worktree {
                    worktree: wt,
                    agent,
                });
            }
        }
        None
    }

    pub fn all_agents(&self) -> Vec<&AgentEntry> {
        let mut agents: Vec<&AgentEntry> = self.agents.values().collect();
        for wt in self.worktrees.values() {
            agents.extend(wt.agents.values());
        }
        agents
    }

    pub fn find_agent_mut(&mut self, id: &str) -> Option<&mut AgentEntry> {
        if let Some(agent) = self.agents.get_mut(id) {
            return Some(agent);
        }
        for wt in self.worktrees.values_mut() {
            if let Some(agent) = wt.agents.get_mut(id) {
                return Some(agent);
            }
        }
        None
    }
}

pub enum AgentLocation<'a> {
    Root(&'a AgentEntry),
    Worktree {
        worktree: &'a WorktreeEntry,
        agent: &'a AgentEntry,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentStatus, WorktreeStatus};
    use indexmap::IndexMap;

    #[test]
    fn given_new_manifest_should_have_version_1_and_empty_collections() {
        let m = Manifest::new("/test".into());
        assert_eq!(m.version, 1);
        assert!(m.worktrees.is_empty());
        assert!(m.agents.is_empty());
        assert_eq!(m.project_root, "/test");
    }

    #[test]
    fn given_manifest_with_root_agent_should_find_by_id() {
        let mut m = Manifest::new("/test".into());
        m.agents.insert(
            "ag-1".into(),
            AgentEntry {
                id: "ag-1".into(),
                name: "claude".into(),
                agent_type: "claude".into(),
                status: AgentStatus::Running,
                prompt: None,
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
        assert!(matches!(m.find_agent("ag-1"), Some(AgentLocation::Root(_))));
        assert!(m.find_agent("ag-999").is_none());
    }

    #[test]
    fn given_manifest_with_worktree_agent_should_find_by_id() {
        let mut m = Manifest::new("/test".into());
        let mut agents = IndexMap::new();
        agents.insert(
            "ag-2".to_string(),
            AgentEntry {
                id: "ag-2".into(),
                name: "claude".into(),
                agent_type: "claude".into(),
                status: AgentStatus::Running,
                prompt: None,
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
        m.worktrees.insert(
            "wt-1".into(),
            WorktreeEntry {
                id: "wt-1".into(),
                name: "test".into(),
                path: "/tmp".into(),
                branch: "pu/test".into(),
                base_branch: None,
                status: WorktreeStatus::Active,
                agents,
                created_at: chrono::Utc::now(),
                merged_at: None,
                error: None,
            },
        );
        assert!(matches!(
            m.find_agent("ag-2"),
            Some(AgentLocation::Worktree { .. })
        ));
    }

    #[test]
    fn given_manifest_with_mixed_agents_should_return_all() {
        let mut m = Manifest::new("/test".into());
        let now = chrono::Utc::now();
        let make_agent = |id: &str| AgentEntry {
            id: id.into(),
            name: "claude".into(),
            agent_type: "claude".into(),
            status: AgentStatus::Running,
            prompt: None,
            started_at: now,
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
        };
        m.agents.insert("ag-root".into(), make_agent("ag-root"));
        let mut wt_agents = IndexMap::new();
        wt_agents.insert("ag-wt".to_string(), make_agent("ag-wt"));
        m.worktrees.insert(
            "wt-1".into(),
            WorktreeEntry {
                id: "wt-1".into(),
                name: "test".into(),
                path: "/tmp".into(),
                branch: "pu/test".into(),
                base_branch: None,
                status: WorktreeStatus::Active,
                agents: wt_agents,
                created_at: now,
                merged_at: None,
                error: None,
            },
        );
        let all = m.all_agents();
        assert_eq!(all.len(), 2);
    }
}
