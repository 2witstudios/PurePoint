use rusqlite::params;

use super::{db::*, operations::*, types::*, *};

impl Ledger {
    /// Atomically deduplicate, reserve all budgets, and store the prelaunch intent.
    /// `resume` must name the current proven-terminal run; it creates a new run
    /// under the same agent. A duplicate never reserves or returns fresh=true.
    /// Repeated attempts supply the same resolved intent. Derive receiver-created
    /// worktree identifiers from the operation identity or reuse the saved intent;
    /// do not mint different worktree intents on concurrent duplicate requests.
    pub fn admit_launch(
        &self,
        caller: &Caller,
        key: &OperationKey,
        intent: &LaunchIntent,
        resume: Option<&MutationGuard>,
        now: i64,
    ) -> Result<Recorded<LaunchReceipt>> {
        self.transaction_at(now, |tx, now| {
            intent.validate()?;
            let descriptor = encode(&("launch", intent, resume))?;
            if let Some(receipt) = check_operation(tx, caller, key, &descriptor, now, &self.limits)?
            {
                let OperationReceipt::Launch(value) = receipt else {
                    return Err(Error::PayloadConflict);
                };
                return Ok(Recorded {
                    value,
                    fresh: false,
                });
            }
            let mut agent = if let Some(guard) = resume {
                let agent = controlled(tx, caller, guard)?;
                let run: RunRecord =
                    get(tx, "runs", &agent.current_run.0)?.ok_or(Error::Corrupt("resume run"))?;
                if run.lifecycle.holds_capacity() || run.outcome.is_none() {
                    return Err(Error::RunUnresolved);
                }
                if run.intent.project != intent.project || run.intent.export != intent.export {
                    return Err(Error::Invalid("resume project/export"));
                }
                agent
            } else {
                budget(tx, "agents", self.limits.agents)?;
                AgentRecord {
                    agent_id: AgentId(pu_core::id::agent_id()),
                    owner_daemon: self.owner.clone(),
                    controller: Some(caller.principal().clone()),
                    epoch: ControllerEpoch(1),
                    revision: SessionRevision(1),
                    current_run: RunId(pu_core::id::session_id()),
                    lifecycle: Lifecycle::Accepted,
                    current_writer: None,
                }
            };
            budget(tx, "runs", self.limits.runs)?;
            let runs: Vec<_> = all::<RunRecord>(tx, "runs")?
                .into_iter()
                .filter(|run| run.lifecycle.holds_capacity())
                .collect();
            for (scope, count, limit) in [
                (CapacityScope::Global, runs.len(), self.limits.global_runs),
                (
                    CapacityScope::Project,
                    runs.iter()
                        .filter(|r| r.intent.project == intent.project)
                        .count(),
                    self.limits.project_runs,
                ),
                (
                    CapacityScope::Runtime,
                    runs.iter()
                        .filter(|r| r.intent.runtime == intent.runtime)
                        .count(),
                    self.limits.runtime_runs,
                ),
                (
                    CapacityScope::Principal,
                    runs.iter()
                        .filter(|r| r.principal.same_identity(caller.principal()))
                        .count(),
                    self.limits.principal_runs,
                ),
                (
                    CapacityScope::Starting,
                    runs.iter().filter(|r| r.startup_pending).count(),
                    self.limits.starting_runs,
                ),
            ] {
                if count >= limit {
                    return Err(Error::Capacity { scope, limit });
                }
            }
            if resume.is_some() {
                agent.current_run = RunId(pu_core::id::session_id());
                agent.lifecycle = Lifecycle::Accepted;
                agent.epoch.0 = increment(agent.epoch.0)?;
                agent.revision.0 = increment(agent.revision.0)?;
                // Resolution fenced the old writer. Its receipts stay independently readable.
                agent.current_writer = None;
                put(tx, "agents", &agent.agent_id.0, &agent)?;
            } else {
                tx.execute(
                    "INSERT INTO agents VALUES (?1, ?2)",
                    params![agent.agent_id.0, encode(&agent)?],
                )?;
            }
            let run = RunRecord {
                run_id: agent.current_run.clone(),
                agent_id: agent.agent_id.clone(),
                principal: caller.principal().clone(),
                intent: intent.clone(),
                lifecycle: Lifecycle::Accepted,
                startup_pending: true,
                supervision: None,
                outcome: None,
            };
            tx.execute(
                "INSERT INTO runs VALUES (?1, ?2, ?3)",
                params![run.run_id.0, run.agent_id.0, encode(&run)?],
            )?;
            let value = LaunchReceipt {
                agent_id: agent.agent_id.clone(),
                run_id: run.run_id.clone(),
                guard: agent.guard(),
            };
            save_operation(
                tx,
                key,
                descriptor,
                run.run_id,
                OperationReceipt::Launch(value.clone()),
            )?;
            Ok(Recorded { value, fresh: true })
        })
    }

