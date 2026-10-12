//! Mesh run guardian: the worker-side process-lifetime supervisor for
//! remotely launched agent runs. See `README.md` in this directory for the
//! contract, limits and evidence.
//!
//! The daemon execs `pu-engine --mesh-guardian ... -- <command> [args]` as the
//! PTY session leader. The guardian forks the agent (same session and tty),
//! injects `PU_MESH_RUN_TOKEN=<token>` into the agent's environment only, and
//! tears the whole run tree down when the daemon dies, the agent exits, or it
//! receives SIGTERM/SIGINT/SIGHUP. It then writes a durable outcome record.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use pu_core::private_fs::{PrivateDir, denied};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(target_os = "linux")]
#[path = "platform_linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod platform;
mod supervisor;

/// Environment variable carrying the run token inside the agent's tree.
pub const RUN_TOKEN_ENV: &str = "PU_MESH_RUN_TOKEN";
/// Environment variable `wrap_spawn` uses to hand the token to the guardian.
pub const GUARDIAN_TOKEN_ENV: &str = "PU_MESH_GUARDIAN_TOKEN";
/// Default SIGTERM-to-SIGKILL grace period.
pub const DEFAULT_GRACE: Duration = Duration::from_millis(2000);
const MAX_GRACE_MS: u64 = 60_000;
const MAX_RUN_ID: usize = 64;
const MIN_TOKEN: usize = 32;
const MAX_TOKEN: usize = 1024;
const MAX_OUTCOME_BYTES: u64 = 64 * 1024;

/// Exit codes for refusals before the agent is forked.
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_OUTCOME_DIR: i32 = 3;

/// What the engine needs to launch one guarded run.
pub struct GuardianLaunch {
    pub run_id: String,
    pub token: String,
    pub outcome_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    DaemonExit,
    AgentExit,
    Signal,
}

/// Durable record of how a run ended. Never contains the raw token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub run_id: String,
    /// SHA-256 hex of the run token.
    pub token_digest: String,
    /// True when no tracked or token-marked process survived termination.
    pub verified: bool,
    /// Agent exit code, or 128+signal; None if it was never reaped.
    pub agent_exit: Option<i32>,
    pub trigger: Trigger,
    pub survivors: Vec<u32>,
    pub finished_at: String,
}

/// Turn an agent command into a guarded one. The token travels in the
/// returned env (`GUARDIAN_TOKEN_ENV`), never on argv.
pub fn wrap_spawn(
    launch: &GuardianLaunch,
    exe: &Path,
    parent: u32,
    command: String,
    args: Vec<String>,
) -> (String, Vec<String>, Vec<(String, String)>) {
    let mut wrapped = vec![
        "--mesh-guardian".to_string(),
        "--run".into(),
        launch.run_id.clone(),
        "--token-env".into(),
        GUARDIAN_TOKEN_ENV.into(),
        "--parent".into(),
        parent.to_string(),
        "--outcome-dir".into(),
        launch.outcome_dir.to_string_lossy().into_owned(),
        "--".into(),
        command,
    ];
    wrapped.extend(args);
    (
        exe.to_string_lossy().into_owned(),
        wrapped,
        vec![(GUARDIAN_TOKEN_ENV.to_string(), launch.token.clone())],
    )
}

/// 256-bit random token as lowercase hex, from the OS CSPRNG.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    // SAFETY: getentropy fills at most 256 bytes into a live buffer.
    let rc = unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) };
    assert_eq!(rc, 0, "getentropy failed: {}", io::Error::last_os_error());
    hex(&bytes)
}

/// `(mechanism, token_digest)` describing how this worker supervises runs.
pub fn supervision_identity(token: &str) -> (String, String) {
    (platform::MECHANISM.to_string(), token_digest(token))
}

pub fn token_digest(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn validate_token(token: &str) -> Result<(), String> {
    if token.len() < MIN_TOKEN || token.len() > MAX_TOKEN {
        return Err(format!("token must be {MIN_TOKEN}..={MAX_TOKEN} hex chars"));
    }
    if !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("token must be hex".into());
    }
    Ok(())
}

pub fn validate_run_id(run_id: &str) -> Result<(), String> {
    if run_id.is_empty()
        || run_id.len() > MAX_RUN_ID
        || !run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(format!("run id must be 1..={MAX_RUN_ID} of [A-Za-z0-9_-]"));
    }
    Ok(())
}

fn outcome_name(run_id: &str) -> String {
    format!("{run_id}.outcome.json")
}

/// Parsed `--mesh-guardian` arguments (everything after the mode flag).
#[derive(Debug)]
pub struct GuardianArgs {
    pub run_id: String,
    pub token_env: String,
    pub parent: u32,
    pub outcome_dir: PathBuf,
    pub grace: Duration,
    pub command: String,
    pub args: Vec<String>,
}

