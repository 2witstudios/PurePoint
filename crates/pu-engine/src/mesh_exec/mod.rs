//! Worker-side bridge execution. Every operation runs inside `Session::dispatch`
//! while the shared authority barrier is held. Mutations of an existing run then
//! take its per-agent command boundary, perform their ledger transition and only
//! then their local effect:
//! authority barrier -> per-agent boundary -> ledger transaction -> effect.
//! A fresh durable acceptance is the only permission for a process launch or a
//! PTY write; duplicates return retained evidence and never act again. A spawn
//! is admitted first (the ledger mints its agent) and then takes the new agent's
//! boundary before any process can exist.

mod bulk;
pub mod local;
pub mod supervisor;

pub use supervisor::{Cause, Prepared, Proof, Supervisor};

use crate::{
    daemon_lifecycle::{mesh_exports::MeshExports, owned_work::WorkOwner},
    engine::{
        Engine,
        mesh_runs::{Delivery, INPUT_DEADLINE, MeshSpawn, MeshSpawnError, RuntimeCommand},
    },
    execution_state::{
        self as state, AgentId, Caller, Ledger, LocalAdmin, MutationGuard, OperationId,
        OperationKey, OperationReceipt, PayloadDigest, Principal, RunId, WindowId,
        WriterCheckpoint, WriterId,
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
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

/// Receiver-approved runtimes; shared with administration grant validation.
pub use crate::mesh_runtime::RUNTIME_IDS;

/// Restart reconciliation keeps retrying runs whose supervisor may still be
/// finishing; it backs off to this interval.
const RECONCILE_MAX_INTERVAL: Duration = Duration::from_secs(30);

/// The per-agent command boundary. Async so local writers can hold it across a
/// PTY write; blocking dispatch threads take it with `blocking_lock`.
pub type AgentBoundary = Arc<AsyncMutex<()>>;

pub struct Executor {
    engine: Weak<Engine>,
    authority: Arc<SessionAuthority>,
    work: Arc<WorkOwner>,
    runtime: tokio::runtime::Handle,
    supervisor: Arc<dyn Supervisor>,
    agents: Mutex<HashMap<String, AgentBoundary>>,
    bulk: bulk::Registry,
}

type Outcome = Result<Body, ErrorCode>;

/// One authenticated, authorized-at-the-session-layer request being dispatched
/// under the shared authority barrier.
struct Call<'a> {
    binding: &'a str,
    ledger: &'a Ledger,
    exports: &'a MeshExports,
    caller: &'a Caller,
    policy: &'a PeerPolicy,
    request: &'a Request,
    peer: String,
    generation: u64,
    now: i64,
}

impl Call<'_> {
    /// Receiver grants for this export and action, evaluated now.
    fn allow(&self, export: &str, action: &Action) -> Result<(), ErrorCode> {
        self.policy
            .authorize(&self.peer, self.generation, export, action, self.now)
            .map(|_| ())
            .map_err(|_| ErrorCode::Denied)
    }

    fn key(&self, mutation: &wire::Mutation) -> Result<OperationKey, ErrorCode> {
        Ok(OperationKey {
            window: WindowId(mutation.window),
            id: OperationId(mutation.id.clone()),
            digest: PayloadDigest(
                self.request
                    .payload_digest()
                    .map_err(|_| ErrorCode::InvalidState)?,
            ),
        })
    }

    /// The run must belong to the agent, the export ID and its current generation.
    fn owned_run(&self, target: &AgentTarget) -> Result<state::RunRecord, ErrorCode> {
        self.exports
            .resolve(&target.export.id, target.export.generation)
            .map_err(|_| ErrorCode::Unavailable)?;
        let run = self
            .ledger
            .run(&RunId(target.run_id.clone()))
            .map_err(|_| ErrorCode::Denied)?;
        if !run_matches(&run, target) {
            return Err(ErrorCode::Denied);
        }
        Ok(run)
    }
}

