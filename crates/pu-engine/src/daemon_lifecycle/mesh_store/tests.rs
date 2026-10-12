use super::*;
use crate::daemon_lifecycle::try_acquire_daemon_lock;
use crate::execution_state::{Caller, Limits};
use pu_core::mesh::{Capability, ExportGrant};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

fn setup() -> (tempfile::TempDir, Arc<DaemonLock>, LocalAdmin) {
    let tmp = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let lock = Arc::new(
        try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
            .unwrap()
            .unwrap(),
    );
    let admin =
        LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
            .unwrap();
    (tmp, lock, admin)
}

fn grants() -> Vec<ExportGrant> {
    vec![ExportGrant {
        export_id: "project-export".into(),
        capabilities: BTreeSet::from([Capability::Inspect]),
        runtimes: BTreeSet::new(),
    }]
}

#[test]
fn given_explicit_initialization_should_restore_exact_identity_without_automatic_replacement() {
    let (tmp, lock, admin) = setup();
    assert!(MeshStore::open(lock.clone()).is_err());
    let store = MeshStore::initialize(lock.clone(), &admin).unwrap();
    let id = store.daemon_id().to_owned();
    let certificate = store.identity().certificate_der().to_vec();
    assert!(MeshStore::initialize(lock.clone(), &admin).is_err());
    drop(store);
    let reopened = MeshStore::open(lock.clone()).unwrap();
    assert_eq!(reopened.daemon_id(), id);
    assert_eq!(reopened.identity().certificate_der(), certificate);
    drop(reopened);
    let path = tmp.path().join("daemon.sock.mesh-identity.json");
    std::fs::write(&path, b"{}").unwrap();
    assert!(MeshStore::open(lock.clone()).is_err());
    assert!(MeshStore::initialize(lock, &admin).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"{}");
}

#[test]
fn given_concurrent_initialization_should_create_exactly_one_identity() {
    let (_tmp, lock, _) = setup();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let lock = lock.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let admin = LocalAdmin::authorized_by_local_policy(
                    &Caller::authenticated_local("admin").unwrap(),
                )
                .unwrap();
                barrier.wait();
                MeshStore::initialize(lock, &admin).is_ok()
            })
        })
        .collect();
    let successes = threads
        .into_iter()
        .map(|thread| usize::from(thread.join().unwrap()))
        .sum::<usize>();
    assert_eq!(successes, 1);
    assert!(MeshStore::open(lock).is_ok());
}

#[test]
fn given_policy_and_anchor_disagree_should_deny_even_current_generation_until_explicit_local_update()
 {
    let (_tmp, lock, admin) = setup();
    let mut store = MeshStore::initialize(lock, &admin).unwrap();
    let ledger = Ledger::in_memory(store.daemon_id(), Limits::default()).unwrap();
    let host = Identity::generate().unwrap();
    let record = store
        .enroll(&ledger, &admin, host.certificate_der(), grants(), None)
        .unwrap();
    let policy = store
        .current_policy(&ledger, host.peer_id(), record.generation, 100)
        .unwrap();
    assert_eq!(policy.exports.len(), 1);
    let mut changed = store.read_policy(host.peer_id()).unwrap();
    changed.exports.clear();
    store.persist_policy(host.peer_id(), &changed).unwrap(); // interrupted update before anchoring
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), record.generation, 100)
            .is_err()
    );
    let updated = store
        .update_policy(
            &ledger,
            &admin,
            host.certificate_der(),
            record.generation,
            vec![],
            None,
        )
        .unwrap();
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), record.generation, 100)
            .is_err()
    );
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), updated.generation, 100)
            .unwrap()
            .exports
            .is_empty()
    );
    ledger
        .revoke_peer(&admin, &peer_name(host.peer_id()))
        .unwrap();
    let revoked = ledger.peer(&peer_name(host.peer_id())).unwrap();
    let changed = store
        .update_policy(
            &ledger,
            &admin,
            host.certificate_der(),
            revoked.generation,
            grants(),
            None,
        )
        .unwrap();
    assert!(changed.revoked);
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), changed.generation, 100)
            .is_err()
    );
}

#[test]
fn given_expired_policy_should_remain_expired_after_clock_rollback_and_restart() {
    let (tmp, lock, admin) = setup();
    let mut store = MeshStore::initialize(lock, &admin).unwrap();
    let path = tmp.path().join("ledger.sqlite");
    let ledger = Ledger::open(&path, store.daemon_id(), Limits::default()).unwrap();
    let host = Identity::generate().unwrap();
    let record = store
        .enroll(&ledger, &admin, host.certificate_der(), grants(), Some(200))
        .unwrap();
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), record.generation, 199)
            .is_ok()
    );
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), record.generation, 200)
            .is_err()
    );
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), record.generation, 100)
            .is_err()
    );
    drop(ledger);
    let reopened = Ledger::open(&path, store.daemon_id(), Limits::default()).unwrap();
    let current = reopened.peer(&peer_name(host.peer_id())).unwrap();
    assert!(
        store
            .current_policy(&reopened, host.peer_id(), current.generation, 100)
            .is_err()
    );
}

