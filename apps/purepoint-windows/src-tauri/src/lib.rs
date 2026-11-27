//! Tauri backend for the PurePoint Windows app (DEC-7).

pub mod attach;
pub mod automation;
pub mod chrome;
pub mod conversations;
pub mod daemon;
pub mod files;
pub mod git;
pub mod installer;
pub mod navigation;
pub mod pointguard;
pub mod secrets;
pub mod subscriptions;
pub mod taskbar;
pub mod updater;
pub mod watchers;
pub mod window_state;

#[cfg(windows)]
mod webview;

/// Label of the single main window declared in `tauri.conf.json` (`"create": false`: `setup`
/// builds it from that declaration, see [`create_main_window`]).
pub const MAIN_WINDOW: &str = "main";

/// Whether this build hides WebView2's default context menu and DevTools.
/// Debug builds keep them for development. The browser accelerator keys (F5,
/// Ctrl+R, Ctrl+P, F12, zoom, ...) are off in every build (`webview.rs`).
pub const LOCK_DOWN_WEBVIEW: bool = !cfg!(debug_assertions);

/// The daemon protocol version this app speaks, from `pu-core`.
pub fn protocol_version() -> u32 {
    pu_core::protocol::PROTOCOL_VERSION
}

/// The main window's page started loading (a reload, or the renderer restarted after a crash):
/// nothing on the old page reads its channels or will close what it opened, so the backend
/// drops it all before the new page starts its own. Terminal sessions and chat sockets close,
/// watchers stop (and release tokens are forgotten), and subscriptions stop. Also runs on the
/// first load, when everything is still empty.
/// Build the main window from its `tauri.conf.json` declaration, with the WebDriver switches
/// a debug build under tauri-driver needs ([`automation::browser_args`]).
fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .expect("tauri.conf.json declares the main window");
    if let Some(args) = automation::browser_args(
        std::env::var("TAURI_WEBVIEW_AUTOMATION").ok().as_deref(),
        std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
            .ok()
            .as_deref(),
    ) {
        config.additional_browser_args = Some(args);
    }
    tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
    Ok(())
}

fn reset_for_page_load<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(attach) = app.try_state::<attach::AttachState>() {
        attach.clear();
    }
    if let Some(sockets) = app.try_state::<pointguard::chat::ChatSockets>() {
        sockets.clear();
    }
    if let Some(watchers) = app.try_state::<watchers::commands::WatcherState>() {
        watchers.reset();
    }
    if let Some(subscriptions) = app.try_state::<subscriptions::Subscriptions>() {
        subscriptions.stop_all();
    }
}

pub fn run() {
    tauri::Builder::default()
        .on_page_load(|webview, payload| {
            use tauri::Manager;
            if webview.label() == MAIN_WINDOW
                && payload.event() == tauri::webview::PageLoadEvent::Started
            {
                reset_for_page_load(webview.app_handle());
            }
        })
        // Links from terminals and chat open in the user's browser (WIN-7.3).
        .plugin(tauri_plugin_opener::init())
        // The window only ever shows the app: other navigations are refused.
        .plugin(navigation::plugin())
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
            files::open::open_in_default_app,
            files::text::fs_stat_text,
            files::text::fs_read_text,
            files::text::fs_read_document,
            files::text::fs_write_text_if_unmodified,
            files::tree::fs_list_dir,
            files::tree::fs_rename,
            files::tree::fs_delete,
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
            pointguard::pointguard_home,
            pointguard::chat::pointguard_chat_open,
            pointguard::chat::pointguard_chat_send,
            pointguard::chat::pointguard_chat_close,
            pointguard::recovery::pointguard_recovery_load,
            pointguard::recovery::pointguard_recovery_save,
            secrets::pairing::pointguard_pairing_has,
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
            create_main_window(app)?;
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

    /// The webview's CSP: images from the app, data URIs and https only (chat images load on a
    /// click; `file:`/UNC never, so Windows never authenticates to a host a reply names), no
    /// `asset:` protocol, no plugins, no `<base>` and no form posts.
    #[test]
    fn csp_allows_https_images_and_locks_down_the_rest() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = config["app"]["security"]["csp"].as_str().unwrap();
        let directives: Vec<Vec<&str>> = csp
            .split(';')
            .map(|d| d.split_whitespace().collect())
            .filter(|d: &Vec<&str>| !d.is_empty())
            .collect();
        let find = |name: &str| {
            directives
                .iter()
                .find(|d| d[0] == name)
                .map(|d| d[1..].to_vec())
                .unwrap_or_else(|| panic!("no {name} in {csp}"))
        };
        assert_eq!(find("img-src"), ["'self'", "data:", "https:"]);
        assert_eq!(find("object-src"), ["'none'"]);
        assert_eq!(find("base-uri"), ["'none'"]);
        assert_eq!(find("form-action"), ["'none'"]);
        assert_eq!(find("default-src"), ["'self'"]);
        assert!(!csp.contains("asset:") && !csp.contains("file:"), "{csp}");
    }
}
