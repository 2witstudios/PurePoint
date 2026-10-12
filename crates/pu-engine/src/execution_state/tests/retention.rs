//! Budgets count only live records; terminal history is retained within a
//! documented horizon and never blocks new work or the local owner.
use super::*;

fn peer_named(ledger: &Ledger, id: &str) -> Caller {
    let registered = ledger.register_peer(&admin(), id).unwrap();
    let anchored = ledger
        .update_peer_authorization(&admin(), id, registered.generation, PayloadDigest([9; 32]))
        .unwrap();
    Caller::authenticated_peer(id, anchored.generation).unwrap()
}

fn rows(ledger: &Ledger, table: &str) -> usize {
    ledger
        .transaction(|tx| {
            Ok(tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)
        })
        .unwrap()
}

fn prelaunch_failure() -> OutcomeProof {
    OutcomeProof::PrelaunchFailure {
        evidence: "source verification failed before dispatch".into(),
    }
}

/// One complete launch whose window is closed and whose run is proven terminal.
fn finished(ledger: &Ledger, caller: &Caller, now: i64, name: &str) -> LaunchReceipt {
    let window = ledger.issue_window(caller, now, 10).unwrap();
    let operation = OperationKey {
        window: window.id,
        id: OperationId::new(name).unwrap(),
        digest: PayloadDigest([3; 32]),
    };
    let receipt = ledger
        .admit_launch(caller, &operation, &intent(), None, now)
        .unwrap()
        .value;
    ledger
        .resolve_run(&receipt.run_id, &prelaunch_failure())
        .unwrap();
    ledger.close_window(caller, window.id).unwrap();
    receipt
}

fn is_capacity(result: &Result<OperationWindow>) -> bool {
    matches!(result, Err(Error::Capacity { .. }))
}

#[test]
fn given_peer_issuing_unbounded_windows_should_exhaust_only_its_own_quota_and_never_block_owner() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let attacker = peer_named(&ledger, "host-a");
    let bystander = peer_named(&ledger, "host-b");
    // when: the attacker never closes a window
    let issued = (0..2_000)
        .map(|_| ledger.issue_window(&attacker, 100, 1_000))
        .take_while(|result| result.is_ok())
        .count();
    // then
    assert_eq!(issued, Limits::default().principal_windows);
    assert!(matches!(
        ledger.issue_window(&attacker, 100, 1_000),
        Err(Error::Capacity {
            scope: CapacityScope::PrincipalRecords("windows"),
            ..
        })
    ));
    assert!(ledger.issue_window(&bystander, 100, 1_000).is_ok());
    assert!(ledger.issue_window(&local(), 100, 1_000).is_ok());
    // and: expiry returns the attacker's own quota without any external scheduler
    assert!(ledger.issue_window(&attacker, 1_101, 1_000).is_ok());
}

#[test]
fn given_peers_filling_shared_window_pool_should_leave_the_owner_reserve() {
    // given
    let limits = Limits {
        windows: 6,
        principal_windows: 2,
        local_reserved_windows: 2,
        ..Limits::default()
    };
    let ledger = Ledger::in_memory("daemon-a", limits).unwrap();
    let peers: Vec<_> = ["host-a", "host-b", "host-c"]
        .iter()
        .map(|id| peer_named(&ledger, id))
        .collect();
    for caller in &peers[..2] {
        for _ in 0..2 {
            ledger.issue_window(caller, 100, 1_000).unwrap();
        }
    }
    // when
    let third = ledger.issue_window(&peers[2], 100, 1_000);
    // then
    assert!(matches!(
        third,
        Err(Error::Capacity {
            scope: CapacityScope::PeerRecords("windows"),
            limit: 4
        })
    ));
    assert!(ledger.issue_window(&local(), 100, 1_000).is_ok());
    assert!(ledger.issue_window(&local(), 100, 1_000).is_ok());
    assert!(is_capacity(&ledger.issue_window(&local(), 100, 1_000)));
}

#[test]
fn given_peer_flooding_operations_on_its_live_run_should_never_block_owner_takeover() {
    // given
    let limits = Limits {
        operations: 8,
        principal_operations: 4,
        local_reserved_operations: 2,
        ..Limits::default()
    };
    let ledger = Ledger::in_memory("daemon-a", limits).unwrap();
    let attacker = peer_named(&ledger, "host-a");
    let window = ledger.issue_window(&attacker, 100, 1_000).unwrap();
    let launch = ledger
        .admit_launch(
            &attacker,
            &OperationKey {
                window: window.id,
                id: OperationId::new("spawn").unwrap(),
                digest: PayloadDigest([1; 32]),
            },
            &intent(),
            None,
            100,
        )
        .unwrap()
        .value;
    let mut guard = launch.guard;
    // when: idempotent re-acquisition on the live run, each in a fresh operation
    let mut accepted = 1;
    for index in 0..20 {
        let key = OperationKey {
            window: window.id,
            id: OperationId::new(format!("acquire-{index}")).unwrap(),
            digest: PayloadDigest([2; 32]),
        };
        match ledger.acquire_control(&attacker, &key, &guard, 100) {
            Ok(recorded) => {
                guard = recorded.value.guard();
                accepted += 1;
            }
            Err(error) => {
                assert!(matches!(
                    error,
                    Error::Capacity {
                        scope: CapacityScope::PrincipalRecords("operations"),
                        ..
                    }
                ));
                break;
            }
        }
    }
    // then
    assert_eq!(accepted, 4);
    let owner_window = ledger.issue_window(&local(), 100, 60).unwrap();
    let takeover = ledger.takeover(
        &admin(),
        &OperationKey {
            window: owner_window.id,
            id: OperationId::new("owner-takeover").unwrap(),
            digest: PayloadDigest([4; 32]),
        },
        &guard,
        None,
        100,
    );
    assert!(takeover.unwrap().fresh);
}

