use std::path::Path;

use pu_core::paths;
use pu_core::protocol::{AgentDefInfo, Response};

use super::super::Engine;

impl Engine {
    pub(in crate::engine) async fn handle_list_agent_defs(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let defs = pu_core::agent_def::list_agent_defs(root);
            let infos: Vec<AgentDefInfo> = defs
                .into_iter()
                .map(|d| AgentDefInfo {
                    name: d.name,
                    agent_type: d.agent_type,
                    template: d.template,
                    inline_prompt: d.inline_prompt,
                    tags: d.tags,
                    scope: d.scope,
                    available_in_command_dialog: d.available_in_command_dialog,
                    icon: d.icon,
                    command: d.command,
                })
                .collect();
            infos
        })
        .await
        {
            Ok(agent_defs) => Response::AgentDefList { agent_defs },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_get_agent_def(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::agent_def::find_agent_def(Path::new(&pr), &n)
        })
        .await
        {
            Ok(Some(d)) => Response::AgentDefDetail {
                name: d.name,
                agent_type: d.agent_type,
                template: d.template,
                inline_prompt: d.inline_prompt,
                tags: d.tags,
                scope: d.scope,
                available_in_command_dialog: d.available_in_command_dialog,
                icon: d.icon,
                command: d.command,
            },
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("agent def '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) async fn handle_save_agent_def(
        &self,
        project_root: &str,
        name: &str,
        agent_type: &str,
        template: Option<String>,
        inline_prompt: Option<String>,
        tags: Vec<String>,
        scope: &str,
        available_in_command_dialog: bool,
        icon: Option<String>,
        command: Option<String>,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::agents_dir,
            paths::global_agents_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let def = pu_core::agent_def::AgentDef {
            name: name.to_string(),
            agent_type: agent_type.to_string(),
            template,
            inline_prompt,
            tags,
            scope: scope.to_string(),
            available_in_command_dialog,
            icon,
            command,
        };
        match tokio::task::spawn_blocking(move || pu_core::agent_def::save_agent_def(&dir, &def))
            .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save agent def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_delete_agent_def(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::agents_dir,
            paths::global_agents_dir,
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
        match tokio::task::spawn_blocking(move || pu_core::agent_def::delete_agent_def(&dir, &n))
            .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete agent def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }
}
