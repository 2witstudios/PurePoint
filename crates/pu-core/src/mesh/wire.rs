//! Restricted bridge protocol; deliberately independent of local IPC.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::valid_identifier;

pub const BRIDGE_VERSION: u32 = 1;
pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
pub const MAX_BULK_FRAME: usize = 256 * 1024;
pub const MAX_PROMPT_BYTES: usize = 32 * 1024;
pub const MAX_INPUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("invalid bridge request")]
    Invalid,
    #[error("bridge protocol version mismatch")]
    Version,
}

/// The TLS-authenticated peer is implicit. Bulk sessions additionally name a
/// live control binding; the server must check it against current authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum Hello {
    /// Short-lived authenticated restart-generation discovery, not a session.
    Discover { version: u32, target_daemon: String },
    Control {
        version: u32,
        target_daemon: String,
        generation: u64,
    },
    Bulk {
        version: u32,
        target_daemon: String,
        generation: u64,
        incarnation: String,
        control_binding: String,
    },
}

impl Hello {
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.is_empty() || bytes.len() > 2048 {
            return Err(WireError::Invalid);
        }
        let hello: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        let (version, target_daemon, generation) = match &hello {
            Self::Discover {
                version,
                target_daemon,
            } => (version, target_daemon, None),
            Self::Control {
                version,
                target_daemon,
                generation,
            } => (version, target_daemon, Some(generation)),
            Self::Bulk {
                version,
                target_daemon,
                generation,
                incarnation,
                control_binding,
            } => {
                require(valid_identifier(incarnation) && valid_identifier(control_binding))?;
                (version, target_daemon, Some(generation))
            }
        };
        if *version != BRIDGE_VERSION {
            return Err(WireError::Version);
        }
        require(
            valid_identifier(target_daemon)
                && generation.is_none_or(|generation| counter(*generation)),
        )?;
        Ok(hello)
    }
}

/// Receiver public identity/version/current generation only. This is a reconnect
/// hint for a NEW strict control handshake, never a session or mutation authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationDiscovery {
    pub version: u32,
    pub daemon_id: String,
    pub generation: u64,
}
impl GenerationDiscovery {
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.is_empty() || bytes.len() > 2048 {
            return Err(WireError::Invalid);
        }
        let response: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        if response.version != BRIDGE_VERSION {
            return Err(WireError::Version);
        }
        require(valid_identifier(&response.daemon_id) && counter(response.generation))?;
        Ok(response)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    pub version: u32,
    pub daemon_id: String,
    pub generation: u64,
    pub incarnation: String,
    pub control_binding: String,
}

impl Welcome {
    /// Validate structure before comparing it to the client's expected receiver,
    /// generation and (for a bulk connection) existing control binding.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.is_empty() || bytes.len() > 2048 {
            return Err(WireError::Invalid);
        }
        let welcome: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        if welcome.version != BRIDGE_VERSION {
            return Err(WireError::Version);
        }
        require(
            valid_identifier(&welcome.daemon_id)
                && valid_identifier(&welcome.incarnation)
                && valid_identifier(&welcome.control_binding)
                && counter(welcome.generation),
        )?;
        Ok(welcome)
    }
}

