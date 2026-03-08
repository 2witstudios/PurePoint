use crate::client;
use crate::commands::cwd_string;
use crate::daemon_ctrl;
use crate::error::CliError;
use chrono::{DateTime, Utc};
use owo_colors::OwoColorize;
use pu_core::protocol::{AgentStatusReport, Request, Response, WorktreeDiffEntry};
use pu_core::types::AgentStatus;
use std::path::Path;

pub async fn run(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = cwd_string()?;

    // Fetch status and diff stats from the daemon
    let status_resp = client::send_request(
        socket,
        &Request::Status {
            project_root: project_root.clone(),
            agent_id: None,
        },
    )
    .await?;

    let diff_resp = client::send_request(
        socket,
        &Request::Diff {
            project_root,
            worktree_id: None,
            stat: true,
        },
    )
    .await?;

    // Extract data from responses
    let (worktrees, root_agents) = match status_resp {
        Response::StatusReport {
            worktrees, agents, ..
        } => (worktrees, agents),
        Response::Error { code, message } => return Err(CliError::DaemonError { code, message }),
        _ => return Err(CliError::Other("unexpected status response".into())),
    };

    let diffs = match diff_resp {
        Response::DiffResult { diffs } => diffs,
        Response::Error { .. } => vec![], // No diffs available is fine
        _ => vec![],
    };

    if json {
        print_json(&worktrees, &root_agents, &diffs);
    } else {
        print_recap(&worktrees, &root_agents, &diffs);
    }

    Ok(())
}

fn format_elapsed(started_at: DateTime<Utc>) -> String {
    let elapsed = Utc::now()
        .signed_duration_since(started_at)
        .num_seconds()
        .max(0);
    if elapsed < 60 {
        format!("{elapsed}s")
    } else if elapsed < 3600 {
        format!("{}m", elapsed / 60)
    } else if elapsed < 86400 {
        let h = elapsed / 3600;
        let m = (elapsed % 3600) / 60;
        if m > 0 {
            format!("{h}h {m}m")
        } else {
            format!("{h}h")
        }
    } else {
        let d = elapsed / 86400;
        let h = (elapsed % 86400) / 3600;
        if h > 0 {
            format!("{d}d {h}h")
        } else {
            format!("{d}d")
        }
    }
}

fn status_label(status: AgentStatus, exit_code: Option<i32>, suspended: bool) -> String {
    if suspended {
        return "suspended".yellow().to_string();
    }
    match status {
        AgentStatus::Streaming => "streaming".green().to_string(),
        AgentStatus::Waiting => "waiting".cyan().to_string(),
        AgentStatus::Broken => match exit_code {
            Some(0) => "done".dimmed().to_string(),
            Some(code) => format!("{}", format!("failed ({code})").red()),
            None => "broken".red().to_string(),
        },
    }
}

fn diff_summary(diff: &WorktreeDiffEntry) -> String {
    if let Some(ref err) = diff.error {
        return format!("  {}", format!("diff error: {err}").red());
    }
    if diff.files_changed == 0 {
        return format!("  {}", "no changes yet".dimmed());
    }
    let files = if diff.files_changed == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", diff.files_changed)
    };
    let mut parts = vec![files];
    if diff.insertions > 0 {
        parts.push(format!("{}", format!("+{}", diff.insertions).green()));
    }
    if diff.deletions > 0 {
        parts.push(format!("{}", format!("-{}", diff.deletions).red()));
    }
    format!("  {}", parts.join(" "))
}

