//! Owner-local disposition of worker-owned runs. Local IPC already admits only
//! the owning UID. These operations take the run's per-agent boundary first
//! and then make their ledger transition under the authority barrier, so a
//! local takeover fences remote writers and a remote peer can never displace
//! the local controller.

use super::{Cause, Executor, Proof};
use crate::{
    daemon_lifecycle::mesh_store::peer_id_from_name,
    execution_state::{
        self as state, AgentId, Caller, Ledger, LocalAdmin, OperationId, OperationKey,
        PayloadDigest, Principal, RunId,
    },
    mesh_sessions,
};
use pu_core::mesh::admin::RunStatus;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// The single local principal for owner-initiated run operations.
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

impl From<state::Error> for RunError {
    fn from(error: state::Error) -> Self {
        match error {
            state::Error::NotFound(_) => Self::NotFound,
            state::Error::ControllerFenced | state::Error::Stale(_) => Self::Fenced,
            state::Error::InvalidTransition | state::Error::RunUnresolved => Self::InvalidState,
            _ => Self::Unavailable,
        }
    }
}

/// Authority-barrier work returns the operation's own result inside the
/// barrier's result, so ledger errors are never re-encoded as session errors.
type Inner<T> = mesh_sessions::Result<Result<T, RunError>>;

fn flatten<T>(result: Inner<T>) -> Result<T, RunError> {
    result.map_err(|_| RunError::Unavailable)?
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

fn controller_label(controller: &Option<Principal>) -> String {
    match controller {
        None => "none".into(),
        Some(Principal::Local { .. }) => "local".into(),
        Some(Principal::Peer { id, .. }) => format!("peer:{id}"),
    }
}

fn lifecycle_label(lifecycle: state::Lifecycle) -> &'static str {
    match lifecycle {
        state::Lifecycle::Accepted => "accepted",
        state::Lifecycle::Starting => "starting",
        state::Lifecycle::Running => "running",
        state::Lifecycle::Stopping => "stopping",
        state::Lifecycle::Unknown => "unknown",
        state::Lifecycle::Failed => "failed",
        state::Lifecycle::Terminated => "terminated",
        state::Lifecycle::Interrupted => "interrupted",
    }
}

impl Executor {
    async fn live(&self, agent: &str) -> bool {
        match self.engine.upgrade() {
            Some(engine) => engine
                .mesh_session(agent)
                .await
                .is_some_and(|session| session.exit_rx.borrow().is_none()),
            None => false,
        }
    }

