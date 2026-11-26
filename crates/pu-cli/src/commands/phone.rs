#[cfg(unix)]
use pu_core::canon::CanonicalPath;
use std::io::{BufRead, IsTerminal, Read, Write};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Subcommand;
use serde_json::{Value, json};

use crate::error::CliError;
#[cfg(unix)]
use crate::phone::UnitPaths;
use crate::phone::{self, Admin};

#[derive(Subcommand)]
pub enum PhoneAction {
    /// Run Point Guard as a systemd user service (Windows: a logon Run entry) so phones can pair with this host
    Install {
        /// Also start it now
        #[arg(long)]
        now: bool,
        /// Tailnet TCP port phones connect to
        #[arg(long, default_value_t = phone::DEFAULT_PORT)]
        port: u16,
    },
    /// Stop and remove the service (device trust and Point Guard state are kept)
    Uninstall,
    /// Start the service
    Start,
    /// Stop the service once Point Guard is idle
    Stop {
        /// Stop even while Point Guard is working (unfinished replies are lost)
        #[arg(long)]
        force: bool,
    },
    /// Restart once idle, moving to this pu's payload if it changed
    Restart {
        /// Restart even while Point Guard is working (unfinished replies are lost)
        #[arg(long)]
        force: bool,
    },
    /// Service, phone reachability and provider state
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Run Point Guard in the foreground (for hosts without systemd or autostart)
    Run {
        /// Tailnet TCP port phones connect to
        #[arg(long, default_value_t = phone::DEFAULT_PORT)]
        port: u16,
    },
    /// Show a one-time QR code to pair a phone or iPad
    Pair {
        /// Seconds the code stays valid (1-300)
        #[arg(long, default_value_t = 120)]
        ttl: u32,
        /// Print the enrollment as text to paste into the app instead of a QR
        #[arg(long)]
        text: bool,
    },
    /// List paired devices and their grants
    Devices {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Change what a paired device may do
    Grant {
        /// Device name or id prefix
        device: String,
        /// Agents and terminals: off, view or control
        #[arg(long, value_parser = ["off", "view", "control"])]
        terminals: Option<String>,
        /// Workspace management and chat: on or off
        #[arg(long, value_parser = ["on", "off"])]
        workspace: Option<String>,
    },
    /// Rename a paired device
    Rename {
        /// Device name or id prefix
        device: String,
        /// New name
        name: String,
    },
    /// Revoke a paired device (it must pair again with a new QR)
    Revoke {
        /// Device name or id prefix
        device: String,
    },
    /// Sign in to a model provider for Point Guard
    Login {
        /// Provider id (omit to list providers)
        provider: Option<String>,
        /// Read an API key from stdin instead of signing in through the provider
        #[arg(long)]
        api_key_stdin: bool,
    },
    /// Choose Point Guard's model as <provider>/<model> (omit to list models)
    Model {
        /// <provider>/<model>
        model: Option<String>,
    },
    /// Choose Point Guard's working folder (applies on restart)
    Cwd {
        /// Absolute path of an existing folder
        path: PathBuf,
    },
}

pub async fn run(action: PhoneAction) -> Result<(), CliError> {
    if !cfg!(target_os = "linux") && !cfg!(windows) {
        return Err(CliError::Other(
            "`pu phone` runs Point Guard on headless Linux hosts. On a Mac, use the PurePoint app: Point Guard → Connect phone."
                .into(),
        ));
    }
    match action {
        PhoneAction::Install { now, port } => install(now, port),
        PhoneAction::Uninstall => uninstall(),
        PhoneAction::Start => start(),
        PhoneAction::Stop { force } => stop(force),
        PhoneAction::Restart { force } => restart(force),
        PhoneAction::Status { json } => status(json),
        PhoneAction::Run { port } => run_foreground(port),
        PhoneAction::Pair { ttl, text } => pair(&admin()?, ttl, text).await,
        PhoneAction::Devices { json } => devices(&admin()?, json),
        PhoneAction::Grant {
            device,
            terminals,
            workspace,
        } => grant(&admin()?, &device, terminals, workspace),
        PhoneAction::Rename { device, name } => {
            let admin = admin()?;
            let id = device_id(&admin, &device)?;
            admin.call(json!({"operation": "devices.rename", "deviceId": id, "name": name}))?;
            println!("renamed to {name:?}");
            Ok(())
        }
        PhoneAction::Revoke { device } => revoke(&admin()?, &device),
        PhoneAction::Login {
            provider,
            api_key_stdin,
        } => login(&admin()?, provider, api_key_stdin).await,
        PhoneAction::Model { model: None } => list_models(&admin()?),
        PhoneAction::Model { model: Some(model) } => {
            let (provider, id) = model
                .split_once('/')
                .ok_or_else(|| CliError::Other("use <provider>/<model>".into()))?;
            select_model(&admin()?, provider, id)
        }
        PhoneAction::Cwd { path } => {
            let path = pu_core::canon::canonicalize(&path)?;
            admin()?.call(json!({"operation": "runtime.configure", "cwd": path}))?;
            println!(
                "working folder set to {}; apply it with `pu phone restart`",
                path.display()
            );
            Ok(())
        }
    }
}

fn revoke(admin: &Admin, device: &str) -> Result<(), CliError> {
    let id = device_id(admin, device)?;
    admin.call(json!({"operation": "devices.revoke", "deviceId": id}))?;
    println!("revoked; its live connections are closed and it must pair again");
    Ok(())
}

fn select_model(admin: &Admin, provider: &str, id: &str) -> Result<(), CliError> {
    admin.call(json!({"operation": "model.select", "provider": provider, "model": id}))?;
    println!("Point Guard now uses {provider}/{id}");
    Ok(())
}

fn list_models(admin: &Admin) -> Result<(), CliError> {
    let listed = admin.call(json!({"operation": "providers"}))?;
    let mut any = false;
    for p in listed["providers"].as_array().into_iter().flatten() {
        if p["configured"].as_bool() != Some(true) {
            continue;
        }
        for m in p["models"].as_array().into_iter().flatten() {
            any = true;
            println!(
                "{}/{}",
                p["id"].as_str().unwrap_or("?"),
                m["id"].as_str().unwrap_or("?")
            );
        }
    }
    if !any {
        println!("No signed-in providers. Sign in with `pu phone login`.");
    }
    Ok(())
}

fn admin() -> Result<Admin, CliError> {
    Admin::connect(&phone::state_dir()?)
}

#[cfg(unix)]
fn home() -> Result<PathBuf, CliError> {
    pu_core::paths::home_dir()
        .ok()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| CliError::Other("HOME is not set to an absolute path".into()))
}

