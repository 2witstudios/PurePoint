use std::path::Path;

use pu_core::paths;
use pu_core::protocol::{Response, TemplateInfo};

use super::super::Engine;

impl Engine {
    pub(in crate::engine) async fn handle_list_templates(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let templates = pu_core::template::list_templates(root);
            let infos: Vec<TemplateInfo> = templates
                .into_iter()
                .map(|t| TemplateInfo {
                    name: t.name,
                    description: t.description,
                    agent: t.agent,
                    source: t.source,
                    variables: pu_core::template::extract_variables(&t.body),
                    command: t.command,
                })
                .collect();
            infos
        })
        .await
        {
            Ok(templates) => Response::TemplateList { templates },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_get_template(
        &self,
        project_root: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let tpl_name = name.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            pu_core::template::find_template(root, &tpl_name)
        })
        .await
        {
            Ok(Some(t)) => Response::TemplateDetail {
                name: t.name,
                description: t.description,
                agent: t.agent,
                variables: pu_core::template::extract_variables(&t.body),
                body: t.body,
                source: t.source,
                command: t.command,
            },
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("template '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) async fn handle_save_template(
        &self,
        project_root: &str,
        name: &str,
        description: &str,
        agent: &str,
        body: &str,
        scope: &str,
        command: Option<String>,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::templates_dir,
            paths::global_templates_dir,
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
        let d = description.to_string();
        let a = agent.to_string();
        let b = body.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::template::save_template_with_command(&dir, &n, &d, &a, &b, command.as_deref())
        })
        .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save template: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    pub(in crate::engine) async fn handle_delete_template(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::templates_dir,
            paths::global_templates_dir,
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
        match tokio::task::spawn_blocking(move || pu_core::template::delete_template(&dir, &n))
            .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete template: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }
}
