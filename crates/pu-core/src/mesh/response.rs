//! Correlated bridge metadata. Receiver-local paths, supervisor tokens and
//! private ledger records are deliberately absent from this network schema.
use super::{
    valid_identifier,
    wire::{AgentTarget, Export, MAX_CONTROL_FRAME, OutputCursor, Request, WireError},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub request_id: String,
    pub daemon_id: String,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Body {
    Inventory {
        exports: Vec<ExportSummary>,
    },
    Status {
        agents: Vec<AgentSummary>,
        next_agent: Option<String>,
    },
    Receipt {
        evidence: Option<ReceiptEvidence>,
    },
    InputReceipt {
        evidence: Option<InputEvidence>,
    },
    /// Receiver-issued, principal-scoped operation window.
    Window {
        window: u64,
        expires_at: i64,
    },
    /// Single input writer for one run/control epoch, fencing its predecessor.
    Writer {
        target: AgentTarget,
        writer: u64,
        epoch: u64,
    },
    /// Output now flows on the paired bulk connection, starting at `cursor`.
    Subscribed {
        target: AgentTarget,
        cursor: OutputCursor,
    },
    Done {},
    Error {
        code: ErrorCode,
    },
}

/// One bounded bulk frame of raw PTY output for one run. `gap` reports bytes
/// that were evicted before they could be sent; they are never silently filled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFrame {
    pub target: AgentTarget,
    pub incarnation: String,
    pub offset: u64,
    pub gap: Option<u64>,
    pub data: Vec<u8>,
    /// The run's output ended (process exited); no further frames follow.
    pub end: bool,
}

impl OutputFrame {
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| WireError::Invalid)?;
        if bytes.len() > super::wire::MAX_BULK_FRAME {
            return Err(WireError::Invalid);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.is_empty() || bytes.len() > super::wire::MAX_BULK_FRAME {
            return Err(WireError::Invalid);
        }
        let frame: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        frame.validate()?;
        Ok(frame)
    }

    fn validate(&self) -> Result<(), WireError> {
        let valid = valid_identifier(&self.target.agent_id)
            && valid_identifier(&self.target.run_id)
            && valid_identifier(&self.target.export.id)
            && self.target.export.generation > 0
            && valid_identifier(&self.incarnation)
            && self.data.len() <= MAX_OUTPUT_CHUNK
            && self.gap.is_none_or(|gap| gap > 0)
            && self.offset <= i64::MAX as u64;
        if valid {
            Ok(())
        } else {
            Err(WireError::Invalid)
        }
    }
}

