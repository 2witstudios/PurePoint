use owo_colors::OwoColorize;
use pu_core::protocol::AgentStatusReport;
use pu_core::types::{AgentStatus, WorktreeEntry};

use super::formatters::{status_colored, status_colored_with_suspended, trigger_progress};

pub(crate) fn print_spawn_result(
    worktree_id: &Option<String>,
    agent_id: &str,
    status: AgentStatus,
) {
    println!(
        "Spawned agent {} ({})",
        agent_id.bold(),
        status_colored(status, None)
    );
    if let Some(wt) = worktree_id {
        println!("  Worktree: {wt}");
    }
}

pub(crate) fn print_status_report(worktrees: &[WorktreeEntry], agents: &[AgentStatusReport]) {
    if worktrees.is_empty() && agents.is_empty() {
        println!("No active agents");
        return;
    }
    if !agents.is_empty() {
        println!(
            "{:<14} {:<16} {}",
            "ID".bold(),
            "NAME".bold(),
            "STATUS".bold()
        );
        for a in agents {
            println!(
                "{:<14} {:<16} {}{}",
                a.id.dimmed(),
                a.name,
                status_colored_with_suspended(a.status, a.exit_code, a.suspended),
                trigger_progress(a)
            );
        }
    }
    for wt in worktrees {
        println!(
            "\n{} {} ({}) — {:?}",
            "Worktree".bold(),
            wt.id.dimmed(),
            wt.branch,
            wt.status,
        );
        if !wt.agents.is_empty() {
            println!(
                "  {:<14} {:<16} {}",
                "ID".bold(),
                "NAME".bold(),
                "STATUS".bold()
            );
            for a in wt.agents.values() {
                println!(
                    "  {:<14} {:<16} {}",
                    a.id.dimmed(),
                    a.name,
                    status_colored_with_suspended(a.status, a.exit_code, a.suspended),
                );
            }
        }
    }
}

pub(crate) fn print_agent_status(a: &AgentStatusReport) {
    println!(
        "{} {} {}{}",
        a.id.dimmed(),
        a.name.bold(),
        status_colored_with_suspended(a.status, a.exit_code, a.suspended),
        trigger_progress(a)
    );
    if let Some(pid) = a.pid {
        println!("  PID:    {pid}");
    }
    if let Some(code) = a.exit_code {
        println!("  Exit:   {code}");
    }
    if let Some(idle) = a.idle_seconds {
        println!("  Idle:   {idle}s");
    }
    if let Some(ref wt) = a.worktree_id {
        println!("  Worktree: {wt}");
    }
    if let (Some(state), Some(idx), Some(total)) =
        (a.trigger_state, a.trigger_seq_index, a.trigger_total)
    {
        println!("  Trigger: {idx}/{total} ({state:?})");
    }
}

pub(crate) fn print_kill_result(killed: &[String]) {
    println!("Killed {} agent(s)", killed.len());
}

pub(crate) fn print_suspend_result(suspended: &[String]) {
    if suspended.is_empty() {
        println!("No agents to bench");
    } else {
        println!("Benched {} agent(s)", suspended.len());
        for id in suspended {
            println!("  {}", id.dimmed());
        }
    }
}

pub(crate) fn print_resume_result(agent_id: &str, status: AgentStatus) {
    println!(
        "Back in play: {} ({})",
        agent_id.bold(),
        status_colored(status, None)
    );
}

pub(crate) fn print_rename_result(agent_id: &str, name: &str) {
    println!("Renamed agent {} to {}", agent_id.bold(), name.green());
}

pub(crate) fn print_assign_trigger_result(agent_id: &str, trigger_name: &str, sequence_len: u32) {
    println!(
        "Assigned trigger {} to agent {} ({} steps)",
        trigger_name.green(),
        agent_id.bold(),
        sequence_len
    );
}

pub(crate) fn print_logs_result(agent_id: &str, data: &str) {
    println!("{}", format!("--- Logs for {agent_id} ---").dimmed());
    print!("{data}");
}

pub(crate) fn print_create_worktree_result(worktree_id: &str) {
    println!("Created worktree {}", worktree_id.bold());
}

pub(crate) fn print_delete_worktree_result(
    worktree_id: &str,
    killed_agents: &[String],
    branch_deleted: bool,
    remote_deleted: bool,
) {
    println!("Deleted worktree {}", worktree_id.bold());
    if !killed_agents.is_empty() {
        println!("  Killed {} agent(s)", killed_agents.len());
    }
    println!(
        "  Branch deleted: {}",
        if branch_deleted {
            "yes".green().to_string()
        } else {
            "no".dimmed().to_string()
        }
    );
    println!(
        "  Remote deleted: {}",
        if remote_deleted {
            "yes".green().to_string()
        } else {
            "no".dimmed().to_string()
        }
    );
}
