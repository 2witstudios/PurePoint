//! Tauri backend for the PurePoint Windows app (DEC-7).

pub mod attach;
pub mod chrome;
pub mod conversations;
pub mod daemon;
pub mod files;
pub mod git;
pub mod installer;
pub mod pointguard;
pub mod secrets;
pub mod subscriptions;
pub mod taskbar;
pub mod updater;
pub mod watchers;
pub mod window_state;

#[cfg(windows)]
mod webview;

/// Label of the single main window declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

/// Whether this build hides WebView2's default context menu and DevTools.
/// Debug builds keep them for development. The browser accelerator keys (F5,
/// Ctrl+R, Ctrl+P, F12, zoom, ...) are off in every build (`webview.rs`).
pub const LOCK_DOWN_WEBVIEW: bool = !cfg!(debug_assertions);

/// The daemon protocol version this app speaks, from `pu-core`.
pub fn protocol_version() -> u32 {
    pu_core::protocol::PROTOCOL_VERSION
}

pub fn run() {
    tauri::Builder::default()
        // Links from terminals and chat open in the user's browser (WIN-7.3).
        .plugin(tauri_plugin_opener::init())
        // Open Project's folder picker (WIN-8.15).
        .plugin(tauri_plugin_dialog::init())
        // Agent signal toasts (WIN-7.7).
        .plugin(tauri_plugin_notification::init())
        // In-place updates (WIN-10.3): the app's `updater_*` commands drive it.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(git::commands::GitState::default())
        .manage(attach::AttachState::default())
        // Each machine's daemon client over the ssh relay (WIN-8.14).
        .manage(daemon::machine::MachineClients::default())
        .manage(pointguard::chat::ChatSockets::default())
        .manage(secrets::pairing::PairingSecrets::default())
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
            daemon::machine::machines_load,
            daemon::machine::machines_add,
            daemon::machine::machines_remove,
            daemon::machine::machines_ssh_config,
            daemon::machine::machine_request,
            daemon::machine::machine_probe,
            daemon::machine::machine_start_daemon,
            daemon::machine::machine_test_connection,
            installer::installer_status,
            installer::installer_run,
            updater::updater_status,
            updater::updater_check,
            updater::updater_download,
            updater::updater_install,
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
            chrome::chrome_set_layout,
            taskbar::taskbar_set_badge,
            pointguard::pointguard_status,
            pointguard::pointguard_start,
            pointguard::pointguard_stop,
            pointguard::pointguard_restart,
            pointguard::pointguard_request,
            pointguard::pointguard_chat_connection,
            pointguard::pointguard_validate_folder,
            pointguard::chat::pointguard_chat_open,
            pointguard::chat::pointguard_chat_send,
            pointguard::chat::pointguard_chat_close,
            pointguard::recovery::pointguard_recovery_load,
            pointguard::recovery::pointguard_recovery_save,
            secrets::pairing::pointguard_pairing_read,
            secrets::pairing::pointguard_pairing_save,
            secrets::pairing::pointguard_pairing_import,
        ])
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                use tauri::Manager;
                let dir = window.app_handle().path().app_data_dir().ok();
                window_state::persist(window, dir.as_deref());
                // Quit waits for the owned Point Guard runtime (WIN-9.3).
                if let Some(pointguard) = window
                    .app_handle()
                    .try_state::<pointguard::PointGuardState>()
                    && pointguard.intercept_close(window.app_handle())
                {
                    api.prevent_close();
                }
            }
        })
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
                let pointguard = pointguard::PointGuardState::for_app(app.handle());
                pointguard.start_at_launch();
                app.manage(pointguard);
                updater::manage(app.handle());
            }
            #[cfg(windows)]
            {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    webview::configure(&window, LOCK_DOWN_WEBVIEW)?;
                    if let Err(error) = chrome::install(&window) {
                        eprintln!("PurePoint: could not install the window chrome: {error}");
                    }
                }
            }
            // The window starts hidden; it is shown where it was last closed.
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                let dir = app.path().app_data_dir().ok();
                window_state::restore(&window, dir.as_deref());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building PurePoint")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                if let Some(pointguard) = app.try_state::<pointguard::PointGuardState>() {
                    pointguard.stop_for_termination();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_the_pu_core_protocol() {
        assert!(protocol_version() > 0);
    }
}
