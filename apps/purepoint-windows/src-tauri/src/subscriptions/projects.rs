//! Port of `Services/DaemonProjectsSubscription.swift`: the daemon's registered projects
//! (protocol 13). The daemon sends the full list of roots when the stream opens and again
//! after every add, remove or reorder, so a project added or removed anywhere shows up
//! without polling. Unlike the status stream it never gives up: a restarted daemon is
//! simply subscribed to again.

use pu_core::protocol::{Request, Response};

use super::{Emit, Retries, RetryPolicy, SubscriptionEvent, SubscriptionKind, SubscriptionState};
use super::{emit_state, next_reply};
use crate::daemon::client::{DaemonClient, DaemonError, write_request};

/// Runs until the task is aborted.
pub async fn run(client: DaemonClient, policy: RetryPolicy, emit: Emit) {
    let mut retries = Retries::new(policy);
    loop {
        let error = stream(&client, &emit).await;
        // `max_retries` is `None` for this stream, so `fail` never ends it.
        if !retries.fail() {
            return emit_state(
                &emit,
                SubscriptionKind::Projects,
                None,
                SubscriptionState::Failed {
                    message: error.to_string(),
                },
            );
        }
        emit_state(
            &emit,
            SubscriptionKind::Projects,
            None,
            retries.retrying(&error),
        );
        retries.sleep().await;
    }
}

async fn stream(client: &DaemonClient, emit: &Emit) -> DaemonError {
    let (mut reader, mut writer) = match client.connect().await {
        Ok(connection) => connection,
        Err(error) => return error,
    };
    if let Err(error) = write_request(&mut writer, &Request::SubscribeProjects).await {
        return error;
    }
    match next_reply(&mut reader).await {
        Ok(Ok(Response::ProjectsSubscribed)) => {}
        Ok(Err(error)) | Err(error) => return error,
        Ok(Ok(_)) => {
            return DaemonError::Io("Unexpected response during projects subscribe".into());
        }
    }
    emit_state(
        emit,
        SubscriptionKind::Projects,
        None,
        SubscriptionState::Subscribed,
    );
    loop {
        match next_reply(&mut reader).await {
            Ok(Ok(Response::ProjectsChanged { projects })) => {
                emit(SubscriptionEvent::Projects { projects });
            }
            Ok(_) => {}
            Err(error) => return error,
        }
    }
}
