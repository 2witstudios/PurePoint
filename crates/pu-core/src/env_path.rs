//! `PATH` composition shared by the daemon (agent and git environments) and
//! the CLI service installer.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Directories PurePoint and agent CLIs install into on Windows, in the order
/// WINAPP-007 prepends them: `~\.pu\bin`, `~\.cargo\bin`, `%APPDATA%\npm`,
/// `%LOCALAPPDATA%\Programs`, `~\.local\bin`. Unset variables are skipped.
pub fn windows_tool_dirs(home: &Path, get: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let var = |key: &str| get(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut dirs = vec![
        home.join(".pu").join("bin"),
        home.join(".cargo").join("bin"),
    ];
    dirs.extend(var("APPDATA").map(|d| d.join("npm")));
    dirs.extend(var("LOCALAPPDATA").map(|d| d.join("Programs")));
    dirs.push(home.join(".local").join("bin"));
    dirs
}

/// Comparison key for one `PATH` entry: Windows directories are
/// case-insensitive and `C:\x\` names `C:\x`.
fn entry_key(dir: &Path) -> String {
    let text = dir.to_string_lossy();
    if cfg!(windows) {
        let trimmed = text.trim_end_matches(['\\', '/']);
        // Keep the root of a drive (`C:\`) distinct from the drive-relative `C:`.
        let trimmed = if trimmed.ends_with(':') {
            &text
        } else {
            trimmed
        };
        trimmed.replace('/', "\\").to_lowercase()
    } else {
        text.into_owned()
    }
}

/// Join `dirs` into one `PATH` value with [`std::env::join_paths`], keeping
/// the first of each duplicate and dropping empty entries and entries the
/// platform cannot represent in `PATH` (a `"` on Windows, a `:` on Unix).
pub fn join_unique(dirs: impl IntoIterator<Item = PathBuf>) -> String {
    let mut seen = std::collections::HashSet::new();
    let kept: Vec<PathBuf> = dirs
        .into_iter()
        .filter(|dir| !dir.as_os_str().is_empty())
        .filter(|dir| std::env::join_paths([dir]).is_ok())
        .filter(|dir| seen.insert(entry_key(dir)))
        .collect();
    std::env::join_paths(kept)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_duplicate_and_empty_entries_should_keep_first_once() {
        let joined = join_unique(["a", "", "b", "a"].map(PathBuf::from));
        let parts: Vec<_> = std::env::split_paths(&joined).collect();
        assert_eq!(parts, [PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[cfg(windows)]
    #[test]
    fn given_windows_path_with_spaces_and_case_variants_should_join_once() {
        let joined = join_unique(
            [
                r"C:\Program Files\nodejs",
                r"c:\program files\NODEJS\",
                r#"C:\bad"quote"#,
                r"C:\",
                r"C:",
            ]
            .map(PathBuf::from),
        );
        assert_eq!(joined, r"C:\Program Files\nodejs;C:\;C:");
    }

    #[cfg(windows)]
    #[test]
    fn given_windows_env_should_list_tool_dirs_in_order() {
        let get = |key: &str| match key {
            "APPDATA" => Some(OsString::from(r"C:\Users\me\AppData\Roaming")),
            "LOCALAPPDATA" => Some(OsString::from("")),
            _ => None,
        };
        let dirs = windows_tool_dirs(Path::new(r"C:\Users\me"), get);
        assert_eq!(
            dirs,
            [
                r"C:\Users\me\.pu\bin",
                r"C:\Users\me\.cargo\bin",
                r"C:\Users\me\AppData\Roaming\npm",
                r"C:\Users\me\.local\bin",
            ]
            .map(PathBuf::from)
        );
    }
}
