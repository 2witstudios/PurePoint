//! Optional OS supervision for the standalone daemon: a systemd user unit on
//! Linux, a launchd agent on macOS. Supervised daemons restart after a crash
//! and start at boot (Linux: with lingering enabled), so a server keeps its
//! daemon without anyone logged in. Windows has a per-user `HKCU` Run value
//! that starts the daemon at logon (WINAPP-011): no restart on failure, and
//! no elevation needed.
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
    #[cfg(windows)]
    RunKey,
}

impl Manager {
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Self::Systemd)
        } else if cfg!(target_os = "macos") {
            Some(Self::Launchd)
        } else {
            Self::other_platform()
        }
    }

    #[cfg(windows)]
    fn other_platform() -> Option<Self> {
        Some(Self::RunKey)
    }

    #[cfg(not(windows))]
    fn other_platform() -> Option<Self> {
        None
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd user unit",
            Self::Launchd => "launchd agent",
            #[cfg(windows)]
            Self::RunKey => "logon Run entry",
        }
    }

    pub fn file_path(self) -> Result<PathBuf, CliError> {
        #[cfg(windows)]
        if self == Self::RunKey {
            return Ok(PathBuf::from(run_key::DISPLAY));
        }
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
            #[cfg(windows)]
            Self::RunKey => unreachable!("returned above"),
        })
    }
}

fn home() -> Result<PathBuf, CliError> {
    pu_core::paths::home_dir().map_err(|_| CliError::Other("HOME is not set".into()))
}

/// The supervisor whose service file is installed, if any.
pub fn installed() -> Option<Manager> {
    let manager = Manager::current()?;
    #[cfg(windows)]
    if manager == Manager::RunKey {
        return run_key::read().ok().flatten().map(|_| manager);
    }
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

/// Only the launchd and systemd managers key on a uid.
#[cfg(windows)]
fn uid() -> u32 {
    0
}

#[cfg(unix)]
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
        // The Run value only fires at logon; start what it would start.
        #[cfg(windows)]
        Manager::RunKey => {
            crate::daemon_ctrl::spawn_detached_daemon(&pu_core::paths::daemon_socket_path()?)
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
        // Nothing supervises the daemon after logon; Shutdown stops it.
        #[cfg(windows)]
        Manager::RunKey => Ok(false),
    }
}

