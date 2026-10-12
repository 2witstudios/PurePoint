use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::Mutex,
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};

use super::{types::bounded, *};

const APP_ID: i64 = 0x50554558;
const VERSION: i64 = 1;
pub(super) const MAX_RECORD_BYTES: usize = 32_768;
const TABLES: &[(&str, &str)] = &[
    (
        "meta",
        "CREATE TABLE meta (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), owner TEXT NOT NULL, next_window INTEGER NOT NULL CHECK(next_window > 0), next_writer INTEGER NOT NULL CHECK(next_writer > 0), last_time INTEGER NOT NULL CHECK(last_time >= 0)) STRICT",
    ),
    (
        "peers",
        "CREATE TABLE peers (id TEXT PRIMARY KEY, data TEXT NOT NULL) STRICT",
    ),
    (
        "windows",
        "CREATE TABLE windows (id INTEGER PRIMARY KEY, data TEXT NOT NULL) STRICT",
    ),
    (
        "agents",
        "CREATE TABLE agents (id TEXT PRIMARY KEY, data TEXT NOT NULL) STRICT",
    ),
    (
        "runs",
        "CREATE TABLE runs (id TEXT PRIMARY KEY, agent TEXT NOT NULL REFERENCES agents(id), data TEXT NOT NULL) STRICT",
    ),
    (
        "operations",
        "CREATE TABLE operations (window INTEGER NOT NULL REFERENCES windows(id), id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(window, id)) STRICT",
    ),
    (
        "writers",
        "CREATE TABLE writers (id INTEGER PRIMARY KEY, agent TEXT NOT NULL REFERENCES agents(id), run TEXT NOT NULL REFERENCES runs(id), data TEXT NOT NULL) STRICT",
    ),
    (
        "inputs",
        "CREATE TABLE inputs (writer INTEGER NOT NULL REFERENCES writers(id), sequence INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(writer, sequence)) STRICT",
    ),
];

pub(super) struct Inner {
    connection: Connection,
    /// Retain receiver observations in this instance even if the clock commit
    /// itself fails. No timed operation runs until this fence is durable.
    observed_time: i64,
    #[cfg(test)]
    fail_commit: bool,
    #[cfg(test)]
    fail_clock_commit: bool,
}

/// One synchronized durable writer. SQLite's exclusive locking mode also rejects
/// a second ledger owner while this instance is open. No connection escapes.
pub struct Ledger {
    pub(super) inner: Mutex<Inner>,
    pub(super) limits: Limits,
    pub(super) owner: String,
}

impl Ledger {
    /// Stable authority identity checked when composing receiver-owned stores.
    pub fn owner_daemon(&self) -> &str {
        &self.owner
    }

    /// Durably fence trusted receiver time for grant expiry as well as windows.
    /// Call from the blocking dispatcher under its authority barrier. A failed
    /// clock commit permits no authorization, and the observation is retained.
    pub fn observe_time(&self, now: i64) -> Result<i64> {
        self.transaction_at(now, |_, fenced_time| Ok(fenced_time))
    }

