use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
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
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl Artifacts {
    pub fn new(project: &Path, launch: &str) -> std::io::Result<Self> {
        let base = project.join(".pu/agent-signals");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&base)?;
        let directory = base.join(launch);
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        // Short absolute path: Unix socket paths cannot hold a worktree path on macOS.
        let socket_directory = std::env::temp_dir().join(format!("pu-signal-{launch}"));
        if let Err(e) = std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&socket_directory)
        {
            let _ = std::fs::remove_dir(&directory);
            return Err(e);
        }
        Ok(Self {
            directory,
            socket_directory,
            files: Vec::new(),
        })
    }
    pub fn write(&mut self, name: &str, data: Vec<u8>) -> std::io::Result<PathBuf> {
        let path = self.directory.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&data)?;
        self.files.push((path.clone(), data));
        Ok(path)
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
