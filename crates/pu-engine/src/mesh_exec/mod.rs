//! Worker-side bridge execution. Every mutation runs inside `Session::dispatch`
//! while the shared authority barrier is held, then takes the per-agent command
//! boundary, then performs its ledger transition, then its local effect:
//! authority barrier -> per-agent boundary -> ledger transaction -> effect.
//! A fresh durable acceptance is the only permission for a process launch or a
//! PTY write; duplicates return retained evidence and never act again.

mod bulk;
pub mod supervisor;
#[cfg(test)]
mod tests;

pub use supervisor::{Prepared, Proof, Supervisor};

use crate::{
    daemon_lifecycle::{mesh_exports::MeshExports, owned_work::WorkOwner},
    engine::{
        Engine,
        mesh_runs::{Delivery, INPUT_DEADLINE, MeshSpawn, MeshSpawnError},
    },
    execution_state::{
        self as state, AgentId, Caller, Ledger, LocalAdmin, MutationGuard, OperationId,
        OperationKey, PayloadDigest, Principal, RunId, WindowId, WriterCheckpoint, WriterId,
    },
    mesh_listener::BridgeHandler,
    mesh_sessions::{self, SessionAuthority},
};
use pu_core::mesh::{
    Action, PeerPolicy,
    response::{self, Body, ErrorCode, ReceiptEvidence},
    wire::{self, AgentTarget, ControlGuard, Operation, Request},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, Weak},
};

/// Receiver-approved runtimes; shared with administration grant validation.
pub use crate::mesh_runtime::RUNTIME_IDS;

pub struct Executor {
    engine: Weak<Engine>,
    authority: Arc<SessionAuthority>,
    work: Arc<WorkOwner>,
    runtime: tokio::runtime::Handle,
    supervisor: Arc<dyn Supervisor>,
    agents: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    bulk: bulk::Registry,
}

type Outcome = Result<Body, ErrorCode>;

impl Executor {
    pub fn new(
        engine: Weak<Engine>,
        authority: Arc<SessionAuthority>,
        work: Arc<WorkOwner>,
        supervisor: Arc<dyn Supervisor>,
    ) -> Arc<Self> {
        Arc::new(Self {
            engine,
            authority,
            work,
            runtime: tokio::runtime::Handle::current(),
            supervisor,
            agents: Mutex::new(HashMap::new()),
            bulk: bulk::Registry::default(),
        })
    }

    /// Output cursors are only meaningful within one daemon incarnation.
    fn incarnation(&self, run: &str) -> String {
        let digest = Sha256::digest(format!("{}:{run}", self.authority.incarnation()));
        hex(&digest[..12])
    }

    fn engine(&self) -> Result<Arc<Engine>, ErrorCode> {
        self.engine.upgrade().ok_or(ErrorCode::Unavailable)
    }

    /// The per-agent command boundary, acquired after the authority barrier and
    /// held through the ledger transition and its local effect.
    fn agent_boundary(&self, agent: &str) -> Arc<Mutex<()>> {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        agents.entry(agent.to_string()).or_default().clone()
    }

