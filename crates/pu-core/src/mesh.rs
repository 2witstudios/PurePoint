//! Transport-independent, receiver-owned authorization for the LAN bridge.

pub mod response;
pub mod wire;

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

/// Individual permissions never imply a wildcard or onward delegation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Inspect,
    ReadOutput,
    Spawn,
    Control,
    Input,
    Stop,
    Suspend,
    Resume,
}

/// Receiver-owned grants name opaque exports, never caller-selected paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportGrant {
    pub export_id: String,
    pub capabilities: BTreeSet<Capability>,
    pub runtimes: BTreeSet<String>,
}

/// An authorization snapshot, subordinate to the execution ledger's live fence.
///
/// Transport must derive the peer from authenticated TLS, and the dispatcher
/// must hold its authority barrier through the side effect. This value alone
/// neither authenticates a caller nor permits a PTY write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerPolicy {
    pub peer_id: String,
    pub generation: u64,
    pub revoked: bool,
    pub expires_at: Option<i64>,
    pub exports: Vec<ExportGrant>,
}

/// Exhaustive operation classification keeps new operations from acquiring
/// permissions merely because they were added to the local IPC enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action<'a> {
    Inspect,
    ReadOutput,
    Spawn { runtime: &'a str },
    AcquireControl,
    Input,
    Resize,
    Stop,
    Suspend,
    Resume { runtime: &'a str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationError {
    #[error("bridge operation is not authorized")]
    Denied,
    #[error("invalid receiver authorization policy")]
    InvalidPolicy,
}

/// Bounded identifier syntax shared by exports, principals and runtime names.
pub fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl PeerPolicy {
    pub fn validate(&self) -> Result<(), AuthorizationError> {
        if !valid_identifier(&self.peer_id)
            || self.generation == 0
            || self.exports.len() > 64
            || self.expires_at.is_some_and(|expiry| expiry < 0)
        {
            return Err(AuthorizationError::InvalidPolicy);
        }
        let mut exports = HashSet::new();
        for grant in &self.exports {
            if !valid_identifier(&grant.export_id)
                || !exports.insert(&grant.export_id)
                || grant.runtimes.len() > 32
                || grant.runtimes.iter().any(|name| !valid_identifier(name))
            {
                return Err(AuthorizationError::InvalidPolicy);
            }
        }
        Ok(())
    }

    fn require_active(
        &self,
        authenticated_peer: &str,
        generation: u64,
        now: i64,
    ) -> Result<(), AuthorizationError> {
        self.validate()?;
        if now < 0
            || self.peer_id != authenticated_peer
            || self.generation != generation
            || self.revoked
            || self.expires_at.is_some_and(|expiry| now >= expiry)
        {
            return Err(AuthorizationError::Denied);
        }
        Ok(())
    }

    /// Check a direct operation against one exact export and runtime.
    pub fn authorize(
        &self,
        authenticated_peer: &str,
        generation: u64,
        export_id: &str,
        action: &Action<'_>,
        now: i64,
    ) -> Result<&ExportGrant, AuthorizationError> {
        self.require_active(authenticated_peer, generation, now)?;
        let grant = self
            .exports
            .iter()
            .find(|grant| grant.export_id == export_id)
            .ok_or(AuthorizationError::Denied)?;
        let required: &[Capability] = match action {
            Action::Inspect => &[Capability::Inspect],
            Action::ReadOutput => &[Capability::ReadOutput],
            Action::Spawn { .. } => &[Capability::Spawn],
            Action::AcquireControl | Action::Resize => &[Capability::Control],
            Action::Input => &[Capability::Control, Capability::Input],
            Action::Stop => &[Capability::Control, Capability::Stop],
            Action::Suspend => &[Capability::Control, Capability::Suspend],
            Action::Resume { .. } => &[Capability::Control, Capability::Resume],
        };
        if required
            .iter()
            .any(|capability| !grant.capabilities.contains(capability))
        {
            return Err(AuthorizationError::Denied);
        }
        if let Action::Spawn { runtime } | Action::Resume { runtime } = action
            && !grant.runtimes.contains(*runtime)
        {
            return Err(AuthorizationError::Denied);
        }
        Ok(grant)
    }

    /// Discovery reveals only explicitly inspectable exports for this peer.
    pub fn visible_exports(
        &self,
        authenticated_peer: &str,
        generation: u64,
        now: i64,
    ) -> Result<Vec<&ExportGrant>, AuthorizationError> {
        self.require_active(authenticated_peer, generation, now)?;
        Ok(self
            .exports
            .iter()
            .filter(|grant| grant.capabilities.contains(&Capability::Inspect))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn policy() -> PeerPolicy {
        PeerPolicy {
            peer_id: "peer-host".into(),
            generation: 4,
            revoked: false,
            expires_at: Some(200),
            exports: vec![ExportGrant {
                export_id: "export-one".into(),
                capabilities: BTreeSet::from([Capability::Inspect, Capability::Spawn]),
                runtimes: BTreeSet::from(["codex".into()]),
            }],
        }
    }

    #[test]
    fn given_export_grant_should_require_current_authenticated_peer_and_exact_resource() {
        // given
        let policy = policy();
        // when / then
        assert!(
            policy
                .authorize("peer-host", 4, "export-one", &Action::Inspect, 100)
                .is_ok()
        );
        assert_eq!(
            policy.authorize("peer-other", 4, "export-one", &Action::Inspect, 100),
            Err(AuthorizationError::Denied)
        );
        assert_eq!(
            policy.authorize("peer-host", 3, "export-one", &Action::Inspect, 100),
            Err(AuthorizationError::Denied)
        );
        assert_eq!(
            policy.authorize("peer-host", 4, "export-other", &Action::Inspect, 100),
            Err(AuthorizationError::Denied)
        );
    }

    #[test]
    fn given_spawn_authority_should_require_selected_runtime_and_not_grant_output_or_control() {
        // given
        let policy = policy();
        // when / then
        assert!(
            policy
                .authorize(
                    "peer-host",
                    4,
                    "export-one",
                    &Action::Spawn { runtime: "codex" },
                    100
                )
                .is_ok()
        );
        for action in [
            Action::Spawn {
                runtime: "terminal",
            },
            Action::ReadOutput,
            Action::Input,
            Action::AcquireControl,
            Action::Stop,
            Action::Resume { runtime: "codex" },
        ] {
            assert_eq!(
                policy.authorize("peer-host", 4, "export-one", &action, 100),
                Err(AuthorizationError::Denied)
            );
        }
    }

    #[test]
    fn given_interactive_permissions_should_require_control_and_a_current_runtime_grant_for_resume()
    {
        // given
        let mut policy = policy();
        policy.exports[0].capabilities = BTreeSet::from([
            Capability::Input,
            Capability::Stop,
            Capability::Suspend,
            Capability::Resume,
        ]);
        // when / then
        for action in [
            Action::Input,
            Action::Stop,
            Action::Suspend,
            Action::Resume { runtime: "codex" },
        ] {
            assert_eq!(
                policy.authorize("peer-host", 4, "export-one", &action, 100),
                Err(AuthorizationError::Denied)
            );
        }
        policy.exports[0].capabilities.insert(Capability::Control);
        for action in [
            Action::Input,
            Action::Resize,
            Action::Stop,
            Action::Suspend,
            Action::Resume { runtime: "codex" },
        ] {
            assert!(
                policy
                    .authorize("peer-host", 4, "export-one", &action, 100)
                    .is_ok()
            );
        }
        assert_eq!(
            policy.authorize(
                "peer-host",
                4,
                "export-one",
                &Action::Resume { runtime: "other" },
                100
            ),
            Err(AuthorizationError::Denied)
        );
    }

    #[test]
    fn given_expired_or_revoked_policy_should_deny_operations_and_inventory() {
        // given
        let mut policy = policy();
        // when / then
        for now in [200, 201, -1] {
            assert_eq!(
                policy.authorize("peer-host", 4, "export-one", &Action::Inspect, now),
                Err(AuthorizationError::Denied)
            );
            assert_eq!(
                policy.visible_exports("peer-host", 4, now),
                Err(AuthorizationError::Denied)
            );
        }
        policy.revoked = true;
        assert_eq!(
            policy.authorize("peer-host", 4, "export-one", &Action::Inspect, 100),
            Err(AuthorizationError::Denied)
        );
        assert_eq!(
            policy.visible_exports("peer-host", 4, 100),
            Err(AuthorizationError::Denied)
        );
    }

    #[test]
    fn given_inventory_request_should_expose_only_inspectable_exports() {
        // given
        let mut policy = policy();
        policy.exports.push(ExportGrant {
            export_id: "hidden-export".into(),
            capabilities: BTreeSet::from([Capability::ReadOutput]),
            runtimes: BTreeSet::new(),
        });
        // when
        let visible = policy.visible_exports("peer-host", 4, 100).unwrap();
        // then
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].export_id, "export-one");
        assert_eq!(
            policy.visible_exports("peer-other", 4, 100),
            Err(AuthorizationError::Denied)
        );
    }

    #[test]
    fn given_ambiguous_or_path_shaped_policy_should_fail_closed() {
        // given
        let original = policy();
        let mut duplicate = original.clone();
        duplicate.exports.push(duplicate.exports[0].clone());
        let mut path = original.clone();
        path.exports[0].export_id = "../project".into();
        let mut runtime = original.clone();
        runtime.exports[0].runtimes.insert("/bin/sh -c".into());
        let mut unversioned = original.clone();
        unversioned.generation = 0;
        let mut large = original.clone();
        large.exports = (0..65)
            .map(|id| ExportGrant {
                export_id: format!("export-{id}"),
                ..original.exports[0].clone()
            })
            .collect();
        // when / then
        for invalid in [duplicate, path, runtime, unversioned, large] {
            assert_eq!(invalid.validate(), Err(AuthorizationError::InvalidPolicy));
            assert_eq!(
                invalid.visible_exports("peer-host", invalid.generation, 100),
                Err(AuthorizationError::InvalidPolicy)
            );
        }
    }

    #[test]
    fn given_unknown_capability_or_policy_field_should_reject_instead_of_widening_access() {
        // given
        let original = serde_json::to_value(policy()).unwrap();
        let mut wildcard = original.clone();
        wildcard["exports"][0]["capabilities"] = serde_json::json!(["all"]);
        let mut future = original.clone();
        future["allow_admin"] = serde_json::json!(true);
        let mut path = original;
        path["exports"][0]["project_root"] = serde_json::json!("/private/project");
        // when / then
        for invalid in [wildcard, future, path] {
            assert!(serde_json::from_value::<PeerPolicy>(invalid).is_err());
        }
    }
}
