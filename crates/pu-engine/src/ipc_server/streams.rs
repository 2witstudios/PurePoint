use crate::engine::{AttachmentCursor, Engine};
use pu_core::protocol::{Request, Response};

use super::{
    ATTACH_OUTPUT_CHUNK_SIZE, Frame, IpcReader, IpcWriter, parse_stream_request, read_frame,
    reject_frame_too_large, write_response,
};

/// Streaming attach sub-loop: sends all buffered output, then streams new output
/// while accepting Input/Resize commands from the client. Like every stream
/// handler, returns whether the connection may go back to request mode, and
/// reads through the connection's `partial` line buffer so a line that is
/// still arriving when the stream ends is finished in request mode.
pub(super) async fn handle_attach_stream(
    reader: &mut IpcReader,
    partial: &mut Vec<u8>,
    writer: &mut IpcWriter,
    engine: &Engine,
    agent_id: &str,
    replay: Option<pu_core::protocol::Replay>,
    client_id: Option<String>,
) -> bool {
    let (buffer, master_fd, mut exit_rx) = match engine.get_attach_handles(agent_id).await {
        Some(handles) => handles,
        None => {
            let _ = write_response(
                writer,
                &Response::Error {
                    code: "AGENT_NOT_FOUND".into(),
                    message: format!("agent {agent_id} was removed during attach"),
                },
            )
            .await;
            return true;
        }
    };

    let snapshot = replay == Some(pu_core::protocol::Replay::Snapshot);
    let identified = client_id.is_some();
    let (attachment, mut size_rx) = engine.register_attachment(agent_id, client_id).await;
    // Always unregister, including failures writing initial replay.
    let keep_open = run_attachment(
        reader,
        partial,
        writer,
        engine,
        agent_id,
        snapshot,
        identified,
        attachment,
        &mut size_rx,
        buffer,
        master_fd,
        &mut exit_rx,
    )
    .await;
    engine.unregister_attachment(agent_id, attachment).await;
    keep_open
}

#[allow(clippy::too_many_arguments)]
async fn run_attachment(
    reader: &mut IpcReader,
    partial: &mut Vec<u8>,
    writer: &mut IpcWriter,
    engine: &Engine,
    agent_id: &str,
    snapshot: bool,
    identified: bool,
    attachment: u64,
    size_rx: &mut tokio::sync::broadcast::Receiver<crate::engine::PtySize>,
    buffer: std::sync::Arc<crate::output_buffer::OutputBuffer>,
    master_fd: std::sync::Arc<std::os::fd::OwnedFd>,
    exit_rx: &mut tokio::sync::watch::Receiver<Option<i32>>,
) -> bool {
    let mut watcher = buffer.subscribe();
    let (data, offset, initial_size) = engine
        .attachment_replay(agent_id, &buffer, snapshot, size_rx)
        .await;
    if write_response(
        writer,
        &Response::AttachReady {
            buffered_bytes: data.len(),
        },
    )
    .await
    .is_err()
    {
        return true;
    }
    let mut cursor = AttachmentCursor {
        offset,
        size: initial_size.clone(),
        snapshot,
        identified,
    };
    if identified && write_size(writer, initial_size).await.is_err() {
        return true;
    }
    if write_output_chunks(writer, agent_id, &data).await.is_err() {
        return true;
    }
    if exit_rx.borrow().is_some() {
        return true;
    }

    loop {
        tokio::select! {
            biased;
            result = size_rx.recv(), if identified => {
                if matches!(result, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break }
                let update = engine.attachment_update(agent_id, &buffer, &mut cursor, size_rx, Some(result)).await;
                if write_attachment_update(writer, agent_id, update).await.is_err() { break }
            }
            Ok(()) = watcher.changed() => {
                let update = engine.attachment_update(agent_id, &buffer, &mut cursor, size_rx, None).await;
                if write_attachment_update(writer, agent_id, update).await.is_err() { break }
            }
            Ok(()) = exit_rx.changed() => {
                let update = engine.attachment_update(agent_id, &buffer, &mut cursor, size_rx, None).await;
                let _ = write_attachment_update(writer, agent_id, update).await;
                break;
            }
            frame = read_frame(reader, partial) => {
                let line = match frame {
                    Ok(Frame::Line(line)) => line,
                    Ok(Frame::Eof) => break,
                    Ok(Frame::TooLarge) => {
                        reject_frame_too_large(writer).await;
                        return false;
                    }
                    // Unreadable input leaves no trustworthy line boundary.
                    Err(_) => return false,
                };
                match parse_stream_request(&line) {
                    Some(Request::Input { data, .. }) => {
                        engine.write_to_pty(&master_fd, &data).await.ok();
                    }
                    Some(Request::Resize { cols, rows, .. }) => {
                        engine.attachment_resize(agent_id, attachment, cols, rows, false).await;
                    }
                    Some(Request::ClaimSize { cols, rows }) => {
                        engine.attachment_resize(agent_id, attachment, cols, rows, true).await;
                    }
                    _ => break,
                }
            }
        }
    }
    tracing::debug!(agent_id, "attach stream ended");
    true
}

