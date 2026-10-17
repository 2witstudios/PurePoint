//! Explicit owner attestation; default denied. This reads no credentials and never
//! contacts the proof URL. A proof record is an operator assertion, not verification.
use serde::Deserialize;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) struct Grant {
    qualified_at_ms: i64,
    expires_at_ms: i64,
    invalidated: AtomicBool,
}
impl Grant {
    pub fn current(&self, now: i64) -> bool {
        // Once this launch observes expiration/rollback, authorization cannot
        // revive under a later clock correction. A new owner-qualified launch
        // must load a current grant explicitly.
        if now < self.qualified_at_ms || now >= self.expires_at_ms {
            self.invalidated.store(true, Ordering::Release);
        }
        !self.invalidated.load(Ordering::Acquire)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u32,
    qualifications: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    tool: String,
    tool_version: String,
    adapter_version: String,
    platform: String,
    execution_mode: String,
    owner_confirmed: bool,
    proof_record_url: String,
    qualified_at_ms: i64,
    expires_at_ms: i64,
}

pub(super) async fn load(
    project: &str,
    tool: &str,
    version: &str,
    args: &[String],
) -> Option<Grant> {
    // Any explicit alternate output/print mode remains unqualified; no RPC proof
    // can authorize the interactive terminal route accidentally.
    let interactive = !args
        .iter()
        .any(|a| matches!(a.as_str(), "--mode" | "--print" | "-p") || a.starts_with("--mode="));
    let project = project.to_owned();
    let tool = tool.to_owned();
    let version = version.to_owned();
    tokio::time::timeout(
        Duration::from_millis(50),
        tokio::task::spawn_blocking(move || {
            load_sync(
                Path::new(&project),
                &tool,
                &version,
                interactive,
                chrono::Utc::now().timestamp_millis(),
            )
        }),
    )
    .await
    .ok()?
    .ok()?
}

fn load_sync(
    project: &Path,
    tool: &str,
    version: &str,
    interactive: bool,
    now: i64,
) -> Option<Grant> {
    if !interactive {
        return None;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(project.join(".pu/agent-signals/qualifications.json"))
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file()
        || meta.len() > 16 * 1024
        || meta.mode() & 0o7777 != 0o600
        || meta.uid() != unsafe { libc::geteuid() }
    {
        return None;
    }
    let mut data = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut data).ok()?;
    if data.len() > 16 * 1024 {
        return None;
    }
    let config: Config = serde_json::from_slice(&data).ok()?;
    if config.schema_version != 1 || config.qualifications.len() > 32 {
        return None;
    }
    config.qualifications.into_iter().find_map(|r| {
        let proof_valid = r
            .proof_record_url
            .strip_prefix("https://")
            .and_then(|s| s.split_once('/'))
            .is_some_and(|(host, path)| !host.is_empty() && !path.is_empty())
            && r.proof_record_url.len() <= 2048
            && !r
                .proof_record_url
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '?' || c == '@');
        let grant = Grant {
            qualified_at_ms: r.qualified_at_ms,
            expires_at_ms: r.expires_at_ms,
            invalidated: AtomicBool::new(false),
        };
        (r.tool == tool
            && r.tool_version == version
            && r.adapter_version == "1"
            && r.platform == "macos"
            && r.execution_mode == "interactive_pty"
            && r.owner_confirmed
            && proof_valid
            && r.qualified_at_ms > 0
            && grant.current(now))
        .then_some(grant)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    fn config(project: &std::path::Path, value: serde_json::Value) {
        let dir = project.join(".pu/agent-signals");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("qualifications.json");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn entry() -> serde_json::Value {
        json!({"schema_version":1,"qualifications":[{"tool":"pi","tool_version":"1.3.0","adapter_version":"1","platform":"macos","execution_mode":"interactive_pty","owner_confirmed":true,"proof_record_url":"https://pagespace.ai/dashboard/drive/record","qualified_at_ms":100,"expires_at_ms":10000}]})
    }
    #[test]
    fn given_explicit_owner_proof_should_enable_only_exact_current_interactive_mac_tuple() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_sync(tmp.path(), "pi", "1.3.0", true, 200).is_none());
        config(tmp.path(), entry());
        let grant = load_sync(tmp.path(), "pi", "1.3.0", true, 200).unwrap();
        assert!(grant.current(200));
        assert!(!grant.current(10000));
        assert!(!grant.current(99));
        assert!(load_sync(tmp.path(), "pi", "1.3.1", true, 200).is_none());
        assert!(load_sync(tmp.path(), "pi", "1.3.0", false, 200).is_none());
        assert!(load_sync(tmp.path(), "pi", "1.3.0", true, 10000).is_none());
    }
    #[test]
    fn given_observed_expiry_should_never_reauthorize_the_same_grant_after_clock_rollback() {
        let tmp = tempfile::tempdir().unwrap();
        config(tmp.path(), entry());
        let grant = load_sync(tmp.path(), "pi", "1.3.0", true, 200).unwrap();
        assert!(grant.current(5000));
        assert!(!grant.current(10000));
        assert!(!grant.current(5000));
    }
    #[test]
    fn given_owner_only_executable_file_should_reject_nonexact_private_mode() {
        let tmp = tempfile::tempdir().unwrap();
        config(tmp.path(), entry());
        std::fs::set_permissions(
            tmp.path().join(".pu/agent-signals/qualifications.json"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(load_sync(tmp.path(), "pi", "1.3.0", true, 200).is_none());
    }
    #[test]
    fn given_missing_proof_or_untrusted_file_should_keep_actionability_false() {
        let tmp = tempfile::tempdir().unwrap();
        for field in [
            "owner_confirmed",
            "proof_record_url",
            "execution_mode",
            "platform",
            "adapter_version",
        ] {
            let mut value = entry();
            value["qualifications"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            config(tmp.path(), value);
            assert!(load_sync(tmp.path(), "pi", "1.3.0", true, 200).is_none());
        }
        config(tmp.path(), entry());
        let path = tmp.path().join(".pu/agent-signals/qualifications.json");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_sync(tmp.path(), "pi", "1.3.0", true, 200).is_none());
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("/dev/null", &path).unwrap();
        assert!(load_sync(tmp.path(), "pi", "1.3.0", true, 200).is_none());
    }
}