/// Write the service file for `engine` and enable it. Returns notes to print.
pub fn install(
    manager: Manager,
    engine: &Path,
    socket: &Path,
    now: bool,
) -> Result<Vec<String>, CliError> {
    // The Run value always serves the default socket (`socket` is it).
    #[cfg(windows)]
    if manager == Manager::RunKey {
        run_key::write(&run_key::command_for(engine))?;
        let mut notes = vec![format!("wrote {}", run_key::DISPLAY)];
        if now {
            start(manager)?;
        }
        if run_key::disabled_in_startup_apps() {
            notes.push(
                "the entry is disabled in Settings > Apps > Startup; enable it there for it to run at logon".into(),
            );
        }
        return Ok(notes);
    }
    let home = home()?;
    let path_env = service_path_env(std::env::var("PATH").ok().as_deref(), &home);
    let file = manager.file_path()?;
    let content = match manager {
        Manager::Systemd => systemd_unit(engine, socket, &path_env),
        Manager::Launchd => {
            let log = pu_core::paths::daemon_log_path()?;
            launchd_plist(engine, socket, &path_env, &log)
        }
        #[cfg(windows)]
        Manager::RunKey => unreachable!("installed above"),
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
        #[cfg(windows)]
        Manager::RunKey => unreachable!("installed above"),
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
    #[cfg(windows)]
    if manager == Manager::RunKey {
        return Ok(run_key::delete()?.then(|| PathBuf::from(run_key::DISPLAY)));
    }
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
        #[cfg(windows)]
        Manager::RunKey => unreachable!("removed above"),
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
        #[cfg(windows)]
        Manager::RunKey => {
            return match run_key::read() {
                Ok(None) => "not registered".into(),
                Err(e) => format!("unknown ({e})"),
                Ok(Some(_)) if run_key::disabled_in_startup_apps() => {
                    "registered, disabled in Startup apps".into()
                }
                Ok(Some(_)) => "registered (starts at logon)".into(),
            };
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

/// The `HKCU\...\CurrentVersion\Run` value that starts the daemon at logon.
#[cfg(windows)]
pub(crate) mod run_key {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const STARTUP_APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    pub(crate) const VALUE: &str = "PurePoint Engine";
    pub(crate) const DISPLAY: &str =
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run\PurePoint Engine";

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn check(rc: u32) -> io::Result<()> {
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(rc as i32))
        }
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: an open key we own.
            unsafe { RegCloseKey(self.0) };
        }
    }

    /// `None` when the key does not exist.
    fn open(path: &str, access: u32) -> io::Result<Option<Key>> {
        let mut key: HKEY = null_mut();
        let path = wide(path);
        // SAFETY: path is NUL-terminated; key receives a handle closed by Key.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(rc)?;
        Ok(Some(Key(key)))
    }

    /// Raw bytes of `value` under `path`; `None` when either is missing.
    fn query(path: &str, value: &str) -> io::Result<Option<Vec<u8>>> {
        let Some(key) = open(path, KEY_QUERY_VALUE)? else {
            return Ok(None);
        };
        let name = wide(value);
        let mut len = 0u32;
        // SAFETY: a size query with no buffer.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                null_mut(),
                null_mut(),
                &mut len,
            )
        };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(rc)?;
        let mut buf = vec![0u8; len as usize];
        // SAFETY: buf holds len bytes.
        check(unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                null_mut(),
                buf.as_mut_ptr(),
                &mut len,
            )
        })?;
        buf.truncate(len as usize);
        Ok(Some(buf))
    }

    /// The registered command line, if the value exists.
    pub(crate) fn read() -> io::Result<Option<String>> {
        Ok(query(RUN, VALUE)?.map(|bytes| {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            String::from_utf16_lossy(&units)
        }))
    }

    pub(crate) fn write(command: &str) -> io::Result<()> {
        let mut key: HKEY = null_mut();
        let path = wide(RUN);
        // SAFETY: path is NUL-terminated; key receives a handle closed by Key.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut key,
                null_mut(),
            )
        })?;
        let key = Key(key);
        let name = wide(VALUE);
        let data = wide(command);
        // SAFETY: data is a NUL-terminated wide string of the given byte size.
        check(unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        })
    }

    /// Delete the value. False when it was not there.
    pub(crate) fn delete() -> io::Result<bool> {
        let Some(key) = open(RUN, KEY_SET_VALUE)? else {
            return Ok(false);
        };
        let name = wide(VALUE);
        // SAFETY: an open key and a NUL-terminated name.
        let rc = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        check(rc).map(|()| true)
    }

    /// Settings > Startup apps records its toggle as a REG_BINARY whose
    /// first byte is even when enabled (2) and odd when disabled (3).
    pub(crate) fn disabled_in_startup_apps() -> bool {
        matches!(
            query(STARTUP_APPROVED, VALUE),
            Ok(Some(bytes)) if bytes.first().is_some_and(|b| b % 2 == 1)
        )
    }

    /// The value's data: the engine path, quoted, without the `\\?\` prefix
    /// `canonicalize` adds.
    pub(crate) fn command_for(engine: &Path) -> String {
        let path = engine.to_string_lossy();
        let path = match path.strip_prefix(r"\\?\") {
            Some(rest) if !rest.starts_with("UNC") => rest,
            _ => &path,
        };
        format!("\"{path}\"")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn given_canonical_engine_path_should_quote_it_without_verbatim_prefix() {
            assert_eq!(
                command_for(Path::new(r"\\?\C:\Users\a b\.pu\bin\pu-engine.exe")),
                r#""C:\Users\a b\.pu\bin\pu-engine.exe""#
            );
            assert_eq!(
                command_for(Path::new(r"C:\x\pu-engine.exe")),
                r#""C:\x\pu-engine.exe""#
            );
        }
    }
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
