use super::*;

#[test]
fn given_sequenced_input_should_deduplicate_conflict_reject_and_preserve_partial() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let agent = running(&ledger, &caller);
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    // when
    let first = ledger
        .accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            1,
            PayloadDigest([1; 32]),
            10,
        )
        .unwrap();
    let duplicate = ledger
        .accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            1,
            PayloadDigest([1; 32]),
            10,
        )
        .unwrap();
    // then
    assert!(first.fresh);
    assert!(!duplicate.fresh);
    assert_eq!(duplicate.value, first.value);
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            1,
            PayloadDigest([2; 32]),
            10
        ),
        Err(Error::PayloadConflict)
    ));
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            3,
            PayloadDigest([3; 32]),
            10
        ),
        Err(Error::SequenceGap { expected: 2 })
    ));
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            2,
            PayloadDigest([2; 32]),
            10
        ),
        Err(Error::InputPending)
    ));
    let partial = ledger
        .finish_input(writer.id, 1, InputOutcome::Partial { bytes: 4 })
        .unwrap();
    assert_eq!(partial.outcome, InputOutcome::Partial { bytes: 4 });
    assert_eq!(
        ledger
            .accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                1,
                PayloadDigest([1; 32]),
                10
            )
            .unwrap()
            .value,
        partial
    );
    assert!(matches!(
        ledger.finish_input(writer.id, 1, InputOutcome::Written),
        Err(Error::InvalidTransition)
    ));
}

#[test]
fn given_restart_should_fence_writer_and_preserve_uncertain_delivery_evidence() {
    // given
    let dir = private_dir();
    let path = dir.path().join("execution.sqlite");
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
    drop(ledger);
    // when
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    // then
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
    let current = ledger.agent(&agent.agent_id).unwrap();
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &current.guard(),
            writer.id,
            1,
            PayloadDigest([1; 32]),
            10
        ),
        Err(Error::WriterFenced)
    ));
    assert!(matches!(
        ledger.finish_input(writer.id, 1, InputOutcome::Written),
        Err(Error::WriterFenced)
    ));
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_replaced_and_compacted_writer_should_never_restart_old_sequences() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            receipts_per_writer: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = local();
    let agent = running(&ledger, &caller);
    let first = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    ledger
        .accept_input(
            &caller,
            &agent.guard(),
            first.id,
            1,
            PayloadDigest([1; 32]),
            10,
        )
        .unwrap();
    ledger
        .finish_input(first.id, 1, InputOutcome::Written)
        .unwrap();
    ledger
        .accept_input(
            &caller,
            &agent.guard(),
            first.id,
            2,
            PayloadDigest([2; 32]),
            10,
        )
        .unwrap();
    ledger
        .finish_input(first.id, 2, InputOutcome::Written)
        .unwrap();
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            first.id,
            1,
            PayloadDigest([1; 32]),
            10
        ),
        Err(Error::ReceiptExpired)
    ));
    // when
    assert!(matches!(
        ledger.open_writer(&caller, &agent.guard(), None),
        Err(Error::ReconciliationRequired)
    ));
    let checkpoint = ledger.writer(first.id).unwrap().checkpoint();
    let replacement = ledger
        .open_writer(&caller, &agent.guard(), Some(checkpoint))
        .unwrap();
    ledger.compact(201).unwrap();
    // then
    assert!(replacement.id.0 > first.id.0);
    assert!(matches!(ledger.writer(first.id), Err(Error::WriterFenced)));
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            first.id,
            1,
            PayloadDigest([1; 32]),
            10
        ),
        Err(Error::WriterFenced)
    ));
    assert!(
        ledger
            .accept_input(
                &caller,
                &agent.guard(),
                replacement.id,
                1,
                PayloadDigest([3; 32]),
                10
            )
            .unwrap()
            .fresh
    );
}

#[test]
fn given_ambiguous_receipts_at_budget_should_reject_without_erasing_evidence() {
    // given
    let ledger = Ledger::in_memory(
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
        .finish_input(writer.id, 1, InputOutcome::Unknown)
        .unwrap();
    // when / then
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            2,
            PayloadDigest([2; 32]),
            10
        ),
        Err(Error::Capacity {
            scope: CapacityScope::Records("writer receipts"),
            limit: 1
        })
    ));
    let replacement = ledger
        .open_writer(
            &caller,
            &agent.guard(),
            Some(ledger.writer(writer.id).unwrap().checkpoint()),
        )
        .unwrap();
    ledger.compact(201).unwrap();
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
    assert_eq!(ledger.writer(writer.id).unwrap().last_sequence, 1);
    assert_ne!(replacement.id, writer.id);
}

