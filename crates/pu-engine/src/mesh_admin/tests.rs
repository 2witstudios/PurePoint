use super::*;
use crate::daemon_lifecycle::try_acquire_daemon_lock;
use crate::execution_state::Caller;
use pu_core::mesh::admin::{ListenConfig, PublicIdentity};
use std::os::unix::fs::PermissionsExt;

fn setup() -> (
    tempfile::TempDir,
    Arc<DaemonLock>,
    Arc<WorkOwner>,
    LocalAdmin,
    Arc<MeshAdminService>,
) {
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let lock = Arc::new(
        try_acquire_daemon_lock(&dir.path().join("daemon.sock"))
            .unwrap()
            .unwrap(),
    );
    let work = WorkOwner::new(&lock);
    let admin =
        LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("owner").unwrap())
            .unwrap();
    let service = MeshAdminService::new(
        lock.clone(),
        Limits::default(),
        SessionLimits::default(),
        16,
        BTreeSet::from(["codex".into()]),
    )
    .unwrap();
    (dir, lock, work, admin, service)
}
async fn initialize(
    service: &Arc<MeshAdminService>,
    work: &Arc<WorkOwner>,
    admin: &LocalAdmin,
) -> PublicIdentity {
    match service
        .handle(work, admin.clone(), MeshAdminRequest::IdentityInitialize {})
        .await
        .unwrap()
    {
        MeshAdminResponse::Identity { identity } => identity,
        _ => panic!("wrong initialization response"),
    }
}
#[tokio::test]
async fn given_uninitialized_service_should_require_explicit_init_and_persist_disabled_configuration()
 {
    // given
    let (dir, _lock, work, admin, service) = setup();
    assert!(
        service
            .handle(&work, admin.clone(), MeshAdminRequest::IdentityShow {})
            .await
            .is_err()
    );
    assert!(!dir.path().join("daemon.sock.mesh-identity.json").exists());
    // when
    let identity = initialize(&service, &work, &admin).await;
    // then
    assert!(!identity.certificate_der.is_empty());
    assert_eq!(identity.fingerprint.len(), 64);
    assert_eq!(
        service
            .handle(&work, admin.clone(), MeshAdminRequest::ConfigShow {})
            .await
            .unwrap(),
        MeshAdminResponse::Config {
            config: ListenConfig::default(),
            revision: 1
        }
    );
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::IdentityInitialize {}
            )
            .await
            .is_err()
    );
    assert_eq!(
        service
            .handle(&work, admin, MeshAdminRequest::IdentityShow {})
            .await
            .unwrap(),
        MeshAdminResponse::Identity { identity }
    );
}

fn reopen(lock: &Arc<DaemonLock>) -> Arc<MeshAdminService> {
    MeshAdminService::new(
        lock.clone(),
        Limits::default(),
        SessionLimits::default(),
        16,
        BTreeSet::from(["codex".into()]),
    )
    .unwrap()
}
async fn register(
    service: &Arc<MeshAdminService>,
    work: &Arc<WorkOwner>,
    admin: &LocalAdmin,
    path: &std::path::Path,
) -> String {
    match service
        .handle(
            work,
            admin.clone(),
            MeshAdminRequest::ExportRegister {
                project_root: path.to_str().unwrap().into(),
                label: "Project".into(),
            },
        )
        .await
        .unwrap()
    {
        MeshAdminResponse::Export { export } => export.id,
        _ => panic!("wrong export response"),
    }
}
fn grants(id: &str) -> Vec<pu_core::mesh::ExportGrant> {
    vec![pu_core::mesh::ExportGrant {
        export_id: id.into(),
        capabilities: BTreeSet::from([pu_core::mesh::Capability::Inspect]),
        runtimes: BTreeSet::from(["codex".into()]),
    }]
}
async fn enroll(
    service: &Arc<MeshAdminService>,
    work: &Arc<WorkOwner>,
    admin: &LocalAdmin,
    peer: &crate::mesh_transport::Identity,
    id: &str,
) -> pu_core::mesh::admin::PeerStatus {
    match service
        .handle(
            work,
            admin.clone(),
            MeshAdminRequest::EnrollManual {
                certificate_der: peer.certificate_der().to_vec(),
                expected_fingerprint: peer_name(peer.peer_id()),
                grants: grants(id),
                expires_at: None,
            },
        )
        .await
        .unwrap()
    {
        MeshAdminResponse::Peer { peer } => peer,
        _ => panic!("wrong enrollment response"),
    }
}
async fn peers(
    service: &Arc<MeshAdminService>,
    work: &Arc<WorkOwner>,
    admin: &LocalAdmin,
) -> Vec<pu_core::mesh::admin::PeerStatus> {
    match service
        .handle(
            work,
            admin.clone(),
            MeshAdminRequest::PeerList {
                after: None,
                limit: 16,
            },
        )
        .await
        .unwrap()
    {
        MeshAdminResponse::Peers { peers, .. } => peers,
        _ => panic!("wrong peer list response"),
    }
}

