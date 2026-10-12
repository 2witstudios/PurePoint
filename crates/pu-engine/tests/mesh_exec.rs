//! Remote execution against real host and worker daemon processes. Each test
//! names one requirement from docs/product/lan-mesh.md. The worker's `claude`
//! runtime is an echoing test agent that leaves a detached, marked descendant,
//! so stopping must prove whole-tree termination.

mod mesh_support;

use mesh_support::*;
use pu_core::mesh::{
    Capability,
    admin::{MeshAdminRequest, MeshAdminResponse, PeerChange},
    response::{Body, ErrorCode, InputOutcome, Lifecycle},
    wire::{AgentTarget, ControlGuard, Mutation, Operation},
};
use pu_core::protocol::{KillTarget, Request, Response};
use std::collections::BTreeSet;
use std::time::Duration;

async fn agent_count(fixture: &Fixture) -> usize {
    match fixture
        .send(Operation::Status {
            export: fixture.route.export.clone(),
            agent_id: None,
            after_agent: None,
            limit: 64,
        })
        .await
    {
        Body::Status { agents, .. } => agents.len(),
        other => panic!("unexpected {other:?}"),
    }
}

async fn take_over(fixture: &Fixture, agent: &str) {
    assert_eq!(
        fixture
            .worker
            .admin(MeshAdminRequest::RunTakeover {
                agent_id: agent.into(),
            })
            .await,
        MeshAdminResponse::Done {}
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_repeated_spawn_operation_should_return_original_receipt_without_launching_twice() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let window = fixture.window().await;
    let spawned = receipt(fixture.send(fixture.spawn_op(window, "spawn-1")).await);

    // when
    let repeated = receipt(fixture.send(fixture.spawn_op(window, "spawn-1")).await);

    // then
    assert_eq!(repeated.target, spawned.target);
    assert_eq!(agent_count(&fixture).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_conflicting_payload_under_one_operation_identity_should_return_conflict() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let window = fixture.window().await;
    receipt(fixture.send(fixture.spawn_op(window, "spawn-1")).await);

    // when
    let conflicting = fixture
        .send(
            fixture
                .route
                .spawn_op(window, "spawn-1", &fixture.project.commit, "different"),
        )
        .await;

    // then
    assert_eq!(
        conflicting,
        Body::Error {
            code: ErrorCode::Conflict
        }
    );
    assert_eq!(agent_count(&fixture).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_missing_source_should_fail_explicitly_without_admission() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let window = fixture.window().await;

    // when
    let missing = fixture
        .send(
            fixture
                .route
                .spawn_op(window, "missing", &"f".repeat(40), ""),
        )
        .await;

    // then
    assert_eq!(
        missing,
        Body::Error {
            code: ErrorCode::SourceMissing
        }
    );
    assert_eq!(agent_count(&fixture).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_retried_spawn_after_source_removed_should_still_return_original_receipt() {
    // given: an admitted spawn, then the worker repository loses that commit
    let fixture = fixture(all_capabilities()).await;
    let window = fixture.window().await;
    let root = &fixture.project.root;
    std::fs::write(root.join("NEXT"), "next\n").unwrap();
    git(root, &["add", "NEXT"]);
    git(root, &["commit", "-q", "-m", "next"]);
    let commit = git(root, &["rev-parse", "HEAD"]);
    let spawn = fixture.route.spawn_op(window, "spawn-1", &commit, "");
    let spawned = receipt(fixture.send(spawn.clone()).await);
    git(root, &["reset", "-q", "--hard", "HEAD~1"]);
    git(root, &["reflog", "expire", "--expire=now", "--all"]);
    git(root, &["gc", "-q", "--prune=now"]);

    // when
    let retried = receipt(fixture.send(spawn).await);

    // then: the retained receipt answers before any source check
    assert_eq!(retried.target, spawned.target);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_ungranted_runtime_should_refuse_at_the_session_without_admission() {
    // given: only inspect and spawn
    let fixture = fixture(BTreeSet::from([Capability::Inspect, Capability::Spawn])).await;
    let window = fixture.window().await;
    let mut codex = fixture.spawn_op(window, "codex");
    if let Operation::Spawn { runtime, .. } = &mut codex {
        *runtime = "codex".into();
    }

    // when / then
    assert_eq!(error_code(fixture.send_raw(codex).await), "MESH_DENIED");
    assert_eq!(agent_count(&fixture).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_granted_suspend_resume_or_transcript_read_should_return_unsupported() {
    // given: every capability, including suspend and resume
    let mut capabilities = all_capabilities();
    capabilities.extend([Capability::Suspend, Capability::Resume]);
    let fixture = fixture(capabilities).await;
    let spawned = fixture.running("spawn-1").await;
    let guard = fixture.guard(&spawned).await;
    let window = fixture.window().await;
    let mutation = |id: &str| Mutation {
        window,
        id: id.into(),
    };

    // when / then
    for operation in [
        Operation::Suspend {
            guard: guard.clone(),
            mutation: mutation("suspend"),
        },
        Operation::Resume {
            guard: guard.clone(),
            mutation: mutation("resume"),
            runtime: "claude".into(),
            source_commit: fixture.project.commit.clone(),
        },
        Operation::ReadConversation {
            target: spawned.target.clone(),
            limit: 10,
            cursor: None,
        },
    ] {
        assert_eq!(
            fixture.send(operation).await,
            Body::Error {
                code: ErrorCode::Unsupported
            }
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_peer_without_grants_should_not_be_issued_windows() {
    // given: a second host enrolled on the worker with no grants
    let fixture = fixture(all_capabilities()).await;
    let stranger = Daemon::start();
    let route = enroll(
        &stranger,
        &fixture.worker,
        &fixture.project,
        BTreeSet::new(),
    )
    .await;

    // when
    let window = route
        .send(&stranger, Operation::IssueWindow { ttl_seconds: 60 })
        .await;

    // then
    assert_eq!(
        window,
        Body::Error {
            code: ErrorCode::Denied
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_input_should_deliver_once_with_exact_receipts_and_never_replay() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let mut watch = fixture.watch(&spawned).await;
    let writer = fixture.writer(&spawned).await;
    let guard = fixture.guard(&spawned).await;
    let input = |sequence: u64, data: &[u8]| Operation::Input {
        guard: guard.clone(),
        writer,
        sequence,
        data: data.to_vec(),
    };

    // when
    let first = fixture.send(input(1, b"ping-from-host\n")).await;
    let repeated = fixture.send(input(1, b"ping-from-host\n")).await;
    let conflicting = fixture.send(input(1, b"different-bytes\n")).await;
    let gap = fixture.send(input(3, b"skipped\n")).await;
    let next = fixture.send(input(2, b"sentinel\n")).await;

    // then
    match &first {
        Body::InputReceipt {
            evidence: Some(evidence),
        } => assert_eq!(evidence.outcome, InputOutcome::Written),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(repeated, first);
    assert_eq!(
        conflicting,
        Body::Error {
            code: ErrorCode::Conflict
        }
    );
    assert_eq!(
        gap,
        Body::Error {
            code: ErrorCode::SequenceGap
        }
    );
    assert!(matches!(next, Body::InputReceipt { .. }), "{next:?}");
    // Each line appears twice (tty echo, then `cat`) for exactly one delivery.
    assert!(
        watch
            .until("sentinel\r\nsentinel", Duration::from_secs(30))
            .await
    );
    let output = String::from_utf8_lossy(&watch.seen);
    assert_eq!(output.matches("ping-from-host").count(), 2, "{output:?}");
    assert_eq!(output.matches("different-bytes").count(), 0, "{output:?}");
    assert_eq!(output.matches("skipped").count(), 0, "{output:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_stop_should_prove_whole_tree_termination_and_release_the_run() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let window = fixture.window().await;

    // when
    let stopped = receipt(
        fixture
            .send(Operation::Stop {
                guard: fixture.guard(&spawned).await,
                mutation: Mutation {
                    window,
                    id: "stop-1".into(),
                },
            })
            .await,
    );

    // then
    assert_eq!(stopped.target, spawned.target);
    fixture.wait(&spawned, Lifecycle::Terminated).await;
    assert!(wait_marker_dead(&fixture.marker, Duration::from_secs(10)));
    assert!(fixture.worker_runs().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_stop_during_guardian_startup_should_still_prove_termination() {
    // given: a spawn stopped immediately, before the runtime prints READY
    let fixture = fixture(all_capabilities()).await;
    let window = fixture.window().await;
    let spawned = receipt(fixture.send(fixture.spawn_op(window, "spawn-1")).await);

    // when
    let guard = fixture.guard(&spawned).await;
    receipt(
        fixture
            .send(Operation::Stop {
                guard,
                mutation: Mutation {
                    window,
                    id: "stop-early".into(),
                },
            })
            .await,
    );

    // then: proof, not Unknown
    fixture.wait(&spawned, Lifecycle::Terminated).await;
    assert!(wait_marker_dead(&fixture.marker, Duration::from_secs(10)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_another_peers_run_should_refuse_its_control() {
    // given: a second host granted control on the same export, and a run with
    // no controller (the owner took over and released it)
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let agent = spawned.target.agent_id.clone();
    take_over(&fixture, &agent).await;
    fixture
        .worker
        .admin(MeshAdminRequest::RunRelease {
            agent_id: agent.clone(),
        })
        .await;
    let other = Daemon::start();
    enroll(
        &other,
        &fixture.worker,
        &fixture.project,
        all_capabilities(),
    )
    .await;
    let route = Route {
        worker: fixture.route.worker.clone(),
        export: fixture.route.export.clone(),
    };
    let window = route.window(&other).await;
    let status = route.status(&other, &agent).await;

    // when
    let acquired = route
        .send(
            &other,
            Operation::AcquireControl {
                guard: ControlGuard {
                    target: AgentTarget { ..status.target },
                    epoch: status.epoch,
                    revision: status.revision,
                },
                mutation: Mutation {
                    window,
                    id: "seize".into(),
                },
            },
        )
        .await;

    // then
    assert_eq!(
        acquired,
        Body::Error {
            code: ErrorCode::Fenced
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_remote_controlled_run_should_fence_local_lifecycle_and_writers() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let agent = spawned.target.agent_id.clone();
    let root = fixture.project.root.to_string_lossy().to_string();

    // when / then
    let local_input = Request::Input {
        agent_id: agent.clone(),
        data: b"local\n".to_vec(),
        submit: false,
    };
    assert_eq!(
        error_code(fixture.worker.call(local_input).await),
        "MESH_REMOTE_CONTROL"
    );
    for request in [
        Request::Kill {
            project_root: root.clone(),
            target: KillTarget::Agent(agent.clone()),
            exclude: vec![],
        },
        Request::Rename {
            project_root: root.clone(),
            agent_id: agent.clone(),
            name: "renamed".into(),
        },
        Request::Resume {
            project_root: root.clone(),
            agent_id: agent.clone(),
        },
    ] {
        assert_eq!(
            error_code(fixture.worker.call(request).await),
            "MESH_OWNED_RUN"
        );
    }
    assert!(marked_alive(&fixture.marker));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_owner_takeover_should_fence_remote_writer_and_allow_local_input() {
    // given: a remote writer and a watch
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let agent = spawned.target.agent_id.clone();
    let remote_guard = fixture.guard(&spawned).await;
    let writer = fixture.writer(&spawned).await;
    let mut watch = fixture.watch(&spawned).await;
    let remote = || Operation::Input {
        guard: remote_guard.clone(),
        writer,
        sequence: 1,
        data: b"remote-line\n".to_vec(),
    };

    // when
    take_over(&fixture, &agent).await;

    // then: the closed link may report the fence first; then it is typed
    if let Response::Error { code, .. } = fixture.send_raw(remote()).await {
        assert!(
            matches!(code.as_str(), "MESH_DENIED" | "MESH_CLOSED"),
            "{code}"
        );
    }
    assert!(
        matches!(
            fixture.send(remote()).await,
            Body::Error {
                code: ErrorCode::Fenced | ErrorCode::Stale
            }
        ),
        "remote writer was not fenced"
    );
    assert!(matches!(
        fixture
            .worker
            .call(Request::Input {
                agent_id: agent.clone(),
                data: b"local-line\n".to_vec(),
                submit: false,
            })
            .await,
        Response::Ok
    ));
    assert!(watch.until("local-line", Duration::from_secs(30)).await);
    assert!(!String::from_utf8_lossy(&watch.seen).contains("remote-line"));
    let runs = fixture.worker_runs().await;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].controller, "local");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_local_control_should_never_be_displaced_by_a_peer() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    take_over(&fixture, &spawned.target.agent_id).await;
    let window = fixture.window().await;
    let guard = fixture.guard(&spawned).await;

    // when
    let acquired = fixture
        .route
        .send_eventually(
            &fixture.host,
            Operation::AcquireControl {
                guard,
                mutation: Mutation {
                    window,
                    id: "reacquire".into(),
                },
            },
        )
        .await;

    // then
    assert_eq!(
        acquired,
        Body::Error {
            code: ErrorCode::Fenced
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_owner_stop_should_prove_termination_and_free_capacity() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    take_over(&fixture, &spawned.target.agent_id).await;

    // when
    assert_eq!(
        fixture
            .worker
            .admin(MeshAdminRequest::RunStop {
                agent_id: spawned.target.agent_id.clone(),
            })
            .await,
        MeshAdminResponse::Done {}
    );

    // then
    fixture.wait(&spawned, Lifecycle::Terminated).await;
    assert!(wait_marker_dead(&fixture.marker, Duration::from_secs(10)));
    assert!(fixture.worker_runs().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_owner_resolution_should_refuse_live_or_settled_runs() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let resolve = || Request::MeshAdmin {
        request: MeshAdminRequest::RunResolve {
            run_id: spawned.target.run_id.clone(),
            evidence: "checked by owner".into(),
        },
    };

    // when / then: a live run is never released by attestation
    assert_eq!(
        error_code(fixture.worker.call(resolve()).await),
        "MESH_RUN_LIVE"
    );

    // when the run has ended with proof
    take_over(&fixture, &spawned.target.agent_id).await;
    fixture
        .worker
        .admin(MeshAdminRequest::RunStop {
            agent_id: spawned.target.agent_id.clone(),
        })
        .await;
    fixture.wait(&spawned, Lifecycle::Terminated).await;

    // then a settled run cannot be re-resolved
    assert_eq!(
        error_code(fixture.worker.call(resolve()).await),
        "MESH_RUN_INVALID_STATE"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_two_watches_of_one_run_should_each_receive_output_independently() {
    // given
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let mut first = fixture.watch(&spawned).await;
    let mut second = fixture.watch(&spawned).await;
    let writer = fixture.writer(&spawned).await;
    let guard = fixture.guard(&spawned).await;
    let send = |sequence: u64, text: &str| Operation::Input {
        guard: guard.clone(),
        writer,
        sequence,
        data: text.as_bytes().to_vec(),
    };

    // when: both see one line, then the first watch closes
    fixture.send(send(1, "both-see-this\n")).await;
    assert!(first.until("both-see-this", Duration::from_secs(30)).await);
    assert!(second.until("both-see-this", Duration::from_secs(30)).await);
    drop(first);
    fixture.send(send(2, "second-still-sees\n")).await;

    // then: closing one watch does not end the other's subscription
    assert!(
        second
            .until("second-still-sees", Duration::from_secs(30))
            .await
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_host_link_reconnect_should_resume_open_watch() {
    // given: an open watch
    let fixture = fixture(all_capabilities()).await;
    let spawned = fixture.running("spawn-1").await;
    let mut watch = fixture.watch(&spawned).await;
    let host_fingerprint = fixture.host.identity().await.fingerprint;
    let generation = match fixture
        .worker
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

    // when: advancing the host's generation closes its link; later output
    // comes from the owner, who takes over to type into the run
    fixture
        .worker
        .admin(MeshAdminRequest::PeerChange {
            fingerprint: host_fingerprint,
            expected_generation: generation,
            change: PeerChange::AdvanceGeneration,
        })
        .await;
    take_over(&fixture, &spawned.target.agent_id).await;
    fixture
        .worker
        .call(Request::Input {
            agent_id: spawned.target.agent_id.clone(),
            data: b"after-reconnect\n".to_vec(),
            submit: false,
        })
        .await;

    // then: the watch resubscribes over a new link and sees later output
    assert!(
        watch
            .until("after-reconnect", Duration::from_secs(20))
            .await
    );
}
