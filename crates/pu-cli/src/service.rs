//! Optional OS supervision for the standalone daemon: a systemd user unit on
//! Linux, a launchd agent on macOS. Supervised daemons restart after a crash
//! and start at boot (Linux: with lingering enabled), so a server keeps its
//! daemon without anyone logged in.
//!
//! Only the default socket is supervised; custom sockets always use a bare
//! detached spawn.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CliError;

pub const SYSTEMD_UNIT: &str = "pu-engine.service";
pub const LAUNCHD_LABEL: &str = "dev.purepoint.engine";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Systemd,
    Launchd,
}

impl Manager {
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Self::Systemd)
        } else if cfg!(target_os = "macos") {
            Some(Self::Launchd)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd user unit",
            Self::Launchd => "launchd agent",
        }
    }

    pub fn file_path(self) -> Result<PathBuf, CliError> {
        let home = home()?;
        Ok(match self {
            Self::Systemd => std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".config"))
                .join("systemd/user")
                .join(SYSTEMD_UNIT),
            Self::Launchd => home
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCHD_LABEL}.plist")),
        })
    }
}

fn home() -> Result<PathBuf, CliError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Other("HOME is not set".into()))
}

/// The supervisor whose service file is installed, if any.
pub fn installed() -> Option<Manager> {
    let manager = Manager::current()?;
    manager.file_path().ok()?.exists().then_some(manager)
}

/// systemd quoting for one ExecStart/Environment word: `%` is a specifier and
/// `"`/`\` are quote metacharacters.
fn systemd_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// `KillMode=mixed`: stop sends SIGTERM to the daemon alone, so it suspends its
/// agents resumably; anything still alive after the timeout is SIGKILLed.
/// `Restart=on-failure`: `pu daemon stop` (exit 0) stays stopped, crashes don't.
pub fn systemd_unit(engine: &Path, socket: &Path, path_env: &str) -> String {
    format!(
        "\
[Unit]
Description=PurePoint daemon (pu-engine)
Documentation=https://github.com/2witstudios/purepoint

[Service]
Type=simple
ExecStart={engine} --socket {socket}
Environment={path}
Restart=on-failure
RestartSec=2
KillMode=mixed
TimeoutStopSec=15

[Install]
WantedBy=default.target
",
        engine = systemd_quote(&engine.to_string_lossy()),
        socket = systemd_quote(&socket.to_string_lossy()),
        path = systemd_quote(&format!("PATH={path_env}")),
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `KeepAlive.SuccessfulExit=false` restarts crashes but not a clean stop.
pub fn launchd_plist(engine: &Path, socket: &Path, path_env: &str, log: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{engine}</string>
    <string>--socket</string>
    <string>{socket}</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#,
        engine = xml_escape(&engine.to_string_lossy()),
        socket = xml_escape(&socket.to_string_lossy()),
        path = xml_escape(path_env),
        log = xml_escape(&log.to_string_lossy()),
    )
}

/// PATH for the supervised daemon: the installing shell's PATH plus the
/// directories PurePoint and agent CLIs are usually installed into, which a
/// non-interactive SSH session's PATH lacks. The daemon still probes the login
/// shell for agent environments; this only has to find `git`, `sh` and friends.
pub fn service_path_env(current: Option<&str>, home: &Path) -> String {
    let mut dirs: Vec<String> = Vec::new();
    let mut push = |dir: String| {
        if !dir.is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };
    for dir in [".pu/bin", ".local/bin", ".cargo/bin"] {
        push(home.join(dir).to_string_lossy().into_owned());
    }
    for dir in current.unwrap_or_default().split(':') {
        push(dir.to_string());
    }
    for dir in [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        push(dir.to_string());
    }
    dirs.join(":")
}

fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

fn run(program: &str, args: &[&str]) -> Result<(), CliError> {
    let output = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| CliError::Other(format!("failed to run {program}: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(CliError::Other(format!(
        "`{program} {}` failed: {}",
        args.join(" "),
        stderr.trim()
    )))
}

/// launchd domains to try: the GUI session when someone is logged in at the
/// console, otherwise the per-user background domain (headless/SSH-only Macs).
fn launchd_domains() -> [String; 2] {
    [format!("gui/{}", uid()), format!("user/{}", uid())]
}

/// Ask the supervisor to start its daemon. Errors mean the supervisor is not
/// usable from here (e.g. no systemd user bus); callers fall back to a spawn.
pub fn start(manager: Manager) -> Result<(), CliError> {
    match manager {
        Manager::Systemd => run("systemctl", &["--user", "start", SYSTEMD_UNIT]),
        Manager::Launchd => {
            let mut last = None;
            for domain in launchd_domains() {
                let target = format!("{domain}/{LAUNCHD_LABEL}");
                match run("launchctl", &["kickstart", &target]) {
                    Ok(()) => return Ok(()),
                    Err(e) => last = Some(e),
                }
            }
            Err(last.unwrap_or_else(|| CliError::Other("launchctl kickstart failed".into())))
        }
    }
}

/// Stop through the supervisor where that differs from a clean Shutdown.
/// Returns false when the caller should send `Shutdown` itself: launchd's
/// `KeepAlive.SuccessfulExit=false` already leaves a clean exit stopped.
pub fn stop(manager: Manager) -> Result<bool, CliError> {
    match manager {
        Manager::Systemd => run("systemctl", &["--user", "stop", SYSTEMD_UNIT]).map(|()| true),
        Manager::Launchd => Ok(false),
    }
}