#[tokio::test]
async fn given_reopened_mesh_should_preserve_identity_and_disabled_listener_without_reset() {
    // given
    let (dir, lock, work, admin, service) = setup();
    let identity = initialize(&service, &work, &admin).await;
    let ledger = std::fs::read(dir.path().join("daemon.sock.mesh-ledger.sqlite")).unwrap();
    drop(service);
    // when
    let reopened = reopen(&lock);
    // then
    assert_eq!(
        reopened
            .handle(&work, admin.clone(), MeshAdminRequest::IdentityShow {})
            .await
            .unwrap(),
        MeshAdminResponse::Identity { identity }
    );
    assert!(
        reopened
            .listener_snapshot(&work, admin.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reopened
            .handle(&work, admin, MeshAdminRequest::IdentityInitialize {})
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("daemon.sock.mesh-ledger.sqlite")).unwrap(),
        ledger
    );
}

#[tokio::test]
async fn given_wrong_oob_pin_or_unknown_runtime_export_should_not_enroll() {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let export = register(&service, &work, &admin, dir.path()).await;
    let peer = crate::mesh_transport::Identity::generate().unwrap();
    // when / then
    assert!(matches!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::EnrollManual {
                    certificate_der: peer.certificate_der().to_vec(),
                    expected_fingerprint: "0".repeat(64),
                    grants: grants(&export),
                    expires_at: None,
                }
            )
            .await,
        Err(Error::FingerprintMismatch)
    ));
    let mut unknown = grants(&export);
    unknown[0].runtimes = BTreeSet::from(["unknown-runtime".into()]);
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::EnrollManual {
                    certificate_der: peer.certificate_der().to_vec(),
                    expected_fingerprint: peer_name(peer.peer_id()),
                    grants: unknown,
                    expires_at: None
                }
            )
            .await
            .is_err()
    );
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::EnrollManual {
                    certificate_der: peer.certificate_der().to_vec(),
                    expected_fingerprint: peer_name(peer.peer_id()),
                    grants: grants("not-issued"),
                    expires_at: None
                }
            )
            .await
            .is_err()
    );
    assert!(peers(&service, &work, &admin).await.is_empty());
    let enrolled = enroll(&service, &work, &admin, &peer, &export).await;
    assert_eq!(enrolled.fingerprint, peer_name(peer.peer_id()));
    assert_eq!(enrolled.policy.exports, grants(&export));
    assert_eq!(peers(&service, &work, &admin).await, vec![enrolled]);
}

