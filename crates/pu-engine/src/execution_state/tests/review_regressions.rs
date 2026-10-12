use super::*;

fn clock(ledger: &Ledger) -> i64 {
    ledger
        .transaction(|tx| {
            Ok(
                tx.query_row("SELECT last_time FROM meta WHERE singleton=1", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .unwrap()
}

#[test]
fn given_window_capacity_rejection_should_preserve_observed_expiry_time() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            windows: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "expires");
    // Budgets count only open, unexpired windows: fill both slots with live ones.
    ledger.issue_window(&caller, 100, 1_000).unwrap();
    ledger.issue_window(&caller, 200, 1_000).unwrap();
    // when
    assert!(matches!(
        ledger.issue_window(&caller, 250, 100),
        Err(Error::Capacity {
            scope: CapacityScope::Records("windows"),
            ..
        })
    ));
    // then
    assert_eq!(clock(&ledger), 250);
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &intent(), None, 100),
        Err(Error::WindowClosed)
    ));
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
}

#[test]
fn given_post_clock_rejections_should_keep_time_and_roll_back_operation_changes() {
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
    let operation = key(&ledger, &caller, "original");
    let request = intent();
    let first = ledger
        .admit_launch(&caller, &operation, &request, None, 100)
        .unwrap()
        .value;
    let mut conflict = operation.clone();
    conflict.digest = PayloadDigest([8; 32]);
    // when / then: conflict, stale transition, capacity and invalid arguments all observe time
    assert!(matches!(
        ledger.admit_launch(&caller, &conflict, &request, None, 200),
        Err(Error::PayloadConflict)
    ));
    assert_eq!(clock(&ledger), 200);
    let current = ledger.begin_start(&caller, &first.guard).unwrap();
    let stop_key = OperationKey {
        window: ledger.issue_window(&caller, 200, 100).unwrap().id,
        id: OperationId::new("stale-stop").unwrap(),
        digest: PayloadDigest([7; 32]),
    };
    assert!(matches!(
        ledger.request_stop(&caller, &stop_key, &first.guard, 201),
        Err(Error::Stale(_))
    ));
    assert_eq!(clock(&ledger), 201);
    assert_eq!(ledger.agent(&first.agent_id).unwrap(), current);
    assert_eq!(ledger.operation_receipt(&caller, &stop_key).unwrap(), None);
    assert!(matches!(
        ledger.admit_launch(&caller, &stop_key, &intent(), None, 202),
        Err(Error::Capacity {
            scope: CapacityScope::Global,
            ..
        })
    ));
    assert_eq!(clock(&ledger), 202);
    assert!(matches!(
        ledger.issue_window(&caller, 203, 0),
        Err(Error::Invalid(_))
    ));
    assert_eq!(clock(&ledger), 203);
    let mut invalid = intent();
    invalid.source_commit = "main".into();
    assert!(matches!(
        ledger.admit_launch(&caller, &stop_key, &invalid, None, 204),
        Err(Error::Invalid(_))
    ));
    assert_eq!(clock(&ledger), 204);
    let other = Caller::authenticated_local("another-client").unwrap();
    assert!(matches!(
        ledger.admit_launch(&other, &stop_key, &intent(), None, 205),
        Err(Error::Unauthorized)
    ));
    assert_eq!(clock(&ledger), 205);
    let missing_peer = Caller::authenticated_peer("unregistered", 1).unwrap();
    assert!(matches!(
        ledger.issue_window(&missing_peer, 206, 100),
        Err(Error::PeerFenced)
    ));
    assert_eq!(clock(&ledger), 206);
    assert!(matches!(
        ledger.issue_window(&caller, -1, 100),
        Err(Error::Invalid(_))
    ));
    assert_eq!(clock(&ledger), 206);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_late_window_error_should_roll_back_every_operation_write() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let window = ledger.issue_window(&caller, 100, 100).unwrap();
    // when: simulate a failure after the operation has already made changes
    let result: Result<()> = ledger.transaction_at(200, |tx, _| {
        tx.execute("DELETE FROM windows WHERE id=?1", [window.id.0])?;
        tx.execute(
            "UPDATE meta SET next_window=next_window+1 WHERE singleton=1",
            [],
        )?;
        Err(Error::WindowClosed)
    });
    // then: the former error-specific commit must never publish those changes
    assert!(matches!(result, Err(Error::WindowClosed)));
    assert_eq!(clock(&ledger), 200);
    let retained: u64 = ledger
        .transaction(|tx| {
            Ok(tx.query_row(
                "SELECT count(*) FROM windows WHERE id=?1",
                [window.id.0],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(retained, 1);
    assert_eq!(
        ledger.issue_window(&caller, 100, 100).unwrap().id.0,
        window.id.0 + 1
    );
}

#[test]
fn given_invalid_expiry_after_counter_write_should_preserve_time_but_not_consume_id() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let window = ledger.issue_window(&caller, 100, 100).unwrap();
    // when: expiry overflow follows the tentative receiver-issued counter update
    assert!(matches!(
        ledger.issue_window(&caller, i64::MAX - 1, 100),
        Err(Error::Invalid(_))
    ));
    // then
    assert_eq!(clock(&ledger), i64::MAX - 1);
    let next = ledger.issue_window(&caller, 100, 1).unwrap();
    assert_eq!(next.id.0, window.id.0 + 1);
    assert_eq!(next.expires_at, i64::MAX);
}

#[test]
fn given_operation_commit_failure_should_leave_no_acceptance_and_keep_durable_time() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    let caller = local();
    let operation = OperationKey {
        window: ledger.issue_window(&caller, 100, 1000).unwrap().id,
        id: OperationId::new("commit-failure").unwrap(),
        digest: PayloadDigest([7; 32]),
    };
    ledger.fail_next_commit();
    // when
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &intent(), None, 200),
        Err(Error::Storage(_))
    ));
    // then
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
    assert_eq!(ledger.operation_receipt(&caller, &operation).unwrap(), None);
    assert_eq!(clock(&ledger), 200);
    drop(ledger);
    let db = rusqlite::Connection::open(&path).unwrap();
    let persisted: i64 = db
        .query_row("SELECT last_time FROM meta WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(persisted, 200);
}

