//! Long-lived daemon subscriptions forwarded to the UI as Tauri events (WIN-5.5).
//!
//! Ports `Services/{DaemonStatusSubscription,DaemonGridSubscription,DaemonProjectsSubscription}.swift`.
//! Each stream runs on its own connection from [`DaemonClient::connect`]:
//!
//! - [`status`]: `subscribe_status` per project root, raising `status-event`. A dropped
//!   established stream, or [`RetryPolicy::STATUS`]'s fourth failed attempt, raises
//!   `subscription-state` `daemon_lost` and ends the loop (the Swift `onDaemonLost`).
//! - [`grid`]: `subscribe_grid` per project root, raising `grid-event`; grid commands are
//!   written on the same connection ([`Subscriptions::send_grid_command`]).
//! - [`projects`]: `subscribe_projects` for the app, raising `projects-changed`; it never
//!   gives up while running.
//!
//! Every stream also raises `subscription-state` (`subscribed`, `retrying`, `daemon_lost`,
//! `failed`). Stopping a stream aborts its task, which closes its connection.
//!
//! Tauri surface (front end: `ui/src/services/subscriptions.ts`): commands
//! `subscriptions_start_project`, `subscriptions_stop_project`,
//! `subscriptions_start_projects`, `subscriptions_stop_projects` and
//! `subscriptions_grid_command`.

pub mod grid;
pub mod projects;
pub mod status;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pu_core::protocol::{AgentStatusReport, GridCommand, Response};
use pu_core::types::WorktreeEntry;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::task::JoinHandle;

use crate::daemon::DaemonState;
use crate::daemon::client::{DaemonClient, DaemonError, LineReader, parse};

pub const STATUS_EVENT: &str = "status-event";
pub const GRID_EVENT: &str = "grid-event";
pub const PROJECTS_EVENT: &str = "projects-changed";
pub const STATE_EVENT: &str = "subscription-state";

/// Reconnect timing, as each Swift subscription hard-codes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// First backoff; doubled after every failure up to `max`.
    pub initial: Duration,
    pub max: Duration,
    /// Failures after which the loop gives up (`maxRetries`); `None` never gives up.
    pub max_retries: Option<u32>,
    /// Consecutive failures after which a daemon that never answered counts as lost
    /// (`retriesBeforeDaemonLost`); `None` never reports `daemon_lost`.
    pub daemon_lost_after: Option<u32>,
}

impl RetryPolicy {
    /// `DaemonStatusSubscription`: 0.5 s doubling to 5 s, 20 retries, lost after 4 (~3.5 s).
    pub const STATUS: Self = Self {
        initial: Duration::from_millis(500),
        max: Duration::from_secs(5),
        max_retries: Some(20),
        daemon_lost_after: Some(4),
    };
    /// `DaemonGridSubscription`: 0.5 s doubling to 5 s, 20 retries.
    pub const GRID: Self = Self {
        initial: Duration::from_millis(500),
        max: Duration::from_secs(5),
        max_retries: Some(20),
        daemon_lost_after: None,
    };
    /// `DaemonProjectsSubscription`: 0.5 s doubling to 10 s, forever.
    pub const PROJECTS: Self = Self {
        initial: Duration::from_millis(500),
        max: Duration::from_secs(10),
        max_retries: None,
        daemon_lost_after: None,
    };
}

/// The policies a [`Subscriptions`] uses (tests shorten them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policies {
    pub status: RetryPolicy,
    pub grid: RetryPolicy,
    pub projects: RetryPolicy,
}

impl Default for Policies {
    fn default() -> Self {
        Self {
            status: RetryPolicy::STATUS,
            grid: RetryPolicy::GRID,
            projects: RetryPolicy::PROJECTS,
        }
    }
}

/// Which stream a `subscription-state` event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionKind {
    Status,
    Grid,
    Projects,
}

/// A stream's state change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SubscriptionState {
    /// The daemon confirmed the subscription.
    Subscribed,
    /// Attempt `attempt` failed; the next one starts after `delay_ms`.
    Retrying {
        attempt: u32,
        delay_ms: u64,
        message: String,
    },
    /// The daemon went away (status only): the loop has ended. The UI ensures a daemon,
    /// inits the project and starts the subscriptions again (`ProjectState.startWatching`).
    DaemonLost,
    /// The loop has ended: the daemon refused the subscription or the retries ran out.
    Failed { message: String },
}