pub(super) async fn write_attachment_update(
    writer: &mut IpcWriter,
    agent_id: &str,
    update: crate::engine::AttachmentUpdate,
) -> std::io::Result<()> {
    if update.resync {
        write_response(writer, &Response::Resync).await?;
    }
    for size in update.sizes {
        write_size(writer, size).await?;
    }
    write_output_chunks(writer, agent_id, &update.data).await
}

async fn write_size(
    writer: &mut IpcWriter,
    size: (u16, u16, Option<String>),
) -> std::io::Result<()> {
    write_response(
        writer,
        &Response::PtySize {
            cols: size.0,
            rows: size.1,
            owner_client_id: size.2,
        },
    )
    .await
}

/// Streaming grid subscription: forwards GridEvent broadcasts to subscriber,
/// accepts incoming GridCommand requests from the subscriber connection.
pub(super) async fn handle_grid_stream(
    reader: &mut IpcReader,
    partial: &mut Vec<u8>,
    writer: &mut IpcWriter,
    engine: &Engine,
    project_root: &str,
) -> bool {
    let mut rx = engine.subscribe_grid(project_root).await;
    let pr = project_root.to_string();

    tracing::debug!(project_root, "grid stream started");

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(command) => {
                        let resp = Response::GridEvent {
                            project_root: pr.clone(),
                            command,
                        };
                        if write_response(writer, &resp).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(project_root, "grid subscriber lagged {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = read_frame(reader, partial) => {
                let line = match frame {
                    Ok(Frame::Line(line)) => line,
                    Ok(Frame::Eof) => break,
                    Ok(Frame::TooLarge) => {
                        reject_frame_too_large(writer).await;
                        return false;
                    }
                    // Unreadable input leaves no trustworthy line boundary.
                    Err(_) => return false,
                };
                match parse_stream_request(&line) {
                    Some(Request::GridCommand { command, .. }) => {
                        let resp = engine.handle_grid_command(&pr, command).await;
                        if write_response(writer, &resp).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }
    }
    tracing::debug!(project_root, "grid stream ended");
    true
}

/// Computes and sends a full status snapshot. Returns `false` if the write failed.
async fn send_status(writer: &mut IpcWriter, engine: &Engine, project_root: &str) -> bool {
    if let Ok((worktrees, agents)) = engine.compute_full_status(project_root).await {
        let resp = Response::StatusEvent { worktrees, agents };
        write_response(writer, &resp).await.is_ok()
    } else {
        true
    }
}

/// Streaming status subscription: pushes full StatusEvent on every state change.
/// Client receives real-time updates without polling.
pub(super) async fn handle_status_stream(
    reader: &mut IpcReader,
    partial: &mut Vec<u8>,
    writer: &mut IpcWriter,
    engine: &Engine,
    project_root: &str,
) -> bool {
    let mut rx = engine.subscribe_status(project_root).await;
    let pr = project_root.to_string();

    tracing::debug!(project_root, "status stream started");

    // Send initial status immediately
    if !send_status(writer, engine, &pr).await {
        return true;
    }

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(()) => {
                        // Drain any queued signals (batch rapid changes)
                        while rx.try_recv().is_ok() {}
                        if !send_status(writer, engine, &pr).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(project_root, "status subscriber lagged {n} messages");
                        if !send_status(writer, engine, &pr).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = read_frame(reader, partial) => {
                match frame {
                    // Any request ends the status stream.
                    Ok(Frame::Line(_)) | Ok(Frame::Eof) => break,
                    Ok(Frame::TooLarge) => {
                        reject_frame_too_large(writer).await;
                        return false;
                    }
                    Err(_) => return false,
                }
            }
        }
    }
    tracing::debug!(project_root, "status stream ended");
    true
}

async fn send_projects(writer: &mut IpcWriter, engine: &Engine) -> bool {
    let mut projects = engine.registered_projects();
    projects.sort();
    write_response(writer, &Response::ProjectsChanged { projects })
        .await
        .is_ok()
}

/// Streaming project registry: pushes the full list of registered roots now
/// and after every add or remove, so every client shows the same projects.
pub(super) async fn handle_projects_stream(
    reader: &mut IpcReader,
    partial: &mut Vec<u8>,
    writer: &mut IpcWriter,
    engine: &Engine,
) -> bool {
    // Subscribe before the first send so no change between them is missed.
    let mut rx = engine.subscribe_projects();
    engine.registry_ready().await;
    if !send_projects(writer, engine).await {
        return true;
    }
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        while rx.try_recv().is_ok() {}
                        if !send_projects(writer, engine).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = read_frame(reader, partial) => {
                match frame {
                    // Any request ends the projects stream.
                    Ok(Frame::Line(_)) | Ok(Frame::Eof) => break,
                    Ok(Frame::TooLarge) => {
                        reject_frame_too_large(writer).await;
                        return false;
                    }
                    Err(_) => return false,
                }
            }
        }
    }
    true
}

