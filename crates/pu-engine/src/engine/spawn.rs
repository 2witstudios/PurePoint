use std::path::Path;
use std::time::Duration;

use indexmap::IndexMap;
use pu_core::config;
use pu_core::manifest;
use pu_core::paths;
use pu_core::protocol::Response;
use pu_core::types::{AgentEntry, AgentStatus, WorktreeEntry, WorktreeStatus};

use super::SpawnParams;
use super::session_repair::inject_initial_prompt;
use crate::git;
use crate::pty_manager::{NativePtyHost, SpawnConfig};

use super::Engine;

impl Engine {
    pub(super) async fn handle_spawn_shell(&self, cwd: &str) -> Response {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let agent_id = pu_core::id::agent_id();

        let env = self.agent_env().await;
        let spawn_config = SpawnConfig {
            command: shell,
            args: vec!["-l".to_string()],
            cwd: cwd.to_string(),
            env,
            env_remove: vec![],
            cols: 120,
            rows: 40,
        };

        let handle = match self.pty_host.spawn(spawn_config).await {
            Ok(h) => h,
            Err(e) => {
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to spawn shell: {e}"),
                };
            }
        };

        // Start exit monitor (cleans up session map when shell exits)
        let exit_rx = handle.exit_rx.clone();
        let sessions = self.sessions.clone();
        let aid = agent_id.clone();
        tokio::spawn(async move {
            let mut rx = exit_rx;
            while rx.changed().await.is_ok() {
                if rx.borrow().is_some() {
                    break;
                }
            }
            sessions.lock().await.remove(&aid);
        });

        self.sessions.lock().await.insert(agent_id.clone(), handle);

