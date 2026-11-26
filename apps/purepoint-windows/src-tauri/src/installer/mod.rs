//! Installs the `pu` CLI, `pu-engine` and the Claude Code plugin for the user (WIN-10.1,
//! REQ-WINAPP-036/037), ported from `Services/CLIInstaller.swift`.
//!
//! - `cli`: the install itself (binaries into `%USERPROFILE%\.pu\bin`, the plugin into the
//!   local marketplace, `claude` registration).
//! - `user_path`: `.pu\bin` on the user `PATH` in `HKCU\Environment`.
//!
//! Tauri surface: commands `installer_status` and `installer_run`, and the `installer-state`
//! event carrying an [`InstallerStatus`] on every change. The app runs the installer in the
//! background at startup; a `pu-engine.exe` replaced while running restarts the daemon
//! through the daemon lifecycle (`Shutdown`, then the new binary).

pub mod cli;
pub mod user_path;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use cli::{InstallReport, Installer, SystemClaude};

/// Event emitted with the new [`InstallerStatus`] whenever it changes.
pub const INSTALLER_STATE_EVENT: &str = "installer-state";

/// The installer's state, as the UI banner shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InstallerStatus {
    Idle,
    Running,
    Installed {
        report: InstallReport,
    },
    /// Some step failed; `message` is the banner text, the report says what did succeed.
    Failed {
        message: String,
        report: Option<InstallReport>,
    },
}

impl InstallerStatus {
    fn finished(report: InstallReport) -> Self {
        if report.errors.is_empty() {
            Self::Installed { report }
        } else {
            Self::Failed {
                message: report.errors.join("\n"),
                report: Some(report),
            }
        }
    }
}

type Listener = Box<dyn Fn(&InstallerStatus) + Send + Sync>;

/// Runs the installer once at a time and tracks its last outcome.
pub struct InstallerService {
    home: Option<PathBuf>,
    sources: Vec<PathBuf>,
    status: Mutex<InstallerStatus>,
    run_lock: tokio::sync::Mutex<()>,
    listener: Mutex<Option<Listener>>,
}

impl InstallerService {
    pub fn new(home: Option<PathBuf>, sources: Vec<PathBuf>) -> Self {
        Self {
            home,
            sources,
            status: Mutex::new(InstallerStatus::Idle),
            run_lock: tokio::sync::Mutex::new(()),
            listener: Mutex::new(None),
        }
    }

    /// The user's home, the app exe's directory (sidecars) and the resource directory,
    /// emitting `installer-state`.
    pub fn for_app(app: &AppHandle) -> Self {
        let exe_dir = std::env::current_exe()
            .and_then(pu_core::canon::canonicalize)
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let resource_dir = app.path().resource_dir().ok();
        let mut sources: Vec<PathBuf> = exe_dir.into_iter().chain(resource_dir).collect();
        sources.dedup();
        let service = Self::new(pu_core::paths::home_dir().ok(), sources);
        let app = app.clone();
        service.on_change(move |status| {
            let _ = app.emit(INSTALLER_STATE_EVENT, status);
        });
        service
    }

    pub fn on_change(&self, listener: impl Fn(&InstallerStatus) + Send + Sync + 'static) {
        *lock(&self.listener) = Some(Box::new(listener));
    }

    pub fn status(&self) -> InstallerStatus {
        lock(&self.status).clone()
    }

    fn set_status(&self, status: InstallerStatus) {
        *lock(&self.status) = status.clone();
        if let Some(listener) = lock(&self.listener).as_ref() {
            listener(&status);
        }
    }

    /// Run the install with the real registry and `claude`, then restart the daemon if its
    /// binary was replaced while running. `restart` is the daemon lifecycle's restart.
    pub async fn run<F, Fut>(&self, restart: F) -> InstallerStatus
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        let Some(home) = self.home.clone() else {
            let status = InstallerStatus::Failed {
                message: "Could not find the user's home directory.".into(),
                report: None,
            };
            self.set_status(status.clone());
            return status;
        };
        let sources = self.sources.clone();
        self.run_with(restart, move || {
            Installer {
                home: home.clone(),
                sources,
                env: &user_path::RegistryEnvironment,
                claude: &SystemClaude { home },
            }
            .run()
        })
        .await
    }

    /// One serialized run of `install` (on the blocking pool), then the restart if needed.
    pub async fn run_with<F, Fut, I>(&self, restart: F, install: I) -> InstallerStatus
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
        I: FnOnce() -> InstallReport + Send + 'static,
    {
        let _serial = self.run_lock.lock().await;
        self.set_status(InstallerStatus::Running);
        let status = match tokio::task::spawn_blocking(install).await {
            Ok(mut report) => {
                if report.restart_daemon {
                    match restart().await {
                        Ok(()) => report.daemon_restarted = true,
                        Err(e) => report.errors.push(format!(
                            "pu-engine.exe was updated, but the daemon did not restart: {e}"
                        )),
                    }
                }
                InstallerStatus::finished(report)
            }
            Err(e) => InstallerStatus::Failed {
                message: format!("The installer stopped unexpectedly: {e}"),
                report: None,
            },
        };
        self.set_status(status.clone());
        status
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run the installer for the app, restarting the daemon through its lifecycle when needed.
async fn run_for_app(app: AppHandle) -> InstallerStatus {
    let service = app.state::<Arc<InstallerService>>().inner().clone();
    let daemon = app.clone();
    service
        .run(move || async move {
            let state = daemon.state::<crate::daemon::DaemonState>();
            let lifecycle = state.lifecycle().map_err(|e| e.to_string())?;
            lifecycle.restart_daemon().await.map_err(|e| e.to_string())
        })
        .await
}

/// Manage the installer and start it in the background, as the Mac does at launch.
pub fn start(app: &AppHandle) {
    app.manage(Arc::new(InstallerService::for_app(app)));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run_for_app(app).await;
    });
}