    /// The run's boundary, held while local control is confirmed and through
    /// the caller's write. None unless the local owner controls the run. Lock
    /// order matches bridge dispatch: authority barrier, then the boundary; the
    /// barrier is released on return while the boundary stays held.
    pub async fn local_input_permit(
        self: &Arc<Self>,
        agent: &str,
    ) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        let (_, admin) = owner().ok()?;
        let boundary = self.agent_boundary(agent);
        let agent = AgentId(agent.to_string());
        self.authority
            .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                let held = boundary.blocking_lock_owned();
                Ok(ledger
                    .agent(&agent)
                    .is_ok_and(|record| is_owner(&record.controller))
                    .then_some(held))
            })
            .await
            .ok()
            .flatten()
    }

    pub async fn local_runs(self: &Arc<Self>) -> Result<Vec<RunStatus>, RunError> {
        let (_, admin) = owner()?;
        let records = flatten(
            self.authority
                .inspect_admin(&self.work, admin, |_, ledger, _, _| {
                    Ok((|| {
                        ledger
                            .unresolved_runs()?
                            .into_iter()
                            .take(256)
                            .map(|run| {
                                let agent = ledger.agent(&run.agent_id)?;
                                Ok((run, agent.controller))
                            })
                            .collect::<Result<Vec<_>, RunError>>()
                    })())
                })
                .await,
        )?;
        let mut statuses = Vec::with_capacity(records.len());
        for (run, controller) in records {
            statuses.push(RunStatus {
                live: self.live(&run.agent_id.0).await,
                agent_id: run.agent_id.0,
                run_id: run.run_id.0,
                export: run.intent.export,
                runtime: run.intent.runtime,
                lifecycle: lifecycle_label(run.lifecycle).into(),
                controller: controller_label(&controller),
            });
        }
        Ok(statuses)
    }

    /// Take (`take`) or release local control. A peer controller is fenced
    /// under that peer's exclusive administration barrier, which also closes
    /// its live control and bulk sessions. The controller is re-read under the
    /// barrier; if it changed since it was chosen, the attempt is retried.
    pub async fn local_control(self: &Arc<Self>, agent: &str, take: bool) -> Result<(), RunError> {
        for _ in 0..3 {
            let expected = self.controller(agent).await?;
            match self.change_controller(agent, take, expected).await? {
                true => return Ok(()),
                false => continue,
            }
        }
        Err(RunError::Unavailable)
    }

    async fn controller(self: &Arc<Self>, agent: &str) -> Result<Option<Principal>, RunError> {
        let (_, admin) = owner()?;
        let agent = AgentId(agent.to_string());
        flatten(
            self.authority
                .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                    Ok(ledger
                        .agent(&agent)
                        .map(|record| record.controller)
                        .map_err(RunError::from))
                })
                .await,
        )
    }

    /// False if the controller is no longer `expected` (caller retries).
    async fn change_controller(
        self: &Arc<Self>,
        agent: &str,
        take: bool,
        expected: Option<Principal>,
    ) -> Result<bool, RunError> {
        let (caller, admin) = owner()?;
        let agent_id = AgentId(agent.to_string());
        let controller = take.then(|| Principal::Local { id: OWNER.into() });
        let check = expected.clone();
        let boundary = self.agent_boundary(agent);
        let change = move |ledger: &Ledger, admin: &LocalAdmin| -> Result<bool, RunError> {
            // Barrier first (held by the caller), then the run's boundary.
            let _held = boundary.blocking_lock();
            let record = ledger.agent(&agent_id)?;
            if record.controller != check {
                return Ok(false);
            }
            let key = local_key(ledger, &caller, "takeover", &agent_id.0)?;
            let now = ledger.observe_time(chrono::Utc::now().timestamp())?;
            ledger.takeover(admin, &key, &record.guard(), controller, now)?;
            Ok(true)
        };
        let result = match &expected {
            Some(Principal::Peer { id, .. }) => {
                let peer = peer_id_from_name(id).map_err(|_| RunError::Unavailable)?;
                self.authority
                    .administer(&self.work, admin, peer, move |_, ledger, admin| {
                        Ok(change(ledger, admin))
                    })
                    .await
            }
            _ => {
                self.authority
                    .inspect_admin(&self.work, admin, move |_, ledger, _, admin| {
                        Ok(change(ledger, admin))
                    })
                    .await
            }
        };
        flatten(result)
    }

    /// Durable stop intent under local control, then one termination request
    /// to the guardian. Termination proof resolves the run later.
    pub async fn local_stop(self: &Arc<Self>, agent: &str) -> Result<(), RunError> {
        let (caller, admin) = owner()?;
        let agent_id = AgentId(agent.to_string());
        let executor = self.clone();
        flatten(
            self.authority
                .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                    let _held = executor.agent_boundary(&agent_id.0).blocking_lock_owned();
                    Ok((|| {
                        let record = ledger.agent(&agent_id)?;
                        if !is_owner(&record.controller) {
                            return Err(RunError::Fenced);
                        }
                        let key = local_key(ledger, &caller, "stop", &agent_id.0)?;
                        let now = ledger.observe_time(chrono::Utc::now().timestamp())?;
                        if ledger
                            .request_stop(&caller, &key, &record.guard(), now)?
                            .fresh
                            && let Some(engine) = executor.engine.upgrade()
                        {
                            // Durable intent first, then the single termination
                            // request, under the same boundary as bridge stops.
                            let agent = agent_id.0.clone();
                            executor
                                .runtime
                                .block_on(async move { engine.mesh_terminate(&agent).await });
                        }
                        Ok(())
                    })())
                })
                .await,
        )
    }

    /// Resolve a run with no live process that is Unknown, or Stopping with no
    /// session left to prove it. Verified supervisor proof is always preferred;
    /// otherwise the owner's attestation is recorded as such.
    pub async fn local_resolve(
        self: &Arc<Self>,
        run: &str,
        evidence: String,
    ) -> Result<(), RunError> {
        let (_, admin) = owner()?;
        let run_id = RunId(run.to_string());
        let agent = {
            let run_id = run_id.clone();
            flatten(
                self.authority
                    .inspect_admin(&self.work, admin.clone(), move |_, ledger, _, _| {
                        Ok(ledger.run(&run_id).map_err(RunError::from))
                    })
                    .await,
            )?
            .agent_id
        };
        let executor = self.clone();
        flatten(
            self.authority
                .inspect_admin(&self.work, admin, move |_, ledger, _, _| {
                    let _held = executor.agent_boundary(&agent.0).blocking_lock_owned();
                    Ok((|| {
                        let live = {
                            let executor = executor.clone();
                            let agent = agent.0.clone();
                            executor
                                .runtime
                                .clone()
                                .block_on(async move { executor.live(&agent).await })
                        };
                        if live {
                            return Err(RunError::Live);
                        }
                        let record = ledger.run(&run_id)?;
                        if !matches!(
                            record.lifecycle,
                            state::Lifecycle::Unknown | state::Lifecycle::Stopping
                        ) {
                            return Err(RunError::InvalidState);
                        }
                        let proof = match record
                            .supervision
                            .as_ref()
                            .map(|identity| executor.supervisor.outcome(&run_id.0, identity))
                        {
                            Some(Proof::Terminated { evidence, cause }) => {
                                state::OutcomeProof::ProcessTreeTerminated {
                                    evidence,
                                    interrupted: cause != Cause::AgentExit,
                                }
                            }
                            _ => state::OutcomeProof::OwnerAttested { evidence },
                        };
                        ledger.resolve_run(&run_id, &proof)?;
                        Ok(())
                    })())
                })
                .await,
        )?;
        self.supervisor.discard(run);
        Ok(())
    }
}
