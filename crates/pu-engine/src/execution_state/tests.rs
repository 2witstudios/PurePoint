use super::*;
use std::sync::{Arc, Barrier};

mod admission;
mod authority;
mod input;
mod storage;

fn admin() -> LocalAdmin {
    LocalAdmin::authorized_by_local_policy(&local()).unwrap()
}

fn running(ledger: &Ledger, caller: &Caller) -> AgentRecord {
    let receipt = launch(ledger, caller);
    ledger.begin_start(caller, &receipt.guard).unwrap();
    ledger
        .record_running(
            &receipt.run_id,
            &SupervisionIdentity {
                mechanism: "test-supervisor".into(),
                token: format!("tree:{}", receipt.run_id.0),
            },
        )
        .unwrap()
}

fn private_dir() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn local() -> Caller {
    Caller::authenticated_local("desktop").unwrap()
}

fn intent() -> LaunchIntent {
    LaunchIntent {
        project: "project-a".into(),
        export: Some("export-a".into()),
        runtime: "claude".into(),
        source_commit: "a".repeat(40),
        worktree: WorktreeIntent {
            id: pu_core::id::worktree_id(),
            path: "/private/project/worktree".into(),
            branch: "pu/example".into(),
        },
    }
}

fn key(ledger: &Ledger, caller: &Caller, name: &str) -> OperationKey {
    OperationKey {
        window: ledger.issue_window(caller, 100, 100).unwrap().id,
        id: OperationId::new(name).unwrap(),
        digest: PayloadDigest([7; 32]),
    }
}

fn launch(ledger: &Ledger, caller: &Caller) -> LaunchReceipt {
    ledger
        .admit_launch(caller, &key(ledger, caller, "spawn"), &intent(), None, 100)
        .unwrap()
        .value
}

#[test]
fn given_reopened_store_should_preserve_intent_and_hold_uncertain_capacity() {
    // given
    let dir = private_dir();
    let path = dir.path().join("execution.sqlite");
    let limits = Limits {
        global_runs: 1,
        ..Limits::default()
    };
    let ledger = Ledger::open(&path, "daemon-a", limits.clone()).unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "original");
    let requested = intent();
    let first = ledger
        .admit_launch(&caller, &operation, &requested, None, 100)
        .unwrap();
    drop(ledger);
    // when
    let ledger = Ledger::open(&path, "daemon-a", limits).unwrap();
    let duplicate = ledger
        .admit_launch(&caller, &operation, &requested, None, 101)
        .unwrap();
    // then
    assert_eq!(duplicate.value, first.value);
    assert!(!duplicate.fresh);
    let run = ledger.run(&first.value.run_id).unwrap();
    assert_eq!(run.intent, requested);
    assert_eq!(run.lifecycle, Lifecycle::Unknown);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    assert!(matches!(
        ledger.admit_launch(
            &caller,
            &key(&ledger, &caller, "excess"),
            &intent(),
            None,
            101
        ),
        Err(Error::Capacity {
            scope: CapacityScope::Global,
            ..
        })
    ));
}

#[test]
fn given_concurrent_duplicate_launches_should_reserve_once_and_reject_conflicts() {
    // given
    let ledger = Arc::new(
        Ledger::in_memory(
            "daemon-a",
            Limits {
                global_runs: 1,
                ..Limits::default()
            },
        )
        .unwrap(),
    );
    let caller = local();
    let operation = key(&ledger, &caller, "same");
    let requested = intent();
    let barrier = Arc::new(Barrier::new(8));
    // when
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (ledger, caller, operation, requested, barrier) = (
                ledger.clone(),
                caller.clone(),
                operation.clone(),
                requested.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                ledger
                    .admit_launch(&caller, &operation, &requested, None, 100)
                    .unwrap()
            })
        })
        .collect();
    let receipts: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    // then
    assert_eq!(receipts.iter().filter(|receipt| receipt.fresh).count(), 1);
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt.value == receipts[0].value)
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    let mut conflict = operation.clone();
    conflict.digest = PayloadDigest([8; 32]);
    assert!(matches!(
        ledger.admit_launch(&caller, &conflict, &requested, None, 100),
        Err(Error::PayloadConflict)
    ));
    let mut changed = requested.clone();
    changed.runtime = "codex".into();
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &changed, None, 100),
        Err(Error::PayloadConflict)
    ));
}

#[test]
fn given_failed_sqlite_commit_should_not_accept_or_consume_capacity() {
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
    let operation = key(&ledger, &caller, "commit-failure");
    ledger.fail_next_commit();
    // when
    let result = ledger.admit_launch(&caller, &operation, &intent(), None, 100);
    // then
    assert!(matches!(result, Err(Error::Storage(_))));
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
    assert!(
        ledger
            .operation_receipt(&caller, &operation)
            .unwrap()
            .is_none()
    );
    assert!(
        ledger
            .admit_launch(&caller, &operation, &intent(), None, 100)
            .unwrap()
            .fresh
    );
}

#[test]
fn given_expired_compacted_window_should_never_reaccept_old_operation() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "expired");
    let request = intent();
    let run = ledger
        .admit_launch(&caller, &operation, &request, None, 100)
        .unwrap()
        .value;
    ledger
        .resolve_run(
            &run.run_id,
            &OutcomeProof::PrelaunchFailure {
                evidence: "source verification failed before dispatch".into(),
            },
        )
        .unwrap();
    // when
    ledger.compact(201).unwrap();
    // then: even a rolled-back receiver clock cannot reissue old IDs
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &request, None, 100),
        Err(Error::WindowClosed)
    ));
    assert!(ledger.issue_window(&caller, 100, 100).unwrap().id.0 > operation.window.0);
}

#[test]
fn given_two_resume_attempts_should_create_only_one_new_run() {
    // given
    let ledger = Arc::new(Ledger::in_memory("daemon-a", Limits::default()).unwrap());
    let caller = local();
    let original = launch(&ledger, &caller);
    assert!(matches!(
        ledger.admit_launch(
            &caller,
            &key(&ledger, &caller, "unsafe"),
            &intent(),
            Some(&original.guard),
            100
        ),
        Err(Error::RunUnresolved)
    ));
    let proof = OutcomeProof::PrelaunchFailure {
        evidence: "dispatch was never entered".into(),
    };
    ledger.resolve_run(&original.run_id, &proof).unwrap();
    ledger.resolve_run(&original.run_id, &proof).unwrap();
    let guard = ledger.agent(&original.agent_id).unwrap().guard();
    let barrier = Arc::new(Barrier::new(2));
    // when
    let threads: Vec<_> = (0..2)
        .map(|n| {
            let (ledger, caller, guard, barrier) = (
                ledger.clone(),
                caller.clone(),
                guard.clone(),
                barrier.clone(),
            );
            let operation = key(&ledger, &caller, &format!("resume-{n}"));
            std::thread::spawn(move || {
                barrier.wait();
                ledger.admit_launch(&caller, &operation, &intent(), Some(&guard), 100)
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    // then
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    assert_ne!(
        ledger.agent(&original.agent_id).unwrap().current_run,
        original.run_id
    );
}
