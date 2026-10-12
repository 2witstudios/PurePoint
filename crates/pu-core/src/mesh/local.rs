//! Trusted local routing and applied listener state. Never accepted as bridge
//! wire operations or remote administrative capabilities.
use super::{
    admin::{ListenConfig, valid_fingerprint},
    wire::{Operation, Request},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRoute {
    pub receiver_certificate_der: Vec<u8>,
    pub expected_fingerprint: String,
    pub control_address: SocketAddr,
    pub request: Request,
}
impl ReadRoute {
    pub fn validate(&self) -> bool {
        let ip = match self.control_address.ip() {
            std::net::IpAddr::V6(ip) => ip
                .to_ipv4_mapped()
                .map_or(self.control_address.ip(), Into::into),
            ip => ip,
        };
        !self.receiver_certificate_der.is_empty()
            && self.receiver_certificate_der.len() <= 16 * 1024
            && valid_fingerprint(&self.expected_fingerprint)
            && self.control_address.port() != 0
            && !ip.is_unspecified()
            && !ip.is_multicast()
            && !matches!(ip,std::net::IpAddr::V4(ip) if ip.is_broadcast())
            && self.request.validate().is_ok()
            && matches!(
                self.request.operation,
                Operation::Inventory {}
                    | Operation::Status { .. }
                    | Operation::Receipt { .. }
                    | Operation::InputReceipt { .. }
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenerStatus {
    NotInitialized,
    Disabled,
    Listening { config: ListenConfig, revision: u64 },
    Failed { reason: ListenerFailure },
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
    #[test]
    fn given_local_read_route_should_reject_effects_and_implicit_targets() {
        let mut route = ReadRoute {
            receiver_certificate_der: vec![1],
            expected_fingerprint: "a".repeat(64),
            control_address: "127.0.0.1:7777".parse().unwrap(),
            request: Request {
                request_id: "query".into(),
                target_daemon: "worker".into(),
                operation: Operation::Inventory {},
            },
        };
        assert!(route.validate());
        route.request.operation = Operation::IssueWindow { ttl_seconds: 60 };
        assert!(!route.validate());
        route.request.operation = Operation::Inventory {};
        for address in [
            "0.0.0.0:7777",
            "[::ffff:0.0.0.0]:7777",
            "224.0.0.1:7777",
            "127.0.0.1:0",
        ] {
            route.control_address = address.parse().unwrap();
            assert!(!route.validate());
        }
    }
}
