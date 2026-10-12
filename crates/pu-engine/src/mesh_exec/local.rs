//! Owner-local disposition of worker-owned runs. Local IPC already admits only
//! the owning UID; these operations take the same per-agent boundary and ledger
//! transitions as bridge requests, so a local takeover fences remote writers
//! and a remote peer can never displace the local controller.

use super::{Executor, Proof};
use crate::{
    daemon_lifecycle::mesh_store::{peer_id_from_name, peer_name},
    execution_state::{
        self as state, AgentId, Caller, Ledger, LocalAdmin, OperationId, OperationKey,
        PayloadDigest, Principal, RunId,
    },
    mesh_sessions::{self, Error as SessionError},
};
use pu_core::mesh::admin::RunStatus;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const OWNER: &str = "owner";

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("no worker-owned run matches")]
    NotFound,
    #[error("the run is controlled by another principal; take over first")]
    Fenced,
    #[error("the run is not in a state that permits this")]
    InvalidState,
    #[error("the run still has a live process; stop it first")]
    Live,
    #[error("mesh run state is unavailable")]
    Unavailable,
}

fn owner() -> Result<(Caller, LocalAdmin), RunError> {
    let caller = Caller::authenticated_local(OWNER).map_err(|_| RunError::Unavailable)?;
    let admin =
        LocalAdmin::authorized_by_local_policy(&caller).map_err(|_| RunError::Unavailable)?;
    Ok((caller, admin))
}

fn is_owner(principal: &Option<Principal>) -> bool {
    matches!(principal, Some(Principal::Local { id }) if id == OWNER)
}

/// One owner-issued operation identity for a local ledger transition.
fn local_key(
    ledger: &Ledger,
    caller: &Caller,
    kind: &str,
    agent: &str,
) -> state::Result<OperationKey> {
    let now = ledger.observe_time(chrono::Utc::now().timestamp())?;
    let window = ledger.issue_window(caller, now, 60)?;
    let id = pu_core::id::session_id();
    Ok(OperationKey {
        window: window.id,
        digest: PayloadDigest(Sha256::digest(format!("{kind}:{agent}:{id}")).into()),
        id: OperationId(id),
    })
}

fn map(error: SessionError) -> RunError {
    match error {
        SessionError::Denied => RunError::Fenced,
        SessionError::Frame => RunError::NotFound,
        SessionError::Capacity => RunError::InvalidState,
        _ => RunError::Unavailable,
    }
}

fn ledger_error(error: state::Error) -> SessionError {
    match error {
        state::Error::NotFound(_) => SessionError::Frame,
        state::Error::ControllerFenced | state::Error::Stale(_) => SessionError::Denied,
        state::Error::InvalidTransition | state::Error::RunUnresolved => SessionError::Capacity,
        _ => SessionError::Operation,
    }
}

