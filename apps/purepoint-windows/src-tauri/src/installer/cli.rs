//! Port of `Services/CLIInstaller.swift` (REQ-WINAPP-036, -037).
//!
//! - Binaries: `pu.exe` and `pu-engine.exe` from the app's sidecar or resource directory
//!   into `%USERPROFILE%\.pu\bin` when the bundled file is newer (modification date, the
//!   Swift `isUpToDate`). Each is copied to a temporary file beside the target, then renamed
//!   over it.
//! - Running binaries: Windows refuses to replace or delete the file of a running image but
//!   lets it be renamed. When the rename over the target fails, the old file is renamed
//!   aside to `<name>.old-<n>` and the new one takes its place; a replaced `pu-engine.exe`
//!   asks the caller to restart the daemon. `.old-*` files are deleted on a later run, once
//!   nothing runs them.
//! - `PATH`: `.pu\bin` joins the user `PATH` ([`super::user_path`]).
//! - Plugin: the bundled `pu-plugin` into the local marketplace at
//!   `%USERPROFILE%\.pu\marketplace\plugins\pu` (freshness by `plugin.json`), the marketplace
//!   manifest beside it, the old `.claude\plugins\purepoint` and `.claude\skills\pu`
//!   removed, then `claude plugins marketplace add` and `claude plugins install pu@purepoint`.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::user_path::{self, UserEnvironment};

/// The binaries installed together: `pu` starts only the engine beside it or in `.pu\bin`.
pub const BINARIES: [&str; 2] = ["pu.exe", pu_core::engine_binary::ENGINE_EXE];

/// The bundled plugin directory's name in the resource directory.
pub const PLUGIN_RESOURCE: &str = "pu-plugin";

/// The marketplace manifest, byte for byte the Mac's.
pub const MARKETPLACE_MANIFEST: &str = r#"{
  "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
  "name": "purepoint",
  "version": "0.1.0",
  "description": "PurePoint agent orchestration plugin",
  "owner": { "name": "PurePoint" },
  "plugins": [
    {
      "name": "pu",
      "source": "./plugins/pu",
      "category": "development",
      "description": "Spawn parallel AI agents in isolated worktrees, manage long-horizon tasks, and coordinate agent teams."
    }
  ]
}"#;

/// Longest a `claude` registration call may run before it is killed.
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(120);

/// What happened to one binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BinaryAction {
    /// Not bundled with this build (a dev build): nothing to install.
    NotBundled,
    UpToDate,
    Installed,
    Updated,
    /// The old file was in use, so it was renamed aside before the new one was put in place.
    ReplacedInUse,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryReport {
    pub name: String,
    pub action: BinaryAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginAction {
    NotBundled,
    UpToDate,
    Installed,
    Failed,
}

/// The `claude` registration, run only when the plugin files changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeAction {
    NotNeeded,
    NotFound,
    Registered,
    Failed,
}

/// The outcome of one install run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub bin_dir: PathBuf,
    pub binaries: Vec<BinaryReport>,
    /// `.old-*` files deleted from an earlier in-use replacement.
    pub stale_removed: u32,
    /// `.old-*` files still in use, left for a later run.
    pub stale_kept: u32,
    pub path_added: bool,
    pub plugin: PluginAction,
    pub claude: ClaudeAction,
    /// `pu-engine.exe` was replaced while in use: the daemon should restart on the new one.
    pub restart_daemon: bool,
    /// Set by the caller once it restarted the daemon.
    pub daemon_restarted: bool,
    pub errors: Vec<String>,
}

/// Runs `claude` for plugin registration.
pub trait ClaudeCli: Send + Sync {
    fn find(&self) -> Option<PathBuf>;
    /// Run `exe args…`; true when it exited 0.
    fn run(&self, exe: &Path, args: &[OsString]) -> io::Result<bool>;
}

/// `claude` on `PATH` (with `PATHEXT`, so `claude.exe` or `claude.cmd`), else the native
/// installer's `%USERPROFILE%\.local\bin\claude.exe`; run with no console window.
pub struct SystemClaude {
    pub home: PathBuf,
}

