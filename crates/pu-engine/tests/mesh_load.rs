//! Synthetic fleet workload: 8 worker daemons x 30 guarded agents driven by one
//! host, with live viewers, one output-flooding run and one deliberately
//! stalled bulk reader. Measures control round trips, daemon memory and
//! descriptors, and verifies complete teardown. Run explicitly:
//!
//!   cargo test -p pu-engine --test mesh_load -- --ignored --nocapture
//!
//! Asserts that viewers stay live beside a stalled bulk reader and that every
//! run tree ends; latency and resource figures are measured and reported in
//! the JSON report recorded in docs/product/lan-mesh.md, not asserted.

mod mesh_support;

use mesh_support::*;
use pu_core::mesh::{
    response::{Body, Lifecycle},
    wire::{Mutation, Operation},
};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const WORKERS: usize = 8;
const AGENTS: usize = 30;

fn agent_script(marker: &str) -> String {
    // Echo input, tick twice a second, flood after receiving "flood".
    format!(
        r#"#!/bin/sh
exec perl -e '$|=1; print "READY\n"; use IO::Select; my $s=IO::Select->new(\*STDIN); my $flood=0;
while (1) {{
  if ($flood) {{ print "x" x 4000, "\n"; next; }}
  if ($s->can_read(0.5)) {{ my $l=<STDIN>; exit 0 unless defined $l; $flood=1 if $l=~/flood/; print "echo:$l"; }}
  else {{ print "tick\n"; }}
}}' {marker}
"#
    )
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[index]
}

fn summarize(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    serde_json::json!({
        "count": samples.len(),
        "p50_ms": percentile(&samples, 0.50),
        "p95_ms": percentile(&samples, 0.95),
        "p99_ms": percentile(&samples, 0.99),
        "max_ms": samples.last().copied().unwrap_or(0.0),
    })
}

fn rss_kib(pid: u32) -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

fn fds(pid: u32) -> usize {
    let out = std::process::Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .count()
        .saturating_sub(1)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "fleet load measurement; run explicitly"]
