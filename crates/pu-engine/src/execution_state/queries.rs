//! Bounded internal inventory queries. Authorization belongs to the caller's
//! retained authority boundary; these values never authorize later effects.

use super::{db::decode, types::bounded, *};
use rusqlite::params;

impl Ledger {
    /// Current runs only, ordered by immutable agent ID. At most 65 rows allow
    /// the bridge to return a 64-row page with an explicit continuation cursor.
    /// Receiver-selected canonical project and export must both match.
    pub fn agents_for_export(
        &self,
        project: &str,
        export: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(AgentRecord, RunRecord)>> {
        bounded(project, 4096, "project")?;
        bounded(export, 128, "export")?;
        if let Some(after) = after {
            bounded(after, 128, "agent cursor")?;
        }
        if !(1..=65).contains(&limit) {
            return Err(Error::Invalid("agent page limit"));
        }
        self.transaction(|tx| {
            let mut statement=tx.prepare(
                "SELECT a.id,a.data,r.id,r.data FROM agents a JOIN runs r ON r.id=json_extract(a.data,'$.current_run') \
                 WHERE a.id>?1 AND json_extract(r.data,'$.intent.project')=?2 \
                 AND json_extract(r.data,'$.intent.export')=?3 ORDER BY a.id LIMIT ?4"
            )?;
            let rows=statement.query_map(params![after.unwrap_or(""),project,export,limit], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?)))?;
            rows.map(|row| {
                let (agent_key,agent,run_key,run)=row?;
                let agent:AgentRecord=decode(agent)?;
                let run:RunRecord=decode(run)?;
                if agent.agent_id.0!=agent_key || run.run_id.0!=run_key || agent.owner_daemon!=self.owner || agent.current_run!=run.run_id || agent.agent_id!=run.agent_id || run.intent.project!=project || run.intent.export.as_deref()!=Some(export) {
                    return Err(Error::Corrupt("inventory ownership"));
                }
                Ok((agent,run))
            }).collect()
        })
    }

    /// Every worker-owned agent identity, bounded by the ledger's agent
    /// budget. Engine-local fencing only; never a bridge response.
    pub fn agent_ids(&self) -> Result<Vec<AgentId>> {
        self.transaction(|tx| {
            let mut statement = tx.prepare("SELECT id FROM agents ORDER BY id LIMIT ?1")?;
            let rows =
                statement.query_map(params![self.limits.agents], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(AgentId(row?))).collect()
        })
    }

    /// Local trusted administration/transport setup only. Includes revoked and
    /// unanchored records so callers cannot mistake catalog membership for access.
    pub fn peers_page(&self, after: Option<&str>, limit: usize) -> Result<Vec<PeerRecord>> {
        if let Some(after) = after {
            bounded(after, 128, "peer cursor")?;
        }
        if !(1..=256).contains(&limit) {
            return Err(Error::Invalid("peer page limit"));
        }
        self.transaction(|tx| {
            let mut statement =
                tx.prepare("SELECT id,data FROM peers WHERE id>?1 ORDER BY id LIMIT ?2")?;
            let rows = statement.query_map(params![after.unwrap_or(""), limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.map(|row| {
                let (key, data) = row?;
                let peer: PeerRecord = decode(data)?;
                if peer.id != key {
                    return Err(Error::Corrupt("peer inventory identity"));
                }
                Ok(peer)
            })
            .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::execution_state::*;

    #[test]
    fn given_current_runs_should_page_only_the_exact_project_and_export() {
        let ledger = Ledger::in_memory("worker", Limits::default()).unwrap();
        let caller = Caller::authenticated_local("admin").unwrap();
        let window = ledger.issue_window(&caller, 10, 100).unwrap();
        let mut selected = Vec::new();
        for (i, project, export) in [
            (1, "/one", "one"),
            (2, "/one", "one"),
            (3, "/two", "one"),
            (4, "/one", "two"),
        ] {
            let intent = LaunchIntent {
                project: project.into(),
                export: Some(export.into()),
                runtime: "codex".into(),
                source_commit: "a".repeat(40),
                worktree: WorktreeIntent {
                    id: format!("wt-{i}"),
                    path: format!("{project}/{i}"),
                    branch: format!("pu/{i}"),
                },
            };
            let key = OperationKey {
                window: window.id,
                id: OperationId(format!("op-{i}")),
                digest: PayloadDigest([i; 32]),
            };
            let receipt = ledger
                .admit_launch(&caller, &key, &intent, None, 11)
                .unwrap()
                .value;
            if i < 3 {
                selected.push(receipt.agent_id.0);
            }
        }
        selected.sort();
        let page = ledger.agents_for_export("/one", "one", None, 1).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].0.agent_id.0, selected[0]);
        let next = ledger
            .agents_for_export("/one", "one", Some(&selected[0]), 1)
            .unwrap();
        assert_eq!(next[0].0.agent_id.0, selected[1]);
        assert!(
            ledger
                .agents_for_export("/one", "one", Some(&selected[1]), 1)
                .unwrap()
                .is_empty()
        );
        assert!(ledger.agents_for_export("/one", "one", None, 0).is_err());
        assert!(ledger.agents_for_export("/one", "one", None, 66).is_err());
        // Consistent decoded ownership cannot replace the durable table key.
        ledger.transaction(|tx| {
            tx.execute("UPDATE agents SET data=json_set(data,'$.agent_id','impostor') WHERE id=?1", [&selected[0]])?;
            tx.execute("UPDATE runs SET data=json_set(data,'$.agent_id','impostor') WHERE json_extract(data,'$.agent_id')=?1", [&selected[0]])?;
            Ok(())
        }).unwrap();
        assert!(matches!(
            ledger.agents_for_export("/one", "one", None, 1),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn given_peer_catalog_should_bound_pages_and_retain_revoked_unanchored_records() {
        let ledger = Ledger::in_memory("worker", Limits::default()).unwrap();
        let admin =
            LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
                .unwrap();
        for peer in ["c", "a", "b"] {
            ledger.register_peer(&admin, peer).unwrap();
        }
        ledger.revoke_peer(&admin, "b").unwrap();
        let first = ledger.peers_page(None, 2).unwrap();
        assert_eq!(
            first.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!(first[1].revoked);
        assert!(first[0].authorization_digest.is_none());
        assert_eq!(ledger.peers_page(Some("b"), 2).unwrap()[0].id, "c");
        assert!(ledger.peers_page(None, 257).is_err());
    }

    #[test]
    fn given_peer_record_claiming_another_key_should_reject_page_without_an_alias_cursor() {
        let ledger = Ledger::in_memory("worker", Limits::default()).unwrap();
        let admin =
            LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
                .unwrap();
        for id in ["a", "b"] {
            ledger.register_peer(&admin, id).unwrap();
        }
        ledger
            .transaction(|tx| {
                tx.execute(
                    "UPDATE peers SET data=(SELECT data FROM peers WHERE id='b') WHERE id='a'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(matches!(ledger.peers_page(None, 1), Err(Error::Corrupt(_))));
    }
}
