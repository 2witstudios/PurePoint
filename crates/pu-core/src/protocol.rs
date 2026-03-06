use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{AgentStatus, WorktreeEntry};

pub const PROTOCOL_VERSION: u32 = 1;

/// Serde helper: encode Vec<u8> as hex in JSON for binary PTY data.
mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
        hex.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        if s.len() % 2 != 0 {
            return Err(D::Error::custom("odd-length hex string"));
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16)
                    .map_err(|e| D::Error::custom(format!("invalid hex: {e}")))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Health,
    Init {
        project_root: String,
    },
    Spawn {
        project_root: String,
        prompt: String,
        #[serde(default = "default_agent_type")]
        agent: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        base: Option<String>,
        #[serde(default)]
        root: bool,
        #[serde(default)]
        worktree: Option<String>,
    },
    Status {
        project_root: String,
        #[serde(default)]
        agent_id: Option<String>,
    },
    Kill {
        project_root: String,
        target: KillTarget,
    },
    Suspend {
        project_root: String,
        target: SuspendTarget,
    },
    Resume {
        project_root: String,
        agent_id: String,
    },
    Logs {
        agent_id: String,
        #[serde(default = "default_tail")]
        tail: usize,
    },
    Attach {
        agent_id: String,
    },
    Input {
        agent_id: String,
        #[serde(with = "hex_bytes")]
        data: Vec<u8>,
    },
    Resize {
        agent_id: String,
        cols: u16,
        rows: u16,
    },
    SubscribeGrid {
        project_root: String,
    },
    SubscribeStatus {
        project_root: String,
    },
    GridCommand {
        project_root: String,
        command: GridCommand,
    },
    Rename {
        project_root: String,
        agent_id: String,
        name: String,
    },
    DeleteWorktree {
        project_root: String,
        worktree_id: String,
    },
    // Template/Prompt CRUD
    ListTemplates {
        project_root: String,
    },
    GetTemplate {
        project_root: String,
        name: String,
    },
    SaveTemplate {
        project_root: String,
        name: String,
        description: String,
        agent: String,
        body: String,
        scope: String,
    },
    DeleteTemplate {
        project_root: String,
        name: String,
        scope: String,
    },
    // Agent def CRUD
    ListAgentDefs {
        project_root: String,
    },
    GetAgentDef {
        project_root: String,
        name: String,
    },
    SaveAgentDef {
        project_root: String,
        name: String,
        agent_type: String,
        #[serde(default)]
        template: Option<String>,
        #[serde(default)]
        inline_prompt: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
        scope: String,
        #[serde(default = "default_true")]
        available_in_command_dialog: bool,
        #[serde(default)]
        icon: Option<String>,
    },
    DeleteAgentDef {
        project_root: String,
        name: String,
        scope: String,
    },
    // Swarm def CRUD
    ListSwarmDefs {
        project_root: String,
    },
    GetSwarmDef {
        project_root: String,
        name: String,
    },
    SaveSwarmDef {
        project_root: String,
        name: String,
        #[serde(default = "default_worktree_count")]
        worktree_count: u32,
        #[serde(default)]
        worktree_template: String,
        #[serde(default)]
        roster: Vec<SwarmRosterEntryPayload>,
        #[serde(default)]
        include_terminal: bool,
        scope: String,
    },
    DeleteSwarmDef {
        project_root: String,
        name: String,
        scope: String,
    },
    // Execution
    RunSwarm {
        project_root: String,
        swarm_name: String,
        #[serde(default)]
        vars: std::collections::HashMap<String, String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GridCommand {
    Split {
        #[serde(default)]
        leaf_id: Option<u32>,
        #[serde(default = "default_axis")]
        axis: String,
    },
    Close {
        #[serde(default)]
        leaf_id: Option<u32>,
    },
    Focus {
        #[serde(default)]
        leaf_id: Option<u32>,
        #[serde(default)]
        direction: Option<String>,
    },
    SetAgent {
        leaf_id: u32,
        agent_id: String,
    },
    GetLayout,
}

fn default_axis() -> String {
    "v".to_string()
}

fn default_agent_type() -> String {
    "claude".to_string()
}

fn default_tail() -> usize {
    500
}

fn default_true() -> bool {
    true
}

fn default_worktree_count() -> u32 {
    1
}

fn default_quantity() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmRosterEntryPayload {
    pub agent_def: String,
    pub role: String,
    #[serde(default = "default_quantity")]
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    pub agent: String,
    pub source: String,
    pub variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefInfo {
    pub name: String,
    pub agent_type: String,
    pub template: Option<String>,
    pub inline_prompt: Option<String>,
    pub tags: Vec<String>,
    pub scope: String,
    pub available_in_command_dialog: bool,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmDefInfo {
    pub name: String,
    pub worktree_count: u32,
    pub worktree_template: String,
    pub roster: Vec<SwarmRosterEntryPayload>,
    pub include_terminal: bool,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillTarget {
    Agent(String),
    Worktree(String),
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspendTarget {
    Agent(String),
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    HealthReport {
        pid: u32,
        uptime_seconds: u64,
        protocol_version: u32,
        projects: Vec<String>,
        agent_count: usize,
    },
    InitResult {
        created: bool,
    },
    SpawnResult {
        worktree_id: Option<String>,
        agent_id: String,
        status: AgentStatus,
    },
    StatusReport {
        worktrees: Vec<WorktreeEntry>,
        agents: Vec<AgentStatusReport>,
    },
    AgentStatus(AgentStatusReport),
    KillResult {
        killed: Vec<String>,
        exit_codes: std::collections::HashMap<String, Option<i32>>,
    },
    SuspendResult {
        suspended: Vec<String>,
    },
    ResumeResult {
        agent_id: String,
        status: AgentStatus,
    },
    LogsResult {
        agent_id: String,
        data: String,
    },
    AttachReady {
        buffered_bytes: usize,
    },
    Output {
        agent_id: String,
        #[serde(with = "hex_bytes")]
        data: Vec<u8>,
    },
    GridSubscribed,
    GridLayout {
        layout: serde_json::Value,
    },
    GridEvent {
        project_root: String,
        command: GridCommand,
    },
    StatusSubscribed,
    StatusEvent {
        worktrees: Vec<WorktreeEntry>,
        agents: Vec<AgentStatusReport>,
    },
    RenameResult {
        agent_id: String,
        name: String,
    },
    DeleteWorktreeResult {
        worktree_id: String,
        killed_agents: Vec<String>,
        branch_deleted: bool,
        remote_deleted: bool,
    },
    TemplateList {
        templates: Vec<TemplateInfo>,
    },
    TemplateDetail {
        name: String,
        description: String,
        agent: String,
        body: String,
        source: String,
        variables: Vec<String>,
    },
    AgentDefList {
        agent_defs: Vec<AgentDefInfo>,
    },
    AgentDefDetail {
        name: String,
        agent_type: String,
        template: Option<String>,
        inline_prompt: Option<String>,
        tags: Vec<String>,
        scope: String,
        available_in_command_dialog: bool,
        icon: Option<String>,
    },
    SwarmDefList {
        swarm_defs: Vec<SwarmDefInfo>,
    },
    SwarmDefDetail {
        name: String,
        worktree_count: u32,
        worktree_template: String,
        roster: Vec<SwarmRosterEntryPayload>,
        include_terminal: bool,
        scope: String,
    },
    RunSwarmResult {
        spawned_agents: Vec<String>,
    },
    RunSwarmPartial {
        spawned_agents: Vec<String>,
        error_code: String,
        error_message: String,
    },
    Ok,
    ShuttingDown,
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentStatusReport {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub status: AgentStatus,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub idle_seconds: Option<u64>,
    pub worktree_id: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default)]
    pub suspended: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Request round-trips ---

    #[test]
    fn given_health_request_should_round_trip() {
        let req = Request::Health;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Health));
    }

    #[test]
    fn given_init_request_should_round_trip() {
        let req = Request::Init {
            project_root: "/test".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Init { project_root } => assert_eq!(project_root, "/test"),
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn given_spawn_request_should_default_agent_to_claude() {
        let json = r#"{"type":"spawn","project_root":"/test","prompt":"fix bug"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::Spawn {
                agent, name, root, ..
            } => {
                assert_eq!(agent, "claude");
                assert!(name.is_none());
                assert!(!root);
            }
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn given_spawn_request_with_all_fields_should_round_trip() {
        let req = Request::Spawn {
            project_root: "/test".into(),
            prompt: "fix auth".into(),
            agent: "codex".into(),
            name: Some("fix-auth".into()),
            base: Some("develop".into()),
            root: false,
            worktree: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Spawn {
                agent, name, base, ..
            } => {
                assert_eq!(agent, "codex");
                assert_eq!(name.unwrap(), "fix-auth");
                assert_eq!(base.unwrap(), "develop");
            }
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn given_status_request_with_agent_id_should_round_trip() {
        let req = Request::Status {
            project_root: "/test".into(),
            agent_id: Some("ag-abc".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Status { agent_id, .. } => assert_eq!(agent_id.unwrap(), "ag-abc"),
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn given_kill_request_with_agent_target_should_round_trip() {
        let req = Request::Kill {
            project_root: "/test".into(),
            target: KillTarget::Agent("ag-abc".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Kill {
                target: KillTarget::Agent(id),
                ..
            } => assert_eq!(id, "ag-abc"),
            _ => panic!("expected Kill with Agent target"),
        }
    }

    #[test]
    fn given_kill_target_all_should_round_trip() {
        let target = KillTarget::All;
        let json = serde_json::to_string(&target).unwrap();
        let parsed: KillTarget = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, KillTarget::All));
    }

    #[test]
    fn given_logs_request_should_default_tail_to_500() {
        let json = r#"{"type":"logs","agent_id":"ag-abc"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::Logs { tail, .. } => assert_eq!(tail, 500),
            _ => panic!("expected Logs"),
        }
    }

    #[test]
    fn given_attach_request_should_round_trip() {
        let req = Request::Attach {
            agent_id: "ag-abc".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Attach { agent_id } => assert_eq!(agent_id, "ag-abc"),
            _ => panic!("expected Attach"),
        }
    }

    #[test]
    fn given_shutdown_request_should_round_trip() {
        let req = Request::Shutdown;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Shutdown));
    }

    // --- Response round-trips ---

    #[test]
    fn given_health_report_should_round_trip() {
        let resp = Response::HealthReport {
            pid: 1234,
            uptime_seconds: 3600,
            protocol_version: PROTOCOL_VERSION,
            projects: vec!["/test".into()],
            agent_count: 5,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::HealthReport {
                pid,
                protocol_version,
                ..
            } => {
                assert_eq!(pid, 1234);
                assert_eq!(protocol_version, PROTOCOL_VERSION);
            }
            _ => panic!("expected HealthReport"),
        }
    }

    #[test]
    fn given_error_response_should_round_trip() {
        let resp = Response::Error {
            code: "NOT_INITIALIZED".into(),
            message: "run pu init".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::Error { code, message } => {
                assert_eq!(code, "NOT_INITIALIZED");
                assert_eq!(message, "run pu init");
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn given_spawn_result_should_round_trip() {
        let resp = Response::SpawnResult {
            worktree_id: Some("wt-abc".into()),
            agent_id: "ag-xyz".into(),
            status: crate::types::AgentStatus::Streaming,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::SpawnResult {
                worktree_id,
                agent_id,
                ..
            } => {
                assert_eq!(worktree_id.unwrap(), "wt-abc");
                assert_eq!(agent_id, "ag-xyz");
            }
            _ => panic!("expected SpawnResult"),
        }
    }

    #[test]
    fn given_kill_result_should_round_trip() {
        let mut exit_codes = std::collections::HashMap::new();
        exit_codes.insert("ag-abc".to_string(), Some(0i32));
        let resp = Response::KillResult {
            killed: vec!["ag-abc".into()],
            exit_codes,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::KillResult { killed, exit_codes } => {
                assert_eq!(killed, vec!["ag-abc"]);
                assert_eq!(exit_codes["ag-abc"], Some(0));
            }
            _ => panic!("expected KillResult"),
        }
    }

    #[test]
    fn given_protocol_version_should_be_1() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    // --- GridCommand round-trips ---

    #[test]
    fn given_grid_split_command_should_round_trip() {
        let cmd = GridCommand::Split {
            leaf_id: Some(2),
            axis: "v".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: GridCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            GridCommand::Split { leaf_id, axis } => {
                assert_eq!(leaf_id, Some(2));
                assert_eq!(axis, "v");
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn given_grid_close_command_should_round_trip() {
        let cmd = GridCommand::Close { leaf_id: None };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: GridCommand = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, GridCommand::Close { leaf_id: None }));
    }

    #[test]
    fn given_grid_focus_command_should_round_trip() {
        let cmd = GridCommand::Focus {
            leaf_id: None,
            direction: Some("right".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: GridCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            GridCommand::Focus { leaf_id, direction } => {
                assert!(leaf_id.is_none());
                assert_eq!(direction.unwrap(), "right");
            }
            _ => panic!("expected Focus"),
        }
    }

    #[test]
    fn given_grid_set_agent_command_should_round_trip() {
        let cmd = GridCommand::SetAgent {
            leaf_id: 3,
            agent_id: "ag-abc".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: GridCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            GridCommand::SetAgent { leaf_id, agent_id } => {
                assert_eq!(leaf_id, 3);
                assert_eq!(agent_id, "ag-abc");
            }
            _ => panic!("expected SetAgent"),
        }
    }

    #[test]
    fn given_grid_get_layout_command_should_round_trip() {
        let cmd = GridCommand::GetLayout;
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: GridCommand = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, GridCommand::GetLayout));
    }

    #[test]
    fn given_subscribe_grid_request_should_round_trip() {
        let req = Request::SubscribeGrid {
            project_root: "/test".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::SubscribeGrid { project_root } => assert_eq!(project_root, "/test"),
            _ => panic!("expected SubscribeGrid"),
        }
    }

    #[test]
    fn given_grid_command_request_should_round_trip() {
        let req = Request::GridCommand {
            project_root: "/test".into(),
            command: GridCommand::Split {
                leaf_id: Some(1),
                axis: "h".into(),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::GridCommand {
                project_root,
                command,
            } => {
                assert_eq!(project_root, "/test");
                assert!(matches!(command, GridCommand::Split { .. }));
            }
            _ => panic!("expected GridCommand"),
        }
    }

    #[test]
    fn given_grid_event_response_should_round_trip() {
        let resp = Response::GridEvent {
            project_root: "/test".into(),
            command: GridCommand::Focus {
                leaf_id: Some(2),
                direction: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::GridEvent {
                project_root,
                command,
            } => {
                assert_eq!(project_root, "/test");
                assert!(matches!(command, GridCommand::Focus { .. }));
            }
            _ => panic!("expected GridEvent"),
        }
    }

    // --- Suspend/Resume round-trips ---

    #[test]
    fn given_suspend_request_with_all_target_should_round_trip() {
        let req = Request::Suspend {
            project_root: "/test".into(),
            target: SuspendTarget::All,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Suspend {
                project_root,
                target,
            } => {
                assert_eq!(project_root, "/test");
                assert!(matches!(target, SuspendTarget::All));
            }
            _ => panic!("expected Suspend"),
        }
    }

    #[test]
    fn given_suspend_request_with_agent_target_should_round_trip() {
        let req = Request::Suspend {
            project_root: "/test".into(),
            target: SuspendTarget::Agent("ag-abc".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Suspend {
                target: SuspendTarget::Agent(id),
                ..
            } => {
                assert_eq!(id, "ag-abc");
            }
            _ => panic!("expected Suspend with Agent target"),
        }
    }

    #[test]
    fn given_resume_request_should_round_trip() {
        let req = Request::Resume {
            project_root: "/test".into(),
            agent_id: "ag-abc".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Resume {
                project_root,
                agent_id,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(agent_id, "ag-abc");
            }
            _ => panic!("expected Resume"),
        }
    }

    #[test]
    fn given_suspend_result_should_round_trip() {
        let resp = Response::SuspendResult {
            suspended: vec!["ag-abc".into(), "ag-def".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::SuspendResult { suspended } => {
                assert_eq!(suspended, vec!["ag-abc", "ag-def"]);
            }
            _ => panic!("expected SuspendResult"),
        }
    }

    #[test]
    fn given_resume_result_should_round_trip() {
        let resp = Response::ResumeResult {
            agent_id: "ag-abc".into(),
            status: crate::types::AgentStatus::Streaming,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::ResumeResult { agent_id, status } => {
                assert_eq!(agent_id, "ag-abc");
                assert_eq!(status, crate::types::AgentStatus::Streaming);
            }
            _ => panic!("expected ResumeResult"),
        }
    }

    // --- Rename round-trips ---

    #[test]
    fn given_rename_request_should_round_trip() {
        let req = Request::Rename {
            project_root: "/test".into(),
            agent_id: "ag-abc".into(),
            name: "new-name".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::Rename {
                project_root,
                agent_id,
                name,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(agent_id, "ag-abc");
                assert_eq!(name, "new-name");
            }
            _ => panic!("expected Rename"),
        }
    }

    #[test]
    fn given_rename_result_should_round_trip() {
        let resp = Response::RenameResult {
            agent_id: "ag-abc".into(),
            name: "new-name".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::RenameResult { agent_id, name } => {
                assert_eq!(agent_id, "ag-abc");
                assert_eq!(name, "new-name");
            }
            _ => panic!("expected RenameResult"),
        }
    }

    // --- DeleteWorktree round-trips ---

    #[test]
    fn given_delete_worktree_request_should_round_trip() {
        let req = Request::DeleteWorktree {
            project_root: "/test".into(),
            worktree_id: "wt-abc".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::DeleteWorktree {
                project_root,
                worktree_id,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(worktree_id, "wt-abc");
            }
            _ => panic!("expected DeleteWorktree"),
        }
    }

    #[test]
    fn given_delete_worktree_result_should_round_trip() {
        let resp = Response::DeleteWorktreeResult {
            worktree_id: "wt-abc".into(),
            killed_agents: vec!["ag-1".into(), "ag-2".into()],
            branch_deleted: true,
            remote_deleted: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::DeleteWorktreeResult {
                worktree_id,
                killed_agents,
                branch_deleted,
                remote_deleted,
            } => {
                assert_eq!(worktree_id, "wt-abc");
                assert_eq!(killed_agents, vec!["ag-1", "ag-2"]);
                assert!(branch_deleted);
                assert!(!remote_deleted);
            }
            _ => panic!("expected DeleteWorktreeResult"),
        }
    }

    #[test]
    fn given_suspend_target_all_should_round_trip() {
        let target = SuspendTarget::All;
        let json = serde_json::to_string(&target).unwrap();
        let parsed: SuspendTarget = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SuspendTarget::All));
    }

    // --- SwarmRosterEntryPayload ---

    #[test]
    fn given_swarm_roster_entry_payload_should_round_trip() {
        // given
        let entry = SwarmRosterEntryPayload {
            agent_def: "reviewer".into(),
            role: "review".into(),
            quantity: 2,
        };

        // when
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: SwarmRosterEntryPayload = serde_json::from_str(&json).unwrap();

        // then
        assert_eq!(parsed.agent_def, "reviewer");
        assert_eq!(parsed.role, "review");
        assert_eq!(parsed.quantity, 2);
    }

    #[test]
    fn given_swarm_roster_entry_payload_should_default_quantity_to_1() {
        // given
        let json = r#"{"agent_def":"builder","role":"build"}"#;

        // when
        let parsed: SwarmRosterEntryPayload = serde_json::from_str(json).unwrap();

        // then
        assert_eq!(parsed.quantity, 1);
    }

    // --- TemplateInfo ---

    #[test]
    fn given_template_info_should_round_trip() {
        // given
        let info = TemplateInfo {
            name: "review".into(),
            description: "Code review".into(),
            agent: "claude".into(),
            source: "local".into(),
            variables: vec!["BRANCH".into()],
        };

        // when
        let json = serde_json::to_string(&info).unwrap();
        let parsed: TemplateInfo = serde_json::from_str(&json).unwrap();

        // then
        assert_eq!(parsed.name, "review");
        assert_eq!(parsed.description, "Code review");
        assert_eq!(parsed.agent, "claude");
        assert_eq!(parsed.source, "local");
        assert_eq!(parsed.variables, vec!["BRANCH"]);
    }

    // --- AgentDefInfo ---

    #[test]
    fn given_agent_def_info_should_round_trip() {
        // given
        let info = AgentDefInfo {
            name: "reviewer".into(),
            agent_type: "claude".into(),
            template: Some("review-template".into()),
            inline_prompt: None,
            tags: vec!["review".into()],
            scope: "local".into(),
            available_in_command_dialog: true,
            icon: Some("magnifyingglass".into()),
        };

        // when
        let json = serde_json::to_string(&info).unwrap();
        let parsed: AgentDefInfo = serde_json::from_str(&json).unwrap();

        // then
        assert_eq!(parsed.name, "reviewer");
        assert_eq!(parsed.agent_type, "claude");
        assert_eq!(parsed.template, Some("review-template".into()));
        assert_eq!(parsed.tags, vec!["review"]);
        assert_eq!(parsed.scope, "local");
        assert!(parsed.available_in_command_dialog);
        assert_eq!(parsed.icon, Some("magnifyingglass".into()));
    }

    // --- SwarmDefInfo ---

    #[test]
    fn given_swarm_def_info_should_round_trip() {
        // given
        let info = SwarmDefInfo {
            name: "full-stack".into(),
            worktree_count: 3,
            worktree_template: "feature".into(),
            roster: vec![SwarmRosterEntryPayload {
                agent_def: "reviewer".into(),
                role: "review".into(),
                quantity: 2,
            }],
            include_terminal: true,
            scope: "local".into(),
        };

        // when
        let json = serde_json::to_string(&info).unwrap();
        let parsed: SwarmDefInfo = serde_json::from_str(&json).unwrap();

        // then
        assert_eq!(parsed.name, "full-stack");
        assert_eq!(parsed.worktree_count, 3);
        assert_eq!(parsed.worktree_template, "feature");
        assert_eq!(parsed.roster.len(), 1);
        assert_eq!(parsed.roster[0].agent_def, "reviewer");
        assert!(parsed.include_terminal);
        assert_eq!(parsed.scope, "local");
    }

    // --- New Request round-trips ---

    #[test]
    fn given_list_templates_request_should_round_trip() {
        // given
        let req = Request::ListTemplates {
            project_root: "/test".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::ListTemplates { project_root } => assert_eq!(project_root, "/test"),
            _ => panic!("expected ListTemplates"),
        }
    }

    #[test]
    fn given_get_template_request_should_round_trip() {
        // given
        let req = Request::GetTemplate {
            project_root: "/test".into(),
            name: "review".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::GetTemplate { project_root, name } => {
                assert_eq!(project_root, "/test");
                assert_eq!(name, "review");
            }
            _ => panic!("expected GetTemplate"),
        }
    }

    #[test]
    fn given_save_template_request_should_round_trip() {
        // given
        let req = Request::SaveTemplate {
            project_root: "/test".into(),
            name: "review".into(),
            description: "Code review".into(),
            agent: "claude".into(),
            body: "Review {{BRANCH}}.".into(),
            scope: "local".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::SaveTemplate {
                project_root,
                name,
                description,
                agent,
                body,
                scope,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(name, "review");
                assert_eq!(description, "Code review");
                assert_eq!(agent, "claude");
                assert_eq!(body, "Review {{BRANCH}}.");
                assert_eq!(scope, "local");
            }
            _ => panic!("expected SaveTemplate"),
        }
    }

    #[test]
    fn given_delete_template_request_should_round_trip() {
        // given
        let req = Request::DeleteTemplate {
            project_root: "/test".into(),
            name: "review".into(),
            scope: "local".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::DeleteTemplate {
                project_root,
                name,
                scope,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(name, "review");
                assert_eq!(scope, "local");
            }
            _ => panic!("expected DeleteTemplate"),
        }
    }

    #[test]
    fn given_list_agent_defs_request_should_round_trip() {
        // given
        let req = Request::ListAgentDefs {
            project_root: "/test".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::ListAgentDefs { project_root } => assert_eq!(project_root, "/test"),
            _ => panic!("expected ListAgentDefs"),
        }
    }

    #[test]
    fn given_save_agent_def_request_should_round_trip() {
        // given
        let req = Request::SaveAgentDef {
            project_root: "/test".into(),
            name: "reviewer".into(),
            agent_type: "claude".into(),
            template: Some("review-tpl".into()),
            inline_prompt: None,
            tags: vec!["review".into()],
            scope: "local".into(),
            available_in_command_dialog: true,
            icon: Some("magnifyingglass".into()),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::SaveAgentDef {
                name,
                agent_type,
                template,
                tags,
                scope,
                available_in_command_dialog,
                icon,
                ..
            } => {
                assert_eq!(name, "reviewer");
                assert_eq!(agent_type, "claude");
                assert_eq!(template, Some("review-tpl".into()));
                assert_eq!(tags, vec!["review"]);
                assert_eq!(scope, "local");
                assert!(available_in_command_dialog);
                assert_eq!(icon, Some("magnifyingglass".into()));
            }
            _ => panic!("expected SaveAgentDef"),
        }
    }

    #[test]
    fn given_save_agent_def_request_should_default_available_in_command_dialog_to_true() {
        // given
        let json = r#"{"type":"save_agent_def","project_root":"/test","name":"x","agent_type":"claude","scope":"local"}"#;

        // when
        let parsed: Request = serde_json::from_str(json).unwrap();

        // then
        match parsed {
            Request::SaveAgentDef {
                available_in_command_dialog,
                ..
            } => {
                assert!(available_in_command_dialog);
            }
            _ => panic!("expected SaveAgentDef"),
        }
    }

    #[test]
    fn given_delete_agent_def_request_should_round_trip() {
        // given
        let req = Request::DeleteAgentDef {
            project_root: "/test".into(),
            name: "reviewer".into(),
            scope: "local".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::DeleteAgentDef {
                project_root,
                name,
                scope,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(name, "reviewer");
                assert_eq!(scope, "local");
            }
            _ => panic!("expected DeleteAgentDef"),
        }
    }

    #[test]
    fn given_list_swarm_defs_request_should_round_trip() {
        // given
        let req = Request::ListSwarmDefs {
            project_root: "/test".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::ListSwarmDefs { project_root } => assert_eq!(project_root, "/test"),
            _ => panic!("expected ListSwarmDefs"),
        }
    }

    #[test]
    fn given_save_swarm_def_request_should_round_trip() {
        // given
        let req = Request::SaveSwarmDef {
            project_root: "/test".into(),
            name: "full-stack".into(),
            worktree_count: 3,
            worktree_template: "feature".into(),
            roster: vec![SwarmRosterEntryPayload {
                agent_def: "reviewer".into(),
                role: "review".into(),
                quantity: 2,
            }],
            include_terminal: true,
            scope: "local".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::SaveSwarmDef {
                name,
                worktree_count,
                roster,
                include_terminal,
                scope,
                ..
            } => {
                assert_eq!(name, "full-stack");
                assert_eq!(worktree_count, 3);
                assert_eq!(roster.len(), 1);
                assert_eq!(roster[0].agent_def, "reviewer");
                assert!(include_terminal);
                assert_eq!(scope, "local");
            }
            _ => panic!("expected SaveSwarmDef"),
        }
    }

    #[test]
    fn given_save_swarm_def_request_should_default_worktree_count_to_1() {
        // given
        let json = r#"{"type":"save_swarm_def","project_root":"/test","name":"x","scope":"local"}"#;

        // when
        let parsed: Request = serde_json::from_str(json).unwrap();

        // then
        match parsed {
            Request::SaveSwarmDef {
                worktree_count,
                worktree_template,
                roster,
                include_terminal,
                ..
            } => {
                assert_eq!(worktree_count, 1);
                assert_eq!(worktree_template, "");
                assert!(roster.is_empty());
                assert!(!include_terminal);
            }
            _ => panic!("expected SaveSwarmDef"),
        }
    }

    #[test]
    fn given_delete_swarm_def_request_should_round_trip() {
        // given
        let req = Request::DeleteSwarmDef {
            project_root: "/test".into(),
            name: "full-stack".into(),
            scope: "local".into(),
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::DeleteSwarmDef {
                project_root,
                name,
                scope,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(name, "full-stack");
                assert_eq!(scope, "local");
            }
            _ => panic!("expected DeleteSwarmDef"),
        }
    }

    #[test]
    fn given_run_swarm_request_should_round_trip() {
        // given
        let mut vars = std::collections::HashMap::new();
        vars.insert("ENV".into(), "staging".into());
        let req = Request::RunSwarm {
            project_root: "/test".into(),
            swarm_name: "full-stack".into(),
            vars,
        };

        // when
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Request::RunSwarm {
                project_root,
                swarm_name,
                vars,
            } => {
                assert_eq!(project_root, "/test");
                assert_eq!(swarm_name, "full-stack");
                assert_eq!(vars["ENV"], "staging");
            }
            _ => panic!("expected RunSwarm"),
        }
    }

    #[test]
    fn given_run_swarm_request_should_default_vars_to_empty() {
        // given
        let json = r#"{"type":"run_swarm","project_root":"/test","swarm_name":"x"}"#;

        // when
        let parsed: Request = serde_json::from_str(json).unwrap();

        // then
        match parsed {
            Request::RunSwarm { vars, .. } => {
                assert!(vars.is_empty());
            }
            _ => panic!("expected RunSwarm"),
        }
    }

    // --- New Response round-trips ---

    #[test]
    fn given_template_list_response_should_round_trip() {
        // given
        let resp = Response::TemplateList {
            templates: vec![TemplateInfo {
                name: "review".into(),
                description: "Code review".into(),
                agent: "claude".into(),
                source: "local".into(),
                variables: vec!["BRANCH".into()],
            }],
        };

        // when
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Response::TemplateList { templates } => {
                assert_eq!(templates.len(), 1);
                assert_eq!(templates[0].name, "review");
                assert_eq!(templates[0].variables, vec!["BRANCH"]);
            }
            _ => panic!("expected TemplateList"),
        }
    }

    #[test]
    fn given_template_detail_response_should_round_trip() {
        // given
        let resp = Response::TemplateDetail {
            name: "review".into(),
            description: "Code review".into(),
            agent: "claude".into(),
            body: "Review {{BRANCH}}.".into(),
            source: "local".into(),
            variables: vec!["BRANCH".into()],
        };

        // when
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Response::TemplateDetail {
                name,
                description,
                agent,
                body,
                source,
                variables,
            } => {
                assert_eq!(name, "review");
                assert_eq!(description, "Code review");
                assert_eq!(agent, "claude");
                assert_eq!(body, "Review {{BRANCH}}.");
                assert_eq!(source, "local");
                assert_eq!(variables, vec!["BRANCH"]);
            }
            _ => panic!("expected TemplateDetail"),
        }
    }

    #[test]
    fn given_agent_def_list_response_should_round_trip() {
        // given
        let resp = Response::AgentDefList {
            agent_defs: vec![AgentDefInfo {
                name: "reviewer".into(),
                agent_type: "claude".into(),
                template: None,
                inline_prompt: None,
                tags: vec![],
                scope: "local".into(),
                available_in_command_dialog: true,
                icon: None,
            }],
        };

        // when
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Response::AgentDefList { agent_defs } => {
                assert_eq!(agent_defs.len(), 1);
                assert_eq!(agent_defs[0].name, "reviewer");
            }
            _ => panic!("expected AgentDefList"),
        }
    }

    #[test]
    fn given_swarm_def_list_response_should_round_trip() {
        // given
        let resp = Response::SwarmDefList {
            swarm_defs: vec![SwarmDefInfo {
                name: "full-stack".into(),
                worktree_count: 3,
                worktree_template: "feature".into(),
                roster: vec![],
                include_terminal: false,
                scope: "local".into(),
            }],
        };

        // when
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Response::SwarmDefList { swarm_defs } => {
                assert_eq!(swarm_defs.len(), 1);
                assert_eq!(swarm_defs[0].name, "full-stack");
                assert_eq!(swarm_defs[0].worktree_count, 3);
            }
            _ => panic!("expected SwarmDefList"),
        }
    }

    #[test]
    fn given_run_swarm_result_response_should_round_trip() {
        // given
        let resp = Response::RunSwarmResult {
            spawned_agents: vec!["ag-abc".into(), "ag-def".into()],
        };

        // when
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Response::RunSwarmResult { spawned_agents } => {
                assert_eq!(spawned_agents, vec!["ag-abc", "ag-def"]);
            }
            _ => panic!("expected RunSwarmResult"),
        }
    }

    #[test]
    fn given_run_swarm_partial_response_should_round_trip() {
        // given
        let resp = Response::RunSwarmPartial {
            spawned_agents: vec!["ag-abc".into()],
            error_code: "SPAWN_FAILED".into(),
            error_message: "could not spawn agent ag-def".into(),
        };

        // when
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        // then
        match parsed {
            Response::RunSwarmPartial {
                spawned_agents,
                error_code,
                error_message,
            } => {
                assert_eq!(spawned_agents, vec!["ag-abc"]);
                assert_eq!(error_code, "SPAWN_FAILED");
                assert_eq!(error_message, "could not spawn agent ag-def");
            }
            _ => panic!("expected RunSwarmPartial"),
        }
    }
}