    /// Open under a private directory established by the daemon's local-boundary
    /// utility, while holding its singleton lock. This checks the immediate
    /// directory/database/sidecars, not every ancestor or a hostile same-UID race.
    /// It does not repair unsafe existing modes, follow symlinks, or replace files.
    pub fn open(path: impl AsRef<Path>, owner_daemon: &str, limits: Limits) -> Result<Self> {
        limits.validate()?;
        bounded(owner_daemon, 128, "owner daemon")?;
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(Error::UnsafePath("explicit parent required"))?;
        check_path(parent, true)?;
        // macOS's /var and /tmp are system symlinks. Resolve ancestors after
        // validating the immediate private boundary; never resolve the DB itself.
        let parent_path = fs::canonicalize(parent)?;
        let parent = parent_path.as_path();
        check_path(parent, true)?;
        let db_path = parent.join(
            path.file_name()
                .ok_or(Error::UnsafePath("database filename"))?,
        );
        let path = db_path.as_path();
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            let sidecar = Path::new(&sidecar);
            if fs::symlink_metadata(sidecar).is_ok() {
                check_path(sidecar, false)?;
            }
        }
        let created = match fs::symlink_metadata(path) {
            Ok(_) => {
                check_path(path, false)?;
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
                }
                options.open(path)?.sync_all()?;
                true
            }
            Err(error) => return Err(error.into()),
        };
        let connection = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let ledger = Self::initialize(connection, owner_daemon, limits, created)?;
        // Persist the directory entry as well as SQLite's FULL synchronous commit.
        fs::File::open(parent)?.sync_all()?;
        Ok(ledger)
    }

    pub fn in_memory(owner_daemon: &str, limits: Limits) -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?, owner_daemon, limits, true)
    }

    fn initialize(
        mut connection: Connection,
        owner: &str,
        limits: Limits,
        new: bool,
    ) -> Result<Self> {
        limits.validate()?;
        bounded(owner, 128, "owner daemon")?;
        connection.busy_timeout(Duration::from_millis(250))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL; PRAGMA locking_mode=EXCLUSIVE;")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let app: i64 = tx.query_row("PRAGMA application_id", [], |row| row.get(0))?;
        let objects: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if new && version == 0 && app == 0 && objects == 0 {
            for (_, sql) in TABLES {
                tx.execute_batch(sql)?;
            }
            tx.pragma_update(None, "application_id", APP_ID)?;
            tx.pragma_update(None, "user_version", VERSION)?;
            tx.execute("INSERT INTO meta VALUES (1, ?1, 1, 1, 0)", [owner])?;
        } else if version != VERSION || app != APP_ID {
            return Err(Error::Schema);
        }
        validate_schema(&tx)?;
        let stored_owner: String =
            tx.query_row("SELECT owner FROM meta WHERE singleton=1", [], |r| r.get(0))?;
        if stored_owner != owner {
            return Err(Error::Corrupt("owner daemon mismatch"));
        }
        super::validation::validate_records(&tx, owner)?;
        recover(&tx)?;
        let observed_time =
            tx.query_row("SELECT last_time FROM meta WHERE singleton=1", [], |row| {
                row.get(0)
            })?;
        tx.commit()?;
        Ok(Self {
            inner: Mutex::new(Inner {
                connection,
                observed_time,
                #[cfg(test)]
                fail_commit: false,
                #[cfg(test)]
                fail_clock_commit: false,
            }),
            limits,
            owner: owner.into(),
        })
    }

    pub(super) fn transaction<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut inner = self.inner.lock().map_err(|_| Error::Poisoned)?;
        run_transaction(&mut inner, f)
    }

    /// Persist trusted receiver time before attempting any operation validation or
    /// writes. Keep the same writer mutex across both transactions: no admission
    /// or authority change can interleave. Every failed operation rolls back all
    /// of its writes, while its already-committed clock observation survives even
    /// an operation COMMIT failure. Failure to persist time prevents the operation.
    pub(super) fn transaction_at<T>(
        &self,
        now: i64,
        f: impl FnOnce(&Transaction<'_>, i64) -> Result<T>,
    ) -> Result<T> {
        if now < 0 {
            return Err(Error::Invalid("receiver time"));
        }
        let mut inner = self.inner.lock().map_err(|_| Error::Poisoned)?;
        inner.observed_time = inner.observed_time.max(now);
        let observed_time = inner.observed_time;
        #[cfg(test)]
        let fail_clock = std::mem::take(&mut inner.fail_clock_commit);
        let tx = inner
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let fenced_time = advance_clock(&tx, observed_time)?;
        #[cfg(test)]
        if fail_clock {
            inject_commit_failure(&tx)?;
        }
        tx.commit()?;
        inner.observed_time = fenced_time;
        run_transaction(&mut inner, |tx| f(tx, fenced_time))
    }

    #[cfg(test)]
    pub(super) fn fail_next_commit(&self) {
        self.inner.lock().unwrap().fail_commit = true;
    }

    #[cfg(test)]
    pub(super) fn fail_next_clock_commit(&self) {
        self.inner.lock().unwrap().fail_clock_commit = true;
    }

    /// Internal engine read: apply current read/export authorization before exposing it.
    pub fn agent(&self, id: &AgentId) -> Result<AgentRecord> {
        bounded(&id.0, 128, "agent ID")?;
        self.transaction(|tx| get(tx, "agents", &id.0)?.ok_or(Error::NotFound("agent")))
    }

    /// Internal engine read: apply current read/export authorization before exposing it.
    pub fn run(&self, id: &RunId) -> Result<RunRecord> {
        bounded(&id.0, 128, "run ID")?;
        self.transaction(|tx| get(tx, "runs", &id.0)?.ok_or(Error::NotFound("run")))
    }

    /// Includes accepted/starting/running/stopping/unknown runs. None are evicted
    /// or released on restart; reconcile with exact-run supervision evidence.
    pub fn unresolved_runs(&self) -> Result<Vec<RunRecord>> {
        self.transaction(|tx| {
            Ok(all::<RunRecord>(tx, "runs")?
                .into_iter()
                .filter(|run| run.lifecycle.holds_capacity())
                .collect())
        })
    }
}