#[test]
fn given_clock_commit_failure_should_prevent_operation_and_keep_live_time_fence() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "expires");
    ledger.fail_next_clock_commit();
    // when: the operation must not run if its preceding durable time fence fails
    assert!(matches!(
        ledger.issue_window(&caller, 200, 100),
        Err(Error::Storage(_))
    ));
    // then: no window was issued; this instance retains observed time for a retry
    assert_eq!(clock(&ledger), 100);
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &intent(), None, 100),
        Err(Error::WindowClosed)
    ));
    assert_eq!(clock(&ledger), 200);
    assert_eq!(
        ledger.issue_window(&caller, 100, 100).unwrap().id.0,
        operation.window.0 + 1
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
}

#[test]
fn given_missing_latest_input_receipt_should_reject_before_recovery() {
    // given: deleting any latest outcome is impossible under normal compaction
    for outcome in [
        InputOutcome::Accepted,
        InputOutcome::Written,
        InputOutcome::Partial { bytes: 3 },
        InputOutcome::Unknown,
    ] {
        let dir = private_dir();
        let path = dir.path().join("ledger.db");
        let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
        let caller = local();
        let agent = running(&ledger, &caller);
        let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
        ledger
            .accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                1,
                PayloadDigest([1; 32]),
                10,
            )
            .unwrap();
        if outcome != InputOutcome::Accepted {
            ledger.finish_input(writer.id, 1, outcome).unwrap();
        }
        drop(ledger);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("DELETE FROM inputs", []).unwrap();
        let writer_before: String = db
            .query_row("SELECT data FROM writers", [], |r| r.get(0))
            .unwrap();
        drop(db);
        // when / then
        assert!(matches!(
            Ledger::open(&path, "daemon-a", Limits::default()),
            Err(Error::Corrupt(_))
        ));
        let db = rusqlite::Connection::open(&path).unwrap();
        let writer_after: String = db
            .query_row("SELECT data FROM writers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(writer_after, writer_before);
    }
}

#[test]
fn given_accepted_input_below_writer_high_water_should_reject_even_with_latest_receipt() {
    // given
    for add_latest in [false, true] {
        let dir = private_dir();
        let path = dir.path().join("ledger.db");
        let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
        let caller = local();
        let agent = running(&ledger, &caller);
        let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
        let accepted = ledger
            .accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                1,
                PayloadDigest([1; 32]),
                10,
            )
            .unwrap()
            .value;
        drop(ledger);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute(
            "UPDATE writers SET data=json_set(data, '$.last_sequence', 2)",
            [],
        )
        .unwrap();
        if add_latest {
            let latest = InputAck {
                sequence: 2,
                outcome: InputOutcome::Written,
                ..accepted
            };
            db.execute(
                "INSERT INTO inputs VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    writer.id.0,
                    latest.sequence,
                    serde_json::to_string(&latest).unwrap()
                ],
            )
            .unwrap();
        }
        drop(db);
        // when / then
        assert!(matches!(
            Ledger::open(&path, "daemon-a", Limits::default()),
            Err(Error::Corrupt(_))
        ));
    }
}

#[test]
fn given_compacted_older_written_receipts_should_reopen_with_latest_evidence_intact() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let ledger = Ledger::open(
        &path,
        "daemon-a",
        Limits {
            receipts_per_writer: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = local();
    let agent = running(&ledger, &caller);
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    ledger
        .accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            1,
            PayloadDigest([1; 32]),
            10,
        )
        .unwrap();
    ledger
        .finish_input(writer.id, 1, InputOutcome::Written)
        .unwrap();
    ledger
        .accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            2,
            PayloadDigest([2; 32]),
            10,
        )
        .unwrap();
    assert_eq!(ledger.input_receipt(writer.id, 1).unwrap(), None);
    drop(ledger);
    // when
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    // then
    assert_eq!(ledger.writer(writer.id).unwrap().last_sequence, 2);
    assert_eq!(ledger.input_receipt(writer.id, 1).unwrap(), None);
    assert_eq!(
        ledger.input_receipt(writer.id, 2).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
}