/// `subscription-state` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateChange {
    pub kind: SubscriptionKind,
    /// `None` for the projects stream.
    pub project_root: Option<String>,
    #[serde(flatten)]
    pub state: SubscriptionState,
}

/// What a subscription raises; `name()` is the Tauri event, the value its payload.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum SubscriptionEvent {
    /// `status-event`: a project's full status (`status_event`).
    Status {
        project_root: String,
        worktrees: Vec<WorktreeEntry>,
        agents: Vec<AgentStatusReport>,
    },
    /// `grid-event`: a grid command for the app to apply (`grid_event`).
    Grid {
        project_root: String,
        command: GridCommand,
    },
    /// `projects-changed`: the daemon's registered roots (`projects_changed`).
    Projects { projects: Vec<String> },
    /// `subscription-state`.
    State(StateChange),
}

impl SubscriptionEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Status { .. } => STATUS_EVENT,
            Self::Grid { .. } => GRID_EVENT,
            Self::Projects { .. } => PROJECTS_EVENT,
            Self::State(_) => STATE_EVENT,
        }
    }
}

/// Delivers events (the app emits them to every window).
pub type Emit = Arc<dyn Fn(SubscriptionEvent) + Send + Sync>;

fn emit_state(
    emit: &Emit,
    kind: SubscriptionKind,
    project_root: Option<&str>,
    state: SubscriptionState,
) {
    emit(SubscriptionEvent::State(StateChange {
        kind,
        project_root: project_root.map(str::to_owned),
        state,
    }));
}

/// Why one connection's stream ended.
#[derive(Debug)]
enum Failure {
    /// Reconnect after the backoff (the Swift catch-all).
    Retry(DaemonError),
    /// Give up (`DaemonStatusError`: the daemon refused the subscription).
    Fatal(String),
}

/// The next reply: the outer error is a closed or broken stream, the inner one a line
/// that did not parse (or `BUSY`).
async fn next_reply(reader: &mut LineReader) -> Result<Result<Response, DaemonError>, DaemonError> {
    match reader.next_line().await {
        Ok(Some(line)) => Ok(parse(line)),
        Ok(None) => Err(DaemonError::Eof),
        Err(e) => Err(DaemonError::Io(format!(
            "Reading from the daemon failed: {e}"
        ))),
    }
}

/// The retry bookkeeping shared by the three loops.
struct Retries {
    policy: RetryPolicy,
    backoff: Duration,
    count: u32,
}

impl Retries {
    fn new(policy: RetryPolicy) -> Self {
        Self {
            policy,
            backoff: policy.initial,
            count: 0,
        }
    }

    /// Count a failure; false once the retries ran out.
    fn fail(&mut self) -> bool {
        self.count += 1;
        self.policy.max_retries.is_none_or(|max| self.count <= max)
    }

    fn daemon_lost(&self) -> bool {
        self.policy.daemon_lost_after == Some(self.count)
    }

    fn retrying(&self, error: &DaemonError) -> SubscriptionState {
        SubscriptionState::Retrying {
            attempt: self.count,
            delay_ms: u64::try_from(self.backoff.as_millis()).unwrap_or(u64::MAX),
            message: error.to_string(),
        }
    }

    async fn sleep(&mut self) {
        tokio::time::sleep(self.backoff).await;
        self.backoff = (self.backoff * 2).min(self.policy.max);
    }
}

struct ProjectSubscriptions {
    status: JoinHandle<()>,
    grid: JoinHandle<()>,
    grid_writer: grid::GridWriter,
}

impl Drop for ProjectSubscriptions {
    fn drop(&mut self) {
        self.status.abort();
        self.grid.abort();
    }
}

#[derive(Default)]
struct Running {
    projects: HashMap<String, ProjectSubscriptions>,
    list: Option<JoinHandle<()>>,
}

/// The app's running subscriptions (Tauri state).
pub struct Subscriptions {
    emit: Emit,
    policies: Policies,
    running: Mutex<Running>,
}

impl Subscriptions {
    pub fn new(emit: Emit, policies: Policies) -> Self {
        Self {
            emit,
            policies,
            running: Mutex::new(Running::default()),
        }
    }

    /// Subscriptions that emit to every window of `app`.
    pub fn for_app(app: AppHandle) -> Self {
        Self::new(
            Arc::new(move |event: SubscriptionEvent| {
                let _ = app.emit(event.name(), &event);
            }),
            Policies::default(),
        )
    }

