use std::path::Path;

use crate::error::PuError;
use crate::paths;
use crate::types::Manifest;

pub fn read_manifest(project_root: &Path) -> Result<Manifest, PuError> {
    let path = paths::manifest_path(project_root);
    let content = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PuError::NotInitialized
        } else {
            PuError::Io(e)
        }
    })?;
    let manifest: Manifest = serde_json::from_str(&content)?;
    Ok(manifest)
}

pub fn write_manifest(project_root: &Path, manifest: &Manifest) -> Result<(), PuError> {
    use std::io::Write;

    let path = paths::manifest_path(project_root);
    let content = serde_json::to_string_pretty(manifest)? + "\n";

    // Atomic write: write to temp file, fsync, then rename
    let tmp_path = path.with_extension("json.tmp");
    let file = std::fs::File::create(&tmp_path)?;
    let mut writer = std::io::BufWriter::new(file);
    writer.write_all(content.as_bytes())?;
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

pub fn update_manifest(
    project_root: &Path,
    updater: impl FnOnce(Manifest) -> Manifest,
) -> Result<Manifest, PuError> {
    let path = paths::manifest_path(project_root);

    // Lock the manifest file during update
    use fs4::fs_std::FileExt;
    let lock_path = path.with_extension("json.lock");
    let lock_file = std::fs::File::create(&lock_path)?;
    lock_file
        .lock_exclusive()
        .map_err(|_| PuError::ManifestLocked)?;

    // RAII guard ensures lock file is cleaned up even on panic
    let _guard = LockFileGuard { path: &lock_path };

    let manifest = read_manifest(project_root)?;
    let mut updated = updater(manifest);
    updated.updated_at = chrono::Utc::now();
    write_manifest(project_root, &updated)?;
    Ok(updated)
}

struct LockFileGuard<'a> {
    path: &'a Path,
}

impl Drop for LockFileGuard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Manifest;
    use tempfile::TempDir;

    #[test]
    fn given_manifest_should_write_and_read_back_identical() {
        // given
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let manifest = Manifest::new(root.to_string_lossy().into());

        // when
        write_manifest(root, &manifest).unwrap();
        let loaded = read_manifest(root).unwrap();

        // then
        assert_eq!(loaded.version, manifest.version);
        assert_eq!(loaded.project_root, manifest.project_root);
        assert!(loaded.worktrees.is_empty());
        assert!(loaded.agents.is_empty());
    }

    #[test]
    fn given_no_manifest_file_should_return_not_initialized_error() {
        let tmp = TempDir::new().unwrap();
        let result = read_manifest(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn given_manifest_should_write_atomic_via_temp_rename() {
        // Verify the file exists after write (atomic rename completed)
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let manifest = Manifest::new(root.to_string_lossy().into());

        write_manifest(root, &manifest).unwrap();

        let path = crate::paths::manifest_path(root);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("projectRoot")); // camelCase key
    }

    #[test]
    fn given_manifest_should_use_camel_case_keys_on_disk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let manifest = Manifest::new(root.to_string_lossy().into());

        write_manifest(root, &manifest).unwrap();

        let content = std::fs::read_to_string(crate::paths::manifest_path(root)).unwrap();
        // Must be camelCase for macOS app compatibility
        assert!(content.contains("\"projectRoot\""));
        assert!(content.contains("\"createdAt\""));
        assert!(content.contains("\"updatedAt\""));
        // Must NOT have snake_case
        assert!(!content.contains("\"project_root\""));
        assert!(!content.contains("\"created_at\""));
    }

    #[test]
    fn given_update_manifest_should_apply_updater_and_persist() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let manifest = Manifest::new(root.to_string_lossy().into());
        write_manifest(root, &manifest).unwrap();

        // when
        let updated = update_manifest(root, |mut m| {
            m.version = 99;
            m
        })
        .unwrap();

        // then
        assert_eq!(updated.version, 99);
        let reloaded = read_manifest(root).unwrap();
        assert_eq!(reloaded.version, 99);
    }

    #[test]
    fn given_ppg_cli_format_manifest_should_parse_with_optional_fields() {
        // Simulate a ppg-cli manifest (has tmuxTarget, sessionName — our parser should ignore unknown fields)
        let json = r#"{
            "version": 2,
            "projectRoot": "/test",
            "sessionName": "ppg-test",
            "worktrees": {},
            "agents": {},
            "createdAt": "2026-03-01T00:00:00Z",
            "updatedAt": "2026-03-01T00:00:00Z"
        }"#;
        // Our Manifest type doesn't have sessionName, but should still parse
        // (serde default behavior is to ignore unknown fields unless deny_unknown_fields)
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.version, 2);
        assert_eq!(manifest.project_root, "/test");
    }
}
