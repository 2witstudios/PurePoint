//! Read-only bridge resource lookup. Invoke inside Session::dispatch while its
//! shared authority guard is held; policy values alone are not authorization.

use crate::{
    daemon_lifecycle::mesh_exports::{MeshExports, ResolvedExport},
    execution_state as state,
};
use pu_core::mesh::{
    Action, PeerPolicy,
    response::{self, Body, ErrorCode},
    wire::{self, Operation, Request},
};

/// Trusted synchronous dispatch hook for Session::dispatch. It exposes only
/// durable mesh records; legacy PID-only processes are never adopted. All
/// mutation operations stay disabled until the execution contract is proven.
pub fn dispatch(
    ledger: &state::Ledger,
    exports: &MeshExports,
    caller: &state::Caller,
    policy: &PeerPolicy,
    request: &Request,
) -> Result<Vec<u8>, wire::WireError> {
    request.validate()?;
    let body =
        read(ledger, exports, caller, policy, request).unwrap_or_else(|code| Body::Error { code });
    response::Response {
        request_id: request.request_id.clone(),
        daemon_id: ledger.owner_daemon().into(),
        body,
    }
    .encode()
}

fn read(
    ledger: &state::Ledger,
    exports: &MeshExports,
    caller: &state::Caller,
    policy: &PeerPolicy,
    request: &Request,
) -> Result<Body, ErrorCode> {
    let (peer, generation) = match caller.principal() {
        state::Principal::Peer { id, generation } => (id, *generation),
        _ => return Err(ErrorCode::Denied),
    };
    if ledger.owner_daemon() != request.target_daemon
        || exports.owner_daemon() != ledger.owner_daemon()
    {
        return Err(ErrorCode::Denied);
    }
    ledger
        .validate_principal(caller)
        .map_err(|_| ErrorCode::Denied)?;
    let now = ledger
        .observe_time(chrono::Utc::now().timestamp())
        .map_err(|_| ErrorCode::Unavailable)?;
    policy
        .visible_exports(peer, generation, now)
        .map_err(|_| ErrorCode::Denied)?;
    let resolve = |export: &wire::Export| {
        policy
            .authorize(peer, generation, &export.id, &Action::Inspect, now)
            .map_err(|_| ErrorCode::Denied)?;
        exports
            .resolve(&export.id, export.generation)
            .map_err(|_| ErrorCode::Unavailable)
    };
    match &request.operation {
        Operation::Inventory {} => {
            let mut visible = Vec::new();
            for grant in policy
                .visible_exports(peer, generation, now)
                .map_err(|_| ErrorCode::Denied)?
            {
                if exports.resolve(&grant.export_id, 1).is_ok() {
                    visible.push(response::ExportSummary {
                        export: wire::Export {
                            id: grant.export_id.clone(),
                            generation: 1,
                        },
                        runtimes: grant.runtimes.iter().cloned().collect(),
                    });
                }
            }
            Ok(Body::Inventory { exports: visible })
        }
        Operation::Status {
            export,
            agent_id,
            after_agent,
            limit,
        } => {
            let resolved = resolve(export)?;
            let records = if let Some(id) = agent_id {
                let agent = ledger
                    .agent(&state::AgentId(id.clone()))
                    .map_err(map_state)?;
                let run = ledger.run(&agent.current_run).map_err(map_state)?;
                verify_run(ledger, &resolved, &agent, &run)?;
                vec![(agent, run)]
            } else {
                ledger
                    .agents_for_export(
                        root(&resolved)?,
                        resolved.id(),
                        after_agent.as_deref(),
                        usize::from(*limit) + 1,
                    )
                    .map_err(map_state)?
            };
            let more = records.len() > usize::from(*limit);
            let agents = records
                .into_iter()
                .take(usize::from(*limit))
                .map(|(agent, run)| summary(export, &agent, &run, caller))
                .collect::<Vec<_>>();
            let next_agent = if more {
                agents.last().map(|a| a.target.agent_id.clone())
            } else {
                None
            };
            exports
                .revalidate(&resolved)
                .map_err(|_| ErrorCode::Unavailable)?;
            Ok(Body::Status { agents, next_agent })
        }
        Operation::Receipt {
            export,
            mutation,
            payload_digest,
        } => {
            let resolved = resolve(export)?;
            let key = state::OperationKey {
                window: state::WindowId(mutation.window),
                id: state::OperationId(mutation.id.clone()),
                digest: state::PayloadDigest(*payload_digest),
            };
            let receipt = ledger.operation_receipt(caller, &key).map_err(map_state)?;
            let evidence = if let Some(receipt) = receipt {
                let (agent_id, run_id, epoch, revision) = match receipt {
                    state::OperationReceipt::Launch(r) => {
                        (r.agent_id, r.run_id, r.guard.epoch, r.guard.revision)
                    }
                    state::OperationReceipt::Agent(a) => {
                        (a.agent_id, a.current_run, a.epoch, a.revision)
                    }
                };
                let agent = ledger.agent(&agent_id).map_err(map_state)?;
                let run = ledger.run(&run_id).map_err(map_state)?;
                verify_run(ledger, &resolved, &agent, &run)?;
                Some(response::ReceiptEvidence {
                    target: wire::AgentTarget {
                        export: export.clone(),
                        agent_id: agent_id.0,
                        run_id: run_id.0,
                    },
                    epoch: epoch.0,
                    revision: revision.0,
                })
            } else {
                None
            };
            exports
                .revalidate(&resolved)
                .map_err(|_| ErrorCode::Unavailable)?;
            Ok(Body::Receipt { evidence })
        }
        Operation::InputReceipt {
            target,
            writer,
            sequence,
        } => {
            let resolved = resolve(&target.export)?;
            let agent = ledger
                .agent(&state::AgentId(target.agent_id.clone()))
                .map_err(map_state)?;
            let run = ledger
                .run(&state::RunId(target.run_id.clone()))
                .map_err(map_state)?;
            verify_run(ledger, &resolved, &agent, &run)?;
            let writer_record = ledger.writer(state::WriterId(*writer)).map_err(map_state)?;
            if writer_record.agent_id != agent.agent_id
                || writer_record.run_id != run.run_id
                || !same_principal(&writer_record.principal, caller.principal())
            {
                return Err(ErrorCode::Denied);
            }
            let evidence = ledger
                .input_receipt(writer_record.id, *sequence)
                .map_err(map_state)?
                .map(|ack| response::InputEvidence {
                    target: target.clone(),
                    writer: ack.writer.0,
                    sequence: ack.sequence,
                    digest: ack.digest.0,
                    payload_bytes: ack.payload_bytes,
                    outcome: match ack.outcome {
                        state::InputOutcome::Accepted => response::InputOutcome::Accepted,
                        state::InputOutcome::Written => response::InputOutcome::Written,
                        state::InputOutcome::Unknown => response::InputOutcome::Unknown,
                        state::InputOutcome::Partial { bytes } => {
                            response::InputOutcome::Partial { bytes }
                        }
                    },
                });
            exports
                .revalidate(&resolved)
                .map_err(|_| ErrorCode::Unavailable)?;
            Ok(Body::InputReceipt { evidence })
        }
        Operation::ReadConversation { .. } | Operation::SubscribeOutput { .. } => {
            Err(ErrorCode::Unsupported)
        }
        _ => Err(ErrorCode::ExecutionDisabled),
    }
}