impl ClaudeCli for SystemClaude {
    fn find(&self) -> Option<PathBuf> {
        which::which("claude").ok().or_else(|| {
            [
                self.home.join(".local").join("bin").join("claude.exe"),
                self.home.join(".claude").join("local").join("claude.exe"),
            ]
            .into_iter()
            .find(|p| p.is_file())
        })
    }

    fn run(&self, exe: &Path, args: &[OsString]) -> io::Result<bool> {
        use std::process::{Command, Stdio};
        let mut command = Command::new(exe);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        let mut child = command.spawn()?;
        let deadline = std::time::Instant::now() + CLAUDE_TIMEOUT;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.success());
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{} did not finish", exe.display()),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// One install run's inputs.
pub struct Installer<'a> {
    /// `%USERPROFILE%` (or `HOME` in tests).
    pub home: PathBuf,
    /// Where the bundled files are, searched in order: the app exe's directory (Tauri
    /// sidecars), then the resource directory.
    pub sources: Vec<PathBuf>,
    pub env: &'a dyn UserEnvironment,
    pub claude: &'a dyn ClaudeCli,
}

impl Installer<'_> {
    pub fn bin_dir(&self) -> PathBuf {
        self.home.join(".pu").join("bin")
    }

    fn marketplace_dir(&self) -> PathBuf {
        self.home.join(".pu").join("marketplace")
    }

    fn plugin_dir(&self) -> PathBuf {
        self.marketplace_dir().join("plugins").join("pu")
    }

    /// Install everything that is out of date. Never fails as a whole: each step's error is
    /// recorded in the report and the remaining steps still run.
    pub fn run(&self) -> InstallReport {
        let bin_dir = self.bin_dir();
        let mut report = InstallReport {
            bin_dir: bin_dir.clone(),
            binaries: Vec::new(),
            stale_removed: 0,
            stale_kept: 0,
            path_added: false,
            plugin: PluginAction::NotBundled,
            claude: ClaudeAction::NotNeeded,
            restart_daemon: false,
            daemon_restarted: false,
            errors: Vec::new(),
        };

        (report.stale_removed, report.stale_kept) = remove_stale(&bin_dir);

        for name in BINARIES {
            let action = match self.bundled(name) {
                None => BinaryAction::NotBundled,
                Some(source) => match install_binary(&source, &bin_dir.join(name)) {
                    Ok(action) => action,
                    Err(e) => {
                        report.errors.push(format!("Could not install {name}: {e}"));
                        BinaryAction::Failed
                    }
                },
            };
            if name == pu_core::engine_binary::ENGINE_EXE && action == BinaryAction::ReplacedInUse {
                report.restart_daemon = true;
            }
            report.binaries.push(BinaryReport {
                name: name.into(),
                action,
            });
        }

        // Only a directory that holds `pu.exe` goes on PATH (a dev build bundles nothing).
        if bin_dir.join(BINARIES[0]).is_file() {
            match user_path::ensure_on_path(self.env, &bin_dir.to_string_lossy()) {
                Ok(added) => report.path_added = added,
                Err(e) => report
                    .errors
                    .push(format!("Could not add {} to PATH: {e}", bin_dir.display())),
            }
        }

        report.plugin = match self.install_plugin() {
            Ok(action) => action,
            Err(e) => {
                report
                    .errors
                    .push(format!("Could not install the Claude Code plugin: {e}"));
                PluginAction::Failed
            }
        };
        if report.plugin == PluginAction::Installed {
            report.claude = self.register_with_claude(&mut report.errors);
        }
        report
    }

    /// The first bundled copy of `relative` among the source directories.
    fn bundled(&self, relative: &str) -> Option<PathBuf> {
        self.sources
            .iter()
            .map(|dir| dir.join(relative))
            .find(|p| p.is_file())
    }

    /// Copy the bundled plugin into the marketplace and write its manifest. `Installed` when
    /// files changed.
    fn install_plugin(&self) -> io::Result<PluginAction> {
        let sentinel = Path::new(".claude-plugin").join("plugin.json");
        let Some(bundled) = self
            .sources
            .iter()
            .map(|dir| dir.join(PLUGIN_RESOURCE))
            .find(|dir| dir.join(&sentinel).is_file())
        else {
            return Ok(PluginAction::NotBundled);
        };
        let plugin_dir = self.plugin_dir();
        let target_sentinel = plugin_dir.join(&sentinel);
        if target_sentinel.is_file() && is_up_to_date(&bundled.join(&sentinel), &target_sentinel) {
            return Ok(PluginAction::UpToDate);
        }

        remove_dir_if_present(&plugin_dir)?;
        copy_dir(&bundled, &plugin_dir)?;

        let manifest_dir = self.marketplace_dir().join(".claude-plugin");
        fs::create_dir_all(&manifest_dir)?;
        fs::write(manifest_dir.join("marketplace.json"), MARKETPLACE_MANIFEST)?;

        // Migrate: remove the old install locations.
        let claude_dir = self.home.join(".claude");
        remove_dir_if_present(&claude_dir.join("plugins").join("purepoint"))?;
        remove_dir_if_present(&claude_dir.join("skills").join("pu"))?;
        Ok(PluginAction::Installed)
    }

    /// Register the local marketplace and install the plugin (both idempotent in `claude`).
    fn register_with_claude(&self, errors: &mut Vec<String>) -> ClaudeAction {
        let Some(claude) = self.claude.find() else {
            return ClaudeAction::NotFound;
        };
        let marketplace = self.marketplace_dir().into_os_string();
        let calls: [Vec<OsString>; 2] = [
            vec![
                "plugins".into(),
                "marketplace".into(),
                "add".into(),
                marketplace,
            ],
            vec!["plugins".into(), "install".into(), "pu@purepoint".into()],
        ];
        let mut action = ClaudeAction::Registered;
        for args in calls {
            let shown = args
                .iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            match self.claude.run(&claude, &args) {
                Ok(true) => {}
                Ok(false) => {
                    errors.push(format!("`claude {shown}` failed"));
                    action = ClaudeAction::Failed;
                }
                Err(e) => {
                    errors.push(format!("Could not run `claude {shown}`: {e}"));
                    action = ClaudeAction::Failed;
                }
            }
        }
        action
    }
}