    /// Exactly one Accepted -> Starting transition, durably before process creation.
    /// Hold the authority and agent barriers across this call and process dispatch.
    /// Starting/Unknown cannot use this API to replay a possibly attempted launch.
    pub fn begin_start(&self, caller: &Caller, guard: &MutationGuard) -> Result<AgentRecord> {
        self.transaction(|tx| {
            let agent = controlled(tx, caller, guard)?;
            let mut run: RunRecord =
                get(tx, "runs", &agent.current_run.0)?.ok_or(Error::Corrupt("start run"))?;
            if run.lifecycle != Lifecycle::Accepted {
                return Err(Error::InvalidTransition);
            }
            run.lifecycle = Lifecycle::Starting;
            change_lifecycle(tx, &run)
        })
    }

    /// Supervisor-only observation, never permission to create a process. The
    /// engine must establish ownership of the exact run before recording Running.
    pub fn record_running(
        &self,
        id: &RunId,
        supervision: &SupervisionIdentity,
    ) -> Result<AgentRecord> {
        bounded(&id.0, 128, "run ID")?;
        supervision.validate()?;
        self.transaction(|tx| {
            let mut run: RunRecord = get(tx, "runs", &id.0)?.ok_or(Error::NotFound("run"))?;
            if !matches!(run.lifecycle, Lifecycle::Starting | Lifecycle::Unknown) {
                return Err(Error::InvalidTransition);
            }
            if run
                .supervision
                .as_ref()
                .is_some_and(|old| old != supervision)
            {
                return Err(Error::InvalidTransition);
            }
            run.lifecycle = Lifecycle::Running;
            run.startup_pending = false;
            run.supervision = Some(supervision.clone());
            change_lifecycle(tx, &run)
        })
    }

    /// Supervisor-only uncertainty, retaining every reservation.
    pub fn mark_unknown(&self, id: &RunId) -> Result<AgentRecord> {
        bounded(&id.0, 128, "run ID")?;
        self.transaction(|tx| {
            let mut run: RunRecord = get(tx, "runs", &id.0)?.ok_or(Error::NotFound("run"))?;
            if !run.lifecycle.holds_capacity() {
                return Err(Error::InvalidTransition);
            }
            run.lifecycle = Lifecycle::Unknown;
            fence_writers(tx, &run.agent_id)?;
            change_lifecycle(tx, &run)
        })
    }

    /// Releases once, only from explicit engine proof. Identical resolution is
    /// idempotent; conflicting proofs reject. PrelaunchFailure asserts that no
    /// process creation could have occurred, not merely a missing process handle.
    pub fn resolve_run(&self, id: &RunId, proof: &OutcomeProof) -> Result<RunRecord> {
        bounded(&id.0, 128, "run ID")?;
        proof.validate()?;
        self.transaction(|tx| {
            let mut run: RunRecord = get(tx, "runs", &id.0)?.ok_or(Error::NotFound("run"))?;
            if let Some(previous) = &run.outcome {
                if previous == proof {
                    return Ok(run);
                }
                return Err(Error::InvalidTransition);
            }
            if !run.lifecycle.holds_capacity() {
                return Err(Error::InvalidTransition);
            }
            run.lifecycle = match proof {
                OutcomeProof::PrelaunchFailure { .. }
                    if run.startup_pending && run.supervision.is_none() =>
                {
                    Lifecycle::Failed
                }
                OutcomeProof::PrelaunchFailure { .. } => return Err(Error::InvalidTransition),
                OutcomeProof::ProcessTreeTerminated {
                    interrupted: true, ..
                } => Lifecycle::Interrupted,
                OutcomeProof::ProcessTreeTerminated {
                    interrupted: false, ..
                } => Lifecycle::Terminated,
                OutcomeProof::OwnerAttested { .. } => Lifecycle::Interrupted,
            };
            run.startup_pending = false;
            run.outcome = Some(proof.clone());
            let agent = change_lifecycle(tx, &run)?;
            fence_writers(tx, &agent.agent_id)?;
            Ok(run)
        })
    }
}

pub(super) fn fence_writers(tx: &rusqlite::Transaction<'_>, agent: &AgentId) -> Result<()> {
    for mut writer in all::<WriterRecord>(tx, "writers")? {
        if writer.agent_id != *agent || !writer.live {
            continue;
        }
        writer.live = false;
        put(tx, "writers", writer.id.0, &writer)?;
        for mut ack in all::<InputAck>(tx, "inputs")? {
            if ack.writer == writer.id && ack.outcome == InputOutcome::Accepted {
                ack.outcome = InputOutcome::Unknown;
                tx.execute(
                    "UPDATE inputs SET data=?3 WHERE writer=?1 AND sequence=?2",
                    params![ack.writer.0, ack.sequence, encode(&ack)?],
                )?;
            }
        }
    }
    Ok(())
}
