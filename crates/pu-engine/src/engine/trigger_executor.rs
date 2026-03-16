use std::path::Path;

use pu_core::manifest;
use pu_core::types::AgentStatus;

use super::Engine;

impl Engine {
    /// Check all agents with active trigger sequences and advance them when idle.
    pub(super) async fn evaluate_idle_triggers(&self, project_root: &str) {
        let manifest = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(_) => return,
        };

        // Collect candidate agents with their trigger name, seq index, and worktree path.
        // Briefly hold the sessions lock to check live status, then release before I/O.
        let candidates: Vec<(String, String, u32, Option<std::path::PathBuf>)> = {
            let sessions = self.sessions.lock().await;
            let mut result = Vec::new();
            for agent in manifest.all_agents() {
                if agent.trigger_state != Some(pu_core::types::TriggerState::Active) {
                    continue;
                }
                let trigger_name = match &agent.trigger_name {
                    Some(name) => name.clone(),
                    None => continue, // No bound trigger, skip
                };
                let seq_index = agent.trigger_seq_index.unwrap_or(0);
                let (status, _, idle_seconds) =
                    self.live_agent_status_sync(&agent.id, agent, &sessions);
                if status != AgentStatus::Running {
                    continue;
                }
                // Only fire idle triggers after 30s of content idle time
                if idle_seconds.unwrap_or(0) < 30 {
                    continue;
                }
                let wt_path = match manifest.find_agent(&agent.id) {
                    Some(pu_core::types::AgentLocation::Worktree { worktree, .. }) => {
                        Some(std::path::PathBuf::from(&worktree.path))
                    }
                    _ => None,
                };
                result.push((agent.id.clone(), trigger_name, seq_index, wt_path));
            }
            result
            // sessions lock dropped here
        };

        if candidates.is_empty() {
            return;
        }

        // Load trigger defs once for this project
        let pr = project_root.to_string();
        let idle_triggers = match tokio::task::spawn_blocking(move || {
            pu_core::trigger_def::triggers_for_event(
                Path::new(&pr),
                &pu_core::trigger_def::TriggerEvent::AgentIdle,
            )
        })
        .await
        {
            Ok(t) => t,
            Err(_) => return,
        };

        // Index triggers by name for O(1) lookup
        let trigger_map: std::collections::HashMap<&str, &pu_core::trigger_def::TriggerDef> =
            idle_triggers.iter().map(|t| (t.name.as_str(), t)).collect();

