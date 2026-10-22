//! `attach_file`: a client sends a file (a pasted screenshot, a dropped file)
//! to the daemon's machine in chunks, so an agent running there can read it by
//! path. Uploads land in `~/.pu/attachments/<upload_id>/<name>`, outside every
//! repository, and are swept once they're a week old.

use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use pu_core::protocol::{Request, Response};

use super::Engine;

/// Largest file one upload may carry.
const ATTACHMENT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Uploads untouched for this long are removed when a new one starts.
const ATTACHMENT_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const UPLOAD_ID_MAX_LEN: usize = 64;
const NAME_MAX_BYTES: usize = 200;
/// Bytes received so far; renamed to the final name on the last chunk.
const PARTIAL_NAME: &str = ".partial";

fn attach_error(code: &str, message: impl Into<String>) -> Response {
    Response::Error {
        code: code.into(),
        message: message.into(),
    }
}

fn is_valid_upload_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= UPLOAD_ID_MAX_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A plain file name from whatever the client called the file: no directory
/// parts, control characters or leading dots, so it can't escape the upload
/// directory or collide with `.partial`.
fn sanitize_name(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .to_string();
    let mut end = cleaned.len().min(NAME_MAX_BYTES);
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    match &cleaned[..end] {
        "" => "attachment".into(),
        name => name.into(),
    }
}

fn io_error(path: &Path, e: &std::io::Error) -> Response {
    attach_error("io_error", format!("{}: {e}", path.display()))
}

/// Removes upload directories not modified within `ttl` of `now`.
fn sweep_stale(dir: &Path, now: SystemTime, ttl: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > ttl);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn attach_chunk(
    dir: &Path,
    upload_id: &str,
    name: &str,
    offset: u64,
    data_base64: &str,
    done: bool,
) -> Response {
    if !is_valid_upload_id(upload_id) {
        return attach_error(
            "invalid_upload_id",
            format!("upload_id must be 1-{UPLOAD_ID_MAX_LEN} letters, digits, '-' or '_'"),
        );
    }
    let data = match BASE64_STANDARD.decode(data_base64) {
        Ok(d) => d,
        Err(e) => return attach_error("invalid_base64", format!("data_base64: {e}")),
    };
    let upload_dir = dir.join(upload_id);
    let partial = upload_dir.join(PARTIAL_NAME);
    if offset == 0 {
        sweep_stale(dir, SystemTime::now(), ATTACHMENT_TTL);
        let _ = std::fs::remove_dir_all(&upload_dir);
        if let Err(e) = DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&upload_dir)
        {
            return io_error(&upload_dir, &e);
        }
    } else {
        let received = std::fs::metadata(&partial).map_or(0, |m| m.len());
        if received != offset {
            return attach_error(
                "offset_mismatch",
                format!("upload {upload_id} has {received} bytes, chunk starts at {offset}"),
            );
        }
    }
    let received = offset + data.len() as u64;
    if received > ATTACHMENT_MAX_BYTES {
        let _ = std::fs::remove_dir_all(&upload_dir);
        return attach_error(
            "too_large",
            format!("attachments are limited to {ATTACHMENT_MAX_BYTES} bytes"),
        );
    }
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&partial)
        .and_then(|mut file| file.write_all(&data));
    if let Err(e) = written {
        return io_error(&partial, &e);
    }
    if !done {
        return Response::AttachFileResult {
            received,
            path: None,
        };
    }
    let target = upload_dir.join(sanitize_name(name));
    if let Err(e) = std::fs::rename(&partial, &target) {
        return io_error(&target, &e);
    }
    Response::AttachFileResult {
        received,
        path: Some(target.to_string_lossy().into_owned()),
    }
}

fn attachments_dir() -> std::io::Result<PathBuf> {
    Ok(pu_core::paths::global_pu_dir()?.join("attachments"))
}

