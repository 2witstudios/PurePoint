use owo_colors::OwoColorize;
use pu_core::protocol::{AgentDefInfo, SwarmDefInfo, SwarmRosterEntryPayload, TemplateInfo};

pub(crate) fn print_template_list(templates: &[TemplateInfo]) {
    if templates.is_empty() {
        println!("No templates");
        return;
    }
    println!(
        "{:<20} {:<12} {:<10} {}",
        "NAME".bold(),
        "AGENT".bold(),
        "SOURCE".bold(),
        "VARIABLES".bold()
    );
    for t in templates {
        println!(
            "{:<20} {:<12} {:<10} {}",
            t.name,
            t.agent,
            t.source,
            t.variables.join(", ")
        );
    }
}

pub(crate) fn print_template_detail(
    name: &str,
    description: &str,
    agent: &str,
    body: &str,
    source: &str,
    variables: &[String],
    command: &Option<String>,
) {
    println!("{} ({})", name.bold(), source.dimmed());
    if !description.is_empty() {
        println!("  {description}");
    }
    println!("  Agent: {agent}");
    if let Some(cmd) = command {
        println!("  Command: {cmd}");
    }
    if !variables.is_empty() {
        println!("  Variables: {}", variables.join(", "));
    }
    println!("---");
    print!("{body}");
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn print_agent_def_detail(
    name: &str,
    agent_type: &str,
    template: &Option<String>,
    inline_prompt: &Option<String>,
    tags: &[String],
    scope: &str,
    available_in_command_dialog: bool,
    icon: &Option<String>,
    command: &Option<String>,
) {
    println!("{} ({})", name.bold(), scope.dimmed());
    println!("  Type: {agent_type}");
    if let Some(cmd) = command {
        println!("  Command: {cmd}");
    }
    if let Some(tpl) = template {
        println!("  Template: {tpl}");
    }
    if let Some(prompt) = inline_prompt {
        println!("  Inline prompt:");
        for line in prompt.lines() {
            println!("    {line}");
        }
    }
    if !tags.is_empty() {
        println!("  Tags: {}", tags.join(", "));
    }
    if let Some(ic) = icon {
        println!("  Icon: {ic}");
    }
    println!("  Command dialog: {available_in_command_dialog}");
}

pub(crate) fn print_agent_def_list(agent_defs: &[AgentDefInfo]) {
    if agent_defs.is_empty() {
        println!("No agent definitions");
        return;
    }
    println!(
        "{:<20} {:<12} {:<10}",
        "NAME".bold(),
        "TYPE".bold(),
        "SCOPE".bold()
    );
    for d in agent_defs {
        println!("{:<20} {:<12} {:<10}", d.name, d.agent_type, d.scope);
    }
}

pub(crate) fn print_swarm_def_detail(
    name: &str,
    worktree_count: u32,
    worktree_template: &str,
    roster: &[SwarmRosterEntryPayload],
    include_terminal: bool,
    scope: &str,
) {
    println!("{} ({})", name.bold(), scope.dimmed());
    println!("  Worktrees: {worktree_count}");
    if !worktree_template.is_empty() {
        println!("  Template: {worktree_template}");
    }
    println!("  Terminal: {include_terminal}");
    if !roster.is_empty() {
        println!("  Roster:");
        for r in roster {
            println!("    {} ({}) x{}", r.agent_def, r.role, r.quantity);
        }
    }
}

pub(crate) fn print_swarm_def_list(swarm_defs: &[SwarmDefInfo]) {
    if swarm_defs.is_empty() {
        println!("No swarm definitions");
        return;
    }
    println!(
        "{:<20} {:<10} {:<10} {}",
        "NAME".bold(),
        "WORKTREES".bold(),
        "SCOPE".bold(),
        "ROSTER".bold()
    );
    for d in swarm_defs {
        let roster_summary: Vec<String> = d
            .roster
            .iter()
            .map(|r| format!("{}x{}", r.agent_def, r.quantity))
            .collect();
        println!(
            "{:<20} {:<10} {:<10} {}",
            d.name,
            d.worktree_count,
            d.scope,
            roster_summary.join(", ")
        );
    }
}
