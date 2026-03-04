use owo_colors::OwoColorize;
use pu_core::protocol::Response;
use pu_core::types::AgentStatus;

use crate::error::CliError;

/// Check a daemon response for errors. On error, print JSON if requested, then return Err.
pub fn check_response(resp: Response, json: bool) -> Result<Response, CliError> {
    if let Response::Error { code, message } = resp {
        if json {
            print_response(&Response::Error { code: code.clone(), message: message.clone() }, true);
        }
        Err(CliError::DaemonError { code, message })
    } else {
        Ok(resp)
    }
}

fn status_colored(status: AgentStatus, exit_code: Option<i32>) -> String {
    match status {
        AgentStatus::Streaming => "streaming".green().to_string(),
        AgentStatus::Waiting => "waiting".cyan().to_string(),
        AgentStatus::Broken => match exit_code {
            Some(0) => "done".dimmed().to_string(),
            _ => "broken".red().to_string(),
        },
    }
}

pub fn print_response(response: &Response, json_mode: bool) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(response).unwrap());
        return;
    }
    match response {
        Response::HealthReport {
            pid,
            uptime_seconds,
            protocol_version,
            projects,
            agent_count,
        } => {
            println!("{}", "Daemon healthy".green().bold());
            println!("  PID:      {pid}");
            println!("  Uptime:   {uptime_seconds}s");
            println!("  Protocol: v{protocol_version}");
            println!("  Projects: {}", projects.len());
            println!("  Agents:   {agent_count}");
        }
        Response::InitResult { created } => {
            if *created {
                println!("{}", "Initialized PurePoint workspace".green());
            } else {
                println!("Already initialized");
            }
        }
        Response::SpawnResult {
            worktree_id,
            agent_id,
            status,
        } => {
            println!(
                "Spawned agent {} ({})",
                agent_id.bold(),
                status_colored(*status, None)
            );
            if let Some(wt) = worktree_id {
                println!("  Worktree: {wt}");
            }
        }
        Response::StatusReport { worktrees, agents } => {
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
                        "{:<14} {:<16} {}",
                        a.id.dimmed(),
                        a.name,
                        status_colored(a.status, a.exit_code)
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
                            status_colored(a.status, a.exit_code),
                        );
                    }
                }
            }
        }
        Response::AgentStatus(a) => {
            println!(
                "{} {} {}",
                a.id.dimmed(),
                a.name.bold(),
                status_colored(a.status, a.exit_code)
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
        }
        Response::KillResult { killed, .. } => {
            println!("Killed {} agent(s)", killed.len());
        }
        Response::SuspendResult { suspended } => {
            println!("Suspended {} agent(s)", suspended.len());
        }
        Response::ResumeResult { agent_id, status } => {
            println!(
                "Resumed agent {} ({})",
                agent_id.bold(),
                status_colored(*status, None)
            );
        }
        Response::RenameResult { agent_id, name } => {
            println!("Renamed agent {} to {}", agent_id.bold(), name.green());
        }
        Response::DeleteWorktreeResult {
            worktree_id,
            killed_agents,
            branch_deleted,
            remote_deleted,
        } => {
            println!("Deleted worktree {}", worktree_id.bold());
            if !killed_agents.is_empty() {
                println!("  Killed {} agent(s)", killed_agents.len());
            }
            println!(
                "  Branch deleted: {}",
                if *branch_deleted {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                }
            );
            println!(
                "  Remote deleted: {}",
                if *remote_deleted {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                }
            );
        }
        Response::LogsResult { agent_id, data } => {
            println!("{}", format!("--- Logs for {agent_id} ---").dimmed());
            print!("{data}");
        }
        Response::ShuttingDown => {
            println!("Daemon shutting down");
        }
        Response::Error { code, message } => {
            eprintln!("{} [{}]: {}", "error".red().bold(), code, message);
        }
        Response::Ok => {}
        Response::Output { data, .. } => {
            print!("{}", String::from_utf8_lossy(data));
        }
        Response::AttachReady { buffered_bytes } => {
            println!("Attached ({buffered_bytes} bytes buffered)");
        }
        Response::GridSubscribed => {
            println!("Grid subscription active");
        }
        Response::GridLayout { layout } => {
            println!("{}", serde_json::to_string_pretty(layout).unwrap());
        }
        Response::GridEvent {
            project_root,
            command,
        } => {
            println!("Grid event for {project_root}: {command:?}");
        }
        Response::StatusSubscribed => {
            println!("Status subscription active");
        }
        Response::StatusEvent { agents, worktrees } => {
            println!(
                "Status update: {} agents, {} worktrees",
                agents.len(),
                worktrees.len()
            );
        }
    }
}
