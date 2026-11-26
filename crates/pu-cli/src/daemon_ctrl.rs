use std::path::{Path, PathBuf};

use pu_core::paths;
use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};

use crate::error::CliError;

#[cfg_attr(windows, allow(dead_code))]
const DAEMON_BINARY: &str = "pu-engine";
/// Names the one `pu-engine` to start: a path, or a name looked up on `PATH`.
const ENGINE_PATH_ENV: &str = "PU_ENGINE_PATH";

/// The `pu-engine` this `pu` may auto-start. A `pu` inside an app bundle
/// starts only the bundle's own engine. Otherwise `PU_ENGINE_PATH` names the
/// only candidate when set; if not, the engine next to `pu` (where every
/// install puts both: the macOS app copies its engine beside `~/.pu/bin/pu`),
/// then `~/.pu/bin`. `PATH` is used only through `PU_ENGINE_PATH`. A candidate
/// must pass [`check_trusted_binary`]; the error names why each one failed.
pub fn find_daemon_binary() -> Result<PathBuf, String> {
    // Resolve symlinks so a linked `pu` still finds its real neighbors.
    let exe = std::env::current_exe()
        .and_then(pu_core::canon::canonicalize)
        .ok();
    let home = pu_core::paths::home_dir().ok();
    let explicit = std::env::var_os(ENGINE_PATH_ENV).filter(|value| !value.is_empty());
    find_daemon_binary_in(exe.as_deref(), home.as_deref(), explicit.as_deref())
}

/// `<x>.app/Contents` when `exe_dir` is that bundle's `Helpers` or `MacOS`.
#[cfg_attr(windows, allow(dead_code))]
fn bundle_contents(exe_dir: &Path) -> Option<&Path> {
    let name = exe_dir.file_name()?;
    let contents = exe_dir.parent()?;
    let is_bundle = (name == "Helpers" || name == "MacOS")
        && contents.file_name()? == "Contents"
        && contents.parent()?.extension()? == "app";
    is_bundle.then_some(contents)
}

/// The engine executable's file name on Windows.
#[cfg(windows)]
const DAEMON_EXE: &str = "pu-engine.exe";

/// Windows: the same order, without the app-bundle case: `PU_ENGINE_PATH`
/// alone when set, else `pu-engine.exe` beside `pu.exe`, then
/// `%USERPROFILE%\.pu\bin`. Trust is the file's owner and DACL
/// ([`pu_core::win_security::check_trusted_binary`]).
#[cfg(windows)]
fn find_daemon_binary_in(
    exe: Option<&Path>,
    home: Option<&Path>,
    explicit: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    let candidates = if let Some(explicit) = explicit {
        match which::which(explicit) {
            Ok(path) => vec![path],
            Err(e) => {
                return Err(format!(
                    "{ENGINE_PATH_ENV}={}: {e}",
                    explicit.to_string_lossy()
                ));
            }
        }
    } else {
        exe.and_then(Path::parent)
            .map(|dir| dir.join(DAEMON_EXE))
            .into_iter()
            .chain(home.map(|home| home.join(".pu").join("bin").join(DAEMON_EXE)))
            .collect()
    };
    let mut reasons = Vec::new();
    for path in candidates {
        match pu_core::win_security::check_trusted_binary(&path) {
            Ok(()) => return Ok(path),
            Err(reason) => reasons.push(format!("{}: {reason}", path.display())),
        }
    }
    if reasons.is_empty() {
        reasons.push("no candidate locations".into());
    }
    Err(reasons.join("; "))
}

#[cfg(unix)]
fn find_daemon_binary_in(
    exe: Option<&Path>,
    home: Option<&Path>,
    explicit: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    let exe_dir = exe.and_then(Path::parent);
    let bundle = exe_dir.and_then(bundle_contents);
    let exe_owner = exe
        .and_then(|exe| std::fs::metadata(exe).ok())
        .map(|m| m.uid());
    // SAFETY: geteuid has no preconditions and cannot fail.
    let owners = allowed_owners(unsafe { libc::geteuid() }, bundle.is_some(), exe_owner);
    let candidates = if let Some(contents) = bundle {
        vec![contents.join("MacOS").join(DAEMON_BINARY)]
    } else if let Some(explicit) = explicit {
        match which::which(explicit) {
            Ok(path) => vec![path],
            Err(e) => {
                return Err(format!(
                    "{ENGINE_PATH_ENV}={}: {e}",
                    explicit.to_string_lossy()
                ));
            }
        }
    } else {
        exe_dir
            .map(|dir| dir.join(DAEMON_BINARY))
            .into_iter()
            .chain(home.map(|home| home.join(".pu/bin").join(DAEMON_BINARY)))
            .collect()
    };
    let mut reasons = Vec::new();
    for path in candidates {
        match check_trusted_binary(&path, &owners) {
            Ok(()) => return Ok(path),
            Err(reason) => reasons.push(format!("{}: {reason}", path.display())),
        }
    }
    if reasons.is_empty() {
        reasons.push("no candidate locations".into());
    }
    Err(reasons.join("; "))
}

