use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Streaming,
    Waiting,
    Broken,
}

impl AgentStatus {
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Streaming | Self::Waiting)
    }
}

impl Serialize for AgentStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = match self {
            AgentStatus::Streaming => "streaming",
            AgentStatus::Waiting => "waiting",
            AgentStatus::Broken => "broken",
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for AgentStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "streaming" => Ok(AgentStatus::Streaming),
            "waiting" => Ok(AgentStatus::Waiting),
            "broken" => Ok(AgentStatus::Broken),
            // Backward compat: map old status values
            "spawning" | "running" => Ok(AgentStatus::Streaming),
            "idle" | "suspended" => Ok(AgentStatus::Waiting),
            "completed" | "failed" | "killed" | "lost" => Ok(AgentStatus::Broken),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["streaming", "waiting", "broken"],
            )),
        }
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

#[derive(Debug, Clone, Serialize)]
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
    /// Whether this agent is currently suspended. Only meaningful when `status.is_alive()`.
    /// Invariant: custom Deserialize infers `true` when `suspended_at` is present (backward
    /// compat with old manifests). The engine sets both `suspended` and `suspended_at`
    /// atomically on suspend/resume.
    #[serde(default)]
    pub suspended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl<'de> Deserialize<'de> for AgentEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            id: String,
            name: String,
            agent_type: String,
            status: AgentStatus,
            prompt: Option<String>,
            started_at: DateTime<Utc>,
            completed_at: Option<DateTime<Utc>>,
            exit_code: Option<i32>,
            error: Option<String>,
            pid: Option<u32>,
            session_id: Option<String>,
            suspended_at: Option<DateTime<Utc>>,
            #[serde(default)]
            suspended: bool,
            #[serde(default)]
            command: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // Backward compat: old manifests have suspended_at set but no suspended field.
        // Ensure suspended is true when suspended_at is present.
        let suspended = raw.suspended || raw.suspended_at.is_some();
        Ok(AgentEntry {
            id: raw.id,
            name: raw.name,
            agent_type: raw.agent_type,
            status: raw.status,
            prompt: raw.prompt,
            started_at: raw.started_at,
            completed_at: raw.completed_at,
            exit_code: raw.exit_code,
            error: raw.error,
            pid: raw.pid,
            session_id: raw.session_id,
            suspended_at: raw.suspended_at,
            suspended,
            command: raw.command,
        })
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub prompt_flag: Option<String>,
    #[serde(default = "crate::serde_defaults::default_true")]
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "crate::serde_defaults::default_agent")]
    pub default_agent: String,
    #[serde(default = "default_agents")]
    pub agents: IndexMap<String, AgentConfig>,
    #[serde(default = "default_env_files")]
    pub env_files: Vec<String>,
}

fn default_env_files() -> Vec<String> {
    vec![".env".to_string(), ".env.local".to_string()]
}

pub fn default_agents() -> IndexMap<String, AgentConfig> {
    // (name, command) — command "shell" is a sentinel the engine resolves to $SHELL
    [
        ("claude", "claude"),
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("terminal", "shell"),
    ]
    .into_iter()
    .map(|(name, cmd)| {
        (
            name.to_string(),
            AgentConfig {
                name: name.to_string(),
                command: cmd.to_string(),
                prompt_flag: None,
                interactive: true,
            },
        )
    })
    .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_agent: crate::serde_defaults::default_agent(),
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
    fn given_agent_status_streaming_should_serialize() {
        let json = serde_json::to_string(&AgentStatus::Streaming).unwrap();
        assert_eq!(json, r#""streaming""#);
    }

    #[test]
    fn given_agent_status_waiting_should_serialize() {
        let json = serde_json::to_string(&AgentStatus::Waiting).unwrap();
        assert_eq!(json, r#""waiting""#);
    }

    #[test]
    fn given_agent_status_broken_should_serialize() {
        let json = serde_json::to_string(&AgentStatus::Broken).unwrap();
        assert_eq!(json, r#""broken""#);
    }

    #[test]
    fn given_all_agent_statuses_should_round_trip_json() {
        let statuses = vec![
            AgentStatus::Streaming,
            AgentStatus::Waiting,
            AgentStatus::Broken,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn given_old_status_values_should_deserialize_to_new() {
        // spawning, running → Streaming
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""spawning""#).unwrap(),
            AgentStatus::Streaming
        );
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""running""#).unwrap(),
            AgentStatus::Streaming
        );
        // idle, suspended → Waiting
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""idle""#).unwrap(),
            AgentStatus::Waiting
        );
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""suspended""#).unwrap(),
            AgentStatus::Waiting
        );
        // completed, failed, killed, lost → Broken
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""completed""#).unwrap(),
            AgentStatus::Broken
        );
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""failed""#).unwrap(),
            AgentStatus::Broken
        );
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""killed""#).unwrap(),
            AgentStatus::Broken
        );
        assert_eq!(
            serde_json::from_str::<AgentStatus>(r#""lost""#).unwrap(),
            AgentStatus::Broken
        );
    }

    #[test]
    fn given_broken_status_should_not_be_alive() {
        assert!(!AgentStatus::Broken.is_alive());
    }

    #[test]
    fn given_active_statuses_should_be_alive() {
        assert!(AgentStatus::Streaming.is_alive());
        assert!(AgentStatus::Waiting.is_alive());
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
            status: AgentStatus::Streaming,
            prompt: Some("fix bug".into()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(1234),
            session_id: None,
            suspended_at: None,
            suspended: false,
            command: None,
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
    fn given_agent_entry_with_suspended_at_but_no_suspended_field_should_infer_suspended() {
        let json = r#"{
            "id": "ag-old",
            "name": "claude",
            "agentType": "claude",
            "status": "waiting",
            "prompt": null,
            "startedAt": "2026-03-01T00:00:00Z",
            "suspendedAt": "2026-03-01T01:00:00Z"
        }"#;
        let entry: AgentEntry = serde_json::from_str(json).unwrap();
        assert!(
            entry.suspended,
            "suspended should be true when suspendedAt is present"
        );
        assert!(entry.suspended_at.is_some());
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
        assert_eq!(entry.status, AgentStatus::Waiting);
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
                status: AgentStatus::Streaming,
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
                status: AgentStatus::Streaming,
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
                status: AgentStatus::Waiting,
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
            status: AgentStatus::Streaming,
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
