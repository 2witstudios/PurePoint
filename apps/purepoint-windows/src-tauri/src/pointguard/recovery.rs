//! Private chat recovery files (WIN-9.4): the Swift `PiLocalRecoveryStore`.
//!
//! The chat keeps its attachment drafts and its submission receipts (each with the text and
//! files of a message whose delivery is in doubt) in `<app data>\PurePoint.PointGuard\
//! <name>.json`, an owner-only directory and files written atomically, as the Mac keeps them
//! in Application Support with 0700/0600. Files over 40 MiB are ignored on load.
//!
//! Tauri surface: `pointguard_recovery_load(name)` returns the JSON text or null;
//! `pointguard_recovery_save(name, json)` replaces it.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// The Mac's 40 MiB bound on a recovery file.
pub const LIMIT: u64 = 40 * 1024 * 1024;
/// The recovery files the chat keeps; any other name is refused.
pub const NAMES: [&str; 2] = ["attachment-drafts", "submissions"];
/// The directory under the app data dir.
pub const DIR: &str = "PurePoint.PointGuard";

fn file(base: &Path, name: &str) -> Result<PathBuf, String> {
    if !NAMES.contains(&name) {
        return Err("Unknown Point Guard recovery file.".into());
    }
    Ok(base.join(format!("{name}.json")))
}

/// The saved JSON, or None when it is missing, too large, not private or not UTF-8.
pub fn load(base: &Path, name: &str) -> Result<Option<String>, String> {
    let path = file(base, name)?;
    match super::runtime::read_private(&path, LIMIT) {
        Ok(data) => Ok(String::from_utf8(data).ok()),
        Err(_) => Ok(None),
    }
}

/// Replace the file with `json`, creating the private directory first.
pub fn save(base: &Path, name: &str, json: &str) -> Result<(), String> {
    let path = file(base, name)?;
    if json.len() as u64 > LIMIT {
        return Err("Point Guard recovery is too large to save.".into());
    }
    pu_core::private_fs::create_private_dir(base).map_err(|e| e.to_string())?;
    pu_core::private_fs::write_private_file(&path, json.as_bytes()).map_err(|e| e.to_string())
}

fn base(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join(DIR))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pointguard_recovery_load(
    app: AppHandle,
    name: String,
) -> Result<Option<String>, String> {
    let base = base(&app)?;
    tauri::async_runtime::spawn_blocking(move || load(&base, &name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn pointguard_recovery_save(
    app: AppHandle,
    name: String,
    json: String,
) -> Result<(), String> {
    let base = base(&app)?;
    tauri::async_runtime::spawn_blocking(move || save(&base, &name, &json))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_saved_file_should_load_it_back_privately() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join(DIR);
        assert_eq!(load(&base, "submissions").unwrap(), None);
        save(&base, "submissions", r#"[{"id":"a"}]"#).unwrap();
        save(&base, "submissions", r#"[{"id":"b"}]"#).unwrap();
        assert_eq!(
            load(&base, "submissions").unwrap().as_deref(),
            Some(r#"[{"id":"b"}]"#)
        );
        pu_core::private_fs::check_private(&base.join("submissions.json")).unwrap();
    }

    #[test]
    fn given_another_name_should_refuse_it() {
        let temp = tempfile::tempdir().unwrap();
        assert!(save(temp.path(), "../escape", "{}").is_err());
        assert!(load(temp.path(), "drafts").is_err());
    }
}