fn print_recap(
    worktrees: &[pu_core::types::WorktreeEntry],
    root_agents: &[AgentStatusReport],
    diffs: &[WorktreeDiffEntry],
) {
    // Count all agents
    let wt_agent_count: usize = worktrees.iter().map(|wt| wt.agents.len()).sum();
    let total_agents = wt_agent_count + root_agents.len();

    if total_agents == 0 && worktrees.is_empty() {
        println!("{}", "No agents to recap".dimmed());
        return;
    }

    // Header
    println!("{}", "Workspace Recap".bold());
    println!("{}", "─".repeat(40).dimmed());

    // Summary line
    let mut summary_parts = Vec::new();
    if total_agents > 0 {
        let label = if total_agents == 1 { "agent" } else { "agents" };
        summary_parts.push(format!("{total_agents} {label}"));
    }
    if !worktrees.is_empty() {
        let label = if worktrees.len() == 1 {
            "worktree"
        } else {
            "worktrees"
        };
        summary_parts.push(format!("{} {label}", worktrees.len()));
    }
    println!("  {}", summary_parts.join(" across "));
    println!();

    // Totals accumulators
    let mut total_files: usize = 0;
    let mut total_insertions: usize = 0;
    let mut total_deletions: usize = 0;

    // Root agents (if any)
    if !root_agents.is_empty() {
        println!("  {} {}", "root".bold(), "(project root)".dimmed());
        for agent in root_agents {
            let elapsed = format_elapsed(agent.started_at);
            println!(
                "    {:<12} {:<10} {:<18} {}",
                agent.name.bold(),
                agent.agent_type.dimmed(),
                status_label(agent.status, agent.exit_code, agent.suspended),
                elapsed.dimmed(),
            );
        }
        println!();
    }

    // Per-worktree sections
    for wt in worktrees {
        let diff = diffs.iter().find(|d| d.worktree_id == wt.id);

        println!("  {} {}", wt.name.bold(), wt.branch.dimmed());

        // Agents in this worktree
        for agent in wt.agents.values() {
            let elapsed = format_elapsed(agent.started_at);
            println!(
                "    {:<12} {:<10} {:<18} {}",
                agent.name.bold(),
                agent.agent_type.dimmed(),
                status_label(agent.status, agent.exit_code, agent.suspended),
                elapsed.dimmed(),
            );
        }

        // Diff stats
        if let Some(d) = diff {
            println!("{}", diff_summary(d));
            total_files += d.files_changed;
            total_insertions += d.insertions;
            total_deletions += d.deletions;
        } else if wt.agents.is_empty() {
            println!("  {}", "no changes yet".dimmed());
        }

        println!();
    }

    // Totals footer
    if total_files > 0 {
        println!("{}", "─".repeat(40).dimmed());
        let files_label = if total_files == 1 { "file" } else { "files" };
        print!("  {} {files_label} changed", total_files.to_string().bold());
        if total_insertions > 0 {
            print!(", {}", format!("+{total_insertions}").green());
        }
        if total_deletions > 0 {
            print!(", {}", format!("-{total_deletions}").red());
        }
        println!();
    }
}

