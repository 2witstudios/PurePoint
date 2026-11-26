//! DEC-2 pipe naming: `\\.\pipe\purepoint-<16 hex>-<role>`.
//!
//! Normalization of the `~/.pu` path (the hash input), in order:
//! 1. `create_dir_all(home\.pu)` — canonicalization needs an existing path,
//!    and both daemon and client may be first to run.
//! 2. `dunce::canonicalize` — `GetFinalPathNameByHandleW`: resolves
//!    junctions/symlinks, `.`/`..`, 8.3 short names and returns the casing
//!    stored on disk; dunce strips `\\?\` when the path is representable
//!    without it (UNC stays `\\server\share\...`).
//! 3. Forward slashes -> backslashes; ASCII-uppercase a drive letter;
//!    strip a trailing backslash.
//! 4. No lowercasing: step 2 already yields on-disk casing, and lowercasing
//!    would merge two distinct directories inside a case-sensitive
//!    (`fsutil file setCaseSensitiveInfo`) folder.
//!
//! Hash = lowercase hex of the first 8 bytes of SHA-256 over the UTF-8 of
//! the normalized string.
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

pub fn normalize_pu_dir(home: &Path) -> io::Result<String> {
    let pu = home.join(".pu");
    std::fs::create_dir_all(&pu)?;
    normalize_existing(&pu)
}

pub fn normalize_existing(path: &Path) -> io::Result<String> {
    let canonical = dunce::canonicalize(path)?;
    let text = canonical
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not unicode"))?;
    let mut s = text.replace('/', "\\");
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        s[..1].make_ascii_uppercase();
    }
    while s.ends_with('\\') && !(s.len() == 3 && s.as_bytes()[1] == b':') {
        s.pop();
    }
    Ok(s)
}

pub fn hash16(normalized: &str) -> String {
    let digest = Sha256::digest(normalized.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

pub fn pipe_name(normalized: &str, role: &str) -> String {
    format!(r"\\.\pipe\purepoint-{}-{role}", hash16(normalized))
}

pub fn daemon_pipe_name(home: &Path) -> io::Result<String> {
    Ok(pipe_name(&normalize_pu_dir(home)?, "daemon"))
}
