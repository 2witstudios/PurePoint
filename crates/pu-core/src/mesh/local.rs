//! Trusted local routing and applied listener state. Never accepted as bridge
//! wire operations or remote administrative capabilities.
use super::{
    admin::{ListenConfig, valid_fingerprint},
    wire::Request,
};
use serde::{Deserialize, Serialize};

/// Local owner request to forward one bridge request to a routed worker. The
/// worker is selected by its pinned certificate fingerprint only; the certificate,
/// daemon identity and addresses come from this host's own enrolled state, never
/// from the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRoute {
    pub worker: String,
    pub request: Request,
}

impl LocalRoute {
    pub fn validate(&self) -> bool {
        valid_fingerprint(&self.worker) && self.request.validate().is_ok()
    }
}

/// Applied listener state, distinct from persisted configuration intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenerStatus {
    NotInitialized,
    Disabled,
    /// Configured, but every enrolled peer is revoked or expired.
    NoActivePeers,
    Listening {
        config: ListenConfig,
        revision: u64,
    },
    Failed {
        reason: ListenerFailure,
    },
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerFailure {
    State,
    Bind,
    Service,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::wire::Operation;

    #[test]
    fn given_local_route_should_require_pinned_worker_and_valid_request() {
        // given
        let mut route = LocalRoute {
            worker: "a".repeat(64),
            request: Request {
                request_id: "query".into(),
                target_daemon: "worker".into(),
                operation: Operation::Inventory {},
            },
        };
        // when / then
        assert!(route.validate());
        route.worker = "studio.local".into();
        assert!(!route.validate());
        route.worker = "a".repeat(64);
        route.request.target_daemon = "../worker".into();
        assert!(!route.validate());
        assert!(
            serde_json::from_str::<LocalRoute>(
                r#"{"worker":"x","request":{},"certificate_der":[1]}"#
            )
            .is_err()
        );
    }
}
