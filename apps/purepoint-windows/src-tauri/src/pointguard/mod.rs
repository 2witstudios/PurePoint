//! The app-managed Point Guard runtime (WIN-9.3, REQ-WINAPP-077).
//!
//! - `seal`: the hash-manifest seal over the bundled runtime (`BundleSeal.swift`).
//! - `runtime`: bundled and development layouts, the readiness descriptor, private state.
//! - `service`: the owned child process: start, readiness, stop, restart with backoff
//!   (`PointGuardServiceModel.swift`, process side).
//! - `admin`: owner admin requests over the bridge's owner pipe.
//! - `tailnet`: this PC's tailnet address for `PI_MOBILE_HOST` (`PointGuardTailnet.swift`).
//! - `lifecycle`: Quit waits for Point Guard (`PointGuardApplicationDelegate.swift`).
//! - `chat`: the chat WebSocket to the local bridge, relayed over a Tauri `Channel` (WIN-9.4).
//! - `recovery`: the chat's private draft and submission recovery files (WIN-9.4).
//!
//! Tauri surface (front end: `ui/src/services/pointguard.ts`): commands `pointguard_status`,
//! `pointguard_start`, `pointguard_stop`, `pointguard_restart`, `pointguard_request`,
//! `pointguard_chat_connection` and `pointguard_validate_folder`, and the `pointguard-state`
//! event carrying a [`service::Snapshot`] on every change. The chat (front end
//! `ui/src/services/pointGuardChat.ts`) adds `pointguard_chat_open`, `pointguard_chat_send`,
//! `pointguard_chat_close`, `pointguard_recovery_load` and `pointguard_recovery_save`.
//!
//! The app starts Point Guard at launch, as the Mac does. A debug build starts it only with
//! `PUREPOINT_POINTGUARD_DEV=1` (then from the repo, see `runtime`), so development runs
//! and the e2e suite never start a bridge against the real profile.

pub mod admin;
pub mod chat;
pub mod lifecycle;
pub mod recovery;
pub mod runtime;
pub mod seal;
pub mod service;
pub mod tailnet;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use lifecycle::{Intercept, MessageBoxPrompt, QuitGate};
use service::{ChatConnection, PointGuardService, Snapshot};

/// Event emitted with the new [`Snapshot`] whenever it changes.
pub const POINTGUARD_STATE_EVENT: &str = "pointguard-state";

/// Tauri state: the service and the quit gate.
pub struct PointGuardState {
    service: PointGuardService,
    gate: QuitGate,
    autostart: bool,
}

impl PointGuardState {
    /// The service for this user, emitting `pointguard-state`.
    pub fn for_app(app: &AppHandle) -> Self {
        let home = pu_core::paths::home_dir()
            .ok()
            .filter(|h| h.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\"));
        // Tauri gives the resource dir as a verbatim `\\?\` path, which node.exe cannot resolve its entry
        // script from (`lstat 'C:'`); the runtime paths are built from it (WIN-10.2).
        let resource_dir = app
            .path()
            .resource_dir()
            .ok()
            .map(|dir| pu_core::canon::normalize(&dir));
        let exe_dir = std::env::current_exe()
            .and_then(pu_core::canon::canonicalize)
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let mut pu_dirs: Vec<PathBuf> = exe_dir.into_iter().chain(resource_dir.clone()).collect();
        pu_dirs.dedup();
        let state_dir = service::default_state_dir(&home);
        let service =
            PointGuardService::new(service::app_config(resource_dir, pu_dirs, home, state_dir));
        let emitter = app.clone();
        service.on_change(move |snapshot| {
            let _ = emitter.emit(POINTGUARD_STATE_EVENT, snapshot);
        });
        let autostart =
            !cfg!(debug_assertions) || std::env::var(runtime::DEV_ENV).is_ok_and(|v| v == "1");
        Self {
            service,
            gate: QuitGate::default(),
            autostart,
        }
    }

    pub fn service(&self) -> &PointGuardService {
        &self.service
    }

    /// Start at launch (the Mac's `onAppear` start), unless this is a debug build without
    /// the development opt-in.
    pub fn start_at_launch(&self) {
        if self.autostart {
            let service = self.service.clone();
            tauri::async_runtime::spawn(async move {
                let _ = service.start(None).await;
            });
        }
    }

    /// The main window's close request: true when the close must be prevented while the
    /// quit decision runs (the app exits itself once Point Guard has stopped).
    pub fn intercept_close(&self, app: &AppHandle) -> bool {
        match self.gate.intercept(self.service.is_active()) {
            Intercept::Allow => false,
            Intercept::Wait => true,
            Intercept::Decide => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<PointGuardState>();
                    let approved =
                        lifecycle::may_terminate(&state.service, Arc::new(MessageBoxPrompt)).await;
                    state.gate.finish(approved);
                    if approved {
                        app.exit(0);
                    }
                });
                true
            }
        }
    }

    /// App exit: bounded wait for the owned child (the Swift `stopForTermination`).
    pub fn stop_for_termination(&self) {
        self.service.stop_for_termination();
    }
}

/// The current state (the last `pointguard-state` payload).
#[tauri::command]
pub fn pointguard_status(state: State<'_, PointGuardState>) -> Snapshot {
    state.service.snapshot()
}

/// Start Point Guard (Start / Retry), optionally in a recovery working folder. Resolves once
/// it is ready or has failed; the failure is also in the state.
#[tauri::command]
pub async fn pointguard_start(
    state: State<'_, PointGuardState>,
    cwd: Option<String>,
) -> Result<(), String> {
    let cwd = cwd.map(|c| runtime::validated_folder(&c)).transpose()?;
    state.service.start(cwd).await
}

/// Stop Point Guard while idle.
#[tauri::command]
pub async fn pointguard_stop(state: State<'_, PointGuardState>) -> Result<(), String> {
    state.service.stop().await
}

/// Stop, then start again.
#[tauri::command]
pub async fn pointguard_restart(
    state: State<'_, PointGuardState>,
    cwd: Option<String>,
) -> Result<(), String> {
    let cwd = cwd.map(|c| runtime::validated_folder(&c)).transpose()?;
    state.service.restart(cwd).await
}

/// One owner admin operation (`status`, `providers`, `auth.start`, `pairing.create`, ...).
#[tauri::command]
pub async fn pointguard_request(
    state: State<'_, PointGuardState>,
    operation: String,
    fields: Option<Map<String, Value>>,
) -> Result<Value, String> {
    state
        .service
        .request(&operation, fields.unwrap_or_default())
        .await
}

/// The local chat endpoint and secret, while ready.
#[tauri::command]
pub fn pointguard_chat_connection(
    state: State<'_, PointGuardState>,
) -> Result<ChatConnection, String> {
    state.service.chat_connection()
}

/// An existing absolute working folder, canonicalized.
#[tauri::command]
pub fn pointguard_validate_folder(path: String) -> Result<String, String> {
    runtime::validated_folder(&path).map(|p| p.display().to_string())
}
