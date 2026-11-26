use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(super) fn opencode_config(input: Option<&str>, plugin: &str) -> Option<String> {
    let mut config: serde_json::Value = serde_json::from_str(input.unwrap_or("{}")).ok()?;
    let object = config.as_object_mut()?;
    let plugins = object
        .entry("plugins")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()?;
    plugins.push(plugin.into());
    serde_json::to_string(&config).ok()
}

pub(super) struct Artifacts {
    pub directory: PathBuf,
    pub socket_directory: PathBuf,
    /// What `PU_SIGNAL_SOCKET` names: the datagram socket path on Unix, the
    /// signal pipe name on Windows.
    pub signal_address: String,
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl Artifacts {
    pub fn new(project: &Path, launch: &str) -> std::io::Result<Self> {
        let base = project.join(".pu").join("agent-signals");
        pu_core::private_fs::create_private_dir(&base)?;
        let directory = base.join(launch);
        pu_core::private_fs::create_new_private_dir(&directory)?;
        // Short absolute path: Unix socket paths cannot hold a worktree path on
        // macOS. Windows signals use a named pipe; this directory still holds
        // the launch's scratch HOME.
        let socket_directory = std::env::temp_dir().join(format!("pu-signal-{launch}"));
        if let Err(e) = pu_core::private_fs::create_new_private_dir(&socket_directory) {
            let _ = std::fs::remove_dir(&directory);
            return Err(e);
        }
        #[cfg(unix)]
        let signal_address = socket_directory.join("s").to_string_lossy().into_owned();
        #[cfg(windows)]
        let signal_address = match signal_pipe_name(project, launch) {
            Ok(name) => name,
            Err(e) => {
                let _ = std::fs::remove_dir(&socket_directory);
                let _ = std::fs::remove_dir(&directory);
                return Err(e);
            }
        };
        Ok(Self {
            directory,
            socket_directory,
            signal_address,
            files: Vec::new(),
        })
    }
    /// Bind this launch's signal endpoint at [`Self::signal_address`].
    pub fn bind_signal(&self) -> std::io::Result<super::SignalSocket> {
        #[cfg(unix)]
        return tokio::net::UnixDatagram::bind(&self.signal_address);
        #[cfg(windows)]
        return pu_transport::signal::SignalListener::bind(&self.signal_address);
    }
    pub fn write(&mut self, name: &str, data: Vec<u8>) -> std::io::Result<PathBuf> {
        let path = self.directory.join(name);
        let mut file = pu_core::private_fs::create_new_private_file(&path)?;
        file.write_all(&data)?;
        self.files.push((path.clone(), data));
        Ok(path)
    }
}
/// `purepoint-<hash16>-signal-<nonce>`: the daemon socket's hash (as for the
/// daemon pipe) and a nonce no other launch can predict.
#[cfg(windows)]
fn signal_pipe_name(project: &Path, launch: &str) -> std::io::Result<String> {
    let socket = pu_core::paths::daemon_socket_path()
        .unwrap_or_else(|_| project.join(".pu").join("daemon.sock"));
    let nonce = format!("{launch}-{}", pu_core::id::session_id());
    pu_transport::signal::pipe_name(&socket, &nonce)
}

/// A Claude `settings.json` hook entry that runs `helper args…`.
///
/// Windows: exec form (`command` + `args`), which Claude runs without a
/// shell, so the helper path (`C:\Program Files\…\pu.exe`) and every
/// argument arrive exactly. No shell-form string works under both Git Bash
/// and pwsh (WIN-1.1 section 9). Unix: the usual shell form, single-quoted.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn claude_hook_command(helper: &Path, args: &[&str]) -> serde_json::Value {
    #[cfg(windows)]
    {
        serde_json::json!({
            "type": "command",
            "command": helper.to_string_lossy(),
            "args": args,
        })
    }
    #[cfg(unix)]
    {
        fn quote(s: &str) -> String {
            format!("'{}'", s.replace('\'', "'\\''"))
        }
        let mut command = quote(&helper.to_string_lossy());
        for arg in args {
            command.push(' ');
            command.push_str(&quote(arg));
        }
        serde_json::json!({"type": "command", "command": command})
    }
}

