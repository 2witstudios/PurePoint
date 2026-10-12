use rusqlite::{OptionalExtension, Transaction, params};

use super::{db::*, launch::fence_writers, operations::*, types::*, *};

impl Ledger {
    /// Opens exactly one receiver-issued writer per agent/run/epoch, fencing its
    /// predecessor. When a predecessor exists, explicitly supply its inspected
    /// checkpoint. The engine must reconcile its receipts and present uncertainty,
    /// never repackage old accepted/partial/unknown bytes under the new identity.
    pub fn open_writer(
        &self,
        caller: &Caller,
        guard: &MutationGuard,
        reconciled: Option<WriterCheckpoint>,
    ) -> Result<WriterRecord> {
        self.transaction(|tx| {
            let mut agent = controlled(tx, caller, guard)?;
            if agent.lifecycle != Lifecycle::Running {
                return Err(Error::InvalidTransition);
            }
            let required = agent
                .current_writer
                .map(|id| {
                    get::<WriterRecord>(tx, "writers", id.0)?
                        .map(|writer| writer.checkpoint())
                        .ok_or(Error::Corrupt("current writer"))
                })
                .transpose()?;
            if required != reconciled {
                return Err(Error::ReconciliationRequired);
            }
            budget(tx, "writers", self.limits.writers)?;
            fence_writers(tx, &agent.agent_id)?;
            let writer = WriterRecord {
                id: WriterId(next_id(tx, "next_writer")?),
                agent_id: agent.agent_id.clone(),
                run_id: agent.current_run.clone(),
                epoch: agent.epoch,
                principal: caller.principal().clone(),
                last_sequence: 0,
                live: true,
            };
            tx.execute(
                "INSERT INTO writers VALUES (?1, ?2, ?3, ?4)",
                params![
                    writer.id.0,
                    writer.agent_id.0,
                    writer.run_id.0,
                    encode(&writer)?
                ],
            )?;
            agent.current_writer = Some(writer.id);
            put(tx, "agents", &agent.agent_id.0, &agent)?;
            Ok(writer)
        })
    }

    /// Durably accepts the next input before one engine-owned write attempt. Only
    /// fresh=true permits that attempt. Stores digest/length, never PTY bytes.
    /// At most one Accepted input is pending per writer. No method retries writes.
    pub fn accept_input(
        &self,
        caller: &Caller,
        guard: &MutationGuard,
        writer_id: WriterId,
        sequence: u64,
        digest: PayloadDigest,
        payload_bytes: u32,
    ) -> Result<Recorded<InputAck>> {
        positive(writer_id.0, "writer")?;
        positive(sequence, "input sequence")?;
        if payload_bytes == 0 || payload_bytes > self.limits.max_input_bytes {
            return Err(Error::Invalid("input size"));
        }
        self.transaction(|tx| {
            let agent = controlled(tx, caller, guard)?;
            let mut writer = live_writer(tx, writer_id, &agent)?;
            if writer.principal != *caller.principal() {
                return Err(Error::WriterFenced);
            }
            if sequence <= writer.last_sequence {
                let ack = acknowledgement(tx, writer_id, sequence)?.ok_or(Error::ReceiptExpired)?;
                if ack.digest != digest || ack.payload_bytes != payload_bytes {
                    return Err(Error::PayloadConflict);
                }
                return Ok(Recorded {
                    value: ack,
                    fresh: false,
                });
            }
            let expected = increment(writer.last_sequence)?;
            if sequence != expected {
                return Err(Error::SequenceGap { expected });
            }
            if agent.lifecycle != Lifecycle::Running {
                return Err(Error::InvalidTransition);
            }
            let mut retained = writer_acks(tx, writer_id)?;
            if retained
                .iter()
                .any(|ack| ack.outcome == InputOutcome::Accepted)
            {
                return Err(Error::InputPending);
            }
            while retained.len() >= self.limits.receipts_per_writer {
                let index = retained
                    .iter()
                    .position(|ack| {
                        matches!(
                            ack.outcome,
                            InputOutcome::Written | InputOutcome::NotWritten
                        )
                    })
                    .ok_or(Error::Capacity {
                        scope: CapacityScope::Records("writer receipts"),
                        limit: self.limits.receipts_per_writer,
                    })?;
                let old = retained.remove(index);
                tx.execute(
                    "DELETE FROM inputs WHERE writer=?1 AND sequence=?2",
                    params![writer_id.0, old.sequence],
                )?;
            }
            budget(tx, "inputs", self.limits.input_receipts)?;
            let value = InputAck {
                writer: writer_id,
                sequence,
                digest,
                payload_bytes,
                outcome: InputOutcome::Accepted,
            };
            tx.execute(
                "INSERT INTO inputs VALUES (?1, ?2, ?3)",
                params![writer_id.0, sequence, encode(&value)?],
            )?;
            writer.last_sequence = sequence;
            put(tx, "writers", writer_id.0, &writer)?;
            Ok(Recorded { value, fresh: true })
        })
    }

