//! Bundle seal for the Point Guard runtime (port of `Services/BundleSeal.swift`).
//!
//! The Mac checks the app's code signature, whose resource seal covers the bundled bridge.
//! Windows code signing is not provisioned, so the seal is a hash manifest (WIN-9.3):
//! `PointGuard\runtime-manifest.json` lists the SHA-256 of every file in the runtime
//! directory (`seal.files`, forward-slash relative paths). Before each launch the app checks
//! that every listed file exists and matches, and that the directory holds nothing else: no
//! unlisted file, no link or reparse point, no other kind of entry. Any difference refuses
//! the launch with [`MODIFIED_MESSAGE`].
//!
//! A release build may also pin the manifest itself: when `PUREPOINT_POINTGUARD_MANIFEST_SHA256`
//! is set at compile time (WIN-10.2 sets it from the staged manifest), the manifest's own
//! SHA-256 must equal it, so a rewritten manifest fails too. The pin lives in the app exe,
//! which Authenticode will cover once signing is provisioned.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The refusal shown when anything in the runtime differs from its manifest.
pub const MODIFIED_MESSAGE: &str = "PurePoint's files were modified after it was installed, so Point Guard won't start. Reinstall PurePoint.";

/// The manifest's file name inside the runtime directory.
pub const MANIFEST: &str = "runtime-manifest.json";

/// The compile-time pin of the manifest's own SHA-256, when the build set one.
pub const PINNED_MANIFEST_SHA256: Option<&str> =
    option_env!("PUREPOINT_POINTGUARD_MANIFEST_SHA256");