async fn write_output_chunks(
    writer: &mut IpcWriter,
    agent_id: &str,
    data: &[u8],
) -> std::io::Result<()> {
    for chunk in data.chunks(ATTACH_OUTPUT_CHUNK_SIZE) {
        let resp = Response::Output {
            agent_id: agent_id.to_string(),
            data: chunk.to_vec(),
        };
        write_response(writer, &resp).await?;
    }
    Ok(())
}

/// How long file events are gathered before one `FsEvent` goes out.
const FS_EVENT_COALESCE: std::time::Duration = std::time::Duration::from_millis(200);
const FS_EVENT_QUEUE: usize = 4096;
/// A client that hasn't drained an `FsEvent` within this long (e.g. a laptop
/// asleep at the far end of an ssh forward) is treated as gone.
const FS_EVENT_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Writes one `FsEvent`; false when the write failed or timed out, which ends
/// the stream.
async fn write_fs_event(
    writer: &mut IpcWriter,
    resp: &Response,
    timeout: std::time::Duration,
) -> bool {
    matches!(
        tokio::time::timeout(timeout, write_response(writer, resp)).await,
        Ok(Ok(()))
    )
}
/// Scan interval of the polling fallback. It only runs on trees too large for
/// native watches, where every scan walks and stats the whole tree.
const FS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

type BoxedWatcher = Box<dyn notify::Watcher + Send>;

/// Watch `root` with the native backend; if a recursive watch cannot be set up
/// (e.g. Linux inotify watch limit on a huge tree), fall back to polling.
/// Returns the watcher and whether it polls.
fn watch_dir<H, F>(
    root: &std::path::Path,
    recursive: bool,
    poll_interval: std::time::Duration,
    handler: F,
    native: impl FnOnce(H) -> notify::Result<BoxedWatcher>,
) -> notify::Result<(BoxedWatcher, bool)>
where
    H: notify::EventHandler,
    F: Fn() -> H,
{
    use notify::{RecursiveMode, Watcher};
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    let native_error = match native(handler()).and_then(|mut w| w.watch(root, mode).map(|()| w)) {
        Ok(watcher) => return Ok((watcher, false)),
        Err(e) => e,
    };
    if !recursive {
        return Err(native_error);
    }
    tracing::warn!(
        root = %root.display(),
        error = %native_error,
        "native recursive watch failed; polling instead"
    );
    let config = notify::Config::default()
        .with_poll_interval(poll_interval)
        .with_follow_symlinks(false);
    let mut poller = notify::PollWatcher::new(handler(), config)?;
    poller.watch(root, mode)?;
    Ok((Box::new(poller), true))
}

