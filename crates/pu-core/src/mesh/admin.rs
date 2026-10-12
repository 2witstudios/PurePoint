//! Owner-local administration payloads. These are never bridge wire operations.
use super::{ExportGrant, PeerPolicy};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// A specific interface; absence means remote listening is disabled.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListenConfig {
    pub bind_address: Option<SocketAddr>,
    pub bulk_bind_address: Option<SocketAddr>,
}

impl ListenConfig {
    pub fn validate(&self) -> bool {
        fn specific(address: SocketAddr) -> bool {
            let ip = address.ip();
            address.port() != 0
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !matches!(ip, std::net::IpAddr::V4(ip) if ip.is_broadcast())
                && !matches!(ip, std::net::IpAddr::V6(ip) if ip.to_ipv4_mapped().is_some_and(|ip| ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast()))
        }
        match (self.bind_address, self.bulk_bind_address) {
            (None, None) => true,
            (Some(control), Some(bulk)) => {
                specific(control)
                    && specific(bulk)
                    && control.ip() == bulk.ip()
                    && control.port() != bulk.port()
            }
            _ => false,
        }
    }
}

fn specific_address(address: SocketAddr) -> bool {
    let ip = match address.ip() {
        std::net::IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(address.ip(), Into::into),
        ip => ip,
    };
    address.port() != 0
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !matches!(ip, std::net::IpAddr::V4(ip) if ip.is_broadcast())
}

/// Host-side routing hint for one enrolled worker. Addresses never authenticate:
/// every connection still pins the worker certificate fingerprint, and the worker
/// must also be enrolled on this host (normally with no grants) to be reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRoute {
    pub fingerprint: String,
    pub daemon_id: String,
    pub label: String,
    pub control_address: SocketAddr,
    pub bulk_address: SocketAddr,
}

impl WorkerRoute {
    pub fn validate(&self) -> bool {
        valid_fingerprint(&self.fingerprint)
            && super::valid_identifier(&self.daemon_id)
            && valid_label(&self.label)
            && specific_address(self.control_address)
            && specific_address(self.bulk_address)
            && self.control_address.ip() == self.bulk_address.ip()
            && self.control_address.port() != self.bulk_address.port()
    }
}

/// Maximum routed workers per host; bounds the persisted configuration record.
pub const MAX_WORKER_ROUTES: usize = 64;

fn valid_label(label: &str) -> bool {
    !label.trim().is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerChange {
    Revoke,
    Regrant,
    AdvanceGeneration,
}

/// Manual enrollment requires an independently supplied fingerprint of exact DER.
/// No invitations, peer assertions of identity, executable overrides or proxies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshAdminRequest {
    IdentityInitialize {},
    IdentityShow {},
    EnrollManual {
        certificate_der: Vec<u8>,
        expected_fingerprint: String,
        grants: Vec<ExportGrant>,
        expires_at: Option<i64>,
    },
    PolicyUpdate {
        certificate_der: Vec<u8>,
        expected_fingerprint: String,
        expected_generation: u64,
        grants: Vec<ExportGrant>,
        expires_at: Option<i64>,
    },
    PeerList {
        after: Option<String>,
        limit: usize,
    },
    PeerChange {
        fingerprint: String,
        expected_generation: u64,
        change: PeerChange,
    },
    ExportRegister {
        project_root: String,
        label: String,
    },
    ExportList {},
    ExportRevoke {
        export_id: String,
    },
    ConfigShow {},
    ConfigSet {
        config: ListenConfig,
    },
    WorkerSet {
        route: WorkerRoute,
    },
    WorkerRemove {
        fingerprint: String,
    },
    WorkerList {},
    /// Worker-owned runs that still hold capacity on this receiver.
    RunList {},
    /// Owner takes control of a run, fencing any remote controller and writer.
    RunTakeover {
        agent_id: String,
    },
    /// Owner gives up control; a granted peer may acquire it again.
    RunRelease {
        agent_id: String,
    },
    /// Durable stop under local control; the guardian proves termination.
    RunStop {
        agent_id: String,
    },
    /// Explicit owner disposition of an Unknown run with no supervisor proof,
    /// after the owner has verified its processes are gone.
    RunResolve {
        run_id: String,
        evidence: String,
    },
}

/// Local view of one worker-owned run; receiver-local only, never bridged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStatus {
    pub agent_id: String,
    pub run_id: String,
    pub export: Option<String>,
    pub runtime: String,
    pub lifecycle: String,
    /// "none", "local" or "peer:<fingerprint>".
    pub controller: String,
    pub live: bool,
}