fn root(export: &ResolvedExport) -> Result<&str, ErrorCode> {
    export.project_root().to_str().ok_or(ErrorCode::Unavailable)
}

fn verify_run(
    ledger: &state::Ledger,
    export: &ResolvedExport,
    agent: &state::AgentRecord,
    run: &state::RunRecord,
) -> Result<(), ErrorCode> {
    if agent.owner_daemon != ledger.owner_daemon()
        || agent.agent_id != run.agent_id
        || run.intent.export.as_deref() != Some(export.id())
        || run.intent.project != root(export)?
    {
        Err(ErrorCode::Unavailable)
    } else {
        Ok(())
    }
}

fn same_principal(left: &state::Principal, right: &state::Principal) -> bool {
    match (left, right) {
        (state::Principal::Peer { id: a, .. }, state::Principal::Peer { id: b, .. })
        | (state::Principal::Local { id: a }, state::Principal::Local { id: b }) => a == b,
        _ => false,
    }
}

fn summary(
    export: &wire::Export,
    agent: &state::AgentRecord,
    run: &state::RunRecord,
    caller: &state::Caller,
) -> response::AgentSummary {
    response::AgentSummary {
        target: wire::AgentTarget {
            export: export.clone(),
            agent_id: agent.agent_id.0.clone(),
            run_id: run.run_id.0.clone(),
        },
        lifecycle: match agent.lifecycle {
            state::Lifecycle::Accepted => response::Lifecycle::Accepted,
            state::Lifecycle::Starting => response::Lifecycle::Starting,
            state::Lifecycle::Running => response::Lifecycle::Running,
            state::Lifecycle::Stopping => response::Lifecycle::Stopping,
            state::Lifecycle::Unknown => response::Lifecycle::Unknown,
            state::Lifecycle::Failed => response::Lifecycle::Failed,
            state::Lifecycle::Terminated => response::Lifecycle::Terminated,
            state::Lifecycle::Interrupted => response::Lifecycle::Interrupted,
        },
        epoch: agent.epoch.0,
        revision: agent.revision.0,
        controller: match &agent.controller {
            None => response::Controller::None,
            Some(principal) if principal == caller.principal() => response::Controller::ThisPeer,
            Some(_) => response::Controller::Other,
        },
        runtime: run.intent.runtime.clone(),
    }
}

fn map_state(error: state::Error) -> ErrorCode {
    match error {
        state::Error::Unauthorized | state::Error::PeerFenced => ErrorCode::Denied,
        state::Error::PayloadConflict => ErrorCode::Conflict,
        state::Error::ReceiptExpired | state::Error::WindowClosed => ErrorCode::ReceiptExpired,
        state::Error::Capacity { .. } => ErrorCode::Capacity,
        _ => ErrorCode::Unavailable,
    }
}

#[cfg(test)]
mod tests;