/// Users whose engine binaries may be started: this user and root, plus, in
/// an app bundle, the owner of the running `pu`. An app installed by another
/// account is owned by that account, and its engine is as trustworthy as the
/// `pu` running now.
#[cfg_attr(windows, allow(dead_code))]
fn allowed_owners(euid: u32, in_bundle: bool, exe_owner: Option<u32>) -> Vec<u32> {
    let mut owners = vec![euid, 0];
    owners.extend(exe_owner.filter(|_| in_bundle));
    owners
}

/// An executable file owned by an allowed user that no other user can
/// modify: anyone who could rewrite it would run code as this user.
#[cfg(unix)]
fn check_trusted_binary(path: &Path, owners: &[u32]) -> Result<(), &'static str> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::metadata(path).map_err(|_| "not found")?;
    if !meta.is_file() {
        return Err("not a file");
    }
    trusted_mode_and_owner(meta.permissions().mode(), meta.uid(), owners)
}

#[cfg_attr(windows, allow(dead_code))]
fn trusted_mode_and_owner(mode: u32, owner: u32, owners: &[u32]) -> Result<(), &'static str> {
    if mode & 0o111 == 0 {
        Err("not executable")
    } else if mode & 0o022 != 0 {
        Err("writable by group or others")
    } else if !owners.contains(&owner) {
        Err("owned by another user")
    } else {
        Ok(())
    }
}

/// Spawn the daemon detached from the caller's session: its own session (so
/// an SSH hangup or the caller's process group dying does not reach it) and no
/// inherited stdio (so `ssh host 'pu health'` returns instead of waiting on
/// the daemon's open descriptors).
#[cfg(unix)]
fn daemon_command(
    binary: &Path,
    socket: &Path,
    stderr: std::process::Stdio,
) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(binary);
    command
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr);
    // SAFETY: setsid is async-signal-safe and touches no parent state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

/// Start the installed supervisor's daemon for the default socket. Returns
/// false (spawn it directly) when no service is installed or the supervisor
/// cannot be reached, e.g. no systemd user bus in this session.
fn start_supervised(socket: &Path) -> bool {
    let is_default = paths::daemon_socket_path().is_ok_and(|default| default == socket);
    let Some(manager) = is_default.then(crate::service::installed).flatten() else {
        return false;
    };
    match crate::service::start(manager) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "warning: {} is installed but could not be started ({e}); starting pu-engine directly",
                manager.name()
            );
            false
        }
    }
}

/// Longest a single health probe may take. A daemon that accepts but never
/// answers would otherwise hold each probe for the full request timeout.
const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// What a single health probe learned about the daemon behind a socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthProbe {
    Healthy,
    /// Nothing answered (no socket, refused, closed or timed out).
    NotAnswering,
    Incompatible {
        protocol_version: u32,
    },
    /// Alive, but at its connection limit; compatibility is unknown.
    Busy {
        message: String,
    },
}

pub async fn probe_daemon_health(socket: &Path) -> HealthProbe {
    let probe = crate::client::send_request(socket, &Request::Health);
    // A relayed probe pays for an ssh handshake first.
    #[cfg(windows)]
    let limit = if crate::machine_target::relay_for(socket).is_some() {
        crate::machine_target::RELAY_HEALTH_TIMEOUT
    } else {
        HEALTH_PROBE_TIMEOUT
    };
    #[cfg(unix)]
    let limit = HEALTH_PROBE_TIMEOUT;
    match tokio::time::timeout(limit, probe).await {
        Ok(Ok(Response::HealthReport {
            protocol_version, ..
        })) if protocol_version == PROTOCOL_VERSION => HealthProbe::Healthy,
        Ok(Ok(Response::HealthReport {
            protocol_version, ..
        })) => HealthProbe::Incompatible { protocol_version },
        Ok(Ok(Response::Error { code, message })) if code == "BUSY" => {
            HealthProbe::Busy { message }
        }
        _ => HealthProbe::NotAnswering,
    }
}