pub fn parse_args(args: &[String]) -> Result<GuardianArgs, String> {
    let split = args
        .iter()
        .position(|a| a == "--")
        .ok_or("missing -- before agent command")?;
    let (flags, rest) = (&args[..split], &args[split + 1..]);
    let (command, agent_args) = rest.split_first().ok_or("missing agent command")?;
    let mut values: HashMap<&str, &str> = HashMap::new();
    let mut it = flags.iter();
    while let Some(flag) = it.next() {
        let name = flag.as_str();
        if !matches!(
            name,
            "--run" | "--token-env" | "--parent" | "--outcome-dir" | "--grace-ms"
        ) {
            return Err(format!("unknown guardian flag {name:?}"));
        }
        let value = it.next().ok_or_else(|| format!("{name} needs a value"))?;
        if values.insert(name, value).is_some() {
            return Err(format!("{name} given twice"));
        }
    }
    let need = |k: &str| values.get(k).copied().ok_or_else(|| format!("missing {k}"));
    let run_id = need("--run")?.to_string();
    validate_run_id(&run_id)?;
    let token_env = need("--token-env")?.to_string();
    if token_env.is_empty()
        || token_env.len() > 128
        || token_env == RUN_TOKEN_ENV
        || !token_env
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("--token-env must be an [A-Z0-9_] name other than the run token".into());
    }
    let parent: u32 = need("--parent")?
        .parse()
        .map_err(|_| "--parent must be a pid")?;
    if parent <= 1 || parent > i32::MAX as u32 {
        return Err("--parent must be a real parent pid".into());
    }
    let grace = match values.get("--grace-ms") {
        Some(v) => {
            let ms: u64 = v.parse().map_err(|_| "--grace-ms must be an integer")?;
            if ms > MAX_GRACE_MS {
                return Err(format!("--grace-ms must be <= {MAX_GRACE_MS}"));
            }
            Duration::from_millis(ms)
        }
        None => DEFAULT_GRACE,
    };
    if command.is_empty() {
        return Err("empty agent command".into());
    }
    Ok(GuardianArgs {
        run_id,
        token_env,
        parent,
        outcome_dir: PathBuf::from(need("--outcome-dir")?),
        grace,
        command: command.clone(),
        args: agent_args.to_vec(),
    })
}

/// True when the NUL-separated env block contains exactly `entry`.
fn env_block_has_entry(block: &[u8], entry: &[u8]) -> bool {
    block.split(|&b| b == 0).any(|e| e == entry)
}

/// True when following ppid links from `pid` reaches `root` (excluding root).
fn chain_reaches(parents: &HashMap<i32, i32>, pid: i32, root: i32) -> bool {
    let mut current = pid;
    for _ in 0..4096 {
        if current == root {
            return current != pid;
        }
        match parents.get(&current) {
            Some(&ppid) if ppid != current && ppid > 0 => current = ppid,
            _ => return false,
        }
    }
    false
}

/// Entry point for `pu-engine --mesh-guardian <args>`; returns the exit code.
pub fn run_guardian(args: &[String]) -> i32 {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pu-engine guardian: {e}");
            return EXIT_USAGE;
        }
    };
    let token = match std::env::var(&parsed.token_env) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("pu-engine guardian: token env {} not set", parsed.token_env);
            return EXIT_USAGE;
        }
    };
    // SAFETY: the guardian is single-threaded; nothing reads env concurrently.
    unsafe { std::env::remove_var(&parsed.token_env) };
    if let Err(e) = validate_token(&token) {
        eprintln!("pu-engine guardian: {e}");
        return EXIT_USAGE;
    }
    if std::env::args().any(|a| a.contains(&token)) {
        eprintln!("pu-engine guardian: refusing: run token appears on argv");
        return EXIT_USAGE;
    }
    // Validate the outcome directory before anything is forked.
    let dir = match PrivateDir::open(&parsed.outcome_dir, false) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("pu-engine guardian: unsafe outcome dir: {e}");
            return EXIT_OUTCOME_DIR;
        }
    };
    match dir.entry(OsStr::new(&outcome_name(&parsed.run_id))) {
        Ok(None) => {}
        Ok(Some(_)) => {
            eprintln!("pu-engine guardian: outcome for run already exists");
            return EXIT_OUTCOME_DIR;
        }
        Err(e) => {
            eprintln!("pu-engine guardian: outcome dir: {e}");
            return EXIT_OUTCOME_DIR;
        }
    }

    // The lease stays open (inheritable) for the guardian's lifetime.
    let (_lease, lease_id) = match open_lease(&dir, &parsed.run_id) {
        Ok(lease) => lease,
        Err(e) => {
            eprintln!("pu-engine guardian: lease setup failed: {e}");
            return EXIT_OUTCOME_DIR;
        }
    };
    let result = match supervisor::supervise(&parsed, &token, lease_id) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pu-engine guardian: {e}");
            return EXIT_USAGE;
        }
    };
    let outcome = Outcome {
        run_id: parsed.run_id.clone(),
        token_digest: token_digest(&token),
        verified: result.survivors.is_empty(),
        agent_exit: result.agent_exit,
        trigger: result.trigger,
        survivors: result.survivors,
        finished_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = write_outcome(&dir, &outcome) {
        eprintln!("pu-engine guardian: failed to write outcome: {e}");
    }
    outcome.agent_exit.unwrap_or_else(|| match result.signal {
        Some(sig) => 128 + sig,
        None => 1,
    })
}

