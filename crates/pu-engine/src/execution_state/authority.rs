use rusqlite::{Transaction, params};

use super::{db::*, launch::fence_writers, operations::*, types::*, *};

enum PeerChange {
    Regrant,
    Advance,
    Revoke,
    Authorization(PayloadDigest),
}

impl Ledger {
    /// Separate local administrative entry point. An asserted generation on Caller
    /// cannot register a peer. Initial generation is receiver-assigned, always 1.
    /// Registration has no policy anchor and denies all peer use until explicitly
    /// anchored with update_peer_authorization.
    pub fn register_peer(&self, admin: &LocalAdmin, id: &str) -> Result<PeerRecord> {
        bounded(id, 128, "peer ID")?;
        self.transaction(|tx| {
            authorize(tx, admin.caller.principal())?;
            if get::<PeerRecord>(tx, "peers", id)?.is_some() {
                return Err(Error::InvalidTransition);
            }
            budget(tx, "peers", self.limits.peers)?;
            let peer = PeerRecord {
                id: id.into(),
                generation: 1,
                revoked: false,
                authorization_revision: 0,
                authorization_digest: None,
            };
            tx.execute(
                "INSERT INTO peers VALUES (?1, ?2)",
                params![id, encode(&peer)?],
            )?;
            Ok(peer)
        })
    }

    /// Explicit local regrant. Increments generation even if already active;
    /// clears affected controllers/windows without releasing or stopping runs.
    pub fn regrant_peer(
        &self,
        admin: &LocalAdmin,
        id: &str,
        expected_generation: u64,
    ) -> Result<PeerRecord> {
        self.change_peer(admin, id, Some(expected_generation), PeerChange::Regrant)
    }

    /// Worker-owned host-generation barrier. Requires a currently active peer.
    pub fn advance_peer_generation(
        &self,
        admin: &LocalAdmin,
        id: &str,
        expected_generation: u64,
    ) -> Result<PeerRecord> {
        self.change_peer(admin, id, Some(expected_generation), PeerChange::Advance)
    }

    /// Must be called under the engine's exclusive authority barrier, after prior
    /// dispatch/write chunks finish. Idempotent when already revoked. No run release.
    pub fn revoke_peer(&self, admin: &LocalAdmin, id: &str) -> Result<PeerRecord> {
        self.change_peer(admin, id, None, PeerChange::Revoke)
    }

    /// Bind durable canonical policy bytes to ledger authority. Under the exclusive
    /// authority barrier, persist/fsync policy bytes first, then commit this anchor.
    /// If either step fails or crashes, a policy/anchor mismatch must deny dispatch.
    /// Compare the digest before constructing a Caller from current generation;
    /// generation alone does not establish grant freshness. Revocation is preserved,
    /// so updating policy does not implicitly regrant a revoked peer.
    pub fn update_peer_authorization(
        &self,
        admin: &LocalAdmin,
        id: &str,
        expected_generation: u64,
        policy_digest: PayloadDigest,
    ) -> Result<PeerRecord> {
        self.change_peer(
            admin,
            id,
            Some(expected_generation),
            PeerChange::Authorization(policy_digest),
        )
    }

    fn change_peer(
        &self,
        admin: &LocalAdmin,
        id: &str,
        expected: Option<u64>,
        change: PeerChange,
    ) -> Result<PeerRecord> {
        bounded(id, 128, "peer ID")?;
        if let Some(generation) = expected {
            positive(generation, "generation")?;
        }
        self.transaction(|tx| {
            authorize(tx, admin.caller.principal())?;
            let mut peer: PeerRecord = get(tx, "peers", id)?.ok_or(Error::PeerFenced)?;
            if expected.is_some_and(|generation| generation != peer.generation) || (matches!(change, PeerChange::Advance) && peer.revoked) {
                return Err(Error::PeerFenced);
            }
            if matches!(change, PeerChange::Revoke) && peer.revoked { return Ok(peer); }
            peer.generation = increment(peer.generation)?;
            match change {
                PeerChange::Regrant => peer.revoked = false,
                PeerChange::Revoke => peer.revoked = true,
                PeerChange::Advance => {},
                PeerChange::Authorization(digest) => {
                    peer.authorization_revision = increment(peer.authorization_revision)?;
                    peer.authorization_digest = Some(digest);
                }
            }
            put(tx, "peers", id, &peer)?;
            for mut window in all::<OperationWindow>(tx, "windows")? {
                if matches!(&window.principal, Principal::Peer { id: owner, .. } if owner == id) {
                    window.closed = true;
                    put(tx, "windows", window.id.0, &window)?;
                }
            }
            for agent in all::<AgentRecord>(tx, "agents")? {
                if matches!(&agent.controller, Some(Principal::Peer { id: owner, .. }) if owner == id) {
                    set_controller(tx, agent, None)?;
                }
            }
            Ok(peer)
        })
    }

