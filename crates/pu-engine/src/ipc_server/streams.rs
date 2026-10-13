use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use crate::engine::Engine;
use pu_core::protocol::{Request, Response};

use super::{
    ATTACH_OUTPUT_CHUNK_SIZE, IpcReader, IpcWriter, MAX_MESSAGE_SIZE, parse_stream_request,
    write_response,
};

/// Streaming attach sub-loop: sends all buffered output, then streams new output
/// while accepting Input/Resize commands from the client.
pub(super) async fn handle_attach_stream(
    reader: &mut IpcReader,
    writer: &mut IpcWriter,
    engine: &Engine,
    agent_id: &str,
) {
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
            return;
        }
    };

    tracing::debug!(agent_id, "attach stream started");

    let mut watcher = buffer.subscribe();

    // Send buffered output in fixed-size chunks so the client can start rendering quickly.
    let mut offset = 0;
    let (data, new_offset) = buffer.read_from(offset);
    offset = new_offset;
    if !data.is_empty() && write_output_chunks(writer, agent_id, &data).await.is_err() {
        tracing::debug!(agent_id, "attach stream ended: write error on initial data");
        return;
    }

    // If process already exited, we've sent all buffered output above — return
    // immediately. Without this, the streaming loop deadlocks: watcher never fires
    // (no new output), exit_rx.changed() is disabled, and reader blocks forever.
    if exit_rx.borrow().is_some() {
        tracing::debug!(agent_id, "attach stream: process already exited");
        return;
    }

    let mut line = String::new();
    loop {
        tokio::select! {
            Ok(()) = watcher.changed() => {
                let (data, new_offset) = buffer.read_from(offset);
                offset = new_offset;
                if !data.is_empty()
                    && write_output_chunks(writer, agent_id, &data).await.is_err()
                {
                    tracing::debug!(agent_id, "attach stream ended: write error");
                    break;
                }
            }
            Ok(()) = exit_rx.changed() => {
                // Drain any remaining buffered output
                let (data, _) = buffer.read_from(offset);
                if !data.is_empty() {
                    let _ = write_output_chunks(writer, agent_id, &data).await;
                }
                tracing::debug!(agent_id, "attach stream ended: process exited");
                break;
            }
            result = async {
                line.clear();
                reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
            } => {
                match parse_stream_request(result, &line) {
                    Some(Request::Input { data, .. }) => {
                        engine.write_to_pty(&master_fd, &data).await.ok();
                    }
                    Some(Request::Resize { cols, rows, .. }) => {
                        engine.resize_pty(&master_fd, cols, rows).await.ok();
                    }
                    _ => break,
                }
            }
        }
    }
    tracing::debug!(agent_id, "attach stream ended");
}

/// Streaming grid subscription: forwards GridEvent broadcasts to subscriber,
/// accepts incoming GridCommand requests from the subscriber connection.
pub(super) async fn handle_grid_stream(
    reader: &mut IpcReader,
    writer: &mut IpcWriter,
    engine: &Engine,
    project_root: &str,
) {
    let mut rx = engine.subscribe_grid(project_root).await;
    let pr = project_root.to_string();

    tracing::debug!(project_root, "grid stream started");

    let mut line = String::new();
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
            result = async {
                line.clear();
                reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
            } => {
                match parse_stream_request(result, &line) {
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
    writer: &mut IpcWriter,
    engine: &Engine,
    project_root: &str,
) {
    let mut rx = engine.subscribe_status(project_root).await;
    let pr = project_root.to_string();

    tracing::debug!(project_root, "status stream started");

    // Send initial status immediately
    if !send_status(writer, engine, &pr).await {
        return;
    }

    let mut line = String::new();
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
            _ = async {
                line.clear();
                reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
            } => {
                break;
            }
        }
    }
    tracing::debug!(project_root, "status stream ended");
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

/// Streaming file-watch subscription: pushes coalesced `FsEvent`s for changes
/// under `root` until the client disconnects (or sends anything).
pub(super) async fn handle_fs_stream(
    reader: &mut IpcReader,
    writer: &mut IpcWriter,
    engine: &Engine,
    root: &str,
    recursive: bool,
) {
    use notify::{EventKind, RecursiveMode, Watcher};
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
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
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
            })?;
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher.watch(&watch_root, mode)?;
        Ok::<_, notify::Error>(watcher)
    })
    .await;
    let _watcher = match watcher {
        Ok(Ok(watcher)) => watcher,
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
    let mut line = String::new();
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
                if write_response(writer, &resp).await.is_err() {
                    break;
                }
            }
            _ = async {
                line.clear();
                reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
            } => {
                break;
            }
        }
    }
    tracing::debug!(root = root_str, "fs stream ended");
}