/// Why a seal check failed; the user sees [`MODIFIED_MESSAGE`], the log sees the detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    /// The manifest is missing, unreadable or has no usable `seal`.
    Manifest(String),
    /// The manifest's own digest differs from the compile-time pin.
    ManifestPin,
    /// A listed file is missing or not a regular file.
    Missing(String),
    /// A listed file's contents differ.
    Mismatch(String),
    /// A file is present that the manifest does not list.
    Unlisted(String),
    /// A link, reparse point or other non-file entry.
    NotRegular(String),
    /// A listed path is absolute, escapes the directory or is otherwise malformed.
    BadPath(String),
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(why) => write!(f, "runtime manifest unusable: {why}"),
            Self::ManifestPin => write!(f, "runtime manifest differs from the pinned digest"),
            Self::Missing(p) => write!(f, "missing: {p}"),
            Self::Mismatch(p) => write!(f, "modified: {p}"),
            Self::Unlisted(p) => write!(f, "not in the manifest: {p}"),
            Self::NotRegular(p) => write!(f, "not a regular file: {p}"),
            Self::BadPath(p) => write!(f, "invalid manifest path: {p}"),
        }
    }
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lowercase hex SHA-256 of a file, streamed.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// A manifest path is relative, forward-slash separated, and stays inside the directory.
pub fn valid_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// True for a link, junction or any other reparse point.
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// Every regular file under `root` as forward-slash relative paths (the manifest excluded).
/// Fails on links, reparse points and other non-file entries.
pub fn list_files(root: &Path) -> Result<Vec<String>, SealError> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| SealError::Manifest(format!("{prefix}: {e}")))?;
        for entry in entries {
            let entry = entry.map_err(|e| SealError::Manifest(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let meta = std::fs::symlink_metadata(entry.path())
                .map_err(|_| SealError::NotRegular(relative.clone()))?;
            if is_reparse(&meta) {
                return Err(SealError::NotRegular(relative));
            }
            if meta.is_dir() {
                stack.push((entry.path(), relative));
            } else if meta.is_file() {
                if relative != MANIFEST {
                    out.push(relative);
                }
            } else {
                return Err(SealError::NotRegular(relative));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The `seal.files` map a manifest for `root` lists (what the staging script writes).
pub fn seal_files(root: &Path) -> Result<BTreeMap<String, String>, SealError> {
    let mut files = BTreeMap::new();
    for relative in list_files(root)? {
        let digest =
            file_sha256(&root.join(&relative)).map_err(|_| SealError::Missing(relative.clone()))?;
        files.insert(relative, digest);
    }
    Ok(files)
}

/// Check `root` against the manifest bytes it was installed with.
pub fn verify(root: &Path, manifest_bytes: &[u8], pin: Option<&str>) -> Result<(), SealError> {
    if let Some(pin) = pin
        && !sha256_hex(manifest_bytes).eq_ignore_ascii_case(pin.trim())
    {
        return Err(SealError::ManifestPin);
    }
    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes)
        .map_err(|e| SealError::Manifest(format!("not JSON: {e}")))?;
    let seal = &manifest["seal"];
    if seal["algorithm"] != "sha256" {
        return Err(SealError::Manifest("seal.algorithm is not sha256".into()));
    }
    let listed = seal["files"]
        .as_object()
        .filter(|files| !files.is_empty())
        .ok_or_else(|| SealError::Manifest("seal.files is missing".into()))?;
    for (relative, digest) in listed {
        if !valid_relative(relative) {
            return Err(SealError::BadPath(relative.clone()));
        }
        let digest = digest
            .as_str()
            .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| SealError::Manifest(format!("bad digest for {relative}")))?;
        let path = root.join(relative);
        let meta =
            std::fs::symlink_metadata(&path).map_err(|_| SealError::Missing(relative.clone()))?;
        if is_reparse(&meta) || !meta.is_file() {
            return Err(SealError::NotRegular(relative.clone()));
        }
        let actual = file_sha256(&path).map_err(|_| SealError::Missing(relative.clone()))?;
        if !actual.eq_ignore_ascii_case(digest) {
            return Err(SealError::Mismatch(relative.clone()));
        }
    }
    for present in list_files(root)? {
        if !listed.contains_key(&present) {
            return Err(SealError::Unlisted(present));
        }
    }
    Ok(())
}

/// Read the manifest of `root` and verify the directory against it and the build's pin.
pub fn verify_dir(root: &Path) -> Result<Vec<u8>, SealError> {
    let bytes = std::fs::read(root.join(MANIFEST))
        .map_err(|e| SealError::Manifest(format!("{MANIFEST}: {e}")))?;
    verify(root, &bytes, PINNED_MANIFEST_SHA256)?;
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small runtime tree plus a manifest sealing it.
    pub(crate) fn sealed_fixture(root: &Path) -> Vec<u8> {
        for (path, body) in [
            ("bin/node.exe", "fake node"),
            ("bridge/main.js", "console.log('bridge')"),
            ("node_modules/ws/index.js", "module.exports = {}"),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, body).unwrap();
        }
        write_manifest(root)
    }

    pub(crate) fn write_manifest(root: &Path) -> Vec<u8> {
        let files = seal_files(root).unwrap();
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "seal": { "algorithm": "sha256", "files": files },
        });
        let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        std::fs::write(root.join(MANIFEST), &bytes).unwrap();
        bytes
    }

    #[test]
    fn given_untouched_runtime_should_verify() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = sealed_fixture(tmp.path());
        assert_eq!(verify(tmp.path(), &bytes, None), Ok(()));
        assert_eq!(
            verify(tmp.path(), &bytes, Some(&sha256_hex(&bytes))),
            Ok(())
        );
    }

    #[test]
    fn given_modified_file_should_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = sealed_fixture(tmp.path());
        std::fs::write(tmp.path().join("bridge/main.js"), "evil()").unwrap();
        assert_eq!(
            verify(tmp.path(), &bytes, None),
            Err(SealError::Mismatch("bridge/main.js".into()))
        );
    }

    #[test]
    fn given_missing_or_extra_file_should_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = sealed_fixture(tmp.path());
        std::fs::write(tmp.path().join("node_modules/ws/extra.js"), "x").unwrap();
        assert_eq!(
            verify(tmp.path(), &bytes, None),
            Err(SealError::Unlisted("node_modules/ws/extra.js".into()))
        );
        std::fs::remove_file(tmp.path().join("node_modules/ws/extra.js")).unwrap();
        std::fs::remove_file(tmp.path().join("node_modules/ws/index.js")).unwrap();
        assert_eq!(
            verify(tmp.path(), &bytes, None),
            Err(SealError::Missing("node_modules/ws/index.js".into()))
        );
    }

    #[test]
    fn given_rewritten_manifest_should_fail_the_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let original = sealed_fixture(tmp.path());
        std::fs::write(tmp.path().join("bridge/main.js"), "evil()").unwrap();
        let rewritten = write_manifest(tmp.path());
        assert_eq!(verify(tmp.path(), &rewritten, None), Ok(()));
        assert_eq!(
            verify(tmp.path(), &rewritten, Some(&sha256_hex(&original))),
            Err(SealError::ManifestPin)
        );
    }

    #[test]
    fn given_escaping_or_malformed_manifest_should_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        sealed_fixture(tmp.path());
        let digest = "0".repeat(64);
        for path in [
            "../x",
            "/abs",
            r"bin\node.exe",
            "C:/x",
            "a//b",
            "./a",
            "a/../b",
        ] {
            let manifest =
                serde_json::json!({"seal": {"algorithm": "sha256", "files": {path: digest}}});
            assert_eq!(
                verify(tmp.path(), manifest.to_string().as_bytes(), None),
                Err(SealError::BadPath(path.into())),
                "{path}"
            );
        }
        for manifest in [
            serde_json::json!({}),
            serde_json::json!({"seal": {"algorithm": "md5", "files": {"a": digest}}}),
            serde_json::json!({"seal": {"algorithm": "sha256", "files": {}}}),
            serde_json::json!({"seal": {"algorithm": "sha256", "files": {"bin/node.exe": "zz"}}}),
        ] {
            assert!(matches!(
                verify(tmp.path(), manifest.to_string().as_bytes(), None),
                Err(SealError::Manifest(_))
            ));
        }
        assert!(matches!(
            verify(tmp.path(), b"not json", None),
            Err(SealError::Manifest(_))
        ));
    }

    #[test]
    fn given_hashes_should_be_lowercase_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
