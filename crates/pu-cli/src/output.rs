use owo_colors::OwoColorize;
use pu_core::protocol::Response;
use pu_core::types::AgentStatus;

fn status_colored(status: AgentStatus) -> String {
    match status {
        AgentStatus::Running => "running".green().to_string(),
        AgentStatus::Spawning => "spawning".yellow().to_string(),
        AgentStatus::Idle => "idle".cyan().to_string(),
        AgentStatus::Completed => "completed".green().bold().to_string(),
        AgentStatus::Failed => "failed".red().to_string(),
        AgentStatus::Killed => "killed".red().to_string(),
        AgentStatus::Lost => "lost".red().dimmed().to_string(),
    }
}

pub fn print_response(response: &Response, json_mode: bool) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(response).unwrap());
        return;
    }
    match response {
        Response::HealthReport { pid, uptime_seconds, protocol_version, projects, agent_count } => {
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
        Response::SpawnResult { worktree_id, agent_id, status } => {
            println!("Spawned agent {} ({})", agent_id.bold(), status_colored(*status));
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
                println!("{:<14} {:<16} {}", "ID".bold(), "NAME".bold(), "STATUS".bold());
                for a in agents {
                    println!("{:<14} {:<16} {}", a.id.dimmed(), a.name, status_colored(a.status));
                }
            }
            for wt in worktrees {
                println!("\n{} {} ({}) — {:?}",
                    "Worktree".bold(),
                    wt.id.dimmed(),
                    wt.branch,
                    wt.status,
                );
                if !wt.agents.is_empty() {
                    println!("  {:<14} {:<16} {}", "ID".bold(), "NAME".bold(), "STATUS".bold());
                    for a in wt.agents.values() {
                        println!("  {:<14} {:<16} {}",
                            a.id.dimmed(),
                            a.name,
                            status_colored(a.status),
                        );
                    }
                }
            }
        }
        Response::AgentStatus(a) => {
            println!("{} {} {}", a.id.dimmed(), a.name.bold(), status_colored(a.status));
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
        Response::GridEvent { project_root, command } => {
            println!("Grid event for {project_root}: {command:?}");
        }
    }
}
