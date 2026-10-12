use super::*;
use crate::daemon_lifecycle::try_acquire_daemon_lock;
use crate::execution_state::Caller;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};

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

#[test]
fn given_exported_project_should_keep_identity_after_reopen_and_never_reuse_revoked_export() {
    let (tmp, lock, admin) = setup();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let mut exports = MeshExports::open(lock.clone(), "worker", 8).unwrap();
    let first = exports.register(&admin, &project, "Project").unwrap();
    let resolved = exports.resolve(&first.id, 1).unwrap();
    assert_eq!(resolved.project_root(), project.canonicalize().unwrap());
    assert!(exports.resolve(&first.id, 2).is_err());
    exports.revoke(&admin, &first.id).unwrap();
    assert!(exports.resolve(&first.id, 1).is_err());
    assert!(exports.revalidate(&resolved).is_err());
    drop(exports);
    let mut reopened = MeshExports::open(lock, "worker", 8).unwrap();
    assert!(reopened.resolve(&first.id, 1).is_err());
    let second = reopened.register(&admin, &project, "Project").unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(reopened.list(&admin).unwrap().len(), 2);
    assert!(reopened.resolve(&second.id, 1).is_ok());
}

#[test]
fn given_replaced_project_or_root_symlink_should_not_retarget_an_existing_export() {
    let (tmp, lock, admin) = setup();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let mut exports = MeshExports::open(lock, "worker", 8).unwrap();
    let first = exports.register(&admin, &project, "Project").unwrap();
    let resolved = exports.resolve(&first.id, 1).unwrap();
    std::fs::rename(&project, tmp.path().join("old-project")).unwrap();
    std::fs::create_dir(&project).unwrap();
    assert!(exports.resolve(&first.id, 1).is_err());
    assert!(exports.revalidate(&resolved).is_err());
    std::fs::remove_dir(&project).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("old-project"), &project).unwrap();
    assert!(exports.resolve(&first.id, 1).is_err());
}

#[test]
fn given_malformed_or_unsafe_export_state_should_fail_closed_without_repair() {
    let (tmp, lock, admin) = setup();
    let mut exports = MeshExports::open(lock.clone(), "worker", 1).unwrap();
    let record = exports.register(&admin, tmp.path(), "Project").unwrap();
    assert!(exports.register(&admin, tmp.path(), "More").is_err());
    assert!(exports.resolve("../../escape", 1).is_err());
    assert!(MeshExports::open(lock.clone(), "other-worker", 1).is_err());
    let path = lock
        .directory
        .checked_path_for(&exports.record_name(&record.id).unwrap())
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(exports.resolve(&record.id, 1).is_err());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, b"{}").unwrap();
    assert!(MeshExports::open(lock, "worker", 1).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
}

#[test]
fn given_unsafe_files_and_torn_revocation_should_never_follow_repair_or_reenable() {
    let (tmp, lock, admin) = setup();
    let mut exports = MeshExports::open(lock.clone(), "worker", 8).unwrap();
    let record = exports.register(&admin, tmp.path(), "Project").unwrap();
    let name = exports.record_name(&record.id).unwrap();
    let path = lock.directory.checked_path_for(&name).unwrap();
    let original = std::fs::read(&path).unwrap();
    let held = tmp.path().join("held");
    std::fs::rename(&path, &held).unwrap();
    std::os::unix::fs::symlink(&held, &path).unwrap();
    assert!(exports.resolve(&record.id, 1).is_err());
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::hard_link(&held, &path).unwrap();
    assert!(exports.resolve(&record.id, 1).is_err());
    assert_eq!(std::fs::read(&held).unwrap(), original);
    std::fs::remove_file(&path).unwrap();
    nix::unistd::mkfifo(
        &path,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    assert!(exports.resolve(&record.id, 1).is_err());
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_fifo()
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&held, &path).unwrap();

    // An empty tombstone represents an interrupted revoke. It remains a fence,
    // and an unsafe replacement cannot be followed to infer "not revoked".
    let revoked = lock
        .directory
        .checked_path_for(&exports.revoked_name(&record.id).unwrap())
        .unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&revoked)
        .unwrap();
    file.sync_all().unwrap();
    assert!(exports.resolve(&record.id, 1).is_err());
    assert!(exports.list(&admin).unwrap()[0].revoked);
    std::fs::remove_file(&revoked).unwrap();
    std::os::unix::fs::symlink("absent-target", &revoked).unwrap();
    assert!(exports.resolve(&record.id, 1).is_err());
    assert!(exports.revoke(&admin, &record.id).is_err());
    assert!(
        std::fs::symlink_metadata(&revoked)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn given_separate_instances_should_enforce_capacity_atomically_and_retain_daemon_ownership() {
    let (tmp, lock, admin) = setup();
    let instances: Vec<_> = (0..8)
        .map(|_| MeshExports::open(lock.clone(), "worker", 1).unwrap())
        .collect();
    let barrier = Arc::new(std::sync::Barrier::new(instances.len()));
    let threads: Vec<_> = instances
        .into_iter()
        .map(|mut exports| {
            let barrier = barrier.clone();
            let project = tmp.path().to_owned();
            let admin = admin.clone();
            std::thread::spawn(move || {
                barrier.wait();
                exports.register(&admin, &project, "Project").is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| usize::from(thread.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let exports = MeshExports::open(lock.clone(), "worker", 1).unwrap();
    assert_eq!(exports.list(&admin).unwrap().len(), 1);
    drop(lock);
    assert!(
        try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
            .unwrap()
            .is_none()
    );
    drop(exports);
    assert!(
        try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn given_concurrent_resolution_from_many_sessions_should_wait_for_the_catalog_not_fail() {
    // given: several handles over one catalog, as concurrent sessions hold
    let (tmp, lock, admin) = setup();
    let mut exports = MeshExports::open(lock.clone(), "worker", 8).unwrap();
    let export = exports.register(&admin, tmp.path(), "project").unwrap();
    let handles: Vec<_> = (0..8)
        .map(|_| Arc::new(MeshExports::open(lock.clone(), "worker", 8).unwrap()))
        .collect();
    // when: all resolve repeatedly at the same time
    let threads: Vec<_> = handles
        .into_iter()
        .map(|exports| {
            let id = export.id.clone();
            std::thread::spawn(move || (0..200).all(|_| exports.resolve(&id, 1).is_ok()))
        })
        .collect();
    // then: contention never surfaces as an unavailable export
    for thread in threads {
        assert!(thread.join().unwrap());
    }
}
