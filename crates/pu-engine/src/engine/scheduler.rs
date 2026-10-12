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
        let owner = self.work_owner.clone();
        let service = scheduler_service(owner.clone(), Duration::from_secs(30), move || {
            let engine = engine.clone();
            async move { engine.scheduler_tick().await }
        });
        if let Some(owner) = owner {
            if let Err(error) = owner.spawn(service) {
                tracing::debug!(%error, "scheduler not started after daemon shutdown");
            }
        } else {
            tokio::spawn(service);
        }
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
                if let Some(next_run) = def.next_run
                    && next_run <= now
                {
                    self.fire_schedule(&def).await;
                    self.advance_schedule(def, now).await;
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

// A stop signal is selected only between ticks. Once a tick starts, keep its
// owner and await every spawn/write/persistence stage before observing stop.
async fn scheduler_service<F, Fut>(
    owner: Option<Arc<crate::daemon_lifecycle::owned_work::WorkOwner>>,
    period: Duration,
    mut tick: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = async {
                match &owner {
                    Some(owner) => owner.stopping().await,
                    None => std::future::pending().await,
                }
            } => break,
            _ = interval.tick() => {}
        }
        // Stop may arrive between polling the stop future and a ready timer.
        // Linearize tick admission with this final stop-state observation.
        if owner.as_ref().is_some_and(|owner| owner.is_stopping()) {
            break;
        }
        tick().await;
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use nix::fcntl::{Flock, FlockArg};
    use pu_core::{
        manifest,
        types::{AgentEntry, Manifest},
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn given_idle_registered_scheduler_should_stop_when_unpolled_server_is_dropped() {
        let tmp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let socket = tmp.path().join("daemon.sock");
        let server = crate::ipc_server::IpcServer::bind(&socket, Engine::new()).unwrap();
        let owner = server.work();
        // No registered projects: the real service never inspects global defs.
        server.engine().start_scheduler();
        drop(server);
        tokio::time::timeout(Duration::from_secs(1), owner.wait_idle())
            .await
            .unwrap();
        assert_eq!(owner.completed(), 1);
        assert!(
            crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_stopping_scheduler_should_finish_blocked_tick_and_never_start_another_tick() {
        let tmp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let root = tmp.path().join("project");
        fs::create_dir_all(paths::pu_dir(&root)).unwrap();
        let mut m = Manifest::new(root.to_string_lossy().into_owned());
        let agent: AgentEntry = serde_json::from_value(serde_json::json!({
            "id":"ag-service", "name":"before", "agentType":"claude",
            "status":"running", "startedAt":"2026-01-01T00:00:00Z"
        }))
        .unwrap();
        m.agents.insert(agent.id.clone(), agent);
        manifest::write_manifest(&root, &m).unwrap();
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(paths::manifest_path(&root).with_extension("json.lock"))
            .unwrap();
        let manifest_lock = Flock::lock(file, FlockArg::LockExclusive).unwrap();
        let socket = tmp.path().join("daemon.sock");
        let server = crate::ipc_server::IpcServer::bind(&socket, Engine::new()).unwrap();
        let owner = server.work();
        let engine = server.engine().clone();
        let ticks = Arc::new(AtomicUsize::new(0));
        let count = ticks.clone();
        let project = root.to_string_lossy().into_owned();
        // Exercise the production service loop with a real blocked engine
        // mutation. No schedule-definition lookup can reach the user's home.
        let service = scheduler_service(Some(owner.clone()), Duration::from_millis(1), move || {
            let engine = engine.clone();
            let project = project.clone();
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                assert!(matches!(
                    engine
                        .handle_request(Request::Rename {
                            project_root: project,
                            agent_id: "ag-service".into(),
                            name: "tick-completed".into()
                        })
                        .await,
                    Response::RenameResult { .. }
                ));
            }
        });
        let service = owner.spawn(service).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !crate::ipc_server::security_tests::another_descriptor_for(&manifest_lock) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        drop(server); // stop the service even though server.run was never polled
        assert_eq!(
            owner
                .drain_for(Duration::from_millis(20))
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::TimedOut
        );
        assert!(!service.is_finished());
        assert!(
            crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
                .unwrap()
                .is_none()
        );
        assert_eq!(ticks.load(Ordering::SeqCst), 1);
        drop(manifest_lock);
        tokio::time::timeout(Duration::from_secs(3), service)
            .await
            .unwrap()
            .unwrap();
        owner.wait_idle().await;
        assert_eq!(ticks.load(Ordering::SeqCst), 1);
        assert_eq!(owner.completed(), 1);
        assert_eq!(
            manifest::read_manifest(&root).unwrap().agents["ag-service"].name,
            "tick-completed"
        );
        assert!(
            owner.spawn(async {}).is_err(),
            "closed service owner refuses new work"
        );
        assert!(crate::ipc_server::IpcServer::bind(&socket, Engine::new()).is_ok());
    }
}
