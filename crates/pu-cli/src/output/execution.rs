use owo_colors::OwoColorize;
use pu_core::protocol::{
    AgentPulseEntry, ScheduleInfo, ScheduleTriggerPayload, TriggerInfo, WorktreeDiffEntry,
    WorktreePulseEntry,
};

use super::formatters::{format_duration, status_colored};

pub(crate) fn print_run_swarm_result(spawned_agents: &[String]) {
    println!("Spawned {} agent(s)", spawned_agents.len());
    for id in spawned_agents {
        println!("  {}", id.dimmed());
    }
}

pub(crate) fn print_run_swarm_partial(
    spawned_agents: &[String],
    error_code: &str,
    error_message: &str,
) {
    println!(
        "{}: {error_message} ({error_code})",
        "Swarm partially failed".red().bold()
    );
    if !spawned_agents.is_empty() {
        println!("Spawned {} agent(s) before failure:", spawned_agents.len());
        for id in spawned_agents {
            println!("  {}", id.dimmed());
        }
    }
}

pub(crate) fn print_diff_result(diffs: &[WorktreeDiffEntry]) {
    if diffs.is_empty() {
        println!("No worktree diffs");
        return;
    }
    for (i, d) in diffs.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let base = d.base_branch.as_deref().unwrap_or("(unknown)");
        println!(
            "{} {} ({} -> {})",
            "Worktree".bold(),
            d.worktree_name.bold(),
            base.dimmed(),
            d.branch.green()
        );
        if let Some(ref err) = d.error {
            println!("  {}: {}", "error".red().bold(), err);
        } else if d.files_changed == 0 && d.diff_output.trim().is_empty() {
            println!("  {}", "No changes".dimmed());
        } else {
            println!(
                "  {} file(s) changed, {} insertion(s), {} deletion(s)",
                d.files_changed, d.insertions, d.deletions
            );
            if !d.diff_output.trim().is_empty() {
                println!();
                print!("{}", d.diff_output);
            }
        }
    }
}

pub(crate) fn print_pulse_report(
    worktrees: &[WorktreePulseEntry],
    root_agents: &[AgentPulseEntry],
) {
    if worktrees.is_empty() && root_agents.is_empty() {
        println!("{}", "No active workspace".dimmed());
        return;
    }

    // Root-level agents
    if !root_agents.is_empty() {
        println!("{}", "Root Agents".bold().underline());
        for a in root_agents {
            print_agent_pulse(a);
        }
        if !worktrees.is_empty() {
            println!();
        }
    }

    for (i, wt) in worktrees.iter().enumerate() {
        if i > 0 {
            println!();
        }
        // Worktree header with elapsed time
        let elapsed = format_duration(wt.elapsed_seconds);
        println!(
            "{} {} {} ({})",
            "Worktree".bold(),
            wt.worktree_name.bold(),
            wt.branch.green(),
            elapsed.dimmed()
        );

        // Git stats
        if let Some(ref err) = wt.diff_error {
            println!("  git: {} {}", "error".red(), err);
        } else if wt.files_changed > 0 {
            println!(
                "  git: {} file(s), {} {}, {} {}",
                wt.files_changed.to_string().bold(),
                format!("+{}", wt.insertions).green(),
                "ins".dimmed(),
                format!("-{}", wt.deletions).red(),
                "del".dimmed()
            );
        } else {
            println!("  git: {}", "no changes yet".dimmed());
        }

        // Agents in this worktree
        if wt.agents.is_empty() {
            println!("  {}", "no agents".dimmed());
        } else {
            for a in &wt.agents {
                print_agent_pulse(a);
            }
        }
    }
}

pub(crate) fn print_agent_pulse(a: &AgentPulseEntry) {
    let status_str = status_colored(a.status, a.exit_code);
    let runtime = format_duration(a.runtime_seconds);
    let idle = a
        .idle_seconds
        .map(|s| {
            if s > 0 {
                format!(" idle {}", format_duration(s as i64))
            } else {
                String::new()
            }
        })
        .unwrap_or_default();

    println!(
        "  {} {} {} ({}{}){}",
        a.id.dimmed(),
        a.name,
        status_str,
        runtime.dimmed(),
        idle.dimmed(),
        a.prompt_snippet
            .as_ref()
            .map(|s| format!("\n    {}", s.dimmed()))
            .unwrap_or_default()
    );
}