async fn given_eight_workers_by_thirty_agents_should_keep_viewers_live_end_every_tree_and_report_latency()
 {
    let marker = marker();
    let _reaper = reap(&marker);
    let started = Instant::now();
    let host = Daemon::start();
    let mut workers = Vec::new();
    let mut routes = Vec::new();
    let mut projects = Vec::new();
    for _ in 0..WORKERS {
        let worker = Daemon::start();
        let project = project(&worker, &marker, Some(&agent_script(&marker))).await;
        let route = enroll(&host, &worker, &project, all_capabilities()).await;
        workers.push(worker);
        routes.push(route);
        projects.push(project);
    }
    let setup_s = started.elapsed().as_secs_f64();

    // Spawn 30 agents on every worker; workers proceed concurrently.
    let spawn_started = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    for (index, (route, project)) in routes.iter().zip(&projects).enumerate() {
        let socket = host.socket.clone();
        let route = Route {
            worker: route.worker.clone(),
            export: route.export.clone(),
        };
        let commit = project.commit.clone();
        tasks.spawn(async move {
            let host = SocketHost(socket);
            let window = match host
                .send(&route, Operation::IssueWindow { ttl_seconds: 3600 })
                .await
            {
                Body::Window { window, .. } => window,
                other => panic!("window: {other:?}"),
            };
            let mut agents = Vec::new();
            let mut latencies = Vec::new();
            for n in 0..AGENTS {
                let t = Instant::now();
                let body = host
                    .send(
                        &route,
                        route.spawn_op(window, &format!("load-{index}-{n}"), &commit, ""),
                    )
                    .await;
                latencies.push(t.elapsed().as_secs_f64() * 1000.0);
                agents.push(receipt(body).target);
            }
            (index, agents, latencies)
        });
    }
    let mut targets = vec![Vec::new(); WORKERS];
    let mut spawn_latencies = Vec::new();
    while let Some(result) = tasks.join_next().await {
        let (index, agents, latencies) = result.unwrap();
        targets[index] = agents;
        spawn_latencies.extend(latencies);
    }
    let spawn_s = spawn_started.elapsed().as_secs_f64();
    for (route, agents) in routes.iter().zip(&targets) {
        for agent in agents {
            route
                .wait_lifecycle(&host, &agent.agent_id, Lifecycle::Running)
                .await;
        }
    }
    let running_s = spawn_started.elapsed().as_secs_f64();

    // Two live viewers per worker through the host.
    let mut viewers = Vec::new();
    for (route, agents) in routes.iter().zip(&targets) {
        for agent in agents.iter().take(2) {
            viewers.push(
                Watch::open(
                    &host,
                    route.local(Operation::SubscribeOutput {
                        target: agent.clone(),
                        cursor: None,
                    }),
                )
                .await,
            );
        }
    }

    // Flood one run on worker 0 and attach a peer that never reads its bulk.
    let flood = targets[0][AGENTS - 1].clone();
    let guard = routes[0].guard(&host, &flood.agent_id).await;
    let writer = match routes[0]
        .send(
            &host,
            Operation::OpenWriter {
                guard: guard.clone(),
                previous: None,
            },
        )
        .await
    {
        Body::Writer { writer, .. } => writer,
        other => panic!("writer: {other:?}"),
    };
    let _stalled = stalled_viewer(&workers[0], &routes[0], &flood).await;
    routes[0]
        .send(
            &host,
            Operation::Input {
                guard,
                writer,
                sequence: 1,
                data: b"flood\n".to_vec(),
            },
        )
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Control round trips on every worker, concurrently, under that load.
    let mut probes = tokio::task::JoinSet::new();
    for (index, (route, agents)) in routes.iter().zip(&targets).enumerate() {
        let socket = host.socket.clone();
        let route = Route {
            worker: route.worker.clone(),
            export: route.export.clone(),
        };
        let agent = agents[3].clone();
        probes.spawn(async move {
            let host = SocketHost(socket);
            let mut samples = Vec::new();
            for _ in 0..40 {
                let t = Instant::now();
                let body = host
                    .send(
                        &route,
                        Operation::Status {
                            export: route.export.clone(),
                            agent_id: Some(agent.agent_id.clone()),
                            after_agent: None,
                            limit: 1,
                        },
                    )
                    .await;
                assert!(matches!(body, Body::Status { .. }), "{body:?}");
                samples.push(t.elapsed().as_secs_f64() * 1000.0);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            (index, samples)
        });
    }
    let mut control = Vec::new();
    let mut control_worker0 = Vec::new();
    while let Some(result) = probes.join_next().await {
        let (index, samples) = result.unwrap();
        if index == 0 {
            control_worker0.extend(samples.iter().copied());
        }
        control.extend(samples);
    }

    // Viewers stay live while one bulk reader is stalled.
    let mut live_viewers = 0;
    for viewer in &mut viewers {
        if viewer.until("tick", Duration::from_secs(5)).await {
            live_viewers += 1;
        }
    }

    let host_rss = rss_kib(host.pid());
    let worker_rss: Vec<u64> = workers.iter().map(|w| rss_kib(w.pid())).collect();
    let worker_fds: Vec<usize> = workers.iter().map(|w| fds(w.pid())).collect();
    let host_fds = fds(host.pid());

    // Teardown: stop every run through the host and prove the trees ended.
    let teardown_started = Instant::now();
    drop(viewers);
    let mut stops = tokio::task::JoinSet::new();
    for (route, agents) in routes.iter().zip(&targets) {
        let socket = host.socket.clone();
        let route = Route {
            worker: route.worker.clone(),
            export: route.export.clone(),
        };
        let agents = agents.clone();
        stops.spawn(async move {
            let host = SocketHost(socket);
            let window = match host
                .send(&route, Operation::IssueWindow { ttl_seconds: 3600 })
                .await
            {
                Body::Window { window, .. } => window,
                other => panic!("window: {other:?}"),
            };
            for agent in agents {
                let status = host.status(&route, &agent.agent_id).await;
                let guard = pu_core::mesh::wire::ControlGuard {
                    target: status.target,
                    epoch: status.epoch,
                    revision: status.revision,
                };
                host.send(
                    &route,
                    Operation::Stop {
                        guard,
                        mutation: Mutation {
                            window,
                            id: format!("stop-{}", agent.agent_id),
                        },
                    },
                )
                .await;
            }
        });
    }
    while let Some(result) = stops.join_next().await {
        result.unwrap();
    }
    for (route, agents) in routes.iter().zip(&targets) {
        for agent in agents {
            route
                .wait_lifecycle(&host, &agent.agent_id, Lifecycle::Terminated)
                .await;
        }
    }
    let teardown_s = teardown_started.elapsed().as_secs_f64();
    let all_dead = wait_marker_dead(&marker, Duration::from_secs(30));

    let report = serde_json::json!({
        "workers": WORKERS,
        "agents_per_worker": AGENTS,
        "total_runs": WORKERS * AGENTS,
        "host_viewers": WORKERS * 2,
        "stalled_bulk_readers": 1,
        "setup_s": setup_s,
        "spawn_all_s": spawn_s,
        "all_running_s": running_s,
        "spawn_request": summarize(spawn_latencies),
        "control_status_all_workers": summarize(control),
        "control_status_worker_with_stalled_reader": summarize(control_worker0),
        "live_viewers": live_viewers,
        "host_rss_kib": host_rss,
        "host_fds": host_fds,
        "worker_rss_kib": worker_rss,
        "worker_fds": worker_fds,
        "teardown_s": teardown_s,
        "all_run_trees_ended": all_dead,
        "platform": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    });
    println!(
        "MESH_LOAD_REPORT {}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    assert_eq!(
        live_viewers,
        WORKERS * 2,
        "a stalled reader starved viewers"
    );
    assert!(all_dead, "run trees survived teardown");
}

/// Host requests from spawned tasks (owned socket path).
struct SocketHost(std::path::PathBuf);

impl SocketHost {
    async fn send(&self, route: &Route, operation: Operation) -> Body {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match call_socket(
                &self.0,
                pu_core::protocol::Request::MeshRoute {
                    route: route.local(operation.clone()),
                },
            )
            .await
            {
                pu_core::protocol::Response::MeshRoute { response } => return response.body,
                other => {
                    assert!(Instant::now() < deadline, "route failed: {other:?}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn status(&self, route: &Route, agent: &str) -> pu_core::mesh::response::AgentSummary {
        match self
            .send(
                route,
                Operation::Status {
                    export: route.export.clone(),
                    agent_id: Some(agent.into()),
                    after_agent: None,
                    limit: 1,
                },
            )
            .await
        {
            Body::Status { mut agents, .. } => agents.remove(0),
            other => panic!("status: {other:?}"),
        }
    }
}

/// A separately enrolled peer that subscribes to the flooding run and never
/// reads its bulk connection. Its control binding stays live.
async fn stalled_viewer(
    worker: &Daemon,
    route: &Route,
    target: &pu_core::mesh::wire::AgentTarget,
) -> impl Drop {
    use pu_engine::{
        mesh_sessions::{ControlClient, SessionLimits},
        mesh_transport::{EnrolledPeers, Identity, MeshTransport, PeerId},
    };
    let identity = Identity::generate().unwrap();
    worker
        .admin(pu_core::mesh::admin::MeshAdminRequest::EnrollManual {
            certificate_der: identity.certificate_der().to_vec(),
            expected_fingerprint: peer_hex(identity.peer_id()),
            grants: vec![pu_core::mesh::ExportGrant {
                export_id: route.export.id.clone(),
                capabilities: BTreeSet::from([
                    pu_core::mesh::Capability::Inspect,
                    pu_core::mesh::Capability::ReadOutput,
                ]),
                runtimes: BTreeSet::new(),
            }],
            expires_at: None,
        })
        .await;
    let (control, bulk) = match worker
        .admin(pu_core::mesh::admin::MeshAdminRequest::ConfigShow {})
        .await
    {
        pu_core::mesh::admin::MeshAdminResponse::Config { config, .. } => (
            config.bind_address.unwrap(),
            config.bulk_bind_address.unwrap(),
        ),
        other => panic!("config: {other:?}"),
    };
    let transport = MeshTransport::new(
        &identity,
        EnrolledPeers::new([route.worker.certificate_der.as_slice()]).unwrap(),
    )
    .unwrap();
    let pin = PeerId::from_certificate_der(&route.worker.certificate_der).unwrap();
    let limits = SessionLimits::default();
    let generation = ControlClient::discover(
        &transport,
        tokio::net::TcpStream::connect(control).await.unwrap(),
        pin,
        route.worker.daemon_id.clone(),
        limits.clone(),
    )
    .await
    .unwrap();
    let mut client = ControlClient::connect(
        &transport,
        tokio::net::TcpStream::connect(control).await.unwrap(),
        pin,
        route.worker.daemon_id.clone(),
        generation.generation,
        limits,
    )
    .await
    .unwrap();
    let bulk = client
        .connect_bulk(
            &transport,
            tokio::net::TcpStream::connect(bulk).await.unwrap(),
        )
        .await
        .unwrap();
    let response: pu_core::mesh::response::Response = client
        .request(&route.request(Operation::SubscribeOutput {
            target: target.clone(),
            cursor: None,
        }))
        .await
        .unwrap();
    assert!(
        matches!(response.body, Body::Subscribed { .. }),
        "{response:?}"
    );
    // Keep control alive (heartbeats) while never reading bulk.
    let daemon = route.worker.daemon_id.clone();
    let heartbeat = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let ping = pu_core::mesh::wire::Request {
                request_id: pu_core::id::session_id(),
                target_daemon: daemon.clone(),
                operation: Operation::Inventory {},
            };
            if client
                .request::<pu_core::mesh::response::Response>(&ping)
                .await
                .is_err()
            {
                return;
            }
        }
    });
    struct Stalled {
        _bulk: Box<dyn std::any::Any + Send>,
        heartbeat: tokio::task::JoinHandle<()>,
    }
    impl Drop for Stalled {
        fn drop(&mut self) {
            self.heartbeat.abort();
        }
    }
    Stalled {
        _bulk: Box::new(bulk),
        heartbeat,
    }
}

fn peer_hex(peer: pu_engine::mesh_transport::PeerId) -> String {
    pu_engine::daemon_lifecycle::mesh_store::peer_name(peer)
}