/// The Swift rule: the target is current when it was modified no earlier than the source.
/// `fs::copy` keeps the source's modification time, so a copied file is current.
pub fn is_up_to_date(source: &Path, target: &Path) -> bool {
    let modified = |path: &Path| fs::metadata(path).and_then(|m| m.modified()).ok();
    match (modified(source), modified(target)) {
        (Some(source), Some(target)) => target >= source,
        _ => false,
    }
}

/// Install `source` at `target` unless `target` is current: copy to a temporary file beside
/// it, then rename it over `target`. If `target` is in use (the rename is refused), rename
/// `target` aside to `<name>.old-<n>` and rename the new file in.
pub fn install_binary(source: &Path, target: &Path) -> io::Result<BinaryAction> {
    let existed = target.is_file();
    if existed && is_up_to_date(source, target) {
        return Ok(BinaryAction::UpToDate);
    }
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other("install target has no directory"))?;
    fs::create_dir_all(dir)?;
    let name = file_name(target);
    let temporary = dir.join(format!(".{name}.tmp.{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    if let Err(e) = fs::copy(source, &temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(e);
    }
    match fs::rename(&temporary, target) {
        Ok(()) if existed => Ok(BinaryAction::Updated),
        Ok(()) => Ok(BinaryAction::Installed),
        Err(_) if existed => {
            let aside = aside_path(target);
            if let Err(e) = fs::rename(target, &aside) {
                let _ = fs::remove_file(&temporary);
                return Err(e);
            }
            if let Err(e) = fs::rename(&temporary, target) {
                let _ = fs::rename(&aside, target);
                let _ = fs::remove_file(&temporary);
                return Err(e);
            }
            Ok(BinaryAction::ReplacedInUse)
        }
        Err(e) => {
            let _ = fs::remove_file(&temporary);
            Err(e)
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `<target>.old-<n>` for the first unused `n` from 1.
pub fn aside_path(target: &Path) -> PathBuf {
    let name = file_name(target);
    (1u32..)
        .map(|n| target.with_file_name(format!("{name}.old-{n}")))
        .find(|p| !p.exists())
        .expect("an unused .old-<n> name")
}

/// True for `<binary>.old-<n>` and for a temporary file an interrupted run left behind.
fn is_stale(name: &str) -> bool {
    BINARIES.iter().any(|binary| {
        name.strip_prefix(binary)
            .and_then(|rest| rest.strip_prefix(".old-"))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            || name
                .strip_prefix('.')
                .and_then(|rest| rest.strip_prefix(binary))
                .is_some_and(|rest| rest.starts_with(".tmp."))
    })
}

/// Delete stale files in `bin_dir`. Returns (deleted, still in use).
pub fn remove_stale(bin_dir: &Path) -> (u32, u32) {
    let Ok(entries) = fs::read_dir(bin_dir) else {
        return (0, 0);
    };
    let (mut removed, mut kept) = (0, 0);
    for entry in entries.flatten() {
        if !is_stale(&entry.file_name().to_string_lossy()) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(_) => kept += 1,
        }
    }
    (removed, kept)
}

fn remove_dir_if_present(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Copy a directory tree; each file keeps its modification time (`fs::copy`).
fn copy_dir(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let to = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::SystemTime;

    use super::super::user_path::{MemoryEnvironment, ValueKind};
    use super::*;

    /// Records `claude` calls; `found` is the path `find` returns.
    #[derive(Default)]
    struct FakeClaude {
        found: Option<PathBuf>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ClaudeCli for FakeClaude {
        fn find(&self) -> Option<PathBuf> {
            self.found.clone()
        }
        fn run(&self, _exe: &Path, args: &[OsString]) -> io::Result<bool> {
            self.calls.lock().unwrap().push(
                args.iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
            );
            Ok(true)
        }
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        bundle: PathBuf,
        env: MemoryEnvironment,
        claude: FakeClaude,
    }

    impl Fixture {
        /// A temp HOME and a bundle directory with both binaries and the plugin.
        fn new() -> Self {
            let tmp = tempfile::TempDir::new().unwrap();
            let home = tmp.path().join("home");
            let bundle = tmp.path().join("bundle");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&bundle).unwrap();
            for name in BINARIES {
                fs::write(bundle.join(name), format!("{name} v1")).unwrap();
            }
            let plugin = bundle.join(PLUGIN_RESOURCE);
            fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
            fs::create_dir_all(plugin.join("skills").join("pu")).unwrap();
            fs::write(plugin.join(".claude-plugin").join("plugin.json"), "{}").unwrap();
            fs::write(plugin.join("skills").join("pu").join("SKILL.md"), "skill").unwrap();
            Self {
                _tmp: tmp,
                home,
                bundle,
                env: MemoryEnvironment::with_path(r"C:\Tools", ValueKind::ExpandString),
                claude: FakeClaude {
                    found: Some(PathBuf::from(r"C:\bin\claude.exe")),
                    ..FakeClaude::default()
                },
            }
        }

        fn installer(&self) -> Installer<'_> {
            Installer {
                home: self.home.clone(),
                sources: vec![self.bundle.clone()],
                env: &self.env,
                claude: &self.claude,
            }
        }

        fn bin(&self, name: &str) -> PathBuf {
            self.home.join(".pu").join("bin").join(name)
        }

        /// Rewrite a bundled binary and date it an hour later than its installed copy.
        fn bundle_newer(&self, name: &str, content: &str) {
            let path = self.bundle.join(name);
            fs::write(&path, content).unwrap();
            let later = SystemTime::now() + Duration::from_secs(3600);
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(later)
                .unwrap();
        }
    }

    fn actions(report: &InstallReport) -> Vec<BinaryAction> {
        report.binaries.iter().map(|b| b.action).collect()
    }

    #[test]
    fn given_fresh_home_should_install_binaries_path_and_plugin() {
        let f = Fixture::new();
        // An old Mac-style install to migrate away from.
        let old_plugin = f.home.join(".claude").join("plugins").join("purepoint");
        fs::create_dir_all(&old_plugin).unwrap();

        let report = f.installer().run();

        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(
            actions(&report),
            [BinaryAction::Installed, BinaryAction::Installed]
        );
        for name in BINARIES {
            assert_eq!(
                fs::read_to_string(f.bin(name)).unwrap(),
                format!("{name} v1")
            );
        }
        assert!(report.path_added);
        let bin_dir = f.home.join(".pu").join("bin");
        assert_eq!(
            f.env.current().unwrap().value,
            format!(r"C:\Tools;{}", bin_dir.display())
        );
        assert_eq!(*f.env.broadcasts.lock().unwrap(), 1);

        assert_eq!(report.plugin, PluginAction::Installed);
        let marketplace = f.home.join(".pu").join("marketplace");
        let plugin = marketplace.join("plugins").join("pu");
        assert_eq!(
            fs::read_to_string(plugin.join("skills").join("pu").join("SKILL.md")).unwrap(),
            "skill"
        );
        assert!(plugin.join(".claude-plugin").join("plugin.json").is_file());
        let manifest =
            fs::read_to_string(marketplace.join(".claude-plugin").join("marketplace.json"))
                .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(manifest["name"], "purepoint");
        assert_eq!(manifest["plugins"][0]["source"], "./plugins/pu");
        assert!(!old_plugin.exists());

        assert_eq!(report.claude, ClaudeAction::Registered);
        assert_eq!(
            *f.claude.calls.lock().unwrap(),
            [
                vec![
                    "plugins".to_string(),
                    "marketplace".into(),
                    "add".into(),
                    marketplace.display().to_string()
                ],
                vec!["plugins".into(), "install".into(), "pu@purepoint".into()],
            ]
        );
        assert!(!report.restart_daemon);
    }

    #[test]
    fn given_installed_and_current_should_change_nothing() {
        let f = Fixture::new();
        f.installer().run();
        let calls = f.claude.calls.lock().unwrap().len();

        let report = f.installer().run();

        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(
            actions(&report),
            [BinaryAction::UpToDate, BinaryAction::UpToDate]
        );
        assert!(!report.path_added);
        assert_eq!(*f.env.writes.lock().unwrap(), 1);
        assert_eq!(*f.env.broadcasts.lock().unwrap(), 1);
        assert_eq!(report.plugin, PluginAction::UpToDate);
        assert_eq!(report.claude, ClaudeAction::NotNeeded);
        assert_eq!(f.claude.calls.lock().unwrap().len(), calls);
    }

    #[test]
    fn given_newer_bundled_binary_should_replace_installed_one() {
        let f = Fixture::new();
        f.installer().run();
        f.bundle_newer("pu.exe", "pu.exe v2");

        let report = f.installer().run();

        assert_eq!(
            actions(&report),
            [BinaryAction::Updated, BinaryAction::UpToDate]
        );
        assert_eq!(fs::read_to_string(f.bin("pu.exe")).unwrap(), "pu.exe v2");
        // No temporary file is left beside it.
        let names: Vec<_> = fs::read_dir(f.home.join(".pu").join("bin"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn given_installed_binary_newer_than_bundle_should_keep_it() {
        let f = Fixture::new();
        f.installer().run();
        fs::write(f.bin("pu.exe"), "locally built").unwrap();
        let report = f.installer().run();
        assert_eq!(report.binaries[0].action, BinaryAction::UpToDate);
        assert_eq!(
            fs::read_to_string(f.bin("pu.exe")).unwrap(),
            "locally built"
        );
    }

    #[test]
    fn given_nothing_bundled_should_skip_everything() {
        let f = Fixture::new();
        let installer = Installer {
            sources: vec![f.home.join("nowhere")],
            ..f.installer()
        };
        let report = installer.run();
        assert_eq!(
            actions(&report),
            [BinaryAction::NotBundled, BinaryAction::NotBundled]
        );
        assert!(!report.path_added);
        assert_eq!(*f.env.writes.lock().unwrap(), 0);
        assert_eq!(report.plugin, PluginAction::NotBundled);
        assert!(f.claude.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn given_no_claude_cli_should_install_plugin_and_report_not_found() {
        let mut f = Fixture::new();
        f.claude.found = None;
        let report = f.installer().run();
        assert_eq!(report.plugin, PluginAction::Installed);
        assert_eq!(report.claude, ClaudeAction::NotFound);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn given_newer_plugin_should_replace_marketplace_copy() {
        let f = Fixture::new();
        f.installer().run();
        let plugin = f
            .home
            .join(".pu")
            .join("marketplace")
            .join("plugins")
            .join("pu");
        fs::write(plugin.join("leftover.md"), "old").unwrap();
        let sentinel = f
            .bundle
            .join(PLUGIN_RESOURCE)
            .join(".claude-plugin")
            .join("plugin.json");
        fs::File::options()
            .write(true)
            .open(&sentinel)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(3600))
            .unwrap();

        let report = f.installer().run();

        assert_eq!(report.plugin, PluginAction::Installed);
        assert!(!plugin.join("leftover.md").exists());
        assert_eq!(f.claude.calls.lock().unwrap().len(), 4);
    }

    #[test]
    fn given_stale_names_should_match_only_installer_leftovers() {
        assert!(is_stale("pu-engine.exe.old-1"));
        assert!(is_stale("pu.exe.old-12"));
        assert!(is_stale(".pu.exe.tmp.4242"));
        assert!(!is_stale("pu.exe"));
        assert!(!is_stale("pu.exe.old-"));
        assert!(!is_stale("pu.exe.old-x"));
        assert!(!is_stale("other.exe.old-1"));
    }

    /// Windows lets a running image's file be renamed but not overwritten or deleted. A
    /// copy of `cmd.exe` installed as `pu-engine.exe` and kept running stands in for the
    /// daemon.
    #[cfg(windows)]
    #[test]
    fn given_running_engine_should_rename_it_aside_replace_it_and_clean_up_later() {
        use std::process::{Command, Stdio};

        let f = Fixture::new();
        let engine = f.bin("pu-engine.exe");
        fs::create_dir_all(engine.parent().unwrap()).unwrap();
        let system = std::env::var_os("SystemRoot").unwrap();
        let cmd = Path::new(&system).join("System32").join("cmd.exe");
        fs::copy(&cmd, &engine).unwrap();
        // Older than the bundle, so the install wants to replace it.
        fs::File::options()
            .write(true)
            .open(&engine)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
        let running = Command::new(&engine)
            .arg("/k")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut running = scopeguard(running);

        // What Windows allows on a running image's file: no overwrite, no delete.
        let probe = engine.with_file_name("probe.tmp");
        fs::write(&probe, "x").unwrap();
        assert!(
            fs::rename(&probe, &engine).is_err(),
            "rename over a running exe"
        );
        assert!(
            fs::copy(&probe, &engine).is_err(),
            "overwrite a running exe"
        );
        assert!(fs::remove_file(&engine).is_err(), "delete a running exe");
        fs::remove_file(&probe).unwrap();

        let report = f.installer().run();

        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(
            actions(&report),
            [BinaryAction::Installed, BinaryAction::ReplacedInUse]
        );
        assert!(report.restart_daemon);
        assert_eq!(
            fs::read_to_string(&engine).unwrap(),
            "pu-engine.exe v1",
            "new binary in place"
        );
        let aside = engine.with_file_name("pu-engine.exe.old-1");
        assert!(aside.is_file(), "old binary renamed aside");

        // Still running: the next launch cannot delete it yet and keeps it.
        let report = f.installer().run();
        assert_eq!((report.stale_removed, report.stale_kept), (0, 1));
        assert!(aside.is_file());

        running.0.kill().unwrap();
        running.0.wait().unwrap();
        let report = f.installer().run();
        assert_eq!((report.stale_removed, report.stale_kept), (1, 0));
        assert!(!aside.exists());
        assert_eq!(
            actions(&report),
            [BinaryAction::UpToDate, BinaryAction::UpToDate]
        );
    }

    #[test]
    fn given_existing_aside_files_should_pick_next_number() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("pu-engine.exe");
        assert_eq!(aside_path(&target), tmp.path().join("pu-engine.exe.old-1"));
        fs::write(tmp.path().join("pu-engine.exe.old-1"), "").unwrap();
        assert_eq!(aside_path(&target), tmp.path().join("pu-engine.exe.old-2"));
    }

    /// Kills the child if a test assertion fails first.
    #[cfg(windows)]
    struct Guard(std::process::Child);

    #[cfg(windows)]
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[cfg(windows)]
    fn scopeguard(child: std::process::Child) -> Guard {
        Guard(child)
    }
}
