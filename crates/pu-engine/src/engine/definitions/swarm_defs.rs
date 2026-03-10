use std::path::Path;

use pu_core::paths;
use pu_core::protocol::{Response, SwarmDefInfo, SwarmRosterEntryPayload};

use super::super::{Engine, SpawnParams};

impl Engine {
    pub(in crate::engine) async fn handle_list_swarm_defs(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let defs = pu_core::swarm_def::list_swarm_defs(root);
            let infos: Vec<SwarmDefInfo> = defs
                .into_iter()
                .map(|d| SwarmDefInfo {
                    name: d.name,
                    worktree_count: d.worktree_count,
                    worktree_template: d.worktree_template,
                    roster: d
                        .roster
                        .into_iter()
                        .map(|r| SwarmRosterEntryPayload {
                            agent_def: r.agent_def,
                            role: r.role,
                            quantity: r.quantity,
                        })
                        .collect(),
                    include_terminal: d.include_terminal,
                    scope: d.scope,
                })
                .collect();
            infos
        })
        .await
        {
            Ok(swarm_defs) => Response::SwarmDefList { swarm_defs },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_get_swarm_def(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::swarm_def::find_swarm_def(Path::new(&pr), &n)
        })
        .await
        {
            Ok(Some(d)) => Response::SwarmDefDetail {
                name: d.name,
                worktree_count: d.worktree_count,
                worktree_template: d.worktree_template,
                roster: d
                    .roster
                    .into_iter()
                    .map(|r| SwarmRosterEntryPayload {
                        agent_def: r.agent_def,
                        role: r.role,
                        quantity: r.quantity,
                    })
                    .collect(),
                include_terminal: d.include_terminal,
                scope: d.scope,
            },
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("swarm def '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) async fn handle_save_swarm_def(
        &self,
        project_root: &str,
        name: &str,
        worktree_count: u32,
        worktree_template: &str,
        roster: Vec<SwarmRosterEntryPayload>,
        include_terminal: bool,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::swarms_dir,
            paths::global_swarms_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let def = pu_core::swarm_def::SwarmDef {
            name: name.to_string(),
            worktree_count,
            worktree_template: worktree_template.to_string(),
            roster: roster
                .into_iter()
                .map(|r| pu_core::swarm_def::SwarmRosterEntry {
                    agent_def: r.agent_def,
                    role: r.role,
                    quantity: r.quantity,
                })
                .collect(),
            include_terminal,
            scope: scope.to_string(),
        };
        match tokio::task::spawn_blocking(move || pu_core::swarm_def::save_swarm_def(&dir, &def))
            .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save swarm def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_delete_swarm_def(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::swarms_dir,
            paths::global_swarms_dir,
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
        match tokio::task::spawn_blocking(move || pu_core::swarm_def::delete_swarm_def(&dir, &n))
            .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete swarm def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    // --- RunSwarm handler ---

    pub(in crate::engine) async fn handle_run_swarm(
        &self,
        project_root: &str,
        swarm_name: &str,
        vars: std::collections::HashMap<String, String>,
    ) -> Response {
        // Read the swarm definition
        let pr = project_root.to_string();
        let sn = swarm_name.to_string();
        let swarm_def = match tokio::task::spawn_blocking(move || {
            pu_core::swarm_def::find_swarm_def(Path::new(&pr), &sn)
        })
        .await
        {
            Ok(Some(def)) => def,
            Ok(None) => {
                return Response::Error {
                    code: "NOT_FOUND".into(),
                    message: format!("swarm def '{swarm_name}' not found"),
                };
            }
            Err(e) => {
                return Response::Error {
                    code: "INTERNAL_ERROR".into(),
                    message: format!("task join error: {e}"),
                };
            }
        };

        // Pre-resolve all agent defs and their prompts once, before iterating worktrees.
        let mut resolved_roster: Vec<(pu_core::agent_def::AgentDef, String, Option<String>, u32)> =
            Vec::new();
        for entry in &swarm_def.roster {
            let pr2 = project_root.to_string();
            let ad_name = entry.agent_def.clone();
            let agent_def = match tokio::task::spawn_blocking(move || {
                pu_core::agent_def::find_agent_def(Path::new(&pr2), &ad_name)
            })
            .await
            {
                Ok(Some(def)) => def,
                Ok(None) => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!(
                            "agent def '{}' referenced by swarm not found",
                            entry.agent_def
                        ),
                    };
                }
                Err(e) => {
                    return Response::Error {
                        code: "INTERNAL_ERROR".into(),
                        message: format!("task join error: {e}"),
                    };
                }
            };

            let (prompt, template_command) = if let Some(ref tpl_name) = agent_def.template {
                let pr3 = project_root.to_string();
                let tn = tpl_name.clone();
                let vars_clone = vars.clone();
                match tokio::task::spawn_blocking(move || {
                    pu_core::template::find_template(Path::new(&pr3), &tn)
                })
                .await
                {
                    Ok(Some(tpl)) => {
                        let rendered = pu_core::template::render(&tpl, &vars_clone);
                        let cmd = pu_core::template::render_command(&tpl, &vars_clone);
                        (rendered, cmd)
                    }
                    Ok(None) => {
                        return Response::Error {
                            code: "NOT_FOUND".into(),
                            message: format!("template '{tpl_name}' not found"),
                        };
                    }
                    Err(e) => {
                        return Response::Error {
                            code: "INTERNAL_ERROR".into(),
                            message: format!("task join error: {e}"),
                        };
                    }
                }
            } else {
                (agent_def.inline_prompt.clone().unwrap_or_default(), None)
            };

            resolved_roster.push((agent_def, prompt, template_command, entry.quantity));
        }

        let mut spawned_agents = Vec::new();

        for wt_index in 0..swarm_def.worktree_count {
            let wt_name = if swarm_def.worktree_template.is_empty() {
                format!("{swarm_name}-{wt_index}")
            } else {
                swarm_def
                    .worktree_template
                    .replace("{index}", &wt_index.to_string())
            };

            let mut worktree_id: Option<String> = None;

            for (agent_def, prompt, template_command, quantity) in &resolved_roster {
                for q in 0..*quantity {
                    let agent_name = format!("{}-{}-{wt_index}-{q}", swarm_name, agent_def.name);

                    // First agent creates the worktree; subsequent agents reuse it
                    let (spawn_name, spawn_worktree) = if worktree_id.is_some() {
                        (Some(agent_name), worktree_id.clone())
                    } else {
                        (Some(wt_name.clone()), None)
                    };

                    // Agent def command takes precedence, then template command
                    let resolved_command = agent_def
                        .command
                        .clone()
                        .or_else(|| template_command.clone());

                    let resp = self
                        .handle_spawn(SpawnParams {
                            project_root: project_root.to_string(),
                            prompt: prompt.to_string(),
                            agent_type: agent_def.agent_type.clone(),
                            name: spawn_name,
                            base: None,
                            root: false,
                            worktree: spawn_worktree,
                            terminal_command: resolved_command,
                            no_auto: false,
                            extra_args: vec![],
                            plan_mode: false,
                            no_trigger: false,
                            trigger: None,
                        })
                        .await;

                    match resp {
                        Response::SpawnResult {
                            agent_id,
                            worktree_id: wt_id,
                            ..
                        } => {
                            spawned_agents.push(agent_id);
                            if worktree_id.is_none() {
                                worktree_id = wt_id;
                            }
                        }
                        Response::Error { code, message } => {
                            return Response::RunSwarmPartial {
                                spawned_agents,
                                error_code: code,
                                error_message: message,
                            };
                        }
                        _ => {}
                    }
                }
            }

            // If include_terminal is set, spawn a bare terminal into this worktree
            if swarm_def.include_terminal {
                if let Some(ref wt_id) = worktree_id {
                    let term_name = format!("{swarm_name}-terminal-{wt_index}");
                    let resp = self
                        .handle_spawn(SpawnParams {
                            project_root: project_root.to_string(),
                            prompt: String::new(),
                            agent_type: "terminal".into(),
                            name: Some(term_name),
                            base: None,
                            root: false,
                            worktree: Some(wt_id.clone()),
                            terminal_command: None,
                            no_auto: false,
                            extra_args: vec![],
                            plan_mode: false,
                            no_trigger: false,
                            trigger: None,
                        })
                        .await;
                    if let Response::SpawnResult { agent_id, .. } = resp {
                        spawned_agents.push(agent_id);
                    }
                }
            }
        }

        Response::RunSwarmResult { spawned_agents }
    }
}
