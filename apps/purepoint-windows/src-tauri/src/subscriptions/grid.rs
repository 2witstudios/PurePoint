//! Port of `Services/DaemonGridSubscription.swift`: a project's `subscribe_grid` stream.
//!
//! Every failure, including an `error` reply, is retried with backoff until the retries
//! run out; there is no `daemon_lost` (the status stream reports it). Grid commands are
//! written on the subscription's own connection; the daemon answers them on it (ignored)
//! and rebroadcasts mutations as `grid_event` to every subscriber, this one included.
//! Applying a command to the pane tree is the front end's (`WorkspaceRegistry`, WIN-6.4).

use std::sync::Arc;

use pu_core::protocol::{GridCommand, Request, Response};

use super::{Emit, Retries, RetryPolicy, SubscriptionEvent, SubscriptionKind, SubscriptionState};
use super::{emit_state, next_reply};
use crate::daemon::client::{BoxWrite, DaemonClient, DaemonError, write_request};

/// The write half of the current grid connection, shared with [`GridWriter::send`].
#[derive(Clone, Default)]
pub struct GridWriter(Arc<tokio::sync::Mutex<Option<BoxWrite>>>);

impl GridWriter {
    /// Write `command` on the current connection; false when there is none or the write
    /// failed (the Swift `try?`).
    pub async fn send(&self, project_root: &str, command: GridCommand) -> bool {
        let mut slot = self.0.lock().await;
        let Some(writer) = slot.as_mut() else {
            return false;
        };
        let request = Request::GridCommand {
            project_root: project_root.to_owned(),
            command,
        };
        write_request(writer, &request).await.is_ok()
    }
}

/// Runs until the retries run out (`failed`) or the task is aborted.
pub async fn run(
    client: DaemonClient,
    project_root: String,
    policy: RetryPolicy,
    emit: Emit,
    writer: GridWriter,
) {
    let report = |state| emit_state(&emit, SubscriptionKind::Grid, Some(&project_root), state);
    let mut retries = Retries::new(policy);
    loop {
        let error = stream(&client, &project_root, &emit, &writer).await;
        *writer.0.lock().await = None;
        if !retries.fail() {
            return report(SubscriptionState::Failed {
                message: error.to_string(),
            });
        }
        report(retries.retrying(&error));
        retries.sleep().await;
    }
}

/// One connection: subscribe, then forward events until the stream ends.
async fn stream(
    client: &DaemonClient,
    project_root: &str,
    emit: &Emit,
    writer: &GridWriter,
) -> DaemonError {
    let (mut reader, write) = match client.connect().await {
        Ok(connection) => connection,
        Err(error) => return error,
    };
    {
        let mut slot = writer.0.lock().await;
        let write = slot.insert(write);
        let request = Request::SubscribeGrid {
            project_root: project_root.to_owned(),
        };
        if let Err(error) = write_request(write, &request).await {
            return error;
        }
    }
    match next_reply(&mut reader).await {
        Ok(Ok(Response::GridSubscribed)) => {}
        Ok(Ok(Response::Error { message, .. })) => {
            return DaemonError::Io(format!("Grid subscribe failed: {message}"));
        }
        Ok(Err(error)) | Err(error) => return error,
        Ok(Ok(_)) => return DaemonError::Io("Unexpected response during grid subscribe".into()),
    }
    emit_state(
        emit,
        SubscriptionKind::Grid,
        Some(project_root),
        SubscriptionState::Subscribed,
    );
    loop {
        match next_reply(&mut reader).await {
            Ok(Ok(Response::GridEvent { command, .. })) => emit(SubscriptionEvent::Grid {
                project_root: project_root.to_owned(),
                command,
            }),
            Ok(Ok(Response::Error { message, .. })) => {
                return DaemonError::Io(format!("Grid subscribe failed: {message}"));
            }
            Ok(Err(error @ DaemonError::Busy(_))) | Err(error) => return error,
            Ok(_) => {}
        }
    }
}
