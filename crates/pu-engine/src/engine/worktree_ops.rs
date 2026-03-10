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
        let manifest_result = manifest::update_manifest(root_path, move |mut m| {
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
                });
            m
        });

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

        // 1. Kill all agents in the worktree
        let agent_ids: Vec<String> = wt.agents.keys().cloned().collect();
        self.kill_agents(&agent_ids).await;

        // 2. Remove git worktree directory
        let root_path = Path::new(project_root);
        let wt_path = paths::worktree_path(root_path, worktree_id);
        git::remove_worktree(root_path, &wt_path).await.ok();

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
            git::remove_worktree(root_path, &wt_path).await.ok();
        }
        if let Some(b) = branch {
            git::delete_local_branch(root_path, b).await.ok();
        }
    }
}
