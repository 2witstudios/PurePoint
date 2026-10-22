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

pub(crate) fn print_project_removed(
    project_root: &str,
    removed: bool,
    suspended: &[String],
    killed: &[String],
) {
    if !removed {
        println!("{project_root} was not a registered project");
        return;
    }
    println!("{} {project_root}", "Removed".green());
    if !suspended.is_empty() {
        println!(
            "  Suspended {} agent(s); they can be resumed once the project is added again",
            suspended.len()
        );
    }
    if !killed.is_empty() {
        println!(
            "  Stopped {} session(s) the project did not list",
            killed.len()
        );
    }
    println!("  Project files and .pu/ were kept (suspending agents only updates the manifest)");
}

pub(crate) fn print_config_report(default_agent: &str, agents: &[AgentConfigInfo]) {
    println!("{}", "Agent Configuration".bold());
    println!("  Default agent: {}", default_agent.green());
    for a in agents {
        println!("\n  {} ({})", a.name.bold(), a.command.dimmed());
        println!("    Launch args: {}", launch_args_label(a));
        if let Some(ref command) = a.resolved_command {
            println!("    Resolved:    {command}");
        } else {
            println!("    Resolved:    {:?}", a.resolved_launch_args);
        }
    }
}

/// Effective launch args and where they come from, in precedence order.
fn launch_args_label(a: &AgentConfigInfo) -> String {
    match (&a.project_launch_args, &a.machine_launch_args) {
        (Some(args), _) => format!("{args} (project)"),
        (None, Some(args)) => format!("{args} (all projects)"),
        (None, None) => "none".dimmed().to_string(),
    }
}

pub(crate) fn print_error(code: &str, message: &str) {
    eprintln!("{} [{}]: {}", "error".red().bold(), code, message);
}

pub(crate) fn print_shutting_down() {
    println!("Daemon shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(project: Option<&str>, machine: Option<&str>) -> AgentConfigInfo {
        AgentConfigInfo {
            id: None,
            available: None,
            name: "codex".into(),
            command: "codex".into(),
            launch_args: None,
            resolved_launch_args: vec![],
            interactive: true,
            project_launch_args: project.map(str::to_owned),
            machine_launch_args: machine.map(str::to_owned),
            resolved_command: None,
            project_command: None,
            inherited_command: None,
            origin: None,
        }
    }

    #[test]
    fn given_project_or_machine_args_should_label_their_source() {
        assert_eq!(
            launch_args_label(&info(Some("--model o3"), Some("--search"))),
            "--model o3 (project)"
        );
        assert_eq!(
            launch_args_label(&info(None, Some("--search"))),
            "--search (all projects)"
        );
    }
}
