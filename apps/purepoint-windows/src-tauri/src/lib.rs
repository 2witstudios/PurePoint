//! Tauri backend for the PurePoint Windows app (DEC-7).

pub mod attach;
pub mod conversations;
pub mod daemon;
pub mod files;
pub mod git;
pub mod installer;
pub mod secrets;
pub mod subscriptions;
pub mod watchers;

#[cfg(windows)]
mod webview;

/// Label of the single main window declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

/// Whether this build hides the WebView2 browser affordances (accelerator
/// keys such as F5, Ctrl+R and Ctrl+P, the default context menu, and DevTools).
/// Debug builds keep them for development.
pub const LOCK_DOWN_WEBVIEW: bool = !cfg!(debug_assertions);

/// The daemon protocol version this app speaks, from `pu-core`.
pub fn protocol_version() -> u32 {
    pu_core::protocol::PROTOCOL_VERSION
}

pub fn run() {
    tauri::Builder::default()
        .manage(git::commands::GitState::default())
        .manage(attach::AttachState::default())
        .invoke_handler(tauri::generate_handler![
            files::index::file_index,
            files::text::fs_stat_text,
            files::text::fs_read_text,
            files::text::fs_write_text_if_unmodified,
            conversations::conversations_list,
            conversations::conversation_snippets,
            conversations::conversation_transcript,
            git::commands::git_branch_review,
            git::commands::git_local_review,
            git::commands::git_review_summary,
            git::commands::git_commit_diff,
            git::commands::git_untracked_preview,
            git::commands::git_review_page,
            git::commands::git_unstaged_diff,
            git::commands::git_working_tree_changes,
            git::commands::git_working_tree_file_diff,
            git::commands::git_pr_list,
            git::commands::git_pr_diff,
            git::commands::git_gh_available,
            daemon::daemon_request,
            daemon::daemon_health,
            daemon::daemon_status,
            installer::installer_status,
            installer::installer_run,
            subscriptions::subscriptions_start_project,
            subscriptions::subscriptions_stop_project,
            subscriptions::subscriptions_start_projects,
            subscriptions::subscriptions_stop_projects,
            subscriptions::subscriptions_grid_command,
            attach::attach_open,
            attach::attach_input,
            attach::attach_resize,
            attach::attach_claim_size,
            attach::attach_blur,
            attach::attach_close,
            watchers::commands::watch_manifest,
            watchers::commands::watch_worktree,
            watchers::commands::watch_file_tree,
            watchers::commands::watch_machines,
            watchers::commands::unwatch,
            watchers::commands::watchers_release,
            watchers::commands::watchers_restore,
            watchers::commands::watchers_discard,
        ])
        .setup(|app| {
            use tauri::Manager;
            {
                let daemon = daemon::DaemonState::for_app(app.handle());
                daemon.start();
                app.manage(daemon);
                app.manage(subscriptions::Subscriptions::for_app(app.handle().clone()));
                app.manage(watchers::commands::WatcherState::new(app.handle().clone()));
                // After the daemon state: a replaced pu-engine.exe restarts it.
                installer::start(app.handle());
            }
            #[cfg(windows)]
            {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    webview::configure(&window, LOCK_DOWN_WEBVIEW)?;
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running PurePoint");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_the_pu_core_protocol() {
        assert!(protocol_version() > 0);
    }
}