/// Create an unlinked private file whose descriptor the agent inherits. Every
/// process still holding it (by dev/ino) is a run member, even when its
/// environment is unreadable or scrubbed.
fn open_lease(dir: &PrivateDir, run_id: &str) -> io::Result<(std::fs::File, (u64, u64))> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    let name = format!(".{run_id}.{}.lease", std::process::id());
    if dir.entry(OsStr::new(&name))?.is_some() {
        return Err(denied("stale lease file"));
    }
    let file = dir.open_file(OsStr::new(&name), true)?;
    dir.remove_file(OsStr::new(&name), &file)?;
    let meta = file.metadata()?;
    // SAFETY: clearing FD_CLOEXEC on a descriptor this function owns.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((file, (meta.dev(), meta.ino())))
}

/// Write to an exclusive 0600 no-follow temp file, fsync, then rename into
/// place only if no outcome exists yet.
fn write_outcome(dir: &PrivateDir, outcome: &Outcome) -> io::Result<()> {
    let name = outcome_name(&outcome.run_id);
    let tmp = format!(".{}.{}.tmp", outcome.run_id, std::process::id());
    if dir.entry(OsStr::new(&tmp))?.is_some() {
        return Err(denied("stale outcome temp file"));
    }
    let mut file = dir.open_file(OsStr::new(&tmp), true)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec(outcome).map_err(io::Error::other)?)?;
        file.sync_all()?;
        dir.verify()?;
        if dir.entry(OsStr::new(&name))?.is_some() {
            return Err(denied("outcome for run already exists"));
        }
        std::fs::rename(
            dir.checked_path_for(OsStr::new(&tmp))?,
            dir.checked_path_for(OsStr::new(&name))?,
        )?;
        dir.sync()
    })();
    if result.is_err() {
        dir.remove_file(OsStr::new(&tmp), &file).ok();
    }
    result
}

/// Read and validate the outcome for `run_id`. `Ok(None)` while the run has
/// not finished. Errors when the directory or file is unsafe, the record is
/// malformed, or it was not written for `token` (digest mismatch).
pub fn read_outcome(outcome_dir: &Path, run_id: &str, token: &str) -> io::Result<Option<Outcome>> {
    validate_token(token).map_err(|e| denied(&e))?;
    read_outcome_by_digest(outcome_dir, run_id, &token_digest(token))
}

/// Restart reconciliation holds only the durable token digest, never the token.
pub fn read_outcome_by_digest(
    outcome_dir: &Path,
    run_id: &str,
    expected_digest: &str,
) -> io::Result<Option<Outcome>> {
    validate_run_id(run_id).map_err(|e| denied(&e))?;
    if expected_digest.len() != 64 || !expected_digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(denied("invalid token digest"));
    }
    let dir = PrivateDir::open(outcome_dir, false)?;
    let name = outcome_name(run_id);
    if dir.entry(OsStr::new(&name))?.is_none() {
        return Ok(None);
    }
    let file = dir.open_file(OsStr::new(&name), false)?;
    let mut bytes = Vec::new();
    file.take(MAX_OUTCOME_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_OUTCOME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "outcome too large",
        ));
    }
    let outcome: Outcome = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if outcome.run_id != run_id {
        return Err(denied("outcome run id mismatch"));
    }
    if !constant_time_eq(outcome.token_digest.as_bytes(), expected_digest.as_bytes()) {
        return Err(denied("outcome token digest mismatch"));
    }
    Ok(Some(outcome))
}

/// Remove a consumed outcome record (after the ledger recorded its proof).
/// Missing records are fine; the directory must still be private.
pub fn discard_outcome(outcome_dir: &Path, run_id: &str) -> io::Result<()> {
    validate_run_id(run_id).map_err(|e| denied(&e))?;
    let dir = PrivateDir::open(outcome_dir, false)?;
    let name = outcome_name(run_id);
    if dir.entry(OsStr::new(&name))?.is_none() {
        return Ok(());
    }
    let file = dir.open_file(OsStr::new(&name), false)?;
    dir.remove_file(OsStr::new(&name), &file)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests;
