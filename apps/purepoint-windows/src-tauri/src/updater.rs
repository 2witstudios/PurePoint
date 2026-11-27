//! In-place updates (WIN-10.3, REQ-WINAPP-076), the port of `Services/CheckForUpdatesViewModel.swift`:
//! Sparkle is replaced by `tauri-plugin-updater` (WINAPP-024).
//!
//! The feed is `latest-windows.json` on the fixed `appcast` GitHub release, beside the Mac's
//! Sparkle `appcast.xml` (`plugins.updater.endpoints` in `tauri.conf.json`). The update is the
//! NSIS setup exe, verified against `plugins.updater.pubkey` (minisign) before it runs, and its
//! signature must name the version the feed announces (`requireSignedVersion`). The committed key
//! is [`PLACEHOLDER_PUBKEY`]; `scripts/build-release.mjs` merges the real one from
//! `TAURI_UPDATER_PUBKEY`. While the placeholder is in use the app never fetches or installs an
//! update and reports [`UpdaterStatus::NotConfigured`] ("Updates not configured").
//!
//! States follow Sparkle's standard driver: check → up to date or available (version and notes)
//! → download (progress, signature verified) → ready to install → install and relaunch. Before
//! the installer runs, Point Guard goes through the quit path (stop, or ask to stop the owned
//! run) and the daemon is shut down; the installer's hooks then stop anything left.
//!
//! Tauri surface (front end: `ui/src/services/updater.ts`): commands `updater_status`,
//! `updater_check`, `updater_download` and `updater_install`, and the `updater-state` event
//! carrying an [`UpdaterStatus`] on every change. The plugin's own commands are not granted to
//! the window (`capabilities/default.json`), so every install goes through these.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_updater::{Error as UpdateError, Update, Updater, UpdaterExt};

/// The committed `plugins.updater.pubkey`: no release key exists yet (WIN-10.6). A build with it
/// refuses to update.
pub const PLACEHOLDER_PUBKEY: &str =
    "PLACEHOLDER-NOT-A-KEY: set TAURI_UPDATER_PUBKEY when running scripts/build-release.mjs";

/// Event emitted with the new [`UpdaterStatus`] whenever it changes.
pub const UPDATER_STATE_EVENT: &str = "updater-state";

/// How long one feed or download request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Download progress is emitted at most once per this many bytes (and at the end).
const PROGRESS_STEP: u64 = 256 * 1024;

/// What the updater is doing, as the menu item and the About status show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdaterStatus {
    /// No check yet.
    Idle,
    /// The build carries the placeholder key (or none): updates are off.
    NotConfigured,
    Checking,
    UpToDate {
        current_version: String,
    },
    Available {
        version: String,
        current_version: String,
        notes: Option<String>,
        /// RFC 3339, when the feed gives one.
        date: Option<String>,
    },
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Downloaded and verified; installing quits and relaunches the app.
    ReadyToRestart {
        version: String,
    },
    /// Point Guard and the daemon are stopping, then the installer runs.
    Installing {
        version: String,
    },
    Error {
        message: String,
    },
}

impl UpdaterStatus {
    /// Whether a new check may start (Sparkle's `canCheckForUpdates`).
    pub fn can_check(&self) -> bool {
        !matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing { .. }
        )
    }
}

/// The public key a config carries, unless it is missing or the placeholder.
pub fn configured_pubkey(config: &tauri::Config) -> Option<String> {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|key| key.as_str())
        .map(str::trim)
        .filter(|key| is_release_key(key))
        .map(str::to_owned)
}

/// A key the updater can trust: not empty and not the placeholder.
pub fn is_release_key(key: &str) -> bool {
    let key = key.trim();
    !key.is_empty() && key != PLACEHOLDER_PUBKEY && !key.starts_with("PLACEHOLDER")
}

/// The user-facing reason for a failed check or download.
fn describe(error: &UpdateError) -> String {
    match error {
        UpdateError::Minisign(_)
        | UpdateError::Base64(_)
        | UpdateError::SignatureUtf8(_)
        | UpdateError::SignedVersionMismatch { .. }
        | UpdateError::MissingSignedVersion => {
            format!("The update is improperly signed and could not be validated. ({error})")
        }
        UpdateError::ReleaseNotFound
        | UpdateError::Reqwest(_)
        | UpdateError::Network(_)
        | UpdateError::Serialization(_)
        | UpdateError::TargetNotFound(_)
        | UpdateError::TargetsNotFound(_) => {
            format!("An error occurred in retrieving update information. ({error})")
        }
        _ => error.to_string(),
    }
}

