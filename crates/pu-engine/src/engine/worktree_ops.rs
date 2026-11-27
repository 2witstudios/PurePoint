use std::path::Path;

use indexmap::IndexMap;
use pu_core::config;
use pu_core::manifest;
use pu_core::paths;
use pu_core::protocol::Response;
use pu_core::types::{WorktreeEntry, WorktreeStatus};

use crate::git;

use super::Engine;

impl Engine {
    pub(super) async fn handle_create_worktree(
        &self,
        project_root: &str,
        name: Option<String>,
        base: Option<String>,
    ) -> Response {
        let root_path = Path::new(project_root);

        // Ensure initialized
        if !paths::manifest_path(root_path).exists() {
            return Response::Error {
                code: "NOT_INITIALIZED".into(),
                message: "not initialized — run `pu init` first".into(),
            };
        }

        // Load config for env_files
        let cfg = match config::load_config_strict(root_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    code: "CONFIG_ERROR".into(),
                    message: format!("failed to load config: {e}"),
                };
            }
        };

        // Resolve name
        let Some(raw) = name else {
            return Response::Error {
                code: "INVALID_ARGUMENT".into(),
                message: "worktree creation requires a name".into(),
            };
        };
        let worktree_name = pu_core::id::normalize_worktree_name(&raw);
        if worktree_name.is_empty() {
            return Response::Error {
                code: "INVALID_ARGUMENT".into(),
                message: "worktree creation requires a name".into(),
            };
        }

        let base_branch = match base {
            Some(b) => b,
            None => git::resolve_base_ref(root_path, "HEAD")
                .await
                .unwrap_or_else(|_| "HEAD".into()),
        };
        let wt_id = pu_core::id::worktree_id();
        let wt_path = paths::worktree_path(root_path, &wt_id);
        let branch = format!("pu/{worktree_name}");
        let rollback_branch = branch.clone();

        if let Err(e) = git::create_worktree(root_path, &wt_path, &branch, &base_branch).await {
            return Response::Error {
                code: "CREATE_WORKTREE_FAILED".into(),
                message: format!("failed to create worktree: {e}"),
            };
        }

        // Install git hooks for trigger gate enforcement
        if let Err(e) = git::install_hooks(&wt_path, root_path).await {
            tracing::warn!("failed to install git hooks in worktree: {e}");
        }

        // Copy env files into new worktree
        for env_file in &cfg.env_files {
            let src = root_path.join(env_file);
            let dst = wt_path.join(env_file);
            match tokio::fs::copy(&src, &dst).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !src.exists() => {}
                Err(e) => tracing::warn!("failed to copy {env_file} to worktree: {e}"),
            }
        }

        // Write manifest entry (worktree only, no agents)
        let cwd = wt_path.to_string_lossy().to_string();
        let wt_id_clone = wt_id.clone();
        // Off the runtime: on Windows the rename retries while the manifest
        // is locked (`pu_core::fs_retry`).
        let manifest_root = root_path.to_path_buf();
        let manifest_result = tokio::task::spawn_blocking(move || {
            manifest::update_manifest(&manifest_root, move |mut m| {
                m.worktrees
                    .entry(wt_id_clone.clone())
                    .or_insert_with(|| WorktreeEntry {
                        id: wt_id_clone,
                        name: worktree_name.clone(),
                        path: cwd,
                        branch,
                        base_branch: Some(base_branch.clone()),
                        status: WorktreeStatus::Active,
                        agents: IndexMap::new(),
                        created_at: chrono::Utc::now(),
                        merged_at: None,
                        error: None,
                    });
                m
            })
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e).into()));

        if let Err(e) = manifest_result {
            // Rollback: remove worktree + branch
            self.rollback_worktree(root_path, Some(&wt_id), Some(&rollback_branch))
                .await;
            return Response::Error {
                code: "CREATE_WORKTREE_FAILED".into(),
                message: format!("failed to update manifest: {e}"),
            };
        }

        self.notify_status_change(project_root).await;

        Response::CreateWorktreeResult { worktree_id: wt_id }
    }

    pub(super) async fn handle_delete_worktree(
        &self,
        project_root: &str,
        worktree_id: &str,
    ) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let wt = match m.worktrees.get(worktree_id) {
            Some(wt) => wt.clone(),
            None => {
                return Response::Error {
                    code: "WORKTREE_NOT_FOUND".into(),
                    message: format!("worktree {worktree_id} not found"),
                };
            }
        };

        // 1. Kill all agents in the worktree, and wait for them to be gone: on
        // Windows an exiting agent's open handles block deleting its files.
        let agent_ids: Vec<String> = wt.agents.keys().cloned().collect();
        let killed = self.kill_agents(&agent_ids).await;
        wait_for_agents_to_exit(&killed, AGENT_EXIT_WAIT).await;

        // 2. Remove git worktree directory
        let root_path = Path::new(project_root);
        let wt_path = paths::worktree_path(root_path, worktree_id);
        let mut directory_error: Option<String> = None;

        if let Err(e) = git::remove_worktree(root_path, &wt_path).await {
            tracing::warn!(worktree_id, "git worktree remove failed: {e}");
            if wt_path.exists() {
                // Fallback: force-remove the directory directly (waiting out
                // sharing locks; the error names a file still held open), then
                // prune git's stale admin metadata so the slot can be reused.
                let target = wt_path.clone();
                let removed =
                    tokio::task::spawn_blocking(move || pu_core::fs_retry::remove_dir_all(&target))
                        .await
                        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
                if let Err(fs_err) = removed {
                    tracing::error!(
                        worktree_id,
                        "worktree directory still present after fallback removal: {fs_err}"
                    );
                    directory_error = Some(format!(
                        "git worktree remove failed ({e}); fallback removal also failed: {fs_err}"
                    ));
                } else {
                    git::prune_worktrees(root_path).await.ok();
                }
            }
            // else: directory is actually gone — transient git warning, not a real problem.
        }

        // If the directory is still on disk, don't delete the branch or the manifest
        // entry — keep it visible and retryable via `pu status`/`pu clean` instead of
        // orphaning it with no way to find it again.
        if let Some(err) = directory_error {
            let wt_id = worktree_id.to_string();
            let pr = project_root.to_string();
            let err_for_manifest = err.clone();
            tokio::task::spawn_blocking(move || {
                manifest::update_manifest(Path::new(&pr), move |mut m| {
                    if let Some(entry) = m.worktrees.get_mut(&wt_id) {
                        entry.status = WorktreeStatus::Failed;
                        entry.error = Some(err_for_manifest);
                    }
                    m
                })
                .ok();
            })
            .await
            .ok();

            self.notify_status_change(project_root).await;

            return Response::DeleteWorktreeResult {
                worktree_id: worktree_id.to_string(),
                killed_agents: agent_ids,
                branch_deleted: false,
                remote_deleted: false,
                directory_removed: false,
                error: Some(err),
            };
        }

        // 3. Delete local branch (soft-fail)
        let branch = wt.branch.clone();
        let branch_deleted = git::delete_local_branch(root_path, &branch).await.is_ok();

        // 4. Delete remote branch (soft-fail)
        let remote_deleted = git::delete_remote_branch(root_path, &branch).await.is_ok();

        // 5. Remove worktree from manifest
        let wt_id = worktree_id.to_string();
        let killed_agents = agent_ids.clone();
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                m.worktrees.shift_remove(&wt_id);
                m
            })
            .ok();
        })
        .await
        .ok();

        self.notify_status_change(project_root).await;

        Response::DeleteWorktreeResult {
            worktree_id: worktree_id.to_string(),
            killed_agents,
            branch_deleted,
            remote_deleted,
            directory_removed: true,
            error: None,
        }
    }

    pub(super) async fn rollback_worktree(
        &self,
        root_path: &Path,
        worktree_id: Option<&str>,
        branch: Option<&str>,
    ) {
        if let Some(wt_id) = worktree_id {
            let wt_path = paths::worktree_path(root_path, wt_id);
            if let Err(e) = git::remove_worktree(root_path, &wt_path).await {
                tracing::warn!("rollback: failed to remove worktree directory: {e}");
            }
        }
        if let Some(b) = branch {
            git::delete_local_branch(root_path, b).await.ok();
        }
    }
}

