use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::AgentStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmRosterEntryPayload {
    pub agent_def: String,
    pub role: String,
    #[serde(default = "crate::serde_defaults::default_quantity")]
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    pub agent: String,
    pub source: String,
    pub variables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
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
pub struct ScheduleInfo {
    pub name: String,
    pub enabled: bool,
    pub recurrence: String,
    pub start_at: DateTime<Utc>,
    pub next_run: Option<DateTime<Utc>>,
    pub trigger: ScheduleTriggerPayload,
    pub project_root: String,
    pub target: String,
    pub scope: String,
    #[serde(default = "crate::serde_defaults::default_true")]
    pub root: bool,
    #[serde(default)]
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleTriggerPayload {
    AgentDef {
        name: String,
    },
    SwarmDef {
        name: String,
        #[serde(default)]
        vars: std::collections::HashMap<String, String>,
    },
    InlinePrompt {
        prompt: String,
        #[serde(default = "crate::serde_defaults::default_agent_type")]
        agent: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriggerActionPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GatePayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GatePayload {
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_exit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub name: String,
    pub description: Option<String>,
    pub on: String,
    pub sequence: Vec<TriggerActionPayload>,
    pub variables: std::collections::HashMap<String, String>,
    pub scope: String,
}

impl From<crate::trigger_def::TriggerDef> for TriggerInfo {
    fn from(d: crate::trigger_def::TriggerDef) -> Self {
        let on = match &d.on {
            crate::trigger_def::TriggerEvent::AgentIdle => "agent_idle",
            crate::trigger_def::TriggerEvent::PreCommit => "pre_commit",
            crate::trigger_def::TriggerEvent::PrePush => "pre_push",
        };
        TriggerInfo {
            name: d.name,
            description: d.description,
            on: on.to_string(),
            sequence: d
                .sequence
                .into_iter()
                .map(TriggerActionPayload::from)
                .collect(),
            variables: d.variables,
            scope: d.scope,
        }
    }
}

impl From<crate::trigger_def::TriggerAction> for TriggerActionPayload {
    fn from(a: crate::trigger_def::TriggerAction) -> Self {
        TriggerActionPayload {
            inject: a.inject,
            gate: a.gate.map(GatePayload::from),
            max_retries: a.max_retries,
        }
    }
}

impl From<crate::trigger_def::GateDef> for GatePayload {
    fn from(g: crate::trigger_def::GateDef) -> Self {
        GatePayload {
            run: g.run,
            expect_exit: g.expect_exit,
        }
    }
}

impl From<TriggerActionPayload> for crate::trigger_def::TriggerAction {
    fn from(a: TriggerActionPayload) -> Self {
        crate::trigger_def::TriggerAction {
            inject: a.inject,
            gate: a.gate.map(crate::trigger_def::GateDef::from),
            max_retries: a.max_retries,
        }
    }
}

impl From<GatePayload> for crate::trigger_def::GateDef {
    fn from(g: GatePayload) -> Self {
        crate::trigger_def::GateDef {
            run: g.run,
            expect_exit: g.expect_exit,
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_seq_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_state: Option<crate::types::TriggerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_total: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorktreeDiffEntry {
    pub worktree_id: String,
    pub worktree_name: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub diff_output: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigInfo {
    pub name: String,
    pub command: String,
    pub launch_args: Option<Vec<String>>,
    pub resolved_launch_args: Vec<String>,
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentPulseEntry {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub status: AgentStatus,
    pub exit_code: Option<i32>,
    pub runtime_seconds: i64,
    pub idle_seconds: Option<u64>,
    pub prompt_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorktreePulseEntry {
    pub worktree_id: String,
    pub worktree_name: String,
    pub branch: String,
    pub elapsed_seconds: i64,
    pub agents: Vec<AgentPulseEntry>,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_error: Option<String>,
}