/// Run identity check shared by dispatch and every output frame.
fn run_matches(run: &state::RunRecord, target: &AgentTarget) -> bool {
    run.agent_id.0 == target.agent_id
        && run.intent.export.as_deref() == Some(target.export.id.as_str())
}

fn mutation_guard(guard: &ControlGuard) -> MutationGuard {
    MutationGuard {
        agent_id: AgentId(guard.target.agent_id.clone()),
        run_id: RunId(guard.target.run_id.clone()),
        epoch: state::ControllerEpoch(guard.epoch),
        revision: state::SessionRevision(guard.revision),
    }
}

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
    pub fn agent_boundary(&self, agent: &str) -> AgentBoundary {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        agents.entry(agent.to_string()).or_default().clone()
    }

    /// Trusted supervisor resolution. Outcome proof comes only from the
    /// supervisor's verified record; otherwise the run stays Unknown and keeps
    /// its capacity until explicit reconciliation. Returns true once resolved.
    pub async fn settle(self: &Arc<Self>, run: RunId) -> bool {
        let supervisor = self.supervisor.clone();
        let Ok(admin) = local_supervisor() else {
            return false;
        };
        let run_id = run.clone();
        let settled = self
            .authority
            .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                let record = ledger
                    .run(&run_id)
                    .map_err(|_| mesh_sessions::Error::Operation)?;
                if !record.lifecycle.holds_capacity() {
                    return Ok(Some(record.agent_id));
                }
                let Some(identity) = record.supervision.as_ref() else {
                    return Ok(None);
                };
                match supervisor.outcome(&run_id.0, identity) {
                    Proof::Terminated { evidence, cause } => {
                        // A requested stop that the tree honoured is a normal
                        // termination; anything else interrupted the run.
                        let interrupted = match cause {
                            Cause::AgentExit => false,
                            Cause::Signal => record.lifecycle != state::Lifecycle::Stopping,
                            Cause::DaemonExit => true,
                        };
                        ledger
                            .resolve_run(
                                &run_id,
                                &state::OutcomeProof::ProcessTreeTerminated {
                                    evidence,
                                    interrupted,
                                },
                            )
                            .map_err(|_| mesh_sessions::Error::Operation)?;
                        Ok(Some(record.agent_id))
                    }
                    Proof::Unproven => {
                        ledger
                            .mark_unknown(&run_id)
                            .map_err(|_| mesh_sessions::Error::Operation)?;
                        Ok(None)
                    }
                }
            })
            .await
            .ok()
            .flatten();
        match settled {
            Some(agent) => {
                self.supervisor.discard(&run.0);
                self.forget_boundary(&agent.0);
                true
            }
            None => false,
        }
    }

    /// Drop an idle boundary of a resolved run so the map stays bounded.
    fn forget_boundary(&self, agent: &str) {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        if agents
            .get(agent)
            .is_some_and(|boundary| Arc::strong_count(boundary) == 1)
        {
            agents.remove(agent);
        }
    }

    /// After a restart every potentially live run is Unknown. A previous
    /// incarnation's guardians may still be terminating their trees, so keep
    /// retrying with backoff until each run has verified proof; runs that never
    /// get proof stay reserved for explicit owner disposition.
    pub async fn reconcile(self: &Arc<Self>) {
        self.fence_known_agents().await;
        if self.reconcile_once().await {
            return;
        }
        let executor = self.clone();
        self.runtime.spawn(async move {
            let mut interval = Duration::from_millis(500);
            loop {
                tokio::time::sleep(interval).await;
                if executor.work.is_stopping() || executor.reconcile_once().await {
                    return;
                }
                interval = (interval * 2).min(RECONCILE_MAX_INTERVAL);
            }
        });
    }

    /// One pass; true when no supervised Unknown run remains unresolved.
    async fn reconcile_once(self: &Arc<Self>) -> bool {
        let Ok(admin) = local_supervisor() else {
            return true;
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
        let mut pending = false;
        for run in runs {
            if run.supervision.is_some()
                && run.lifecycle == state::Lifecycle::Unknown
                && !self.settle(run.run_id).await
            {
                pending = true;
            }
        }
        !pending
    }

    /// Every agent the ledger knows is worker-owned: local lifecycle paths must
    /// never resume or kill it, including after this daemon restarted.
    async fn fence_known_agents(self: &Arc<Self>) {
        let (Some(engine), Ok(admin)) = (self.engine.upgrade(), local_supervisor()) else {
            return;
        };
        let agents = self
            .authority
            .inspect_admin(&self.work, admin, |_, ledger, _, _| {
                ledger
                    .agent_ids()
                    .map_err(|_| mesh_sessions::Error::Operation)
            })
            .await
            .unwrap_or_default();
        engine.register_mesh_agents(agents.into_iter().map(|id| id.0));
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

    fn dispatch_operation(self: &Arc<Self>, call: &Call<'_>) -> Outcome {
        match &call.request.operation {
            Operation::Inventory {}
            | Operation::Status { .. }
            | Operation::Receipt { .. }
            | Operation::InputReceipt { .. } => crate::mesh_read::read(
                call.ledger,
                call.exports,
                call.caller,
                call.policy,
                call.request,
            ),
            Operation::IssueWindow { ttl_seconds } => self.issue_window(call, *ttl_seconds),
            Operation::CloseWindow { window } => {
                call.ledger
                    .close_window(call.caller, WindowId(*window))
                    .map_err(code)?;
                Ok(Body::Done {})
            }
            Operation::Spawn {
                mutation,
                export,
                runtime,
                source_commit,
                prompt,
            } => self.spawn(
                call,
                &SpawnRequest {
                    key: call.key(mutation)?,
                    export,
                    runtime,
                    source_commit,
                    prompt,
                },
            ),
            Operation::AcquireControl { guard, mutation } => {
                self.acquire(call, guard, &call.key(mutation)?)
            }
            Operation::Stop { guard, mutation } => self.stop(call, guard, &call.key(mutation)?),
            Operation::Resize {
                guard, cols, rows, ..
            } => self.resize(call, guard, *cols, *rows),
            Operation::OpenWriter { guard, previous } => {
                self.open_writer(call, guard, previous.as_ref())
            }
            Operation::Input {
                guard,
                writer,
                sequence,
                data,
            } => self.input(call, guard, WriterId(*writer), *sequence, data),
            Operation::SubscribeOutput { target, cursor } => {
                self.subscribe(call, target, cursor.as_ref())
            }
            Operation::UnsubscribeOutput { target } => {
                call.allow(&target.export.id, &Action::ReadOutput)?;
                self.bulk
                    .send(call.binding, bulk::Command::Unsubscribe(target.clone()))?;
                Ok(Body::Done {})
            }
            // Explicit v1 decisions: remote suspend/resume and native transcript
            // reads are not offered; spawn a new run or observe output instead.
            Operation::Suspend { .. }
            | Operation::Resume { .. }
            | Operation::ReadConversation { .. } => Err(ErrorCode::Unsupported),
        }
    }

    /// Windows are only for principals granted at least one capability.
    fn issue_window(&self, call: &Call<'_>, ttl_seconds: u32) -> Outcome {
        if call
            .policy
            .exports
            .iter()
            .all(|grant| grant.capabilities.is_empty())
        {
            return Err(ErrorCode::Denied);
        }
        let window = call
            .ledger
            .issue_window(call.caller, call.now, i64::from(ttl_seconds))
            .map_err(code)?;
        Ok(Body::Window {
            window: window.id.0,
            expires_at: window.expires_at,
        })
    }

    /// Authorize `action` on the guard's run, then run `effect` with the
    /// ledger guard while holding that agent's command boundary.
    fn guarded(
        &self,
        call: &Call<'_>,
        guard: &ControlGuard,
        action: Action<'_>,
        effect: impl FnOnce(&MutationGuard, &state::RunRecord) -> Outcome,
    ) -> Outcome {
        call.allow(&guard.target.export.id, &action)?;
        let run = call.owned_run(&guard.target)?;
        let boundary = self.agent_boundary(&guard.target.agent_id);
        let _held = boundary.blocking_lock();
        effect(&mutation_guard(guard), &run)
    }

    /// Only the principal that spawned a run may take control of it; grants on
    /// a shared export never let one peer seize another peer's run.
    fn acquire(&self, call: &Call<'_>, guard: &ControlGuard, key: &OperationKey) -> Outcome {
        self.guarded(call, guard, Action::AcquireControl, |mutation, run| {
            if !same_principal(&run.principal, call.caller.principal()) {
                return Err(ErrorCode::Fenced);
            }
            let agent = call
                .ledger
                .acquire_control(call.caller, key, mutation, call.now)
                .map_err(code)?
                .value;
            Ok(receipt(&guard.target.export, &agent))
        })
    }

    fn stop(&self, call: &Call<'_>, guard: &ControlGuard, key: &OperationKey) -> Outcome {
        self.guarded(call, guard, Action::Stop, |mutation, _| {
            let recorded = call
                .ledger
                .request_stop(call.caller, key, mutation, call.now)
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
        })
    }

    fn resize(&self, call: &Call<'_>, guard: &ControlGuard, cols: u16, rows: u16) -> Outcome {
        if !(1..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
            return Err(ErrorCode::InvalidState);
        }
        self.guarded(call, guard, Action::Resize, |mutation, _| {
            call.ledger
                .validate_dispatch(call.caller, mutation)
                .map_err(code)?;
            let engine = self.engine()?;
            let agent = guard.target.agent_id.clone();
            if !self
                .runtime
                .block_on(async move { engine.mesh_resize(&agent, cols, rows).await })
            {
                return Err(ErrorCode::InvalidState);
            }
            Ok(Body::Done {})
        })
    }

    fn open_writer(
        &self,
        call: &Call<'_>,
        guard: &ControlGuard,
        previous: Option<&wire::WriterCheckpoint>,
    ) -> Outcome {
        self.guarded(call, guard, Action::Input, |mutation, _| {
            let writer = call
                .ledger
                .open_writer(
                    call.caller,
                    mutation,
                    previous.map(|p| WriterCheckpoint {
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
        })
    }

    fn input(
        &self,
        call: &Call<'_>,
        guard: &ControlGuard,
        writer: WriterId,
        sequence: u64,
        data: &[u8],
    ) -> Outcome {
        self.guarded(call, guard, Action::Input, |mutation, _| {
            let digest: [u8; 32] = Sha256::digest(data).into();
            let bytes = u32::try_from(data.len()).map_err(|_| ErrorCode::InvalidState)?;
            let recorded = call
                .ledger
                .accept_input(
                    call.caller,
                    mutation,
                    writer,
                    sequence,
                    PayloadDigest(digest),
                    bytes,
                )
                .map_err(code)?;
            let ack = if recorded.fresh {
                // Exactly one bounded attempt; its precise outcome is final. If
                // recording it fails the receipt stays Accepted and becomes
                // Unknown at reopen; it is never redelivered.
                let outcome = self.deliver(&guard.target.agent_id, data)?;
                call.ledger
                    .finish_input(writer, sequence, outcome)
                    .map_err(|_| ErrorCode::Unavailable)?
            } else {
                recorded.value
            };
            Ok(Body::InputReceipt {
                evidence: Some(input_evidence(&guard.target, &ack)),
            })
        })
    }

    fn deliver(&self, agent: &str, data: &[u8]) -> Result<state::InputOutcome, ErrorCode> {
        let engine = self.engine()?;
        let agent = agent.to_string();
        let delivery = match self
            .runtime
            .block_on(async move { engine.mesh_session(&agent).await })
        {
            Some(session) => session.write_once(data, INPUT_DEADLINE),
            None => Delivery::NotWritten,
        };
        Ok(match delivery {
            Delivery::Written => state::InputOutcome::Written,
            Delivery::Partial(bytes) => state::InputOutcome::Partial { bytes },
            Delivery::NotWritten => state::InputOutcome::NotWritten,
            Delivery::Unknown => state::InputOutcome::Unknown,
        })
    }

    fn subscribe(
        &self,
        call: &Call<'_>,
        target: &AgentTarget,
        cursor: Option<&wire::OutputCursor>,
    ) -> Outcome {
        call.allow(&target.export.id, &Action::ReadOutput)?;
        call.owned_run(target)?;
        let engine = self.engine()?;
        let agent = target.agent_id.clone();
        let session = self
            .runtime
            .block_on(async move { engine.mesh_session(&agent).await })
            .ok_or(ErrorCode::InvalidState)?;
        let incarnation = self.incarnation(&target.run_id);
        let start = match cursor {
            Some(cursor) if cursor.incarnation == incarnation => {
                let offset = usize::try_from(cursor.offset).map_err(|_| ErrorCode::InvalidState)?;
                if offset > session.output.current_offset() {
                    return Err(ErrorCode::InvalidState);
                }
                offset
            }
            // A different incarnation's output no longer exists; the new cursor
            // makes that discontinuity explicit to the host.
            _ => 0,
        };
        self.bulk.send(
            call.binding,
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

    fn spawn(self: &Arc<Self>, call: &Call<'_>, spawn: &SpawnRequest<'_>) -> Outcome {
        call.allow(
            &spawn.export.id,
            &Action::Spawn {
                runtime: spawn.runtime,
            },
        )?;
        if !RUNTIME_IDS.contains(&spawn.runtime) {
            return Err(ErrorCode::Denied);
        }
        let resolved = call
            .exports
            .resolve(&spawn.export.id, spawn.export.generation)
            .map_err(|_| ErrorCode::Unavailable)?;
        let root = resolved.project_root();
        // A retried operation returns its retained receipt before any work: no
        // git process, and a since-removed commit cannot change the answer.
        if let Some(receipt) = call
            .ledger
            .operation_receipt(call.caller, &spawn.key)
            .map_err(code)?
        {
            let OperationReceipt::Launch(launch) = receipt else {
                return Err(ErrorCode::Conflict);
            };
            return Ok(launch_receipt(spawn.export, &launch));
        }
        // Missing or ambiguous source fails before any reservation; there is no
        // branch, commit or machine fallback.
        let root_owned = root.to_path_buf();
        let commit = spawn.source_commit.to_string();
        self.runtime
            .block_on(async move {
                crate::mesh_source::verify_existing_commit(&root_owned, &commit).await
            })
            .map_err(|_| ErrorCode::SourceMissing)?;
        // The engine must exist before a run is admitted that it would launch.
        let engine = self.engine()?;
        let recorded = call
            .ledger
            .admit_launch(
                call.caller,
                &spawn.key,
                &launch_intent(root, &call.peer, spawn),
                None,
                call.now,
            )
            .map_err(code)?;
        let launch = recorded.value;
        let evidence = launch_receipt(spawn.export, &launch);
        if !recorded.fresh {
            return Ok(evidence);
        }
        let boundary = self.agent_boundary(&launch.agent_id.0);
        let _held = boundary.blocking_lock();
        call.ledger
            .begin_start(call.caller, &launch.guard)
            .map_err(code)?;
        let prelaunch = |reason: String| {
            tracing::warn!(run = %launch.run_id.0, %reason, "mesh run failed before launch");
            let _ = call.ledger.resolve_run(
                &launch.run_id,
                &state::OutcomeProof::PrelaunchFailure { evidence: reason },
            );
            ErrorCode::Unavailable
        };
        let RuntimeCommand {
            command,
            args,
            session_id,
            inject_prompt,
        } = match Engine::mesh_runtime_command(root, spawn.runtime, spawn.prompt) {
            Ok(value) => value,
            Err(MeshSpawnError::Prelaunch(reason)) => return Err(prelaunch(reason)),
        };
        let prepared = match self.supervisor.prepare(&launch.run_id.0, command, args) {
            Ok(prepared) => prepared,
            Err(error) => return Err(prelaunch(format!("supervisor: {error}"))),
        };
        let worktree = worktree_id(&call.peer, &spawn.key);
        let spec = MeshSpawn {
            agent_id: launch.agent_id.0.clone(),
            run_id: launch.run_id.0.clone(),
            project_root: root.to_path_buf(),
            runtime: spawn.runtime.to_string(),
            prompt: spawn.prompt.to_string(),
            source_commit: spawn.source_commit.to_string(),
            branch: format!("pu/mesh-{worktree}"),
            worktree_id: worktree,
            command: prepared.command,
            args: prepared.args,
            env: prepared.env,
        };
        let spawned = match self
            .runtime
            .block_on(async move { engine.spawn_mesh_run(spec, inject_prompt, session_id).await })
        {
            Ok(spawned) => spawned,
            Err(MeshSpawnError::Prelaunch(reason)) => return Err(prelaunch(reason)),
        };
        // A process now exists; any later failure leaves the run reserved.
        if call
            .ledger
            .record_running(&launch.run_id, &prepared.identity)
            .is_err()
        {
            let _ = call.ledger.mark_unknown(&launch.run_id);
        }
        self.watch_exit(launch.run_id.clone(), spawned.exit_rx);
        Ok(evidence)
    }
}

/// A validated spawn request's receiver-relevant fields.
struct SpawnRequest<'a> {
    key: OperationKey,
    export: &'a wire::Export,
    runtime: &'a str,
    source_commit: &'a str,
    prompt: &'a str,
}

/// Receiver-derived and stable for duplicates of one operation.
fn launch_intent(root: &Path, peer: &str, spawn: &SpawnRequest<'_>) -> state::LaunchIntent {
    let worktree = worktree_id(peer, &spawn.key);
    state::LaunchIntent {
        project: root.to_string_lossy().to_string(),
        export: Some(spawn.export.id.clone()),
        runtime: spawn.runtime.to_string(),
        source_commit: spawn.source_commit.to_string(),
        worktree: state::WorktreeIntent {
            path: pu_core::paths::worktree_path(root, &worktree)
                .to_string_lossy()
                .to_string(),
            branch: format!("pu/mesh-{worktree}"),
            id: worktree,
        },
    }
}

fn launch_receipt(export: &wire::Export, launch: &state::LaunchReceipt) -> Body {
    Body::Receipt {
        evidence: Some(ReceiptEvidence {
            target: AgentTarget {
                export: export.clone(),
                agent_id: launch.agent_id.0.clone(),
                run_id: launch.run_id.0.clone(),
            },
            epoch: launch.guard.epoch.0,
            revision: launch.guard.revision.0,
        }),
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
        request.validate()?;
        let body = self
            .call(binding, ledger, exports, caller, policy, request)
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
    /// Common checks for every bridge request, then the operation itself.
    fn call(
        &self,
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
        // `self` is always owned by the Arc the listener holds.
        let this = self.self_arc().ok_or(ErrorCode::Unavailable)?;
        this.dispatch_operation(&Call {
            binding,
            ledger,
            exports,
            caller,
            policy,
            request,
            peer,
            generation,
            now,
        })
    }

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
        if !run_matches(&run, target) {
            return Err(mesh_sessions::Error::Denied);
        }
        Ok(())
    }
}

fn same_principal(left: &Principal, right: &Principal) -> bool {
    match (left, right) {
        (Principal::Peer { id: a, .. }, Principal::Peer { id: b, .. })
        | (Principal::Local { id: a }, Principal::Local { id: b }) => a == b,
        _ => false,
    }
}

fn local_supervisor() -> Result<LocalAdmin, state::Error> {
    LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("supervisor")?)
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
        other => {
            tracing::warn!(error = %other, "mesh ledger operation failed");
            ErrorCode::Unavailable
        }
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
