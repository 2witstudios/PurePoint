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
    let mut manifest: Manifest = serde_json::from_str(&content)?;
    clear_persisted_signals(&mut manifest);
    Ok(manifest)
}

pub fn write_manifest(project_root: &Path, manifest: &Manifest) -> Result<(), PuError> {
    use std::io::Write;

    let path = paths::manifest_path(project_root);
    // AgentEntry is also live wire data; strip only the persistence copy.
    let mut persisted = manifest.clone();
    clear_persisted_signals(&mut persisted);
    let content = serde_json::to_string_pretty(&persisted)? + "\n";

    // Atomic write: write to a temp file unique to this writer, fsync, then
    // rename. A shared temp path would let two writers interleave into it.
    static WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let file = std::fs::File::create(&tmp_path)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(content.as_bytes())?;
        let file = writer
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, &path)
    };
    // The temp name is unique per write, so a failed write must clean up after
    // itself or every failure leaves another file in .pu/.
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp_path);
    })?;
    Ok(())
}

fn clear_persisted_signals(manifest: &mut Manifest) {
    for agent in manifest.agents.values_mut() {
        agent.signal = None;
    }
    for worktree in manifest.worktrees.values_mut() {
        for agent in worktree.agents.values_mut() {
            agent.signal = None;
        }
    }
}

pub fn update_manifest(
    project_root: &Path,
    updater: impl FnOnce(Manifest) -> Manifest,
) -> Result<Manifest, PuError> {
    let path = paths::manifest_path(project_root);

    // Lock the manifest during update. The lock file is never deleted: unlinking
    // it while another writer waits on it would let a third writer lock a fresh
    // file, and two writers would then both "hold" the lock.
    use fs4::FileExt;
    let lock_path = path.with_extension("json.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    // Fully qualified: std's inherent `File::lock` would otherwise shadow the trait.
    FileExt::lock(&lock_file).map_err(|_| PuError::ManifestLocked)?;

    let manifest = read_manifest(project_root)?;
    let mut updated = updater(manifest);
    updated.updated_at = chrono::Utc::now();
    write_manifest(project_root, &updated)?;
    Ok(updated)
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

    #[test]
    fn given_concurrent_updates_should_lose_none() {
        let tmp = TempDir::new().unwrap();
        let path = paths::manifest_path(tmp.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_manifest(
            tmp.path(),
            &Manifest::new(tmp.path().to_string_lossy().into_owned()),
        )
        .unwrap();

        let threads: Vec<_> = (0..8)
            .map(|t| {
                let root = tmp.path().to_path_buf();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        let id = format!("ag-{t}-{i}");
                        update_manifest(&root, |mut m| {
                            let entry: crate::types::AgentEntry =
                                serde_json::from_value(serde_json::json!({
                                    "id": id,
                                    "name": id,
                                    "agentType": "claude",
                                    "status": "running",
                                    "startedAt": "2026-01-01T00:00:00Z",
                                }))
                                .unwrap();
                            m.agents.insert(id.clone(), entry);
                            m
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(read_manifest(tmp.path()).unwrap().agents.len(), 200);
    }
    fn manifest_with_live_signals() -> Manifest {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/signals-status-v1.json")).unwrap();
        let worktree: crate::types::WorktreeEntry =
            serde_json::from_value(value["worktrees"][0].clone()).unwrap();
        let mut root = worktree.agents["ag-worker"].clone();
        root.id = "ag-root".into();
        root.signal = Some(serde_json::from_value(value["agents"][0]["signal"].clone()).unwrap());
        let mut manifest = Manifest::new("/fixture/project".into());
        manifest.agents.insert(root.id.clone(), root);
        manifest.worktrees.insert(worktree.id.clone(), worktree);
        manifest
    }
    #[test]
    fn given_persisted_root_and_worktree_signals_should_clear_them_on_load_before_original_expiry()
    {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(paths::pu_dir(tmp.path())).unwrap();
        let manifest = manifest_with_live_signals();
        let now = 1792195200001;
        assert!(
            manifest
                .all_agents()
                .iter()
                .all(|a| a.signal.as_ref().unwrap().is_current(now))
        );
        // Simulate older writers which persisted a still-current snapshot.
        std::fs::write(
            paths::manifest_path(tmp.path()),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let loaded = read_manifest(tmp.path()).unwrap();
        assert_eq!(loaded.all_agents().len(), 2);
        assert!(loaded.all_agents().iter().all(|a| a.signal.is_none()));
        // The same serde types are also live status wire and must retain signals.
        let wire: Manifest =
            serde_json::from_str(&serde_json::to_string(&manifest).unwrap()).unwrap();
        assert!(
            wire.all_agents()
                .iter()
                .all(|a| a.signal.as_ref().unwrap().is_current(now))
        );
    }
    #[test]
    fn given_live_manifest_signals_should_omit_them_on_disk_without_mutating_the_caller() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(paths::pu_dir(tmp.path())).unwrap();
        let manifest = manifest_with_live_signals();
        write_manifest(tmp.path(), &manifest).unwrap();
        let disk: Manifest = serde_json::from_str(
            &std::fs::read_to_string(paths::manifest_path(tmp.path())).unwrap(),
        )
        .unwrap();
        assert!(disk.all_agents().iter().all(|a| a.signal.is_none()));
        assert!(manifest.all_agents().iter().all(|a| a.signal.is_some()));
    }
    #[test]
    fn given_legacy_manifest_without_signal_should_preserve_all_persistent_fields() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(paths::pu_dir(tmp.path())).unwrap();
        let mut legacy = serde_json::to_value(manifest_with_live_signals()).unwrap();
        legacy["agents"]["ag-root"]
            .as_object_mut()
            .unwrap()
            .remove("signal");
        legacy["worktrees"]["wt-1"]["agents"]["ag-worker"]
            .as_object_mut()
            .unwrap()
            .remove("signal");
        std::fs::write(
            paths::manifest_path(tmp.path()),
            serde_json::to_string(&legacy).unwrap(),
        )
        .unwrap();
        let loaded = read_manifest(tmp.path()).unwrap();
        assert_eq!(serde_json::to_value(&loaded).unwrap(), legacy);
        write_manifest(tmp.path(), &loaded).unwrap();
        let stored: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(paths::manifest_path(tmp.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(stored, legacy);
    }
}
