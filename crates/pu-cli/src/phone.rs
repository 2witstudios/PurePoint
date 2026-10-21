//! Headless Point Guard phone host (`pu phone`): where the bridge lives, how
//! the owner reaches its admin socket, the systemd unit, and the terminal QR.
//!
//! Zero-trust stance: the CLI verifies the admin socket's directory and file
//! ownership and modes before sending the owner token, never reads a token
//! from a group/other-accessible file, and talks only to the exact socket the
//! bridge creates under the private state directory.

use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::CliError;

pub const UNIT: &str = "pu-pointguard.service";
pub const DEFAULT_PORT: u16 = 8787;
const ADMIN_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE: usize = 1024 * 1024;

fn home() -> Result<PathBuf, CliError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| CliError::Other("HOME is not set to an absolute path".into()))
}

/// `${XDG_STATE_HOME:-~/.local/state}/purepoint/point-guard`, matching the
/// bridge's own default. A relative XDG value is ignored, never resolved.
pub fn state_dir_from(xdg_state: Option<&str>, home: &Path) -> PathBuf {
    xdg_state
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/state"))
        .join("purepoint/point-guard")
}

pub fn state_dir() -> Result<PathBuf, CliError> {
    Ok(state_dir_from(
        std::env::var("XDG_STATE_HOME").ok().as_deref(),
        &home()?,
    ))
}

/// Versioned bridge payloads: `~/.pu/pointguard/<version>/`.
pub fn payload_root() -> Result<PathBuf, CliError> {
    Ok(home()?.join(".pu/pointguard"))
}

pub fn payload_dir(version: &str) -> Result<PathBuf, CliError> {
    Ok(payload_root()?.join(version))
}

pub fn admin_socket(state: &Path) -> PathBuf {
    state.join("admin/admin.sock")
}

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Owner-only check on an entry's metadata (never follows symlinks).
pub fn private_entry(meta: &std::fs::Metadata, expect_dir: bool, uid: u32) -> bool {
    !meta.file_type().is_symlink()
        && (if expect_dir { meta.is_dir() } else { true })
        && meta.uid() == uid
        && meta.permissions().mode() & 0o077 == 0
}

fn require_private(path: &Path, expect_dir: bool, what: &str) -> Result<(), CliError> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CliError::Other(format!(
                "Point Guard is not running ({what} missing). Start it with `pu phone start`."
            ))
        } else {
            CliError::Other(format!("cannot inspect {}: {e}", path.display()))
        }
    })?;
    if !private_entry(&meta, expect_dir, euid()) {
        return Err(CliError::Other(format!(
            "{} is not private to this user; refusing to use it",
            path.display()
        )));
    }
    Ok(())
}

/// Read the owner admin token, only from a private regular file.
pub fn read_token(file: &Path) -> Result<String, CliError> {
    let meta = std::fs::symlink_metadata(file)
        .map_err(|e| CliError::Other(format!("cannot read owner token: {e}")))?;
    if !meta.is_file() || !private_entry(&meta, false, euid()) || meta.len() > 4096 {
        return Err(CliError::Other(
            "owner token is not a private regular file; refusing to use it".into(),
        ));
    }
    let token = std::fs::read_to_string(file)?.trim().to_string();
    if token.len() < 32 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(CliError::Other("owner token is malformed".into()));
    }
    Ok(token)
}