        for (agent_id, trigger_name, seq_index, wt_path) in &candidates {
            let trigger = match trigger_map.get(trigger_name.as_str()) {
                Some(t) => t,
                None => {
                    // Trigger was removed since spawn — mark failed
                    self.update_trigger_state(
                        project_root,
                        agent_id,
                        pu_core::types::TriggerState::Failed,
                        None,
                        None,
                    )
                    .await;
                    continue;
                }
            };

            let sequence = &trigger.sequence;
            let seq_index = *seq_index as usize;
            if seq_index >= sequence.len() {
                self.update_trigger_state(
                    project_root,
                    agent_id,
                    pu_core::types::TriggerState::Completed,
                    None,
                    None,
                )
                .await;
                continue;
            }

            let action = &sequence[seq_index];
            let cwd = wt_path
                .as_deref()
                .unwrap_or_else(|| Path::new(project_root));

            // If action has a gate, evaluate it first (no lock held)
            if let Some(ref gate) = action.gate {
                let resolved_run =
                    pu_core::trigger_def::substitute_variables(&gate.run, &trigger.variables);

                // Mark as Gating while the command runs
                self.update_trigger_state(
                    project_root,
                    agent_id,
                    pu_core::types::TriggerState::Gating,
                    None,
                    None,
                )
                .await;

                match crate::gate::run_gate_command(&resolved_run, cwd).await {
                    Ok((exit_code, stdout, stderr)) => {
                        let expect_exit = gate.expect_exit.unwrap_or(0);
                        if exit_code != expect_exit {
                            let max_retries = action
                                .max_retries
                                .unwrap_or(crate::gate::DEFAULT_GATE_MAX_RETRIES);
                            let manifest = self.read_manifest_async(project_root).await;
                            let attempts = manifest
                                .ok()
                                .and_then(|m| {
                                    m.find_agent(agent_id).map(|loc| match loc {
                                        pu_core::types::AgentLocation::Root(a) => a.gate_attempts,
                                        pu_core::types::AgentLocation::Worktree {
                                            agent, ..
                                        } => agent.gate_attempts,
                                    })
                                })
                                .flatten()
                                .unwrap_or(0);

                            if attempts < max_retries {
                                let failure_msg = format!(
                                    "\n\nGate '{resolved_run}' failed (exit {exit_code}, expected {expect_exit}):\n{stdout}{stderr}\nPlease fix the issues and try again.\n"
                                );
                                if let Err(e) = self.inject_text(agent_id, &failure_msg).await {
                                    tracing::warn!(agent_id, "failed to inject gate failure: {e}");
                                }
                                self.update_trigger_state(
                                    project_root,
                                    agent_id,
                                    pu_core::types::TriggerState::Active,
                                    None,
                                    Some(attempts + 1),
                                )
                                .await;
                            } else {
                                self.update_trigger_state(
                                    project_root,
                                    agent_id,
                                    pu_core::types::TriggerState::Failed,
                                    None,
                                    None,
                                )
                                .await;
                            }
                            continue;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(agent_id, gate = %resolved_run, "gate command error: {e}");
                        self.update_trigger_state(
                            project_root,
                            agent_id,
                            pu_core::types::TriggerState::Failed,
                            None,
                            None,
                        )
                        .await;
                        continue;
                    }
                }
            }

            // Inject text if present — only advance on success
            if let Some(ref inject_text) = action.inject {
                let resolved =
                    pu_core::trigger_def::substitute_variables(inject_text, &trigger.variables);
                match self.inject_text(agent_id, &resolved).await {
                    Ok(true) => {} // success, proceed to advance
                    Ok(false) => {
                        tracing::warn!(agent_id, "inject_text: session not found, marking failed");
                        self.update_trigger_state(
                            project_root,
                            agent_id,
                            pu_core::types::TriggerState::Failed,
                            None,
                            None,
                        )
                        .await;
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!(agent_id, "inject_text failed: {e}, marking failed");
                        self.update_trigger_state(
                            project_root,
                            agent_id,
                            pu_core::types::TriggerState::Failed,
                            None,
                            None,
                        )
                        .await;
                        continue;
                    }
                }
            }

            // Advance sequence index
            let new_index = seq_index as u32 + 1;
            let new_state = if new_index >= sequence.len() as u32 {
                pu_core::types::TriggerState::Completed
            } else {
                pu_core::types::TriggerState::Active
            };
            self.update_trigger_state(project_root, agent_id, new_state, Some(new_index), Some(0))
                .await;
        }
    }

    /// Inject text into an agent's PTY using chunked typing + Enter submission.
    /// Returns `Ok(true)` on success, `Ok(false)` if the session was not found.
    pub(super) async fn inject_text(
        &self,
        agent_id: &str,
        text: &str,
    ) -> Result<bool, std::io::Error> {
        let fd = {
            let sessions = self.sessions.lock().await;
            sessions.get(agent_id).map(|handle| handle.master_fd())
        };
        match fd {
            Some(fd) => {
                self.pty_host
                    .write_chunked_submit(&fd, text.as_bytes())
                    .await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub(super) async fn update_trigger_state(
        &self,
        project_root: &str,
        agent_id: &str,
        state: pu_core::types::TriggerState,
        seq_index: Option<u32>,
        gate_attempts: Option<u32>,
    ) {
        let agent_id = agent_id.to_string();
        let pr = project_root.to_string();
        let result = tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                if let Some(agent) = m.find_agent_mut(&agent_id) {
                    agent.trigger_state = Some(state);
                    if let Some(idx) = seq_index {
                        agent.trigger_seq_index = Some(idx);
                    }
                    if let Some(attempts) = gate_attempts {
                        agent.gate_attempts = Some(attempts);
                    }
                }
                m
            })
        })
        .await;
        match result {
            Ok(Ok(_)) => {
                self.notify_status_change(project_root).await;
            }
            Ok(Err(e)) => {
                tracing::warn!("failed to update trigger state in manifest: {e}");
            }
            Err(e) => {
                tracing::warn!("trigger state update task panicked: {e}");
            }
        }
    }
}