/// Check compatibility before any command; never restart a live daemon because
/// doing so would terminate its agent sessions.
pub async fn check_daemon_health(socket: &Path) -> Result<bool, CliError> {
    match probe_daemon_health(socket).await {
        HealthProbe::Healthy => Ok(true),
        HealthProbe::NotAnswering => Ok(false),
        HealthProbe::Incompatible { protocol_version } => Err(CliError::Other(format!(
            "daemon protocol v{protocol_version} is incompatible with CLI protocol v{PROTOCOL_VERSION} at {} — update pu and pu-engine together and restart the daemon after saving active agent work",
            socket.display()
        ))),
        // BUSY establishes liveness, but cannot establish protocol compatibility.
        HealthProbe::Busy { message } => Err(CliError::DaemonError {
            code: "BUSY".into(),
            message,
        }),
    }
}

/// Probe up to three times, 200 ms apart, so a busy or restarting daemon
/// that misses one check isn't reported as not answering.
pub async fn probe_daemon_health_with_retries(socket: &Path) -> HealthProbe {
    let mut probe = HealthProbe::NotAnswering;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        probe = probe_daemon_health(socket).await;
        if !matches!(probe, HealthProbe::NotAnswering) {
            break;
        }
    }
    probe
}

/// A machine's daemon is never auto-started (MACH-003): only check it.
pub async fn ensure_remote_daemon(socket: &Path, machine: &str) -> Result<(), CliError> {
    match probe_daemon_health_with_retries(socket).await {
        HealthProbe::Healthy => Ok(()),
        HealthProbe::NotAnswering => Err(CliError::Other(format!(
            "daemon not running on {machine}; start it with `pu machine start {machine}`"
        ))),
        HealthProbe::Incompatible { protocol_version } => Err(CliError::Other(format!(
            "daemon on {machine} speaks protocol v{protocol_version}, but this CLI speaks v{PROTOCOL_VERSION} — update PurePoint on both machines"
        ))),
        HealthProbe::Busy { message } => Err(CliError::DaemonError {
            code: "BUSY".into(),
            message: format!("daemon on {machine}: {message}"),
        }),
    }
}

pub async fn ensure_daemon(socket: &Path) -> Result<(), CliError> {
    if let Some(machine) = crate::machine_target::remote_machine() {
        return ensure_remote_daemon(socket, machine).await;
    }
    // Retry before concluding there is no daemon: a busy or restarting daemon
    // can miss one check. (A daemon started needlessly exits on its own, since
    // pu-engine holds a single-instance lock, but the retry avoids the churn.)
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        if check_daemon_health(socket).await? {
            return Ok(());
        }
    }

    // A supervisor accepting the start does not establish that its daemon
    // became healthy: the supervised daemon can exit before answering (e.g. a
    // directly started daemon still holds the single-instance lock), so fall
    // back to the direct spawn before giving up.
    let supervised = start_supervised(socket);
    if !supervised {
        spawn_detached_daemon(socket)?;
    }
    if wait_for_healthy(socket).await?.is_some() {
        return Ok(());
    }
    if supervised {
        spawn_detached_daemon(socket)?;
        if wait_for_healthy(socket).await?.is_some() {
            return Ok(());
        }
    }
    Err(CliError::Other(
        "daemon did not start within 3 seconds".into(),
    ))
}

/// Poll health until the daemon answers or the budget runs out, with
/// exponential backoff: 10, 20, 40, 80, 160, 320, 640ms. The 3s budget is
/// wall-clock and includes the probes themselves, which can each take up to
/// HEALTH_PROBE_TIMEOUT against a daemon that never answers. Returns
/// `Ok(Some(()))` once healthy, `Ok(None)` if the budget ran out, and
/// propagates probe errors (incompatible protocol, BUSY) immediately.
async fn wait_for_healthy(socket: &Path) -> Result<Option<()>, CliError> {
    let poll = async {
        let mut delay_ms = 10u64;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            if check_daemon_health(socket).await? {
                return Ok::<(), CliError>(());
            }
            delay_ms = (delay_ms * 2).min(640);
        }
    };
    match tokio::time::timeout(std::time::Duration::from_secs(3), poll).await {
        Ok(result) => result.map(Some),
        Err(_) => Ok(None),
    }
}

