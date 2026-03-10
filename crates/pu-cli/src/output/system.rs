use owo_colors::OwoColorize;
use pu_core::protocol::AgentConfigInfo;

pub(crate) fn print_health_report(
    pid: u32,
    uptime_seconds: u64,
    protocol_version: u32,
    projects: &[String],
    agent_count: usize,
) {
    println!("{}", "Daemon healthy".green().bold());
    println!("  PID:      {pid}");
    println!("  Uptime:   {uptime_seconds}s");
    println!("  Protocol: v{protocol_version}");
    println!("  Projects: {}", projects.len());
    println!("  Agents:   {agent_count}");
}

pub(crate) fn print_init_result(created: bool) {
    if created {
        println!("{}", "Initialized PurePoint workspace".green());
    } else {
        println!("Already initialized");
    }
}

pub(crate) fn print_config_report(default_agent: &str, agents: &[AgentConfigInfo]) {
    println!("{}", "Agent Configuration".bold());
    println!("  Default agent: {}", default_agent.green());
    for a in agents {
        println!("\n  {} ({})", a.name.bold(), a.command.dimmed());
        if let Some(ref args) = a.launch_args {
            println!("    Launch args: {args:?}");
        } else {
            println!("    Launch args: {} (using defaults)", "none".dimmed());
        }
        println!("    Resolved:    {:?}", a.resolved_launch_args);
    }
}

pub(crate) fn print_error(code: &str, message: &str) {
    eprintln!("{} [{}]: {}", "error".red().bold(), code, message);
}

pub(crate) fn print_shutting_down() {
    println!("Daemon shutting down");
}