/// Write the service file for `engine` and enable it. Returns notes to print.
pub fn install(
    manager: Manager,
    engine: &Path,
    socket: &Path,
    now: bool,
) -> Result<Vec<String>, CliError> {
    let home = home()?;
    let path_env = service_path_env(std::env::var("PATH").ok().as_deref(), &home);
    let file = manager.file_path()?;
    let content = match manager {
        Manager::Systemd => systemd_unit(engine, socket, &path_env),
        Manager::Launchd => {
            let log = pu_core::paths::daemon_log_path()?;
            launchd_plist(engine, socket, &path_env, &log)
        }
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file, content)?;

    let mut notes = vec![format!("wrote {}", file.display())];
    match manager {
        Manager::Systemd => {
            run("systemctl", &["--user", "daemon-reload"])?;
            run("systemctl", &["--user", "enable", SYSTEMD_UNIT])?;
            if now {
                run("systemctl", &["--user", "start", SYSTEMD_UNIT])?;
            }
            if !linger_enabled() {
                notes.push(format!(
                    "lingering is off: the daemon stops when your last session ends and won't start at boot. Enable it with `sudo loginctl enable-linger {}`",
                    user_name()
                ));
            }
        }
        Manager::Launchd => {
            let file_str = file.to_string_lossy().into_owned();
            let mut loaded = Err(CliError::Other("launchctl bootstrap failed".into()));
            for domain in launchd_domains() {
                // Re-installing: drop the old definition first (ignore "not loaded").
                let _ = run(
                    "launchctl",
                    &["bootout", &format!("{domain}/{LAUNCHD_LABEL}")],
                );
                loaded = run("launchctl", &["bootstrap", &domain, &file_str]);
                if loaded.is_ok() {
                    break;
                }
            }
            loaded?;
            notes.push(
                "the PurePoint app manages its own daemon; on a Mac you also use the app on, prefer the app".into(),
            );
        }
    }
    Ok(notes)
}

pub fn uninstall(manager: Manager) -> Result<Option<PathBuf>, CliError> {
    let file = manager.file_path()?;
    if !file.exists() {
        return Ok(None);
    }
    match manager {
        Manager::Systemd => {
            let _ = run("systemctl", &["--user", "disable", "--now", SYSTEMD_UNIT]);
            std::fs::remove_file(&file)?;
            let _ = run("systemctl", &["--user", "daemon-reload"]);
        }
        Manager::Launchd => {
            for domain in launchd_domains() {
                let _ = run(
                    "launchctl",
                    &["bootout", &format!("{domain}/{LAUNCHD_LABEL}")],
                );
            }
            std::fs::remove_file(&file)?;
        }
    }
    Ok(Some(file))
}

/// Human-readable supervisor state, e.g. `active` or `inactive`.
pub fn state(manager: Manager) -> String {
    let output = match manager {
        Manager::Systemd => Command::new("systemctl")
            .args(["--user", "is-active", SYSTEMD_UNIT])
            .stdin(std::process::Stdio::null())
            .output(),
        Manager::Launchd => {
            let loaded = launchd_domains().iter().any(|domain| {
                Command::new("launchctl")
                    .args(["print", &format!("{domain}/{LAUNCHD_LABEL}")])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            });
            return if loaded { "loaded" } else { "not loaded" }.to_string();
        }
    };
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() {
                "unknown".into()
            } else {
                text
            }
        }
        Err(_) => "unknown (systemctl unavailable)".into(),
    }
}

fn linger_enabled() -> bool {
    Command::new("loginctl")
        .args([
            "show-user",
            &uid().to_string(),
            "--property=Linger",
            "--value",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "yes")
}

fn user_name() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "$USER".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_paths_with_specials_should_quote_systemd_unit() {
        let unit = systemd_unit(
            Path::new("/home/a b/.pu/bin/pu-engine"),
            Path::new("/home/a b/.pu/daemon.sock"),
            "/usr/bin:/odd%dir",
        );
        assert!(
            unit.contains(
                "ExecStart=\"/home/a b/.pu/bin/pu-engine\" --socket \"/home/a b/.pu/daemon.sock\"\n"
            ),
            "{unit}"
        );
        assert!(
            unit.contains("Environment=\"PATH=/usr/bin:/odd%%dir\"\n"),
            "{unit}"
        );
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("KillMode=mixed\n"));
        assert!(unit.contains("WantedBy=default.target\n"));
    }

    #[test]
    fn given_quote_or_backslash_should_escape_systemd_word() {
        assert_eq!(systemd_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn given_paths_should_render_launchd_plist_with_escaping() {
        let plist = launchd_plist(
            Path::new("/Users/me/.pu/bin/pu-engine"),
            Path::new("/Users/me/.pu/daemon.sock"),
            "/usr/bin:/a&b",
            Path::new("/Users/me/.pu/daemon.log"),
        );
        assert!(plist.contains(&format!("<string>{LAUNCHD_LABEL}</string>")));
        assert!(plist.contains("<string>/Users/me/.pu/bin/pu-engine</string>"));
        assert!(plist.contains("<string>/usr/bin:/a&amp;b</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
    }

    #[test]
    fn given_minimal_path_should_add_install_dirs_without_duplicates() {
        let path = service_path_env(
            Some("/usr/bin:/bin:/home/me/.pu/bin"),
            Path::new("/home/me"),
        );
        let dirs: Vec<_> = path.split(':').collect();
        assert_eq!(
            &dirs[..3],
            [
                "/home/me/.pu/bin",
                "/home/me/.local/bin",
                "/home/me/.cargo/bin"
            ]
        );
        assert!(dirs.contains(&"/usr/local/bin"));
        let mut unique = dirs.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), dirs.len(), "{path}");
    }

    #[test]
    fn given_no_path_should_still_include_system_dirs() {
        let path = service_path_env(None, Path::new("/root"));
        assert!(path.split(':').any(|d| d == "/usr/bin"));
        assert!(!path.contains("::"));
    }
}
