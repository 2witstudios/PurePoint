//! DEC-2 pipe naming: `\\.\pipe\purepoint-<16 hex>-<role>`.
//!
//! The hash input is the normalized socket path (`~/.pu/daemon.sock`, or a
//! custom/test socket path), so each daemon home, each test home and each
//! per-machine socket gets its own pipe. Normalization (WIN-1.2,
//! `docs/research/windows-ipc.md`):
//!
//! 1. `dunce::canonicalize` the parent: resolves junctions, `.`/`..`, 8.3
//!    names and returns on-disk casing, without a `\\?\` prefix. Naming never
//!    creates anything (a client must not make directories in a home it only
//!    connects to): when the parent does not exist yet, its nearest existing
//!    ancestor is canonicalized and the missing components are appended as
//!    written, which is what canonicalizing gives once `Listener::bind` has
//!    created them.
//! 2. Append the file name; `/` becomes `\`; uppercase a drive letter; strip
//!    a trailing `\`. No lowercasing: canonical casing is already stable, and
//!    lowercasing would merge distinct folders in case-sensitive directories.
//!
//! Hash: lowercase hex of the first 8 bytes of SHA-256 over the UTF-8 string.
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Normalized form of `socket_path` used as the hash input.
pub fn normalize_socket_path(socket_path: &Path) -> io::Result<String> {
    let parent = socket_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file = socket_path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "socket path has no file name")
    })?;
    let canonical = canonicalize_lenient(parent)?.join(file);
    let text = canonical
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not unicode"))?;
    Ok(normalize_text(text))
}

/// `dunce::canonicalize`, or for a path that does not exist yet, its nearest
/// existing ancestor canonicalized with the rest appended textually.
fn canonicalize_lenient(path: &Path) -> io::Result<std::path::PathBuf> {
    match dunce::canonicalize(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        result => return result,
    }
    // `absolute` resolves `.`/`..` and separators textually.
    let abs = std::path::absolute(path)?;
    let mut missing = Vec::new();
    let mut base = abs.as_path();
    loop {
        match dunce::canonicalize(base) {
            Ok(mut canonical) => {
                canonical.extend(missing.iter().rev());
                return Ok(canonical);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                base = parent;
            }
            _ => return Ok(abs),
        }
    }
}

fn normalize_text(text: &str) -> String {
    let mut s = text.replace('/', "\\");
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        s[..1].make_ascii_uppercase();
    }
    while s.ends_with('\\') && !(s.len() == 3 && s.as_bytes()[1] == b':') {
        s.pop();
    }
    s
}

/// First 8 bytes of SHA-256 as 16 lowercase hex characters.
pub fn hash16(normalized: &str) -> String {
    let digest = Sha256::digest(normalized.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `\\.\pipe\purepoint-<hash16>-<role>`.
pub fn pipe_name(normalized: &str, role: &str) -> String {
    format!(r"\\.\pipe\purepoint-{}-{role}", hash16(normalized))
}

/// The daemon pipe for a socket path.
pub fn pipe_name_for_socket(socket_path: &Path) -> io::Result<String> {
    Ok(pipe_name(&normalize_socket_path(socket_path)?, "daemon"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_two_homes_should_name_different_pipes() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let na = pipe_name_for_socket(&a.path().join(".pu").join("daemon.sock")).unwrap();
        let nb = pipe_name_for_socket(&b.path().join(".pu").join("daemon.sock")).unwrap();
        assert_ne!(na, nb);
        assert!(
            na.starts_with(r"\\.\pipe\purepoint-") && na.ends_with("-daemon"),
            "{na}"
        );
    }

    #[test]
    fn given_spellings_of_one_path_should_name_one_pipe() {
        let home = tempfile::tempdir().unwrap();
        let pu = home.path().join(".pu");
        std::fs::create_dir_all(&pu).unwrap();
        let plain = pipe_name_for_socket(&pu.join("daemon.sock")).unwrap();
        let text = pu.to_str().unwrap();
        let variants = [
            format!("{}/daemon.sock", text.replace('\\', "/")),
            format!("{}\\..\\.pu\\daemon.sock", text),
            format!("{}\\daemon.sock", text.to_uppercase()),
        ];
        for v in variants {
            assert_eq!(pipe_name_for_socket(Path::new(&v)).unwrap(), plain, "{v}");
        }
    }

    #[test]
    fn given_missing_parent_should_name_without_creating_and_match_after_creation() {
        let home = tempfile::tempdir().unwrap();
        let pu = home.path().join("new home").join(".pu");
        let sock = pu.join("daemon.sock");
        let before = pipe_name_for_socket(&sock).unwrap();
        assert!(
            !home.path().join("new home").exists(),
            "naming created a dir"
        );
        let dotted = home
            .path()
            .join("x")
            .join("..")
            .join("new home/.pu/daemon.sock");
        assert_eq!(pipe_name_for_socket(&dotted).unwrap(), before);
        std::fs::create_dir_all(&pu).unwrap();
        assert_eq!(pipe_name_for_socket(&sock).unwrap(), before);
    }

    #[test]
    fn given_different_socket_names_should_name_different_pipes() {
        let home = tempfile::tempdir().unwrap();
        let a = pipe_name_for_socket(&home.path().join("daemon.sock")).unwrap();
        let b = pipe_name_for_socket(&home.path().join("other.sock")).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn given_text_should_normalize_drive_and_separators() {
        assert_eq!(normalize_text("c:/Users/x/"), r"C:\Users\x");
        assert_eq!(normalize_text("C:\\"), "C:\\");
    }
}