/// No peer identity, authorization policy or paths can be supplied by a request.
/// The receiver derives its caller from the authenticated connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub request_id: String,
    pub target_daemon: String,
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Inventory {},
    Status {
        export: Export,
        agent_id: Option<String>,
        #[serde(default)]
        after_agent: Option<String>,
        #[serde(default = "default_status_limit")]
        limit: u16,
    },
    IssueWindow {
        ttl_seconds: u32,
    },
    CloseWindow {
        window: u64,
    },
    Receipt {
        export: Export,
        mutation: Mutation,
        /// Original operation digest, solely for matching retained evidence.
        /// New mutations always compute their digest from canonical request data.
        payload_digest: [u8; 32],
    },
    Spawn {
        mutation: Mutation,
        export: Export,
        runtime: String,
        source_commit: String,
        prompt: String,
    },
    AcquireControl {
        guard: ControlGuard,
        mutation: Mutation,
    },
    Stop {
        guard: ControlGuard,
        mutation: Mutation,
    },
    Suspend {
        guard: ControlGuard,
        mutation: Mutation,
    },
    Resume {
        guard: ControlGuard,
        mutation: Mutation,
        runtime: String,
        source_commit: String,
    },
    OpenWriter {
        guard: ControlGuard,
        previous: Option<WriterCheckpoint>,
    },
    Input {
        guard: ControlGuard,
        writer: u64,
        sequence: u64,
        data: Vec<u8>,
    },
    InputReceipt {
        target: AgentTarget,
        writer: u64,
        sequence: u64,
    },
    Resize {
        guard: ControlGuard,
        mutation: Mutation,
        cols: u16,
        rows: u16,
    },
    SubscribeOutput {
        target: AgentTarget,
        cursor: Option<OutputCursor>,
    },
    ReadConversation {
        target: AgentTarget,
        limit: u16,
        cursor: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTarget {
    pub export: Export,
    pub agent_id: String,
    pub run_id: String,
}

impl AgentTarget {
    fn validate(&self) -> Result<(), WireError> {
        self.export.validate()?;
        require(valid_identifier(&self.agent_id) && valid_identifier(&self.run_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlGuard {
    pub target: AgentTarget,
    pub epoch: u64,
    pub revision: u64,
}

impl ControlGuard {
    fn validate(&self) -> Result<(), WireError> {
        self.target.validate()?;
        require(counter(self.epoch) && counter(self.revision))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterCheckpoint {
    pub writer: u64,
    pub last_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputCursor {
    pub incarnation: String,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutation {
    pub window: u64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub id: String,
    pub generation: u64,
}

impl Export {
    fn validate(&self) -> Result<(), WireError> {
        require(valid_identifier(&self.id) && counter(self.generation))
    }
}

impl Mutation {
    fn validate(&self) -> Result<(), WireError> {
        require(valid_identifier(&self.id) && counter(self.window))
    }
}

fn default_status_limit() -> u16 {
    64
}

fn require(condition: bool) -> Result<(), WireError> {
    if condition {
        Ok(())
    } else {
        Err(WireError::Invalid)
    }
}

fn counter(value: u64) -> bool {
    value > 0 && value <= i64::MAX as u64
}

fn exact_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Request {
    /// Reject over-budget input before JSON allocation; unknown fields fail closed.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.is_empty() || bytes.len() > MAX_CONTROL_FRAME {
            return Err(WireError::Invalid);
        }
        let request: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), WireError> {
        if !valid_identifier(&self.request_id) || !valid_identifier(&self.target_daemon) {
            return Err(WireError::Invalid);
        }
        match &self.operation {
            Operation::Inventory {} => Ok(()),
            Operation::Status {
                export,
                agent_id,
                after_agent,
                limit,
            } => {
                export.validate()?;
                require(
                    agent_id.as_deref().is_none_or(valid_identifier)
                        && after_agent.as_deref().is_none_or(valid_identifier)
                        && (1..=64).contains(limit)
                        && (agent_id.is_none() || after_agent.is_none()),
                )
            }
            Operation::IssueWindow { ttl_seconds } => require((1..=86400).contains(ttl_seconds)),
            Operation::CloseWindow { window } => require(counter(*window)),
            Operation::Receipt {
                export, mutation, ..
            } => {
                export.validate()?;
                mutation.validate()
            }
            Operation::Spawn {
                mutation,
                export,
                runtime,
                source_commit,
                prompt,
            } => {
                mutation.validate()?;
                export.validate()?;
                require(
                    valid_identifier(runtime)
                        && exact_commit(source_commit)
                        && prompt.len() <= MAX_PROMPT_BYTES
                        && !prompt.contains('\0'),
                )
            }
            Operation::AcquireControl { guard, mutation }
            | Operation::Stop { guard, mutation }
            | Operation::Suspend { guard, mutation } => {
                guard.validate()?;
                mutation.validate()
            }
            Operation::Resume {
                guard,
                mutation,
                runtime,
                source_commit,
            } => {
                guard.validate()?;
                mutation.validate()?;
                require(valid_identifier(runtime) && exact_commit(source_commit))
            }
            Operation::OpenWriter { guard, previous } => {
                guard.validate()?;
                require(previous.as_ref().is_none_or(|checkpoint| {
                    counter(checkpoint.writer) && checkpoint.last_sequence <= i64::MAX as u64
                }))
            }
            Operation::Input {
                guard,
                writer,
                sequence,
                data,
            } => {
                guard.validate()?;
                require(
                    counter(*writer)
                        && counter(*sequence)
                        && !data.is_empty()
                        && data.len() <= MAX_INPUT_BYTES,
                )
            }
            Operation::InputReceipt {
                target,
                writer,
                sequence,
            } => {
                target.validate()?;
                require(counter(*writer) && counter(*sequence))
            }
            Operation::Resize {
                guard,
                mutation,
                cols,
                rows,
            } => {
                guard.validate()?;
                mutation.validate()?;
                require((1..=4096).contains(cols) && (1..=4096).contains(rows))
            }
            Operation::SubscribeOutput { target, cursor } => {
                target.validate()?;
                require(
                    cursor
                        .as_ref()
                        .is_none_or(|cursor| valid_identifier(&cursor.incarnation)),
                )
            }
            Operation::ReadConversation {
                target,
                limit,
                cursor,
            } => {
                target.validate()?;
                require(
                    (1..=200).contains(limit)
                        && cursor.as_ref().is_none_or(|cursor| {
                            !cursor.is_empty()
                                && cursor.len() <= 4096
                                && !cursor.chars().any(char::is_control)
                        }),
                )
            }
        }
    }

    /// Canonical typed encoding, versioned and receiver-scoped. Correlation IDs
    /// are excluded so reconnect reconciliation preserves the operation identity.
    /// Never accept a digest supplied by a remote peer in place of this value.
    pub fn payload_digest(&self) -> Result<[u8; 32], WireError> {
        self.validate()?;
        let canonical = serde_json::to_vec(&(
            "purepoint-bridge-operation-v1",
            &self.target_daemon,
            &self.operation,
        ))
        .map_err(|_| WireError::Invalid)?;
        Ok(Sha256::digest(canonical).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_status_pages_should_default_to_small_bound_and_reject_ambiguous_or_invalid_cursors() {
        let base = serde_json::json!({"request_id":"status","target_daemon":"worker","operation":{"type":"status","export":{"id":"project","generation":1},"agent_id":null}});
        let decoded = Request::decode(&serde_json::to_vec(&base).unwrap()).unwrap();
        assert!(matches!(
            decoded.operation,
            Operation::Status {
                limit: 64,
                after_agent: None,
                ..
            }
        ));
        let mut page = base.clone();
        page["operation"]["after_agent"] = "agent-previous".into();
        page["operation"]["limit"] = 1.into();
        assert!(Request::decode(&serde_json::to_vec(&page).unwrap()).is_ok());
        for limit in [0, 65] {
            let mut invalid = page.clone();
            invalid["operation"]["limit"] = limit.into();
            assert!(Request::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        page["operation"]["agent_id"] = "agent-selected".into();
        assert!(Request::decode(&serde_json::to_vec(&page).unwrap()).is_err());
        page["operation"]["agent_id"] = serde_json::Value::Null;
        page["operation"]["after_agent"] = "../agent".into();
        assert!(Request::decode(&serde_json::to_vec(&page).unwrap()).is_err());
        let mut selected = base;
        selected["operation"]["agent_id"] = "agent-selected".into();
        assert!(Request::decode(&serde_json::to_vec(&selected).unwrap()).is_ok());
    }

    #[test]
    fn given_generation_discovery_should_be_bounded_targeted_and_carry_no_binding_or_resource_authority()
     {
        // given: a discovery request needs a receiver target, never an old generation
        let hello = br#"{"role":"discover","version":1,"target_daemon":"worker"}"#;
        assert!(matches!(
            Hello::decode(hello).unwrap(),
            Hello::Discover { .. }
        ));
        let response = br#"{"version":1,"daemon_id":"worker","generation":3}"#;
        assert_eq!(GenerationDiscovery::decode(response).unwrap().generation, 3);
        // when / then: discovery cannot carry sender identity, binding or policy
        for bytes in [
            br#"{"role":"discover","version":1,"target_daemon":"worker","generation":2}"#
                .as_slice(),
            br#"{"role":"discover","version":1,"target_daemon":"worker","peer_id":"claimed"}"#
                .as_slice(),
            br#"{"role":"discover","version":2,"target_daemon":"worker"}"#.as_slice(),
            br#"{"role":"discover","version":1,"target_daemon":"../worker"}"#.as_slice(),
        ] {
            assert!(Hello::decode(bytes).is_err());
        }
        for bytes in [
            br#"{"version":1,"daemon_id":"worker","generation":3,"control_binding":"borrowed"}"#
                .as_slice(),
            br#"{"version":1,"daemon_id":"worker","generation":3,"exports":[]}"#.as_slice(),
            br#"{"version":1,"daemon_id":"worker","generation":0}"#.as_slice(),
            br#"{"version":2,"daemon_id":"worker","generation":3}"#.as_slice(),
        ] {
            assert!(GenerationDiscovery::decode(bytes).is_err());
        }
        assert!(GenerationDiscovery::decode(&vec![b' '; 2049]).is_err());
    }

    #[test]
    fn given_receipt_lookup_should_require_original_digest_without_accepting_asserted_mutation_digests()
     {
        // given
        let mut receipt = serde_json::json!({"request_id":"lookup","target_daemon":"worker","operation":{"type":"receipt","export":{"id":"project","generation":1},"mutation":{"window":1,"id":"operation"},"payload_digest":vec![7u8;32]}});
        // when / then
        assert!(Request::decode(&serde_json::to_vec(&receipt).unwrap()).is_ok());
        receipt["operation"]["payload_digest"] = serde_json::json!([7, 7]);
        assert!(Request::decode(&serde_json::to_vec(&receipt).unwrap()).is_err());
        receipt["operation"]
            .as_object_mut()
            .unwrap()
            .remove("payload_digest");
        assert!(Request::decode(&serde_json::to_vec(&receipt).unwrap()).is_err());
        let mutation = serde_json::json!({"request_id":"spawn","target_daemon":"worker","operation":{"type":"spawn","export":{"id":"project","generation":1},"mutation":{"window":1,"id":"operation"},"runtime":"codex","source_commit":"a".repeat(40),"prompt":"work","payload_digest":vec![7u8;32]}});
        assert!(Request::decode(&serde_json::to_vec(&mutation).unwrap()).is_err());
    }

    #[test]
    fn given_welcome_should_validate_receiver_binding_before_client_uses_it() {
        // given
        let mut value = serde_json::json!({"version":1,"daemon_id":"worker","generation":2,"incarnation":"boot","control_binding":"binding"});
        // when / then
        assert!(Welcome::decode(&serde_json::to_vec(&value).unwrap()).is_ok());
        for field in ["daemon_id", "incarnation", "control_binding"] {
            let mut changed = value.clone();
            changed[field] = "../invalid".into();
            assert!(Welcome::decode(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        value["version"] = 2.into();
        assert_eq!(
            Welcome::decode(&serde_json::to_vec(&value).unwrap()),
            Err(WireError::Version)
        );
        value["version"] = 1.into();
        value["generation"] = 0.into();
        assert!(Welcome::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn given_network_request_should_reject_admin_forwarding_and_claimed_principal() {
        // given / when / then
        let valid = br#"{"request_id":"req-1","target_daemon":"worker-1","operation":{"type":"inventory"}}"#;
        assert!(Request::decode(valid).is_ok());
        for bytes in [
            br#"{"request_id":"req-1","target_daemon":"worker-1","operation":{"type":"shutdown"}}"#.as_slice(),
            br#"{"request_id":"req-1","target_daemon":"worker-1","caller":"admin","operation":{"type":"inventory"}}"#.as_slice(),
            br#"{"request_id":"req-1","target_daemon":"worker-1","operation":{"type":"inventory","project_root":"/private/project"}}"#.as_slice(),
        ] {
            assert!(Request::decode(bytes).is_err());
        }
    }

    #[test]
    fn given_spawn_should_bind_exact_source_and_receiver_export_in_canonical_digest() {
        // given
        let bytes = br#"{"request_id":"req-1","target_daemon":"worker-1","operation":{"type":"spawn","mutation":{"window":1,"id":"op-1"},"export":{"id":"export-1","generation":2},"runtime":"codex","source_commit":"1111111111111111111111111111111111111111","prompt":"do work"}}"#;
        // when
        let first = Request::decode(bytes).unwrap();
        let mut duplicate = first.clone();
        duplicate.request_id = "req-2".into();
        // then: correlation identifiers may change on receipt reconciliation.
        assert_eq!(
            first.payload_digest().unwrap(),
            duplicate.payload_digest().unwrap()
        );
        duplicate.target_daemon = "worker-2".into();
        assert_ne!(
            first.payload_digest().unwrap(),
            duplicate.payload_digest().unwrap()
        );
        let mut invalid = serde_json::from_slice::<serde_json::Value>(bytes).unwrap();
        for (field, value) in [("source_commit", "main"), ("runtime", "/bin/sh")] {
            let mut changed = invalid.clone();
            changed["operation"][field] = value.into();
            assert!(Request::decode(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        invalid["operation"]["command"] = "arbitrary override".into();
        assert!(Request::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }

    #[test]
    fn given_bulk_handshake_should_require_control_binding_and_current_version() {
        // given
        let bytes = br#"{"role":"bulk","version":1,"target_daemon":"worker-1","generation":4,"incarnation":"boot-1","control_binding":"binding-1"}"#;
        // when / then
        assert!(Hello::decode(bytes).is_ok());
        let original: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        for field in ["control_binding", "incarnation"] {
            let mut value = original.clone();
            value.as_object_mut().unwrap().remove(field);
            assert!(Hello::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut value = original.clone();
        value["version"] = 2.into();
        assert_eq!(
            Hello::decode(&serde_json::to_vec(&value).unwrap()),
            Err(WireError::Version)
        );
        value = original;
        value["role"] = "control".into();
        assert!(Hello::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn given_interactive_input_should_require_exact_run_control_guard_and_bounded_bytes() {
        // given
        let mut request = Request {
            request_id: "request".into(),
            target_daemon: "worker".into(),
            operation: Operation::Input {
                guard: ControlGuard {
                    target: AgentTarget {
                        export: Export {
                            id: "export".into(),
                            generation: 1,
                        },
                        agent_id: "agent".into(),
                        run_id: "run".into(),
                    },
                    epoch: 1,
                    revision: 1,
                },
                writer: 1,
                sequence: 1,
                data: vec![0, 255, 13],
            },
        };
        // when / then
        assert!(request.validate().is_ok());
        let original_digest = request.payload_digest().unwrap();
        let Operation::Input { data, .. } = &mut request.operation else {
            unreachable!()
        };
        data.push(10);
        assert_ne!(original_digest, request.payload_digest().unwrap());
        let Operation::Input { guard, .. } = &mut request.operation else {
            unreachable!()
        };
        guard.epoch = 0;
        assert!(request.validate().is_err());
        let Operation::Input { guard, data, .. } = &mut request.operation else {
            unreachable!()
        };
        guard.epoch = 1;
        *data = vec![0; MAX_INPUT_BYTES + 1];
        assert!(request.validate().is_err());
    }

    #[test]
    fn given_resume_or_bulk_observation_should_reject_unscoped_or_unbounded_requests() {
        // given
        let bytes = br#"{"request_id":"req","target_daemon":"worker","operation":{"type":"resume","mutation":{"window":1,"id":"op"},"guard":{"target":{"export":{"id":"export","generation":1},"agent_id":"agent","run_id":"run"},"epoch":2,"revision":3},"runtime":"codex","source_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
        // when / then
        assert!(Request::decode(bytes).is_ok());
        let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        value["operation"]["guard"]["target"]["run_id"] = "".into();
        assert!(Request::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(Request::decode(&vec![b' '; MAX_CONTROL_FRAME + 1]).is_err());
        assert!(Request::decode(br#"{"request_id":"req","target_daemon":"worker","operation":{"type":"stop","target":"all"}}"#).is_err());
    }
}
