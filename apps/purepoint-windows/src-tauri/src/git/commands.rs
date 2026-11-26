//! Tauri commands over [`GitService`] for the review views (front end:
//! `ui/src/services/git.ts`).
//!
//! Each host has one service, so its caches and patch snapshots outlive single calls (a
//! review page reads the snapshot an earlier call created). Calls run on the blocking pool.
//! `machine_id` picks a remote machine's host, whose git runs through that machine's daemon
//! (`git_exec` over `ssh.exe … pu daemon proxy`, WIN-5.3).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::State;

use super::host::{DaemonProjectHost, LocalProjectHost, ProjectHost};
use super::model::{
    DiffData, FileDiff, GitBranchReview, GitError, GitLocalReview, GitResult, GitReviewSummary,
    PullRequestInfo, ReviewPage, ReviewPageCursor,
};
use super::service::GitService;
use crate::daemon::DaemonGitExec;

type Shared = Arc<Mutex<GitService>>;

/// The git services, one per host.
#[derive(Default)]
pub struct GitState {
    services: Mutex<HashMap<String, Shared>>,
}

impl GitState {
    fn service(&self, machine_id: Option<&str>) -> Shared {
        let key = machine_id.map_or_else(
            || "local".to_string(),
            |id| format!("machine:{}", id.to_lowercase()),
        );
        let mut services = self
            .services
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        services
            .entry(key)
            .or_insert_with(|| {
                let host: Arc<dyn ProjectHost> = match machine_id {
                    None => Arc::new(LocalProjectHost),
                    Some(id) => {
                        Arc::new(DaemonProjectHost::new(DaemonGitExec::for_machine(id), id))
                    }
                };
                Arc::new(Mutex::new(GitService::new(None, host)))
            })
            .clone()
    }
}

async fn with_service<T: Send + 'static>(
    state: State<'_, GitState>,
    machine_id: Option<String>,
    work: impl FnOnce(&mut GitService) -> GitResult<T> + Send + 'static,
) -> GitResult<T> {
    let service = state.service(machine_id.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        work(&mut service)
    })
    .await
    .map_err(|error| GitError::Io(error.to_string()))?
}

#[tauri::command]
pub async fn git_branch_review(
    state: State<'_, GitState>,
    root: String,
    base_branch: Option<String>,
    summaries_only: bool,
    machine_id: Option<String>,
) -> GitResult<GitBranchReview> {
    with_service(state, machine_id, move |git| {
        Ok(git.fetch_branch_review(&root, base_branch.as_deref(), summaries_only))
    })
    .await
}

#[tauri::command]
pub async fn git_local_review(
    state: State<'_, GitState>,
    root: String,
    summaries_only: bool,
    machine_id: Option<String>,
) -> GitResult<GitLocalReview> {
    with_service(state, machine_id, move |git| {
        Ok(git.fetch_local_review(&root, summaries_only))
    })
    .await
}

#[tauri::command]
pub async fn git_review_summary(
    state: State<'_, GitState>,
    root: String,
    base_branch: Option<String>,
    machine_id: Option<String>,
) -> GitResult<GitReviewSummary> {
    with_service(state, machine_id, move |git| {
        Ok(git.review_summary(&root, base_branch.as_deref()))
    })
    .await
}

#[tauri::command]
pub async fn git_commit_diff(
    state: State<'_, GitState>,
    root: String,
    sha: String,
    summaries_only: bool,
    machine_id: Option<String>,
) -> GitResult<Vec<FileDiff>> {
    with_service(state, machine_id, move |git| {
        git.fetch_commit_diff(&root, &sha, summaries_only)
    })
    .await
}

#[tauri::command]
pub async fn git_untracked_preview(
    state: State<'_, GitState>,
    root: String,
    name: String,
    machine_id: Option<String>,
) -> GitResult<FileDiff> {
    with_service(state, machine_id, move |git| {
        Ok(git.untracked_preview(&name, &root, None))
    })
    .await
}

#[tauri::command]
pub async fn git_review_page(
    state: State<'_, GitState>,
    root: String,
    file: FileDiff,
    cursor: Option<ReviewPageCursor>,
    machine_id: Option<String>,
) -> GitResult<ReviewPage> {
    with_service(state, machine_id, move |git| {
        git.fetch_review_page(&file, &root, cursor.as_ref())
    })
    .await
}

#[tauri::command]
pub async fn git_unstaged_diff(
    state: State<'_, GitState>,
    root: String,
    machine_id: Option<String>,
) -> GitResult<DiffData> {
    with_service(state, machine_id, move |git| {
        Ok(git.fetch_unstaged_diff(&root))
    })
    .await
}

#[tauri::command]
pub async fn git_working_tree_changes(
    state: State<'_, GitState>,
    root: String,
    machine_id: Option<String>,
) -> GitResult<Vec<FileDiff>> {
    with_service(state, machine_id, move |git| {
        git.fetch_working_tree_changes(&root)
    })
    .await
}

#[tauri::command]
pub async fn git_working_tree_file_diff(
    state: State<'_, GitState>,
    root: String,
    file: FileDiff,
    machine_id: Option<String>,
) -> GitResult<FileDiff> {
    with_service(state, machine_id, move |git| {
        git.fetch_working_tree_file_diff(&root, &file)
    })
    .await
}

#[tauri::command]
pub async fn git_pr_list(
    state: State<'_, GitState>,
    cwd: String,
    branch: Option<String>,
    machine_id: Option<String>,
) -> GitResult<Vec<PullRequestInfo>> {
    with_service(state, machine_id, move |git| {
        git.fetch_pr_list_checked(&cwd, branch.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn git_pr_diff(
    state: State<'_, GitState>,
    cwd: String,
    pr_number: i64,
    summaries_only: bool,
    machine_id: Option<String>,
) -> GitResult<DiffData> {
    with_service(state, machine_id, move |git| {
        git.fetch_pr_diff_checked(&cwd, pr_number, summaries_only)
    })
    .await
}

#[tauri::command]
pub async fn git_gh_available(
    state: State<'_, GitState>,
    cwd: String,
    machine_id: Option<String>,
) -> GitResult<bool> {
    with_service(state, machine_id, move |git| Ok(git.is_gh_available(&cwd))).await
}
