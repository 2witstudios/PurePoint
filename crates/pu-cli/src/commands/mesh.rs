//! Explicit owner-local administration. Enrollment is manual out-of-band only.
use crate::error::CliError;
use crate::{client, daemon_ctrl, output};
use clap::Subcommand;
use pu_core::mesh::admin::{ListenConfig, MeshAdminRequest, PeerChange, WorkerRoute};
use pu_core::protocol::{Request, Response};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Subcommand)]
pub enum MeshAction {
    /// Initialize/show this receiver's public identity (never private keys)
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },
    /// Manual OOB enrollment: verify this pin independently before entering it
    EnrollManual {
        #[arg(long)]
        certificate_file: PathBuf,
        #[arg(long)]
        expected_fingerprint: String,
        /// JSON array of receiver-issued exports, explicit capabilities and runtime IDs
        #[arg(long)]
        grants_file: PathBuf,
        /// Expiry as Unix seconds; omit only for a deliberate non-expiring grant
        #[arg(long)]
        expires_at: Option<i64>,
    },
    /// Replace grants; does not implicitly regrant a revoked peer
    PolicyUpdate {
        #[arg(long)]
        certificate_file: PathBuf,
        #[arg(long)]
        expected_fingerprint: String,
        #[arg(long)]
        expected_generation: u64,
        #[arg(long)]
        grants_file: PathBuf,
        #[arg(long)]
        expires_at: Option<i64>,
    },
    /// Bounded local policy page, including revoked and expired peers
    Peers {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 64)]
        limit: usize,
    },
    Peer {
        #[command(subcommand)]
        action: PeerAction,
    },
    Export {
        #[command(subcommand)]
        action: ExportAction,
    },
    /// Persist specific-interface listening configuration; default is disabled
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Host-side routes to enrolled workers (addresses are hints, never trust)
    Worker {
        #[command(subcommand)]
        action: WorkerAction,
    },
    /// Applied listener state (not configured intent)
    Listener,
    /// Owner-local disposition of worker-owned runs on this machine
    Run {
        #[command(subcommand)]
        action: RunAction,
    },
    /// Inspect, spawn, control and watch runs on a routed worker through its
    /// pinned, authenticated bridge
    Remote {
        /// Worker fingerprint or route label
        #[arg(long)]
        worker: String,
        #[command(subcommand)]
        action: super::mesh_remote::RemoteAction,
    },
}
#[derive(Debug, Subcommand)]
pub enum WorkerAction {
    /// Route an already enrolled worker; take values from its `identity show`
    Add {
        #[arg(long)]
        fingerprint: String,
        #[arg(long)]
        daemon_id: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        control_address: SocketAddr,
        #[arg(long)]
        bulk_address: SocketAddr,
    },
    Remove {
        fingerprint: String,
    },
    List,
}
#[derive(Debug, Subcommand)]
pub enum RunAction {
    /// Worker-owned runs that still hold capacity on this machine
    List,
    /// Take local control, fencing any remote controller and writer
    Takeover { agent_id: String },
    /// Give up local control so a granted peer may acquire it
    Release { agent_id: String },
    /// Durably stop a locally controlled run; its guardian proves termination
    Stop { agent_id: String },
    /// Resolve an Unknown run after verifying its processes are gone
    Resolve {
        run_id: String,
        #[arg(long)]
        evidence: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum IdentityAction {
    Initialize,
    Show,
    /// Write this daemon's public certificate (DER) for a peer's enrollment and
    /// print its fingerprint to verify out of band. Never writes a private key.
    Export {
        #[arg(long)]
        out: PathBuf,
    },
}
#[derive(Debug, Subcommand)]
pub enum PeerAction {
    Revoke {
        fingerprint: String,
        #[arg(long)]
        expected_generation: u64,
    },
    Regrant {
        fingerprint: String,
        #[arg(long)]
        expected_generation: u64,
    },
    AdvanceGeneration {
        fingerprint: String,
        #[arg(long)]
        expected_generation: u64,
    },
}
#[derive(Debug, Subcommand)]
pub enum ExportAction {
    Register {
        project_root: PathBuf,
        #[arg(long)]
        label: String,
    },
    List,
    Revoke {
        export_id: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    Show,
    Disable,
    /// A literal specific-interface IP:port, not a hostname/wildcard
    Enable {
        #[arg(long)]
        control_address: SocketAddr,
        #[arg(long)]
        bulk_address: SocketAddr,
    },
}

/// What one `pu mesh` command does: every command maps to exactly one plan.
enum Plan {
    Admin(MeshAdminRequest),
    ExportIdentity(PathBuf),
    Listener,
    Remote {
        worker: String,
        action: super::mesh_remote::RemoteAction,
    },
}

impl MeshAction {
    fn plan(self) -> Result<Plan, CliError> {
        let request = match self {
            Self::Identity {
                action: IdentityAction::Export { out },
            } => return Ok(Plan::ExportIdentity(out)),
            Self::Listener => return Ok(Plan::Listener),
            Self::Remote { worker, action } => return Ok(Plan::Remote { worker, action }),
            Self::Run { action } => match action {
                RunAction::List => MeshAdminRequest::RunList {},
                RunAction::Takeover { agent_id } => MeshAdminRequest::RunTakeover { agent_id },
                RunAction::Release { agent_id } => MeshAdminRequest::RunRelease { agent_id },
                RunAction::Stop { agent_id } => MeshAdminRequest::RunStop { agent_id },
                RunAction::Resolve { run_id, evidence } => {
                    MeshAdminRequest::RunResolve { run_id, evidence }
                }
            },
            Self::Worker { action } => match action {
                WorkerAction::Add {
                    fingerprint,
                    daemon_id,
                    label,
                    control_address,
                    bulk_address,
                } => MeshAdminRequest::WorkerSet {
                    route: WorkerRoute {
                        fingerprint,
                        daemon_id,
                        label,
                        control_address,
                        bulk_address,
                    },
                },
                WorkerAction::Remove { fingerprint } => {
                    MeshAdminRequest::WorkerRemove { fingerprint }
                }
                WorkerAction::List => MeshAdminRequest::WorkerList {},
            },
            Self::Identity {
                action: IdentityAction::Initialize,
            } => MeshAdminRequest::IdentityInitialize {},
            Self::Identity {
                action: IdentityAction::Show,
            } => MeshAdminRequest::IdentityShow {},
            Self::EnrollManual {
                certificate_file,
                expected_fingerprint,
                grants_file,
                expires_at,
            } => MeshAdminRequest::EnrollManual {
                certificate_der: read_bounded(&certificate_file, 16 * 1024)?,
                expected_fingerprint,
                grants: serde_json::from_slice(&read_bounded(&grants_file, 64 * 1024)?)?,
                expires_at,
            },
            Self::PolicyUpdate {
                certificate_file,
                expected_fingerprint,
                expected_generation,
                grants_file,
                expires_at,
            } => MeshAdminRequest::PolicyUpdate {
                certificate_der: read_bounded(&certificate_file, 16 * 1024)?,
                expected_fingerprint,
                expected_generation,
                grants: serde_json::from_slice(&read_bounded(&grants_file, 64 * 1024)?)?,
                expires_at,
            },
            Self::Peers { after, limit } => MeshAdminRequest::PeerList { after, limit },
            Self::Peer { action } => {
                let (fingerprint, expected_generation, change) = match action {
                    PeerAction::Revoke {
                        fingerprint,
                        expected_generation,
                    } => (fingerprint, expected_generation, PeerChange::Revoke),
                    PeerAction::Regrant {
                        fingerprint,
                        expected_generation,
                    } => (fingerprint, expected_generation, PeerChange::Regrant),
                    PeerAction::AdvanceGeneration {
                        fingerprint,
                        expected_generation,
                    } => (
                        fingerprint,
                        expected_generation,
                        PeerChange::AdvanceGeneration,
                    ),
                };
                MeshAdminRequest::PeerChange {
                    fingerprint,
                    expected_generation,
                    change,
                }
            }
            Self::Export { action } => match action {
                ExportAction::Register {
                    project_root,
                    label,
                } => MeshAdminRequest::ExportRegister {
                    // Relative to where the owner runs the command; the daemon
                    // still resolves and pins the canonical directory itself.
                    project_root: std::path::absolute(&project_root)?
                        .into_os_string()
                        .into_string()
                        .map_err(|_| CliError::Other("export root must be UTF-8".into()))?,
                    label,
                },
                ExportAction::List => MeshAdminRequest::ExportList {},
                ExportAction::Revoke { export_id } => MeshAdminRequest::ExportRevoke { export_id },
            },
            Self::Config { action } => match action {
                ConfigAction::Show => MeshAdminRequest::ConfigShow {},
                ConfigAction::Disable => MeshAdminRequest::ConfigSet {
                    config: ListenConfig::default(),
                },
                ConfigAction::Enable {
                    control_address,
                    bulk_address,
                } => MeshAdminRequest::ConfigSet {
                    config: ListenConfig {
                        bind_address: Some(control_address),
                        bulk_bind_address: Some(bulk_address),
                    },
                },
            },
        };
        if !request.validate() {
            return Err(CliError::Other(
                "invalid or unbounded mesh administration request".into(),
            ));
        }
        Ok(Plan::Admin(request))
    }
}

/// Public material only; refuses to overwrite or follow an existing path.
fn write_new_public(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, CliError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(CliError::Other(
            "mesh input must be a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(CliError::Other("mesh input exceeds its byte limit".into()));
    }
    Ok(bytes)
}

async fn local(socket: &Path, request: &Request, json: bool) -> Result<Response, CliError> {
    output::check_response(client::send_request(socket, request).await?, json)
}

pub async fn run(socket: &Path, action: MeshAction, json: bool) -> Result<(), CliError> {
    let plan = action.plan()?;
    daemon_ctrl::ensure_daemon(socket).await?;
    match plan {
        Plan::Admin(request) => administer(socket, request, json).await,
        Plan::ExportIdentity(out) => export_identity(socket, &out, json).await,
        Plan::Listener => {
            let response = local(socket, &Request::MeshListenerStatus {}, json).await?;
            output::print_response(&response, json)
        }
        Plan::Remote { worker, action } => {
            super::mesh_remote::run(socket, &worker, action, json).await
        }
    }
}

async fn administer(socket: &Path, request: MeshAdminRequest, json: bool) -> Result<(), CliError> {
    let response = local(socket, &Request::MeshAdmin { request }, json).await?;
    if !matches!(response, Response::MeshAdmin { .. }) {
        return Err(CliError::Other(
            "unexpected mesh administration response".into(),
        ));
    }
    output::print_response(&response, json)
}

/// Write the public certificate for a peer's out-of-band enrollment.
async fn export_identity(socket: &Path, out: &Path, json: bool) -> Result<(), CliError> {
    let request = Request::MeshAdmin {
        request: MeshAdminRequest::IdentityShow {},
    };
    let identity = match local(socket, &request, json).await? {
        Response::MeshAdmin {
            response: pu_core::mesh::admin::MeshAdminResponse::Identity { identity },
        } => identity,
        _ => return Err(CliError::Other("unexpected identity response".into())),
    };
    write_new_public(out, &identity.certificate_der)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "certificate_file": out,
                "fingerprint": identity.fingerprint,
                "daemon_id": identity.daemon_id,
            })
        );
    } else {
        println!("certificate: {}", out.display());
        println!("fingerprint: {}", identity.fingerprint);
        println!("daemon id:   {}", identity.daemon_id);
        println!("Verify the fingerprint with the other machine's owner before they enroll it.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    impl MeshAction {
        /// The administration request this command plans; panics otherwise.
        fn request(self) -> Result<MeshAdminRequest, CliError> {
            match self.plan()? {
                Plan::Admin(request) => Ok(request),
                _ => panic!("not an administration command"),
            }
        }
    }
    use clap::Parser;
    #[test]
    fn given_explicit_mesh_cli_should_parse_and_roundtrip_local_request() {
        // given
        let cli =
            crate::Cli::try_parse_from(["pu", "mesh", "config", "disable", "--json"]).unwrap();
        // when / then
        match cli.command {
            crate::Commands::Mesh { action, json } => {
                assert!(json);
                assert_eq!(
                    action.request().unwrap(),
                    MeshAdminRequest::ConfigSet {
                        config: ListenConfig::default()
                    }
                );
            }
            _ => panic!("wrong command"),
        }
        assert!(crate::Cli::try_parse_from(["pu", "mesh", "invitation", "accept"]).is_err());
        assert!(
            crate::Cli::try_parse_from([
                "pu",
                "mesh",
                "enroll-manual",
                "--certificate-file",
                "cert.der",
                "--grants-file",
                "grants.json"
            ])
            .is_err()
        );
    }

    #[test]
    fn given_manual_enrollment_files_should_require_explicit_pin_and_preserve_typed_grants() {
        // given
        let dir = tempfile::tempdir().unwrap();
        let cert = dir.path().join("cert.der");
        let grants = dir.path().join("grants.json");
        std::fs::write(&cert, [1, 2, 3]).unwrap();
        std::fs::write(
            &grants,
            r#"[{"export_id":"local-export","capabilities":["inspect"],"runtimes":["codex"]}]"#,
        )
        .unwrap();
        // when
        let request = MeshAction::EnrollManual {
            certificate_file: cert.clone(),
            expected_fingerprint: "a".repeat(64),
            grants_file: grants.clone(),
            expires_at: Some(12345),
        }
        .request()
        .unwrap();
        // then
        assert_eq!(
            serde_json::from_slice::<MeshAdminRequest>(&serde_json::to_vec(&request).unwrap())
                .unwrap(),
            request
        );
        match request {
            MeshAdminRequest::EnrollManual {
                certificate_der,
                grants,
                ..
            } => {
                assert_eq!(certificate_der, [1, 2, 3]);
                assert_eq!(grants[0].export_id, "local-export");
            }
            _ => panic!("wrong request"),
        }
        std::fs::write(&cert, vec![0; 16 * 1024 + 1]).unwrap();
        assert!(
            MeshAction::EnrollManual {
                certificate_file: cert,
                expected_fingerprint: "a".repeat(64),
                grants_file: grants,
                expires_at: None
            }
            .request()
            .is_err()
        );
    }

    #[test]
    fn given_mesh_config_enable_should_require_two_valid_explicit_addresses() {
        // given / when / then
        assert!(
            crate::Cli::try_parse_from([
                "pu",
                "mesh",
                "config",
                "enable",
                "--control-address",
                "127.0.0.1:8443"
            ])
            .is_err()
        );
        let cli = crate::Cli::try_parse_from([
            "pu",
            "mesh",
            "config",
            "enable",
            "--control-address",
            "127.0.0.1:8443",
            "--bulk-address",
            "127.0.0.1:8444",
        ])
        .unwrap();
        if let crate::Commands::Mesh { action, .. } = cli.command {
            assert!(action.request().is_ok());
        } else {
            panic!("wrong command");
        }
        assert!(
            MeshAction::Config {
                action: ConfigAction::Enable {
                    control_address: "127.0.0.1:8443".parse().unwrap(),
                    bulk_address: "127.0.0.1:8443".parse().unwrap()
                }
            }
            .request()
            .is_err()
        );
        assert!(
            MeshAction::Peers {
                after: None,
                limit: 257
            }
            .request()
            .is_err()
        );
    }

    #[tokio::test]
    async fn given_existing_v8_daemon_without_admin_should_fail_unknown_request_without_retry_or_restart()
     {
        // given: only a scratch Unix IPC responder, never a real daemon process.
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("ipc.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                let request: Request = serde_json::from_str(&line).unwrap();
                let response = if index == 0 {
                    assert!(matches!(request, Request::Health));
                    Response::HealthReport {
                        pid: 0,
                        uptime_seconds: 0,
                        protocol_version: 8,
                        projects: vec![],
                        agent_count: 0,
                    }
                } else {
                    assert!(matches!(
                        request,
                        Request::MeshAdmin {
                            request: MeshAdminRequest::IdentityShow {}
                        }
                    ));
                    Response::Error {
                        code: "PARSE_ERROR".into(),
                        message: "unknown mesh admin request".into(),
                    }
                };
                let bytes = format!("{}\n", serde_json::to_string(&response).unwrap());
                stream.get_mut().write_all(bytes.as_bytes()).await.unwrap();
            }
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
        });
        // when / then
        assert!(
            matches!(run(&socket,MeshAction::Identity { action:IdentityAction::Show },false).await,
            Err(CliError::DaemonError { code,.. }) if code=="PARSE_ERROR")
        );
        server.await.unwrap();
    }
}
