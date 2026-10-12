//! Bounded retention of terminal history. Budgets count only live records, so
//! retention exists to bound storage, not to free capacity; it never releases a
//! run slot, never removes Accepted/Unknown/Partial input evidence or a receipt
//! that could still be replayed, and never rewinds window/writer counters.
//!
//! Horizons, applied in this order:
//! 1. Operation receipts: kept while their window is open and unexpired, or while
//!    their run holds capacity. After that, replay of the identity already fails
//!    closed with `WindowClosed`, so the receipt is removed.
//! 2. Runs/agents: the newest `Limits::retained_terminal_runs` proven-terminal runs
//!    (by admission order) are kept. Older terminal runs are removed only when no
//!    receipt names them and none of their writers is live or retains uncertain
//!    input evidence. An agent is removed together with its last remaining run.
//! 3. Writers: fenced, non-current writers whose receipts are all Written or
//!    NotWritten are removed with those receipts.
//! 4. Windows: closed or expired windows without receipts are removed. Their IDs
//!    are never reissued, so a removed window always reads as closed.
//!
//! Each step removes at most `batch` rows per call; open runs it to completion.

use rusqlite::{Transaction, params};

use super::{db::*, *};

/// One bounded retention pass at trusted receiver time. Returns rows removed.
pub(super) fn retain(
    tx: &Transaction<'_>,
    now: i64,
    limits: &Limits,
    batch: usize,
) -> Result<usize> {
    let batch = i64::try_from(batch).unwrap_or(i64::MAX);
    let mut removed = tx.execute(
        &format!(
            "DELETE FROM operations WHERE rowid IN (SELECT o.rowid FROM operations o \
             JOIN windows w ON w.id=o.window JOIN runs r ON r.id=json_extract(o.data,'$.run') \
             WHERE (json_extract(w.data,'$.closed')=1 OR json_extract(w.data,'$.expires_at')<=?1) \
             AND json_extract(r.data,'$.lifecycle') IN {TERMINAL} ORDER BY o.rowid LIMIT ?2)"
        ),
        params![now, batch],
    )?;
    removed += retain_runs(tx, limits, batch)?;
    let writers: Vec<i64> = {
        let mut statement = tx.prepare(&format!(
            "SELECT w.id FROM writers w JOIN agents a ON a.id=w.agent \
             WHERE json_extract(w.data,'$.live')=0 \
             AND json_extract(a.data,'$.current_writer') IS NOT w.id \
             AND NOT EXISTS (SELECT 1 FROM inputs i WHERE i.writer=w.id \
                 AND json_extract(i.data,'$.outcome') NOT IN {CERTAIN}) \
             ORDER BY w.id LIMIT ?1"
        ))?;
        statement
            .query_map([batch], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    for writer in writers {
        tx.execute("DELETE FROM inputs WHERE writer=?1", [writer])?;
        removed += tx.execute("DELETE FROM writers WHERE id=?1", [writer])?;
    }
    removed += tx.execute(
        "DELETE FROM windows WHERE id IN (SELECT w.id FROM windows w \
         WHERE (json_extract(w.data,'$.closed')=1 OR json_extract(w.data,'$.expires_at')<=?1) \
         AND NOT EXISTS (SELECT 1 FROM operations o WHERE o.window=w.id) ORDER BY w.id LIMIT ?2)",
        params![now, batch],
    )?;
    Ok(removed)
}

/// Run retention to completion, e.g. on open after reopen fencing.
pub(super) fn retain_all(tx: &Transaction<'_>, now: i64, limits: &Limits) -> Result<()> {
    while retain(tx, now, limits, 100_000)? > 0 {}
    Ok(())
}

fn retain_runs(tx: &Transaction<'_>, limits: &Limits, batch: i64) -> Result<usize> {
    let horizon = i64::try_from(limits.retained_terminal_runs).unwrap_or(i64::MAX);
    let candidates: Vec<(String, String)> = {
        let mut statement = tx.prepare(&format!(
            "SELECT r.id, r.agent FROM runs r JOIN agents a ON a.id=r.agent \
             WHERE json_extract(r.data,'$.lifecycle') IN {TERMINAL} \
             AND (json_extract(a.data,'$.current_run') IS NOT r.id \
                 OR NOT EXISTS (SELECT 1 FROM runs s WHERE s.agent=r.agent AND s.id<>r.id)) \
             AND r.rowid NOT IN (SELECT rowid FROM runs \
                 WHERE json_extract(data,'$.lifecycle') IN {TERMINAL} ORDER BY rowid DESC LIMIT ?1) \
             AND r.id NOT IN (SELECT json_extract(o.data,'$.run') FROM operations o) \
             AND r.id NOT IN (SELECT w.run FROM writers w WHERE json_extract(w.data,'$.live')=1 \
                 OR EXISTS (SELECT 1 FROM inputs i WHERE i.writer=w.id \
                     AND json_extract(i.data,'$.outcome') NOT IN {CERTAIN})) \
             ORDER BY r.rowid LIMIT ?2"
        ))?;
        statement
            .query_map(params![horizon, batch], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    let mut removed = 0;
    for (run, agent_id) in candidates {
        let agent: AgentRecord =
            get(tx, "agents", &agent_id)?.ok_or(Error::Corrupt("run agent"))?;
        if agent.current_run.0 == run {
            // The agent goes with its current run, but only once it is the last run.
            let runs: usize = tx.query_row(
                "SELECT count(*) FROM runs WHERE agent=?1",
                [&agent_id],
                |r| r.get(0),
            )?;
            if runs != 1 {
                continue;
            }
            tx.execute(
                "DELETE FROM inputs WHERE writer IN (SELECT id FROM writers WHERE agent=?1)",
                [&agent_id],
            )?;
            tx.execute("DELETE FROM writers WHERE agent=?1", [&agent_id])?;
            removed += tx.execute("DELETE FROM runs WHERE id=?1", [&run])?;
            removed += tx.execute("DELETE FROM agents WHERE id=?1", [&agent_id])?;
        } else {
            // Superseded by resume; the current writer always belongs to the current run.
            tx.execute(
                "DELETE FROM inputs WHERE writer IN (SELECT id FROM writers WHERE run=?1)",
                [&run],
            )?;
            tx.execute("DELETE FROM writers WHERE run=?1", [&run])?;
            removed += tx.execute("DELETE FROM runs WHERE id=?1", [&run])?;
        }
    }
    Ok(removed)
}
