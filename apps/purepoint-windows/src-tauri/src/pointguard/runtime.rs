//! Where the Point Guard runtime lives and how its files are trusted (the Windows side of
//! `Models/PointGuardRuntime.swift`, used by the service).
//!
//! Layout:
//! - Bundled (every build): `<resource dir>\PointGuard\` holds `runtime-manifest.json`
//!   (`layout: "windows"`, `paths`, `seal`), `bin\node.exe`, `bin\point-guard-lock.exe`,
//!   `bridge\`, `node_modules\`, `docs\`, `support\` and `package.json`, as staged by
//!   `scripts/stage-pointguard-runtime.mjs` (the same layout `pu phone` unpacks). The seal is
//!   checked before every launch (`seal.rs`). `pu.exe` is the app's sidecar (beside the exe or
//!   in the resource dir), else the installed `%USERPROFILE%\.pu\bin\pu.exe`; each candidate
//!   must pass `check_trusted_binary`.
//! - Development (debug builds only, and only with `PUREPOINT_POINTGUARD_DEV=1`): the system
//!   `node` on `PATH`, the repo's `apps/purepoint-mobile/bridge/main.js`, and the repo's
//!   `target\debug\pu.exe` and `pu-point-guard-lock.exe`. Unsealed, as the Mac skips its seal
//!   in Debug.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::seal;

/// The Pi the bridge is built for; a payload for another one is refused (as on the Mac).
pub const PI_VERSION: &str = "1.3.0";
/// Name of the runtime directory inside the resource dir.
pub const BUNDLE_DIR: &str = "PointGuard";
/// Opt-in for the debug-only development layout.
pub const DEV_ENV: &str = "PUREPOINT_POINTGUARD_DEV";

const INCOMPLETE: &str = "Point Guard package is incomplete. Reinstall PurePoint; its bundled resources are missing or invalid.";
const UNSUPPORTED: &str = "Unsupported Point Guard package. Install a supported PurePoint update.";

/// Which layout a runtime came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    Bundled,
    Development,
}

/// The files one launch uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub node: PathBuf,
    pub entry: PathBuf,
    pub lock_helper: PathBuf,
    pub pu: PathBuf,
    pub layout: Layout,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    contract_version: u32,
    layout: String,
    pi_version: String,
    architecture: String,
    paths: Paths,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Paths {
    node: String,
    lock_helper: String,
    entry: String,
    instructions: String,
    skills: Vec<String>,
}

/// The architecture this app was built for, as the manifest names it.
pub fn app_architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Load and seal-check a bundled runtime directory. `pu` is the app's `pu.exe`.
pub fn load_bundled(dir: &Path, pu: Result<PathBuf, String>) -> Result<Runtime, String> {
    // The bridge runs JavaScript from this directory, so refuse to start it unless the
    // directory still matches its manifest.
    let bytes = seal::verify_dir(dir).map_err(|error| {
        eprintln!("PurePoint: Point Guard seal check failed: {error}");
        seal::MODIFIED_MESSAGE.to_string()
    })?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| UNSUPPORTED.to_string())?;
    if manifest.schema_version != 1
        || manifest.contract_version != 1
        || manifest.layout != "windows"
        || manifest.pi_version != PI_VERSION
        || manifest.architecture != app_architecture()
    {
        return Err(UNSUPPORTED.into());
    }
    let resolve = |relative: &str| -> Result<PathBuf, String> {
        if !seal::valid_relative(relative) {
            return Err(INCOMPLETE.into());
        }
        let path = dir.join(relative.replace('/', "\\"));
        if path.is_file() {
            Ok(path)
        } else {
            Err(INCOMPLETE.into())
        }
    };
    let node = resolve(&manifest.paths.node)?;
    let lock_helper = resolve(&manifest.paths.lock_helper)?;
    let entry = resolve(&manifest.paths.entry)?;
    resolve(&manifest.paths.instructions)?;
    for skill in &manifest.paths.skills {
        resolve(skill)?;
    }
    if manifest.paths.skills.len() < 2 {
        return Err("Point Guard package is missing its bundled skills.".into());
    }
    #[cfg(windows)]
    for binary in [&node, &lock_helper] {
        if pu_core::win_security::check_trusted_file(binary).is_err() {
            return Err(format!(
                "{} is writable by other users; Point Guard won't run it. Reinstall PurePoint.",
                binary.display()
            ));
        }
    }
    Ok(Runtime {
        node,
        entry,
        lock_helper,
        pu: pu?,
        layout: Layout::Bundled,
    })
}

