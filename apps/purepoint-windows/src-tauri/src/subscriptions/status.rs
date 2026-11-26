//! Port of `Services/DaemonStatusSubscription.swift`: a project's `subscribe_status` stream.
//!
//! Losing an established stream means the daemon went away; even if the endpoint answers
//! again it may be a replacement that has never initialized this project or resumed its
//! agents, so the loop reports `daemon_lost` and ends instead of reconnecting. So does a
//! daemon that never answered within [`RetryPolicy::daemon_lost_after`] attempts. `BUSY`
//! (alive, at its connection limit) is retried. An `error` reply to the subscribe, or any
//! other reply, ends the loop as `failed`.
//!
//! Agent models are built from the raw payload by the front end (`DaemonWorkspaceService`).

use pu_core::protocol::{Request, Response};

use super::{
    Emit, Failure, Retries, RetryPolicy, SubscriptionEvent, SubscriptionKind, SubscriptionState,
};
use super::{emit_state, next_reply};
use crate::daemon::client::{DaemonClient, DaemonError, write_request};

/// Runs until the loop ends (`daemon_lost` or `failed`) or the task is aborted.
pub async fn run(client: DaemonClient, project_root: String, policy: RetryPolicy, emit: Emit) {
    let report = |state| emit_state(&emit, SubscriptionKind::Status, Some(&project_root), state);
    let mut subscribed = false;
    let mut retries = Retries::new(policy);
    loop {
        let error = match stream(&client, &project_root, &emit, &mut subscribed).await {
            Failure::Fatal(message) => return report(SubscriptionState::Failed { message }),
            Failure::Retry(error) => error,
        };
        if !retries.fail() {
            return report(SubscriptionState::Failed {
                message: error.to_string(),
            });
        }
        if subscribed || retries.daemon_lost() {
            return report(SubscriptionState::DaemonLost);
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
    subscribed: &mut bool,
) -> Failure {
    let (mut reader, mut writer) = match client.connect().await {
        Ok(connection) => connection,
        Err(error) => return Failure::Retry(error),
    };
    let request = Request::SubscribeStatus {
        project_root: project_root.to_owned(),
    };
    if let Err(error) = write_request(&mut writer, &request).await {
        return Failure::Retry(error);
    }
    match next_reply(&mut reader).await {
        Ok(Ok(Response::StatusSubscribed)) => {}
        // At its connection limit the daemon is alive: retry, don't give up.
        Ok(Err(error @ DaemonError::Busy(_))) => return Failure::Retry(error),
        Ok(Ok(Response::Error { message, .. })) => {
            return Failure::Fatal(format!("Status subscribe failed: {message}"));
        }
        Ok(_) => return Failure::Fatal("Unexpected response during status subscribe".into()),
        Err(error) => return Failure::Retry(error),
    }
    *subscribed = true;
    emit_state(
        emit,
        SubscriptionKind::Status,
        Some(project_root),
        SubscriptionState::Subscribed,
    );
    // The write half stays open: closing it would end the daemon's stream.
    loop {
        match next_reply(&mut reader).await {
            Ok(Ok(Response::StatusEvent { worktrees, agents })) => {
                emit(SubscriptionEvent::Status {
                    project_root: project_root.to_owned(),
                    worktrees,
                    agents,
                })
            }
            Ok(_) => {}
            Err(error) => return Failure::Retry(error),
        }
    }
}
