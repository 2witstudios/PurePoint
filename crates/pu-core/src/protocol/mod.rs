mod encoding;
mod grid;
mod payloads;
mod targets;

#[cfg(test)]
mod tests;

pub use grid::*;
pub use payloads::*;
pub use targets::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{AgentStatus, WorktreeEntry};

pub const PROTOCOL_VERSION: u32 = 5;

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
        #[serde(default = "crate::serde_defaults::default_agent_type")]
        agent: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        base: Option<String>,
        #[serde(default)]
        root: bool,
        #[serde(default)]
        worktree: Option<String>,
        #[serde(default)]
        command: Option<String>,
        /// Skip auto-mode launch args (--dangerously-skip-permissions, --full-auto, etc.)
        #[serde(default)]
        no_auto: bool,
        /// Additional CLI args appended after launch args (from --agent-args)
        #[serde(default)]
        extra_args: Vec<String>,
        #[serde(default)]
        plan_mode: bool,
        #[serde(default)]
        no_trigger: bool,
        #[serde(default)]
        trigger: Option<String>,
    },
    Status {
        project_root: String,
        #[serde(default)]
        agent_id: Option<String>,
    },
    Kill {
        project_root: String,
        target: KillTarget,
        #[serde(default)]
        exclude: Vec<String>,
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
    SpawnShell {
        cwd: String,
    },
    Attach {
        agent_id: String,
    },
    Input {
        agent_id: String,
        #[serde(with = "encoding")]
        data: Vec<u8>,
        /// When true, the engine sends data as chunked typed input then submits
        /// with Enter (\r). This avoids a race where a single atomic write of
        /// text+Enter causes the TUI to swallow the Enter keypress.
        #[serde(default)]
        submit: bool,
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
    AssignTrigger {
        project_root: String,
        agent_id: String,
        trigger_name: String,
    },
    CreateWorktree {
        project_root: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        base: Option<String>,
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
        #[serde(default)]
        command: Option<String>,
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
        #[serde(default = "crate::serde_defaults::default_true")]
        available_in_command_dialog: bool,
        #[serde(default)]
        icon: Option<String>,
        #[serde(default)]
        command: Option<String>,
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
        #[serde(default = "crate::serde_defaults::default_worktree_count")]
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
    // Schedule CRUD
    ListSchedules {
        project_root: String,
    },
    GetSchedule {
        project_root: String,
        name: String,
    },
    SaveSchedule {
        project_root: String,
        name: String,
        #[serde(default = "default_enabled")]
        enabled: bool,
        recurrence: String,
        start_at: DateTime<Utc>,
        trigger: ScheduleTriggerPayload,
        #[serde(default)]
        target: String,
        scope: String,
        #[serde(default = "crate::serde_defaults::default_true")]
        root: bool,
        #[serde(default)]
        agent_name: Option<String>,
    },
    DeleteSchedule {
        project_root: String,
        name: String,
        scope: String,
    },
    EnableSchedule {
        project_root: String,
        name: String,
    },
    DisableSchedule {
        project_root: String,
        name: String,
    },
    // Config
    GetConfig {
        project_root: String,
    },
    UpdateAgentConfig {
        project_root: String,
        agent_name: String,
        launch_args: Option<Vec<String>>,
    },
    // Trigger CRUD
    ListTriggers {
        project_root: String,
    },
    GetTrigger {
        project_root: String,
        name: String,
    },
    SaveTrigger {
        project_root: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        on: String,
        sequence: Vec<TriggerActionPayload>,
        #[serde(default)]
        variables: std::collections::HashMap<String, String>,
        scope: String,
    },
    DeleteTrigger {
        project_root: String,
        name: String,
        scope: String,
    },
    EvaluateGate {
        event: String,
        project_root: String,
        worktree_path: String,
    },
    Shutdown,
    Diff {
        project_root: String,
        #[serde(default)]
        worktree_id: Option<String>,
        #[serde(default)]
        stat: bool,
    },
    Pulse {
        project_root: String,
    },
}

fn default_tail() -> usize {
    500
}

fn default_enabled() -> bool {
    true
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
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
        #[serde(with = "encoding")]
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
    AssignTriggerResult {
        agent_id: String,
        trigger_name: String,
        sequence_len: u32,
    },
    CreateWorktreeResult {
        worktree_id: String,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
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
    ScheduleList {
        schedules: Vec<ScheduleInfo>,
    },
    ScheduleDetail {
        name: String,
        enabled: bool,
        recurrence: String,
        start_at: DateTime<Utc>,
        next_run: Option<DateTime<Utc>>,
        trigger: ScheduleTriggerPayload,
        project_root: String,
        target: String,
        scope: String,
        #[serde(default = "crate::serde_defaults::default_true")]
        root: bool,
        #[serde(default)]
        agent_name: Option<String>,
        created_at: DateTime<Utc>,
    },
    ConfigReport {
        default_agent: String,
        agents: Vec<AgentConfigInfo>,
    },
    TriggerList {
        triggers: Vec<TriggerInfo>,
    },
    TriggerDetail(TriggerInfo),
    GateResult {
        passed: bool,
        output: String,
    },
    DiffResult {
        diffs: Vec<WorktreeDiffEntry>,
    },
    PulseReport {
        worktrees: Vec<WorktreePulseEntry>,
        root_agents: Vec<AgentPulseEntry>,
    },
    Ok,
    ShuttingDown,
    Error {
        code: String,
        message: String,
    },
}