/// Raw output bytes per bulk frame; JSON byte arrays stay below the bulk cap.
pub const MAX_OUTPUT_CHUNK: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Denied,
    Unavailable,
    Conflict,
    ReceiptExpired,
    ExecutionDisabled,
    Unsupported,
    Capacity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSummary {
    pub export: Export,
    pub runtimes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Accepted,
    Starting,
    Running,
    Stopping,
    Unknown,
    Failed,
    Terminated,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Controller {
    None,
    ThisPeer,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSummary {
    pub target: AgentTarget,
    pub lifecycle: Lifecycle,
    pub epoch: u64,
    pub revision: u64,
    pub controller: Controller,
    pub runtime: String,
}

/// A historical acceptance snapshot, never the current lifecycle or permission
/// to execute again. Query Status to reconcile current durable state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEvidence {
    pub target: AgentTarget,
    pub epoch: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputOutcome {
    Accepted,
    Written,
    Partial { bytes: u32 },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputEvidence {
    pub target: AgentTarget,
    pub writer: u64,
    pub sequence: u64,
    pub digest: [u8; 32],
    pub payload_bytes: u32,
    pub outcome: InputOutcome,
}

impl Response {
    /// Match identity/correlation and response kind before exposing a decoded
    /// payload to the host. Transport authentication remains a separate step.
    pub fn decode(bytes: &[u8], request: &Request) -> Result<Self, WireError> {
        request.validate()?;
        if bytes.is_empty() || bytes.len() > MAX_CONTROL_FRAME {
            return Err(WireError::Invalid);
        }
        let response: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        response.validate()?;
        if response.request_id != request.request_id || response.daemon_id != request.target_daemon
        {
            return Err(WireError::Invalid);
        }
        use super::wire::Operation;
        let matches = match (&request.operation, &response.body) {
            (_, Body::Error { .. }) | (Operation::Inventory {}, Body::Inventory { .. }) => true,
            (Operation::IssueWindow { .. }, Body::Window { .. })
            | (Operation::CloseWindow { .. }, Body::Done {})
            | (Operation::Resize { .. }, Body::Done {}) => true,
            (Operation::Spawn { export, .. }, Body::Receipt { evidence: Some(e) }) => {
                e.target.export == *export
            }
            (
                Operation::AcquireControl { guard, .. } | Operation::Stop { guard, .. },
                Body::Receipt { evidence: Some(e) },
            ) => e.target == guard.target,
            (Operation::Resume { guard, .. }, Body::Receipt { evidence: Some(e) }) => {
                e.target.export == guard.target.export && e.target.agent_id == guard.target.agent_id
            }
            (Operation::OpenWriter { guard, .. }, Body::Writer { target, .. }) => {
                *target == guard.target
            }
            (
                Operation::Input {
                    guard,
                    writer,
                    sequence,
                    ..
                },
                Body::InputReceipt { evidence: Some(e) },
            ) => e.target == guard.target && e.writer == *writer && e.sequence == *sequence,
            (Operation::SubscribeOutput { target, .. }, Body::Subscribed { target: t, .. }) => {
                t == target
            }
            (
                Operation::Status {
                    export,
                    agent_id,
                    after_agent,
                    limit,
                },
                Body::Status { agents, next_agent },
            ) => {
                agents.iter().all(|a| {
                    a.target.export == *export
                        && agent_id.as_ref().is_none_or(|id| a.target.agent_id == *id)
                        && after_agent
                            .as_ref()
                            .is_none_or(|id| a.target.agent_id > *id)
                }) && agents.len() <= usize::from(*limit)
                    && (agent_id.is_none() || (agents.len() <= 1 && next_agent.is_none()))
            }
            (Operation::Receipt { export, .. }, Body::Receipt { evidence }) => {
                evidence.as_ref().is_none_or(|e| e.target.export == *export)
            }
            (
                Operation::InputReceipt {
                    target,
                    writer,
                    sequence,
                },
                Body::InputReceipt { evidence },
            ) => evidence.as_ref().is_none_or(|e| {
                e.target == *target && e.writer == *writer && e.sequence == *sequence
            }),
            _ => false,
        };
        if !matches {
            return Err(WireError::Invalid);
        }
        Ok(response)
    }

    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| WireError::Invalid)?;
        if bytes.len() > MAX_CONTROL_FRAME {
            return Err(WireError::Invalid);
        }
        Ok(bytes)
    }

    fn validate(&self) -> Result<(), WireError> {
        let counter = |n: u64| n > 0 && n <= i64::MAX as u64;
        let export = |e: &Export| valid_identifier(&e.id) && counter(e.generation);
        let target = |t: &AgentTarget| {
            export(&t.export) && valid_identifier(&t.agent_id) && valid_identifier(&t.run_id)
        };
        let valid = valid_identifier(&self.request_id)
            && valid_identifier(&self.daemon_id)
            && match &self.body {
                Body::Inventory { exports } => {
                    exports.len() <= 64
                        && exports.iter().all(|e| {
                            export(&e.export)
                                && e.runtimes.len() <= 32
                                && e.runtimes.iter().all(|r| valid_identifier(r))
                        })
                }
                Body::Status { agents, next_agent } => {
                    agents.len() <= 64
                        && agents
                            .windows(2)
                            .all(|pair| pair[0].target.agent_id < pair[1].target.agent_id)
                        && agents.iter().all(|a| {
                            target(&a.target)
                                && counter(a.epoch)
                                && counter(a.revision)
                                && valid_identifier(&a.runtime)
                        })
                        && next_agent.as_ref().is_none_or(|n| {
                            valid_identifier(n)
                                && agents.last().is_some_and(|a| a.target.agent_id == *n)
                        })
                }
                Body::Receipt { evidence } => evidence
                    .as_ref()
                    .is_none_or(|r| target(&r.target) && counter(r.epoch) && counter(r.revision)),
                Body::InputReceipt { evidence } => evidence.as_ref().is_none_or(|r| {
                    target(&r.target)
                        && counter(r.writer)
                        && counter(r.sequence)
                        && (1..=super::wire::MAX_INPUT_BYTES as u32).contains(&r.payload_bytes)
                        && match r.outcome {
                            InputOutcome::Partial { bytes } => bytes > 0 && bytes < r.payload_bytes,
                            _ => true,
                        }
                }),
                Body::Window { window, expires_at } => counter(*window) && *expires_at > 0,
                Body::Writer {
                    target: t,
                    writer,
                    epoch,
                } => target(t) && counter(*writer) && counter(*epoch),
                Body::Subscribed { target: t, cursor } => {
                    target(t)
                        && valid_identifier(&cursor.incarnation)
                        && cursor.offset <= i64::MAX as u64
                }
                Body::Done {} | Body::Error { .. } => true,
            };
        if valid {
            Ok(())
        } else {
            Err(WireError::Invalid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn given_status_page_should_reject_duplicates_prior_cursor_and_excess_rows() {
        let export = Export {
            id: "project".into(),
            generation: 1,
        };
        let request = Request {
            request_id: "request".into(),
            target_daemon: "worker".into(),
            operation: super::super::wire::Operation::Status {
                export: export.clone(),
                agent_id: None,
                after_agent: Some("agent-b".into()),
                limit: 1,
            },
        };
        let agent = AgentSummary {
            target: AgentTarget {
                export,
                agent_id: "agent-c".into(),
                run_id: "run".into(),
            },
            lifecycle: Lifecycle::Unknown,
            epoch: 1,
            revision: 1,
            controller: Controller::None,
            runtime: "codex".into(),
        };
        let mut response = Response {
            request_id: "request".into(),
            daemon_id: "worker".into(),
            body: Body::Status {
                agents: vec![agent.clone()],
                next_agent: None,
            },
        };
        assert!(Response::decode(&response.encode().unwrap(), &request).is_ok());
        if let Body::Status { agents, .. } = &mut response.body {
            agents.push(agent);
        }
        assert!(Response::decode(&serde_json::to_vec(&response).unwrap(), &request).is_err());
        if let Body::Status { agents, .. } = &mut response.body {
            agents.pop();
            agents[0].target.agent_id = "agent-a".into();
        }
        assert!(Response::decode(&response.encode().unwrap(), &request).is_err());
    }
    #[test]
    fn given_correlated_receipt_for_another_run_should_reject_before_host_reconciliation() {
        let target = AgentTarget {
            export: Export {
                id: "project".into(),
                generation: 1,
            },
            agent_id: "agent".into(),
            run_id: "run".into(),
        };
        let request = Request {
            request_id: "request".into(),
            target_daemon: "worker".into(),
            operation: super::super::wire::Operation::InputReceipt {
                target: target.clone(),
                writer: 4,
                sequence: 7,
            },
        };
        let evidence = InputEvidence {
            target,
            writer: 4,
            sequence: 7,
            digest: [1; 32],
            payload_bytes: 5,
            outcome: InputOutcome::Unknown,
        };
        let mut response = Response {
            request_id: "request".into(),
            daemon_id: "worker".into(),
            body: Body::InputReceipt {
                evidence: Some(evidence),
            },
        };
        assert!(Response::decode(&response.encode().unwrap(), &request).is_ok());
        if let Body::InputReceipt { evidence: Some(e) } = &mut response.body {
            e.target.run_id = "another-run".into();
        }
        assert!(Response::decode(&response.encode().unwrap(), &request).is_err());
    }
    #[test]
    fn given_remote_metadata_should_require_exact_receiver_request_and_bounded_shape() {
        let request = Request {
            request_id: "request".into(),
            target_daemon: "worker".into(),
            operation: super::super::wire::Operation::Inventory {},
        };
        let mut response = Response {
            request_id: "request".into(),
            daemon_id: "worker".into(),
            body: Body::Inventory { exports: vec![] },
        };
        assert_eq!(
            Response::decode(&response.encode().unwrap(), &request).unwrap(),
            response
        );
        response.daemon_id = "other".into();
        assert!(Response::decode(&response.encode().unwrap(), &request).is_err());
        response.daemon_id = "worker".into();
        response.request_id = "old".into();
        assert!(Response::decode(&response.encode().unwrap(), &request).is_err());
        response.request_id = "request".into();
        response.body = Body::Receipt { evidence: None };
        assert!(Response::decode(&response.encode().unwrap(), &request).is_err());
        let bytes=br#"{"request_id":"request","daemon_id":"worker","body":{"type":"inventory","exports":[],"path":"/private"}}"#;
        assert!(Response::decode(bytes, &request).is_err());
        response.body = Body::Inventory {
            exports: vec![
                ExportSummary {
                    export: Export {
                        id: "export".into(),
                        generation: 1
                    },
                    runtimes: vec![]
                };
                65
            ],
        };
        assert!(response.encode().is_err());
    }
}