pub(crate) fn spawn_detached_daemon(socket: &Path) -> Result<(), CliError> {
    let binary = find_daemon_binary().map_err(|reasons| CliError::Other(format!(
        "no usable pu-engine ({reasons}) — install PurePoint (see docs/guide/linux-server.md), `cargo install --path crates/pu-engine`, or set PU_ENGINE_PATH"
    )))?;

    // Redirect daemon stderr to log file so startup errors are diagnosable
    let log = paths::daemon_log_path().ok().and_then(|log_path| {
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok()
    });

    // Start daemon
    #[cfg(unix)]
    {
        let stderr_target = log
            .map(std::process::Stdio::from)
            .unwrap_or_else(std::process::Stdio::null);
        daemon_command(&binary, socket, stderr_target)
            .spawn()
            .map_err(CliError::Io)?;
    }
    #[cfg(windows)]
    win_spawn::spawn_detached(&binary, socket, log).map_err(CliError::Io)?;
    Ok(())
}

/// Spawning the daemon detached on Windows (WINAPP-009, REQ-WINAPP-030).
#[cfg(windows)]
mod win_spawn {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
        CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, UpdateProcThreadAttribute,
    };

    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;

    /// One argument quoted for `CommandLineToArgvW` / the MSVC CRT.
    pub(super) fn quote_arg(arg: &OsStr, out: &mut Vec<u16>) {
        let wide: Vec<u16> = arg.encode_wide().collect();
        let needs_quotes = wide.is_empty()
            || wide
                .iter()
                .any(|&c| c == b' ' as u16 || c == b'\t' as u16 || c == QUOTE);
        if !needs_quotes {
            out.extend_from_slice(&wide);
            return;
        }
        out.push(QUOTE);
        let mut backslashes = 0;
        for &c in &wide {
            if c == BACKSLASH {
                backslashes += 1;
                continue;
            }
            let run = if c == QUOTE {
                backslashes * 2 + 1
            } else {
                backslashes
            };
            out.extend(std::iter::repeat_n(BACKSLASH, run));
            backslashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
        out.push(QUOTE);
    }

    fn wide_nul(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Start `binary --socket <socket>` with no console, in its own process
    /// group, out of the caller's job when the job allows it, with stdin and
    /// stdout on NUL and stderr on `log` (or NUL). Only those three handles
    /// are inherited (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`), so a pipe the
    /// caller inherited (an SSH session's stdout, say) never keeps the
    /// caller's session waiting on the daemon, and closing the terminal that
    /// ran `pu` does not reach it.
    pub(super) fn spawn_detached(
        binary: &Path,
        socket: &Path,
        log: Option<File>,
    ) -> std::io::Result<()> {
        let nul = || {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("NUL")
        };
        let stdin = nul()?;
        let stdout = nul()?;
        let stderr = match log {
            Some(file) => file,
            None => nul()?,
        };
        let handles: [HANDLE; 3] = [
            stdin.as_raw_handle(),
            stdout.as_raw_handle(),
            stderr.as_raw_handle(),
        ];
        for handle in handles {
            // SAFETY: our own open file handles, made inheritable only so the
            // handle list below may name them.
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }
                == 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }

        // The daemon outlives this shell: never hand it a relative socket
        // path, and never pin the caller's directory (Windows cannot delete
        // a directory that is some process's working directory).
        let socket = std::path::absolute(socket)?;
        let mut cmdline = Vec::new();
        quote_arg(binary.as_os_str(), &mut cmdline);
        cmdline.push(b' ' as u16);
        quote_arg(OsStr::new("--socket"), &mut cmdline);
        cmdline.push(b' ' as u16);
        quote_arg(socket.as_os_str(), &mut cmdline);
        cmdline.push(0);
        let application = wide_nul(binary.as_os_str());
        let cwd = pu_core::paths::home_dir()
            .ok()
            .filter(|home| home.is_dir())
            .map(|home| wide_nul(home.as_os_str()));

        // SAFETY: the attribute list is sized by the first call, lives in
        // `attr_buf` until DeleteProcThreadAttributeList, and every pointer
        // passed to CreateProcessW outlives the call.
        unsafe {
            let mut size = 0usize;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
            let mut attr_buf = vec![0u8; size];
            let attrs = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            if InitializeProcThreadAttributeList(attrs, 1, 0, &mut size) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let result = (|| {
                if UpdateProcThreadAttribute(
                    attrs,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handles.as_ptr() as *const _,
                    std::mem::size_of_val(&handles),
                    null_mut(),
                    null(),
                ) == 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                let mut startup: STARTUPINFOEXW = std::mem::zeroed();
                startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
                startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
                startup.StartupInfo.hStdInput = handles[0];
                startup.StartupInfo.hStdOutput = handles[1];
                startup.StartupInfo.hStdError = handles[2];
                startup.lpAttributeList = attrs;
                let base = DETACHED_PROCESS
                    | CREATE_NEW_PROCESS_GROUP
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT;
                // Leave the caller's job (an SSH session's, say) when it
                // allows breakaway; otherwise stay in it.
                let mut last = None;
                for flags in [base | CREATE_BREAKAWAY_FROM_JOB, base] {
                    let mut info: PROCESS_INFORMATION = std::mem::zeroed();
                    let mut line = cmdline.clone();
                    let created = CreateProcessW(
                        application.as_ptr(),
                        line.as_mut_ptr(),
                        null(),
                        null(),
                        1,
                        flags,
                        null(),
                        cwd.as_ref().map_or(null(), |c| c.as_ptr()),
                        &startup.StartupInfo,
                        &mut info,
                    );
                    if created != 0 {
                        CloseHandle(info.hThread);
                        CloseHandle(info.hProcess);
                        return Ok(());
                    }
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(ERROR_ACCESS_DENIED as i32) {
                        return Err(error);
                    }
                    last = Some(error);
                }
                Err(last.unwrap_or_else(|| std::io::Error::other("CreateProcessW failed")))
            })();
            DeleteProcThreadAttributeList(attrs);
            result
        }
    }
}