type Listener = Box<dyn Fn(&UpdaterStatus) + Send + Sync>;

#[derive(Default)]
struct Pending {
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}

struct Inner {
    configured: bool,
    current_version: String,
    status: Mutex<UpdaterStatus>,
    pending: Mutex<Pending>,
    listeners: Mutex<Vec<Listener>>,
}

/// The update state machine; the Tauri commands drive it.
#[derive(Clone)]
pub struct UpdaterService {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl UpdaterService {
    /// `configured`: the app carries a release key ([`configured_pubkey`]); `current_version`:
    /// the app's version, which the feed's is compared with.
    pub fn new(configured: bool, current_version: impl Into<String>) -> Self {
        let status = if configured {
            UpdaterStatus::Idle
        } else {
            UpdaterStatus::NotConfigured
        };
        Self {
            inner: Arc::new(Inner {
                configured,
                current_version: current_version.into(),
                status: Mutex::new(status),
                pending: Mutex::new(Pending::default()),
                listeners: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.inner.configured
    }

    pub fn on_change(&self, listener: impl Fn(&UpdaterStatus) + Send + Sync + 'static) {
        lock(&self.inner.listeners).push(Box::new(listener));
    }

    pub fn status(&self) -> UpdaterStatus {
        lock(&self.inner.status).clone()
    }

    fn set_status(&self, status: UpdaterStatus) {
        let changed = {
            let mut current = lock(&self.inner.status);
            let changed = *current != status;
            *current = status.clone();
            changed
        };
        if changed {
            for listener in lock(&self.inner.listeners).iter() {
                listener(&status);
            }
        }
    }

    /// Claim the next step: false (and nothing changes) while another one runs.
    fn begin(&self, next: UpdaterStatus, allowed: impl Fn(&UpdaterStatus) -> bool) -> bool {
        {
            let mut current = lock(&self.inner.status);
            if !allowed(&current) {
                return false;
            }
            *current = next.clone();
        }
        for listener in lock(&self.inner.listeners).iter() {
            listener(&next);
        }
        true
    }

    /// Check the feed. `updater` builds the plugin's updater; it is not called when updates are
    /// not configured or a check, download or install is already running.
    pub async fn check<F>(&self, updater: F) -> UpdaterStatus
    where
        F: FnOnce() -> Result<Updater, UpdateError>,
    {
        if !self.inner.configured {
            self.set_status(UpdaterStatus::NotConfigured);
            return self.status();
        }
        if !self.begin(UpdaterStatus::Checking, UpdaterStatus::can_check) {
            return self.status();
        }
        *lock(&self.inner.pending) = Pending::default();
        let result = match updater() {
            Ok(updater) => updater.check().await,
            Err(error) => Err(error),
        };
        let status = match result {
            Ok(Some(update)) => {
                let status = UpdaterStatus::Available {
                    version: update.version.clone(),
                    current_version: update.current_version.clone(),
                    notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
                    date: update.date.and_then(|date| {
                        date.format(&time::format_description::well_known::Rfc3339)
                            .ok()
                    }),
                };
                lock(&self.inner.pending).update = Some(update);
                status
            }
            Ok(None) => UpdaterStatus::UpToDate {
                current_version: self.inner.current_version.clone(),
            },
            Err(error) => UpdaterStatus::Error {
                message: describe(&error),
            },
        };
        self.set_status(status);
        self.status()
    }

    /// Download the available update and verify its signature (Sparkle's Install Update).
    pub async fn download(&self) -> UpdaterStatus {
        let Some(update) = lock(&self.inner.pending).update.clone() else {
            return self.status();
        };
        let version = update.version.clone();
        let start = UpdaterStatus::Downloading {
            version: version.clone(),
            downloaded: 0,
            total: None,
        };
        if !self.begin(start, |s| matches!(s, UpdaterStatus::Available { .. })) {
            return self.status();
        }
        let mut downloaded: u64 = 0;
        let mut reported: u64 = 0;
        let result = update
            .download(
                |chunk, total| {
                    downloaded += chunk as u64;
                    if downloaded - reported >= PROGRESS_STEP || Some(downloaded) == total {
                        reported = downloaded;
                        self.set_status(UpdaterStatus::Downloading {
                            version: version.clone(),
                            downloaded,
                            total,
                        });
                    }
                },
                || {},
            )
            .await;
        let status = match result {
            Ok(bytes) => {
                lock(&self.inner.pending).bytes = Some(bytes);
                UpdaterStatus::ReadyToRestart { version }
            }
            Err(error) => {
                *lock(&self.inner.pending) = Pending::default();
                UpdaterStatus::Error {
                    message: describe(&error),
                }
            }
        };
        self.set_status(status);
        self.status()
    }

    /// Take the verified download for installing; `None` unless one is ready.
    pub fn take_for_install(&self) -> Option<(Update, Vec<u8>)> {
        let version = match self.status() {
            UpdaterStatus::ReadyToRestart { version } => version,
            _ => return None,
        };
        let mut pending = lock(&self.inner.pending);
        let (Some(update), Some(bytes)) = (pending.update.clone(), pending.bytes.take()) else {
            return None;
        };
        drop(pending);
        if !self.begin(UpdaterStatus::Installing { version }, |s| {
            matches!(s, UpdaterStatus::ReadyToRestart { .. })
        }) {
            lock(&self.inner.pending).bytes = Some(bytes);
            return None;
        }
        Some((update, bytes))
    }

    /// The install did not happen (Point Guard kept running): ready again, with the same bytes.
    pub fn restore(&self, update: Update, bytes: Vec<u8>) {
        let version = update.version.clone();
        *lock(&self.inner.pending) = Pending {
            update: Some(update),
            bytes: Some(bytes),
        };
        self.set_status(UpdaterStatus::ReadyToRestart { version });
    }

    /// The installer could not be started.
    pub fn fail(&self, message: String) {
        *lock(&self.inner.pending) = Pending::default();
        self.set_status(UpdaterStatus::Error { message });
    }
}

/// The plugin's updater for this app, with the request timeout.
pub fn app_updater<R: Runtime>(app: &AppHandle<R>) -> Result<Updater, UpdateError> {
    app.updater_builder().timeout(REQUEST_TIMEOUT).build()
}

/// Manage the service, emitting `updater-state`.
pub fn manage(app: &AppHandle) {
    let service = UpdaterService::new(
        configured_pubkey(app.config()).is_some(),
        app.package_info().version.to_string(),
    );
    let emitter = app.clone();
    service.on_change(move |status| {
        let _ = emitter.emit(UPDATER_STATE_EVENT, status);
    });
    app.manage(service);
}

/// The last state (the last `updater-state` payload).
#[tauri::command]
pub fn updater_status(state: State<'_, UpdaterService>) -> UpdaterStatus {
    state.status()
}

/// Check for updates (the Check for Updates… command).
#[tauri::command]
pub async fn updater_check(app: AppHandle) -> Result<UpdaterStatus, String> {
    let service = app.state::<UpdaterService>().inner().clone();
    Ok(service.check(|| app_updater(&app)).await)
}

/// Download and verify the available update.
#[tauri::command]
pub async fn updater_download(state: State<'_, UpdaterService>) -> Result<UpdaterStatus, String> {
    let service = state.inner().clone();
    Ok(service.download().await)
}

/// Install and relaunch: Point Guard through the quit path, the daemon shut down, then the NSIS
/// installer (passive, `/UPDATE`, relaunching the app) and this process exits.
#[tauri::command]
pub async fn updater_install(app: AppHandle) -> Result<UpdaterStatus, String> {
    let service = app.state::<UpdaterService>().inner().clone();
    let Some((update, bytes)) = service.take_for_install() else {
        return Ok(service.status());
    };
    if let Some(pointguard) = app.try_state::<crate::pointguard::PointGuardState>()
        && !pointguard.stop_for_update().await
    {
        service.restore(update, bytes);
        return Ok(service.status());
    }
    if let Some(daemon) = app.try_state::<crate::daemon::DaemonState>()
        && let Ok(lifecycle) = daemon.lifecycle()
    {
        lifecycle.stop_daemon().await;
    }
    // On Windows this launches the installer and exits the process.
    if let Err(error) = update.install(bytes) {
        service.fail(format!("The update could not be installed. ({error})"));
    }
    Ok(service.status())
}
