mod encoding;
mod grid;
mod inventory;
mod payloads;
mod targets;

#[cfg(test)]
mod tests;

pub use crate::channel::{ChannelAuthor, ChannelMessage, ChannelReaction, ChannelReference};
pub use grid::*;
pub use inventory::*;
pub use payloads::*;
pub use targets::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{AgentStatus, WorktreeEntry};
use indexmap::IndexMap;

pub const PROTOCOL_VERSION: u32 = 14;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Read {
        project_root: String,
        agent_id: String,
        limit: usize,
        #[serde(default)]
        since: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
    },
    ChannelRead {
        project_root: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        after: Option<u64>,
        #[serde(default)]
        before: Option<u64>,
        #[serde(default = "default_channel_limit")]
        limit: usize,
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        parent_id: Option<String>,
        #[serde(default)]
        known_revision: Option<u64>,
    },
    ChannelSend {
        project_root: String,
        #[serde(default)]
        agent_id: Option<String>,
        text: String,
        #[serde(default)]
        parent_id: Option<String>,
        #[serde(default)]
        references: Vec<ChannelReference>,
    },
    ChannelEdit {
        project_root: String,
        #[serde(default)]
        agent_id: Option<String>,
        message_id: String,
        text: String,
    },
    ChannelReact {
        project_root: String,
        #[serde(default)]
        agent_id: Option<String>,
        message_id: String,
        #[serde(default = "default_channel_emoji")]
        emoji: String,
        active: bool,
    },
    /// A daemon-owned inventory; no project root means all registered projects.
    Inventory {
        #[serde(default)]
        project_root: Option<String>,
        kind: InventoryKind,
        #[serde(default)]
        state: Option<AgentQueryState>,
    },
    ResolveAgent {
        agent_id: String,
        #[serde(default)]
        project_root: Option<String>,
    },
    /// Set the registered projects' order, which every client shows. Registered
    /// roots left out follow in their current order; unknown roots are ignored.
    ReorderProjects {
        project_roots: Vec<String>,
    },
    Health,
    Init {
        project_root: String,
    },
    /// Forget a registered project on this daemon. Its files and `.pu/` are kept
    /// (suspending agents only updates the manifest), so a later `Init` brings
    /// the project back as it was.
    RemoveProject {
        project_root: String,
        /// Suspend the project's live agents instead of refusing the removal.
        #[serde(default)]
        suspend_agents: bool,
    },
    /// Stream the registered project roots: once now, then on every change.
    SubscribeProjects,
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
        /// Launch the bare agent command, ignoring configured launch args (project and machine).
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
        /// The app tab this agent is for; recorded on its manifest entry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement: Option<crate::types::SpawnPlacement>,
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
        #[serde(default)]
        replay: Option<Replay>,
        #[serde(default)]
        client_id: Option<String>,
    },
    ClaimSize {
        cols: u16,
        rows: u16,
    },
    Input {
        agent_id: String,
        #[serde(with = "encoding")]
        data: Vec<u8>,
        /// When true, the engine types data and submits it with Enter (\r).
        /// For agents with a readable input box (Claude) delivery is confirmed
        /// against the screen and failure returns `DELIVERY_FAILED`; other
        /// agents get chunked typing plus a delayed Enter.
        #[serde(default)]
        submit: bool,
        #[serde(default)]
        // A dedicated operation socket: EOF cancels any remaining delivery steps.
        cancel_on_disconnect: bool,
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
    GetGlobalAgentSettings,
    /// Fields left out keep their stored value.
    UpdateGlobalAgentSettings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_status_tracking: Option<bool>,
        /// Machine-level launch args string per agent type; a blank string
        /// removes that agent's entry. Agents left out keep their stored value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        launch_args: Option<IndexMap<String, String>>,
        /// Machine-level command per agent type; a blank string removes it.
        /// An agent type that is not built in adds that agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commands: Option<IndexMap<String, String>>,
        /// Agent types whose machine-level command and launch args are removed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remove_agents: Option<Vec<String>>,
    },
    GetConfig {
        project_root: String,
    },
    UpdateAgentConfig {
        project_root: String,
        agent_name: String,
        /// A shell-style string or a list of args; `null` clears the project value.
        #[serde(
            default,
            deserialize_with = "crate::types::launch_args_serde::deserialize"
        )]
        launch_args: Option<Vec<String>>,
        /// The project's command; left out keeps it, a blank string clears it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// Drop the agent from the project config.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        remove: bool,
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
    // File and git access confined to registered projects (MACH-007).
    FsList {
        path: String,
        #[serde(default)]
        recursive: bool,
        #[serde(default = "crate::serde_defaults::default_true")]
        include_hidden: bool,
        #[serde(default = "default_fs_list_max_entries")]
        max_entries: usize,
        /// Folder browsing for choosing a project root (the iPad's Open Project): lists
        /// only the subdirectories of any directory the daemon user can read, never
        /// files, non-recursively. `~` names the daemon user's home.
        #[serde(default)]
        directories_only: bool,
    },
    FsStat {
        path: String,
    },
    FsRead {
        path: String,
        #[serde(default = "default_fs_read_max_bytes")]
        max_bytes: u64,
    },
    FsWrite {
        path: String,
        data_base64: String,
        #[serde(default)]
        expected_modified_ms: Option<i64>,
        #[serde(default)]
        create_dirs: bool,
    },
    FsDelete {
        path: String,
    },
    SubscribeFs {
        root: String,
        #[serde(default = "crate::serde_defaults::default_true")]
        recursive: bool,
    },
    /// One chunk of a file sent to the daemon's machine, so an agent there can
    /// read it by path. Chunks arrive in order: `offset` is the number of bytes
    /// already received, and 0 starts the upload over.
    AttachFile {
        upload_id: String,
        name: String,
        #[serde(default)]
        offset: u64,
        data_base64: String,
        #[serde(default)]
        done: bool,
    },
    GitExec {
        cwd: String,
        program: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        stdin_base64: Option<String>,
        #[serde(default = "default_git_exec_timeout_ms")]
        timeout_ms: u64,
    },
}