    /// Trusted supervisor resolution. Outcome proof comes only from the
    /// supervisor's verified record; otherwise the run stays Unknown and keeps
    /// its capacity until explicit reconciliation.
    pub async fn settle(self: &Arc<Self>, run: RunId) {
        let supervisor = self.supervisor.clone();
        let Ok(admin) = local_supervisor() else {
            return;
        };
        let _ = self
            .authority
            .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                let record = ledger
                    .run(&run)
                    .map_err(|_| mesh_sessions::Error::Operation)?;
                if !record.lifecycle.holds_capacity() {
                    return Ok(());
                }
                let Some(identity) = record.supervision.as_ref() else {
                    return Ok(());
                };
                let result = match supervisor.outcome(&run.0, identity) {
                    Proof::Terminated {
                        evidence,
                        interrupted,
                    } => ledger
                        .resolve_run(
                            &run,
                            &state::OutcomeProof::ProcessTreeTerminated {
                                evidence,
                                interrupted,
                            },
                        )
                        .map(|_| ()),
                    Proof::Unproven => ledger.mark_unknown(&run).map(|_| ()),
                };
                result.map_err(|_| mesh_sessions::Error::Operation)
            })
            .await;
    }

    /// After a restart every potentially live run is Unknown. Resolve those
    /// whose supervisor left verified proof; leave the rest reserved.
    pub async fn reconcile(self: &Arc<Self>) {
        let Ok(admin) = local_supervisor() else {
            return;
        };
        let runs = self
            .authority
            .inspect_admin(&self.work, admin, |_, ledger, _, _| {
                ledger
                    .unresolved_runs()
                    .map_err(|_| mesh_sessions::Error::Operation)
            })
            .await
            .unwrap_or_default();
        for run in runs {
            if run.supervision.is_some() && run.lifecycle == state::Lifecycle::Unknown {
                self.settle(run.run_id).await;
            }
        }
    }

    fn watch_exit(
        self: &Arc<Self>,
        run: RunId,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
    ) {
        let executor = self.clone();
        self.runtime.spawn(async move {
            while exit.borrow_and_update().is_none() {
                if exit.changed().await.is_err() {
                    break;
                }
            }
            executor.settle(run).await;
        });
    }

    fn mutate(
        self: &Arc<Self>,
        binding: &str,
        ledger: &Ledger,
        exports: &MeshExports,
        caller: &Caller,
        policy: &PeerPolicy,
        request: &Request,
    ) -> Outcome {
        let (peer, generation) = match caller.principal() {
            Principal::Peer { id, generation } => (id.clone(), *generation),
            Principal::Local { .. } => return Err(ErrorCode::Denied),
        };
        if request.target_daemon != ledger.owner_daemon()
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
        let allow = |export: &str, action: &Action| {
            policy
                .authorize(&peer, generation, export, action, now)
                .map_err(|_| ErrorCode::Denied)
        };
        match &request.operation {
            Operation::IssueWindow { ttl_seconds } => {
                let window = ledger
                    .issue_window(caller, now, i64::from(*ttl_seconds))
                    .map_err(code)?;
                Ok(Body::Window {
                    window: window.id.0,
                    expires_at: window.expires_at,
                })
            }
            Operation::CloseWindow { window } => {
                ledger
                    .close_window(caller, WindowId(*window))
                    .map_err(code)?;
                Ok(Body::Done {})
            }
            Operation::Spawn {
                mutation,
                export,
                runtime,
                source_commit,
                prompt,
            } => {
                allow(&export.id, &Action::Spawn { runtime })?;
                if !RUNTIME_IDS.contains(&runtime.as_str()) {
                    return Err(ErrorCode::Denied);
                }
                let resolved = exports
                    .resolve(&export.id, export.generation)
                    .map_err(|_| ErrorCode::Unavailable)?;
                self.spawn(
                    ledger,
                    caller,
                    &peer,
                    key(request, mutation)?,
                    export,
                    resolved.project_root(),
                    runtime,
                    source_commit,
                    prompt,
                    now,
                )
            }
            Operation::AcquireControl { guard, mutation } => {
                allow(&guard.target.export.id, &Action::AcquireControl)?;
                let (mutation_guard, _) = self.target(ledger, exports, guard)?;
                let _boundary = self.agent_boundary(&guard.target.agent_id);
                let _held = _boundary.lock().unwrap_or_else(|e| e.into_inner());
                let agent = ledger
                    .acquire_control(caller, &key(request, mutation)?, &mutation_guard, now)
                    .map_err(code)?
                    .value;
                Ok(receipt(&guard.target.export, &agent))
            }
            Operation::Stop { guard, mutation } => {
                allow(&guard.target.export.id, &Action::Stop)?;
                let (mutation_guard, _) = self.target(ledger, exports, guard)?;
                let boundary = self.agent_boundary(&guard.target.agent_id);
                let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
                let recorded = ledger
                    .request_stop(caller, &key(request, mutation)?, &mutation_guard, now)
                    .map_err(code)?;
                if recorded.fresh {
                    // Durable stop intent first, then the single termination
                    // request. Proof of termination arrives from the supervisor.
                    let engine = self.engine()?;
                    let agent = guard.target.agent_id.clone();
                    self.runtime
                        .block_on(async move { engine.mesh_terminate(&agent).await });
                }
                Ok(receipt(&guard.target.export, &recorded.value))
            }
            Operation::Resize {
                guard, cols, rows, ..
            } => {
                allow(&guard.target.export.id, &Action::Resize)?;
                if !(1..=1000).contains(cols) || !(1..=1000).contains(rows) {
                    return Err(ErrorCode::InvalidState);
                }
                let (mutation_guard, _) = self.target(ledger, exports, guard)?;
                let boundary = self.agent_boundary(&guard.target.agent_id);
                let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
                ledger
                    .validate_dispatch(caller, &mutation_guard)
                    .map_err(code)?;
                let engine = self.engine()?;
                let agent = guard.target.agent_id.clone();
                let (cols, rows) = (*cols, *rows);
                if !self
                    .runtime
                    .block_on(async move { engine.mesh_resize(&agent, cols, rows).await })
                {
                    return Err(ErrorCode::InvalidState);
                }
                Ok(Body::Done {})
            }
            Operation::OpenWriter { guard, previous } => {
                allow(&guard.target.export.id, &Action::Input)?;
                let (mutation_guard, _) = self.target(ledger, exports, guard)?;
                let boundary = self.agent_boundary(&guard.target.agent_id);
                let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
                let writer = ledger
                    .open_writer(
                        caller,
                        &mutation_guard,
                        previous.as_ref().map(|p| WriterCheckpoint {
                            writer: WriterId(p.writer),
                            last_sequence: p.last_sequence,
                        }),
                    )
                    .map_err(code)?;
                Ok(Body::Writer {
                    target: guard.target.clone(),
                    writer: writer.id.0,
                    epoch: writer.epoch.0,
                })
            }
            Operation::Input {
                guard,
                writer,
                sequence,
                data,
            } => {
                allow(&guard.target.export.id, &Action::Input)?;
                let (mutation_guard, _) = self.target(ledger, exports, guard)?;
                let boundary = self.agent_boundary(&guard.target.agent_id);
                let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
                let digest: [u8; 32] = Sha256::digest(data).into();
                let bytes = u32::try_from(data.len()).map_err(|_| ErrorCode::InvalidState)?;
                let recorded = ledger
                    .accept_input(
                        caller,
                        &mutation_guard,
                        WriterId(*writer),
                        *sequence,
                        PayloadDigest(digest),
                        bytes,
                    )
                    .map_err(code)?;
                let ack = if recorded.fresh {
                    // Exactly one bounded attempt; its precise outcome is final.
                    let engine = self.engine()?;
                    let agent = guard.target.agent_id.clone();
                    let delivery = match self
                        .runtime
                        .block_on(async move { engine.mesh_session(&agent).await })
                    {
                        Some(session) => session.write_once(data, INPUT_DEADLINE),
                        None => Delivery::NotWritten,
                    };
                    let outcome = match delivery {
                        Delivery::Written => state::InputOutcome::Written,
                        Delivery::Partial(bytes) => state::InputOutcome::Partial { bytes },
                        Delivery::NotWritten => state::InputOutcome::NotWritten,
                        Delivery::Unknown => state::InputOutcome::Unknown,
                    };
                    // If this fails the receipt stays Accepted and becomes
                    // Unknown at reopen; it is never redelivered.
                    ledger
                        .finish_input(WriterId(*writer), *sequence, outcome)
                        .map_err(|_| ErrorCode::Unavailable)?
                } else {
                    recorded.value
                };
                Ok(Body::InputReceipt {
                    evidence: Some(input_evidence(&guard.target, &ack)),
                })
            }
            Operation::SubscribeOutput { target, cursor } => {
                allow(&target.export.id, &Action::ReadOutput)?;
                self.owned_run(ledger, exports, target)?;
                let engine = self.engine()?;
                let agent = target.agent_id.clone();
                let session = self
                    .runtime
                    .block_on(async move { engine.mesh_session(&agent).await })
                    .ok_or(ErrorCode::InvalidState)?;
                let incarnation = self.incarnation(&target.run_id);
                let start = match cursor {
                    Some(cursor) if cursor.incarnation == incarnation => {
                        let offset =
                            usize::try_from(cursor.offset).map_err(|_| ErrorCode::InvalidState)?;
                        if offset > session.output.current_offset() {
                            return Err(ErrorCode::InvalidState);
                        }
                        offset
                    }
                    // A different incarnation's output no longer exists; the new
                    // cursor makes that discontinuity explicit to the host.
                    _ => 0,
                };
                self.bulk.send(
                    binding,
                    bulk::Command::Subscribe(bulk::Subscribe {
                        target: target.clone(),
                        incarnation: incarnation.clone(),
                        start,
                        output: session.output,
                        exit: session.exit_rx,
                    }),
                )?;
                Ok(Body::Subscribed {
                    target: target.clone(),
                    cursor: wire::OutputCursor {
                        incarnation,
                        offset: start as u64,
                    },
                })
            }
            Operation::UnsubscribeOutput { target } => {
                allow(&target.export.id, &Action::ReadOutput)?;
                self.bulk
                    .send(binding, bulk::Command::Unsubscribe(target.clone()))?;
                Ok(Body::Done {})
            }
            // Explicit v1 decisions: remote suspend/resume and native transcript
            // reads are not offered; spawn a new run or observe output instead.
            Operation::Suspend { .. }
            | Operation::Resume { .. }
            | Operation::ReadConversation { .. } => Err(ErrorCode::Unsupported),
            Operation::Inventory {}
            | Operation::Status { .. }
            | Operation::Receipt { .. }
            | Operation::InputReceipt { .. } => Err(ErrorCode::Unsupported),
        }
    }

    /// The run must belong to the agent, the export ID and its current generation.
    fn owned_run(
        &self,
        ledger: &Ledger,
        exports: &MeshExports,
        target: &AgentTarget,
    ) -> Result<state::RunRecord, ErrorCode> {
        exports
            .resolve(&target.export.id, target.export.generation)
            .map_err(|_| ErrorCode::Unavailable)?;
        let run = ledger
            .run(&RunId(target.run_id.clone()))
            .map_err(|_| ErrorCode::Denied)?;
        if run.agent_id.0 != target.agent_id
            || run.intent.export.as_deref() != Some(target.export.id.as_str())
        {
            return Err(ErrorCode::Denied);
        }
        Ok(run)
    }

    fn target(
        &self,
        ledger: &Ledger,
        exports: &MeshExports,
        guard: &ControlGuard,
    ) -> Result<(MutationGuard, state::RunRecord), ErrorCode> {
        let run = self.owned_run(ledger, exports, &guard.target)?;
        Ok((
            MutationGuard {
                agent_id: AgentId(guard.target.agent_id.clone()),
                run_id: RunId(guard.target.run_id.clone()),
                epoch: state::ControllerEpoch(guard.epoch),
                revision: state::SessionRevision(guard.revision),
            },
            run,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(
        self: &Arc<Self>,
        ledger: &Ledger,
        caller: &Caller,
        peer: &str,
        key: OperationKey,
        export: &wire::Export,
        root: &Path,
        runtime: &str,
        source_commit: &str,
        prompt: &str,
        now: i64,
    ) -> Outcome {
        // Missing or ambiguous source fails before any reservation; there is no
        // branch, commit or machine fallback.
        let root_owned = root.to_path_buf();
        let commit = source_commit.to_string();
        self.runtime
            .block_on(async move {
                crate::mesh_source::verify_existing_commit(&root_owned, &commit).await
            })
            .map_err(|_| ErrorCode::SourceMissing)?;
        // Receiver-derived and stable for duplicates of this operation.
        let worktree = worktree_id(peer, &key);
        let branch = format!("pu/mesh-{worktree}");
        let intent = state::LaunchIntent {
            project: root.to_string_lossy().to_string(),
            export: Some(export.id.clone()),
            runtime: runtime.to_string(),
            source_commit: source_commit.to_string(),
            worktree: state::WorktreeIntent {
                id: worktree.clone(),
                path: pu_core::paths::worktree_path(root, &worktree)
                    .to_string_lossy()
                    .to_string(),
                branch: branch.clone(),
            },
        };
        let recorded = ledger
            .admit_launch(caller, &key, &intent, None, now)
            .map_err(code)?;
        let launch = recorded.value;
        let target = AgentTarget {
            export: export.clone(),
            agent_id: launch.agent_id.0.clone(),
            run_id: launch.run_id.0.clone(),
        };
        let evidence = Body::Receipt {
            evidence: Some(ReceiptEvidence {
                target,
                epoch: launch.guard.epoch.0,
                revision: launch.guard.revision.0,
            }),
        };
        if !recorded.fresh {
            return Ok(evidence);
        }
        let boundary = self.agent_boundary(&launch.agent_id.0);
        let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
        ledger.begin_start(caller, &launch.guard).map_err(code)?;
        let prelaunch = |reason: String| {
            let _ = ledger.resolve_run(
                &launch.run_id,
                &state::OutcomeProof::PrelaunchFailure { evidence: reason },
            );
            ErrorCode::Unavailable
        };
        let (command, args, session_id, inject) =
            match Engine::mesh_runtime_command(root, runtime, prompt) {
                Ok(value) => value,
                Err(MeshSpawnError::Prelaunch(reason)) => return Err(prelaunch(reason)),
            };
        let prepared = match self.supervisor.prepare(&launch.run_id.0, command, args) {
            Ok(prepared) => prepared,
            Err(error) => return Err(prelaunch(format!("supervisor: {error}"))),
        };
        let engine = self.engine()?;
        let spec = MeshSpawn {
            agent_id: launch.agent_id.0.clone(),
            run_id: launch.run_id.0.clone(),
            project_root: root.to_path_buf(),
            runtime: runtime.to_string(),
            prompt: prompt.to_string(),
            source_commit: source_commit.to_string(),
            worktree_id: worktree,
            branch,
            command: prepared.command,
            args: prepared.args,
            env: prepared.env,
        };
        let spawned = match self
            .runtime
            .block_on(async move { engine.spawn_mesh_run(spec, inject, session_id).await })
        {
            Ok(spawned) => spawned,
            Err(MeshSpawnError::Prelaunch(reason)) => return Err(prelaunch(reason)),
        };
        // A process now exists; any later failure leaves the run reserved.
        if ledger
            .record_running(&launch.run_id, &prepared.identity)
            .is_err()
        {
            let _ = ledger.mark_unknown(&launch.run_id);
        }
        self.watch_exit(launch.run_id.clone(), spawned.exit_rx);
        Ok(evidence)
    }
}

impl BridgeHandler for Executor {
    fn dispatch(
        &self,
        binding: &str,
        ledger: &Ledger,
        exports: &MeshExports,
        caller: &Caller,
        policy: &PeerPolicy,
        request: &Request,
    ) -> mesh_sessions::Result<Vec<u8>> {
        if matches!(
            request.operation,
            Operation::Inventory {}
                | Operation::Status { .. }
                | Operation::Receipt { .. }
                | Operation::InputReceipt { .. }
        ) {
            return Ok(crate::mesh_read::dispatch(
                ledger, exports, caller, policy, request,
            )?);
        }
        request.validate()?;
        // `self` is always owned by an Arc held by the listener.
        let this = self.self_arc().ok_or(mesh_sessions::Error::Operation)?;
        let body = this
            .mutate(binding, ledger, exports, caller, policy, request)
            .unwrap_or_else(|code| Body::Error { code });
        Ok(response::Response {
            request_id: request.request_id.clone(),
            daemon_id: ledger.owner_daemon().into(),
            body,
        }
        .encode()?)
    }

    fn control_opened(&self, binding: &str) {
        self.bulk.open(binding);
    }

    fn control_closed(&self, binding: &str) {
        self.bulk.close(binding);
    }

    fn serve_bulk(
        self: Arc<Self>,
        session: mesh_sessions::Session<tokio::net::TcpStream>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(bulk::serve(self, session))
    }
}

impl Executor {
    fn self_arc(&self) -> Option<Arc<Self>> {
        self.bulk.owner.lock().ok()?.upgrade()
    }

    /// Record the owning Arc so trait methods taking `&self` can start tasks.
    pub fn bind_self(self: &Arc<Self>) {
        if let Ok(mut owner) = self.bulk.owner.lock() {
            *owner = Arc::downgrade(self);
        }
    }

    pub(crate) fn ledger_check_run(
        ledger: &Ledger,
        target: &AgentTarget,
    ) -> Result<(), mesh_sessions::Error> {
        let run = ledger
            .run(&RunId(target.run_id.clone()))
            .map_err(|_| mesh_sessions::Error::Denied)?;
        if run.agent_id.0 != target.agent_id
            || run.intent.export.as_deref() != Some(target.export.id.as_str())
        {
            return Err(mesh_sessions::Error::Denied);
        }
        Ok(())
    }
}

fn local_supervisor() -> Result<LocalAdmin, state::Error> {
    LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("supervisor")?)
}

fn key(request: &Request, mutation: &wire::Mutation) -> Result<OperationKey, ErrorCode> {
    Ok(OperationKey {
        window: WindowId(mutation.window),
        id: OperationId(mutation.id.clone()),
        digest: PayloadDigest(
            request
                .payload_digest()
                .map_err(|_| ErrorCode::InvalidState)?,
        ),
    })
}

fn worktree_id(peer: &str, key: &OperationKey) -> String {
    let digest = Sha256::digest(format!("{peer}:{}:{}", key.window.0, key.id.0));
    format!("wt-mesh-{}", hex(&digest[..8]))
}

fn receipt(export: &wire::Export, agent: &state::AgentRecord) -> Body {
    Body::Receipt {
        evidence: Some(ReceiptEvidence {
            target: AgentTarget {
                export: export.clone(),
                agent_id: agent.agent_id.0.clone(),
                run_id: agent.current_run.0.clone(),
            },
            epoch: agent.epoch.0,
            revision: agent.revision.0,
        }),
    }
}

fn input_evidence(target: &AgentTarget, ack: &state::InputAck) -> response::InputEvidence {
    response::InputEvidence {
        target: target.clone(),
        writer: ack.writer.0,
        sequence: ack.sequence,
        digest: ack.digest.0,
        payload_bytes: ack.payload_bytes,
        outcome: match ack.outcome {
            state::InputOutcome::Accepted => response::InputOutcome::Accepted,
            state::InputOutcome::Written => response::InputOutcome::Written,
            state::InputOutcome::Partial { bytes } => response::InputOutcome::Partial { bytes },
            state::InputOutcome::NotWritten => response::InputOutcome::NotWritten,
            state::InputOutcome::Unknown => response::InputOutcome::Unknown,
        },
    }
}

/// Precise, non-sensitive mapping; internal details never cross the bridge.
fn code(error: state::Error) -> ErrorCode {
    use state::Error as E;
    match error {
        E::Capacity { .. } => ErrorCode::Capacity,
        E::PayloadConflict => ErrorCode::Conflict,
        E::WindowClosed | E::ReceiptExpired => ErrorCode::ReceiptExpired,
        E::Stale(_) => ErrorCode::Stale,
        E::ControllerFenced | E::WriterFenced => ErrorCode::Fenced,
        E::SequenceGap { .. } => ErrorCode::SequenceGap,
        E::ReconciliationRequired => ErrorCode::ReconciliationRequired,
        E::InputPending => ErrorCode::InputPending,
        E::InvalidTransition | E::RunUnresolved | E::Invalid(_) => ErrorCode::InvalidState,
        E::Unauthorized | E::PeerFenced | E::NotFound(_) => ErrorCode::Denied,
        _ => ErrorCode::Unavailable,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
