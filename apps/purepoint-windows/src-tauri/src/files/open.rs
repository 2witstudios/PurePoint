//! The file tree's Open in Default App (WIN-8.15): a project file handed to `ShellExecute`.
//!
//! The opener plugin's own `open_path` is not granted to the webview: on Windows the "default
//! app" of a script, shortcut or installer is to run it, so a file an agent wrote could execute
//! with one click. This command opens only files inside an open project ([`super::roots`]) whose
//! type Windows does not execute; for the others the front end offers Reveal in Explorer.

use std::path::Path;

use serde::Serialize;

use super::roots::Access;

/// Types `ShellExecute` runs (or hands to an interpreter or installer) instead of opening as a
/// document. Kept in step with `DEFAULT_APP_BLOCKED_EXTENSIONS` in `ui/src/services/opener.ts`;
/// `PATHEXT` adds to it at run time.
pub const BLOCKED_EXTENSIONS: &[&str] = &[
    "appref-ms",
    "application",
    "appx",
    "appxbundle",
    "bat",
    "chm",
    "cmd",
    "com",
    "cpl",
    "diagcab",
    "exe",
    "gadget",
    "hta",
    "inf",
    "ins",
    "isp",
    "jar",
    "js",
    "jse",
    "library-ms",
    "lnk",
    "msc",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "pif",
    "ps1",
    "ps1xml",
    "ps2",
    "psc1",
    "psd1",
    "psm1",
    "reg",
    "scf",
    "scr",
    "search-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    "shb",
    "shs",
    "sys",
    "url",
    "vb",
    "vbe",
    "vbs",
    "website",
    "ws",
    "wsc",
    "wsf",
    "wsh",
    "xbap",
];

/// Why Open in Default App did nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum OpenError {
    /// The file is of a type Windows would run.
    Blocked(String),
    /// Outside the open projects, missing, or the shell failed.
    Io(String),
}

/// The extension Windows sees: trailing dots and spaces are dropped from names, so `a.bat.`
/// is `a.bat`. Lowercase, without the dot.
fn shell_extension(name: &str) -> Option<String> {
    let name = name.trim_end_matches(['.', ' ']);
    let (_, extension) = name.rsplit_once('.')?;
    Some(extension.to_ascii_lowercase())
}

/// `PATHEXT`'s entries, lowercase without dots.
fn path_extensions(pathext: Option<&str>) -> Vec<String> {
    pathext
        .unwrap_or_default()
        .split(';')
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// Whether opening `path` in its default app would run it. A name with a stream (`a.txt:x.exe`)
/// or a shell-namespace extension (`x.{CLSID}`) is refused too.
pub fn is_blocked(path: &Path, pathext: Option<&str>) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    if name.contains(':') {
        return true;
    }
    let Some(extension) = shell_extension(&name) else {
        return false;
    };
    extension.starts_with('{')
        || BLOCKED_EXTENSIONS.contains(&extension.as_str())
        || path_extensions(pathext).contains(&extension)
}

/// The resolved path to open, after the project and type checks.
pub fn checked(path: &Path, pathext: Option<&str>) -> Result<std::path::PathBuf, OpenError> {
    if is_blocked(path, pathext) {
        return Err(OpenError::Blocked(format!(
            "{} is a program or script, so it is not opened directly.",
            path.display()
        )));
    }
    let resolved = super::roots::global()
        .check(path, Access::Read)
        .map_err(|e| match e {
            super::text::FsError::Conflict(m) | super::text::FsError::Io(m) => OpenError::Io(m),
        })?;
    // A link's target is checked too (`a.txt` linking to `run.cmd`).
    let is_dir = std::fs::metadata(&resolved).is_ok_and(|m| m.is_dir());
    if !is_dir && is_blocked(&resolved, pathext) {
        return Err(OpenError::Blocked(format!(
            "{} is a program or script, so it is not opened directly.",
            path.display()
        )));
    }
    Ok(resolved)
}

/// `open_in_default_app(path)`: a project file or folder in the app Windows associates with it.
#[tauri::command]
pub async fn open_in_default_app(path: String) -> Result<(), OpenError> {
    tauri::async_runtime::spawn_blocking(move || {
        let pathext = std::env::var("PATHEXT").ok();
        let resolved = checked(Path::new(&path), pathext.as_deref())?;
        tauri_plugin_opener::open_path(&resolved, None::<&str>)
            .map_err(|e| OpenError::Io(format!("{}: {e}", path)))
    })
    .await
    .map_err(|e| OpenError::Io(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL;.PY";

    #[test]
    fn given_documents_should_allow_them() {
        for name in [
            "README.md",
            "a.txt",
            "image.PNG",
            "notes",
            "archive.tar.gz",
            ".env",
        ] {
            assert!(!is_blocked(Path::new(name), Some(PATHEXT)), "{name}");
        }
    }

    #[test]
    fn given_programs_and_scripts_should_block_them() {
        for name in [
            "run.js",
            "x.vbs",
            "x.wsf",
            "x.hta",
            "x.bat",
            "x.cmd",
            "x.lnk",
            "x.url",
            "x.scr",
            "x.msi",
            "x.exe",
            "x.cpl",
            "x.jar",
            "x.appref-ms",
            "x.settingcontent-ms",
            "x.ps1",
            "x.com",
            "x.pif",
            "SETUP.EXE",
        ] {
            assert!(is_blocked(Path::new(name), None), "{name}");
        }
    }

    #[test]
    fn given_pathext_should_block_its_types() {
        assert!(is_blocked(Path::new("tool.py"), Some(PATHEXT)));
        assert!(!is_blocked(Path::new("tool.py"), None));
        assert!(!is_blocked(Path::new("tool.py"), Some("")));
    }

    #[test]
    fn given_disguised_names_should_block_them() {
        for name in [
            "x.bat.",
            "x.cmd  ",
            "x.exe. .",
            "a.txt:evil.exe",
            "x.{20D04FE0-3AEA}",
        ] {
            assert!(is_blocked(Path::new(name), None), "{name}");
        }
    }

    #[test]
    fn given_a_path_outside_the_projects_should_refuse_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "a").unwrap();
        assert!(matches!(checked(&file, None), Err(OpenError::Io(_))));
        assert!(matches!(
            checked(&dir.path().join("x.bat"), None),
            Err(OpenError::Blocked(_))
        ));
    }

    #[test]
    fn given_a_project_file_should_resolve_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = pu_core::canon::canonicalize(dir.path())
            .unwrap()
            .join("Proj");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        assert!(super::super::roots::global().add(&root));
        assert_eq!(checked(&file, None), Ok(file));
    }

    #[test]
    fn should_list_the_same_types_as_the_front_end() {
        let source = include_str!("../../../ui/src/services/opener.ts");
        for extension in BLOCKED_EXTENSIONS {
            assert!(
                source.contains(&format!("'{extension}'")),
                "opener.ts lacks {extension}"
            );
        }
    }

    #[test]
    fn should_not_grant_the_plugins_open_path_to_the_webview() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../../capabilities/default.json")).unwrap();
        let permissions = capability["permissions"].as_array().unwrap();
        for permission in permissions {
            let id = permission
                .as_str()
                .or_else(|| permission["identifier"].as_str())
                .unwrap();
            assert!(
                !id.starts_with("opener:allow-open-path") && id != "opener:allow-default-paths",
                "{id}"
            );
        }
    }

    #[test]
    fn should_serialize_kind_and_message() {
        assert_eq!(
            serde_json::to_value(OpenError::Blocked("m".into())).unwrap(),
            serde_json::json!({ "kind": "blocked", "message": "m" })
        );
    }
}
