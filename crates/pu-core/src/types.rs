use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Spawning,
    Running,
    Idle,
    Completed,
    Failed,
    Killed,
    Lost,
    Suspended,
}

impl AgentStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed | Self::Lost)
    }
}

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
pub struct AgentEntry {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub status: AgentStatus,
    pub prompt: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspended_at: Option<DateTime<Utc>>,
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
                return Some(AgentLocation::Worktree { worktree: wt, agent });
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub prompt_flag: Option<String>,
    #[serde(default = "default_true")]
    pub interactive: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_agent")]
    pub default_agent: String,
    #[serde(default = "default_agents")]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default = "default_env_files")]
    pub env_files: Vec<String>,
}

fn default_agent() -> String {
    "claude".to_string()
}

fn default_env_files() -> Vec<String> {
    vec![".env".to_string(), ".env.local".to_string()]
}

pub fn default_agents() -> HashMap<String, AgentConfig> {
    let mut map = HashMap::new();
    map.insert(
        "claude".to_string(),
        AgentConfig {
            name: "claude".to_string(),
            command: "claude".to_string(),
            prompt_flag: None,
            interactive: true,
        },
    );
    map.insert(
        "codex".to_string(),
        AgentConfig {
            name: "codex".to_string(),
            command: "codex".to_string(),
            prompt_flag: None,
            interactive: true,
        },
    );
    map.insert(
        "opencode".to_string(),
        AgentConfig {
            name: "opencode".to_string(),
            command: "opencode".to_string(),
            prompt_flag: None,
            interactive: true,
        },
    );
    map.insert(
        "terminal".to_string(),
        AgentConfig {
            name: "terminal".to_string(),
            command: "shell".to_string(), // sentinel — engine resolves to $SHELL
            prompt_flag: None,
            interactive: true,
        },
    );
    map
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_agent: default_agent(),
            agents: default_agents(),
            env_files: default_env_files(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    // --- AgentStatus ---

    #[test]
    fn given_agent_status_running_should_serialize_to_camel_case() {
        let json = serde_json::to_string(&AgentStatus::Running).unwrap();
        assert_eq!(json, r#""running""#);
    }

    #[test]
    fn given_agent_status_json_should_deserialize_from_camel_case() {
        let status: AgentStatus = serde_json::from_str(r#""completed""#).unwrap();
        assert_eq!(status, AgentStatus::Completed);
    }

    #[test]
    fn given_all_agent_statuses_should_round_trip_json() {
        let statuses = vec![
            AgentStatus::Spawning,
            AgentStatus::Running,
            AgentStatus::Idle,
            AgentStatus::Completed,
            AgentStatus::Failed,
            AgentStatus::Killed,
            AgentStatus::Lost,
            AgentStatus::Suspended,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn given_terminal_statuses_should_report_terminal() {
        assert!(AgentStatus::Completed.is_terminal());
        assert!(AgentStatus::Failed.is_terminal());
        assert!(AgentStatus::Killed.is_terminal());
        assert!(AgentStatus::Lost.is_terminal());
    }

    #[test]
    fn given_active_statuses_should_report_not_terminal() {
        assert!(!AgentStatus::Spawning.is_terminal());
        assert!(!AgentStatus::Running.is_terminal());
        assert!(!AgentStatus::Idle.is_terminal());
        assert!(!AgentStatus::Suspended.is_terminal());
    }

    // --- WorktreeStatus ---

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

    // --- AgentEntry ---

    #[test]
    fn given_agent_entry_should_serialize_camel_case_keys() {
        let entry = AgentEntry {
            id: "ag-abc".into(),
            name: "claude".into(),
            agent_type: "claude".into(),
            status: AgentStatus::Running,
            prompt: Some("fix bug".into()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(1234),
            session_id: None,
            suspended_at: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        // Should use camelCase per manifest compat
        assert!(json.contains("agentType"));
        assert!(json.contains("startedAt"));
        // Optional None fields with skip_serializing_if should be absent
        assert!(!json.contains("completedAt"));
        assert!(!json.contains("exitCode"));
        assert!(!json.contains("sessionId"));
    }

    #[test]
    fn given_agent_entry_json_should_deserialize_camel_case() {
        let json = r#"{
            "id": "ag-xyz",
            "name": "claude",
            "agentType": "claude",
            "status": "idle",
            "prompt": null,
            "startedAt": "2026-03-01T00:00:00Z",
            "pid": 5678
        }"#;
        let entry: AgentEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, "ag-xyz");
        assert_eq!(entry.status, AgentStatus::Idle);
        assert_eq!(entry.pid, Some(5678));
    }

    // --- WorktreeEntry ---

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

    // --- Manifest ---

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
                status: AgentStatus::Idle,
                prompt: None,
                started_at: chrono::Utc::now(),
                completed_at: None,
                exit_code: None,
                error: None,
                pid: None,
                session_id: None,
                suspended_at: None,
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
            },
        );
        let all = m.all_agents();
        assert_eq!(all.len(), 2);
    }

    // --- Config ---

    #[test]
    fn given_default_config_should_have_claude_agent() {
        let config = Config::default();
        assert_eq!(config.default_agent, "claude");
        assert!(config.agents.contains_key("claude"));
        let claude = &config.agents["claude"];
        assert_eq!(claude.command, "claude");
        assert!(claude.prompt_flag.is_none());
        assert!(claude.interactive);
    }

    #[test]
    fn given_default_config_should_have_codex_and_opencode_agents() {
        let config = Config::default();
        assert!(config.agents.contains_key("codex"));
        assert_eq!(config.agents["codex"].command, "codex");
        assert!(config.agents.contains_key("opencode"));
        assert_eq!(config.agents["opencode"].command, "opencode");
    }

    #[test]
    fn given_default_config_should_have_terminal_agent() {
        let config = Config::default();
        assert!(config.agents.contains_key("terminal"));
        let terminal = &config.agents["terminal"];
        assert_eq!(terminal.command, "shell");
        assert!(terminal.prompt_flag.is_none());
        assert!(terminal.interactive);
    }

    #[test]
    fn given_config_should_round_trip_yaml() {
        let config = Config::default();
        let yaml = serde_yml::to_string(&config).unwrap();
        let parsed: Config = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.default_agent, "claude");
        assert!(parsed.agents.contains_key("claude"));
    }
}
