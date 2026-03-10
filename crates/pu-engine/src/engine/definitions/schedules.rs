use std::path::Path;

use pu_core::paths;
use pu_core::protocol::{Response, ScheduleInfo, ScheduleTriggerPayload};

use super::super::Engine;

impl Engine {
    pub(in crate::engine) async fn handle_list_schedules(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let defs = pu_core::schedule_def::list_schedule_defs(root);
            let infos: Vec<ScheduleInfo> =
                defs.into_iter().map(Self::schedule_def_to_info).collect();
            infos
        })
        .await
        {
            Ok(schedules) => Response::ScheduleList { schedules },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_get_schedule(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::schedule_def::find_schedule_def(Path::new(&pr), &n)
        })
        .await
        {
            Ok(Some(d)) => Self::schedule_def_to_detail(d),
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("schedule '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) async fn handle_save_schedule(
        &self,
        project_root: &str,
        name: &str,
        enabled: bool,
        recurrence: &str,
        start_at: chrono::DateTime<chrono::Utc>,
        trigger: ScheduleTriggerPayload,
        target: &str,
        scope: &str,
        root: bool,
        agent_name: Option<String>,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::schedules_dir,
            paths::global_schedules_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let rec = match Self::parse_recurrence(recurrence) {
            Ok(r) => r,
            Err(msg) => {
                return Response::Error {
                    code: "INVALID_INPUT".into(),
                    message: msg,
                };
            }
        };
        let now = chrono::Utc::now();
        let next_run = if enabled {
            pu_core::schedule_def::next_occurrence(start_at, &rec, now)
        } else {
            None
        };
        let def = pu_core::schedule_def::ScheduleDef {
            name: name.to_string(),
            enabled,
            recurrence: rec,
            start_at,
            next_run,
            trigger: Self::payload_to_trigger(&trigger),
            project_root: project_root.to_string(),
            target: target.to_string(),
            root,
            agent_name,
            scope: scope.to_string(),
            created_at: now,
        };
        match tokio::task::spawn_blocking(move || {
            pu_core::schedule_def::save_schedule_def(&dir, &def)
        })
        .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save schedule: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_delete_schedule(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::schedules_dir,
            paths::global_schedules_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::schedule_def::delete_schedule_def(&dir, &n)
        })
        .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete schedule: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_enable_schedule(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let mut def = match pu_core::schedule_def::find_schedule_def(root, &n) {
                Some(d) => d,
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("schedule '{n}' not found"),
                    };
                }
            };
            def.enabled = true;
            let now = chrono::Utc::now();
            def.next_run =
                pu_core::schedule_def::next_occurrence(def.start_at, &def.recurrence, now);
            let dir = paths::schedules_dir(root);
            match pu_core::schedule_def::save_schedule_def(&dir, &def) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to save schedule: {e}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    pub(in crate::engine) async fn handle_disable_schedule(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let mut def = match pu_core::schedule_def::find_schedule_def(root, &n) {
                Some(d) => d,
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("schedule '{n}' not found"),
                    };
                }
            };
            def.enabled = false;
            def.next_run = None;
            let dir = paths::schedules_dir(root);
            match pu_core::schedule_def::save_schedule_def(&dir, &def) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to save schedule: {e}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    pub(in crate::engine) fn schedule_def_to_info(
        d: pu_core::schedule_def::ScheduleDef,
    ) -> ScheduleInfo {
        ScheduleInfo {
            name: d.name,
            enabled: d.enabled,
            recurrence: Self::recurrence_to_string(&d.recurrence),
            start_at: d.start_at,
            next_run: d.next_run,
            trigger: Self::trigger_to_payload(&d.trigger),
            project_root: d.project_root,
            target: d.target,
            scope: d.scope,
            root: d.root,
            agent_name: d.agent_name,
            created_at: d.created_at,
        }
    }

    pub(in crate::engine) fn schedule_def_to_detail(
        d: pu_core::schedule_def::ScheduleDef,
    ) -> Response {
        Response::ScheduleDetail {
            name: d.name,
            enabled: d.enabled,
            recurrence: Self::recurrence_to_string(&d.recurrence),
            start_at: d.start_at,
            next_run: d.next_run,
            trigger: Self::trigger_to_payload(&d.trigger),
            project_root: d.project_root,
            target: d.target,
            scope: d.scope,
            root: d.root,
            agent_name: d.agent_name,
            created_at: d.created_at,
        }
    }

    pub(in crate::engine) fn recurrence_to_string(r: &pu_core::schedule_def::Recurrence) -> String {
        match r {
            pu_core::schedule_def::Recurrence::None => "none",
            pu_core::schedule_def::Recurrence::Hourly => "hourly",
            pu_core::schedule_def::Recurrence::Daily => "daily",
            pu_core::schedule_def::Recurrence::Weekdays => "weekdays",
            pu_core::schedule_def::Recurrence::Weekly => "weekly",
            pu_core::schedule_def::Recurrence::Monthly => "monthly",
        }
        .to_string()
    }

    pub(in crate::engine) fn parse_recurrence(
        s: &str,
    ) -> Result<pu_core::schedule_def::Recurrence, String> {
        match s {
            "none" => Ok(pu_core::schedule_def::Recurrence::None),
            "hourly" => Ok(pu_core::schedule_def::Recurrence::Hourly),
            "daily" => Ok(pu_core::schedule_def::Recurrence::Daily),
            "weekdays" => Ok(pu_core::schedule_def::Recurrence::Weekdays),
            "weekly" => Ok(pu_core::schedule_def::Recurrence::Weekly),
            "monthly" => Ok(pu_core::schedule_def::Recurrence::Monthly),
            other => Err(format!("unknown recurrence: {other}")),
        }
    }

    pub(in crate::engine) fn trigger_to_payload(
        t: &pu_core::schedule_def::ScheduleTrigger,
    ) -> ScheduleTriggerPayload {
        match t {
            pu_core::schedule_def::ScheduleTrigger::AgentDef { name } => {
                ScheduleTriggerPayload::AgentDef { name: name.clone() }
            }
            pu_core::schedule_def::ScheduleTrigger::SwarmDef { name, vars } => {
                ScheduleTriggerPayload::SwarmDef {
                    name: name.clone(),
                    vars: vars.clone(),
                }
            }
            pu_core::schedule_def::ScheduleTrigger::InlinePrompt { prompt, agent } => {
                ScheduleTriggerPayload::InlinePrompt {
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                }
            }
        }
    }

    pub(in crate::engine) fn payload_to_trigger(
        p: &ScheduleTriggerPayload,
    ) -> pu_core::schedule_def::ScheduleTrigger {
        match p {
            ScheduleTriggerPayload::AgentDef { name } => {
                pu_core::schedule_def::ScheduleTrigger::AgentDef { name: name.clone() }
            }
            ScheduleTriggerPayload::SwarmDef { name, vars } => {
                pu_core::schedule_def::ScheduleTrigger::SwarmDef {
                    name: name.clone(),
                    vars: vars.clone(),
                }
            }
            ScheduleTriggerPayload::InlinePrompt { prompt, agent } => {
                pu_core::schedule_def::ScheduleTrigger::InlinePrompt {
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                }
            }
        }
    }
}