#[tokio::test]
async fn given_revoked_peer_should_require_explicit_current_generation_regrant_and_preserve_revocation_on_policy_update()
 {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let export = register(&service, &work, &admin, dir.path()).await;
    let identity = crate::mesh_transport::Identity::generate().unwrap();
    let original = enroll(&service, &work, &admin, &identity, &export).await;
    let change = |generation, change| MeshAdminRequest::PeerChange {
        fingerprint: original.fingerprint.clone(),
        expected_generation: generation,
        change,
    };
    // when / then
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                change(original.generation - 1, PeerChange::Revoke)
            )
            .await
            .is_err()
    );
    service
        .handle(
            &work,
            admin.clone(),
            change(original.generation, PeerChange::Revoke),
        )
        .await
        .unwrap();
    let revoked = peers(&service, &work, &admin).await.remove(0);
    assert!(revoked.revoked);
    assert_eq!(revoked.generation, original.generation + 1);
    let updated = service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::PolicyUpdate {
                certificate_der: identity.certificate_der().to_vec(),
                expected_fingerprint: original.fingerprint.clone(),
                expected_generation: revoked.generation,
                grants: grants(&export),
                expires_at: Some(1),
            },
        )
        .await
        .unwrap();
    let updated = match updated {
        MeshAdminResponse::Peer { peer } => peer,
        _ => panic!("wrong policy response"),
    };
    assert!(updated.revoked);
    assert_eq!(updated.policy.expires_at, Some(1));
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                change(revoked.generation, PeerChange::Regrant)
            )
            .await
            .is_err()
    );
    service
        .handle(
            &work,
            admin.clone(),
            change(updated.generation, PeerChange::Regrant),
        )
        .await
        .unwrap();
    let regranted = peers(&service, &work, &admin).await.remove(0);
    assert!(!regranted.revoked);
    assert_eq!(regranted.generation, updated.generation + 1);
    service
        .handle(
            &work,
            admin.clone(),
            change(regranted.generation, PeerChange::AdvanceGeneration),
        )
        .await
        .unwrap();
    assert_eq!(
        peers(&service, &work, &admin).await[0].generation,
        regranted.generation + 1
    );
}

#[tokio::test]
async fn given_damaged_policy_should_still_allow_exact_generation_revoke_but_never_implicit_regrant()
 {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let export = register(&service, &work, &admin, dir.path()).await;
    let identity = crate::mesh_transport::Identity::generate().unwrap();
    let original = enroll(&service, &work, &admin, &identity, &export).await;
    let path = dir.path().join(format!(
        "daemon.sock.mesh-peer-{}.json",
        original.fingerprint
    ));
    std::fs::write(&path, b"{}").unwrap();
    // when / then
    service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::PeerChange {
                fingerprint: original.fingerprint.clone(),
                expected_generation: original.generation,
                change: PeerChange::Revoke,
            },
        )
        .await
        .unwrap();
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::PeerList {
                    after: None,
                    limit: 16
                }
            )
            .await
            .is_err()
    );
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::PeerChange {
                    fingerprint: original.fingerprint.clone(),
                    expected_generation: original.generation + 1,
                    change: PeerChange::Regrant
                }
            )
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
    let repair = service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::PolicyUpdate {
                certificate_der: identity.certificate_der().to_vec(),
                expected_fingerprint: original.fingerprint.clone(),
                expected_generation: original.generation + 1,
                grants: grants(&export),
                expires_at: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(repair,MeshAdminResponse::Peer { peer } if peer.revoked));
}