#[test]
fn given_thousands_of_completed_runs_should_continue_admitting_new_work() {
    // given
    let limits = Limits {
        agents: 32,
        runs: 32,
        windows: 32,
        operations: 32,
        local_reserved_windows: 4,
        local_reserved_operations: 4,
        retained_terminal_runs: 16,
        retention_batch: 4,
        ..Limits::default()
    };
    let ledger = Ledger::in_memory("daemon-a", limits.clone()).unwrap();
    let caller = local();
    // when: a lifetime far beyond every record budget
    for index in 0..2_000 {
        finished(&ledger, &caller, 100 + index as i64, &format!("op-{index}"));
    }
    // then: new work is admitted and terminal history is bounded by the horizon
    let fresh = finished(&ledger, &caller, 100_000, "after-history");
    assert_eq!(
        ledger.run(&fresh.run_id).unwrap().lifecycle,
        Lifecycle::Failed
    );
    let horizon = limits.retained_terminal_runs + limits.retention_batch;
    assert!(rows(&ledger, "runs") <= horizon);
    assert!(rows(&ledger, "agents") <= horizon);
    assert!(rows(&ledger, "windows") <= limits.retention_batch + 1);
    assert!(rows(&ledger, "operations") <= limits.retention_batch + 1);
}

#[test]
fn given_many_replaced_writers_with_certain_receipts_should_keep_opening_writers() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            writers: 2,
            input_receipts: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = local();
    let mut agent = running(&ledger, &caller);
    let mut previous: Option<WriterCheckpoint> = None;
    // when
    for index in 0..20u8 {
        let writer = ledger
            .open_writer(&caller, &agent.guard(), previous)
            .unwrap();
        agent = ledger.agent(&agent.agent_id).unwrap();
        for sequence in 1..=3 {
            ledger
                .accept_input(
                    &caller,
                    &agent.guard(),
                    writer.id,
                    sequence,
                    PayloadDigest([index; 32]),
                    4,
                )
                .unwrap();
            ledger
                .finish_input(writer.id, sequence, InputOutcome::Written)
                .unwrap();
        }
        previous = Some(ledger.writer(writer.id).unwrap().checkpoint());
    }
    // then: only the current writer and a bounded backlog remain
    assert!(rows(&ledger, "writers") <= 2 + Limits::default().retention_batch);
}

#[test]
fn given_reopen_should_compact_terminal_history_without_touching_unknown_evidence() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let limits = Limits {
        retained_terminal_runs: 1,
        ..Limits::default()
    };
    let ledger = Ledger::open(&path, "daemon-a", limits.clone()).unwrap();
    let caller = local();
    let oldest = finished(&ledger, &caller, 100, "oldest");
    let newest = finished(&ledger, &caller, 101, "newest");
    let unknown_key = key(&ledger, &caller, "unknown");
    let unknown_intent = intent();
    let unknown = ledger
        .admit_launch(&caller, &unknown_key, &unknown_intent, None, 100)
        .unwrap()
        .value;
    ledger.mark_unknown(&unknown.run_id).unwrap();
    let agent = running(&ledger, &caller);
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    ledger
        .accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            1,
            PayloadDigest([5; 32]),
            4,
        )
        .unwrap();
    drop(ledger);
    // when
    let ledger = Ledger::open(&path, "daemon-a", limits).unwrap();
    // then: old terminal history is gone, newest within the horizon remains
    assert!(matches!(
        ledger.run(&oldest.run_id),
        Err(Error::NotFound("run"))
    ));
    assert!(matches!(
        ledger.agent(&oldest.agent_id),
        Err(Error::NotFound("agent"))
    ));
    assert_eq!(
        ledger.run(&newest.run_id).unwrap().lifecycle,
        Lifecycle::Failed
    );
    // and: unresolved evidence is untouched and its receipt replays read-only
    assert_eq!(
        ledger.run(&unknown.run_id).unwrap().lifecycle,
        Lifecycle::Unknown
    );
    let replay = ledger
        .admit_launch(&caller, &unknown_key, &unknown_intent, None, 102)
        .unwrap();
    assert_eq!(replay.value, unknown);
    assert!(!replay.fresh);
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 2);
    // and: identifiers never rewind
    assert!(ledger.issue_window(&caller, 103, 10).unwrap().id.0 > unknown_key.window.0);
}