        Response::SpawnResult {
            worktree_id: None,
            agent_id,
            status: AgentStatus::Running,
        }
    }

    pub(super) async fn handle_spawn(&self, params: SpawnParams) -> Response {
        let SpawnParams {
            project_root,
            prompt,
            agent_type,
            name,
            base,
            root,
            worktree,
            terminal_command,
            no_auto,
            extra_args,
            plan_mode,
            no_trigger,
            trigger: trigger_param,
        } = params;
        let root_path = Path::new(&project_root);

        // Ensure initialized
        if !paths::manifest_path(root_path).exists() {
            return Response::Error {
                code: "NOT_INITIALIZED".into(),
                message: "not initialized — run `pu init` first".into(),
            };
        }

        // Resolve agent config (strict: surface YAML parse errors)
        let cfg = match config::load_config_strict(root_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    code: "CONFIG_ERROR".into(),
                    message: format!("failed to load config: {e}"),
                };
            }
        };
        let agent_cfg = match config::resolve_agent(&cfg, &agent_type) {
            Some(c) => c.clone(),
            None => {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: format!("unknown agent type: {agent_type}"),
                };
            }
        };

        let agent_id = pu_core::id::agent_id();
        let creating_new_worktree = !root && worktree.is_none();
        let agent_name = if creating_new_worktree {
            // Worktree spawns require a user-provided name (becomes the branch slug)
            let Some(raw) = name else {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: "worktree spawn requires a name".into(),
                };
            };
            let normalized = pu_core::id::normalize_worktree_name(&raw);
            if normalized.is_empty() {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: "worktree spawn requires a name".into(),
                };
            }
            normalized
        } else {
            // Root agents and existing-worktree agents get auto-generated names
            name.unwrap_or_else(pu_core::id::root_agent_name)
        };
        let base_branch = match base {
            Some(b) => b,
            None => git::resolve_base_ref(root_path, "HEAD")
                .await
                .unwrap_or_else(|_| "HEAD".into()),
        };

        // Normalize empty command to None
        let terminal_command = terminal_command.filter(|c| !c.is_empty());

        // Plan mode requires a prompt-driven agent that understands EnterPlanMode.
        // Reject early for terminal agents or terminal_command spawns where the
        // prefix would be meaningless or actively harmful.
        if plan_mode
            && (prompt.is_empty() || terminal_command.is_some() || agent_type == "terminal")
        {
            return Response::Error {
                code: "INVALID_ARGUMENT".into(),
                message: "plan mode requires a prompt-driven non-terminal agent".into(),
            };
        }

        // When plan_mode is active, prefix the prompt with instructions to enter plan mode.
        // This keeps bypass permissions as the base while guiding the agent into plan mode
        // via its own tool (EnterPlanMode) rather than conflicting CLI flags.
        let prompt = if plan_mode {
            format!(
                "[PLAN MODE] You MUST call the EnterPlanMode tool immediately before doing anything else. \
                 Do not read files, do not explore — call EnterPlanMode first. \
                 Once in plan mode, research and plan before making changes.\n\n{prompt}"
            )
        } else {
            prompt.to_string()
        };
        let prompt = &prompt;

        // When a terminal command is set, it becomes the PTY process directly
        let (command, args, session_id, inject_prompt_via_stdin) = if let Some(ref cmd) =
            terminal_command
        {
            let has_metacharacters = cmd.contains('|')
                || cmd.contains("&&")
                || cmd.contains(';')
                || cmd.contains('>')
                || cmd.contains('<')
                || cmd.contains('$');

            let (cmd_bin, cmd_args) = if has_metacharacters {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                (shell, vec!["-c".to_string(), cmd.clone()])
            } else {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if parts.is_empty() {
                    // Shouldn't happen after filter, but handle gracefully
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                    (shell, vec![])
                } else {
                    (
                        parts[0].to_string(),
                        parts[1..].iter().map(ToString::to_string).collect(),
                    )
                }
            };
            (cmd_bin, cmd_args, None, false)
        } else {
            // Standard agent flow
            let (command, cmd_args) = match Self::parse_agent_command(&agent_cfg, &agent_type) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let mut args = cmd_args;

            // Add agent-type-specific launch args from config (or defaults).
            // --no-auto skips only the built-in defaults; explicit user-configured
            // launchArgs are always applied.
            let launch_args = if no_auto && agent_cfg.launch_args.is_none() {
                Vec::new()
            } else {
                pu_core::types::resolved_launch_args(&agent_type, agent_cfg.launch_args.as_deref())
            };
            if launch_args.is_empty() && agent_cfg.launch_args.is_some() {
                tracing::info!(agent_type, "auto-mode disabled via config (launchArgs: [])");
            }
            for arg in launch_args.into_iter().rev() {
                if !args.iter().any(|a| a == &arg) {
                    args.insert(0, arg);
                }
            }

            // Append extra args from --agent-args (always applied, even with --no-auto)
            args.extend(extra_args.iter().cloned());

            // Generate session ID for claude agents (enables resume via --resume)
            let session_id = if agent_type == "claude" {
                let id = pu_core::id::session_id();
                args.push("--session-id".into());
                args.push(id.clone());
                Some(id)
            } else {
                None
            };

            // Claude prompt via argv can stall first render in some terminals; keep stdin injection
            // for Claude (and terminal agent). Codex/OpenCode accept startup prompts via CLI args.
            let inject_prompt_via_stdin =
                Self::should_inject_prompt_via_stdin(&agent_type, agent_cfg.interactive, prompt);
            if !inject_prompt_via_stdin && !prompt.is_empty() {
                let prompt_flag =
                    Self::resolved_prompt_flag(&agent_type, agent_cfg.prompt_flag.as_deref());
                if let Some(flag) = prompt_flag {
                    args.push(flag);
                    args.push(prompt.to_string());
                } else {
                    // Default prompt style is positional (for example codex [PROMPT]).
                    args.push(prompt.to_string());
                }
            }

            (command, args, session_id, inject_prompt_via_stdin)
        };

        // Determine working directory
        let (cwd, worktree_id) = if root || worktree.is_some() {
            // Spawn in project root or existing worktree
            let wt_id = worktree.clone();
            let dir = if let Some(ref wt) = worktree {
                paths::worktree_path(root_path, wt)
                    .to_string_lossy()
                    .to_string()
            } else {
                project_root.to_string()
            };
            (dir, wt_id)
        } else {
            // Create new worktree
            let wt_id = pu_core::id::worktree_id();
            let wt_path = paths::worktree_path(root_path, &wt_id);
            let branch = format!("pu/{agent_name}");

            if let Err(e) = git::create_worktree(root_path, &wt_path, &branch, &base_branch).await {
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to create worktree: {e}"),
                };
            }

            // Install git hooks for trigger gate enforcement
            if let Err(e) = git::install_hooks(&wt_path, root_path).await {
                tracing::warn!("failed to install git hooks in worktree: {e}");
            }

            // Copy env files (e.g., .env, .env.local) into new worktree
            for env_file in &cfg.env_files {
                let src = root_path.join(env_file);
                let dst = wt_path.join(env_file);
                match tokio::fs::copy(&src, &dst).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && !src.exists() => {} // source doesn't exist, skip
                    Err(e) => tracing::warn!("failed to copy {env_file} to worktree: {e}"),
                }
            }

            (wt_path.to_string_lossy().to_string(), Some(wt_id))
        };

        // Spawn PTY process
        let mut env = self.agent_env().await;
        env.push(("PU_AGENT_ID".into(), agent_id.clone()));
        env.push(("PU_PROJECT_ROOT".into(), project_root.to_string()));
        let spawn_config = SpawnConfig {
            command,
            args,
            cwd: cwd.clone(),
            env,
            env_remove: vec!["CLAUDECODE".into()],
            cols: 120,
            rows: 40,
        };

        // Track whether we created a new worktree (for rollback on failure)
        let created_worktree = !root && worktree.is_none() && worktree_id.is_some();
        let rollback_branch = if created_worktree {
            Some(format!("pu/{agent_name}"))
        } else {
            None
        };

        let handle = match self.pty_host.spawn(spawn_config).await {
            Ok(h) => h,
            Err(e) => {
                if created_worktree {
                    self.rollback_worktree(
                        root_path,
                        worktree_id.as_deref(),
                        rollback_branch.as_deref(),
                    )
                    .await;
                }
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to spawn process: {e}"),
                };
            }
        };

        if inject_prompt_via_stdin {
            let prompt_bytes = prompt.as_bytes().to_vec();
            let pending = self.pending_initial_inputs.clone();
            pending
                .lock()
                .await
                .insert(agent_id.clone(), prompt_bytes.clone());

            let output_buffer = handle.output_buffer.clone();
            let master_fd = handle.master_fd();
            let mut exit_rx = handle.exit_rx.clone();
            let pty_host = NativePtyHost::new();
            let aid = agent_id.clone();

            tokio::spawn(async move {
                let mut watcher = output_buffer.subscribe();
                let timeout = tokio::time::sleep(Duration::from_millis(1800));
                tokio::pin!(timeout);

                loop {
                    tokio::select! {
                        _ = &mut timeout => {
                            // Fallback or quiet-period expired — inject now
                            break;
                        }
                        Ok(()) = exit_rx.changed() => {
                            // Process exited before we could inject — abort
                            pending.lock().await.remove(&aid);
                            tracing::debug!(agent_id = %aid, "prompt injection aborted: process exited");
                            return;
                        }
                        Ok(()) = watcher.changed() => {
                            // Got output — reset to a 450ms quiet period
                            timeout
                                .as_mut()
                                .reset(tokio::time::Instant::now() + Duration::from_millis(450));
                        }
                    }
                }

                // Inject the prompt
                if inject_initial_prompt(&pty_host, &master_fd, &aid, &prompt_bytes).await {
                    tracing::debug!(agent_id = %aid, "prompt injected at spawn time");
                } else {
                    tracing::warn!(agent_id = %aid, "failed to inject prompt at spawn time");
                }
                pending.lock().await.remove(&aid);
            });
        }

        let pid = handle.pid;

        // Store handle in session map BEFORE writing manifest.
        // ManifestWatcher in Swift fires on manifest write and immediately
        // tries to attach — the session must already be in the map.
        self.sessions.lock().await.insert(agent_id.clone(), handle);

        // Bind trigger if explicitly specified via --trigger <name>
        let (trigger_name, trigger_total) = if no_trigger {
            (None, None)
        } else if let Some(ref name) = trigger_param {
            let pr = project_root.to_string();
            let name_clone = name.clone();
            let found = tokio::task::spawn_blocking(move || {
                let triggers = pu_core::trigger_def::triggers_for_event(
                    Path::new(&pr),
                    &pu_core::trigger_def::TriggerEvent::AgentIdle,
                );
                triggers
                    .into_iter()
                    .find(|t| t.name == name_clone)
                    .map(|t| {
                        let len = t.sequence.len() as u32;
                        (t.name, len)
                    })
            })
            .await
            .unwrap_or(None);
            match found {
                Some((tname, total)) if total > 0 => (Some(tname), Some(total)),
                Some(_) => {
                    return Response::Error {
                        code: "INVALID_TRIGGER".into(),
                        message: format!("trigger '{name}' has empty sequence"),
                    };
                }
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("trigger '{name}' not found"),
                    };
                }
            }
        } else {
            (None, None)
        };

        // Update manifest
        let agent_entry = AgentEntry {
            id: agent_id.clone(),
            name: agent_name.clone(),
            agent_type,
            status: AgentStatus::Running,
            prompt: Some(prompt.to_string()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(pid),
            session_id,
            suspended_at: None,
            suspended: false,
            command: terminal_command,
            plan_mode,
            trigger_seq_index: trigger_name.as_ref().map(|_| 0),
            trigger_state: trigger_name
                .as_ref()
                .map(|_| pu_core::types::TriggerState::Active),
            trigger_total,
            gate_attempts: trigger_name.as_ref().map(|_| 0),
            no_trigger,
            trigger_name: trigger_name.clone(),
        };

        let wt_id_for_manifest = worktree_id.clone();
        let agent_id_clone = agent_id.clone();
        let manifest_result = manifest::update_manifest(root_path, move |mut m| {
            if let Some(ref wt_id) = wt_id_for_manifest {
                // Add or update worktree entry
                let wt_entry = m
                    .worktrees
                    .entry(wt_id.clone())
                    .or_insert_with(|| WorktreeEntry {
                        id: wt_id.clone(),
                        name: agent_name.clone(),
                        path: cwd.clone(),
                        branch: format!("pu/{agent_name}"),
                        base_branch: Some(base_branch.clone()),
                        status: WorktreeStatus::Active,
                        agents: IndexMap::new(),
                        created_at: chrono::Utc::now(),
                        merged_at: None,
                        error: None,
                    });
                wt_entry.agents.insert(agent_id_clone, agent_entry);
            } else {
                m.agents.insert(agent_id_clone, agent_entry);
            }
            m
        });

        if let Err(e) = manifest_result {
            // Rollback: remove session and kill process
            if let Some(handle) = self.sessions.lock().await.remove(&agent_id) {
                self.pty_host
                    .kill(&handle, Duration::from_secs(2))
                    .await
                    .ok();
            }
            if created_worktree {
                self.rollback_worktree(
                    root_path,
                    worktree_id.as_deref(),
                    rollback_branch.as_deref(),
                )
                .await;
            }
            return Response::Error {
                code: "SPAWN_FAILED".into(),
                message: format!("failed to update manifest: {e}"),
            };
        }

        self.notify_status_change(&project_root).await;

        Response::SpawnResult {
            worktree_id,
            agent_id,
            status: AgentStatus::Running,
        }
    }
}