pub(crate) fn print_schedule_list(schedules: &[ScheduleInfo]) {
    if schedules.is_empty() {
        println!("No schedules");
        return;
    }
    println!(
        "{:<20} {:<10} {:<10} {:<10} {}",
        "NAME".bold(),
        "RECURRENCE".bold(),
        "ENABLED".bold(),
        "SCOPE".bold(),
        "NEXT RUN".bold()
    );
    for s in schedules {
        let next = s
            .next_run
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string());
        let enabled_str = if s.enabled {
            "yes".green().to_string()
        } else {
            "no".dimmed().to_string()
        };
        println!(
            "{:<20} {:<10} {:<10} {:<10} {}",
            s.name, s.recurrence, enabled_str, s.scope, next
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn print_schedule_detail(
    name: &str,
    enabled: bool,
    recurrence: &str,
    start_at: &chrono::DateTime<chrono::Utc>,
    next_run: &Option<chrono::DateTime<chrono::Utc>>,
    trigger: &ScheduleTriggerPayload,
    scope: &str,
    root: bool,
    agent_name: &Option<String>,
) {
    println!("{} ({})", name.bold(), scope.dimmed());
    println!("  Enabled:    {enabled}");
    println!("  Recurrence: {recurrence}");
    println!("  Root:       {root}");
    if let Some(an) = agent_name {
        println!("  Agent name: {an}");
    }
    println!("  Start at:   {}", start_at.format("%Y-%m-%d %H:%M UTC"));
    if let Some(nr) = next_run {
        println!("  Next run:   {}", nr.format("%Y-%m-%d %H:%M UTC"));
    }
    match trigger {
        ScheduleTriggerPayload::AgentDef { name } => {
            println!("  Trigger:    agent-def ({name})");
        }
        ScheduleTriggerPayload::SwarmDef { name, vars } => {
            println!("  Trigger:    swarm-def ({name})");
            if !vars.is_empty() {
                for (k, v) in vars {
                    println!("    {k}={v}");
                }
            }
        }
        ScheduleTriggerPayload::InlinePrompt { prompt, agent } => {
            println!("  Trigger:    inline-prompt ({agent})");
            println!("  Prompt:     {prompt}");
        }
    }
}

pub(crate) fn print_trigger_list(triggers: &[TriggerInfo]) {
    if triggers.is_empty() {
        println!("No triggers");
        return;
    }
    println!(
        "{:<20} {:<14} {:<10} {}",
        "NAME".bold(),
        "EVENT".bold(),
        "SCOPE".bold(),
        "ACTIONS".bold()
    );
    for t in triggers {
        println!(
            "{:<20} {:<14} {:<10} {}",
            t.name,
            t.on,
            t.scope,
            t.sequence.len()
        );
    }
}

pub(crate) fn print_trigger_detail(t: &TriggerInfo) {
    println!("{} ({})", t.name.bold(), t.scope.dimmed());
    if let Some(ref desc) = t.description {
        println!("  {desc}");
    }
    println!("  Event: {}", t.on);
    println!("  Actions: {}", t.sequence.len());
    for (i, action) in t.sequence.iter().enumerate() {
        if let Some(ref inject) = action.inject {
            println!("  [{}] inject: {inject}", i + 1);
        }
        if let Some(ref gate) = action.gate {
            println!("  [{}] gate: {}", i + 1, gate.run);
        }
    }
    if !t.variables.is_empty() {
        println!("  Variables:");
        for (k, v) in &t.variables {
            println!("    {k}={v}");
        }
    }
}

pub(crate) fn print_gate_result(passed: bool, output: &str) {
    if !output.is_empty() {
        print!("{output}");
    }
    if passed {
        println!("{}", "All gates passed".green());
    } else {
        println!("{}", "Gate check failed".red().bold());
    }
}