/// Parse an HTTP/1.1 response read to EOF (the server sends `Connection: close`).
pub fn parse_http_response(raw: &[u8]) -> Result<(u16, Value), CliError> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| CliError::Other("malformed admin response".into()))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|_| CliError::Other("malformed admin response".into()))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| CliError::Other("malformed admin status line".into()))?;
    let chunked = head.lines().any(|l| {
        l.to_ascii_lowercase()
            .replace(' ', "")
            .starts_with("transfer-encoding:chunked")
    });
    let mut body = raw[split + 4..].to_vec();
    if chunked {
        body = dechunk(&body)?;
    }
    let value = serde_json::from_slice(&body)
        .map_err(|_| CliError::Other(format!("admin returned HTTP {status} without JSON")))?;
    Ok((status, value))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, CliError> {
    let bad = || CliError::Other("malformed chunked admin response".into());
    let mut out = Vec::new();
    loop {
        let end = data.windows(2).position(|w| w == b"\r\n").ok_or_else(bad)?;
        let size_text = std::str::from_utf8(&data[..end]).map_err(|_| bad())?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| bad())?;
        data = &data[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err(bad());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

/// The owner admin channel of a running bridge.
pub struct Admin {
    socket: PathBuf,
    token: String,
}

impl Admin {
    /// Verify the private state, socket directory and socket before trusting them.
    pub fn connect(state: &Path) -> Result<Self, CliError> {
        require_private(state, true, "state directory")?;
        let socket = admin_socket(state);
        require_private(socket.parent().expect("admin dir"), true, "admin directory")?;
        let meta = std::fs::symlink_metadata(&socket).map_err(|_| {
            CliError::Other(
                "Point Guard is not running (no admin socket). Start it with `pu phone start`."
                    .into(),
            )
        })?;
        use std::os::unix::fs::FileTypeExt;
        if !meta.file_type().is_socket() || meta.uid() != euid() {
            return Err(CliError::Other(format!(
                "{} is not this user's admin socket; refusing to use it",
                socket.display()
            )));
        }
        let token = read_token(&state.join("admin-token"))?;
        Ok(Admin { socket, token })
    }

    /// Run one admin operation. Errors carry the bridge's sanitized message.
    pub fn call(&self, request: Value) -> Result<Value, CliError> {
        let body = serde_json::to_vec(&request)?;
        let mut stream = UnixStream::connect(&self.socket).map_err(|e| {
            CliError::Other(format!(
                "cannot reach Point Guard admin ({e}). Is it running? Try `pu phone status`."
            ))
        })?;
        stream.set_read_timeout(Some(ADMIN_TIMEOUT))?;
        stream.set_write_timeout(Some(ADMIN_TIMEOUT))?;
        let head = format!(
            "POST /admin/v1 HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.token,
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(&body)?;
        let mut raw = Vec::new();
        stream
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut raw)?;
        if raw.len() > MAX_RESPONSE {
            return Err(CliError::Other("admin response too large".into()));
        }
        let (status, value) = parse_http_response(&raw)?;
        if status == 200 && value["ok"] == json!(true) {
            return Ok(value["result"].clone());
        }
        let message = value["error"]["message"]
            .as_str()
            .unwrap_or("Point Guard rejected the request")
            .to_string();
        Err(CliError::Other(message))
    }
}

/// The payload's `runtime-manifest.json`, checked against this `pu`.
pub fn read_manifest(payload: &Path) -> Result<Value, CliError> {
    let file = payload.join("runtime-manifest.json");
    let text = std::fs::read_to_string(&file).map_err(|_| {
        CliError::Other(format!(
            "phone access is not installed for pu {} ({} missing). Install it with:\n  curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | PU_WITH_PHONE=1 sh",
            env!("CARGO_PKG_VERSION"),
            file.display()
        ))
    })?;
    let manifest: Value = serde_json::from_str(&text)?;
    check_manifest(&manifest, env!("CARGO_PKG_VERSION"))?;
    Ok(manifest)
}

pub fn check_manifest(manifest: &Value, pu_version: &str) -> Result<(), CliError> {
    if manifest["schemaVersion"] != json!(1) || manifest["layout"] != json!("linux") {
        return Err(CliError::Other("unsupported phone access payload".into()));
    }
    let built_for = manifest["puVersion"].as_str().unwrap_or("");
    if built_for != pu_version {
        return Err(CliError::Other(format!(
            "phone access payload is for pu {built_for}, but this pu is {pu_version}; reinstall with PU_WITH_PHONE=1"
        )));
    }
    Ok(())
}

/// systemd quoting for one ExecStart/Environment word.
pub fn systemd_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// Variables a user manager may have imported that must never reach the
/// bridge or the Pi child it launches.
pub const UNSET_ENVIRONMENT: &[&str] = &[
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_EXTRA_CA_CERTS",
    "PI_MOBILE_HOST",
    "PI_MOBILE_CWD",
    "PI_MOBILE_SESSION",
    "PI_MOBILE_TOKEN_FILE",
    "PI_MOBILE_PU_SKILL",
    "PI_CODING_AGENT_DIR",
    "POINT_GUARD_WORKFLOW_SOURCE",
    "POINT_GUARD_APP_LIFETIME",
    "POINT_GUARD_CLIENT_ID",
    "POINT_GUARD_INSTANCE_ID",
    "POINT_GUARD_PHONE",
    "PU_AGENT_ID",
    "PU_PROJECT_ROOT",
    "PU_ENGINE_PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
];

pub struct UnitPaths<'a> {
    pub payload: &'a Path,
    pub state: &'a Path,
    pub home: &'a Path,
    pub pu: &'a Path,
    pub port: u16,
}

/// The explicit environment the bridge runs with (unit and `pu phone run`).
pub fn bridge_environment(p: &UnitPaths) -> Vec<(String, String)> {
    let pu_home = p.home.join(".pu");
    vec![
        ("HOME".into(), p.home.display().to_string()),
        (
            "PATH".into(),
            format!("{}:/usr/bin:/bin", pu_home.join("bin").display()),
        ),
        ("POINT_GUARD_STATE_DIR".into(), p.state.display().to_string()),
        ("POINT_GUARD_PU_PATH".into(), p.pu.display().to_string()),
        ("POINT_GUARD_PU_HOME".into(), pu_home.display().to_string()),
        (
            "POINT_GUARD_LOCK_HELPER_PATH".into(),
            p.payload.join("bin/point-guard-lock").display().to_string(),
        ),
        ("POINT_GUARD_SUPERVISOR".into(), "systemd".into()),
        ("PI_MOBILE_PORT".into(), p.port.to_string()),
    ]
}

/// `Restart=on-failure`: an idle `runtime.stop` (exit 0) stays stopped.
/// `KillMode=mixed`: SIGTERM reaches the bridge, which drains its Pi child.
pub fn systemd_unit(p: &UnitPaths) -> String {
    let mut env = String::new();
    for (key, value) in bridge_environment(p) {
        env.push_str(&format!(
            "Environment={}\n",
            systemd_quote(&format!("{key}={value}"))
        ));
    }
    format!(
        "\
[Unit]
Description=PurePoint Point Guard phone bridge
Documentation=https://github.com/2witstudios/purepoint
Wants=pu-engine.service
After=pu-engine.service

[Service]
Type=simple
ExecStart={node} {entry} --managed
WorkingDirectory={home}
UnsetEnvironment={unset}
{env}UMask=0077
NoNewPrivileges=yes
Restart=on-failure
RestartSec=5
KillMode=mixed
TimeoutStopSec=30

[Install]
WantedBy=default.target
",
        node = systemd_quote(&p.payload.join("bin/node").to_string_lossy()),
        entry = systemd_quote(&p.payload.join("bridge/main.js").to_string_lossy()),
        home = systemd_quote(&p.home.to_string_lossy()),
        unset = UNSET_ENVIRONMENT.join(" "),
    )
}

pub fn unit_file() -> Result<PathBuf, CliError> {
    Ok(std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(home()?.join(".config"))
        .join("systemd/user")
        .join(UNIT))
}

/// Payload directory an installed unit points at, if any.
pub fn unit_payload(unit: &str) -> Option<PathBuf> {
    let line = unit.lines().find(|l| l.starts_with("ExecStart="))?;
    let rest = line.strip_prefix("ExecStart=\"")?;
    let node = rest.split('"').next()?;
    Some(Path::new(node).parent()?.parent()?.to_path_buf())
}

/// Render a QR as terminal text: explicit black-on-white so dark themes don't
/// invert it, a 4-module quiet zone, and two modules per character row.
pub fn render_qr(payload: &str) -> Result<(String, usize), CliError> {
    let code = qrcode::QrCode::with_error_correction_level(payload, qrcode::EcLevel::M)
        .map_err(|e| CliError::Other(format!("cannot encode QR: {e}")))?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 4usize;
    let size = width + 2 * quiet;
    let dark = |x: usize, y: usize| -> bool {
        if x < quiet || y < quiet || x >= width + quiet || y >= width + quiet {
            return false;
        }
        colors[(y - quiet) * width + (x - quiet)] == qrcode::Color::Dark
    };
    let mut out = String::new();
    let mut y = 0;
    while y < size {
        out.push_str("\x1b[30;107m");
        for x in 0..size {
            let top = dark(x, y);
            let bottom = y + 1 < size && dark(x, y + 1);
            out.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        out.push_str("\x1b[0m\n");
        y += 2;
    }
    Ok((out, size))
}

/// Resolve a device by exact name or unique id prefix among active devices.
pub fn resolve_device<'a>(devices: &'a [Value], query: &str) -> Result<&'a Value, CliError> {
    let active: Vec<&Value> = devices
        .iter()
        .filter(|d| d["revokedAt"].is_null())
        .collect();
    let by_name: Vec<&Value> = active
        .iter()
        .copied()
        .filter(|d| d["name"].as_str() == Some(query))
        .collect();
    let matches = if by_name.is_empty() {
        active
            .into_iter()
            .filter(|d| {
                query.len() >= 4
                    && d["deviceId"]
                        .as_str()
                        .is_some_and(|id| id.starts_with(&query.to_ascii_lowercase()))
            })
            .collect()
    } else {
        by_name
    };
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(CliError::Other(format!(
            "no paired device named {query:?} (or id prefix of at least 4 characters); see `pu phone devices`"
        ))),
        _ => Err(CliError::Other(format!(
            "{query:?} matches several devices; use a longer id prefix"
        ))),
    }
}

