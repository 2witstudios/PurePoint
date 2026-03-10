use pu_core::error::PuError;
use pu_core::protocol::{AgentStatusReport, GridCommand, Response};
use pu_core::types::WorktreeEntry;

use super::Engine;

impl Engine {
    // --- Grid ---

    pub(super) async fn handle_subscribe_grid(&self, project_root: &str) -> Response {
        self.ensure_grid_channel(project_root).await;
        Response::GridSubscribed
    }

    pub async fn handle_grid_command(&self, project_root: &str, command: GridCommand) -> Response {
        // For GetLayout, read the grid-layout.json directly
        if matches!(command, GridCommand::GetLayout) {
            let root = project_root.to_string();
            return match tokio::task::spawn_blocking(move || {
                let path =
                    pu_core::paths::pu_dir(std::path::Path::new(&root)).join("grid-layout.json");
                std::fs::read_to_string(path)
            })
            .await
            {
                Ok(Ok(contents)) => match serde_json::from_str(&contents) {
                    Ok(layout) => Response::GridLayout { layout },
                    Err(e) => Response::Error {
                        code: "PARSE_ERROR".into(),
                        message: format!("invalid grid layout JSON: {e}"),
                    },
                },
                _ => Response::GridLayout {
                    layout: serde_json::Value::Null,
                },
            };
        }

        // Broadcast mutation commands to subscribers
        let channels = self.grid_channels.lock().await;
        if let Some(tx) = channels.get(project_root) {
            let _ = tx.send(command.clone());
        }
        Response::Ok
    }

    async fn ensure_grid_channel(&self, project_root: &str) {
        let mut channels = self.grid_channels.lock().await;
        channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
    }

    /// Get a grid broadcast receiver for a project (used by IPC server for streaming).
    pub async fn subscribe_grid(
        &self,
        project_root: &str,
    ) -> tokio::sync::broadcast::Receiver<GridCommand> {
        let mut channels = self.grid_channels.lock().await;
        let tx = channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
        tx.subscribe()
    }

    // --- Status Push ---

    pub(super) async fn handle_subscribe_status(&self, project_root: &str) -> Response {
        self.ensure_status_channel(project_root).await;
        Response::StatusSubscribed
    }

    async fn ensure_status_channel(&self, project_root: &str) {
        let mut channels = self.status_channels.lock().await;
        channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
    }

    /// Get a status broadcast receiver for a project (used by IPC server for streaming).
    pub async fn subscribe_status(
        &self,
        project_root: &str,
    ) -> tokio::sync::broadcast::Receiver<()> {
        let mut channels = self.status_channels.lock().await;
        let tx = channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
        tx.subscribe()
    }

    /// Notify all status subscribers that state has changed.
    pub(super) async fn notify_status_change(&self, project_root: &str) {
        let channels = self.status_channels.lock().await;
        if let Some(tx) = channels.get(project_root) {
            let _ = tx.send(());
        }
    }

    /// Compute a full status report for a project (used by status push and handle_status).
    pub async fn compute_full_status(
        &self,
        project_root: &str,
    ) -> Result<(Vec<WorktreeEntry>, Vec<AgentStatusReport>), PuError> {
        let m = self.read_manifest_async(project_root).await?;
        let sessions = self.sessions.lock().await;
        let mut agents: Vec<AgentStatusReport> = m
            .agents
            .values()
            .map(|a| self.build_agent_status_report(a, &sessions, None))
            .collect();
        agents.sort_by_key(|a| a.started_at);
        let worktrees: Vec<WorktreeEntry> = m
            .worktrees
            .into_values()
            .map(|mut wt| {
                for agent in wt.agents.values_mut() {
                    let (status, exit_code, _idle) =
                        self.live_agent_status_sync(&agent.id, agent, &sessions);
                    agent.status = status;
                    agent.exit_code = exit_code;
                }
                wt
            })
            .collect();
        Ok((worktrees, agents))
    }
}