/// The first trusted `pu.exe` among the app's sidecar locations and the user's install.
pub fn find_pu(dirs: &[PathBuf], home: Option<&Path>) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = dirs.iter().map(|d| d.join("pu.exe")).collect();
    if let Some(home) = home {
        candidates.push(home.join(".pu").join("bin").join("pu.exe"));
    }
    for candidate in &candidates {
        if !candidate.is_file() {
            continue;
        }
        #[cfg(windows)]
        if pu_core::win_security::check_trusted_binary(candidate).is_err() {
            continue;
        }
        return Ok(candidate.clone());
    }
    Err("The bundled pu CLI is missing. Reinstall PurePoint.".into())
}

/// The repo root of this source tree (debug builds run from it).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

/// The debug-only development runtime: the system `node`, the repo's bridge and binaries.
pub fn development(repo: &Path) -> Result<Runtime, String> {
    let node = which::which("node")
        .map_err(|_| "Development Point Guard needs Node.js on PATH.".to_string())?;
    let entry = repo.join("apps/purepoint-mobile/bridge/main.js");
    let debug = repo.join("target").join("debug");
    let pu = debug.join("pu.exe");
    let lock_helper = debug.join("pu-point-guard-lock.exe");
    for (path, hint) in [
        (&entry, "the repo's apps/purepoint-mobile bridge"),
        (&pu, "`cargo build -p pu-cli`"),
        (&lock_helper, "`cargo build -p pu-cli`"),
    ] {
        if !path.is_file() {
            return Err(format!(
                "Development Point Guard is missing {} ({hint}).",
                path.display()
            ));
        }
    }
    Ok(Runtime {
        node,
        entry,
        lock_helper,
        pu,
        layout: Layout::Development,
    })
}

/// Pick the runtime: the bundled one when present, else the development one in a debug
/// build that opted in.
pub fn locate(
    resource_dir: Option<&Path>,
    pu_dirs: &[PathBuf],
    home: Option<&Path>,
    dev_opt_in: bool,
) -> Result<Runtime, String> {
    if let Some(dir) = resource_dir.map(|r| r.join(BUNDLE_DIR))
        && dir.join(seal::MANIFEST).exists()
    {
        return load_bundled(&dir, find_pu(pu_dirs, home));
    }
    if cfg!(debug_assertions) && dev_opt_in {
        return development(&repo_root());
    }
    Err(INCOMPLETE.into())
}

/// The `admin.json` readiness descriptor the bridge writes once it serves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub schema_version: u32,
    pub contract_version: u32,
    pub pid: u32,
    pub instance_id: String,
    /// The owner pipe (Windows); the Mac's `adminURL` is never used here.
    pub admin_socket: Option<String>,
    #[serde(rename = "nativeChatURL")]
    pub native_chat_url: String,
    #[serde(rename = "chatURL")]
    pub chat_url: Option<String>,
    pub desktop_client_id: String,
    pub host_id: Option<String>,
    #[serde(rename = "certificateSHA256")]
    pub certificate_sha256: Option<String>,
}

impl Descriptor {
    /// Only the exact owned child's launch, serving the pipe for this state directory.
    pub fn belongs(&self, pid: u32, instance_id: &str, expected_pipe: &str) -> bool {
        self.pid == pid
            && self.instance_id == instance_id
            && self.schema_version == 1
            && self.contract_version == 1
            && self.admin_socket.as_deref() == Some(expected_pipe)
            && local_url(&self.native_chat_url, "ws", "/v1")
            && uuid::Uuid::parse_str(&self.desktop_client_id).is_ok()
    }
}

/// `scheme://127.0.0.1:<port><path>` with nothing else (the Swift `localURL`).
pub fn local_url(value: &str, scheme: &str, path: &str) -> bool {
    let Some(rest) = value
        .strip_prefix(scheme)
        .and_then(|r| r.strip_prefix("://127.0.0.1:"))
    else {
        return false;
    };
    let Some((port, tail)) = rest.split_once('/') else {
        return false;
    };
    port.parse::<u16>().is_ok_and(|p| p > 0) && !port.starts_with('0') && format!("/{tail}") == path
}