impl Drop for Artifacts {
    fn drop(&mut self) {
        // Compare-and-remove only exact owned bytes; later edits survive removal.
        for (path, bytes) in &self.files {
            if std::fs::read(path).is_ok_and(|current| current == *bytes) {
                let _ = std::fs::remove_file(path);
            }
        }
        let _ = std::fs::remove_file(self.socket_directory.join("s"));
        let _ = std::fs::remove_dir(self.socket_directory.join("home"));
        let _ = std::fs::remove_dir(&self.socket_directory);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn given_owned_artifacts_should_remove_only_unchanged_files() {
        let project = tempfile::tempdir().unwrap();
        let launch = pu_core::id::session_id();
        let mut artifacts = Artifacts::new(project.path(), &launch).unwrap();
        let untouched = artifacts.write("untouched", b"owned".to_vec()).unwrap();
        let edited = artifacts.write("edited", b"owned".to_vec()).unwrap();
        std::fs::write(&edited, b"later edit").unwrap();
        let unrelated = artifacts.directory.join("unrelated");
        std::fs::write(&unrelated, b"other").unwrap();
        drop(artifacts);
        assert!(!untouched.exists());
        assert_eq!(std::fs::read(edited).unwrap(), b"later edit");
        assert_eq!(std::fs::read(unrelated).unwrap(), b"other");
    }
    #[cfg(windows)]
    #[test]
    fn given_pu_exe_path_with_spaces_should_write_exec_form_hook() {
        let helper = Path::new(r"C:\Program Files\Pure Point\bin\pu.exe");
        let hook = claude_hook_command(helper, &["__agent-signal", "x y", "q\"uote"]);
        assert_eq!(
            hook,
            serde_json::json!({
                "type": "command",
                "command": r"C:\Program Files\Pure Point\bin\pu.exe",
                "args": ["__agent-signal", "x y", "q\"uote"],
            })
        );
        // Exec form never goes through a shell: no quoting is added.
        assert!(hook.get("shell").is_none());
    }
    #[cfg(unix)]
    #[test]
    fn given_helper_path_with_spaces_should_quote_shell_form_hook() {
        let hook = claude_hook_command(Path::new("/opt/Pure Point/pu"), &["__agent-signal"]);
        assert_eq!(hook["command"], "'/opt/Pure Point/pu' '__agent-signal'");
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn given_launch_should_name_a_private_signal_pipe() {
        let project = tempfile::tempdir().unwrap();
        let launch = pu_core::id::session_id();
        let artifacts = Artifacts::new(project.path(), &launch).unwrap();
        assert!(
            artifacts.signal_address.starts_with(r"\\.\pipe\purepoint-")
                && artifacts
                    .signal_address
                    .contains(&format!("-signal-{launch}-")),
            "{}",
            artifacts.signal_address
        );
        let _socket = artifacts.bind_signal().unwrap();
        assert!(
            artifacts.bind_signal().is_err(),
            "the pipe name is exclusive"
        );
        pu_core::private_fs::check_private(&artifacts.directory).unwrap();
        pu_core::private_fs::check_private(&artifacts.socket_directory).unwrap();
    }
    #[test]
    fn given_inline_opencode_config_should_preserve_user_plugins_and_settings() {
        let input = r#"{"plugins":["existing"],"model":"mine","permissions":{"*":"deny"}}"#;
        let merged = opencode_config(Some(input), "/owned/plugin").unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            value["plugins"],
            serde_json::json!(["existing", "/owned/plugin"])
        );
        assert_eq!(value["permissions"]["*"], "deny");
        assert_eq!(value["model"], "mine");
        assert!(opencode_config(Some("{malformed"), "/owned/plugin").is_none());
    }
}