pub fn default_channel_limit() -> usize {
    100
}
pub fn default_channel_emoji() -> String {
    "👍".into()
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
    ChannelHistory {
        messages: Vec<ChannelMessage>,
        revision: u64,
        latest_sequence: u64,
        has_more: bool,
        oldest_sequence: Option<u64>,
        unchanged: bool,
        self_author_id: String,
        reply_counts: std::collections::BTreeMap<String, u64>,
    },
    ChannelMessage {
        message: ChannelMessage,
        revision: u64,
    },
    Inventory(InventoryReport),
    AgentResolved {
        agent_id: String,
        project_root: Option<String>,
        worktree_id: Option<String>,
    },
    /// The registered projects in their new order.
    ProjectOrder {
        project_roots: Vec<String>,
    },
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
    ProjectRemoved {
        project_root: String,
        /// False when the root was not registered.
        removed: bool,
        /// Agents suspended so they can be resumed once the project is added again.
        suspended: Vec<String>,
        /// Live sessions the manifest did not list, which cannot be resumed.
        #[serde(default)]
        killed: Vec<String>,
    },
    ProjectsSubscribed,
    ProjectsChanged {
        projects: Vec<String>,
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
    ReadResult(crate::read::ReadReport),
    AttachReady {
        buffered_bytes: usize,
    },
    Resync,
    PtySize {
        cols: u16,
        rows: u16,
        owner_client_id: Option<String>,
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
        directory_removed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
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
    GlobalAgentSettingsReport {
        /// Deprecated and always `false`: Codex YOLO was removed. Still sent so
        /// protocol-12 apps that require the field can decode the report; drop it
        /// with the next protocol version bump.
        #[serde(default)]
        codex_yolo: bool,
        #[serde(default = "crate::serde_defaults::default_true")]
        agent_status_tracking: bool,
        /// Machine-level launch args string per agent type.
        #[serde(default)]
        launch_args: IndexMap<String, String>,
        /// Machine-level command per agent type. `None` from daemons that
        /// predate machine-level commands.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commands: Option<IndexMap<String, String>>,
    },
    ConfigReport {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal_available: Option<bool>,
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
    FsListResult {
        path: String,
        entries: Vec<FsEntry>,
        truncated: bool,
    },
    FsStatResult {
        path: String,
        exists: bool,
        is_dir: bool,
        size: u64,
        modified_ms: i64,
    },
    FsReadResult {
        path: String,
        data_base64: String,
        size: u64,
        modified_ms: i64,
        truncated: bool,
    },
    FsWriteResult {
        path: String,
        size: u64,
        modified_ms: i64,
    },
    FsSubscribed,
    FsEvent {
        root: String,
        paths: Vec<String>,
    },
    /// `path` is set once the last chunk (`done`) is written.
    AttachFileResult {
        received: u64,
        #[serde(default)]
        path: Option<String>,
    },
    GitExecResult {
        exit_code: i32,
        stdout_base64: String,
        stderr_base64: String,
        truncated: bool,
    },
    Ok,
    ShuttingDown,
    Error {
        code: String,
        message: String,
    },
}

impl Request {
    /// Mutable project scope for legacy project-scoped operations.
    pub fn project_root_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Read { project_root, .. }
            | Self::ChannelRead { project_root, .. }
            | Self::ChannelSend { project_root, .. }
            | Self::ChannelEdit { project_root, .. }
            | Self::ChannelReact { project_root, .. }
            | Self::Init { project_root, .. }
            | Self::Spawn { project_root, .. }
            | Self::Status { project_root, .. }
            | Self::Kill { project_root, .. }
            | Self::Suspend { project_root, .. }
            | Self::Resume { project_root, .. }
            | Self::SubscribeGrid { project_root, .. }
            | Self::SubscribeStatus { project_root, .. }
            | Self::GridCommand { project_root, .. }
            | Self::Rename { project_root, .. }
            | Self::AssignTrigger { project_root, .. }
            | Self::CreateWorktree { project_root, .. }
            | Self::DeleteWorktree { project_root, .. }
            | Self::ListTemplates { project_root, .. }
            | Self::GetTemplate { project_root, .. }
            | Self::SaveTemplate { project_root, .. }
            | Self::DeleteTemplate { project_root, .. }
            | Self::ListAgentDefs { project_root, .. }
            | Self::GetAgentDef { project_root, .. }
            | Self::SaveAgentDef { project_root, .. }
            | Self::DeleteAgentDef { project_root, .. }
            | Self::ListSwarmDefs { project_root, .. }
            | Self::GetSwarmDef { project_root, .. }
            | Self::SaveSwarmDef { project_root, .. }
            | Self::DeleteSwarmDef { project_root, .. }
            | Self::RunSwarm { project_root, .. }
            | Self::ListSchedules { project_root, .. }
            | Self::GetSchedule { project_root, .. }
            | Self::SaveSchedule { project_root, .. }
            | Self::DeleteSchedule { project_root, .. }
            | Self::EnableSchedule { project_root, .. }
            | Self::DisableSchedule { project_root, .. }
            | Self::GetConfig { project_root, .. }
            | Self::UpdateAgentConfig { project_root, .. }
            | Self::ListTriggers { project_root, .. }
            | Self::GetTrigger { project_root, .. }
            | Self::SaveTrigger { project_root, .. }
            | Self::DeleteTrigger { project_root, .. }
            | Self::EvaluateGate { project_root, .. }
            | Self::Diff { project_root, .. }
            | Self::Pulse { project_root, .. } => Some(project_root),
            _ => None,
        }
    }
}

/// Replay strategy. Omission preserves retained raw replay for legacy clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Replay {
    #[default]
    Full,
    Snapshot,
}