/// The owner pipe the bridge serves for `state` (`ownerPipeName` in `platform.js`).
pub fn owner_pipe(state: &Path) -> std::io::Result<String> {
    let normalized = pu_transport::naming::normalize_socket_path(&state.join("admin.sock"))?;
    Ok(pu_transport::naming::pipe_name(
        &normalized,
        "pointguard-owner",
    ))
}

/// Read Point Guard state only from a private regular file (`PointGuardPrivateFile.read`).
pub fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    const NOT_PRIVATE: &str = "Point Guard state must be a private regular file. Restore its owner-only permissions before retrying.";
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > limit {
        return Err(NOT_PRIVATE.into());
    }
    pu_core::private_fs::check_private(path).map_err(|_| NOT_PRIVATE.to_string())?;
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.len() as u64 > limit {
        return Err("Point Guard state is too large.".into());
    }
    Ok(data)
}

/// A private credential file (`PointGuardPrivateFile.token`).
pub fn read_token(path: &Path) -> Result<String, String> {
    let data = read_private(path, 4096)?;
    let value = String::from_utf8(data)
        .map(|s| s.trim().to_string())
        .map_err(|_| "Invalid local Point Guard credential.".to_string())?;
    if value.len() < 32 || value.len() > 1024 || !value.bytes().all(|b| (33..=126).contains(&b)) {
        return Err("Invalid local Point Guard credential.".into());
    }
    Ok(value)
}

/// The descriptor for this launch, when the bridge has written a valid one.
pub fn read_descriptor(state: &Path, pid: u32, instance_id: &str) -> Option<Descriptor> {
    let data = read_private(&state.join("admin.json"), 65536).ok()?;
    let descriptor: Descriptor = serde_json::from_slice(&data).ok()?;
    let pipe = owner_pipe(state).ok()?;
    descriptor
        .belongs(pid, instance_id, &pipe)
        .then_some(descriptor)
}

/// The bridge's `error.json` message for this launch (`PointGuardStartupFailure`).
pub fn startup_failure(state: &Path, pid: u32, instance_id: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Failure {
        schema_version: u32,
        pid: u32,
        instance_id: String,
        message: String,
        recovery: String,
    }
    let data = read_private(&state.join("error.json"), 65536).ok()?;
    let failure: Failure = serde_json::from_slice(&data).ok()?;
    (failure.schema_version == 1
        && failure.pid == pid
        && failure.instance_id == instance_id
        && !failure.message.is_empty()
        && failure.message.len() <= 4096
        && failure.recovery.len() <= 4096)
        .then(|| format!("{} {}", failure.message, failure.recovery))
}

