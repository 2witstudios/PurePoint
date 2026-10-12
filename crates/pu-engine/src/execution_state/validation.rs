//! Fail closed on malformed or inconsistent persisted records before recovery writes.
use std::collections::{HashMap, HashSet};

use rusqlite::Transaction;

use super::{db::*, operations::StoredOperation, types::*, *};

pub(super) fn validate_records(tx: &Transaction<'_>, owner: &str) -> Result<()> {
    // Check storage keys against typed identities, not just JSON syntax. Limits are
    // checked before decoding to avoid allocating an adversarial oversized record.
    for (table, mismatch) in [
        ("peers", "id IS NOT json_extract(data, '$.id')"),
        ("windows", "id IS NOT json_extract(data, '$.id')"),
        ("agents", "id IS NOT json_extract(data, '$.agent_id')"),
        (
            "runs",
            "id IS NOT json_extract(data, '$.run_id') OR agent IS NOT json_extract(data, '$.agent_id')",
        ),
        (
            "operations",
            "window IS NOT json_extract(data, '$.key.window') OR id IS NOT json_extract(data, '$.key.id')",
        ),
        (
            "writers",
            "id IS NOT json_extract(data, '$.id') OR agent IS NOT json_extract(data, '$.agent_id') OR run IS NOT json_extract(data, '$.run_id')",
        ),
        (
            "inputs",
            "writer IS NOT json_extract(data, '$.writer') OR sequence IS NOT json_extract(data, '$.sequence')",
        ),
    ] {
        let count: usize =
            tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        if count > 100_000 {
            return Err(Error::Corrupt("hard record budget"));
        }
        if tx.prepare(&format!("SELECT 1 FROM {table} WHERE length(CAST(data AS BLOB)) > {MAX_RECORD_BYTES} OR NOT json_valid(data)"))?.exists([])? {
            return Err(Error::Corrupt("record encoding or size"));
        }
        if tx
            .prepare(&format!("SELECT 1 FROM {table} WHERE {mismatch}"))?
            .exists([])?
        {
            return Err(Error::Corrupt("record key mismatch"));
        }
    }
    let (next_window, next_writer, clock): (u64, u64, i64) = tx.query_row(
        "SELECT next_window, next_writer, last_time FROM meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    valid(positive(next_window, "window counter"))?;
    valid(positive(next_writer, "writer counter"))?;
    ensure(clock >= 0, "clock")?;

    let peers: HashMap<_, _> = all::<PeerRecord>(tx, "peers")?
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    for peer in peers.values() {
        valid(bounded(&peer.id, 128, "peer ID"))?;
        valid(positive(peer.generation, "peer generation"))?;
        ensure(
            peer.authorization_revision < peer.generation,
            "authorization revision",
        )?;
        ensure(
            peer.authorization_digest.is_some() == (peer.authorization_revision > 0),
            "authorization anchor",
        )?;
    }
    let windows: HashMap<_, _> = all::<OperationWindow>(tx, "windows")?
        .into_iter()
        .map(|w| (w.id, w))
        .collect();
    for window in windows.values() {
        valid(positive(window.id.0, "window"))?;
        valid(window.principal.validate())?;
        ensure(
            window.id.0 < next_window && window.expires_at > 0,
            "window bounds",
        )?;
    }

    let agents: HashMap<_, _> = all::<AgentRecord>(tx, "agents")?
        .into_iter()
        .map(|a| (a.agent_id.clone(), a))
        .collect();
    let runs: HashMap<_, _> = all::<RunRecord>(tx, "runs")?
        .into_iter()
        .map(|r| (r.run_id.clone(), r))
        .collect();
    let writers: HashMap<_, _> = all::<WriterRecord>(tx, "writers")?
        .into_iter()
        .map(|w| (w.id, w))
        .collect();
    for agent in agents.values() {
        validate_agent_shape(agent, owner)?;
        if let Some(Principal::Peer { id, generation }) = &agent.controller {
            let peer = peers.get(id).ok_or(Error::Corrupt("controller peer"))?;
            ensure(
                !peer.revoked
                    && peer.generation == *generation
                    && peer.authorization_digest.is_some(),
                "controller generation",
            )?;
        }
        let run = runs
            .get(&agent.current_run)
            .ok_or(Error::Corrupt("current run"))?;
        ensure(
            run.agent_id == agent.agent_id && run.lifecycle == agent.lifecycle,
            "agent/run mismatch",
        )?;
        if let Some(id) = agent.current_writer {
            let writer = writers.get(&id).ok_or(Error::Corrupt("current writer"))?;
            ensure(
                writer.agent_id == agent.agent_id && writer.run_id == agent.current_run,
                "agent/writer mismatch",
            )?;
        }
    }
    for run in runs.values() {
        valid(bounded(&run.run_id.0, 128, "run ID"))?;
        valid(bounded(&run.agent_id.0, 128, "agent ID"))?;
        valid(run.intent.validate())?;
        valid(run.principal.validate())?;
        let agent = agents
            .get(&run.agent_id)
            .ok_or(Error::Corrupt("run agent"))?;
        if run.lifecycle.holds_capacity() {
            ensure(
                agent.current_run == run.run_id && run.outcome.is_none(),
                "live run ownership",
            )?;
        } else {
            ensure(
                run.outcome.is_some() && !run.startup_pending,
                "terminal run proof",
            )?;
        }
        if let Some(supervision) = &run.supervision {
            valid(supervision.validate())?;
        }
        match run.lifecycle {
            Lifecycle::Accepted | Lifecycle::Starting => ensure(
                run.startup_pending && run.supervision.is_none(),
                "startup evidence",
            )?,
            Lifecycle::Running => ensure(
                !run.startup_pending && run.supervision.is_some(),
                "running evidence",
            )?,
            _ => {}
        }
        if let Some(proof) = &run.outcome {
            valid(proof.validate())?;
            let valid_proof = match proof {
                OutcomeProof::PrelaunchFailure { .. } => {
                    run.lifecycle == Lifecycle::Failed && run.supervision.is_none()
                }
                OutcomeProof::ProcessTreeTerminated { interrupted, .. } => {
                    run.lifecycle
                        == if *interrupted {
                            Lifecycle::Interrupted
                        } else {
                            Lifecycle::Terminated
                        }
                }
                OutcomeProof::OwnerAttested { .. } => run.lifecycle == Lifecycle::Interrupted,
            };
            ensure(valid_proof, "outcome/lifecycle mismatch")?;
        }
    }
    for writer in writers.values() {
        valid(positive(writer.id.0, "writer"))?;
        valid(positive(writer.epoch.0, "writer epoch"))?;
        valid(writer.principal.validate())?;
        ensure(
            writer.id.0 < next_writer && writer.last_sequence <= i64::MAX as u64,
            "writer bounds",
        )?;
        let agent = agents
            .get(&writer.agent_id)
            .ok_or(Error::Corrupt("writer agent"))?;
        let run = runs
            .get(&writer.run_id)
            .ok_or(Error::Corrupt("writer run"))?;
        ensure(
            run.agent_id == writer.agent_id && writer.epoch.0 <= agent.epoch.0,
            "writer ownership",
        )?;
        if writer.live {
            ensure(
                agent.current_writer == Some(writer.id)
                    && agent.current_run == writer.run_id
                    && agent.epoch == writer.epoch
                    && agent.controller.as_ref() == Some(&writer.principal)
                    && agent.lifecycle == Lifecycle::Running,
                "live writer ownership",
            )?;
        }
    }
    let mut pending = HashSet::new();
    let mut latest = HashSet::new();
    for ack in all::<InputAck>(tx, "inputs")? {
        let writer = writers
            .get(&ack.writer)
            .ok_or(Error::Corrupt("input writer"))?;
        valid(positive(ack.sequence, "input sequence"))?;
        ensure(
            ack.sequence <= writer.last_sequence
                && ack.payload_bytes > 0
                && ack.payload_bytes <= 1_048_576,
            "input bounds",
        )?;
        if ack.sequence == writer.last_sequence {
            latest.insert(writer.id);
        }
        match ack.outcome {
            InputOutcome::Accepted => ensure(
                writer.live && ack.sequence == writer.last_sequence && pending.insert(writer.id),
                "pending input ownership",
            )?,
            InputOutcome::Partial { bytes } => ensure(
                bytes > 0 && bytes < ack.payload_bytes,
                "partial input bounds",
            )?,
            _ => {}
        }
    }
    // Older Written receipts may be evicted only while a newer one is inserted;
    // whole-writer compaction removes the writer too. A retained nonempty writer
    // must therefore retain its latest receipt, irrespective of the outcome.
    for writer in writers.values() {
        ensure(
            writer.last_sequence == 0 || latest.contains(&writer.id),
            "latest input receipt",
        )?;
    }
    for operation in all::<StoredOperation>(tx, "operations")? {
        valid(operation.key.validate())?;
        ensure(
            windows.contains_key(&operation.key.window),
            "operation window",
        )?;
        let run = runs
            .get(&operation.run)
            .ok_or(Error::Corrupt("operation run"))?;
        ensure(
            operation.descriptor.len() <= MAX_RECORD_BYTES
                && serde_json::from_str::<serde_json::Value>(&operation.descriptor).is_ok(),
            "operation descriptor",
        )?;
        match &operation.receipt {
            OperationReceipt::Launch(receipt) => {
                valid(receipt.guard.validate())?;
                ensure(
                    receipt.run_id == operation.run
                        && receipt.agent_id == run.agent_id
                        && receipt.guard.agent_id == receipt.agent_id
                        && receipt.guard.run_id == receipt.run_id,
                    "launch receipt",
                )?;
            }
            OperationReceipt::Agent(agent) => {
                validate_agent_shape(agent, owner)?;
                ensure(
                    agent.current_run == operation.run && agent.agent_id == run.agent_id,
                    "mutation receipt",
                )?;
            }
        }
    }
    Ok(())
}

fn validate_agent_shape(agent: &AgentRecord, owner: &str) -> Result<()> {
    valid(agent.guard().validate())?;
    ensure(agent.owner_daemon == owner, "agent owner")?;
    if let Some(controller) = &agent.controller {
        valid(controller.validate())?;
    }
    Ok(())
}

fn valid(result: Result<()>) -> Result<()> {
    result.map_err(|_| Error::Corrupt("invalid record field"))
}

fn ensure(condition: bool, name: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Corrupt(name))
    }
}
