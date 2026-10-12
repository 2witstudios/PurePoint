//! Multi-process mesh proof harness. Each daemon is a real `pu-engine` process
//! with its own private state directory and isolated HOME, driven over its
//! local socket exactly like the CLI. Processes are killed on drop; agent
//! processes carry a per-test marker and are reaped by marker only.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pu_core::mesh::{
    Capability, ExportGrant,
    admin::{ListenConfig, MeshAdminRequest, MeshAdminResponse, PublicIdentity, WorkerRoute},
    local::LocalRoute,
    response::{AgentSummary, Body, Lifecycle, ReceiptEvidence},
    wire::{ControlGuard, Export, Mutation, Operation, Request as WireRequest},
};
use pu_core::protocol::{Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const EXE: &str = env!("CARGO_BIN_EXE_pu-engine");

pub fn private_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

/// One real daemon process. Its state directory survives restarts.
pub struct Daemon {
    pub dir: tempfile::TempDir,
    pub home: tempfile::TempDir,
    pub socket: PathBuf,
    child: Option<Child>,
}

impl Daemon {
    pub fn start() -> Self {
        let dir = private_dir();
        let home = private_dir();
        let socket = dir.path().join("daemon.sock");
        let mut daemon = Self {
            dir,
            home,
            socket,
            child: None,
        };
        daemon.launch();
        daemon
    }

    pub fn launch(&mut self) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.path().join("test-daemon.log"))
            .unwrap();
        let child = Command::new(EXE)
            .arg("--socket")
            .arg(&self.socket)
            .env("HOME", self.home.path())
            .env_remove("PU_PROJECT_ROOT")
            .env_remove("PU_AGENT_ID")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.child = Some(child);
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::os::unix::net::UnixStream::connect(&self.socket).is_err() {
            assert!(Instant::now() < deadline, "daemon never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    /// Abrupt death: no shutdown path runs.
    pub fn kill9(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub async fn call(&self, request: Request) -> Response {
        call_socket(&self.socket, request).await
    }

    pub async fn admin(&self, request: MeshAdminRequest) -> MeshAdminResponse {
        match self.call(Request::MeshAdmin { request }).await {
            Response::MeshAdmin { response } => response,
            other => panic!("admin request failed: {other:?}"),
        }
    }

    pub async fn identity(&self) -> PublicIdentity {
        match self.admin(MeshAdminRequest::IdentityShow {}).await {
            MeshAdminResponse::Identity { identity } => identity,
            other => panic!("unexpected {other:?}"),
        }
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("test-daemon.log")).unwrap_or_default()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.kill9();
    }
}

/// One local IPC request/response, owned so it can run in a spawned task.
pub async fn call_socket(socket: &Path, request: Request) -> Response {
    let stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut line = serde_json::to_vec(&request).unwrap();
    line.push(b'\n');
    writer.write_all(&line).await.unwrap();
    let mut text = String::new();
    BufReader::new(reader).read_line(&mut text).await.unwrap();
    serde_json::from_str(&text).unwrap()
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub fn free_pair() -> (SocketAddr, SocketAddr) {
    let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (a.local_addr().unwrap(), b.local_addr().unwrap())
}

/// A prepared worker project whose `claude` runtime is an echoing test agent
/// that also leaves a detached, marked descendant.
pub struct Project {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    pub commit: String,
}

pub async fn project(worker: &Daemon, marker: &str, agent_script: Option<&str>) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "test"]);
    std::fs::write(root.join("README"), "worker project\n").unwrap();
    git(&root, &["add", "README"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    let commit = git(&root, &["rev-parse", "HEAD"]);
    if let Response::Error { code, message } = worker
        .call(Request::Init {
            project_root: root.to_string_lossy().into(),
        })
        .await
    {
        panic!("init failed: {code} {message}");
    }
    let script = root.join(".pu").join("fake-agent.sh");
    let body = agent_script.map(str::to_string).unwrap_or_else(|| {
        format!(
            "#!/bin/sh\n( exec perl -e 'use POSIX; exit 0 if fork; POSIX::setsid(); sleep 300' {marker} & )\necho READY\nexec cat\n"
        )
    });
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        root.join(".pu").join("config.yaml"),
        format!(
            "agents:\n  claude:\n    name: claude\n    command: {}\n    launchArgs: []\n",
            script.display()
        ),
    )
    .unwrap();
    Project { dir, root, commit }
}

pub fn all_capabilities() -> BTreeSet<Capability> {
    BTreeSet::from([
        Capability::Inspect,
        Capability::ReadOutput,
        Capability::Spawn,
        Capability::Control,
        Capability::Input,
        Capability::Stop,
    ])
}

/// Host-side handle for one routed worker.
pub struct Route {
    pub worker: PublicIdentity,
    pub export: Export,
}

/// Owner setup on both machines: explicit identities, out-of-band pins,
/// receiver export and grants, explicit listener, host route.
pub async fn enroll(
    host: &Daemon,
    worker: &Daemon,
    project: &Project,
    capabilities: BTreeSet<Capability>,
) -> Route {
    for daemon in [host, worker] {
        if !matches!(
            daemon
                .call(Request::MeshAdmin {
                    request: MeshAdminRequest::IdentityShow {}
                })
                .await,
            Response::MeshAdmin { .. }
        ) {
            daemon.admin(MeshAdminRequest::IdentityInitialize {}).await;
        }
    }
    let host_identity = host.identity().await;
    let worker_identity = worker.identity().await;
    let export_id = match worker
        .admin(MeshAdminRequest::ExportRegister {
            project_root: project.root.to_string_lossy().into(),
            label: "project".into(),
        })
        .await
    {
        MeshAdminResponse::Export { export } => export.id,
        other => panic!("unexpected {other:?}"),
    };
    worker
        .admin(MeshAdminRequest::EnrollManual {
            certificate_der: host_identity.certificate_der.clone(),
            expected_fingerprint: host_identity.fingerprint.clone(),
            grants: vec![ExportGrant {
                export_id: export_id.clone(),
                capabilities,
                runtimes: BTreeSet::from(["claude".into()]),
            }],
            expires_at: None,
        })
        .await;
    host.admin(MeshAdminRequest::EnrollManual {
        certificate_der: worker_identity.certificate_der.clone(),
        expected_fingerprint: worker_identity.fingerprint.clone(),
        grants: vec![],
        expires_at: None,
    })
    .await;
    // Keep an already configured listener so other hosts' routes stay valid.
    let (control, bulk) = match worker.admin(MeshAdminRequest::ConfigShow {}).await {
        MeshAdminResponse::Config {
            config:
                ListenConfig {
                    bind_address: Some(control),
                    bulk_bind_address: Some(bulk),
                },
            ..
        } => (control, bulk),
        _ => {
            let (control, bulk) = free_pair();
            worker
                .admin(MeshAdminRequest::ConfigSet {
                    config: ListenConfig {
                        bind_address: Some(control),
                        bulk_bind_address: Some(bulk),
                    },
                })
                .await;
            (control, bulk)
        }
    };
    host.admin(MeshAdminRequest::WorkerSet {
        route: WorkerRoute {
            fingerprint: worker_identity.fingerprint.clone(),
            daemon_id: worker_identity.daemon_id.clone(),
            label: format!("worker-{}", &worker_identity.fingerprint[..8]),
            control_address: control,
            bulk_address: bulk,
        },
    })
    .await;
    Route {
        worker: worker_identity,
        export: Export {
            id: export_id,
            generation: 1,
        },
    }
}

impl Route {
    pub fn request(&self, operation: Operation) -> WireRequest {
        WireRequest {
            request_id: pu_core::id::session_id(),
            target_daemon: self.worker.daemon_id.clone(),
            operation,
        }
    }

    pub fn local(&self, operation: Operation) -> LocalRoute {
        LocalRoute {
            worker: self.worker.fingerprint.clone(),
            request: self.request(operation),
        }
    }

    pub async fn send_raw(&self, host: &Daemon, operation: Operation) -> Response {
        host.call(Request::MeshRoute {
            route: self.local(operation),
        })
        .await
    }

    pub async fn send(&self, host: &Daemon, operation: Operation) -> Body {
        match self.send_raw(host, operation).await {
            Response::MeshRoute { response } => response.body,
            other => panic!("route failed: {other:?}"),
        }
    }

    /// The host never retries automatically; after a peer restart the first
    /// request may fail on the stale link and the next reconnects.
    pub async fn send_eventually(&self, host: &Daemon, operation: Operation) -> Body {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.send_raw(host, operation.clone()).await {
                Response::MeshRoute { response } => return response.body,
                other => {
                    assert!(
                        Instant::now() < deadline,
                        "route never recovered: {other:?}"
                    );
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    pub async fn window(&self, host: &Daemon) -> u64 {
        match self
            .send_eventually(host, Operation::IssueWindow { ttl_seconds: 3600 })
            .await
        {
            Body::Window { window, .. } => window,
            other => panic!("unexpected {other:?}"),
        }
    }

    pub fn spawn_op(&self, window: u64, id: &str, commit: &str, prompt: &str) -> Operation {
        Operation::Spawn {
            mutation: Mutation {
                window,
                id: id.into(),
            },
            export: self.export.clone(),
            runtime: "claude".into(),
            source_commit: commit.into(),
            prompt: prompt.into(),
        }
    }

    pub async fn status(&self, host: &Daemon, agent: &str) -> AgentSummary {
        match self
            .send_eventually(
                host,
                Operation::Status {
                    export: self.export.clone(),
                    agent_id: Some(agent.into()),
                    after_agent: None,
                    limit: 1,
                },
            )
            .await
        {
            Body::Status { mut agents, .. } => agents.remove(0),
            other => panic!("unexpected {other:?}"),
        }
    }

    pub async fn wait_lifecycle(&self, host: &Daemon, agent: &str, wanted: Lifecycle) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let status = self.status(host, agent).await;
            if status.lifecycle == wanted {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "lifecycle stayed {:?}, wanted {wanted:?}",
                status.lifecycle
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn guard(&self, host: &Daemon, agent: &str) -> ControlGuard {
        let status = self.status(host, agent).await;
        ControlGuard {
            target: status.target,
            epoch: status.epoch,
            revision: status.revision,
        }
    }
}

pub fn receipt(body: Body) -> ReceiptEvidence {
    match body {
        Body::Receipt {
            evidence: Some(evidence),
        } => evidence,
        other => panic!("expected receipt, got {other:?}"),
    }
}

/// Stream a remote run's output through the host daemon's local watch.
pub struct Watch {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    _writer: tokio::net::unix::OwnedWriteHalf,
    pub seen: Vec<u8>,
    pub gaps: u64,
    pub ended: bool,
}

impl Watch {
    pub async fn open(host: &Daemon, route: LocalRoute) -> Self {
        let stream = tokio::net::UnixStream::connect(&host.socket).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut line = serde_json::to_vec(&Request::MeshWatch { route }).unwrap();
        line.push(b'\n');
        writer.write_all(&line).await.unwrap();
        let mut watch = Self {
            reader: BufReader::new(reader),
            _writer: writer,
            seen: Vec::new(),
            gaps: 0,
            ended: false,
        };
        match watch.next().await {
            Some(Response::MeshRoute { response }) => {
                assert!(
                    matches!(response.body, Body::Subscribed { .. }),
                    "{response:?}"
                )
            }
            other => panic!("watch refused: {other:?}"),
        }
        watch
    }

    async fn next(&mut self) -> Option<Response> {
        let mut text = String::new();
        if self.reader.read_line(&mut text).await.ok()? == 0 {
            return None;
        }
        serde_json::from_str(&text).ok()
    }

    /// Read frames until `needle` appears or the deadline passes.
    pub async fn until(&mut self, needle: &str, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while !String::from_utf8_lossy(&self.seen).contains(needle) {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            match tokio::time::timeout(left, self.next()).await {
                Ok(Some(Response::MeshOutput { frame })) => {
                    self.gaps += frame.gap.unwrap_or(0);
                    self.seen.extend_from_slice(&frame.data);
                    if frame.end {
                        self.ended = true;
                        return String::from_utf8_lossy(&self.seen).contains(needle);
                    }
                }
                Ok(_) | Err(_) => return false,
            }
        }
        true
    }
}

pub fn marker() -> String {
    format!(
        "pu-mesh-proof-{}",
        pu_core::id::session_id().replace('-', "")
    )
}

pub fn marked_alive(marker: &str) -> bool {
    Command::new("pgrep")
        .args(["-f", marker])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// The marked descendant starts asynchronously after the agent prints READY.
pub fn wait_marker_alive(marker: &str, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while !marked_alive(marker) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

pub fn wait_marker_dead(marker: &str, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while marked_alive(marker) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// Never leak test processes, even on failure.
pub struct Reaper(pub String);

impl Drop for Reaper {
    fn drop(&mut self) {
        let _ = Command::new("pkill").args(["-9", "-f", &self.0]).status();
    }
}

pub fn reap(marker: &str) -> Reaper {
    Reaper(marker.to_string())
}

pub fn assert_receipt_target(evidence: &ReceiptEvidence, export: &Export) {
    assert_eq!(evidence.target.export, *export);
}

pub fn assert_dir(path: &Path) {
    assert!(path.is_dir(), "{}", path.display());
}