#[test]
fn given_corrupt_policy_should_preserve_identity_and_allow_only_explicit_repair_for_the_same_peer()
{
    let (tmp, lock, admin) = setup();
    let mut store = MeshStore::initialize(lock, &admin).unwrap();
    let ledger = Ledger::in_memory(store.daemon_id(), Limits::default()).unwrap();
    let host = Identity::generate().unwrap();
    let record = store
        .enroll(&ledger, &admin, host.certificate_der(), grants(), None)
        .unwrap();
    let path = tmp.path().join(store.policy_name(host.peer_id()));
    std::fs::write(&path, b"{").unwrap();
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), record.generation, 100)
            .is_err()
    );
    let other = Identity::generate().unwrap();
    assert!(
        store
            .update_policy(
                &ledger,
                &admin,
                other.certificate_der(),
                record.generation,
                grants(),
                None
            )
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{");
    let repaired = store
        .update_policy(
            &ledger,
            &admin,
            host.certificate_der(),
            record.generation,
            grants(),
            None,
        )
        .unwrap();
    assert!(
        store
            .current_policy(&ledger, host.peer_id(), repaired.generation, 100)
            .is_ok()
    );
    let foreign = Ledger::in_memory("different-daemon", Limits::default()).unwrap();
    assert!(
        store
            .current_policy(&foreign, host.peer_id(), repaired.generation, 100)
            .is_err()
    );
    assert!(
        store
            .enroll(&foreign, &admin, other.certificate_der(), grants(), None)
            .is_err()
    );
}

#[tokio::test]
async fn given_authenticated_tls_connection_should_recheck_grants_and_live_revocation_before_caller_creation()
 {
    use crate::mesh_transport::{EnrolledPeers, MeshTransport, Role};
    let (_tmp, lock, admin) = setup();
    let mut store = MeshStore::initialize(lock, &admin).unwrap();
    let ledger = Ledger::in_memory(store.daemon_id(), Limits::default()).unwrap();
    let host = Identity::generate().unwrap();
    let record = store
        .enroll(&ledger, &admin, host.certificate_der(), grants(), None)
        .unwrap();
    let server = MeshTransport::new(
        store.identity(),
        EnrolledPeers::new([host.certificate_der()]).unwrap(),
    )
    .unwrap();
    let client = MeshTransport::new(
        &host,
        EnrolledPeers::new([store.identity().certificate_der()]).unwrap(),
    )
    .unwrap();
    let (left, right) = tokio::io::duplex(16 * 1024);
    let (worker_connection, host_connection) = tokio::join!(
        server.accept(left, Role::Control),
        client.connect(right, Role::Control, store.identity().peer_id())
    );
    let connection = worker_connection.unwrap();
    let _host_connection = host_connection.unwrap();
    assert!(
        store
            .authorize_connection(
                &ledger,
                &connection,
                record.generation,
                "project-export",
                &Action::Inspect,
                100
            )
            .is_ok()
    );
    assert!(
        store
            .authorize_connection(
                &ledger,
                &connection,
                record.generation,
                "project-export",
                &Action::Input,
                100
            )
            .is_err()
    );
    assert!(
        store
            .authorize_connection(
                &ledger,
                &connection,
                record.generation,
                "other-export",
                &Action::Inspect,
                100
            )
            .is_err()
    );
    ledger.revoke_peer(&admin, &record.id).unwrap();
    assert!(
        store
            .authorize_connection(
                &ledger,
                &connection,
                record.generation,
                "project-export",
                &Action::Inspect,
                100
            )
            .is_err()
    );
}

#[test]
fn given_unsafe_or_oversized_identity_file_should_refuse_without_repair() {
    use std::os::unix::fs::{MetadataExt, symlink};
    for kind in ["symlink", "hardlink", "public", "large"] {
        let (tmp, lock, admin) = setup();
        let path = tmp.path().join("daemon.sock.mesh-identity.json");
        let target = tmp.path().join("unrelated");
        std::fs::write(&target, b"untouched").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        match kind {
            "symlink" => symlink(&target, &path).unwrap(),
            "hardlink" => std::fs::hard_link(&target, &path).unwrap(),
            _ => {
                std::fs::write(
                    &path,
                    if kind == "large" {
                        vec![b' '; MAX_IDENTITY_BYTES + 1]
                    } else {
                        b"{}".to_vec()
                    },
                )
                .unwrap();
                std::fs::set_permissions(
                    &path,
                    std::fs::Permissions::from_mode(if kind == "public" { 0o644 } else { 0o600 }),
                )
                .unwrap();
            }
        }
        let before = std::fs::symlink_metadata(&path).unwrap();
        assert!(MeshStore::open(lock.clone()).is_err(), "{kind}");
        assert!(MeshStore::initialize(lock, &admin).is_err(), "{kind}");
        let after = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (before.ino(), before.mode(), before.len()),
            (after.ino(), after.mode(), after.len())
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    }
}
