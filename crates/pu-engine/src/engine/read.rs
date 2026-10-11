use std::path::Path;

use pu_core::protocol::Response;
use pu_core::read::{
    ReadError, ReadRecord, ReadReport, RecordKind, Runtime, SourceKind, Transcript, paginate,
};

use crate::read_source::{
    SourceError, commit_binding, discover_codex, load_binding, read_jsonl, read_opencode,
    resolve_exact,
};

use super::Engine;

impl Engine {
    pub(super) async fn watch_codex_binding(
        &self,
        project_root: String,
        agent_id: String,
        cwd: String,
    ) {
        let sessions = self.sessions.clone();
        let pid = match sessions.lock().await.get(&agent_id) {
            Some(h) => h.pid,
            None => return,
        };
        tokio::spawn(async move {
            let mut captured = None;
            loop {
                let owned = sessions
                    .lock()
                    .await
                    .get(&agent_id)
                    .is_some_and(|h| h.pid == pid);
                if !owned && captured.is_none() {
                    tracing::warn!(
                        agent_id,
                        pid,
                        "Codex exited before exact identity capture; pu read requires --session ID"
                    );
                    break;
                }
                if captured.is_none() {
                    match discover_codex(pid, &cwd).await {
                        Ok(binding) => captured = Some(binding),
                        Err(SourceError::Ambiguous) => {
                            tracing::warn!(
                                agent_id,
                                "ambiguous Codex session binding; exact ID required"
                            );
                            break;
                        }
                        Err(_) => {}
                    }
                }
                if let Some(binding) = &captured {
                    let root = project_root.clone();
                    let agent = agent_id.clone();
                    let binding = binding.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        commit_binding(Path::new(&root), &agent, Some(pid), None, &binding)
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => break,
                        // Manifest publication can lag the first owned rollout descriptor.
                        Ok(Err(SourceError::Io(_))) if owned => {}
                        _ => {
                            tracing::warn!(
                                agent_id,
                                "native binding was not committed; ownership changed or persistence failed"
                            );
                            break;
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });
    }

    pub(super) async fn handle_read(
        &self,
        project_root: &str,
        agent_id: &str,
        limit: usize,
        since: Option<&str>,
        requested_session: Option<&str>,
    ) -> Response {
        if !(1..=200).contains(&limit) {
            return read_error(ReadError::InvalidLimit.into());
        }
        let live = {
            let sessions = self.sessions.lock().await;
            sessions.get(agent_id).map(|h| {
                (
                    h.agent_type.clone(),
                    h.cwd.clone(),
                    h.pid,
                    h.started_at,
                    h.output_buffer.clone(),
                )
            })
        };
        let (agent_type, cwd, session_id, terminal_command, expected_pid) =
            if project_root.is_empty() {
                match &live {
                    Some((kind, cwd, ..)) => (
                        kind.clone(),
                        cwd.clone(),
                        None,
                        false,
                        live.as_ref().map(|x| x.2),
                    ),
                    None => return Self::agent_not_found(agent_id),
                }
            } else {
                let manifest = match self.read_manifest_async(project_root).await {
                    Ok(m) => m,
                    Err(e) => return Self::error_response(&e),
                };
                match manifest.find_agent(agent_id) {
                    Some(pu_core::types::AgentLocation::Root(agent)) => (
                        agent.agent_type.clone(),
                        project_root.to_owned(),
                        agent.session_id.clone(),
                        agent.command.is_some(),
                        agent.pid,
                    ),
                    Some(pu_core::types::AgentLocation::Worktree { worktree, agent }) => (
                        agent.agent_type.clone(),
                        worktree.path.clone(),
                        agent.session_id.clone(),
                        agent.command.is_some(),
                        agent.pid,
                    ),
                    None => return Self::agent_not_found(agent_id),
                }
            };
        let runtime = match agent_type.as_str() {
            _ if terminal_command => Runtime::Terminal,
            "claude" => Runtime::Claude,
            "codex" => Runtime::Codex,
            "opencode" => Runtime::Opencode,
            "pi" => Runtime::Pi,
            "terminal" => Runtime::Terminal,
            _ => {
                return Response::Error {
                    code: "READ_UNSUPPORTED_RUNTIME".into(),
                    message: format!("agent type '{agent_type}' has no transcript adapter"),
                };
            }
        };
        if runtime == Runtime::Terminal {
            if requested_session.is_some() {
                return read_error(SourceError::IdentityMismatch);
            }
            let Some((_, _, pid, started, buffer)) = live else {
                return Response::Error {
                    code: "READ_SOURCE_UNAVAILABLE".into(),
                    message: "terminal capture is unavailable after its process has been reaped"
                        .into(),
                };
            };
            let (text, generation) = buffer.terminal_snapshot();
            let session = format!("terminal:{pid}:{started}");
            let source = format!("{agent_id}:{session}:{generation}");
            let transcript = Transcript {
                records: text
                    .lines()
                    .enumerate()
                    .map(|(i, line)| ReadRecord {
                        id: format!("row:{i}"),
                        kind: RecordKind::Terminal,
                        text: line.to_owned(),
                        tool_name: None,
                        tool_call_id: None,
                        is_error: None,
                    })
                    .collect(),
                pending_tail: false,
            };
            return match paginate(&source, &transcript, since, limit) {
                Ok(page) => Response::ReadResult(ReadReport {
                    agent_id: agent_id.into(),
                    runtime,
                    session_id: session,
                    source_kind: SourceKind::TerminalText,
                    page,
                }),
                Err(e) => read_error(e.into()),
            };
        }
        if requested_session
            .is_some_and(|id| session_id.as_deref().is_some_and(|saved| id != saved))
        {
            return read_error(SourceError::IdentityMismatch);
        }
        let env = self.agent_env().await;
        let root = project_root.to_owned();
        let agent = agent_id.to_owned();
        let known = requested_session.map(str::to_owned).or(session_id.clone());
        let cwd_expected = cwd.clone();
        let loaded =
            tokio::task::spawn_blocking(move || load_binding(Path::new(&root), &agent)).await;
        let needs_bind = session_id.is_none() || !matches!(&loaded, Ok(Ok(_)));
        let binding = match loaded {
            Ok(Ok(binding)) => binding,
            Ok(Err(SourceError::Io(e))) if e.kind() == std::io::ErrorKind::NotFound => {
                let resolved = if let Some(id) = known.as_deref() {
                    let id = id.to_owned();
                    let cwd = cwd.clone();
                    tokio::task::spawn_blocking(move || resolve_exact(runtime, &id, &cwd, &env))
                        .await
                        .unwrap_or_else(|e| Err(SourceError::Io(std::io::Error::other(e))))
                } else if runtime == Runtime::Codex {
                    match &live {
                        Some((_, _, pid, ..)) => discover_codex(*pid, &cwd).await,
                        None => Err(SourceError::IdentityUnavailable),
                    }
                } else {
                    Err(SourceError::IdentityUnavailable)
                };
                match resolved {
                    Ok(binding) => binding,
                    Err(e) => return read_error(e),
                }
            }
            Ok(Err(e)) => return read_error(e),
            Err(e) => {
                return Response::Error {
                    code: "READ_SOURCE_UNAVAILABLE".into(),
                    message: e.to_string(),
                };
            }
        };
        if binding.runtime != runtime
            || binding.cwd != cwd_expected
            || known.as_ref().is_some_and(|id| id != &binding.session_id)
        {
            return read_error(SourceError::IdentityMismatch);
        }
        let persist_binding = binding.clone();
        let expected_session = session_id.clone();
        let source = format!(
            "{project_root}:{agent_id}:{runtime:?}:{}:{}",
            binding.session_id,
            binding.path.display()
        );
        let session_id = binding.session_id.clone();
        let transcript = tokio::task::spawn_blocking(move || match runtime {
            Runtime::Opencode => read_opencode(&binding),
            _ => read_jsonl(&binding),
        })
        .await;
        match transcript {
            Ok(Ok(transcript)) => {
                if needs_bind && !project_root.is_empty() {
                    let root = project_root.to_owned();
                    let agent = agent_id.to_owned();
                    let result = tokio::task::spawn_blocking(move || {
                        commit_binding(
                            Path::new(&root),
                            &agent,
                            expected_pid,
                            expected_session.as_deref(),
                            &persist_binding,
                        )
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => return read_error(e),
                        Err(e) => return read_error(SourceError::Io(std::io::Error::other(e))),
                    }
                }
                match paginate(&source, &transcript, since, limit) {
                    Ok(page) => Response::ReadResult(ReadReport {
                        agent_id: agent_id.into(),
                        runtime,
                        session_id,
                        source_kind: SourceKind::Transcript,
                        page,
                    }),
                    Err(e) => read_error(e.into()),
                }
            }
            Ok(Err(e)) => read_error(e),
            Err(e) => Response::Error {
                code: "READ_SOURCE_UNAVAILABLE".into(),
                message: e.to_string(),
            },
        }
    }
}

fn read_error(error: SourceError) -> Response {
    let code = match &error {
        SourceError::IdentityUnavailable => "READ_IDENTITY_UNAVAILABLE",
        SourceError::Ambiguous => "READ_AMBIGUOUS_SOURCE",
        SourceError::IdentityMismatch => "READ_IDENTITY_MISMATCH",
        SourceError::UnsupportedSchema(_) => "READ_UNSUPPORTED_SCHEMA",
        SourceError::Io(_) | SourceError::Database(_) => "READ_SOURCE_UNAVAILABLE",
        SourceError::Parse(ReadError::InvalidCursor) => "READ_INVALID_CURSOR",
        SourceError::Parse(ReadError::StaleCursor) => "READ_STALE_CURSOR",
        SourceError::Parse(ReadError::InvalidLimit) => "INVALID_ARGUMENT",
        SourceError::Parse(ReadError::TooLarge) => "READ_SOURCE_TOO_LARGE",
        SourceError::Parse(_) => "READ_INVALID_TRANSCRIPT",
    };
    Response::Error {
        code: code.into(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::Engine;
    use crate::read_source::Binding;
    use pu_core::protocol::{Request, Response};

    #[tokio::test]
    async fn given_suspended_agent_should_read_its_bound_transcript_without_a_live_terminal() {
        // given
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let engine = Engine::new();
        engine
            .handle_request(Request::Init {
                project_root: root.clone(),
            })
            .await;
        pu_core::manifest::update_manifest(temp.path(), |mut m| {
            let agent = serde_json::from_value(serde_json::json!({"id":"ag-read","name":"read","agentType":"claude","status":"running","startedAt":"2026-10-10T00:00:00Z","sessionId":"session1","suspended":true})).unwrap();
            m.agents.insert("ag-read".into(), agent);
            m
        }).unwrap();
        let source = temp.path().join("session1.jsonl");
        std::fs::write(&source, "{\"type\":\"assistant\",\"uuid\":\"a1\",\"sessionId\":\"session1\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Persisted result\"}]}}\n").unwrap();
        let dir = temp.path().join(".pu/read-bindings");
        std::fs::create_dir_all(&dir).unwrap();
        let binding = Binding {
            version: 1,
            runtime: pu_core::read::Runtime::Claude,
            session_id: "session1".into(),
            path: source,
            cwd: root.clone(),
        };
        std::fs::write(
            dir.join("ag-read.json"),
            serde_json::to_vec(&binding).unwrap(),
        )
        .unwrap();
        // when
        let response = engine
            .handle_request(Request::Read {
                project_root: root,
                agent_id: "ag-read".into(),
                limit: 50,
                since: None,
                session_id: None,
            })
            .await;
        // then
        match response {
            Response::ReadResult(report) => {
                assert_eq!(report.page.records[0].text, "Persisted result")
            }
            other => panic!("expected read result, got {other:?}"),
        }
    }
}