/// The installer's last state (the last `installer-state` payload).
#[tauri::command]
pub fn installer_status(state: State<'_, Arc<InstallerService>>) -> InstallerStatus {
    state.status()
}

/// Run the installer now (after a failure, from the banner) and return its outcome.
#[tauri::command]
pub async fn installer_run(app: AppHandle) -> Result<InstallerStatus, String> {
    Ok(run_for_app(app).await)
}

#[cfg(test)]
mod tests {
    use super::cli::{ClaudeAction, PluginAction};
    use super::*;

    fn report(restart: bool, errors: &[&str]) -> InstallReport {
        InstallReport {
            bin_dir: PathBuf::from(r"C:\u\.pu\bin"),
            binaries: Vec::new(),
            stale_removed: 0,
            stale_kept: 0,
            path_added: false,
            plugin: PluginAction::UpToDate,
            claude: ClaudeAction::NotNeeded,
            restart_daemon: restart,
            daemon_restarted: false,
            errors: errors.iter().map(|e| e.to_string()).collect(),
        }
    }

    fn service() -> (InstallerService, Arc<Mutex<Vec<InstallerStatus>>>) {
        let service = InstallerService::new(None, Vec::new());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        service.on_change(move |s| sink.lock().unwrap().push(s.clone()));
        (service, seen)
    }

    #[tokio::test]
    async fn given_clean_run_should_report_installed_without_restart() {
        let (service, seen) = service();
        let restarted = Arc::new(Mutex::new(false));
        let flag = restarted.clone();
        let status = service
            .run_with(
                move || async move {
                    *flag.lock().unwrap() = true;
                    Ok(())
                },
                || report(false, &[]),
            )
            .await;
        assert_eq!(
            status,
            InstallerStatus::Installed {
                report: report(false, &[])
            }
        );
        assert!(!*restarted.lock().unwrap());
        assert_eq!(seen.lock().unwrap()[0], InstallerStatus::Running);
        assert_eq!(service.status(), status);
    }

    #[tokio::test]
    async fn given_engine_replaced_in_use_should_restart_daemon() {
        let (service, _) = service();
        let status = service
            .run_with(|| async { Ok(()) }, || report(true, &[]))
            .await;
        let InstallerStatus::Installed { report } = status else {
            panic!("{status:?}");
        };
        assert!(report.daemon_restarted);
    }

    #[tokio::test]
    async fn given_failed_restart_or_step_should_surface_failure() {
        let (service, seen) = service();
        let status = service
            .run_with(|| async { Err("no daemon".into()) }, || report(true, &[]))
            .await;
        let InstallerStatus::Failed { message, .. } = &status else {
            panic!("{status:?}");
        };
        assert!(message.contains("no daemon"), "{message}");
        assert_eq!(seen.lock().unwrap().last(), Some(&status));

        let status = service
            .run_with(
                || async { Ok(()) },
                || report(false, &["Could not install pu.exe"]),
            )
            .await;
        assert!(
            matches!(&status, InstallerStatus::Failed { message, .. } if message == "Could not install pu.exe")
        );
    }

    #[tokio::test]
    async fn given_no_home_should_fail_without_installing() {
        let (service, _) = service();
        let status = service.run(|| async { Ok(()) }).await;
        assert!(matches!(
            status,
            InstallerStatus::Failed { report: None, .. }
        ));
    }

    #[test]
    fn given_status_should_serialize_for_the_ui() {
        let json = serde_json::to_value(InstallerStatus::Installed {
            report: report(false, &[]),
        })
        .unwrap();
        assert_eq!(json["state"], "installed");
        assert_eq!(json["report"]["pathAdded"], false);
        assert_eq!(json["report"]["plugin"], "upToDate");
        assert_eq!(json["report"]["restartDaemon"], false);
    }
}
