//! Where a Windows client looks for `pu-engine.exe`, and which candidate it
//! may start. Shared by `pu` (`daemon_ctrl`) and the Windows app's daemon
//! lifecycle, so both apply the same search order and trust check.

use std::path::{Path, PathBuf};

/// The engine executable's file name on Windows.
pub const ENGINE_EXE: &str = "pu-engine.exe";

/// Names the one `pu-engine` to start: a path, or a name looked up on `PATH`.
pub const ENGINE_PATH_ENV: &str = "PU_ENGINE_PATH";

/// `pu-engine.exe` in each of `dirs` (in order, duplicates dropped), then
/// `%USERPROFILE%\.pu\bin\pu-engine.exe`.
pub fn candidates<'a>(
    dirs: impl IntoIterator<Item = &'a Path>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let installed = home.map(|home| home.join(".pu").join("bin").join(ENGINE_EXE));
    for path in dirs
        .into_iter()
        .map(|dir| dir.join(ENGINE_EXE))
        .chain(installed)
    {
        if !out.iter().any(|seen| seen == &path) {
            out.push(path);
        }
    }
    out
}

/// The first candidate that passes
/// [`check_trusted_binary`](crate::win_security::check_trusted_binary). The
/// error names why each candidate failed.
pub fn first_trusted(candidates: impl IntoIterator<Item = PathBuf>) -> Result<PathBuf, String> {
    let mut reasons = Vec::new();
    for path in candidates {
        match crate::win_security::check_trusted_binary(&path) {
            Ok(()) => return Ok(path),
            Err(reason) => reasons.push(format!("{}: {reason}", path.display())),
        }
    }
    if reasons.is_empty() {
        reasons.push("no candidate locations".into());
    }
    Err(reasons.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_dirs_and_home_should_list_dirs_first_without_duplicates() {
        let home = Path::new(r"C:\Users\u");
        let app = Path::new(r"C:\App");
        let found = candidates([app, app], Some(home));
        assert_eq!(
            found,
            vec![
                app.join(ENGINE_EXE),
                home.join(".pu").join("bin").join(ENGINE_EXE)
            ]
        );
        assert!(candidates([], None).is_empty());
    }

    #[test]
    fn given_no_candidates_should_say_so() {
        assert_eq!(first_trusted([]), Err("no candidate locations".into()));
    }
}