#[test]
fn given_retained_receipt_for_terminal_run_should_remain_readable_until_horizon() {
    // given: a terminal run whose receipt lives in a long window
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let kept_intent = intent();
    let window = ledger.issue_window(&caller, 100, 10_000).unwrap();
    let operation = OperationKey {
        window: window.id,
        id: OperationId::new("kept").unwrap(),
        digest: PayloadDigest([8; 32]),
    };
    let kept = ledger
        .admit_launch(&caller, &operation, &kept_intent, None, 100)
        .unwrap()
        .value;
    ledger
        .resolve_run(&kept.run_id, &prelaunch_failure())
        .unwrap();
    // when: far more terminal runs than the run horizon complete meanwhile
    for index in 0..(Limits::default().retained_terminal_runs + 50) {
        finished(
            &ledger,
            &caller,
            200 + index as i64,
            &format!("later-{index}"),
        );
    }
    ledger.compact(5_000).unwrap();
    // then: the receipt and its run stay readable while the window is open
    assert_eq!(
        ledger.operation_receipt(&caller, &operation).unwrap(),
        Some(OperationReceipt::Launch(kept.clone()))
    );
    assert_eq!(
        ledger.run(&kept.run_id).unwrap().lifecycle,
        Lifecycle::Failed
    );
    let replay = ledger
        .admit_launch(&caller, &operation, &kept_intent, None, 5_000)
        .unwrap();
    assert!(!replay.fresh);
    // when: the window expires
    ledger.compact(10_101).unwrap();
    // then: receipt and run are compacted, and the old identity can never relaunch
    assert!(matches!(
        ledger.operation_receipt(&caller, &operation),
        Err(Error::WindowClosed)
    ));
    assert!(matches!(
        ledger.run(&kept.run_id),
        Err(Error::NotFound("run"))
    ));
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &kept_intent, None, 100),
        Err(Error::WindowClosed)
    ));
}

#[test]
fn given_peer_accumulating_uncertain_input_should_exhaust_only_its_own_quota() {
    // given: a two-receipt quota per peer and a larger global budget
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            principal_input_receipts: 2,
            input_receipts: 16,
            ..Limits::default()
        },
    )
    .unwrap();
    let noisy = peer_named(&ledger, "noisy-peer");
    let quiet = peer_named(&ledger, "quiet-peer");
    let partial_inputs = |caller: &Caller, count: u64| {
        let agent = running(&ledger, caller);
        let writer = ledger.open_writer(caller, &agent.guard(), None).unwrap();
        (1..=count)
            .map(|sequence| {
                let accepted = ledger.accept_input(
                    caller,
                    &agent.guard(),
                    writer.id,
                    sequence,
                    PayloadDigest([sequence as u8; 32]),
                    8,
                );
                if accepted.is_ok() {
                    ledger
                        .finish_input(writer.id, sequence, InputOutcome::Partial { bytes: 4 })
                        .unwrap();
                }
                accepted
            })
            .collect::<Vec<_>>()
    };

    // when: one peer keeps leaving uncertain evidence behind
    let noisy_results = partial_inputs(&noisy, 3);

    // then: only that peer is refused; another peer can still write input
    assert!(noisy_results[0].is_ok() && noisy_results[1].is_ok());
    assert!(matches!(
        noisy_results[2],
        Err(Error::Capacity {
            scope: CapacityScope::PrincipalRecords("inputs"),
            ..
        })
    ));
    assert!(partial_inputs(&quiet, 1)[0].is_ok());
}

#[test]
fn given_uncertain_input_of_ended_runs_should_be_retained_without_blocking_new_input() {
    // given: a peer's quota filled by uncertain evidence of one run
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            principal_input_receipts: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let peer = peer_named(&ledger, "peer");
    let first = running(&ledger, &peer);
    let writer = ledger.open_writer(&peer, &first.guard(), None).unwrap();
    ledger
        .accept_input(
            &peer,
            &first.guard(),
            writer.id,
            1,
            PayloadDigest([1; 32]),
            8,
        )
        .unwrap();
    ledger
        .finish_input(writer.id, 1, InputOutcome::Partial { bytes: 4 })
        .unwrap();

    // when: that run ends with proof
    ledger
        .resolve_run(
            &first.current_run,
            &OutcomeProof::ProcessTreeTerminated {
                evidence: "tree ended".into(),
                interrupted: false,
            },
        )
        .unwrap();

    // then: the evidence is still readable and a new run accepts input
    assert!(ledger.input_receipt(writer.id, 1).unwrap().is_some());
    let second = running(&ledger, &peer);
    let writer = ledger.open_writer(&peer, &second.guard(), None).unwrap();
    assert!(
        ledger
            .accept_input(
                &peer,
                &second.guard(),
                writer.id,
                1,
                PayloadDigest([2; 32]),
                8
            )
            .is_ok()
    );
}
