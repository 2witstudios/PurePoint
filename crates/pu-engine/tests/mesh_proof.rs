//! Composed proof against real, separately running daemon processes: enroll,
//! export, spawn, disconnect, reconnect, observe and explicitly control the
//! original run, plus abrupt worker/host death, revocation during execution and
//! concurrent admission from two hosts.

mod mesh_support;

use mesh_support::*;
use pu_core::mesh::{
    Capability,
    admin::{MeshAdminRequest, MeshAdminResponse, PeerChange},
    response::{Body, Controller, ErrorCode, InputOutcome, Lifecycle},
    wire::{Mutation, Operation, WriterCheckpoint},
};
use pu_core::protocol::Response;
use std::collections::BTreeSet;
use std::time::Duration;

async fn running(
    host: &Daemon,
    route: &Route,
    project: &Project,
    id: &str,
) -> pu_core::mesh::response::ReceiptEvidence {
    let window = route.window(host).await;
    let spawned = receipt(
        route
            .send(host, route.spawn_op(window, id, &project.commit, ""))
            .await,
    );
    route
        .wait_lifecycle(host, &spawned.target.agent_id, Lifecycle::Running)
        .await;
    spawned
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_worker_daemon_killed_should_terminate_tree_and_reconcile_original_run_after_restart()
{
    // given: a remote run with a detached, marked descendant
    let marker = marker();
    let _reaper = reap(&marker);
    let host = Daemon::start();
    let mut worker = Daemon::start();
    let project = project(&worker, &marker, None).await;
    let route = enroll(&host, &worker, &project, all_capabilities()).await;
    let spawned = running(&host, &route, &project, "spawn-crash").await;
    let mut watch = Watch::open(
        &host,
        route.local(Operation::SubscribeOutput {
            target: spawned.target.clone(),
            cursor: None,
        }),
    )
    .await;
    assert!(watch.until("READY", Duration::from_secs(30)).await);
    assert!(wait_marker_alive(&marker, Duration::from_secs(10)));

    // when: the worker daemon dies abruptly
    worker.kill9();

    // then: the guardian enforces the run-tree lifetime without the daemon
    assert!(
        wait_marker_dead(&marker, Duration::from_secs(15)),
        "run tree outlived the worker daemon"
    );

    // when: the worker restarts from its retained state
    worker.launch();

    // then: the host reconnects with the new generation and sees the original
    // run proven Interrupted, with capacity released; nothing was relaunched
    route
        .wait_lifecycle(&host, &spawned.target.agent_id, Lifecycle::Interrupted)
        .await;
    let status = route.status(&host, &spawned.target.agent_id).await;
    assert_eq!(status.target, spawned.target);
    assert_eq!(
        status.controller,
        Controller::None,
        "restart clears peer control"
    );
    match worker.admin(MeshAdminRequest::RunList {}).await {
        MeshAdminResponse::Runs { runs } => assert!(runs.is_empty(), "{runs:?}"),
        other => panic!("unexpected {other:?}"),
    }
    // a new run can be admitted on the same export
    let again = running(&host, &route, &project, "spawn-after-restart").await;
    assert_ne!(again.target.agent_id, spawned.target.agent_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_host_daemon_killed_should_preserve_run_and_reconnect_to_control_it_without_replay() {
    // given: a remote run with an open writer and one written input
    let marker = marker();
    let _reaper = reap(&marker);
    let mut host = Daemon::start();
    let worker = Daemon::start();
    let project = project(&worker, &marker, None).await;
    let route = enroll(&host, &worker, &project, all_capabilities()).await;
    let spawned = running(&host, &route, &project, "spawn-host-crash").await;
    let agent = spawned.target.agent_id.clone();
    assert!(wait_marker_alive(&marker, Duration::from_secs(10)));
    let guard = route.guard(&host, &agent).await;
    let writer = match route
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
        other => panic!("unexpected {other:?}"),
    };
    let first = route
        .send(
            &host,
            Operation::Input {
                guard: guard.clone(),
                writer,
                sequence: 1,
                data: b"before-crash\n".to_vec(),
            },
        )
        .await;
    assert!(matches!(first, Body::InputReceipt { .. }), "{first:?}");

    // when: the host dies abruptly and restarts
    host.kill9();
    tokio::time::sleep(Duration::from_millis(500)).await;
    // then: the worker-owned run kept running
    assert!(marked_alive(&marker));
    host.launch();

    // then: the original run is still Running and still controlled by this
    // host's stable identity; its writer checkpoint is visible to it
    let status = route.status(&host, &agent).await;
    assert_eq!(status.target, spawned.target);
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(status.controller, Controller::ThisPeer);
    let checkpoint = status.writer.clone().expect("controller sees its writer");
    assert_eq!(
        checkpoint,
        WriterCheckpoint {
            writer,
            last_sequence: 1
        }
    );
    // the retained receipt is exact; nothing is replayed
    match route
        .send(
            &host,
            Operation::InputReceipt {
                target: spawned.target.clone(),
                writer,
                sequence: 1,
            },
        )
        .await
    {
        Body::InputReceipt {
            evidence: Some(evidence),
        } => assert_eq!(evidence.outcome, InputOutcome::Written),
        other => panic!("unexpected {other:?}"),
    }
    // a replacement writer must name the inspected checkpoint
    let current = route.guard(&host, &agent).await;
    assert_eq!(
        route
            .send(
                &host,
                Operation::OpenWriter {
                    guard: current.clone(),
                    previous: None,
                },
            )
            .await,
        Body::Error {
            code: ErrorCode::ReconciliationRequired
        }
    );
    let replacement = match route
        .send(
            &host,
            Operation::OpenWriter {
                guard: current.clone(),
                previous: Some(checkpoint),
            },
        )
        .await
    {
        Body::Writer { writer, .. } => writer,
        other => panic!("unexpected {other:?}"),
    };
    // the old writer is fenced; the new one delivers
    match route
        .send(
            &host,
            Operation::Input {
                guard: current.clone(),
                writer,
                sequence: 2,
                data: b"stale\n".to_vec(),
            },
        )
        .await
    {
        Body::Error { code } => assert_eq!(code, ErrorCode::Fenced),
        other => panic!("old writer not fenced: {other:?}"),
    }
    let mut watch = Watch::open(
        &host,
        route.local(Operation::SubscribeOutput {
            target: spawned.target.clone(),
            cursor: None,
        }),
    )
    .await;
    route
        .send(
            &host,
            Operation::Input {
                guard: current.clone(),
                writer: replacement,
                sequence: 1,
                data: b"after-reconnect\n".to_vec(),
            },
        )
        .await;
    assert!(
        watch
            .until("after-reconnect", Duration::from_secs(10))
            .await
    );
    let echoed = String::from_utf8_lossy(&watch.seen);
    assert_eq!(
        echoed.matches("before-crash").count(),
        2,
        "pre-crash input must appear exactly once (tty echo + cat), never replayed: {echoed:?}"
    );

    // when: the reconnected host stops its original run
    let window = route.window(&host).await;
    receipt(
        route
            .send(
                &host,
                Operation::Stop {
                    guard: route.guard(&host, &agent).await,
                    mutation: Mutation {
                        window,
                        id: "stop-after-reconnect".into(),
                    },
                },
            )
            .await,
    );
    // then
    route
        .wait_lifecycle(&host, &agent, Lifecycle::Terminated)
        .await;
    assert!(wait_marker_dead(&marker, Duration::from_secs(10)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_revocation_during_execution_should_fence_peer_and_retain_run_for_owner() {
    // given
    let marker = marker();
    let _reaper = reap(&marker);
    let host = Daemon::start();
    let worker = Daemon::start();
    let project = project(&worker, &marker, None).await;
    let route = enroll(&host, &worker, &project, all_capabilities()).await;
    let spawned = running(&host, &route, &project, "spawn-revoke").await;
    let agent = spawned.target.agent_id.clone();
    assert!(wait_marker_alive(&marker, Duration::from_secs(10)));
    let host_fingerprint = host.identity().await.fingerprint;
    let generation = match worker
        .admin(MeshAdminRequest::PeerList {
            after: None,
            limit: 8,
        })
        .await
    {
        MeshAdminResponse::Peers { peers, .. } => {
            peers
                .into_iter()
                .find(|p| p.fingerprint == host_fingerprint)
                .unwrap()
                .generation
        }
        other => panic!("unexpected {other:?}"),
    };

    // when: the owner revokes the host while its run executes
    worker
        .admin(MeshAdminRequest::PeerChange {
            fingerprint: host_fingerprint,
            expected_generation: generation,
            change: PeerChange::Revoke,
        })
        .await;

    // then: the host can no longer observe or control anything
    match route
        .send_raw(
            &host,
            Operation::Status {
                export: route.export.clone(),
                agent_id: Some(agent.clone()),
                after_agent: None,
                limit: 1,
            },
        )
        .await
    {
        Response::Error { code, .. } => {
            assert!(
                matches!(code.as_str(), "MESH_DENIED" | "MESH_WORKER_OFFLINE"),
                "{code}"
            )
        }
        other => panic!("revoked host still served: {other:?}"),
    }
    // and the run keeps executing for the owner's explicit disposition
    assert!(marked_alive(&marker), "revocation must not kill the run");
    match worker.admin(MeshAdminRequest::RunList {}).await {
        MeshAdminResponse::Runs { runs } => {
            assert_eq!(runs.len(), 1);
            assert!(runs[0].live);
        }
        other => panic!("unexpected {other:?}"),
    }
    worker
        .admin(MeshAdminRequest::RunTakeover {
            agent_id: agent.clone(),
        })
        .await;
    worker
        .admin(MeshAdminRequest::RunStop {
            agent_id: agent.clone(),
        })
        .await;
    assert!(wait_marker_dead(&marker, Duration::from_secs(15)));
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        match worker.admin(MeshAdminRequest::RunList {}).await {
            MeshAdminResponse::Runs { runs } if runs.is_empty() => break,
            _ => {
                assert!(std::time::Instant::now() < deadline, "run never released");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_two_hosts_spawning_concurrently_should_admit_each_operation_exactly_once() {
    // given: two hosts enrolled on one worker
    let marker = marker();
    let _reaper = reap(&marker);
    let first = Daemon::start();
    let second = Daemon::start();
    let worker = Daemon::start();
    let project = project(
        &worker,
        &marker,
        Some(&format!("#!/bin/sh\nexec perl -e 'sleep 300' {marker}\n")),
    )
    .await;
    let route_a = enroll(
        &first,
        &worker,
        &project,
        BTreeSet::from([Capability::Inspect, Capability::Spawn]),
    )
    .await;
    // Both hosts are granted the project's one export.
    let route_b = enroll(
        &second,
        &worker,
        &project,
        BTreeSet::from([Capability::Inspect, Capability::Spawn]),
    )
    .await;
    let window_a = route_a.window(&first).await;
    let window_b = route_b.window(&second).await;

    // when: both hosts send their operations, each twice, concurrently
    let mut tasks = tokio::task::JoinSet::new();
    for (i, (host, route, window)) in [(&first, &route_a, window_a), (&second, &route_b, window_b)]
        .into_iter()
        .enumerate()
    {
        for op in 0..3 {
            for _ in 0..2 {
                let socket = host.socket.clone();
                let request = pu_core::protocol::Request::MeshRoute {
                    route: route.local(route.spawn_op(
                        window,
                        &format!("op-{i}-{op}"),
                        &project.commit,
                        "",
                    )),
                };
                tasks.spawn(async move { call_socket(&socket, request).await });
            }
        }
    }
    let mut bodies = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Response::MeshRoute { response } => bodies.push(response.body),
            other => panic!("route failed: {other:?}"),
        }
    }
    assert!(
        bodies.iter().all(|b| matches!(b, Body::Receipt { .. })),
        "{bodies:?}\nworker log:\n{}",
        worker.log()
    );

    // then: duplicates return the same receipt; six distinct runs exist on the
    // shared export, visible to both hosts
    let mut agents = BTreeSet::new();
    for body in bodies {
        agents.insert(receipt(body).target.agent_id);
    }
    assert_eq!(agents.len(), 6, "{agents:?}");
    for (host, route) in [(&first, &route_a), (&second, &route_b)] {
        match route
            .send(
                host,
                Operation::Status {
                    export: route.export.clone(),
                    agent_id: None,
                    after_agent: None,
                    limit: 64,
                },
            )
            .await
        {
            Body::Status { agents, .. } => assert_eq!(agents.len(), 6),
            other => panic!("unexpected {other:?}"),
        }
    }
}