impl Executor {
    /// True only while the local owner holds control of this agent's run.
    pub async fn local_controls(self: &Arc<Self>, agent: &str) -> bool {
        let Ok((_, admin)) = owner() else {
            return false;
        };
        let agent = AgentId(agent.to_string());
        self.authority
            .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                ledger
                    .agent(&agent)
                    .map(|record| is_owner(&record.controller))
                    .map_err(|_| SessionError::Operation)
            })
            .await
            .unwrap_or(false)
    }

    pub async fn local_runs(self: &Arc<Self>) -> Result<Vec<RunStatus>, RunError> {
        let (_, admin) = owner()?;
        let records = self
            .authority
            .inspect_admin(&self.work, admin, |_, ledger, _, _| {
                let runs = ledger.unresolved_runs().map_err(ledger_error)?;
                runs.into_iter()
                    .take(256)
                    .map(|run| {
                        let agent = ledger.agent(&run.agent_id).map_err(ledger_error)?;
                        Ok((run, agent.controller))
                    })
                    .collect::<mesh_sessions::Result<Vec<_>>>()
            })
            .await
            .map_err(map)?;
        let engine = self.engine.upgrade();
        let mut statuses = Vec::with_capacity(records.len());
        for (run, controller) in records {
            let live = match &engine {
                Some(engine) => engine
                    .mesh_session(&run.agent_id.0)
                    .await
                    .is_some_and(|session| session.exit_rx.borrow().is_none()),
                None => false,
            };
            statuses.push(RunStatus {
                agent_id: run.agent_id.0,
                run_id: run.run_id.0,
                export: run.intent.export,
                runtime: run.intent.runtime,
                lifecycle: format!("{:?}", run.lifecycle).to_lowercase(),
                controller: match controller {
                    None => "none".into(),
                    Some(Principal::Local { .. }) => "local".into(),
                    Some(Principal::Peer { id, .. }) => format!("peer:{id}"),
                },
                live,
            });
        }
        Ok(statuses)
    }

    /// Take (Some) or release (None) local control. Taking control from a peer
    /// runs under that peer's exclusive administration barrier, which also
    /// closes its live control and bulk sessions.
    pub async fn local_control(self: &Arc<Self>, agent: &str, take: bool) -> Result<(), RunError> {
        let (caller, admin) = owner()?;
        let agent_id = AgentId(agent.to_string());
        let current = {
            let agent_id = agent_id.clone();
            self.authority
                .inspect_admin(&self.work, admin.clone(), move |_, ledger, _, _| {
                    ledger.agent(&agent_id).map_err(ledger_error)
                })
                .await
                .map_err(map)?
        };
        let controller = take.then(|| Principal::Local { id: OWNER.into() });
        let boundary = self.agent_boundary(agent);
        let change = move |ledger: &Ledger, admin: &LocalAdmin| -> mesh_sessions::Result<()> {
            let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
            let record = ledger.agent(&agent_id).map_err(ledger_error)?;
            let key = local_key(ledger, &caller, "takeover", &agent_id.0).map_err(ledger_error)?;
            let now = ledger
                .observe_time(chrono::Utc::now().timestamp())
                .map_err(ledger_error)?;
            ledger
                .takeover(admin, &key, &record.guard(), controller, now)
                .map_err(ledger_error)?;
            Ok(())
        };
        match &current.controller {
            Some(Principal::Peer { id, .. }) => {
                let peer = peer_id_from_name(id).map_err(|_| RunError::Unavailable)?;
                debug_assert_eq!(peer_name(peer), *id);
                self.authority
                    .administer(&self.work, admin, peer, move |_, ledger, admin| {
                        change(ledger, admin)
                    })
                    .await
                    .map_err(map)
            }
            _ => self
                .authority
                .inspect_admin(&self.work, admin, move |_, ledger, _, admin| {
                    change(ledger, admin)
                })
                .await
                .map_err(map),
        }
    }

    /// Durable stop intent under local control, then one termination request
    /// to the guardian. Termination proof resolves the run later.
    pub async fn local_stop(self: &Arc<Self>, agent: &str) -> Result<(), RunError> {
        let (caller, admin) = owner()?;
        let agent_id = AgentId(agent.to_string());
        let boundary = self.agent_boundary(agent);
        let fresh = self
            .authority
            .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
                let record = ledger.agent(&agent_id).map_err(ledger_error)?;
                if !is_owner(&record.controller) {
                    return Err(SessionError::Denied);
                }
                let key = local_key(ledger, &caller, "stop", &agent_id.0).map_err(ledger_error)?;
                let now = ledger
                    .observe_time(chrono::Utc::now().timestamp())
                    .map_err(ledger_error)?;
                ledger
                    .request_stop(&caller, &key, &record.guard(), now)
                    .map(|recorded| recorded.fresh)
                    .map_err(ledger_error)
            })
            .await
            .map_err(map)?;
        if fresh && let Some(engine) = self.engine.upgrade() {
            engine.mesh_terminate(agent).await;
        }
        Ok(())
    }

    /// Resolve an Unknown run. Verified supervisor proof is always preferred;
    /// otherwise the owner's attestation is recorded as the evidence. A run with
    /// a live process is never released this way.
    pub async fn local_resolve(
        self: &Arc<Self>,
        run: &str,
        evidence: String,
    ) -> Result<(), RunError> {
        let (_, admin) = owner()?;
        let run_id = RunId(run.to_string());
        let agent = {
            let run_id = run_id.clone();
            self.authority
                .inspect_admin(&self.work, admin.clone(), move |_, ledger, _, _| {
                    ledger.run(&run_id).map_err(ledger_error)
                })
                .await
                .map_err(map)?
                .agent_id
        };
        if let Some(engine) = self.engine.upgrade()
            && engine
                .mesh_session(&agent.0)
                .await
                .is_some_and(|session| session.exit_rx.borrow().is_none())
        {
            return Err(RunError::Live);
        }
        let supervisor = self.supervisor.clone();
        let boundary = self.agent_boundary(&agent.0);
        self.authority
            .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                let _held = boundary.lock().unwrap_or_else(|e| e.into_inner());
                let record = ledger.run(&run_id).map_err(ledger_error)?;
                if record.lifecycle != state::Lifecycle::Unknown {
                    return Err(SessionError::Capacity);
                }
                let proof = match record
                    .supervision
                    .as_ref()
                    .map(|identity| supervisor.outcome(&run_id.0, identity))
                {
                    Some(Proof::Terminated {
                        evidence,
                        interrupted,
                    }) => state::OutcomeProof::ProcessTreeTerminated {
                        evidence,
                        interrupted,
                    },
                    _ => state::OutcomeProof::ProcessTreeTerminated {
                        evidence: format!("owner attestation: {evidence}"),
                        interrupted: true,
                    },
                };
                ledger.resolve_run(&run_id, &proof).map_err(ledger_error)?;
                Ok(())
            })
            .await
            .map_err(map)
    }
}
