use super::*;

#[test]
fn given_unknown_or_modified_schema_should_fail_closed_without_reinitialization() {
    // given
    for mutation in [
        "PRAGMA user_version=99",
        "CREATE TABLE alien(value TEXT)",
        "DROP TABLE peers",
        "ALTER TABLE peers ADD COLUMN extra TEXT",
    ] {
        let dir = private_dir();
        let path = dir.path().join("ledger.db");
        let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
        launch(&ledger, &local());
        drop(ledger);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(mutation).unwrap();
        drop(db);
        // when / then
        assert!(matches!(
            Ledger::open(&path, "daemon-a", Limits::default()),
            Err(Error::Schema)
        ));
    }
}

#[test]
fn given_second_writer_should_refuse_while_original_remains_usable() {
    // given
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
    // when / then
    assert!(matches!(
        Ledger::open(&path, "daemon-a", Limits::default()),
        Err(Error::Storage(_))
    ));
    assert_eq!(launch(&ledger, &local()).agent_id.0.len(), 11);
}

#[test]
fn given_corrupted_authority_records_should_fail_closed_on_open() {
    // given
    for data in ["{}", r#"{"id":"host-a","generation":0,"revoked":false}"#] {
        let dir = private_dir();
        let path = dir.path().join("ledger.db");
        let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
        ledger.register_peer(&admin(), "host-a").unwrap();
        drop(ledger);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("UPDATE peers SET data=?1", [data]).unwrap();
        drop(db);
        // when / then
        assert!(matches!(
            Ledger::open(&path, "daemon-a", Limits::default()),
            Err(Error::Corrupt(_))
        ));
    }
}

#[test]
fn given_unsafe_files_or_private_directory_should_refuse_without_chmod_or_following() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    // given
    let dir = private_dir();
    let target = dir.path().join("unrelated");
    std::fs::write(&target, b"original").unwrap();
    let link = dir.path().join("ledger.db");
    symlink(&target, &link).unwrap();
    // when / then
    assert!(matches!(
        Ledger::open(&link, "daemon-a", Limits::default()),
        Err(Error::UnsafePath(_))
    ));
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Ledger::open(dir.path().join("new.db"), "daemon-a", Limits::default()),
        Err(Error::UnsafePath(_))
    ));
    assert_eq!(
        std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn given_adversarial_fields_should_return_errors_without_writes_or_panics() {
    // given
    let ledger = Ledger::in_memory("daemon-a", Limits::default()).unwrap();
    let caller = local();
    let operation = key(&ledger, &caller, "bad-data");
    // when / then
    for bad in [
        "".to_string(),
        "x".repeat(4097),
        "project\0suffix".to_string(),
    ] {
        let mut request = intent();
        request.project = bad;
        assert!(matches!(
            ledger.admit_launch(&caller, &operation, &request, None, 100),
            Err(Error::Invalid(_))
        ));
    }
    let mut bad_commit = intent();
    bad_commit.source_commit = "main".into();
    assert!(matches!(
        ledger.admit_launch(&caller, &operation, &bad_commit, None, 100),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        ledger.issue_window(&caller, i64::MAX, 100),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        ledger.issue_window(&caller, -1, 100),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        ledger.issue_window(&caller, 100, i64::MAX),
        Err(Error::Invalid(_))
    ));
    assert_eq!(ledger.unresolved_runs().unwrap().len(), 0);
}

#[test]
fn given_corrupt_counter_or_run_proof_should_fail_closed_before_recovery() {
    // given
    for mutation in [
        "UPDATE meta SET next_window=1",
        "UPDATE runs SET data=json_set(data, '$.outcome', json('{\"PrelaunchFailure\":{\"evidence\":\"fake\"}}'))",
        "UPDATE agents SET data=json_set(data, '$.current_run', 'unrelated-run')",
        "UPDATE windows SET data=json_set(data, '$.id', 9999)",
    ] {
        let dir = private_dir();
        let path = dir.path().join("ledger.db");
        let ledger = Ledger::open(&path, "daemon-a", Limits::default()).unwrap();
        launch(&ledger, &local());
        drop(ledger);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(mutation).unwrap();
        drop(db);
        // when / then
        assert!(matches!(
            Ledger::open(&path, "daemon-a", Limits::default()),
            Err(Error::Corrupt(_))
        ));
    }
}

#[test]
fn given_empty_foreign_or_damaged_database_should_never_silently_replace_it() {
    use std::os::unix::fs::PermissionsExt;
    // given
    for bytes in [b"".as_slice(), b"not sqlite".as_slice()] {
        let dir = private_dir();
        let path = dir.path().join("ledger.db");
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        // when / then
        assert!(Ledger::open(&path, "daemon-a", Limits::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let dir = private_dir();
    let path = dir.path().join("ledger.db");
    drop(Ledger::open(&path, "daemon-a", Limits::default()).unwrap());
    assert!(matches!(
        Ledger::open(&path, "other-daemon", Limits::default()),
        Err(Error::Corrupt(_))
    ));
    assert!(Ledger::open(&path, "daemon-a", Limits::default()).is_ok());
}