/// Point Guard chat access implied by a device's grants: chat can run `pu`, so
/// sending needs the broadest grant.
pub fn chat_level(device: &Value) -> &'static str {
    match (
        device["terminals"].as_str().unwrap_or("off"),
        device["workspaceManagement"].as_bool().unwrap_or(false),
    ) {
        ("control", true) => "send",
        ("view" | "control", _) => "read",
        _ => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_xdg_state_should_use_it_only_when_absolute() {
        let home = Path::new("/home/u");
        assert_eq!(
            state_dir_from(None, home),
            PathBuf::from("/home/u/.local/state/purepoint/point-guard")
        );
        assert_eq!(
            state_dir_from(Some("/s"), home),
            PathBuf::from("/s/purepoint/point-guard")
        );
        assert_eq!(
            state_dir_from(Some("rel"), home),
            PathBuf::from("/home/u/.local/state/purepoint/point-guard")
        );
    }

    #[test]
    fn given_http_responses_should_parse_plain_and_chunked_bodies() {
        let plain = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"ok\":true,\"result\":{\"a\":1}}";
        let (status, value) = parse_http_response(plain).unwrap();
        assert_eq!(status, 200);
        assert_eq!(value["result"]["a"], 1);
        let chunked = b"HTTP/1.1 401 Unauthorized\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"ok\"\r\n6\r\n:false}\r\n0\r\n\r\n";
        let (status, value) = parse_http_response(chunked).unwrap();
        assert_eq!(status, 401);
        assert_eq!(value["ok"], false);
        assert!(parse_http_response(b"garbage").is_err());
        assert!(parse_http_response(b"HTTP/1.1 200 OK\r\n\r\nnot json").is_err());
    }

    #[test]
    fn given_manifest_should_require_linux_layout_and_matching_version() {
        let ok = json!({"schemaVersion": 1, "layout": "linux", "puVersion": "1.2.3"});
        assert!(check_manifest(&ok, "1.2.3").is_ok());
        let err = check_manifest(&ok, "1.2.4").unwrap_err().to_string();
        assert!(err.contains("1.2.3") && err.contains("1.2.4"), "{err}");
        assert!(check_manifest(&json!({"schemaVersion": 1, "layout": "mac", "puVersion": "1.2.3"}), "1.2.3").is_err());
        assert!(check_manifest(&json!({}), "1.2.3").is_err());
    }

    fn sample_unit() -> String {
        systemd_unit(&UnitPaths {
            payload: Path::new("/home/a b/.pu/pointguard/1.2.3"),
            state: Path::new("/home/a b/.local/state/purepoint/point-guard"),
            home: Path::new("/home/a b"),
            pu: Path::new("/home/a b/.pu/bin/pu"),
            port: 8787,
        })
    }

    #[test]
    fn given_paths_should_render_hardened_unit_with_explicit_environment() {
        let unit = sample_unit();
        assert!(unit.contains(
            "ExecStart=\"/home/a b/.pu/pointguard/1.2.3/bin/node\" \"/home/a b/.pu/pointguard/1.2.3/bridge/main.js\" --managed\n"
        ), "{unit}");
        assert!(unit.contains("UMask=0077\n"));
        assert!(unit.contains("NoNewPrivileges=yes\n"));
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("UnsetEnvironment=NODE_OPTIONS NODE_PATH"));
        assert!(unit.contains("LD_PRELOAD"));
        assert!(unit.contains("Environment=\"POINT_GUARD_SUPERVISOR=systemd\"\n"));
        assert!(unit.contains("Environment=\"PATH=/home/a b/.pu/bin:/usr/bin:/bin\"\n"));
        assert!(unit.contains(
            "Environment=\"POINT_GUARD_LOCK_HELPER_PATH=/home/a b/.pu/pointguard/1.2.3/bin/point-guard-lock\"\n"
        ));
        assert!(unit.contains("Wants=pu-engine.service\n"));
        assert!(!unit.contains("PI_MOBILE_HOST="));
    }

    #[test]
    fn given_unit_should_recover_the_payload_it_runs() {
        assert_eq!(
            unit_payload(&sample_unit()),
            Some(PathBuf::from("/home/a b/.pu/pointguard/1.2.3"))
        );
        assert_eq!(unit_payload("[Service]\n"), None);
    }

    #[test]
    fn given_specials_should_escape_systemd_words() {
        assert_eq!(systemd_quote(r#"a"b\c%d"#), r#""a\"b\\c%%d""#);
    }

    #[test]
    fn given_payload_should_render_scannable_sized_black_on_white_qr() {
        let payload = r#"{"type":"pi-mobile-pairing","version":1,"endpoint":"wss://100.101.102.103:8787/v1","hostId":"6f1c2c1e-4a5b-4c6d-8e9f-0a1b2c3d4e5f","certificateSHA256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","enrollmentToken":"AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCd","expiresAt":1790000000000}"#;
        let (text, size) = render_qr(payload).unwrap();
        assert!(size <= 80, "fits an 80-column terminal: {size}");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), size.div_ceil(2));
        assert!(lines.iter().all(|l| l.starts_with("\x1b[30;107m") && l.ends_with("\x1b[0m")));
        // Quiet zone: the first character row is blank.
        assert!(lines[0].trim_start_matches("\x1b[30;107m").starts_with("    "));
    }

    #[test]
    fn given_devices_should_resolve_by_name_or_unique_prefix_among_active() {
        let devices = vec![
            json!({"deviceId": "aaaa1111-0000-4000-8000-000000000000", "name": "iPhone"}),
            json!({"deviceId": "aaaa2222-0000-4000-8000-000000000000", "name": "iPad"}),
            json!({"deviceId": "bbbb3333-0000-4000-8000-000000000000", "name": "Old", "revokedAt": 1}),
        ];
        assert_eq!(resolve_device(&devices, "iPad").unwrap()["name"], "iPad");
        assert_eq!(resolve_device(&devices, "AAAA1").unwrap()["name"], "iPhone");
        assert!(resolve_device(&devices, "aaaa").is_err(), "ambiguous");
        assert!(resolve_device(&devices, "aaa").is_err(), "prefix too short");
        assert!(resolve_device(&devices, "Old").is_err(), "revoked");
        assert!(resolve_device(&devices, "bbbb").is_err(), "revoked prefix");
    }

    #[test]
    fn given_grants_should_derive_chat_level() {
        assert_eq!(chat_level(&json!({})), "none");
        assert_eq!(chat_level(&json!({"terminals": "view"})), "read");
        assert_eq!(chat_level(&json!({"terminals": "control"})), "read");
        assert_eq!(chat_level(&json!({"terminals": "control", "workspaceManagement": true})), "send");
        assert_eq!(chat_level(&json!({"terminals": "off", "workspaceManagement": true})), "none");
    }

    #[test]
    fn given_modes_should_accept_only_owner_private_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t");
        std::fs::write(&file, "x".repeat(43)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_token(&file).unwrap(), "x".repeat(43));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_token(&file).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("l");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(read_token(&link).is_err());
        std::fs::write(&file, "short").unwrap();
        assert!(read_token(&file).is_err());
    }
}
