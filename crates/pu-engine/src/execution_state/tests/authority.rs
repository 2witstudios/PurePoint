use super::*;

fn peer(ledger: &Ledger) -> Caller {
    let registered = ledger.register_peer(&admin(), "host-a").unwrap();
    let authorized = ledger
        .update_peer_authorization(
            &admin(),
            "host-a",
            registered.generation,
            PayloadDigest([9; 32]),
        )
        .unwrap();
    Caller::authenticated_peer("host-a", authorized.generation).unwrap()
}

#[test]
fn given_unregistered_or_revoked_peer_should_fail_inside_admission_and_dispatch() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let forged = Caller::authenticated_peer("host-a", 1).unwrap();
    assert!(matches!(
        ledger.issue_window(&forged, 100, 100),
        Err(Error::PeerFenced)
    ));
    assert!(matches!(
        LocalAdmin::authorized_by_local_policy(&forged),
        Err(Error::Unauthorized)
    ));
    let caller = peer(&ledger);
    let operation = key(&ledger, &caller, "spawn");
    let request = intent();
    let launch = ledger
        .admit_launch(&caller, &operation, &request, None, 100)
        .unwrap()
        .value;
    // when
    ledger.revoke_peer(&admin(), "host-a").unwrap();
    // then: cached receipts do not bypass revocation
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &request, None, 100),
        Err(Error::PeerFenced)
    ));
    assert!(matches!(
        ledger.validate_dispatch(&caller, &launch.guard),
        Err(Error::PeerFenced)
    ));
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    let current = ledger.agent(&launch.agent_id).unwrap();
    assert_eq!(current.controller, None);
    assert!(current.epoch.0 > launch.guard.epoch.0);
}

#[test]
fn given_takeover_should_fence_old_controller_and_keep_capacity() {
    // given
    let ledger = Ledger::in_memory(
        "daemon-a",
        Limits {
            global_runs: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let caller = peer(&ledger);
    let current = running(&ledger, &caller);
    let local_caller = local();
    // when
    let operation = key(&ledger, &local_caller, "takeover");
    let taken = ledger
        .takeover(
            &admin(),
            &operation,
            &current.guard(),
            Some(local_caller.principal().clone()),
            100,
        )
        .unwrap();
    // then
    assert!(taken.value.epoch.0 > current.epoch.0);
    assert!(matches!(
        ledger.validate_dispatch(&caller, &current.guard()),
        Err(Error::Stale(_))
    ));
    assert!(matches!(
        ledger.validate_dispatch(&caller, &taken.value.guard()),
        Err(Error::ControllerFenced)
    ));
    assert!(
        ledger
            .validate_dispatch(&local_caller, &taken.value.guard())
            .is_ok()
    );
    assert!(
        !ledger
            .takeover(
                &admin(),
                &operation,
                &current.guard(),
                Some(local_caller.principal().clone()),
                100
            )
            .unwrap()
            .fresh
    );
    assert!(matches!(
        ledger.admit_launch(
            &local_caller,
            &key(&ledger, &local_caller, "excess"),
            &intent(),
            None,
            100
        ),
        Err(Error::Capacity {
            scope: CapacityScope::Global,
            ..
        })
    ));
}

#[test]
fn given_host_generation_change_should_require_explicit_acquisition_and_fence_old_host() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let old = peer(&ledger);
    let current = running(&ledger, &old);
    // when
    let advanced = ledger
        .advance_peer_generation(&admin(), "host-a", 2)
        .unwrap();
    let new = Caller::authenticated_peer("host-a", advanced.generation).unwrap();
    // then
    assert!(matches!(
        ledger.validate_dispatch(&old, &current.guard()),
        Err(Error::PeerFenced)
    ));
    let fenced = ledger.agent(&current.agent_id).unwrap();
    assert_eq!(fenced.controller, None);
    assert!(matches!(
        ledger.validate_dispatch(&new, &fenced.guard()),
        Err(Error::ControllerFenced)
    ));
    let acquired = ledger
        .acquire_control(&new, &key(&ledger, &new, "acquire"), &fenced.guard(), 100)
        .unwrap();
    assert!(
        ledger
            .validate_dispatch(&new, &acquired.value.guard())
            .is_ok()
    );
    let revoked = ledger.revoke_peer(&admin(), "host-a").unwrap();
    let regranted = ledger
        .regrant_peer(&admin(), "host-a", revoked.generation)
        .unwrap();
    assert!(regranted.generation > revoked.generation);
    assert_eq!(ledger.agent(&current.agent_id).unwrap().controller, None);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_restart_should_advance_peer_generation_close_windows_and_retain_receipts() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    let caller = peer(&ledger);
    let operation = key(&ledger, &caller, "original");
    let request = intent();
    let first = ledger
        .admit_launch(&caller, &operation, &request, None, 100)
        .unwrap();
    drop(ledger);
    // when
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    // then
    assert_eq!(ledger.peer("host-a").unwrap().generation, 3);
    assert!(matches!(
        ledger.validate_principal(&caller),
        Err(Error::PeerFenced)
    ));
    let current_caller = Caller::authenticated_peer("host-a", 3).unwrap();
    let duplicate = ledger
        .admit_launch(&current_caller, &operation, &request, None, 100)
        .unwrap();
    assert_eq!(duplicate.value, first.value);
    assert!(!duplicate.fresh);
    let new_operation = OperationKey {
        id: OperationId::new("old-window-replay").unwrap(),
        ..operation
    };
    assert!(matches!(
        ledger.admit_launch(&current_caller, &new_operation, &request, None, 100),
        Err(Error::WindowClosed)
    ));
    let current = ledger.agent(&first.value.agent_id).unwrap();
    assert_eq!(current.controller, None);
    assert!(current.epoch.0 > first.value.guard.epoch.0);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_wrong_principal_or_stale_generation_should_not_use_another_window() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let owner = peer(&ledger);
    let operation = key(&ledger, &owner, "private");
    let other = Caller::authenticated_local("other-client").unwrap();
    // when / then
    assert!(matches!(
        ledger.admit_launch(&other, &operation, &intent(), None, 100),
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        ledger.close_window(&other, operation.window),
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        ledger.advance_peer_generation(&admin(), "host-a", 99),
        Err(Error::PeerFenced)
    ));
    assert_eq!(ledger.peer("host-a").unwrap().generation, 2);
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
}

#[test]
fn given_revocation_commit_failure_should_roll_back_entire_authority_fence() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = peer(&ledger);
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
    ledger.fail_next_commit();
    // when / then
    assert!(matches!(
        ledger.revoke_peer(&admin(), "host-a"),
        Err(Error::Storage(_))
    ));
    assert!(ledger.validate_dispatch(&caller, &agent.guard()).is_ok());
    assert!(ledger.writer(writer.id).unwrap().live);
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Accepted
    );
    ledger.revoke_peer(&admin(), "host-a").unwrap();
    assert!(matches!(
        ledger.validate_dispatch(&caller, &agent.guard()),
        Err(Error::PeerFenced)
    ));
    assert_eq!(
        ledger.input_receipt(writer.id, 1).unwrap().unwrap().outcome,
        InputOutcome::Unknown
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
}