impl Engine {
    pub(super) async fn handle_attach_file(&self, request: Request) -> Response {
        let Request::AttachFile {
            upload_id,
            name,
            offset,
            data_base64,
            done,
        } = request
        else {
            return attach_error("INTERNAL_ERROR", "not an attach_file request");
        };
        tokio::task::spawn_blocking(move || match attachments_dir() {
            Ok(dir) => attach_chunk(&dir, &upload_id, &name, offset, &data_base64, done),
            Err(e) => attach_error("io_error", format!("attachments directory: {e}")),
        })
        .await
        .unwrap_or_else(|e| attach_error("INTERNAL_ERROR", format!("task join error: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn b64(data: &[u8]) -> String {
        BASE64_STANDARD.encode(data)
    }

    fn path_of(response: Response) -> PathBuf {
        match response {
            Response::AttachFileResult {
                path: Some(path), ..
            } => PathBuf::from(path),
            other => panic!("expected a finished upload, got {other:?}"),
        }
    }

    fn error_code(response: Response) -> String {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn given_single_chunk_should_write_file_under_upload_and_return_its_path() {
        // given
        let tmp = TempDir::new().unwrap();

        // when
        let path = path_of(attach_chunk(
            tmp.path(),
            "u1",
            "Screen Shot.png",
            0,
            &b64(b"png"),
            true,
        ));

        // then
        assert_eq!(path, tmp.path().join("u1").join("Screen Shot.png"));
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
    }

    #[test]
    fn given_chunks_in_order_should_reassemble_the_file() {
        // given
        let tmp = TempDir::new().unwrap();
        let first = attach_chunk(tmp.path(), "u1", "a.bin", 0, &b64(b"hello "), false);
        assert!(matches!(
            first,
            Response::AttachFileResult {
                received: 6,
                path: None
            }
        ));

        // when
        let path = path_of(attach_chunk(
            tmp.path(),
            "u1",
            "a.bin",
            6,
            &b64(b"world"),
            true,
        ));

        // then
        assert_eq!(std::fs::read(path).unwrap(), b"hello world");
    }

    #[test]
    fn given_chunk_at_wrong_offset_should_refuse_it() {
        // given
        let tmp = TempDir::new().unwrap();
        attach_chunk(tmp.path(), "u1", "a.bin", 0, &b64(b"abc"), false);

        // when
        let response = attach_chunk(tmp.path(), "u1", "a.bin", 2, &b64(b"x"), true);

        // then
        assert_eq!(error_code(response), "offset_mismatch");
    }

    #[test]
    fn given_new_first_chunk_should_restart_the_upload() {
        // given
        let tmp = TempDir::new().unwrap();
        attach_chunk(tmp.path(), "u1", "a.bin", 0, &b64(b"stale"), false);

        // when
        let path = path_of(attach_chunk(
            tmp.path(),
            "u1",
            "a.bin",
            0,
            &b64(b"new"),
            true,
        ));

        // then
        assert_eq!(std::fs::read(path).unwrap(), b"new");
    }

    #[test]
    fn given_upload_id_with_path_characters_should_refuse_it() {
        // given
        let tmp = TempDir::new().unwrap();

        // when
        let response = attach_chunk(tmp.path(), "../escape", "a.png", 0, &b64(b"x"), true);

        // then
        assert_eq!(error_code(response), "invalid_upload_id");
    }

    #[test]
    fn given_name_with_directories_should_keep_file_inside_upload() {
        // given
        let tmp = TempDir::new().unwrap();

        // when
        let path = path_of(attach_chunk(
            tmp.path(),
            "u1",
            "../../etc/passwd",
            0,
            &b64(b"x"),
            true,
        ));

        // then
        assert_eq!(path, tmp.path().join("u1").join("passwd"));
    }

    #[test]
    fn given_hidden_or_empty_name_should_use_a_visible_name() {
        assert_eq!(sanitize_name(".partial"), "partial");
        assert_eq!(sanitize_name(""), "attachment");
        assert_eq!(sanitize_name("dir/"), "attachment");
        assert_eq!(sanitize_name("a\nb.png"), "ab.png");
    }

    #[test]
    fn given_long_multibyte_name_should_truncate_on_a_char_boundary() {
        // given
        let name = "é".repeat(NAME_MAX_BYTES);

        // when
        let sanitized = sanitize_name(&name);

        // then
        assert_eq!(sanitized.len(), NAME_MAX_BYTES);
    }

    #[test]
    fn given_upload_past_size_limit_should_refuse_and_discard_it() {
        // given
        let tmp = TempDir::new().unwrap();
        attach_chunk(tmp.path(), "u1", "a.bin", 0, &b64(b"abc"), false);
        let partial = tmp.path().join("u1").join(PARTIAL_NAME);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&partial)
            .unwrap()
            .set_len(ATTACHMENT_MAX_BYTES)
            .unwrap();

        // when
        let response = attach_chunk(
            tmp.path(),
            "u1",
            "a.bin",
            ATTACHMENT_MAX_BYTES,
            &b64(b"x"),
            true,
        );

        // then
        assert_eq!(error_code(response), "too_large");
        assert!(!tmp.path().join("u1").exists());
    }

    #[test]
    fn given_upload_older_than_ttl_should_sweep_it_and_keep_fresh_ones() {
        // given
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("old")).unwrap();
        let later = SystemTime::now() + ATTACHMENT_TTL + Duration::from_secs(60);

        // when
        sweep_stale(tmp.path(), later, ATTACHMENT_TTL);
        std::fs::create_dir(tmp.path().join("fresh")).unwrap();
        sweep_stale(tmp.path(), SystemTime::now(), ATTACHMENT_TTL);

        // then
        assert!(!tmp.path().join("old").exists());
        assert!(tmp.path().join("fresh").exists());
    }

    #[test]
    fn given_new_upload_dir_should_be_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt as _;
        // given
        let tmp = TempDir::new().unwrap();

        // when
        attach_chunk(tmp.path(), "u1", "a.png", 0, &b64(b"x"), true);

        // then
        let mode = std::fs::metadata(tmp.path().join("u1"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