/// Exact SHA256 certificate fingerprint syntax, without prefixes or case aliases.
pub fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl MeshAdminRequest {
    /// Structural limits are checked again by the receiver, not only by the CLI.
    pub fn validate(&self) -> bool {
        fn enrollment(cert: &[u8], pin: &str, grants: &[ExportGrant], expiry: Option<i64>) -> bool {
            !cert.is_empty()
                && cert.len() <= 16 * 1024
                && valid_fingerprint(pin)
                && PeerPolicy {
                    peer_id: pin.into(),
                    generation: 1,
                    revoked: false,
                    expires_at: expiry,
                    exports: grants.to_vec(),
                }
                .validate()
                .is_ok()
        }
        match self {
            Self::EnrollManual {
                certificate_der,
                expected_fingerprint,
                grants,
                expires_at,
            } => enrollment(certificate_der, expected_fingerprint, grants, *expires_at),
            Self::PolicyUpdate {
                certificate_der,
                expected_fingerprint,
                expected_generation,
                grants,
                expires_at,
            } => {
                *expected_generation > 0
                    && enrollment(certificate_der, expected_fingerprint, grants, *expires_at)
            }
            Self::PeerList { after, limit } => {
                (1..=256).contains(limit) && after.as_ref().is_none_or(|id| valid_fingerprint(id))
            }
            Self::PeerChange {
                fingerprint,
                expected_generation,
                ..
            } => valid_fingerprint(fingerprint) && *expected_generation > 0,
            Self::ExportRegister {
                project_root,
                label,
            } => {
                project_root.len() <= 4096
                    && !project_root.contains('\0')
                    && std::path::Path::new(project_root).is_absolute()
                    && valid_label(label)
            }
            Self::ExportRevoke { export_id } => super::valid_identifier(export_id),
            Self::ConfigSet { config } => config.validate(),
            Self::WorkerSet { route } => route.validate(),
            Self::WorkerRemove { fingerprint } => valid_fingerprint(fingerprint),
            Self::RunTakeover { agent_id }
            | Self::RunRelease { agent_id }
            | Self::RunStop { agent_id } => super::valid_identifier(agent_id),
            Self::RunResolve { run_id, evidence } => {
                super::valid_identifier(run_id)
                    && !evidence.trim().is_empty()
                    && evidence.len() <= 512
                    && !evidence.chars().any(char::is_control)
            }
            _ => true,
        }
    }
}