/// An existing absolute working folder (`PointGuardWorkingFolder.validated`).
pub fn validated_folder(path: &str) -> Result<PathBuf, String> {
    const CHOOSE: &str = "Choose an existing working folder before starting Point Guard.";
    let raw = Path::new(path);
    if !raw.is_absolute() {
        return Err(CHOOSE.into());
    }
    let canonical = pu_core::canon::canonicalize(raw).map_err(|_| CHOOSE.to_string())?;
    if !canonical.is_dir() {
        return Err(CHOOSE.into());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(pipe: &str) -> Descriptor {
        Descriptor {
            schema_version: 1,
            contract_version: 1,
            pid: 42,
            instance_id: "i".into(),
            admin_socket: Some(pipe.into()),
            native_chat_url: "ws://127.0.0.1:5000/v1".into(),
            chat_url: None,
            desktop_client_id: "6f1c7a52-6f5a-4f0e-9a43-3a0d5f7e9b10".into(),
            host_id: None,
            certificate_sha256: None,
        }
    }

    #[test]
    fn given_descriptor_should_belong_only_to_the_exact_launch() {
        let pipe = r"\\.\pipe\purepoint-0123456789abcdef-pointguard-owner";
        let d = descriptor(pipe);
        assert!(d.belongs(42, "i", pipe));
        assert!(!d.belongs(43, "i", pipe));
        assert!(!d.belongs(42, "j", pipe));
        assert!(!d.belongs(42, "i", r"\\.\pipe\other"));
        let mut remote = d.clone();
        remote.native_chat_url = "ws://0.0.0.0:5000/v1".into();
        assert!(!remote.belongs(42, "i", pipe));
        let mut client = d.clone();
        client.desktop_client_id = "nope".into();
        assert!(!client.belongs(42, "i", pipe));
        let mut version = d;
        version.contract_version = 2;
        assert!(!version.belongs(42, "i", pipe));
    }

    #[test]
    fn given_urls_should_accept_only_plain_loopback() {
        assert!(local_url("ws://127.0.0.1:8787/v1", "ws", "/v1"));
        for bad in [
            "wss://127.0.0.1:8787/v1",
            "ws://localhost:8787/v1",
            "ws://127.0.0.1:0/v1",
            "ws://127.0.0.1:99999/v1",
            "ws://127.0.0.1:8787/v1?x",
            "ws://127.0.0.1:8787/v2",
            "ws://u@127.0.0.1:8787/v1",
            "ws://127.0.0.1:8787",
        ] {
            assert!(!local_url(bad, "ws", "/v1"), "{bad}");
        }
    }

    #[test]
    fn given_bundled_runtime_should_check_the_manifest_contract() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for path in [
            "bin/node.exe",
            "bin/point-guard-lock.exe",
            "bridge/main.js",
            "docs/point-guard.md",
            "support/pu/SKILL.md",
            "support/pu-cli/SKILL.md",
        ] {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, path).unwrap();
        }
        let write = |layout: &str, arch: &str| {
            let _ = std::fs::remove_file(dir.join(seal::MANIFEST));
            let files = seal::seal_files(dir).unwrap();
            let manifest = serde_json::json!({
                "schemaVersion": 1, "contractVersion": 1, "layout": layout,
                "piVersion": PI_VERSION, "architecture": arch,
                "paths": {
                    "node": "bin/node.exe", "lockHelper": "bin/point-guard-lock.exe",
                    "entry": "bridge/main.js", "instructions": "docs/point-guard.md",
                    "skills": ["support/pu/SKILL.md", "support/pu-cli/SKILL.md"],
                },
                "seal": {"algorithm": "sha256", "files": files},
            });
            std::fs::write(dir.join(seal::MANIFEST), manifest.to_string()).unwrap();
        };
        let pu = || Ok(PathBuf::from(r"C:\pu.exe"));
        write("windows", app_architecture());
        let runtime = load_bundled(dir, pu()).unwrap();
        assert_eq!(runtime.node, dir.join(r"bin\node.exe"));
        assert_eq!(runtime.entry, dir.join(r"bridge\main.js"));
        assert_eq!(runtime.layout, Layout::Bundled);
        write("linux", app_architecture());
        assert_eq!(load_bundled(dir, pu()).unwrap_err(), UNSUPPORTED);
        write("windows", "sparc");
        assert_eq!(load_bundled(dir, pu()).unwrap_err(), UNSUPPORTED);
        write("windows", app_architecture());
        std::fs::write(dir.join("bridge/main.js"), "evil()").unwrap();
        assert_eq!(load_bundled(dir, pu()).unwrap_err(), seal::MODIFIED_MESSAGE);
    }

    #[test]
    fn given_no_bundle_in_release_or_without_opt_in_should_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            locate(Some(tmp.path()), &[], None, false).unwrap_err(),
            INCOMPLETE
        );
    }

    #[test]
    fn given_pu_candidates_should_take_the_first_present() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(home.join(".pu/bin")).unwrap();
        assert!(find_pu(std::slice::from_ref(&a), Some(&home)).is_err());
        std::fs::write(home.join(".pu/bin/pu.exe"), "pu").unwrap();
        assert_eq!(
            find_pu(std::slice::from_ref(&a), Some(&home)).unwrap(),
            home.join(".pu/bin/pu.exe")
        );
        std::fs::write(a.join("pu.exe"), "pu").unwrap();
        assert_eq!(
            find_pu(std::slice::from_ref(&a), Some(&home)).unwrap(),
            a.join("pu.exe")
        );
    }

    #[test]
    fn given_folder_should_require_an_existing_absolute_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(validated_folder("relative").is_err());
        assert!(validated_folder(&tmp.path().join("none").to_string_lossy()).is_err());
        let file = tmp.path().join("f");
        std::fs::write(&file, "").unwrap();
        assert!(validated_folder(&file.to_string_lossy()).is_err());
        let ok = validated_folder(&tmp.path().to_string_lossy()).unwrap();
        assert!(ok.is_dir());
        assert!(!ok.to_string_lossy().starts_with(r"\\?\"));
    }
}
