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
        .invoke_handler(tauri::generate_handler![
            files::index::file_index,
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
        ])
        .setup(|app| {
            {
                use tauri::Manager;
                let daemon = daemon::DaemonState::for_app(app.handle());
                daemon.start();
                app.manage(daemon);
            }
            #[cfg(windows)]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    webview::configure(&window, LOCK_DOWN_WEBVIEW)?;
                }
            }
            #[cfg(not(windows))]
            let _ = app;
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
