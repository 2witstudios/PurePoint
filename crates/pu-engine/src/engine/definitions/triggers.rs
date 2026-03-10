use std::path::Path;

use pu_core::protocol::Response;

use super::super::{Engine, SaveTriggerParams};

impl Engine {
    pub(in crate::engine) async fn handle_list_triggers(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            let defs = pu_core::trigger_def::list_trigger_defs(Path::new(&pr));
            let triggers: Vec<_> = defs
                .into_iter()
                .map(pu_core::protocol::TriggerInfo::from)
                .collect();
            Response::TriggerList { triggers }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    pub(in crate::engine) async fn handle_get_trigger(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            match pu_core::trigger_def::find_trigger_def(Path::new(&pr), &name) {
                Some(def) => Response::TriggerDetail(pu_core::protocol::TriggerInfo::from(def)),
                None => Response::Error {
                    code: "NOT_FOUND".into(),
                    message: format!("trigger not found: {name}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    pub(in crate::engine) async fn handle_save_trigger(
        &self,
        params: SaveTriggerParams,
    ) -> Response {
        let pr = params.project_root;
        let name = params.name;
        // Normalize hyphenated form to underscored form (consistent with handle_evaluate_gate)
        let on = params.on.replace('-', "_");
        let scope = params.scope;
        let description = params.description;
        let sequence = params.sequence;
        let variables = params.variables;
        tokio::task::spawn_blocking(move || {
            let event = match on.as_str() {
                "agent_idle" => pu_core::trigger_def::TriggerEvent::AgentIdle,
                "pre_commit" => pu_core::trigger_def::TriggerEvent::PreCommit,
                "pre_push" => pu_core::trigger_def::TriggerEvent::PrePush,
                other => {
                    return Response::Error {
                        code: "INVALID_ARGUMENT".into(),
                        message: format!("unknown trigger event: {other}"),
                    };
                }
            };
            let actions: Vec<pu_core::trigger_def::TriggerAction> =
                sequence.into_iter().map(Into::into).collect();
            let def = pu_core::trigger_def::TriggerDef {
                name: name.clone(),
                description,
                on: event,
                sequence: actions,
                variables,
                scope: scope.clone(),
            };
            let dir = match Self::resolve_scope_dir(
                &pr,
                &scope,
                pu_core::paths::triggers_dir,
                pu_core::paths::global_triggers_dir,
            ) {
                Ok(d) => d,
                Err(e) => {
                    return Response::Error {
                        code: "IO_ERROR".into(),
                        message: e,
                    };
                }
            };
            match pu_core::trigger_def::save_trigger_def(&dir, &def) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to save trigger: {e}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    pub(in crate::engine) async fn handle_delete_trigger(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let name = name.to_string();
        let scope = scope.to_string();
        tokio::task::spawn_blocking(move || {
            let dir = match Self::resolve_scope_dir(
                &pr,
                &scope,
                pu_core::paths::triggers_dir,
                pu_core::paths::global_triggers_dir,
            ) {
                Ok(d) => d,
                Err(e) => {
                    return Response::Error {
                        code: "IO_ERROR".into(),
                        message: e,
                    };
                }
            };
            match pu_core::trigger_def::delete_trigger_def(&dir, &name) {
                Ok(true) => Response::Ok,
                Ok(false) => Response::Error {
                    code: "NOT_FOUND".into(),
                    message: format!("trigger not found: {name}"),
                },
                Err(e) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to delete trigger: {e}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    pub(in crate::engine) async fn handle_evaluate_gate(
        &self,
        event: &str,
        project_root: &str,
        worktree_path: &str,
    ) -> Response {
        // Normalize hyphenated form (from git hooks) to underscored form
        let normalized_event = event.replace('-', "_");
        let trigger_event = match normalized_event.as_str() {
            "pre_commit" => pu_core::trigger_def::TriggerEvent::PreCommit,
            "pre_push" => pu_core::trigger_def::TriggerEvent::PrePush,
            other => {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: format!("unsupported gate event: {other}"),
                };
            }
        };

        let triggers = {
            let pr = project_root.to_string();
            let evt = trigger_event.clone();
            match tokio::task::spawn_blocking(move || {
                pu_core::trigger_def::triggers_for_event(Path::new(&pr), &evt)
            })
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    return Response::Error {
                        code: "INTERNAL_ERROR".into(),
                        message: format!("task join error: {e}"),
                    };
                }
            }
        };

        if triggers.is_empty() {
            return Response::GateResult {
                passed: true,
                output: String::new(),
            };
        }

        let wt = worktree_path.to_string();
        match crate::gate::evaluate_trigger_gates(&triggers, Path::new(&wt)).await {
            Ok(result) => Response::GateResult {
                passed: result.passed,
                output: result.output,
            },
            Err(e) => Response::GateResult {
                passed: false,
                output: format!("gate evaluation error: {e}"),
            },
        }
    }
}