#[cfg(unix)]
fn check_path(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err(Error::UnsafePath("unexpected file type"));
    }
    // SAFETY: geteuid has no preconditions and does not mutate process state.
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || (!directory && meta.nlink() != 1)
    {
        return Err(Error::UnsafePath("owner, mode, or link count"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_path(_: &Path, _: bool) -> Result<()> {
    Err(Error::UnsafePath("private file validation requires Unix"))
}

fn validate_schema(tx: &Transaction<'_>) -> Result<()> {
    let check: String = tx.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(Error::Schema);
    }
    let mut statement = tx.prepare(
        "SELECT name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let actual: Vec<(String, String)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    if actual.len() != TABLES.len()
        || TABLES
            .iter()
            .any(|(name, sql)| !actual.iter().any(|(n, s)| n == name && s == sql))
    {
        return Err(Error::Schema);
    }
    if tx.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(Error::Schema);
    }
    Ok(())
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_string(value).map_err(|_| Error::Invalid("record serialization"))?;
    if value.len() > MAX_RECORD_BYTES {
        return Err(Error::Invalid("record too large"));
    }
    Ok(value)
}

pub(super) fn decode<T: DeserializeOwned>(value: String) -> Result<T> {
    if value.len() > MAX_RECORD_BYTES {
        return Err(Error::Corrupt("record too large"));
    }
    serde_json::from_str(&value).map_err(|_| Error::Corrupt("record encoding"))
}

// SQL identifiers here are module-private constants, never caller strings.
pub(super) fn get<T: DeserializeOwned>(
    tx: &Transaction<'_>,
    table: &str,
    id: impl rusqlite::ToSql,
) -> Result<Option<T>> {
    let data = tx
        .query_row(
            &format!("SELECT data FROM {table} WHERE id=?1"),
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    data.map(decode).transpose()
}

pub(super) fn all<T: DeserializeOwned>(tx: &Transaction<'_>, table: &str) -> Result<Vec<T>> {
    let count: usize = tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
    if count > 100_000 {
        return Err(Error::Corrupt("hard record budget"));
    }
    let mut statement = tx.prepare(&format!("SELECT data FROM {table} ORDER BY rowid"))?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|row| decode(row?)).collect()
}

pub(super) fn put<T: Serialize>(
    tx: &Transaction<'_>,
    table: &str,
    id: impl rusqlite::ToSql,
    value: &T,
) -> Result<()> {
    let changed = tx.execute(
        &format!("UPDATE {table} SET data=?2 WHERE id=?1"),
        params![id, encode(value)?],
    )?;
    if changed != 1 {
        return Err(Error::Corrupt("update target missing"));
    }
    Ok(())
}

pub(super) fn budget(tx: &Transaction<'_>, table: &'static str, limit: usize) -> Result<()> {
    let count: usize = tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
    if count >= limit {
        return Err(Error::Capacity {
            scope: CapacityScope::Records(table),
            limit,
        });
    }
    Ok(())
}

pub(super) fn next_id(tx: &Transaction<'_>, field: &str) -> Result<u64> {
    let id: u64 = tx.query_row(
        &format!("SELECT {field} FROM meta WHERE singleton=1"),
        [],
        |r| r.get(0),
    )?;
    let next = super::types::increment(id)?;
    tx.execute(
        &format!("UPDATE meta SET {field}=?1 WHERE singleton=1"),
        [next],
    )?;
    Ok(id)
}

fn advance_clock(tx: &Transaction<'_>, now: i64) -> Result<i64> {
    if now < 0 {
        return Err(Error::Invalid("receiver time"));
    }
    tx.execute(
        "UPDATE meta SET last_time=max(last_time, ?1) WHERE singleton=1",
        [now],
    )?;
    Ok(
        tx.query_row("SELECT last_time FROM meta WHERE singleton=1", [], |r| {
            r.get(0)
        })?,
    )
}

fn run_transaction<T>(
    inner: &mut Inner,
    f: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<T> {
    #[cfg(test)]
    let fail = std::mem::take(&mut inner.fail_commit);
    let tx = inner
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = f(&tx)?;
    #[cfg(test)]
    if fail {
        inject_commit_failure(&tx)?;
    }
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
fn inject_commit_failure(tx: &Transaction<'_>) -> Result<()> {
    // Real deferred FK failure at COMMIT, after the prospective result exists.
    tx.execute_batch("PRAGMA defer_foreign_keys=ON; INSERT INTO operations VALUES (-1, '__commit_fault__', '{}');")?;
    Ok(())
}

fn recover(tx: &Transaction<'_>) -> Result<()> {
    for mut peer in all::<PeerRecord>(tx, "peers")? {
        if !peer.revoked {
            peer.generation = super::types::increment(peer.generation)?;
            put(tx, "peers", &peer.id, &peer)?;
        }
    }
    for mut window in all::<OperationWindow>(tx, "windows")? {
        window.closed = true;
        put(tx, "windows", window.id.0, &window)?;
    }
    for mut agent in all::<AgentRecord>(tx, "agents")? {
        if matches!(agent.controller, Some(Principal::Peer { .. })) {
            agent.controller = None;
            agent.epoch.0 = super::types::increment(agent.epoch.0)?;
            agent.revision.0 = super::types::increment(agent.revision.0)?;
            put(tx, "agents", &agent.agent_id.0, &agent)?;
        }
    }
    for mut run in all::<RunRecord>(tx, "runs")? {
        if run.lifecycle.holds_capacity() {
            run.lifecycle = Lifecycle::Unknown;
            put(tx, "runs", &run.run_id.0, &run)?;
            let mut agent: AgentRecord =
                get(tx, "agents", &run.agent_id.0)?.ok_or(Error::Corrupt("run agent"))?;
            agent.lifecycle = Lifecycle::Unknown;
            agent.revision.0 = super::types::increment(agent.revision.0)?;
            put(tx, "agents", &agent.agent_id.0, &agent)?;
        }
    }
    for mut writer in all::<WriterRecord>(tx, "writers")? {
        writer.live = false;
        put(tx, "writers", writer.id.0, &writer)?;
    }
    for mut ack in all::<InputAck>(tx, "inputs")? {
        if ack.outcome == InputOutcome::Accepted {
            ack.outcome = InputOutcome::Unknown;
            tx.execute(
                "UPDATE inputs SET data=?3 WHERE writer=?1 AND sequence=?2",
                params![ack.writer.0, ack.sequence, encode(&ack)?],
            )?;
        }
    }
    Ok(())
}