#[tokio::test]
async fn given_explicit_paired_config_should_persist_intent_notify_and_build_checked_transport_without_binding()
 {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let export = register(&service, &work, &admin, dir.path()).await;
    let identity = crate::mesh_transport::Identity::generate().unwrap();
    enroll(&service, &work, &admin, &identity, &export).await;
    let mut revisions = service.subscribe_revision();
    let old = *revisions.borrow();
    let config = ListenConfig {
        bind_address: Some("127.0.0.1:18443".parse().unwrap()),
        bulk_bind_address: Some("127.0.0.1:18444".parse().unwrap()),
    };
    // when
    let response = service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::ConfigSet {
                config: config.clone(),
            },
        )
        .await
        .unwrap();
    revisions.changed().await.unwrap();
    // then
    assert_eq!(*revisions.borrow(), old + 1);
    assert_eq!(
        response,
        MeshAdminResponse::Config {
            config: config.clone(),
            revision: old + 1
        }
    );
    let snapshot = service
        .listener_snapshot(&work, admin.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.config, config);
    assert_eq!(snapshot.revision, old + 1);
    assert_eq!(
        snapshot.authority.daemon_id(),
        match service
            .handle(&work, admin.clone(), MeshAdminRequest::IdentityShow {})
            .await
            .unwrap()
        {
            MeshAdminResponse::Identity { identity } => identity.daemon_id,
            _ => panic!("identity"),
        }
    );
    service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::ConfigSet {
                config: ListenConfig::default(),
            },
        )
        .await
        .unwrap();
    assert!(
        service
            .listener_snapshot(&work, admin)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn given_missing_or_malformed_existing_state_should_fail_closed_and_preserve_recovery_data() {
    // given / when / then: each missing/corrupt entry has a fresh scratch lifecycle.
    for suffix in [
        ".mesh-identity.json",
        ".mesh-ledger.sqlite",
        ".mesh-listen.json",
    ] {
        for missing in [true, false] {
            let (dir, lock, work, admin, service) = setup();
            initialize(&service, &work, &admin).await;
            drop(service);
            let path = dir.path().join(format!("daemon.sock{suffix}"));
            if missing {
                std::fs::remove_file(&path).unwrap();
            } else {
                std::fs::write(&path, b"damaged-recovery-data").unwrap();
            }
            let reopened = reopen(&lock);
            assert!(
                reopened
                    .listener_snapshot(&work, admin.clone())
                    .await
                    .is_err()
            );
            assert!(
                reopened
                    .handle(&work, admin.clone(), MeshAdminRequest::IdentityShow {})
                    .await
                    .is_err()
            );
            assert!(
                reopened
                    .handle(&work, admin, MeshAdminRequest::IdentityInitialize {})
                    .await
                    .is_err()
            );
            if missing {
                assert!(!path.exists());
            } else {
                assert_eq!(std::fs::read(&path).unwrap(), b"damaged-recovery-data");
            }
        }
    }
}

#[tokio::test]
async fn given_symlinked_config_should_not_write_other_entries() {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let config = dir.path().join("daemon.sock.mesh-listen.json");
    let saved = std::fs::read(&config).unwrap();
    std::fs::remove_file(&config).unwrap();
    let target = dir.path().join("untouched");
    std::fs::write(&target, &saved).unwrap();
    std::os::unix::fs::symlink(&target, &config).unwrap();
    // when / then
    assert!(
        service
            .handle(
                &work,
                admin,
                MeshAdminRequest::ConfigSet {
                    config: ListenConfig::default()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&target).unwrap(), saved);
}

#[tokio::test]
async fn given_export_revocation_should_be_irreversible_and_reenrollment_get_new_identity() {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let first = register(&service, &work, &admin, dir.path()).await;
    // when
    service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::ExportRevoke {
                export_id: first.clone(),
            },
        )
        .await
        .unwrap();
    let second = register(&service, &work, &admin, dir.path()).await;
    // then
    assert_ne!(first, second);
    match service
        .handle(&work, admin, MeshAdminRequest::ExportList {})
        .await
        .unwrap()
    {
        MeshAdminResponse::Exports { exports } => {
            assert_eq!(exports.len(), 2);
            assert!(
                exports
                    .iter()
                    .any(|export| export.id == first && export.revoked)
            );
            assert!(
                exports
                    .iter()
                    .any(|export| export.id == second && !export.revoked)
            );
        }
        _ => panic!("wrong export list"),
    }
}

#[tokio::test]
async fn given_cancelled_config_waiter_should_complete_accepted_change_and_keep_work_owned() {
    // given: block exclusive authority with actual owned work, not a mock store.
    let (_dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let authority = service
        .current_authority(&work, admin.clone())
        .await
        .unwrap();
    let (entered_sender, entered) = tokio::sync::oneshot::channel();
    let (release_sender, release) = std::sync::mpsc::channel();
    let held = {
        let work = work.clone();
        let admin = admin.clone();
        tokio::spawn(async move {
            authority
                .administer_exports(&work, admin, move |_, _| {
                    let _ = entered_sender.send(());
                    release.recv().unwrap();
                    Ok(())
                })
                .await
        })
    };
    entered.await.unwrap();
    let old = *service.subscribe_revision().borrow();
    let waiter = {
        let service = service.clone();
        let work = work.clone();
        let admin = admin.clone();
        tokio::spawn(async move {
            service
                .handle(
                    &work,
                    admin,
                    MeshAdminRequest::ConfigSet {
                        config: ListenConfig::default(),
                    },
                )
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while service.state.try_lock().is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // when
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    release_sender.send(()).unwrap();
    held.await.unwrap().unwrap();
    work.wait_idle().await;
    // then
    assert_eq!(
        service
            .handle(&work, admin, MeshAdminRequest::ConfigShow {})
            .await
            .unwrap(),
        MeshAdminResponse::Config {
            config: ListenConfig::default(),
            revision: old + 1
        }
    );
}

#[tokio::test]
async fn given_cached_identity_corruption_should_reject_reload_setup_and_preserve_bytes() {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let path = dir.path().join("daemon.sock.mesh-identity.json");
    std::fs::write(&path, b"damaged-cached-identity").unwrap();
    // when / then
    assert!(
        service
            .current_authority(&work, admin.clone())
            .await
            .is_err()
    );
    assert!(
        service
            .listener_snapshot(&work, admin.clone())
            .await
            .is_err()
    );
    assert!(
        service
            .handle(&work, admin, MeshAdminRequest::IdentityShow {})
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"damaged-cached-identity");
}

#[tokio::test]
async fn given_local_policy_pages_should_follow_exact_ledger_cursor_including_revoked_peers() {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let export = register(&service, &work, &admin, dir.path()).await;
    for _ in 0..2 {
        let identity = crate::mesh_transport::Identity::generate().unwrap();
        enroll(&service, &work, &admin, &identity, &export).await;
    }
    let first = service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::PeerList {
                after: None,
                limit: 1,
            },
        )
        .await
        .unwrap();
    let (first, next) = match first {
        MeshAdminResponse::Peers {
            mut peers,
            next_after,
        } => (peers.remove(0), next_after),
        _ => panic!("wrong page"),
    };
    assert_eq!(next, Some(first.fingerprint.clone()));
    service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::PeerChange {
                fingerprint: first.fingerprint.clone(),
                expected_generation: first.generation,
                change: PeerChange::Revoke,
            },
        )
        .await
        .unwrap();
    // when / then
    let second = service
        .handle(
            &work,
            admin.clone(),
            MeshAdminRequest::PeerList {
                after: next,
                limit: 1,
            },
        )
        .await
        .unwrap();
    match second {
        MeshAdminResponse::Peers { peers, .. } => {
            assert_eq!(peers.len(), 1);
            assert!(peers[0].fingerprint > first.fingerprint);
        }
        _ => panic!("wrong second page"),
    }
    let all = peers(&service, &work, &admin).await;
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .any(|peer| peer.fingerprint == first.fingerprint && peer.revoked)
    );
}

#[tokio::test]
async fn given_replaced_private_directory_should_refuse_writes_and_preserve_both_directories() {
    // given
    let (dir, _lock, work, admin, service) = setup();
    initialize(&service, &work, &admin).await;
    let root = dir.path();
    let old = root.with_extension("retained-mesh-admin");
    let identity = std::fs::read(root.join("daemon.sock.mesh-identity.json")).unwrap();
    std::fs::rename(root, &old).unwrap();
    std::fs::create_dir(root).unwrap();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let unrelated = root.join("unrelated");
    std::fs::write(&unrelated, b"preserved").unwrap();
    // when / then
    assert!(
        service
            .handle(
                &work,
                admin.clone(),
                MeshAdminRequest::ConfigSet {
                    config: ListenConfig::default()
                }
            )
            .await
            .is_err()
    );
    assert!(service.listener_snapshot(&work, admin).await.is_err());
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"preserved");
    assert_eq!(
        std::fs::read(old.join("daemon.sock.mesh-identity.json")).unwrap(),
        identity
    );
    assert!(!root.join("daemon.sock.mesh-listen.json").exists());
    // Restore the scratch pathname for ordinary TempDir cleanup only.
    std::fs::remove_dir_all(root).unwrap();
    std::fs::rename(old, root).unwrap();
}