    /// Supervisor-only completion of the single write attempt, still under the
    /// authority/agent barriers. Written describes PTY interface delivery only.
    /// Partial/Unknown are final evidence; no transition back to Accepted exists.
    /// If persistence fails after bytes may have been written, do not redeliver;
    /// retain the barriers, attempt to record Unknown, or fail closed until recovery.
    pub fn finish_input(
        &self,
        writer_id: WriterId,
        sequence: u64,
        outcome: InputOutcome,
    ) -> Result<InputAck> {
        positive(writer_id.0, "writer")?;
        positive(sequence, "input sequence")?;
        if outcome == InputOutcome::Accepted {
            return Err(Error::InvalidTransition);
        }
        self.transaction(|tx| {
            let writer: WriterRecord =
                get(tx, "writers", writer_id.0)?.ok_or(Error::WriterFenced)?;
            let agent: AgentRecord =
                get(tx, "agents", &writer.agent_id.0)?.ok_or(Error::Corrupt("writer agent"))?;
            live_writer(tx, writer_id, &agent)?;
            authorize(tx, &writer.principal)?;
            let mut ack = acknowledgement(tx, writer_id, sequence)?.ok_or(Error::ReceiptExpired)?;
            if let InputOutcome::Partial { bytes } = outcome
                && (bytes == 0 || bytes >= ack.payload_bytes)
            {
                return Err(Error::Invalid("partial byte count"));
            }
            if ack.outcome == outcome {
                return Ok(ack);
            }
            if ack.outcome != InputOutcome::Accepted {
                return Err(Error::InvalidTransition);
            }
            ack.outcome = outcome;
            tx.execute(
                "UPDATE inputs SET data=?3 WHERE writer=?1 AND sequence=?2",
                params![writer_id.0, sequence, encode(&ack)?],
            )?;
            Ok(ack)
        })
    }

    /// Engine-authorized read, including fenced writers. Recheck current caller
    /// authorization against writer -> agent -> project before exposing evidence.
    pub fn input_receipt(&self, writer: WriterId, sequence: u64) -> Result<Option<InputAck>> {
        positive(writer.0, "writer")?;
        positive(sequence, "input sequence")?;
        self.transaction(|tx| acknowledgement(tx, writer, sequence))
    }

    /// Engine-authorized read used to reconcile a predecessor before replacement.
    pub fn writer(&self, id: WriterId) -> Result<WriterRecord> {
        positive(id.0, "writer")?;
        self.transaction(|tx| get(tx, "writers", id.0)?.ok_or(Error::WriterFenced))
    }
}

fn live_writer(tx: &Transaction<'_>, id: WriterId, agent: &AgentRecord) -> Result<WriterRecord> {
    let writer: WriterRecord = get(tx, "writers", id.0)?.ok_or(Error::WriterFenced)?;
    if !writer.live
        || writer.agent_id != agent.agent_id
        || writer.run_id != agent.current_run
        || writer.epoch != agent.epoch
        || agent.current_writer != Some(id)
        || agent.controller.as_ref() != Some(&writer.principal)
    {
        return Err(Error::WriterFenced);
    }
    Ok(writer)
}

fn acknowledgement(
    tx: &Transaction<'_>,
    writer: WriterId,
    sequence: u64,
) -> Result<Option<InputAck>> {
    tx.query_row(
        "SELECT data FROM inputs WHERE writer=?1 AND sequence=?2",
        params![writer.0, sequence],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(decode)
    .transpose()
}

fn writer_acks(tx: &Transaction<'_>, writer: WriterId) -> Result<Vec<InputAck>> {
    let mut statement = tx.prepare("SELECT data FROM inputs WHERE writer=?1 ORDER BY sequence")?;
    statement
        .query_map([writer.0], |r| r.get::<_, String>(0))?
        .map(|row| decode(row?))
        .collect()
}