    /// Check stored active generation, controller, epoch, run and revision together.
    /// This pure check is useful only while the engine retains its shared authority
    /// barrier and per-agent boundary through the ensuing side effect.
    pub fn validate_dispatch(&self, caller: &Caller, guard: &MutationGuard) -> Result<AgentRecord> {
        self.transaction(|tx| controlled(tx, caller, guard))
    }

    /// Validate a peer before agent selection. Admission and mutations repeat this
    /// check transactionally; this method alone never authorizes a later effect.
    pub fn validate_principal(&self, caller: &Caller) -> Result<()> {
        self.transaction(|tx| authorize(tx, caller.principal()))
    }

    /// Read for local engine administration; engine applies read authorization.
    pub fn peer(&self, id: &str) -> Result<PeerRecord> {
        bounded(id, 128, "peer ID")?;
        self.transaction(|tx| get(tx, "peers", id)?.ok_or(Error::NotFound("peer")))
    }

    /// Ordinary acquisition cannot displace an existing controller. The engine
    /// separately authorizes the requested agent/export and control grant.
    pub fn acquire_control(
        &self,
        caller: &Caller,
        key: &OperationKey,
        guard: &MutationGuard,
        now: i64,
    ) -> Result<Recorded<AgentRecord>> {
        let descriptor = encode(&("acquire", guard))?;
        self.transaction(|tx| {
            if let Some(receipt) = check_operation(tx, caller, key, &descriptor, now, &self.limits)?
            {
                return agent_receipt(receipt);
            }
            let agent = check_guard(tx, guard)?;
            if agent
                .controller
                .as_ref()
                .is_some_and(|controller| controller != caller.principal())
            {
                return Err(Error::ControllerFenced);
            }
            let value = set_controller(tx, agent, Some(caller.principal().clone()))?;
            save_operation(
                tx,
                key,
                descriptor,
                value.current_run.clone(),
                OperationReceipt::Agent(value.clone()),
            )?;
            Ok(Recorded { value, fresh: true })
        })
    }

    /// Explicit local administrative takeover, under an exclusive authority barrier.
    /// Passing None removes control; passing a peer requires a stored active peer.
    pub fn takeover(
        &self,
        admin: &LocalAdmin,
        key: &OperationKey,
        guard: &MutationGuard,
        controller: Option<Principal>,
        now: i64,
    ) -> Result<Recorded<AgentRecord>> {
        let descriptor = encode(&("takeover", guard, &controller))?;
        self.transaction(|tx| {
            if let Some(receipt) =
                check_operation(tx, &admin.caller, key, &descriptor, now, &self.limits)?
            {
                return agent_receipt(receipt);
            }
            if let Some(controller) = &controller {
                authorize(tx, controller)?;
            }
            let agent = check_guard(tx, guard)?;
            let value = set_controller(tx, agent, controller)?;
            save_operation(
                tx,
                key,
                descriptor,
                value.current_run.clone(),
                OperationReceipt::Agent(value.clone()),
            )?;
            Ok(Recorded { value, fresh: true })
        })
    }

    /// Durable stop intent, not termination evidence. Holds its slot until resolve_run.
    pub fn request_stop(
        &self,
        caller: &Caller,
        key: &OperationKey,
        guard: &MutationGuard,
        now: i64,
    ) -> Result<Recorded<AgentRecord>> {
        let descriptor = encode(&("stop", guard))?;
        self.transaction(|tx| {
            if let Some(receipt) = check_operation(tx, caller, key, &descriptor, now, &self.limits)?
            {
                return agent_receipt(receipt);
            }
            let agent = controlled(tx, caller, guard)?;
            let mut run: RunRecord =
                get(tx, "runs", &agent.current_run.0)?.ok_or(Error::Corrupt("stop run"))?;
            if !run.lifecycle.holds_capacity() || run.lifecycle == Lifecycle::Stopping {
                return Err(Error::InvalidTransition);
            }
            run.lifecycle = Lifecycle::Stopping;
            let value = change_lifecycle(tx, &run)?;
            fence_writers(tx, &agent.agent_id)?;
            save_operation(
                tx,
                key,
                descriptor,
                value.current_run.clone(),
                OperationReceipt::Agent(value.clone()),
            )?;
            Ok(Recorded { value, fresh: true })
        })
    }
}

fn agent_receipt(receipt: OperationReceipt) -> Result<Recorded<AgentRecord>> {
    let OperationReceipt::Agent(value) = receipt else {
        return Err(Error::PayloadConflict);
    };
    Ok(Recorded {
        value,
        fresh: false,
    })
}

fn set_controller(
    tx: &Transaction<'_>,
    mut agent: AgentRecord,
    controller: Option<Principal>,
) -> Result<AgentRecord> {
    agent.controller = controller;
    agent.epoch.0 = increment(agent.epoch.0)?;
    agent.revision.0 = increment(agent.revision.0)?;
    fence_writers(tx, &agent.agent_id)?;
    put(tx, "agents", &agent.agent_id.0, &agent)?;
    Ok(agent)
}