/// How long worktree removal waits for each killed agent's process tree to
/// exit (REQ-WINAPP-044).
const AGENT_EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Wait until every killed agent's exit has been observed (its exit watch
/// holds a status), up to `limit` each. Returns at once for agents that
/// already exited, so a completed kill adds no delay.
async fn wait_for_agents_to_exit(
    killed: &[(String, crate::pty_manager::AgentHandle)],
    limit: std::time::Duration,
) {
    for (id, handle) in killed {
        let mut exit = handle.exit_rx.clone();
        let exited = async {
            while exit.borrow_and_update().is_none() {
                if exit.changed().await.is_err() {
                    break;
                }
            }
        };
        if tokio::time::timeout(limit, exited).await.is_err() {
            tracing::warn!(
                agent_id = id,
                "agent still exiting; removing its worktree anyway"
            );
        }
    }
}

#[cfg(all(test, windows))]
mod lock_tests {
    use super::*;
    use pu_core::protocol::{Request, Response};
    use std::io::{BufRead, Write};

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// PowerShell opens `path` with no sharing until a line arrives on stdin.
    fn hold_open(path: &Path) -> std::process::Child {
        let script = format!(
            "$f = [System.IO.File]::Open('{}', 'Open', 'Read', 'None'); \
             [Console]::Out.WriteLine('ready'); [Console]::Out.Flush(); \
             [void][Console]::In.ReadLine(); $f.Close()",
            path.display().to_string().replace('\'', "''")
        );
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "ready");
        child
    }

    /// WIN-3.13: a file in the worktree held open by another process makes
    /// deletion fail with an error naming that file, and keeps the worktree
    /// retryable; once the file is closed, the retried delete succeeds.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_file_held_open_in_worktree_should_name_it_then_delete_after_close() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["commit", "--allow-empty", "-qm", "fixture"]);
        let engine = Engine::with_project_registry(temp.path().join("registry.json")).unwrap();
        let root = project.to_string_lossy().into_owned();
        assert!(matches!(
            engine
                .handle_request(Request::Init {
                    project_root: root.clone()
                })
                .await,
            Response::InitResult { .. }
        ));
        // Created directly (not through CreateWorktree, whose path handling
        // is WIN-3.11's), exactly as the engine records it.
        let worktree_id = pu_core::id::worktree_id();
        let wt_path = paths::worktree_path(&project, &worktree_id);
        git(
            &project,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "pu/locked",
                wt_path.to_str().unwrap(),
            ],
        );
        let (id, cwd) = (worktree_id.clone(), wt_path.to_string_lossy().into_owned());
        manifest::update_manifest(&project, move |mut m| {
            m.worktrees.insert(
                id.clone(),
                WorktreeEntry {
                    id,
                    name: "locked".into(),
                    path: cwd,
                    branch: "pu/locked".into(),
                    base_branch: None,
                    status: WorktreeStatus::Active,
                    agents: IndexMap::new(),
                    created_at: chrono::Utc::now(),
                    merged_at: None,
                    error: None,
                },
            );
            m
        })
        .unwrap();
        let held = wt_path.join("held.log");
        std::fs::write(&held, b"busy").unwrap();
        let mut holder = hold_open(&held);

        let delete = Request::DeleteWorktree {
            project_root: root.clone(),
            worktree_id: worktree_id.clone(),
        };
        let Response::DeleteWorktreeResult {
            directory_removed,
            error,
            ..
        } = engine.handle_request(delete.clone()).await
        else {
            panic!("expected a delete result");
        };
        assert!(!directory_removed);
        let error = error.unwrap();
        // Name the file the way the engine does (canonical), so an 8.3 TEMP
        // spelling such as RUNNER~1 on CI still matches.
        let held_name = pu_core::canon::canonicalize(&held).unwrap_or_else(|_| held.clone());
        assert!(error.contains(&held_name.display().to_string()), "{error}");
        assert!(error.contains("locked"), "{error}");
        assert!(held.exists());

        writeln!(holder.stdin.take().unwrap()).unwrap();
        assert!(holder.wait().unwrap().success());
        let Response::DeleteWorktreeResult {
            directory_removed,
            error,
            ..
        } = engine.handle_request(delete).await
        else {
            panic!("expected a delete result");
        };
        assert!(directory_removed, "{error:?}");
        assert!(!wt_path.exists());
    }
}
