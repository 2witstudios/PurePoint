use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pu_core::paths;
use pu_core::protocol::{Request, Response};

use super::Engine;

impl Engine {
    /// Start a background task that periodically checks for due schedules and fires them.
    pub fn start_scheduler(self: &Arc<Self>) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                engine.scheduler_tick().await;
            }
        });
    }

    async fn scheduler_tick(&self) {
        let projects = self.registered_projects();
        for project_root in projects {
            let defs = {
                let pr = project_root.clone();
                match tokio::task::spawn_blocking(move || {
                    pu_core::schedule_def::list_schedule_defs(Path::new(&pr))
                })
                .await
                {
                    Ok(d) => d,
                    Err(_) => continue,
                }
            };

            let now = chrono::Utc::now();
            for def in defs {
                if !def.enabled {
                    continue;
                }
                if let Some(next_run) = def.next_run {
                    if next_run <= now {
                        self.fire_schedule(&def).await;
                        self.advance_schedule(def, now).await;
                    }
                }
            }

            // Evaluate agent_idle triggers for active agents
            self.evaluate_idle_triggers(&project_root).await;
        }
    }

    async fn fire_schedule(&self, schedule: &pu_core::schedule_def::ScheduleDef) {
        let result = match &schedule.trigger {
            pu_core::schedule_def::ScheduleTrigger::AgentDef { name } => {
                // Resolve agent def to get its type and prompt
                let pr = schedule.project_root.clone();
                let project_path = Path::new(&pr);
                if let Some(def) = pu_core::agent_def::find_agent_def(project_path, name) {
                    let empty_vars = std::collections::HashMap::new();
                    let (prompt, template_command) = if let Some(ref ip) = def.inline_prompt {
                        (ip.clone(), None)
                    } else if let Some(ref tpl_name) = def.template {
                        match pu_core::template::find_template(project_path, tpl_name) {
                            Some(tpl) => {
                                let rendered = pu_core::template::render(&tpl, &empty_vars);
                                let cmd = pu_core::template::render_command(&tpl, &empty_vars);
                                (rendered, cmd)
                            }
                            None => (
                                format!("Scheduled: agent def '{name}' (template not found)"),
                                None,
                            ),
                        }
                    } else {
                        (format!("Scheduled: run agent def '{name}'"), None)
                    };
                    self.handle_request(Request::Spawn {
                        project_root: pr,
                        prompt,
                        agent: def.agent_type,
                        name: schedule.agent_name.clone(),
                        base: None,
                        root: schedule.root,
                        worktree: None,
                        command: def.command.or(template_command),
                        no_auto: false,
                        extra_args: vec![],
                        plan_mode: false,
                        no_trigger: false,
                        trigger: None,
                    })
                    .await
                } else {
                    Response::Error {
                        code: "NOT_FOUND".to_string(),
                        message: format!("agent def '{name}' not found"),
                    }
                }
            }
            pu_core::schedule_def::ScheduleTrigger::SwarmDef { name, vars } => {
                self.handle_request(Request::RunSwarm {
                    project_root: schedule.project_root.clone(),
                    swarm_name: name.clone(),
                    vars: vars.clone(),
                })
                .await
            }
            pu_core::schedule_def::ScheduleTrigger::InlinePrompt { prompt, agent } => {
                self.handle_request(Request::Spawn {
                    project_root: schedule.project_root.clone(),
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                    name: schedule.agent_name.clone(),
                    base: None,
                    root: schedule.root,
                    worktree: None,
                    command: None,
                    no_auto: false,
                    extra_args: vec![],
                    plan_mode: false,
                    no_trigger: false,
                    trigger: None,
                })
                .await
            }
        };

        if let Response::Error { code, message } = result {
            tracing::warn!(
                schedule = schedule.name,
                code,
                message,
                "scheduled task failed"
            );
        } else {
            tracing::info!(schedule = schedule.name, "scheduled task fired");
        }
    }

    async fn advance_schedule(
        &self,
        mut schedule: pu_core::schedule_def::ScheduleDef,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        let is_one_shot = schedule.recurrence == pu_core::schedule_def::Recurrence::None;
        if is_one_shot {
            schedule.enabled = false;
            schedule.next_run = None;
        } else {
            schedule.next_run = pu_core::schedule_def::next_occurrence(
                schedule.start_at,
                &schedule.recurrence,
                now,
            );
        }
        let pr = schedule.project_root.clone();
        let scope = schedule.scope.clone();
        let def = schedule;
        if let Err(e) = tokio::task::spawn_blocking(move || {
            let dir = if scope == "global" {
                paths::global_schedules_dir()?
            } else {
                paths::schedules_dir(Path::new(&pr))
            };
            pu_core::schedule_def::save_schedule_def(&dir, &def)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
        {
            tracing::warn!(error = %e, "failed to advance schedule");
        }
    }
}