    fn running(&self) -> std::sync::MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start `project_root`'s status and grid subscriptions, replacing any running ones
    /// (`ProjectState.startStatusSubscription` and `startGridSubscription`). Must be called
    /// inside a Tokio runtime.
    pub fn start_project(&self, client: &DaemonClient, project_root: &str) {
        let grid_writer = grid::GridWriter::default();
        let status = tokio::spawn(status::run(
            client.clone(),
            project_root.to_owned(),
            self.policies.status,
            self.emit.clone(),
        ));
        let grid = tokio::spawn(grid::run(
            client.clone(),
            project_root.to_owned(),
            self.policies.grid,
            self.emit.clone(),
            grid_writer.clone(),
        ));
        let previous = self.running().projects.insert(
            project_root.to_owned(),
            ProjectSubscriptions {
                status,
                grid,
                grid_writer,
            },
        );
        drop(previous);
    }

    /// Stop `project_root`'s subscriptions; false if none were running.
    pub fn stop_project(&self, project_root: &str) -> bool {
        let removed = self.running().projects.remove(project_root);
        removed.is_some()
    }

    /// Follow the registered projects unless already following them
    /// (`AppState.followLocalProjects`): `before` runs first (the app ensures a daemon).
    /// False if it was already running. Must be called inside a Tokio runtime.
    pub fn start_projects(
        &self,
        client: &DaemonClient,
        before: impl Future<Output = ()> + Send + 'static,
    ) -> bool {
        let mut running = self.running();
        if running
            .list
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return false;
        }
        let run = projects::run(client.clone(), self.policies.projects, self.emit.clone());
        running.list = Some(tokio::spawn(async move {
            before.await;
            run.await;
        }));
        true
    }

    /// Stop following the registered projects; false if it was not running.
    pub fn stop_projects(&self) -> bool {
        let task = self.running().list.take();
        task.map(|task| task.abort()).is_some()
    }

    /// Write a grid command on `project_root`'s grid connection; false when it has none
    /// (`DaemonGridSubscription.sendCommand`).
    pub async fn send_grid_command(&self, project_root: &str, command: GridCommand) -> bool {
        let writer = self
            .running()
            .projects
            .get(project_root)
            .map(|subs| subs.grid_writer.clone());
        match writer {
            Some(writer) => writer.send(project_root, command).await,
            None => false,
        }
    }

    /// Roots with running status and grid subscriptions.
    pub fn project_roots(&self) -> Vec<String> {
        let mut roots: Vec<String> = self.running().projects.keys().cloned().collect();
        roots.sort();
        roots
    }
}

/// Start (or restart) a project's status and grid subscriptions.
#[tauri::command]
pub async fn subscriptions_start_project(
    daemon: State<'_, DaemonState>,
    subscriptions: State<'_, Subscriptions>,
    project_root: String,
) -> Result<(), DaemonError> {
    let lifecycle = daemon.lifecycle()?;
    subscriptions.start_project(lifecycle.client(), &project_root);
    Ok(())
}

/// Stop a project's subscriptions; false if none were running.
#[tauri::command]
pub fn subscriptions_stop_project(
    subscriptions: State<'_, Subscriptions>,
    project_root: String,
) -> bool {
    subscriptions.stop_project(&project_root)
}

/// Follow the daemon's registered projects, starting the daemon first if needed.
#[tauri::command]
pub async fn subscriptions_start_projects(
    daemon: State<'_, DaemonState>,
    subscriptions: State<'_, Subscriptions>,
) -> Result<bool, DaemonError> {
    let lifecycle = daemon.lifecycle()?;
    let client = lifecycle.client().clone();
    Ok(subscriptions.start_projects(&client, async move {
        let _ = lifecycle.ensure_daemon().await;
    }))
}

/// Stop following the registered projects.
#[tauri::command]
pub fn subscriptions_stop_projects(subscriptions: State<'_, Subscriptions>) -> bool {
    subscriptions.stop_projects()
}

/// Send a grid command on a project's grid subscription; false when it has none.
#[tauri::command]
pub async fn subscriptions_grid_command(
    subscriptions: State<'_, Subscriptions>,
    project_root: String,
    command: GridCommand,
) -> Result<bool, DaemonError> {
    Ok(subscriptions
        .send_grid_command(&project_root, command)
        .await)
}