/// Public material only. Private keys never occur in an administrative response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicIdentity {
    pub daemon_id: String,
    pub certificate_der: Vec<u8>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerStatus {
    pub fingerprint: String,
    pub generation: u64,
    pub revoked: bool,
    pub authorization_revision: u64,
    pub policy: PeerPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportStatus {
    pub id: String,
    pub generation: u64,
    pub label: String,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshAdminResponse {
    Identity {
        identity: PublicIdentity,
    },
    Peer {
        peer: PeerStatus,
    },
    Peers {
        peers: Vec<PeerStatus>,
        next_after: Option<String>,
    },
    Export {
        export: ExportStatus,
    },
    Exports {
        exports: Vec<ExportStatus>,
    },
    Config {
        config: ListenConfig,
        revision: u64,
    },
    Workers {
        workers: Vec<WorkerRoute>,
    },
    Runs {
        runs: Vec<RunStatus>,
    },
    Done {},
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn given_listener_configuration_should_require_specific_interface_and_nonzero_port() {
        // given / when / then
        assert!(ListenConfig::default().validate());
        for address in [
            "0.0.0.0:8443",
            "[::]:8443",
            "224.0.0.1:8443",
            "[ff02::1]:8443",
            "127.0.0.1:0",
        ] {
            assert!(
                !ListenConfig {
                    bind_address: Some(address.parse().unwrap()),
                    bulk_bind_address: Some("127.0.0.1:8444".parse().unwrap())
                }
                .validate()
            );
        }
        assert!(
            ListenConfig {
                bind_address: Some("127.0.0.1:8443".parse().unwrap()),
                bulk_bind_address: Some("127.0.0.1:8444".parse().unwrap())
            }
            .validate()
        );
    }
    #[test]
    fn given_local_admin_payload_should_roundtrip_ipc_and_reject_bridge_proxy_or_unknown_fields() {
        // given
        let request = MeshAdminRequest::ConfigSet {
            config: ListenConfig::default(),
        };
        let ipc = crate::protocol::Request::MeshAdmin {
            request: request.clone(),
        };
        let encoded = serde_json::to_vec(&ipc).unwrap();
        // when / then
        match serde_json::from_slice::<crate::protocol::Request>(&encoded).unwrap() {
            crate::protocol::Request::MeshAdmin { request: decoded } => {
                assert_eq!(decoded, request)
            }
            _ => panic!("wrong local request"),
        }
        assert!(serde_json::from_slice::<super::super::wire::Request>(&encoded).is_err());
        assert!(
            serde_json::from_str::<MeshAdminRequest>(
                r#"{"operation":"identity_show","remote_admin":true}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<MeshAdminRequest>(
                r#"{"operation":"invitation","secret":"anything"}"#
            )
            .is_err()
        );
        let response = MeshAdminResponse::Config {
            config: ListenConfig::default(),
            revision: 1,
        };
        let encoded = serde_json::to_vec(&crate::protocol::Response::MeshAdmin {
            response: response.clone(),
        })
        .unwrap();
        match serde_json::from_slice::<crate::protocol::Response>(&encoded).unwrap() {
            crate::protocol::Response::MeshAdmin { response: decoded } => {
                assert_eq!(decoded, response)
            }
            _ => panic!("wrong local response"),
        }
    }

    #[test]
    fn given_oversized_or_ambiguous_admin_request_should_reject_before_mutation() {
        // given / when / then
        for (after, limit) in [(None, 0), (None, 257), (Some("../peer".into()), 1)] {
            assert!(!MeshAdminRequest::PeerList { after, limit }.validate());
        }
        assert!(
            !MeshAdminRequest::EnrollManual {
                certificate_der: vec![0; 16 * 1024 + 1],
                expected_fingerprint: "a".repeat(64),
                grants: vec![],
                expires_at: None,
            }
            .validate()
        );
        for pin in ["A".repeat(64), "a".repeat(63), "--help".into()] {
            assert!(
                !MeshAdminRequest::PeerChange {
                    fingerprint: pin,
                    expected_generation: 1,
                    change: PeerChange::Regrant
                }
                .validate()
            );
        }
    }
    #[test]
    fn given_worker_route_should_require_pin_identity_and_specific_paired_addresses() {
        // given
        let mut route = WorkerRoute {
            fingerprint: "a".repeat(64),
            daemon_id: "worker".into(),
            label: "studio".into(),
            control_address: "192.168.1.20:8443".parse().unwrap(),
            bulk_address: "192.168.1.20:8444".parse().unwrap(),
        };
        // when / then
        assert!(
            MeshAdminRequest::WorkerSet {
                route: route.clone()
            }
            .validate()
        );
        for (control, bulk) in [
            ("0.0.0.0:8443", "0.0.0.0:8444"),
            ("[::ffff:0.0.0.0]:8443", "[::ffff:0.0.0.0]:8444"),
            ("192.168.1.20:8443", "192.168.1.21:8444"),
            ("192.168.1.20:8443", "192.168.1.20:8443"),
            ("224.0.0.1:8443", "224.0.0.1:8444"),
        ] {
            route.control_address = control.parse().unwrap();
            route.bulk_address = bulk.parse().unwrap();
            assert!(!route.validate(), "{control} {bulk}");
        }
        route.control_address = "192.168.1.20:8443".parse().unwrap();
        route.bulk_address = "192.168.1.20:8444".parse().unwrap();
        route.fingerprint = "A".repeat(64);
        assert!(!route.validate());
        route.fingerprint = "a".repeat(64);
        route.daemon_id = "../worker".into();
        assert!(!route.validate());
        assert!(
            !MeshAdminRequest::WorkerRemove {
                fingerprint: "x".into()
            }
            .validate()
        );
    }

    #[test]
    fn given_paired_listener_config_should_require_same_specific_ip_and_distinct_explicit_ports() {
        // given / when / then
        for (control, bulk) in [
            (Some("127.0.0.1:8443"), None),
            (None, Some("127.0.0.1:8444")),
            (Some("127.0.0.1:8443"), Some("127.0.0.1:8443")),
            (Some("127.0.0.1:8443"), Some("127.0.0.2:8444")),
            (Some("127.0.0.1:8443"), Some("127.0.0.1:0")),
        ] {
            assert!(
                !ListenConfig {
                    bind_address: control.map(|s| s.parse().unwrap()),
                    bulk_bind_address: bulk.map(|s| s.parse().unwrap())
                }
                .validate()
            );
        }
    }
}
