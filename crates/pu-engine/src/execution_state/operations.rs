use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::{
    db::*,
    retention::{retain, retain_all},
    types::*,
    *,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredOperation {
    pub key: OperationKey,
    pub descriptor: String,
    pub run: RunId,
    pub receipt: OperationReceipt,
}

impl Ledger {
    /// Receiver-issued and principal-scoped. `now` is trusted receiver Unix time,
    /// never client time; a durable high-water mark prevents clock rollback replay.
    pub fn issue_window(
        &self,
        caller: &Caller,
        now: i64,
        ttl_seconds: i64,
    ) -> Result<OperationWindow> {
        self.transaction_at(now, |tx, now| {
            if ttl_seconds <= 0 || ttl_seconds > self.limits.max_window_seconds {
                return Err(Error::Invalid("window TTL"));
            }
            authorize(tx, caller.principal())?;
            retain(tx, now, &self.limits, self.limits.retention_batch)?;
            live_budget(tx, Live::Windows, caller.principal(), now, &self.limits)?;
            let window = OperationWindow {
                id: WindowId(next_id(tx, "next_window")?),
                principal: caller.principal().clone(),
                expires_at: now
                    .checked_add(ttl_seconds)
                    .ok_or(Error::Invalid("window expiry"))?,
                closed: false,
            };
            tx.execute(
                "INSERT INTO windows VALUES (?1, ?2)",
                params![window.id.0, encode(&window)?],
            )?;
            Ok(window)
        })
    }

    pub fn close_window(&self, caller: &Caller, id: WindowId) -> Result<()> {
        positive(id.0, "window")?;
        self.transaction(|tx| {
            authorize(tx, caller.principal())?;
            let mut window = owned_window(tx, caller, id)?;
            window.closed = true;
            put(tx, "windows", id.0, &window)
        })
    }

    /// Returns retained evidence even after window close, but rechecks peer
    /// generation/revocation and principal ownership. The engine also rechecks grants.
    pub fn operation_receipt(
        &self,
        caller: &Caller,
        key: &OperationKey,
    ) -> Result<Option<OperationReceipt>> {
        key.validate()?;
        self.transaction(|tx| {
            authorize(tx, caller.principal())?;
            owned_window(tx, caller, key.window)?;
            let Some(operation) = operation(tx, key)? else {
                return Ok(None);
            };
            if operation.key.digest != key.digest {
                return Err(Error::PayloadConflict);
            }
            Ok(Some(operation.receipt))
        })
    }

    /// Runs every retention step to completion at trusted receiver time. The same
    /// steps also run on open and, bounded, inside window/admission/writer calls,
    /// so no external scheduler is required. See `retention` for the horizons.
    /// Unknown/partial evidence and capacity-holding runs stay durable. Removed
    /// window/writer IDs always reject and counters never rewind.
    pub fn compact(&self, now: i64) -> Result<()> {
        self.transaction_at(now, |tx, now| retain_all(tx, now, &self.limits))
    }
}

pub(super) fn authorize(tx: &Transaction<'_>, principal: &Principal) -> Result<()> {
    principal.validate()?;
    if let Principal::Peer { id, generation } = principal {
        let peer: PeerRecord = get(tx, "peers", id)?.ok_or(Error::PeerFenced)?;
        if peer.revoked || peer.generation != *generation || peer.authorization_digest.is_none() {
            return Err(Error::PeerFenced);
        }
    }
    Ok(())
}

fn owned_window(tx: &Transaction<'_>, caller: &Caller, id: WindowId) -> Result<OperationWindow> {
    let window: OperationWindow = get(tx, "windows", id.0)?.ok_or(Error::WindowClosed)?;
    if !window.principal.same_identity(caller.principal()) {
        return Err(Error::Unauthorized);
    }
    Ok(window)
}

fn operation(tx: &Transaction<'_>, key: &OperationKey) -> Result<Option<StoredOperation>> {
    tx.query_row(
        "SELECT data FROM operations WHERE window=?1 AND id=?2",
        params![key.window.0, key.id.0],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(decode)
    .transpose()
}

pub(super) fn check_operation(
    tx: &Transaction<'_>,
    caller: &Caller,
    key: &OperationKey,
    descriptor: &str,
    now: i64,
    limits: &Limits,
) -> Result<Option<OperationReceipt>> {
    key.validate()?;
    authorize(tx, caller.principal())?;
    let window = owned_window(tx, caller, key.window)?;
    if let Some(operation) = operation(tx, key)? {
        if operation.key.digest != key.digest || operation.descriptor != descriptor {
            return Err(Error::PayloadConflict);
        }
        return Ok(Some(operation.receipt));
    }
    if window.closed || window.expires_at <= now {
        return Err(Error::WindowClosed);
    }
    live_budget(tx, Live::Operations, caller.principal(), now, limits)?;
    Ok(None)
}

pub(super) fn save_operation(
    tx: &Transaction<'_>,
    key: &OperationKey,
    descriptor: String,
    run: RunId,
    receipt: OperationReceipt,
) -> Result<()> {
    let operation = StoredOperation {
        key: key.clone(),
        descriptor,
        run,
        receipt,
    };
    tx.execute(
        "INSERT INTO operations VALUES (?1, ?2, ?3)",
        params![key.window.0, key.id.0, encode(&operation)?],
    )?;
    Ok(())
}

pub(super) fn check_guard(tx: &Transaction<'_>, guard: &MutationGuard) -> Result<AgentRecord> {
    guard.validate()?;
    let agent: AgentRecord =
        get(tx, "agents", &guard.agent_id.0)?.ok_or(Error::NotFound("agent"))?;
    if agent.guard() != *guard {
        return Err(Error::Stale(Box::new(agent)));
    }
    Ok(agent)
}

pub(super) fn controlled(
    tx: &Transaction<'_>,
    caller: &Caller,
    guard: &MutationGuard,
) -> Result<AgentRecord> {
    authorize(tx, caller.principal())?;
    let agent = check_guard(tx, guard)?;
    if agent.controller.as_ref() != Some(caller.principal()) {
        return Err(Error::ControllerFenced);
    }
    Ok(agent)
}

pub(super) fn change_lifecycle(tx: &Transaction<'_>, run: &RunRecord) -> Result<AgentRecord> {
    let mut agent: AgentRecord =
        get(tx, "agents", &run.agent_id.0)?.ok_or(Error::Corrupt("run agent"))?;
    if agent.current_run != run.run_id {
        return Err(Error::InvalidTransition);
    }
    agent.lifecycle = run.lifecycle;
    agent.revision.0 = increment(agent.revision.0)?;
    put(tx, "runs", &run.run_id.0, run)?;
    put(tx, "agents", &agent.agent_id.0, &agent)?;
    Ok(agent)
}
