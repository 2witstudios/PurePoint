//! Restricted bridge protocol; deliberately independent of local IPC.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::valid_identifier;

pub const BRIDGE_VERSION: u32 = 1;
pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
pub const MAX_BULK_FRAME: usize = 256 * 1024;
pub const MAX_PROMPT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("invalid bridge request")]
    Invalid,
    #[error("bridge protocol version mismatch")]
    Version,
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
    Spawn {
        mutation: Mutation,
        export: Export,
        runtime: String,
        source_commit: String,
        prompt: String,
    },
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
}
