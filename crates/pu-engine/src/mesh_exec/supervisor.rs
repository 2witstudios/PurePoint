//! Process-tree lifetime supervision for mesh runs. The selected v1 contract is
//! the per-run guardian (`crate::mesh_guardian`): it terminates the marked run
//! tree when the daemon dies, the agent exits or it is signalled, and leaves a
//! verified outcome record. Anything short of verified proof keeps the run
//! Unknown with its capacity reserved.

use crate::execution_state::SupervisionIdentity;
use crate::mesh_guardian::{self, GuardianLaunch, Trigger};
use std::path::PathBuf;

/// A wrapped launch: the guardian command line plus the environment carrying
/// its secret token (never argv), and the durable identity recorded for it.
pub struct Prepared {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub identity: SupervisionIdentity,
}

pub enum Proof {
    Terminated { evidence: String, interrupted: bool },
    Unproven,
}

pub trait Supervisor: Send + Sync + 'static {
    fn prepare(
        &self,
        run_id: &str,
        command: String,
        args: Vec<String>,
    ) -> std::io::Result<Prepared>;
    fn outcome(&self, run_id: &str, identity: &SupervisionIdentity) -> Proof;
}

pub struct Guardian {
    executable: PathBuf,
    outcome_dir: PathBuf,
}

impl Guardian {
    pub fn new(executable: PathBuf, outcome_dir: PathBuf) -> Self {
        Self {
            executable,
            outcome_dir,
        }
    }
}

impl Supervisor for Guardian {
    fn prepare(
        &self,
        run_id: &str,
        command: String,
        args: Vec<String>,
    ) -> std::io::Result<Prepared> {
        let token = mesh_guardian::new_token();
        let (mechanism, digest) = mesh_guardian::supervision_identity(&token);
        let launch = GuardianLaunch {
            run_id: run_id.to_string(),
            token,
            outcome_dir: self.outcome_dir.clone(),
        };
        let (command, args, env) =
            mesh_guardian::wrap_spawn(&launch, &self.executable, std::process::id(), command, args);
        Ok(Prepared {
            command,
            args,
            env,
            // Only the digest is durable; the raw token lives in the guardian.
            identity: SupervisionIdentity {
                mechanism,
                token: digest,
            },
        })
    }

    fn outcome(&self, run_id: &str, identity: &SupervisionIdentity) -> Proof {
        if !identity.mechanism.starts_with("guardian-v1-") {
            return Proof::Unproven;
        }
        match mesh_guardian::read_outcome_by_digest(&self.outcome_dir, run_id, &identity.token) {
            Ok(Some(outcome)) if outcome.verified => {
                let trigger = match outcome.trigger {
                    Trigger::DaemonExit => "daemon_exit",
                    Trigger::AgentExit => "agent_exit",
                    Trigger::Signal => "signal",
                };
                Proof::Terminated {
                    evidence: format!(
                        "{} verified tree termination: trigger={trigger} exit={:?}",
                        identity.mechanism, outcome.agent_exit
                    ),
                    interrupted: matches!(outcome.trigger, Trigger::DaemonExit),
                }
            }
            _ => Proof::Unproven,
        }
    }
}
