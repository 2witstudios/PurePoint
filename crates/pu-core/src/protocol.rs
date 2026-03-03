use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{AgentStatus, WorktreeEntry};

pub const PROTOCOL_VERSION: u32 = 1;

/// Serde helper: encode Vec<u8> as hex in JSON for binary PTY data.
mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = data.iter().map(|b| format!("{b:02x}")).collect::<String>();
        encoded.serialize(serializer)
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
    GridCommand {
        project_root: String,
        command: GridCommand,
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
        let req = Request::Init { project_root: "/test".into() };
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
            Request::Spawn { agent, name, root, .. } => {
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
            Request::Spawn { agent, name, base, .. } => {
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
            Request::Kill { target: KillTarget::Agent(id), .. } => assert_eq!(id, "ag-abc"),
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
        let req = Request::Attach { agent_id: "ag-abc".into() };
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
            Response::HealthReport { pid, protocol_version, .. } => {
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
            status: crate::types::AgentStatus::Spawning,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::SpawnResult { worktree_id, agent_id, .. } => {
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
        let cmd = GridCommand::Split { leaf_id: Some(2), axis: "v".into() };
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
        let cmd = GridCommand::Focus { leaf_id: None, direction: Some("right".into()) };
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
        let cmd = GridCommand::SetAgent { leaf_id: 3, agent_id: "ag-abc".into() };
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
        let req = Request::SubscribeGrid { project_root: "/test".into() };
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
            command: GridCommand::Split { leaf_id: Some(1), axis: "h".into() },
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        match parsed {
            Request::GridCommand { project_root, command } => {
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
            command: GridCommand::Focus { leaf_id: Some(2), direction: None },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::GridEvent { project_root, command } => {
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
            Request::Suspend { project_root, target } => {
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
            Request::Suspend { target: SuspendTarget::Agent(id), .. } => {
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
            Request::Resume { project_root, agent_id } => {
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
            status: crate::types::AgentStatus::Running,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        match parsed {
            Response::ResumeResult { agent_id, status } => {
                assert_eq!(agent_id, "ag-abc");
                assert_eq!(status, crate::types::AgentStatus::Running);
            }
            _ => panic!("expected ResumeResult"),
        }
    }

    #[test]
    fn given_suspend_target_all_should_round_trip() {
        let target = SuspendTarget::All;
        let json = serde_json::to_string(&target).unwrap();
        let parsed: SuspendTarget = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SuspendTarget::All));
    }
}