// Windows binary discovery, trust and the detached spawn.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn make_exe(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"MZ").unwrap();
        pu_core::win_security::set_private_dacl(path, false).unwrap();
    }

    fn find(exe_dir: &Path, home: Option<&Path>, explicit: Option<&Path>) -> Option<PathBuf> {
        let exe = exe_dir.join("pu.exe");
        find_daemon_binary_in(Some(&exe), home, explicit.map(Path::as_os_str)).ok()
    }

    #[test]
    fn given_daemon_beside_pu_should_prefer_it_over_pu_bin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let home = tmp.path().join("home");
        let installed = home.join(".pu").join("bin").join("pu-engine.exe");
        make_exe(&exe_dir.join("pu-engine.exe"));
        make_exe(&installed);
        assert_eq!(
            find(&exe_dir, Some(&home), None),
            Some(exe_dir.join("pu-engine.exe"))
        );
        std::fs::remove_file(exe_dir.join("pu-engine.exe")).unwrap();
        assert_eq!(find(&exe_dir, Some(&home), None), Some(installed));
    }

    #[test]
    fn given_explicit_engine_path_should_use_only_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        make_exe(&exe_dir.join("pu-engine.exe"));
        let dev = tmp.path().join("dev").join("pu-engine.exe");
        make_exe(&dev);
        assert_eq!(find(&exe_dir, None, Some(&dev)), Some(dev));
        let missing = tmp.path().join("missing").join("pu-engine.exe");
        assert_eq!(find(&exe_dir, None, Some(&missing)), None);
    }

    #[test]
    fn given_untrusted_candidates_should_name_each_reason() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let home = tmp.path().join("home");
        let engine = exe_dir.join("pu-engine.exe");
        make_exe(&engine);
        pu_core::win_security::grant_for_test(&engine, "WD").unwrap(); // Everyone
        let err =
            find_daemon_binary_in(Some(&exe_dir.join("pu.exe")), Some(&home), None).unwrap_err();
        let writable = format!("{}: writable by other users", engine.display());
        assert!(err.contains(&writable), "{err}");
        assert!(err.contains("pu-engine.exe: not found"), "{err}");
    }

    #[test]
    fn given_args_with_spaces_and_quotes_should_quote_for_the_crt() {
        let quoted = |arg: &str| {
            let mut out = Vec::new();
            win_spawn::quote_arg(std::ffi::OsStr::new(arg), &mut out);
            String::from_utf16(&out).unwrap()
        };
        assert_eq!(quoted("--socket"), "--socket");
        assert_eq!(quoted(r"C:\a b\pu-engine.exe"), r#""C:\a b\pu-engine.exe""#);
        assert_eq!(quoted(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quoted(r"C:\x y\"), r#""C:\x y\\""#);
        assert_eq!(quoted(""), r#""""#);
    }

    #[test]
    fn given_detached_spawn_should_send_stderr_to_the_log() {
        // where.exe rejects its `--socket <path>` arguments on stderr, which
        // must land in the log.
        let tmp = tempfile::TempDir::new().unwrap();
        let log_path = tmp.path().join("daemon.log");
        let log = std::fs::File::create(&log_path).unwrap();
        let system = std::env::var_os("SystemRoot").unwrap();
        let binary = Path::new(&system).join("System32").join("where.exe");
        win_spawn::spawn_detached(&binary, &tmp.path().join("d.sock"), Some(log)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let text = std::fs::read_to_string(&log_path).unwrap_or_default();
            if text.contains("ERROR") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "log: {text:?}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

// These exercise Unix ownership/mode trust checks, setsid and Unix-socket fake daemons.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn given_find_daemon_binary_should_return_path_or_none() {
        let result = find_daemon_binary().ok();
        // If found, verify the path is an executable file
        if let Some(path) = result {
            assert!(path.exists(), "found binary does not exist: {path:?}");
            assert!(
                path.to_string_lossy().contains("pu-engine"),
                "binary path should contain 'pu-engine': {path:?}"
            );
        }
        // If not found, that's fine — just verify no panic
    }

    /// Lookup as a `pu` in `exe_dir` would do it.
    fn find(
        exe_dir: Option<&Path>,
        home: Option<&Path>,
        explicit: Option<&std::ffi::OsStr>,
    ) -> Option<PathBuf> {
        let exe = exe_dir.map(|dir| dir.join("pu"));
        find_daemon_binary_in(exe.as_deref(), home, explicit).ok()
    }

    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn given_daemon_beside_pu_should_prefer_it_over_install_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let home = tmp.path().join("home");
        make_executable(&exe_dir.join("pu-engine"));
        make_executable(&home.join(".pu/bin/pu-engine"));
        assert_eq!(
            find(Some(&exe_dir), Some(&home), None),
            Some(exe_dir.join("pu-engine"))
        );
    }

    #[test]
    fn given_daemon_only_in_install_dirs_should_find_pu_bin_but_never_cargo_bin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("empty");
        let home = tmp.path().join("home");
        make_executable(&home.join(".cargo/bin/pu-engine"));
        assert_eq!(find(Some(&exe_dir), Some(&home), None), None);
        make_executable(&home.join(".pu/bin/pu-engine"));
        assert_eq!(
            find(Some(&exe_dir), Some(&home), None),
            Some(home.join(".pu/bin/pu-engine"))
        );
    }

    #[test]
    fn given_pu_inside_app_bundle_should_start_only_the_bundled_engine() {
        let tmp = tempfile::TempDir::new().unwrap();
        let contents = tmp.path().join("PurePoint.app/Contents");
        let helpers = contents.join("Helpers");
        let home = tmp.path().join("home");
        make_executable(&helpers.join("pu-engine"));
        make_executable(&home.join(".pu/bin/pu-engine"));
        let path_engine = tmp.path().join("path/pu-engine");
        make_executable(&path_engine);
        let explicit = Some(path_engine.as_os_str());
        assert_eq!(
            find(Some(&helpers), Some(&home), explicit),
            None,
            "a missing bundled engine must not fall back"
        );
        make_executable(&contents.join("MacOS/pu-engine"));
        for exe_dir in [&helpers, &contents.join("MacOS")] {
            assert_eq!(
                find(Some(exe_dir), Some(&home), explicit),
                Some(contents.join("MacOS/pu-engine"))
            );
        }
    }

    #[test]
    fn given_explicit_engine_path_should_use_only_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        make_executable(&exe_dir.join("pu-engine"));
        let dev = tmp.path().join("dev/pu-engine");
        make_executable(&dev);
        assert_eq!(find(Some(&exe_dir), None, Some(dev.as_os_str())), Some(dev));
        let missing = tmp.path().join("missing/pu-engine");
        assert_eq!(find(Some(&exe_dir), None, Some(missing.as_os_str())), None);
    }

    #[test]
    fn given_engine_others_can_modify_should_refuse_it() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let engine = exe_dir.join("pu-engine");
        make_executable(&engine);
        for shared in [0o775, 0o757, 0o777] {
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(shared)).unwrap();
            assert_eq!(find(Some(&exe_dir), None, None), None, "{shared:o}");
        }
    }

    #[test]
    fn given_engine_owned_by_another_user_should_refuse_it() {
        assert_eq!(trusted_mode_and_owner(0o755, 501, &[501, 0]), Ok(()));
        assert_eq!(trusted_mode_and_owner(0o755, 0, &[501, 0]), Ok(()));
        assert_eq!(
            trusted_mode_and_owner(0o755, 502, &[501, 0]),
            Err("owned by another user")
        );
        assert_eq!(
            trusted_mode_and_owner(0o644, 501, &[501]),
            Err("not executable")
        );
        assert_eq!(
            trusted_mode_and_owner(0o775, 501, &[501]),
            Err("writable by group or others")
        );
    }

    #[test]
    fn given_bundle_installed_by_another_user_should_trust_its_owner_only_in_bundle() {
        // The running `pu` (uid 502) installed the app; we run as 501.
        let in_bundle = allowed_owners(501, true, Some(502));
        assert_eq!(trusted_mode_and_owner(0o755, 502, &in_bundle), Ok(()));
        assert_eq!(
            trusted_mode_and_owner(0o775, 502, &in_bundle),
            Err("writable by group or others")
        );
        assert_eq!(
            trusted_mode_and_owner(0o755, 503, &in_bundle),
            Err("owned by another user")
        );
        // Outside a bundle the owner of `pu` earns no extra trust.
        let outside = allowed_owners(501, false, Some(502));
        assert_eq!(
            trusted_mode_and_owner(0o755, 502, &outside),
            Err("owned by another user")
        );
    }

    #[test]
    fn given_untrusted_candidates_should_name_each_reason() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let home = tmp.path().join("home");
        let engine = exe_dir.join("pu-engine");
        make_executable(&engine);
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = find_daemon_binary_in(Some(&exe_dir.join("pu")), Some(&home), None).unwrap_err();
        assert!(
            err.contains("bin/pu-engine: writable by group or others"),
            "{err}"
        );
        assert!(err.contains(".pu/bin/pu-engine: not found"), "{err}");
    }

    #[test]
    fn given_non_executable_candidate_should_skip_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let path = home.join(".pu/bin/pu-engine");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        assert_eq!(find(None, Some(&home), None), None);
    }

    #[test]
    fn given_daemon_spawn_should_detach_into_its_own_session() {
        // A stand-in daemon records its pid and process group; setsid makes it
        // the leader of a new session and process group (pgid == pid).
        let tmp = tempfile::TempDir::new().unwrap();
        let script = tmp.path().join("fake-engine");
        let out = tmp.path().join("out");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho $$ $(ps -o pgid= -p $$) > {}\n",
                out.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = daemon_command(
            &script,
            Path::new("/nonexistent.sock"),
            std::process::Stdio::null(),
        );
        // On Linux a just-written script can be briefly "text file busy" while
        // another test thread's fork still holds its write descriptor.
        let mut spawned = command.spawn();
        for _ in 0..100 {
            match &spawned {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    spawned = command.spawn();
                }
                _ => break,
            }
        }
        let mut child = spawned.unwrap();
        child.wait().unwrap();
        let report = std::fs::read_to_string(&out).unwrap();
        let ids: Vec<&str> = report.split_whitespace().collect();
        assert_eq!(ids.len(), 2, "{report:?}");
        assert_eq!(ids[0], ids[1], "daemon must lead its own process group");
        // SAFETY: getpgrp only reads process state.
        let own_pgid = unsafe { libc::getpgrp() };
        assert_ne!(ids[1], own_pgid.to_string());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_running_daemon_should_report_healthy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("test.sock");

        let engine = pu_engine::engine::Engine::new();
        let server = pu_engine::ipc_server::IpcServer::bind(&sock, engine).unwrap();
        let handle = tokio::spawn(async move {
            server.run().await.ok();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let healthy = check_daemon_health(&sock).await.unwrap();
        assert!(healthy);
        ensure_daemon(&sock).await.unwrap();

        crate::client::send_request(&sock, &pu_core::protocol::Request::Shutdown)
            .await
            .ok();
        handle.await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_no_daemon_should_report_not_healthy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("nope.sock");
        let healthy = check_daemon_health(&sock).await.unwrap();
        assert!(!healthy);
    }

    /// Serve one canned reply line (or none) to every connection on `sock`.
    fn fake_daemon(sock: &Path, reply: Option<String>) -> tokio::task::JoinHandle<()> {
        let listener = tokio::net::UnixListener::bind(sock).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let reply = reply.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                    let (reader, mut writer) = stream.split();
                    let mut line = String::new();
                    tokio::io::BufReader::new(reader)
                        .read_line(&mut line)
                        .await
                        .ok();
                    match reply {
                        Some(reply) => {
                            writer.write_all(reply.as_bytes()).await.ok();
                        }
                        None => std::future::pending::<()>().await,
                    }
                });
            }
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_daemon_at_connection_limit_should_report_busy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("busy.sock");
        let server = fake_daemon(
            &sock,
            Some(
                "{\"type\":\"error\",\"code\":\"BUSY\",\"message\":\"daemon connection limit reached\"}\n".into(),
            ),
        );

        assert!(
            matches!(check_daemon_health(&sock).await, Err(CliError::DaemonError { code, .. }) if code == "BUSY")
        );
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_incompatible_daemon_should_reject_without_stopping_it() {
        for version in [6, pu_core::protocol::PROTOCOL_VERSION + 1] {
            let tmp = tempfile::TempDir::new().unwrap();
            let sock = tmp.path().join("old.sock");
            let reply = serde_json::to_string(&Response::HealthReport {
                pid: 1,
                uptime_seconds: 0,
                protocol_version: version,
                projects: vec![],
                agent_count: 1,
            })
            .unwrap()
                + "\n";
            let server = fake_daemon(&sock, Some(reply));

            let error = ensure_daemon(&sock).await.unwrap_err().to_string();
            assert!(error.contains("protocol"), "{error}");
            assert!(error.contains("restart"), "{error}");
            let error = crate::commands::prompt::run_list(&sock, false)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("protocol"), "{error}");
            // The old daemon remains reachable, preserving its live sessions.
            assert!(
                matches!(crate::client::send_request(&sock, &Request::Health).await,
                Ok(Response::HealthReport { protocol_version, .. }) if protocol_version == version)
            );
            server.abort();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_busy_daemon_should_not_assume_protocol_compatibility() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("busy.sock");
        let server = fake_daemon(
            &sock,
            Some(
                "{\"type\":\"error\",\"code\":\"BUSY\",\"message\":\"daemon connection limit reached\"}\n".into(),
            ),
        );
        assert!(
            matches!(ensure_daemon(&sock).await, Err(CliError::DaemonError { code, .. }) if code == "BUSY")
        );
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_remote_endpoint_without_daemon_should_fail_without_starting_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("forward.sock");
        let error = ensure_remote_daemon(&sock, "studio")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("daemon not running on studio"), "{error}");
        assert!(error.contains("pu machine start studio"), "{error}");
        assert!(!sock.exists(), "nothing may bind the forwarded socket path");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_remote_daemon_with_other_protocol_should_report_incompatible() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("old.sock");
        let reply = serde_json::to_string(&Response::HealthReport {
            pid: 1,
            uptime_seconds: 0,
            protocol_version: PROTOCOL_VERSION + 1,
            projects: vec![],
            agent_count: 0,
        })
        .unwrap()
            + "\n";
        let server = fake_daemon(&sock, Some(reply));
        let error = ensure_remote_daemon(&sock, "studio")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("studio") && error.contains("protocol"),
            "{error}"
        );
        assert_eq!(
            probe_daemon_health(&sock).await,
            HealthProbe::Incompatible {
                protocol_version: PROTOCOL_VERSION + 1
            }
        );
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_healthy_daemon_wait_for_healthy_should_return_some() {
        // given
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("healthy.sock");
        let engine = pu_engine::engine::Engine::new();
        let server = pu_engine::ipc_server::IpcServer::bind(&sock, engine).unwrap();
        let handle = tokio::spawn(async move {
            server.run().await.ok();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // when
        let result = wait_for_healthy(&sock).await;

        // then
        assert!(matches!(result, Ok(Some(()))), "{result:?}");
        crate::client::send_request(&sock, &Request::Shutdown)
            .await
            .ok();
        handle.await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_incompatible_daemon_wait_for_healthy_should_propagate_the_error() {
        // given
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("old.sock");
        let reply = serde_json::to_string(&Response::HealthReport {
            pid: 1,
            uptime_seconds: 0,
            protocol_version: PROTOCOL_VERSION + 1,
            projects: vec![],
            agent_count: 0,
        })
        .unwrap()
            + "\n";
        let server = fake_daemon(&sock, Some(reply));

        // when
        let started = std::time::Instant::now();
        let result = wait_for_healthy(&sock).await;

        // then: the probe error surfaces immediately, never masked as a timeout
        let error = result.unwrap_err().to_string();
        assert!(error.contains("protocol"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_absent_daemon_wait_for_healthy_should_time_out_as_none() {
        // given: no listener at all, so every probe fails instantly
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("absent.sock");

        // when
        let started = std::time::Instant::now();
        let result = wait_for_healthy(&sock).await;

        // then: the full 3s budget is spent before giving up, without an error
        assert!(matches!(result, Ok(None)), "{result:?}");
        assert!(started.elapsed() >= std::time::Duration::from_secs(3));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_daemon_that_never_answers_should_give_up_quickly() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("mute.sock");
        let server = fake_daemon(&sock, None);

        let started = std::time::Instant::now();
        assert!(!check_daemon_health(&sock).await.unwrap());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        server.abort();
    }
}
