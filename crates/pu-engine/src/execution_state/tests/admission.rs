use super::*;

#[test]
fn given_distinct_callers_racing_should_enforce_one_global_cap() {
    // given
    let dir = private_dir();
    let ledger = Arc::new(
        Ledger::open(
            dir.path().join("ledger.db"),
            "daemon-a",
            Limits {
                global_runs: 3,
                ..Limits::default()
            },
        )
        .unwrap(),
    );
    let barrier = Arc::new(Barrier::new(12));
    // when
    let threads: Vec<_> = (0..12)
        .map(|n| {
            let caller = Caller::authenticated_local(format!("client-{n}")).unwrap();
            let operation = key(&ledger, &caller, "race");
            let (ledger, barrier) = (ledger.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                ledger.admit_launch(&caller, &operation, &intent(), None, 100)
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    // then
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 3);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(
                r,
                Err(Error::Capacity {
                    scope: CapacityScope::Global,
                    limit: 3
                })
            ))
            .count(),
        9
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 3);
}

#[test]
fn given_each_admission_budget_should_reject_without_receipt_or_reservation() {
    // given
    for (scope, limits) in [
        (
            CapacityScope::Project,
            Limits {
                project_runs: 1,
                ..Limits::default()
            },
        ),
        (
            CapacityScope::Runtime,
            Limits {
                runtime_runs: 1,
                ..Limits::default()
            },
        ),
        (
            CapacityScope::Principal,
            Limits {
                principal_runs: 1,
                ..Limits::default()
            },
        ),
        (
            CapacityScope::Starting,
            Limits {
                starting_runs: 1,
                ..Limits::default()
            },
        ),
    ] {
        let ledger = Ledger::in_memory("daemon-a", limits).unwrap();
        let caller = local();
        let first = launch(&ledger, &caller);
        ledger.mark_unknown(&first.run_id).unwrap();
        let operation = key(&ledger, &caller, "over-budget");
        // when
        let result = ledger.admit_launch(&caller, &operation, &intent(), None, 100);
        // then
        assert!(
            matches!(result, Err(Error::Capacity { scope: actual, limit: 1 }) if actual == scope)
        );
        assert_eq!(ledger.operation_receipt(&caller, &operation).unwrap(), None);
        assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    }
}

#[test]
fn given_stop_or_host_loss_should_retain_slot_until_explicit_tree_proof() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            global_runs: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = local();
    let current = running(&ledger, &caller);
    let operation = key(&ledger, &caller, "stop");
    // when
    let stopping = ledger
        .request_stop(&caller, &operation, &current.guard(), 100)
        .unwrap();
    // then
    assert!(stopping.fresh);
    assert!(
        !ledger
            .request_stop(&caller, &operation, &current.guard(), 100)
            .unwrap()
            .fresh
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    assert!(matches!(
        ledger.resolve_run(
            &current.current_run,
            &OutcomeProof::PrelaunchFailure {
                evidence: "missing PID".into()
            }
        ),
        Err(Error::InvalidTransition)
    ));
    ledger.mark_unknown(&current.current_run).unwrap();
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    let proof = OutcomeProof::ProcessTreeTerminated {
        evidence: "supervisor tree token reaped with no descendants".into(),
        interrupted: true,
    };
    ledger.resolve_run(&current.current_run, &proof).unwrap();
    ledger.resolve_run(&current.current_run, &proof).unwrap();
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
    launch(&ledger, &caller);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_starting_intent_should_reject_second_start_and_stale_revision() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let receipt = launch(&ledger, &caller);
    // when
    let starting = ledger.begin_start(&caller, &receipt.guard).unwrap();
    // then
    assert!(matches!(
        ledger.begin_start(&caller, &receipt.guard),
        Err(Error::Stale(_))
    ));
    assert!(matches!(
        ledger.begin_start(&caller, &starting.guard()),
        Err(Error::InvalidTransition)
    ));
    let unknown = ledger.mark_unknown(&receipt.run_id).unwrap();
    assert!(matches!(
        ledger.begin_start(&caller, &unknown.guard()),
        Err(Error::InvalidTransition)
    ));
    assert!(matches!(
        ledger.admit_launch(
            &caller,
            &key(&ledger, &caller, "resume-unknown"),
            &intent(),
            Some(&unknown.guard()),
            100
        ),
        Err(Error::RunUnresolved)
    ));
}

#[test]
fn given_closed_window_with_live_operation_should_preserve_receipt_and_reject_new_write() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "original");
    let request = intent();
    let original = ledger
        .admit_launch(&caller, &operation, &request, None, 100)
        .unwrap();
    ledger.close_window(&caller, operation.window).unwrap();
    // when
    ledger.compact(201).unwrap();
    let duplicate = ledger
        .admit_launch(&caller, &operation, &request, None, 201)
        .unwrap();
    // then
    assert_eq!(duplicate.value, original.value);
    assert!(!duplicate.fresh);
    let new = OperationKey {
        id: OperationId::new("new").unwrap(),
        ..operation
    };
    assert!(matches!(
        ledger.admit_launch(&caller, &new, &request, None, 201),
        Err(Error::WindowClosed)
    ));
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_expiry_observed_on_rejected_request_should_not_resurrect_after_clock_rollback() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "expires");
    let request = intent();
    // when
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &request, None, 200),
        Err(Error::WindowClosed)
    ));
    // then
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &request, None, 100),
        Err(Error::WindowClosed)
    ));
}

#[test]
fn given_record_budgets_should_reject_and_never_evict_live_or_unknown_runs() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            operations: 1,
            runs: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = local();
    let first = launch(&ledger, &caller);
    ledger.mark_unknown(&first.run_id).unwrap();
    // when
    ledger.compact(201).unwrap();
    // then
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    let operation = OperationKey {
        window: ledger.issue_window(&caller, 201, 100).unwrap().id,
        id: OperationId::new("bounded").unwrap(),
        digest: PayloadDigest([1; 32]),
    };
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &intent(), None, 201),
        Err(Error::Capacity {
            scope: CapacityScope::Records("operations"),
            limit: 1
        })
    ));
}