#[test]
fn given_peer_without_policy_anchor_should_deny_even_current_generation() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let registered = ledger.register_peer(&admin(), "unanchored").unwrap();
    let caller = Caller::authenticated_peer("unanchored", registered.generation).unwrap();
    // when / then
    assert_eq!(registered.authorization_revision, 0);
    assert_eq!(registered.authorization_digest, None);
    assert!(matches!(
        ledger.validate_principal(&caller),
        Err(Error::PeerFenced)
    ));
    assert!(matches!(
        ledger.issue_window(&caller, 100, 100),
        Err(Error::PeerFenced)
    ));
    let anchored = ledger
        .update_peer_authorization(
            &admin(),
            "unanchored",
            registered.generation,
            PayloadDigest([4; 32]),
        )
        .unwrap();
    assert_eq!(anchored.authorization_revision, 1);
    assert_eq!(anchored.authorization_digest, Some(PayloadDigest([4; 32])));
    assert!(anchored.generation > registered.generation);
    assert!(
        ledger
            .validate_principal(
                &Caller::authenticated_peer("unanchored", anchored.generation).unwrap()
            )
            .is_ok()
    );
}

#[test]
fn given_authorization_update_should_atomically_fence_and_preserve_anchor_across_restart() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    let caller = peer(&ledger);
    let agent = running(&ledger, &caller);
    let window = ledger.issue_window(&caller, 100, 100).unwrap();
    let writer = ledger.open_writer(&caller, &agent.guard(), None).unwrap();
    let original = ledger.peer("host-a").unwrap();
    ledger.fail_next_commit();
    // when / then: failed policy commit cannot partially authorize/fence
    assert!(matches!(
        ledger.update_peer_authorization(
            &admin(),
            "host-a",
            original.generation,
            PayloadDigest([5; 32])
        ),
        Err(Error::Storage(_))
    ));
    assert_eq!(ledger.peer("host-a").unwrap(), original);
    assert!(ledger.validate_dispatch(&caller, &agent.guard()).is_ok());
    assert!(ledger.writer(writer.id).unwrap().live);
    let updated = ledger
        .update_peer_authorization(
            &admin(),
            "host-a",
            original.generation,
            PayloadDigest([5; 32]),
        )
        .unwrap();
    assert_eq!(
        updated.authorization_revision,
        original.authorization_revision + 1
    );
    assert!(matches!(
        ledger.validate_principal(&caller),
        Err(Error::PeerFenced)
    ));
    assert_eq!(ledger.agent(&agent.agent_id).unwrap().controller, None);
    assert!(!ledger.writer(writer.id).unwrap().live);
    let current = Caller::authenticated_peer("host-a", updated.generation).unwrap();
    let operation = OperationKey {
        window: window.id,
        id: OperationId::new("old-window").unwrap(),
        digest: PayloadDigest([1; 32]),
    };
    assert!(matches!(
        ledger.admit_launch(&current, &operation, &intent(), None, 100),
        Err(Error::WindowClosed)
    ));
    drop(ledger);
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    let reopened = ledger.peer("host-a").unwrap();
    assert_eq!(reopened.generation, updated.generation + 1);
    assert_eq!(reopened.authorization_digest, updated.authorization_digest);
    assert_eq!(
        reopened.authorization_revision,
        updated.authorization_revision
    );
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 1);
    let revoked = ledger.revoke_peer(&admin(), "host-a").unwrap();
    let reanchored = ledger
        .update_peer_authorization(
            &admin(),
            "host-a",
            revoked.generation,
            PayloadDigest([6; 32]),
        )
        .unwrap();
    assert!(reanchored.revoked);
    assert!(matches!(
        ledger.validate_principal(
            &Caller::authenticated_peer("host-a", reanchored.generation).unwrap()
        ),
        Err(Error::PeerFenced)
    ));
    let regranted = ledger
        .regrant_peer(&admin(), "host-a", reanchored.generation)
        .unwrap();
    assert_eq!(
        regranted.authorization_digest,
        reanchored.authorization_digest
    );
    assert!(
        ledger
            .validate_principal(
                &Caller::authenticated_peer("host-a", regranted.generation).unwrap()
            )
            .is_ok()
    );
}