#[cfg(unix)]
fn systemctl(args: &[&str]) -> Result<(), CliError> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| CliError::Other(format!("failed to run systemctl: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(CliError::Other(format!(
        "`systemctl --user {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(unix)]
fn unit_state() -> String {
    Command::new("systemctl")
        .args(["--user", "is-active", phone::UNIT])
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// The payload this `pu` was released with, verified, and the `pu` beside it.
#[cfg(unix)]
fn current_payload() -> Result<(PathBuf, PathBuf), CliError> {
    let payload = phone::payload_dir(phone::PU_VERSION)?;
    phone::read_manifest(&payload)?;
    for file in ["bin/node", "bin/point-guard-lock", "bridge/main.js"] {
        let path = payload.join(file);
        let meta = std::fs::symlink_metadata(&path).map_err(|_| {
            CliError::Other(format!(
                "phone access payload is incomplete: {} missing",
                path.display()
            ))
        })?;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        if meta.file_type().is_symlink()
            || (meta.uid() != uid && meta.uid() != 0)
            || meta.permissions().mode() & 0o022 != 0
        {
            return Err(CliError::Other(format!(
                "{} is writable by others or not owned by you; refusing to run it",
                path.display()
            )));
        }
    }
    let pu = std::env::current_exe()?
        .canonical()
        .map_err(|e| CliError::Other(format!("cannot locate pu: {e}")))?;
    Ok((payload, pu))
}

#[cfg(unix)]
fn render_unit(port: u16) -> Result<(String, PathBuf), CliError> {
    let (payload, pu) = current_payload()?;
    let home = home()?;
    let state = phone::state_dir()?;
    let unit = phone::systemd_unit(&UnitPaths {
        payload: &payload,
        state: &state,
        home: &home,
        pu: &pu,
        port,
    })?;
    Ok((unit, payload))
}

#[cfg(unix)]
fn installed_port() -> u16 {
    phone::unit_file()
        .ok()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|unit| {
            unit.lines()
                .find_map(|l| l.strip_prefix("Environment=\"PI_MOBILE_PORT="))
                .and_then(|v| v.trim_end_matches('"').parse().ok())
        })
        .unwrap_or(phone::DEFAULT_PORT)
}

#[cfg(unix)]
fn write_unit(unit: &str) -> Result<PathBuf, CliError> {
    let file = phone::unit_file()?;
    phone::write_unit_file(&file, unit)?;
    systemctl(&["daemon-reload"])?;
    Ok(file)
}

#[cfg(unix)]
fn install(now: bool, port: u16) -> Result<(), CliError> {
    let (unit, payload) = render_unit(port)?;
    let file = write_unit(&unit)?;
    systemctl(&["enable", phone::UNIT])?;
    println!("wrote {} (runs {})", file.display(), payload.display());
    if now {
        systemctl(&["start", phone::UNIT])?;
        println!("started; pair a phone with `pu phone pair`");
    } else {
        println!("start it with `pu phone start`");
    }
    if !linger_enabled() {
        println!(
            "lingering is off: Point Guard stops when your last session ends. Enable it with `sudo loginctl enable-linger $USER`"
        );
    }
    if !Path::new("/usr/bin/tailscale").exists()
        && !Path::new("/usr/sbin/tailscale").exists()
        && !Path::new("/usr/local/bin/tailscale").exists()
        && !Path::new("/snap/bin/tailscale").exists()
    {
        println!("Tailscale is not installed; phones reach this host only over your tailnet");
    }
    Ok(())
}

#[cfg(unix)]
fn linger_enabled() -> bool {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    Command::new("loginctl")
        .args([
            "show-user",
            &uid.to_string(),
            "--property=Linger",
            "--value",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
}

#[cfg(unix)]
fn uninstall() -> Result<(), CliError> {
    let file = phone::unit_file()?;
    if !file.exists() {
        println!("phone service is not installed");
        return Ok(());
    }
    let _ = systemctl(&["disable", "--now", phone::UNIT]);
    pu_core::fs_retry::remove_file(&file)?;
    let _ = systemctl(&["daemon-reload"]);
    println!(
        "removed {}; paired devices and Point Guard state are kept in {}",
        file.display(),
        phone::state_dir()?.display()
    );
    Ok(())
}

/// Ask the bridge to drain and exit while idle; it refuses during work.
/// Fails closed when the unit runs but its admin channel can't be reached.
#[cfg(unix)]
fn stop_idle() -> Result<bool, CliError> {
    let admin = match admin() {
        Ok(admin) => admin,
        Err(_) if !phone::unit_running(&unit_state()) => return Ok(false),
        Err(e) => {
            return Err(CliError::Other(format!(
                "{} is running but its admin channel is unavailable ({e}), so it can't be stopped only when idle; retry shortly, or pass --force to stop it anyway (unfinished replies are lost)",
                phone::UNIT
            )));
        }
    };
    admin.call(json!({"operation": "runtime.stop"}))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if unit_state() != "active" && unit_state() != "deactivating" {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(CliError::Other(
        "Point Guard did not stop within 30s".into(),
    ))
}

#[cfg(unix)]
fn start() -> Result<(), CliError> {
    let (unit, payload) = render_unit(installed_port())?;
    let installed = std::fs::read_to_string(phone::unit_file()?).ok();
    phone::check_startable(installed.as_deref(), &unit, &payload)?;
    systemctl(&["start", phone::UNIT])?;
    println!("started {}; check `pu phone status`", phone::UNIT);
    Ok(())
}

#[cfg(unix)]
fn stop(force: bool) -> Result<(), CliError> {
    if !force {
        stop_idle()?;
    }
    systemctl(&["stop", phone::UNIT])?;
    println!("stopped");
    Ok(())
}

#[cfg(unix)]
fn restart(force: bool) -> Result<(), CliError> {
    let port = installed_port();
    let (unit, payload) = render_unit(port)?;
    if !force {
        stop_idle()?;
    }
    systemctl(&["stop", phone::UNIT])?;
    let file = phone::unit_file()?;
    if std::fs::read_to_string(&file).ok().as_deref() != Some(unit.as_str()) {
        write_unit(&unit)?;
    }
    systemctl(&["start", phone::UNIT])?;
    prune_payloads(&payload);
    println!("restarted on {}", payload.display());
    Ok(())
}

/// Remove payload versions nothing runs any more: only plain `X.Y.Z`
/// directories older than `keep`, never the unit's payload, never newer
/// versions or `install.sh` staging directories.
#[cfg(unix)]
fn prune_payloads(keep: &Path) {
    let Ok(root) = phone::payload_root() else {
        return;
    };
    let running = phone::unit_file()
        .ok()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|unit| phone::unit_payload(&unit));
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir
            && Some(&path) != running.as_ref()
            && phone::prunable(&entry.file_name().to_string_lossy(), keep)
        {
            let _ = pu_core::fs_retry::remove_dir_all(&path);
        }
    }
}

#[cfg(unix)]
fn status(as_json: bool) -> Result<(), CliError> {
    let service = unit_state();
    let state = admin().and_then(|a| a.call(json!({"operation": "status"})));
    if as_json {
        let value = match &state {
            Ok(s) => json!({"service": service, "pointGuard": s}),
            Err(e) => json!({"service": service, "error": e.to_string()}),
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("service:   {service}");
    let s = match state {
        Ok(s) => s,
        Err(e) => {
            println!("bridge:    {e}");
            return Ok(());
        }
    };
    for line in bridge_status_lines(&s) {
        println!("{line}");
    }
    Ok(())
}

/// The bridge's own status (`status` admin operation) as `pu phone status` lines.
pub(crate) fn bridge_status_lines(s: &Value) -> Vec<String> {
    let mut lines = vec![
        format!(
            "host:      {} ({})",
            s["hostLabel"].as_str().unwrap_or("?"),
            s["hostKind"].as_str().unwrap_or("?")
        ),
        format!("phase:     {}", s["phase"].as_str().unwrap_or("?")),
    ];
    match s["chatURL"].as_str() {
        Some(url) => lines.push(format!("phones:    ready at {url}")),
        None => {
            lines.push("phones:    not reachable".into());
            if let Some(m) = s["remote"]["message"].as_str() {
                lines.push(format!("           {m}"));
            }
            if let Some(r) = s["remote"]["recovery"].as_str() {
                lines.push(format!("           {r}"));
            }
        }
    }
    let model = match (s["provider"].as_str(), s["model"].as_str()) {
        (Some(p), Some(m)) => format!("{p}/{m}"),
        _ => "none (sign in with `pu phone login`)".into(),
    };
    lines.push(format!("model:     {model}"));
    lines.push(format!("folder:    {}", s["cwd"].as_str().unwrap_or("?")));
    if let Some(recovery) = s["recovery"].as_str() {
        lines.push(format!("recovery:  {recovery}"));
    }
    lines
}

/// Windows: the same subcommands over a logon Run entry (`phone_windows`).
#[cfg(windows)]
fn print_lines(lines: Vec<String>) -> Result<(), CliError> {
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

#[cfg(windows)]
fn install(now: bool, port: u16) -> Result<(), CliError> {
    print_lines(crate::phone_windows::Host::real()?.install(now, port)?)
}

#[cfg(windows)]
fn uninstall() -> Result<(), CliError> {
    print_lines(crate::phone_windows::Host::real()?.uninstall()?)
}

#[cfg(windows)]
fn start() -> Result<(), CliError> {
    print_lines(crate::phone_windows::Host::real()?.start()?)
}

#[cfg(windows)]
fn stop(force: bool) -> Result<(), CliError> {
    print_lines(crate::phone_windows::Host::real()?.stop(force)?)
}

#[cfg(windows)]
fn restart(force: bool) -> Result<(), CliError> {
    print_lines(crate::phone_windows::Host::real()?.restart(force)?)
}

#[cfg(windows)]
fn status(as_json: bool) -> Result<(), CliError> {
    let host = crate::phone_windows::Host::real()?;
    let value = host.status_value();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    print_lines(host.status_lines(&value))
}

#[cfg(windows)]
fn run_foreground(port: u16) -> Result<(), CliError> {
    crate::phone_windows::Host::real()?.run_foreground(port)
}

#[cfg(unix)]
fn run_foreground(port: u16) -> Result<(), CliError> {
    let (payload, pu) = current_payload()?;
    let home = home()?;
    let state = phone::state_dir()?;
    let paths = UnitPaths {
        payload: &payload,
        state: &state,
        home: &home,
        pu: &pu,
        port,
    };
    let mut command = Command::new(payload.join("bin/node"));
    command
        .arg(payload.join("bridge/main.js"))
        .arg("--managed")
        .current_dir(&home)
        .env_clear();
    for (key, value) in phone::bridge_environment(&paths) {
        command.env(key, value);
    }
    // Foreground runs have no systemd restart overlap to wait for.
    command.env_remove("POINT_GUARD_SUPERVISOR");
    for key in ["USER", "LOGNAME", "LANG", "LC_ALL", "TZ"] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    // SAFETY: umask has no preconditions; the bridge also sets modes explicitly.
    unsafe {
        libc::umask(0o077);
    }
    let status = command
        .status()
        .map_err(|e| CliError::Other(format!("cannot start Point Guard: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Other(format!("Point Guard exited ({status})")))
    }
}

fn devices_list(admin: &Admin) -> Result<Vec<Value>, CliError> {
    let result = admin.call(json!({"operation": "devices.list"}))?;
    Ok(result["devices"].as_array().cloned().unwrap_or_default())
}

fn device_id(admin: &Admin, query: &str) -> Result<String, CliError> {
    let devices = devices_list(admin)?;
    let device = phone::resolve_device(&devices, query)?;
    Ok(device["deviceId"].as_str().unwrap_or_default().to_string())
}

fn describe(device: &Value) -> String {
    format!(
        "terminals {}, workspace {}, chat {}",
        device["terminals"].as_str().unwrap_or("off"),
        if device["workspaceManagement"].as_bool() == Some(true) {
            "on"
        } else {
            "off"
        },
        phone::chat_level(device)
    )
}

fn devices(admin: &Admin, as_json: bool) -> Result<(), CliError> {
    let devices = devices_list(admin)?;
    if as_json {
        let list: Vec<Value> = devices
            .iter()
            .map(|d| {
                json!({
                    "deviceId": d["deviceId"],
                    "name": d["name"],
                    "createdAt": d["createdAt"],
                    "revokedAt": d["revokedAt"],
                    "terminals": d["terminals"],
                    "workspaceManagement": d["workspaceManagement"],
                    "chat": phone::chat_level(d),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    let active: Vec<&Value> = devices
        .iter()
        .filter(|d| d["revokedAt"].is_null())
        .collect();
    if active.is_empty() {
        println!("No paired devices. Pair one with `pu phone pair`.");
        return Ok(());
    }
    for d in active {
        let id = d["deviceId"].as_str().unwrap_or("");
        println!(
            "{:<8}  {:<24}  {}",
            &id[..id.len().min(8)],
            d["name"].as_str().unwrap_or("?"),
            describe(d)
        );
    }
    Ok(())
}

fn grant(
    admin: &Admin,
    device: &str,
    terminals: Option<String>,
    workspace: Option<String>,
) -> Result<(), CliError> {
    if terminals.is_none() && workspace.is_none() {
        return Err(CliError::Other(
            "pass --terminals off|view|control and/or --workspace on|off".into(),
        ));
    }
    let id = device_id(admin, device)?;
    // Downgrades first, so a combined change never briefly widens access.
    let mut ops = Vec::new();
    if let Some(w) = &workspace {
        ops.push((w == "off", json!({"operation": "devices.workspaceManagement", "deviceId": id, "workspaceManagement": w == "on"})));
    }
    if let Some(t) = &terminals {
        ops.push((
            t != "control",
            json!({"operation": "devices.terminals", "deviceId": id, "terminals": t}),
        ));
    }
    ops.sort_by_key(|(downgrade, _)| !*downgrade);
    if terminals.as_deref() == Some("control") || workspace.as_deref() == Some("on") {
        println!(
            "note: Control plus workspace management lets this device chat with Point Guard, which can run commands and manage agents on this host"
        );
    }
    for (_, op) in ops {
        admin.call(op)?;
    }
    let devices = devices_list(admin)?;
    if let Some(d) = devices
        .iter()
        .find(|d| d["deviceId"].as_str() == Some(id.as_str()))
    {
        println!("{}: {}", d["name"].as_str().unwrap_or("?"), describe(d));
    }
    Ok(())
}

/// What `pu phone pair` shows for `payload`: the QR, or the paste code when
/// asked for text, when stdout is not a terminal, or when the QR is too wide.
fn pairing_output(
    payload: &str,
    ttl: u32,
    text: bool,
    terminal: bool,
    columns: Option<usize>,
) -> Result<String, CliError> {
    let (qr, size) = phone::render_qr(payload)?;
    let mut out = String::new();
    if text || !terminal || columns.is_some_and(|c| c < size) {
        if !text && columns.is_some_and(|c| c < size) {
            out.push_str(&format!(
                "Terminal is narrower than the QR ({size} columns); showing the paste code instead.\n"
            ));
        }
        out.push_str(
            "In the PurePoint app: Connections → Pair a computer → Paste code, then paste:\n\n",
        );
        out.push_str(&format!("purepoint-pair:{payload}\n\n"));
    } else {
        out.push_str(&qr);
        out.push_str(
            "Scan with the PurePoint app: Connections → Pair a computer → Scan pairing code.\n",
        );
    }
    out.push_str(&format!(
        "One phone can use this code within {ttl}s. Press Ctrl-C to cancel; the code is revoked when this command ends.\n"
    ));
    Ok(out)
}

async fn pair(admin: &Admin, ttl: u32, text: bool) -> Result<(), CliError> {
    if !(1..=300).contains(&ttl) {
        return Err(CliError::Other("--ttl must be 1-300 seconds".into()));
    }
    // Handlers exist before the enrollment does, so no signal can end this
    // process between creation and the revoke guard.
    let signals = Signals::new()?;
    let created = admin.call(json!({"operation": "pairing.create", "ttlSeconds": ttl}))?;
    let enrollment = created["enrollmentId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let payload = created["payload"].as_str().unwrap_or_default().to_string();
    let expires = created["expiresAt"].as_i64().unwrap_or_default();
    // Revoke whatever happens next unless the enrollment was consumed.
    let guard = RevokeGuard {
        admin,
        enrollment: &enrollment,
        expires,
        armed: true,
    };
    print!(
        "{}",
        pairing_output(
            &payload,
            ttl,
            text,
            std::io::stdout().is_terminal(),
            terminal_columns()
        )?
    );
    let _ = std::io::stdout().flush();
    let mut guard = guard;
    let mut signals = signals;
    match wait_for_enrollment(admin, &enrollment, &mut signals).await? {
        Enrolled::Device(id) => {
            guard.armed = false;
            let devices = devices_list(admin)?;
            match devices
                .iter()
                .find(|d| d["deviceId"].as_str() == Some(id.as_str()))
            {
                Some(d) => println!(
                    "Paired {:?}: {}. Change it with `pu phone grant`.",
                    d["name"].as_str().unwrap_or("device"),
                    describe(d)
                ),
                None => println!("Paired a device. Check it with `pu phone devices`."),
            }
            Ok(())
        }
        Enrolled::Ended(status) => Err(CliError::Other(format!(
            "pairing code {status}; run `pu phone pair` again"
        ))),
    }
}

/// Ctrl-C, Ctrl-Break and closing the console window all end the wait.
#[cfg(windows)]
struct Signals {
    c: tokio::signal::windows::CtrlC,
    brk: tokio::signal::windows::CtrlBreak,
    close: tokio::signal::windows::CtrlClose,
}

#[cfg(windows)]
impl Signals {
    fn new() -> Result<Self, CliError> {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
        Ok(Signals {
            c: ctrl_c()?,
            brk: ctrl_break()?,
            close: ctrl_close()?,
        })
    }

    async fn any(&mut self) -> &'static str {
        tokio::select! {
            _ = self.c.recv() => "canceled",
            _ = self.brk.recv() => "canceled",
            _ = self.close.recv() => "session ended",
        }
    }
}

/// SIGINT, SIGTERM and SIGHUP (an SSH drop) all end the wait.
#[cfg(unix)]
struct Signals {
    term: tokio::signal::unix::Signal,
    hup: tokio::signal::unix::Signal,
    int: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    fn new() -> Result<Self, CliError> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Signals {
            term: signal(SignalKind::terminate())?,
            hup: signal(SignalKind::hangup())?,
            int: signal(SignalKind::interrupt())?,
        })
    }

    async fn any(&mut self) -> &'static str {
        tokio::select! {
            _ = self.term.recv() => "canceled",
            _ = self.hup.recv() => "session ended",
            _ = self.int.recv() => "canceled",
        }
    }
}

enum Enrolled {
    Device(String),
    Ended(String),
}

async fn wait_for_enrollment(
    admin: &Admin,
    enrollment: &str,
    signals: &mut Signals,
) -> Result<Enrolled, CliError> {
    let mut consumed_at: Option<Instant> = None;
    loop {
        tokio::select! {
            reason = signals.any() => return Err(CliError::Other(reason.into())),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
        let status =
            admin.call(json!({"operation": "pairing.status", "enrollmentId": enrollment}))?;
        match (status["status"].as_str(), status["deviceId"].as_str()) {
            (Some("pending"), _) => {}
            // Consumed but not yet durable: wait briefly for the device id. A
            // failed write never retries, so the phone must pair again.
            (Some("consumed"), None) => {
                if consumed_at.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(15)
                {
                    return Ok(Enrolled::Ended(
                        "was used but the device was not saved".into(),
                    ));
                }
            }
            (Some("consumed"), Some(id)) => return Ok(Enrolled::Device(id.to_string())),
            (Some(other), _) => return Ok(Enrolled::Ended(other.to_string())),
            (None, _) => return Ok(Enrolled::Ended("unknown".into())),
        }
    }
}

struct RevokeGuard<'a> {
    admin: &'a Admin,
    enrollment: &'a str,
    expires: i64,
    armed: bool,
}

impl Drop for RevokeGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let revoked = self
            .admin
            .call(json!({"operation": "pairing.revoke", "enrollmentId": self.enrollment}))
            .is_ok();
        if !revoked {
            let left = (self.expires - chrono::Utc::now().timestamp_millis()).max(0) / 1000;
            eprintln!(
                "warning: could not confirm the code was revoked; it may stay valid for {left}s more"
            );
        }
    }
}

#[cfg(windows)]
fn terminal_columns() -> Option<usize> {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: the info struct is zeroed and filled by the call; a non-console
    // handle just fails.
    unsafe {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        let ok = GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &mut info) != 0;
        let width = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
        (ok && width > 0).then_some(width as usize)
    }
}

#[cfg(unix)]
fn terminal_columns() -> Option<usize> {
    // SAFETY: TIOCGWINSZ fills a winsize struct for a terminal fd.
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        (libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0 && size.ws_col > 0)
            .then_some(size.ws_col as usize)
    }
}

/// Read one line without echo when stdin is a console.
#[cfg(windows)]
fn read_secret(prompt: &str) -> Result<String, CliError> {
    use windows_sys::Win32::System::Console::{
        ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
    };
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    // SAFETY: console mode calls on the stdin handle; a pipe just fails.
    let saved = unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0u32;
        (GetConsoleMode(input, &mut mode) != 0
            && SetConsoleMode(input, mode & !ENABLE_ECHO_INPUT) != 0)
            .then_some((input, mode))
    };
    let mut line = String::new();
    let result = std::io::stdin().lock().read_line(&mut line);
    if let Some((input, mode)) = saved {
        // SAFETY: restores the mode read above.
        unsafe { SetConsoleMode(input, mode) };
        eprintln!();
    }
    result?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Read one line without echo when stdin is a terminal.
#[cfg(unix)]
fn read_secret(prompt: &str) -> Result<String, CliError> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal();
    // SAFETY: termios calls on stdin with a zeroed struct filled by tcgetattr.
    let saved = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if tty && libc::tcgetattr(libc::STDIN_FILENO, &mut t) == 0 {
            let saved = t;
            t.c_lflag &= !libc::ECHO;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t);
            Some(saved)
        } else {
            None
        }
    };
    let mut line = String::new();
    let result = stdin.lock().read_line(&mut line);
    if let Some(saved) = saved {
        // SAFETY: restores the attributes read above.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &saved);
        }
        eprintln!();
    }
    result?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn read_line(prompt: &str) -> Result<String, CliError> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

async fn login(
    admin: &Admin,
    provider: Option<String>,
    api_key_stdin: bool,
) -> Result<(), CliError> {
    let listed = admin.call(json!({"operation": "providers"}))?;
    let providers = listed["providers"].as_array().cloned().unwrap_or_default();
    let Some(provider) = provider else {
        for p in &providers {
            let mut methods = Vec::new();
            if p["oauth"].as_bool() == Some(true) {
                methods.push("sign-in");
            }
            if p["apiKey"].as_bool() == Some(true) {
                methods.push("api key");
            }
            println!(
                "{:<20} {:<28} {}{}",
                p["id"].as_str().unwrap_or("?"),
                p["name"].as_str().unwrap_or(""),
                methods.join(", "),
                if p["configured"].as_bool() == Some(true) {
                    "  (configured)"
                } else {
                    ""
                }
            );
        }
        println!("\nSign in with `pu phone login <provider>` (add --api-key-stdin to pipe a key).");
        return Ok(());
    };
    let entry = providers
        .iter()
        .find(|p| p["id"].as_str() == Some(provider.as_str()))
        .ok_or_else(|| {
            CliError::Other(format!(
                "unknown provider {provider:?}; list them with `pu phone login`"
            ))
        })?;
    let kind = if api_key_stdin || entry["oauth"].as_bool() != Some(true) {
        "api_key"
    } else {
        "oauth"
    };
    let mut piped_key = None;
    if api_key_stdin {
        let mut key = String::new();
        std::io::stdin().read_to_string(&mut key)?;
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(CliError::Other("no API key on stdin".into()));
        }
        piped_key = Some(key);
    }
    let mut signals = Signals::new()?;
    let started =
        admin.call(json!({"operation": "auth.start", "provider": provider, "type": kind}))?;
    let attempt = started["attemptId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Cancel the attempt on any exit except success.
    let mut guard = CancelLogin {
        admin,
        attempt: &attempt,
        armed: true,
    };
    let mut shown = 0usize;
    let mut answered: Option<String> = None;
    loop {
        let s = admin.call(json!({"operation": "auth.status", "attemptId": attempt}))?;
        let events = s["events"].as_array().cloned().unwrap_or_default();
        for event in events.iter().skip(shown) {
            if let Some(text) = event["instructions"].as_str().or(event["message"].as_str()) {
                println!("{text}");
            }
            if let Some(url) = event["url"].as_str().or(event["verificationUri"].as_str()) {
                println!("Open on any device: {url}");
            }
            if let Some(code) = event["userCode"].as_str() {
                println!("Code: {code}");
            }
            for link in event["links"].as_array().into_iter().flatten() {
                if let Some(url) = link["url"].as_str() {
                    println!("  {url}");
                }
            }
        }
        shown = events.len();
        match s["status"].as_str() {
            Some("complete") => {
                guard.armed = false;
                println!(
                    "Signed in to {provider}. Apply it with `pu phone restart`, then choose a model with `pu phone model` (lists them)."
                );
                return Ok(());
            }
            Some("pending") => {}
            Some(other) => {
                return Err(CliError::Other(
                    s["error"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("sign-in {other}")),
                ));
            }
            None => return Err(CliError::Other("unexpected sign-in status".into())),
        }
        let prompt = &s["prompt"];
        if let Some(id) = prompt["id"].as_str()
            && answered.as_deref() != Some(id)
        {
            let message = prompt["message"].as_str().unwrap_or("Value");
            let value = match prompt["type"].as_str() {
                Some("secret") => match piped_key.take() {
                    Some(key) => key,
                    None => read_secret(&format!("{message}: "))?,
                },
                Some("select") => {
                    let options = prompt["options"].as_array().cloned().unwrap_or_default();
                    for (i, o) in options.iter().enumerate() {
                        eprintln!(
                            "  {}) {}",
                            i + 1,
                            o["label"].as_str().or(o["id"].as_str()).unwrap_or("?")
                        );
                    }
                    let choice = read_line(&format!("{message} [1-{}]: ", options.len()))?;
                    let index = choice
                        .parse::<usize>()
                        .ok()
                        .filter(|i| (1..=options.len()).contains(i))
                        .ok_or_else(|| CliError::Other("invalid choice".into()))?;
                    options[index - 1]["id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                }
                _ => read_line(&format!("{message}: "))?,
            };
            admin.call(json!({"operation": "auth.respond", "attemptId": attempt, "promptId": id, "value": value}))?;
            answered = Some(id.to_string());
        }
        tokio::select! {
            reason = signals.any() => return Err(CliError::Other(reason.into())),
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
    }
}

struct CancelLogin<'a> {
    admin: &'a Admin,
    attempt: &'a str,
    armed: bool,
}

impl Drop for CancelLogin<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self
                .admin
                .call(json!({"operation": "auth.cancel", "attemptId": self.attempt}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const PAYLOAD: &str =
        r#"{"type":"pi-mobile-pairing","version":1,"endpoint":"wss://100.64.1.2:8787/v1"}"#;

    #[test]
    fn given_pairing_payload_should_print_qr_or_paste_code() {
        let qr = pairing_output(PAYLOAD, 120, false, true, Some(200)).unwrap();
        assert!(qr.contains("\x1b[30;107m") && qr.contains("Scan with the PurePoint app"));
        assert!(!qr.contains("purepoint-pair:"));
        let text = pairing_output(PAYLOAD, 120, true, true, Some(200)).unwrap();
        assert!(
            text.contains(&format!("purepoint-pair:{PAYLOAD}\n")),
            "{text}"
        );
        let piped = pairing_output(PAYLOAD, 120, false, false, None).unwrap();
        assert!(piped.contains("purepoint-pair:"));
        let narrow = pairing_output(PAYLOAD, 120, false, true, Some(20)).unwrap();
        assert!(narrow.contains("narrower than the QR") && narrow.contains("purepoint-pair:"));
        assert!(narrow.contains("within 120s"));
    }
}

/// The admin subcommands on Windows, against a fake bridge on the owner pipe.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::tests::PAYLOAD;
    use super::*;
    use crate::phone_windows::fakes::{bridge, operations};

    fn device() -> Value {
        json!({"deviceId": "aaaa1111-0000-4000-8000-000000000000", "name": "iPhone", "terminals": "control", "workspaceManagement": true})
    }

    fn connect(state: &std::path::Path) -> Admin {
        Admin::connect(state).unwrap()
    }

    #[tokio::test]
    async fn given_a_phone_scanning_the_code_should_pair_and_show_its_grants() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let fake = bridge(&state, |r| match r["operation"].as_str() {
            Some("pairing.create") => {
                Ok(json!({"enrollmentId": "e1", "payload": PAYLOAD, "expiresAt": 1}))
            }
            Some("pairing.status") => {
                Ok(json!({"status": "consumed", "deviceId": device()["deviceId"]}))
            }
            Some("devices.list") => Ok(json!({"devices": [device()]})),
            _ => Err("unexpected".into()),
        });
        pair(&connect(&state), 30, true).await.unwrap();
        assert_eq!(
            operations(&fake),
            ["pairing.create", "pairing.status", "devices.list"]
        );
        assert_eq!(fake.requests.lock().unwrap()[0]["ttlSeconds"], 30);
    }

    #[tokio::test]
    async fn given_an_expired_code_should_fail_and_revoke_it() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let fake = bridge(&state, |r| match r["operation"].as_str() {
            Some("pairing.create") => {
                Ok(json!({"enrollmentId": "e1", "payload": PAYLOAD, "expiresAt": 1}))
            }
            Some("pairing.status") => Ok(json!({"status": "expired"})),
            Some("pairing.revoke") => Ok(json!({})),
            _ => Err("unexpected".into()),
        });
        let err = pair(&connect(&state), 30, true)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("expired") && err.contains("pu phone pair"),
            "{err}"
        );
        assert_eq!(
            operations(&fake),
            ["pairing.create", "pairing.status", "pairing.revoke"]
        );
        assert!(pair(&connect(&state), 0, true).await.is_err(), "ttl bounds");
    }

    #[test]
    fn given_devices_grant_and_revoke_should_resolve_names_and_downgrade_first() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let fake = bridge(&state, |r| match r["operation"].as_str() {
            Some("devices.list") => Ok(json!({"devices": [device()]})),
            Some(_) => Ok(json!({})),
            None => Err("unexpected".into()),
        });
        let admin = connect(&state);
        devices(&admin, true).unwrap();
        grant(&admin, "iPhone", Some("control".into()), Some("off".into())).unwrap();
        revoke(&admin, "aaaa1").unwrap();
        assert!(grant(&admin, "iPhone", None, None).is_err());
        assert!(revoke(&admin, "nobody").is_err());
        assert_eq!(
            operations(&fake),
            [
                "devices.list",
                "devices.list",
                "devices.workspaceManagement",
                "devices.terminals",
                "devices.list",
                "devices.list",
                "devices.revoke",
                "devices.list",
            ]
        );
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests[2]["workspaceManagement"], false);
        assert_eq!(requests[3]["terminals"], "control");
        assert_eq!(requests[6]["deviceId"], device()["deviceId"]);
    }

    #[tokio::test]
    async fn given_login_and_model_should_drive_the_providers_api() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let fake = bridge(&state, |r| match r["operation"].as_str() {
            Some("providers") => Ok(
                json!({"providers": [{"id": "anthropic", "name": "Anthropic", "oauth": true, "configured": true, "models": [{"id": "claude"}]}]}),
            ),
            Some("auth.start") => Ok(json!({"attemptId": "a1"})),
            Some("auth.status") => Ok(
                json!({"status": "complete", "events": [{"url": "https://example.invalid/device"}]}),
            ),
            Some("model.select") => Ok(json!({})),
            _ => Err("unexpected".into()),
        });
        let admin = connect(&state);
        login(&admin, Some("anthropic".into()), false)
            .await
            .unwrap();
        assert!(login(&admin, Some("nope".into()), false).await.is_err());
        list_models(&admin).unwrap();
        select_model(&admin, "anthropic", "claude").unwrap();
        assert_eq!(
            operations(&fake),
            [
                "providers",
                "auth.start",
                "auth.status",
                "providers",
                "providers",
                "model.select"
            ]
        );
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests[1]["type"], "oauth");
        assert_eq!(requests[5]["model"], "claude");
    }
}