#[test]
fn given_fencing_pending_writer_should_mark_unknown_and_forbid_old_delivery() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
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
    // when
    let taken = ledger
        .takeover(
            &admin(),
            &key(&ledger, &caller, "takeover"),
            &agent.guard(),
            None,
            100,
        )
        .unwrap()
        .value;
    // then
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
    assert!(matches!(
        ledger.finish_input(writer.id, 1, InputOutcome::Written),
        Err(Error::WriterFenced)
    ));
    assert_eq!(taken.controller, None);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_failed_input_acceptance_should_not_advance_sequence_or_return_acceptance() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let agent = running(&ledger, &caller);
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    ledger.fail_next_commit();
    // when
    let result = ledger.accept_input(
        &caller,
        &agent.guard(),
        writer.id,
        1,
        PayloadDigest([1; 32]),
        10,
    );
    // then
    assert!(matches!(result, Err(Error::Storage(_))));
    assert_eq!(ledger.writer(writer.id).unwrap().last_sequence, 0);
    assert_eq!(ledger.input_receipt(writer.id, 1).unwrap(), None);
    assert!(
        ledger
            .accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                1,
                PayloadDigest([1; 32]),
                10
            )
            .unwrap()
            .fresh
    );
}

#[test]
fn given_write_result_commit_failure_should_not_allow_a_second_delivery_attempt() {
    // given
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
    // when: the engine may have written the bytes before this failed commit
    ledger.fail_next_commit();
    assert!(matches!(
        ledger.finish_input(writer.id, 1, InputOutcome::Written),
        Err(Error::Storage(_))
    ));
    // then: duplicate returns evidence only, and a restart keeps the uncertainty
    assert!(
        !ledger
            .accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                1,
                PayloadDigest([1; 32]),
                10
            )
            .unwrap()
            .fresh
    );
    drop(ledger);
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
}

#[test]
fn given_invalid_sequence_size_or_partial_count_should_reject_without_panic() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let agent = running(&ledger, &caller);
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    // when / then
    for seq in [0, u64::MAX] {
        assert!(matches!(
            ledger.accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                seq,
                PayloadDigest([1; 32]),
                10
            ),
            Err(Error::Invalid(_))
        ));
    }
    for bytes in [0, u32::MAX] {
        assert!(matches!(
            ledger.accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                1,
                PayloadDigest([1; 32]),
                bytes
            ),
            Err(Error::Invalid(_))
        ));
    }
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
    for bytes in [0, 10, u32::MAX] {
        assert!(matches!(
            ledger.finish_input(writer.id, 1, InputOutcome::Partial { bytes }),
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Accepted
    );
}

#[test]
fn given_global_writer_or_input_budget_should_not_fence_or_advance_on_rejection() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            writers: 1,
            input_receipts: 1,
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
        .finish_input(writer.id, 1, InputOutcome::Partial { bytes: 3 })
        .unwrap();
    // when / then: uncertain evidence, unlike Written receipts, consumes the budget
    assert!(matches!(
        ledger.open_writer(
            &caller,
            &agent.guard(),
            Some(ledger.writer(writer.id).unwrap().checkpoint())
        ),
        Err(Error::Capacity {
            scope: CapacityScope::Records("writers"),
            ..
        })
    ));
    assert!(ledger.writer(writer.id).unwrap().live);
    assert!(matches!(
        ledger.accept_input(
            &caller,
            &agent.guard(),
            writer.id,
            2,
            PayloadDigest([2; 32]),
            10
        ),
        Err(Error::Capacity {
            scope: CapacityScope::Records("inputs"),
            ..
        })
    ));
    assert_eq!(ledger.writer(writer.id).unwrap().last_sequence, 1);
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Partial { bytes: 3 }
    );
}

#[test]
fn given_partial_written_and_unknown_receipts_should_preserve_each_outcome_across_reopen() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    let caller = local();
    let agent = running(&ledger, &caller);
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    let outcomes = [
        InputOutcome::Written,
        InputOutcome::Partial { bytes: 3 },
        InputOutcome::Unknown,
    ];
    for (index, outcome) in outcomes.iter().enumerate() {
        let sequence = (index + 1) as u64;
        ledger
            .accept_input(
                &caller,
                &agent.guard(),
                writer.id,
                sequence,
                PayloadDigest([index as u8; 32]),
                10,
            )
            .unwrap();
        ledger
            .finish_input(writer.id, sequence, outcome.clone())
            .unwrap();
    }
    drop(ledger);
    // when
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    // then
    for (index, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            &ledger
                .input_receipt(writer.id, (index + 1) as u64)
                .unwrap()
                .unwrap()
                .outcome,
            outcome
        );
    }
    assert_eq!(ledger.writer(writer.id).unwrap().last_sequence, 3);
    assert!(!ledger.writer(writer.id).unwrap().live);
}