fn print_json(
    worktrees: &[pu_core::types::WorktreeEntry],
    root_agents: &[AgentStatusReport],
    diffs: &[WorktreeDiffEntry],
) {
    let wt_json: Vec<serde_json::Value> = worktrees
        .iter()
        .map(|wt| {
            let diff = diffs.iter().find(|d| d.worktree_id == wt.id);
            let agents_json: Vec<serde_json::Value> = wt
                .agents
                .values()
                .map(|a| {
                    serde_json::json!({
                        "id": a.id,
                        "name": a.name,
                        "agent_type": a.agent_type,
                        "status": format!("{:?}", a.status),
                        "started_at": a.started_at.to_rfc3339(),
                        "exit_code": a.exit_code,
                        "suspended": a.suspended,
                    })
                })
                .collect();
            serde_json::json!({
                "id": wt.id,
                "name": wt.name,
                "branch": wt.branch,
                "status": format!("{:?}", wt.status),
                "agents": agents_json,
                "diff": diff.map(|d| serde_json::json!({
                    "files_changed": d.files_changed,
                    "insertions": d.insertions,
                    "deletions": d.deletions,
                })),
            })
        })
        .collect();

    let root_json: Vec<serde_json::Value> = root_agents
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "name": a.name,
                "agent_type": a.agent_type,
                "status": format!("{:?}", a.status),
                "started_at": a.started_at.to_rfc3339(),
                "exit_code": a.exit_code,
                "suspended": a.suspended,
            })
        })
        .collect();

    let (total_files, total_ins, total_del) =
        diffs
            .iter()
            .fold((0usize, 0usize, 0usize), |(f, i, d), entry| {
                (
                    f + entry.files_changed,
                    i + entry.insertions,
                    d + entry.deletions,
                )
            });

    let recap = serde_json::json!({
        "worktrees": wt_json,
        "root_agents": root_json,
        "totals": {
            "files_changed": total_files,
            "insertions": total_ins,
            "deletions": total_del,
        },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&recap).expect("JSON serialization failed")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pu_core::protocol::AgentStatusReport;
    use pu_core::types::{AgentEntry, WorktreeEntry, WorktreeStatus};

    fn make_agent_report(
        id: &str,
        name: &str,
        status: AgentStatus,
        exit_code: Option<i32>,
    ) -> AgentStatusReport {
        AgentStatusReport {
            id: id.into(),
            name: name.into(),
            agent_type: "claude".into(),
            status,
            pid: Some(1234),
            exit_code,
            idle_seconds: None,
            worktree_id: None,
            started_at: Utc::now() - chrono::Duration::minutes(15),
            session_id: None,
            prompt: None,
            suspended: false,
        }
    }

    fn make_worktree(id: &str, name: &str, branch: &str) -> WorktreeEntry {
        let agent_entry = AgentEntry {
            id: format!("ag-{id}"),
            name: "worker".into(),
            agent_type: "claude".into(),
            status: AgentStatus::Streaming,
            prompt: None,
            started_at: Utc::now() - chrono::Duration::minutes(10),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(5678),
            session_id: None,
            suspended_at: None,
            suspended: false,
            command: None,
        };
        let mut agents = indexmap::IndexMap::new();
        agents.insert(agent_entry.id.clone(), agent_entry);

        WorktreeEntry {
            id: id.into(),
            name: name.into(),
            path: format!("/tmp/{id}"),
            branch: branch.into(),
            base_branch: Some("main".into()),
            status: WorktreeStatus::Active,
            agents,
            created_at: Utc::now(),
            merged_at: None,
        }
    }

    fn make_diff(wt_id: &str, name: &str) -> WorktreeDiffEntry {
        WorktreeDiffEntry {
            worktree_id: wt_id.into(),
            worktree_name: name.into(),
            branch: format!("pu/{name}"),
            base_branch: Some("main".into()),
            diff_output: String::new(),
            files_changed: 3,
            insertions: 47,
            deletions: 12,
            error: None,
        }
    }

    #[test]
    fn given_empty_workspace_should_print_no_agents() {
        print_recap(&[], &[], &[]);
    }

    #[test]
    fn given_worktrees_with_agents_should_not_panic() {
        let wt = make_worktree("wt-abc", "fix-auth", "pu/fix-auth");
        let diff = make_diff("wt-abc", "fix-auth");
        print_recap(&[wt], &[], &[diff]);
    }

    #[test]
    fn given_root_agents_should_not_panic() {
        let agent = make_agent_report("ag-root", "point-guard", AgentStatus::Waiting, None);
        print_recap(&[], &[agent], &[]);
    }

    #[test]
    fn given_mixed_agents_should_not_panic() {
        let wt = make_worktree("wt-abc", "fix-auth", "pu/fix-auth");
        let diff = make_diff("wt-abc", "fix-auth");
        let root = make_agent_report("ag-root", "guard", AgentStatus::Streaming, None);
        print_recap(&[wt], &[root], &[diff]);
    }

    #[test]
    fn given_done_agent_should_show_done_status() {
        let label = status_label(AgentStatus::Broken, Some(0), false);
        assert!(label.contains("done"));
    }

    #[test]
    fn given_failed_agent_should_show_failed_with_code() {
        let label = status_label(AgentStatus::Broken, Some(1), false);
        assert!(label.contains("failed"));
        assert!(label.contains("1"));
    }

    #[test]
    fn given_suspended_agent_should_show_suspended() {
        let label = status_label(AgentStatus::Waiting, None, true);
        assert!(label.contains("suspended"));
    }

    #[test]
    fn format_elapsed_seconds() {
        let elapsed = format_elapsed(Utc::now() - chrono::Duration::seconds(30));
        assert!(elapsed.ends_with('s'));
    }

    #[test]
    fn format_elapsed_minutes() {
        let elapsed = format_elapsed(Utc::now() - chrono::Duration::minutes(5));
        assert!(elapsed.ends_with('m'));
    }

    #[test]
    fn format_elapsed_hours() {
        let elapsed = format_elapsed(Utc::now() - chrono::Duration::hours(2));
        assert!(elapsed.contains('h'));
    }

    #[test]
    fn diff_summary_no_changes() {
        let diff = WorktreeDiffEntry {
            worktree_id: "wt-1".into(),
            worktree_name: "test".into(),
            branch: "pu/test".into(),
            base_branch: None,
            diff_output: String::new(),
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            error: None,
        };
        let summary = diff_summary(&diff);
        assert!(summary.contains("no changes"));
    }

    #[test]
    fn diff_summary_with_changes() {
        let diff = WorktreeDiffEntry {
            worktree_id: "wt-1".into(),
            worktree_name: "test".into(),
            branch: "pu/test".into(),
            base_branch: None,
            diff_output: String::new(),
            files_changed: 3,
            insertions: 47,
            deletions: 12,
            error: None,
        };
        let summary = diff_summary(&diff);
        assert!(summary.contains("3 files"));
    }

    #[test]
    fn diff_summary_with_error() {
        let diff = WorktreeDiffEntry {
            worktree_id: "wt-1".into(),
            worktree_name: "test".into(),
            branch: "pu/test".into(),
            base_branch: None,
            diff_output: String::new(),
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            error: Some("worktree not found".into()),
        };
        let summary = diff_summary(&diff);
        assert!(summary.contains("diff error"));
    }

    #[test]
    fn json_output_should_not_panic() {
        let wt = make_worktree("wt-abc", "fix-auth", "pu/fix-auth");
        let diff = make_diff("wt-abc", "fix-auth");
        let root = make_agent_report("ag-root", "guard", AgentStatus::Streaming, None);
        print_json(&[wt], &[root], &[diff]);
    }
}