/// Streaming file-watch subscription: pushes coalesced `FsEvent`s for changes
/// under `root` until the client disconnects (or sends anything).
pub(super) async fn handle_fs_stream(
    reader: &mut IpcReader,
    partial: &mut Vec<u8>,
    writer: &mut IpcWriter,
    engine: &Engine,
    root: &str,
    recursive: bool,
) {
    use notify::EventKind;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = match engine.resolve_fs_dir(root).await {
        Ok(dir) => dir,
        Err(resp) => {
            let _ = write_response(writer, &resp).await;
            return;
        }
    };
    let root_str = root.to_string_lossy().into_owned();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<std::path::PathBuf>>(FS_EVENT_QUEUE);
    // A full queue or a watcher error means events were lost: report the root
    // so the client rescans instead of trusting a partial picture.
    let overflowed = Arc::new(AtomicBool::new(false));
    let overflow_flag = overflowed.clone();
    let watch_root = root.clone();
    let watcher = tokio::task::spawn_blocking(move || {
        let handler = move || {
            let tx = tx.clone();
            let overflow_flag = overflow_flag.clone();
            move |res: notify::Result<notify::Event>| {
                let paths = match res {
                    Ok(event) if matches!(event.kind, EventKind::Access(_)) => return,
                    Ok(event) => event.paths,
                    Err(_) => {
                        overflow_flag.store(true, Ordering::Relaxed);
                        Vec::new()
                    }
                };
                if tx.try_send(paths).is_err() {
                    overflow_flag.store(true, Ordering::Relaxed);
                }
            }
        };
        watch_dir(&watch_root, recursive, FS_POLL_INTERVAL, handler, |h| {
            // Never follow a symlink out of the root (inotify and kqueue would).
            let config = notify::Config::default().with_follow_symlinks(false);
            <notify::RecommendedWatcher as notify::Watcher>::new(h, config)
                .map(|w| Box::new(w) as BoxedWatcher)
        })
    })
    .await;
    let _watcher = match watcher {
        Ok(Ok((watcher, _polling))) => watcher,
        Ok(Err(e)) => {
            let resp = Response::Error {
                code: "watch_failed".into(),
                message: format!("cannot watch {root_str}: {e}"),
            };
            let _ = write_response(writer, &resp).await;
            return;
        }
        Err(e) => {
            let resp = Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            };
            let _ = write_response(writer, &resp).await;
            return;
        }
    };

    tracing::debug!(root = root_str, "fs stream started");

    let mut pending: BTreeSet<String> = BTreeSet::new();
    let mut flush_at: Option<tokio::time::Instant> = None;
    loop {
        let flush = async {
            match flush_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            Some(paths) = rx.recv() => {
                pending.extend(
                    paths
                        .into_iter()
                        .filter(|p| p.starts_with(&root))
                        .map(|p| p.to_string_lossy().into_owned()),
                );
                if overflowed.load(Ordering::Relaxed) {
                    pending.insert(root_str.clone());
                }
                if !pending.is_empty() && flush_at.is_none() {
                    flush_at = Some(tokio::time::Instant::now() + FS_EVENT_COALESCE);
                }
            }
            _ = flush => {
                flush_at = None;
                if overflowed.swap(false, Ordering::Relaxed) {
                    pending.insert(root_str.clone());
                }
                let resp = Response::FsEvent {
                    root: root_str.clone(),
                    paths: std::mem::take(&mut pending).into_iter().collect(),
                };
                if !write_fs_event(writer, &resp, FS_EVENT_WRITE_TIMEOUT).await {
                    break;
                }
            }
            frame = read_frame(reader, partial) => {
                // The connection closes after a watch stream either way.
                if matches!(frame, Ok(Frame::TooLarge)) {
                    reject_frame_too_large(writer).await;
                }
                break;
            }
        }
    }
    tracing::debug!(root = root_str, "fs stream ended");
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn given_client_not_reading_should_time_out_fs_event_write() {
        let (ours, _theirs) = tokio::net::UnixStream::pair().unwrap();
        let (_reader, mut writer) = ours.into_split();
        // Far more than the socket buffers hold, and nobody reads `_theirs`.
        let resp = Response::FsEvent {
            root: "/r".into(),
            paths: vec!["/r/".to_string() + &"x".repeat(1000); 16_384],
        };
        let started = std::time::Instant::now();
        let written =
            write_fs_event(&mut writer, &resp, std::time::Duration::from_millis(200)).await;
        assert!(!written);
        // Bounded (it never blocks on the stalled reader); generous for loaded CI.
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
    }
    use std::sync::mpsc;

    fn collector() -> (
        impl Fn() -> mpsc::Sender<notify::Result<notify::Event>>,
        mpsc::Receiver<notify::Result<notify::Event>>,
    ) {
        let (tx, rx) = mpsc::channel();
        (move || tx.clone(), rx)
    }

    fn failing_native(
        _: mpsc::Sender<notify::Result<notify::Event>>,
    ) -> notify::Result<BoxedWatcher> {
        Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch))
    }

    #[test]
    fn given_native_recursive_watch_fails_should_poll_and_still_report_changes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let (handler, rx) = collector();
        let interval = std::time::Duration::from_millis(200);
        let (_watcher, polling) =
            watch_dir(&root, true, interval, handler, failing_native).unwrap();
        assert!(polling);
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/new.txt"), "x").unwrap();
        let deadline = std::time::Instant::now() + interval * 20;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let event = rx
                .recv_timeout(remaining)
                .expect("polling watcher reported no change")
                .unwrap();
            if event.paths.iter().any(|p| p.ends_with("sub/new.txt")) {
                break;
            }
        }
    }

    #[test]
    fn given_native_non_recursive_watch_fails_should_report_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (handler, _rx) = collector();
        assert!(watch_dir(tmp.path(), false, FS_POLL_INTERVAL, handler, failing_native).is_err());
    }

    #[test]
    fn given_native_watch_works_should_not_poll() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (handler, _rx) = collector();
        let (_watcher, polling) = watch_dir(tmp.path(), true, FS_POLL_INTERVAL, handler, |h| {
            notify::recommended_watcher(h).map(|w| Box::new(w) as BoxedWatcher)
        })
        .unwrap();
        assert!(!polling);
    }
}
