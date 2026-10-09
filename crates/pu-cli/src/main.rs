mod client;
mod commands;
mod daemon_ctrl;
mod error;
mod output;
mod skill;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pu", about = "PurePoint workspace orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read and post to the shared project channel (does not wake agents)
    Channel {
        #[arg(long, global = true)]
        project_root: Option<String>,
        #[command(subcommand)]
        action: commands::channel::ChannelAction,
    },
    /// Initialize a PurePoint workspace
    Init {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Spawn an agent in a new worktree
    Spawn {
        /// The prompt for the agent (optional if --template or --file provided)
        prompt: Option<String>,
        /// Agent type (default: claude)
        #[arg(short, long)]
        agent: Option<String>,
        /// Worktree name
        #[arg(short, long)]
        name: Option<String>,
        /// Base branch
        #[arg(short, long)]
        base: Option<String>,
        /// Spawn in project root (no worktree)
        #[arg(long, conflicts_with = "worktree")]
        root: bool,
        /// Add to existing worktree
        #[arg(short, long)]
        worktree: Option<String>,
        /// Use a saved prompt template by name
        #[arg(long, conflicts_with = "file")]
        template: Option<String>,
        /// Read prompt from a file path
        #[arg(long, conflicts_with = "template")]
        file: Option<String>,
        /// Command to run in the terminal (for terminal agents)
        #[arg(long)]
        command: Option<String>,
        /// Variable substitution (KEY=VALUE), repeatable
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
        /// Skip auto-mode flags (--dangerously-skip-permissions, --sandbox, etc.)
        #[arg(long)]
        no_auto: bool,
        /// Extra CLI flags passed directly to the agent (space-separated)
        #[arg(long)]
        agent_args: Option<String>,
        /// Launch agent in plan/architect mode (read-only research)
        #[arg(long)]
        plan: bool,
        /// Disable event triggers for this agent
        #[arg(long)]
        no_trigger: bool,
        /// Bind an idle trigger to this agent (name of trigger in .pu/triggers/)
        #[arg(long, conflicts_with = "no_trigger")]
        trigger: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show workspace status
    Status {
        /// Show single agent status
        #[arg(long)]
        agent: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Bench (suspend) agents — pull them off the court
    #[command(long_about = "Bench (suspend) agents — pull them off the court.\n\n\
        When using --all, the invoking agent may also be suspended.\n\
        Use `pu play <agent_id>` to resume a benched agent.")]
    Bench {
        /// Agent ID to bench
        #[arg(conflicts_with = "all")]
        agent_id: Option<String>,
        /// Bench all active agents
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Put a benched agent back in play (resume)
    Play {
        /// Agent ID to resume
        agent_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Kill agents
    Kill {
        /// Kill specific agent
        #[arg(long, conflicts_with_all = ["worktree", "all"])]
        agent: Option<String>,
        /// Kill all agents in worktree
        #[arg(short, long, conflicts_with = "all")]
        worktree: Option<String>,
        /// Kill all agents
        #[arg(long)]
        all: bool,
        /// Also kill root-level agents (point guards). By default --all only kills worktree agents.
        #[arg(long, requires = "all")]
        include_root: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Attach to an agent's terminal
    Attach {
        /// Agent ID
        agent_id: String,
    },
    /// View agent output logs
    Logs {
        /// Agent ID
        agent_id: String,
        /// Number of bytes to read from tail
        #[arg(long, default_value = "500")]
        tail: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check daemon health
    Health {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage saved prompt templates
    Prompt {
        #[command(subcommand)]
        action: PromptAction,
    },
    /// Manage saved agent definitions
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Manage swarm compositions
    Swarm {
        #[command(subcommand)]
        action: SwarmAction,
    },
    /// Send text or keys to an agent's terminal
    Send {
        /// Agent ID
        agent_id: String,
        /// Text to send
        text: Option<String>,
        /// Don't append Enter after text
        #[arg(long)]
        no_enter: bool,
        /// Send a control key sequence (e.g., C-c, C-d)
        #[arg(long, conflicts_with = "text")]
        keys: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Control the pane grid layout
    Grid {
        /// Workspace to act on (an id from `pu grid show`; default: the one on screen).
        /// Leaf and tab ids are only unique within a workspace.
        #[arg(long, global = true)]
        workspace: Option<String>,
        #[command(subcommand)]
        action: GridAction,
    },
    /// Manage scheduled tasks
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },
    /// Manage event-driven triggers
    Trigger {
        #[command(subcommand)]
        action: TriggerAction,
    },
    /// Evaluate git hook gates
    Gate {
        /// Event type: pre-commit or pre-push
        event: String,
        /// Project root path
        #[arg(long)]
        project_root: Option<String>,
    },
    /// Workspace pulse — agents, runtimes, and git stats at a glance
    Pulse {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show git diffs across agent worktrees
    Diff {
        /// Diff a specific worktree
        #[arg(long)]
        worktree: Option<String>,
        /// Show file summary instead of full diff
        #[arg(long)]
        stat: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Live dashboard showing all agents in real-time
    Watch {
        /// Refresh interval in milliseconds (default: 800)
        #[arg(long)]
        interval: Option<u64>,
    },
    /// Remove worktrees, their agents, and branches
    Clean {
        /// Remove a specific worktree
        #[arg(long, conflicts_with = "all")]
        worktree: Option<String>,
        /// Remove all worktrees
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum PromptAction {
    /// List available prompt templates
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a prompt template
    Show {
        /// Template name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a prompt template
    Create {
        /// Template name
        name: String,
        /// Template body
        #[arg(long)]
        body: String,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
        /// Agent type
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a prompt template
    Delete {
        /// Template name
        name: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// List agent definitions
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create an agent definition
    Create {
        /// Agent definition name
        name: String,
        /// Agent type
        #[arg(long, default_value = "claude")]
        agent_type: String,
        /// Prompt template name to use
        #[arg(long)]
        template: Option<String>,
        /// Inline prompt text
        #[arg(long)]
        inline_prompt: Option<String>,
        /// Command to run (for terminal agents)
        #[arg(long)]
        command: Option<String>,
        /// Comma-separated tags
        #[arg(long, default_value = "")]
        tags: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show an agent definition
    Show {
        /// Agent definition name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete an agent definition
    Delete {
        /// Agent definition name
        name: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SwarmAction {
    /// List swarm definitions
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a swarm definition
    Create {
        /// Swarm name
        name: String,
        /// Number of worktrees
        #[arg(long, default_value = "1")]
        worktrees: u32,
        /// Worktree template name
        #[arg(long, default_value = "")]
        worktree_template: String,
        /// Roster entry: "agent_def:role:quantity" (repeatable)
        #[arg(long = "roster", value_name = "AGENT:ROLE:QTY")]
        roster: Vec<String>,
        /// Include terminal in swarm
        #[arg(long)]
        include_terminal: bool,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a swarm definition
    Show {
        /// Swarm name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a swarm definition
    Delete {
        /// Swarm name
        name: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Run a swarm
    Run {
        /// Swarm name
        name: String,
        /// Variable substitution (KEY=VALUE), repeatable
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum GridAction {
    /// Show current grid layout
    Show {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Split a pane
    Split {
        /// Axis: v (vertical/left-right) or h (horizontal/top-bottom)
        #[arg(long, default_value = "v")]
        axis: String,
        /// Leaf ID to split (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
    },
    /// Close a pane
    Close {
        /// Leaf ID to close (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
    },
    /// Move focus to another pane
    Focus {
        /// Direction: up, down, left, right
        #[arg(long)]
        direction: Option<String>,
        /// Focus specific leaf ID
        #[arg(long)]
        leaf: Option<u32>,
    },
    /// Show an agent in a pane's active tab
    Assign {
        /// Agent ID
        agent_id: String,
        /// Leaf ID (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
    },
    /// Manage the tabs inside a pane
    Tab {
        #[command(subcommand)]
        action: TabAction,
    },
}

#[derive(Subcommand)]
enum TabAction {
    /// Open a tab after the pane's active tab (empty unless --agent is given)
    New {
        /// Leaf ID (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
        /// Agent ID to show in the new tab
        #[arg(long)]
        agent: Option<String>,
    },
    /// Select a tab by 1-based position, or the next/previous tab
    #[command(group(
        clap::ArgGroup::new("target")
            .required(true)
            .args(["index", "next", "prev"])
    ))]
    Select {
        /// 1-based tab position in the pane
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        index: Option<u32>,
        /// Select the next tab
        #[arg(long)]
        next: bool,
        /// Select the previous tab
        #[arg(long)]
        prev: bool,
        /// Leaf ID (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
    },
    /// Close a tab (default: the pane's active tab)
    Close {
        /// Leaf ID (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
        /// Tab ID to close
        #[arg(long)]
        tab: Option<u32>,
    },
    /// Move a tab to another pane
    Move {
        /// Tab ID to move
        tab_id: u32,
        /// Destination leaf ID
        #[arg(long)]
        to: u32,
        /// 1-based destination position (default: append)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        index: Option<u32>,
    },
    /// Move a tab into a new pane split off its current pane
    Break {
        /// Tab ID (default: the focused pane's active tab)
        #[arg(long)]
        tab: Option<u32>,
        /// Axis: v (vertical/left-right) or h (horizontal/top-bottom)
        #[arg(long, default_value = "v", value_parser = ["v", "h"])]
        axis: String,
    },
}

#[derive(Subcommand)]
enum ScheduleAction {
    /// List schedules
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a schedule
    Create {
        /// Schedule name
        name: String,
        /// Recurrence: none, hourly, daily, weekdays, weekly, monthly
        #[arg(long, default_value = "none")]
        recurrence: String,
        /// Start time (RFC 3339 or YYYY-MM-DDTHH:MM:SS)
        #[arg(long)]
        start_at: String,
        /// Trigger type: agent-def, swarm-def, inline-prompt
        #[arg(long = "trigger")]
        trigger_type: String,
        /// Trigger name (for agent-def or swarm-def triggers)
        #[arg(long)]
        trigger_name: Option<String>,
        /// Trigger prompt (for inline-prompt trigger)
        #[arg(long)]
        trigger_prompt: Option<String>,
        /// Agent type for inline-prompt trigger
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Variable substitution (KEY=VALUE), repeatable
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Spawn as root agent (in project root, not a worktree)
        #[arg(long)]
        root: bool,
        /// Worktree/branch name (required when not --root)
        #[arg(long = "name")]
        agent_name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a schedule
    Show {
        /// Schedule name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a schedule
    Delete {
        /// Schedule name
        name: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Enable a schedule
    Enable {
        /// Schedule name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Disable a schedule
    Disable {
        /// Schedule name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum TriggerAction {
    /// List triggers
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a trigger
    Show {
        /// Trigger name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a trigger
    Create {
        /// Trigger name
        name: String,
        /// Event type: agent_idle, pre_commit, pre_push
        #[arg(long = "on")]
        event: String,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Inject text (repeatable, creates sequence steps)
        #[arg(long)]
        inject: Vec<String>,
        /// Gate command (repeatable, creates gate-only sequence steps)
        #[arg(long)]
        gate: Vec<String>,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a trigger
    Delete {
        /// Trigger name
        name: String,
        /// Scope: local or global
        #[arg(long, default_value = "local")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Assign a trigger to an idle agent
    Assign {
        /// Agent ID
        agent_id: String,
        /// Trigger name
        trigger_name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let socket = match pu_core::paths::daemon_socket_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Background plugin freshness check (non-blocking)
    std::thread::spawn(skill::ensure_plugin_current);

    let result = match cli.command {
        Commands::Init { json } => commands::init::run(&socket, json).await,
        Commands::Spawn {
            prompt,
            agent,
            name,
            base,
            root,
            worktree,
            template,
            file,
            command,
            vars,
            no_auto,
            agent_args,
            plan,
            no_trigger,
            trigger,
            json,
        } => {
            commands::spawn::run(
                &socket, prompt, agent, name, base, root, worktree, template, file, command, vars,
                no_auto, agent_args, plan, no_trigger, trigger, json,
            )
            .await
        }
        Commands::Bench {
            agent_id,
            all,
            json,
        } => commands::bench::run_bench(&socket, agent_id, all, json).await,
        Commands::Play { agent_id, json } => {
            commands::bench::run_play(&socket, &agent_id, json).await
        }
        Commands::Status { agent, json } => commands::status::run(&socket, agent, json).await,
        Commands::Kill {
            agent,
            worktree,
            all,
            include_root,
            json,
        } => commands::kill::run(&socket, agent, worktree, all, include_root, json).await,
        Commands::Attach { agent_id } => commands::attach::run(&socket, &agent_id).await,
        Commands::Logs {
            agent_id,
            tail,
            json,
        } => commands::logs::run(&socket, &agent_id, tail, json).await,
        Commands::Health { json } => commands::health::run(&socket, json).await,
        Commands::Prompt { action } => match action {
            PromptAction::List { json } => commands::prompt::run_list(&socket, json).await,
            PromptAction::Show { name, json } => {
                commands::prompt::run_show(&socket, &name, json).await
            }
            PromptAction::Create {
                name,
                body,
                description,
                agent,
                scope,
                json,
            } => {
                commands::prompt::run_create(
                    &socket,
                    &name,
                    &body,
                    &description,
                    &agent,
                    &scope,
                    json,
                )
                .await
            }
            PromptAction::Delete { name, scope, json } => {
                commands::prompt::run_delete(&socket, &name, &scope, json).await
            }
        },
        Commands::Agent { action } => match action {
            AgentAction::List { json } => commands::agent_def::run_list(&socket, json).await,
            AgentAction::Create {
                name,
                agent_type,
                template,
                inline_prompt,
                command,
                tags,
                scope,
                json,
            } => {
                commands::agent_def::run_create(
                    &socket,
                    &name,
                    &agent_type,
                    template,
                    inline_prompt,
                    command,
                    &tags,
                    &scope,
                    json,
                )
                .await
            }
            AgentAction::Show { name, json } => {
                commands::agent_def::run_show(&socket, &name, json).await
            }
            AgentAction::Delete { name, scope, json } => {
                commands::agent_def::run_delete(&socket, &name, &scope, json).await
            }
        },
        Commands::Swarm { action } => match action {
            SwarmAction::List { json } => commands::swarm::run_list(&socket, json).await,
            SwarmAction::Create {
                name,
                worktrees,
                worktree_template,
                roster,
                include_terminal,
                scope,
                json,
            } => {
                commands::swarm::run_create(
                    &socket,
                    &name,
                    worktrees,
                    &worktree_template,
                    roster,
                    include_terminal,
                    &scope,
                    json,
                )
                .await
            }
            SwarmAction::Show { name, json } => {
                commands::swarm::run_show(&socket, &name, json).await
            }
            SwarmAction::Delete { name, scope, json } => {
                commands::swarm::run_delete(&socket, &name, &scope, json).await
            }
            SwarmAction::Run { name, vars, json } => {
                commands::swarm::run_run(&socket, &name, vars, json).await
            }
        },
        Commands::Send {
            agent_id,
            text,
            no_enter,
            keys,
            json,
        } => commands::send::run(&socket, &agent_id, text, no_enter, keys, json).await,
        Commands::Grid { workspace, action } => {
            commands::grid::run(&socket, workspace, action).await
        }
        Commands::Trigger { action } => match action {
            TriggerAction::List { json } => commands::trigger::run_list(&socket, json).await,
            TriggerAction::Show { name, json } => {
                commands::trigger::run_show(&socket, &name, json).await
            }
            TriggerAction::Create {
                name,
                event,
                description,
                inject,
                gate,
                scope,
                json,
            } => {
                commands::trigger::run_create(
                    &socket,
                    commands::trigger::CreateTriggerParams {
                        name,
                        event,
                        description,
                        injects: inject,
                        gates: gate,
                        scope,
                        json,
                    },
                )
                .await
            }
            TriggerAction::Delete { name, scope, json } => {
                commands::trigger::run_delete(&socket, &name, &scope, json).await
            }
            TriggerAction::Assign {
                agent_id,
                trigger_name,
                json,
            } => commands::trigger::run_assign(&socket, &agent_id, &trigger_name, json).await,
        },
        Commands::Gate {
            event,
            project_root,
        } => commands::gate::run(&socket, &event, project_root).await,
        Commands::Channel {
            project_root,
            action,
        } => commands::channel::run(&socket, project_root, action).await,
        Commands::Pulse { json } => commands::pulse::run(&socket, json).await,
        Commands::Diff {
            worktree,
            stat,
            json,
        } => commands::diff::run(&socket, worktree, stat, json).await,
        Commands::Watch { interval } => commands::watch::run(&socket, interval).await,
        Commands::Clean {
            worktree,
            all,
            json,
        } => commands::clean::run(&socket, worktree, all, json).await,
        Commands::Schedule { action } => match action {
            ScheduleAction::List { json } => commands::schedule::run_list(&socket, json).await,
            ScheduleAction::Create {
                name,
                recurrence,
                start_at,
                trigger_type,
                trigger_name,
                trigger_prompt,
                agent,
                vars,
                scope,
                root,
                agent_name,
                json,
            } => {
                commands::schedule::run_create(
                    &socket,
                    &name,
                    &recurrence,
                    &start_at,
                    &trigger_type,
                    trigger_name.as_deref(),
                    trigger_prompt.as_deref(),
                    &agent,
                    vars,
                    &scope,
                    root,
                    agent_name,
                    json,
                )
                .await
            }
            ScheduleAction::Show { name, json } => {
                commands::schedule::run_show(&socket, &name, json).await
            }
            ScheduleAction::Delete { name, scope, json } => {
                commands::schedule::run_delete(&socket, &name, &scope, json).await
            }
            ScheduleAction::Enable { name, json } => {
                commands::schedule::run_enable(&socket, &name, json).await
            }
            ScheduleAction::Disable { name, json } => {
                commands::schedule::run_disable(&socket, &name, json).await
            }
        },
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_channel_commands_should_parse_routing_and_reject_invalid_cursors() {
        let cli = Cli::try_parse_from([
            "pu",
            "channel",
            "send",
            "hello",
            "--reply-to",
            "msg-1",
            "--commit",
            "abcdef0",
            "--pr",
            "42",
            "--project-root",
            "/project",
            "--json",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Commands::Channel { project_root: Some(root), action: commands::channel::ChannelAction::Send { json: true, .. } } if root == "/project")
        );
        for args in [
            vec!["pu", "channel", "read", "--since", "1", "--before", "5"],
            vec!["pu", "channel", "read", "--limit", "201"],
            vec!["pu", "channel", "read", "--limit", "0"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        for args in [
            vec![
                "pu", "channel", "read", "--search", "hello", "--thread", "msg-1",
            ],
            vec!["pu", "channel", "edit", "msg-1", "changed"],
            vec!["pu", "channel", "react", "msg-1", "--remove"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }

    fn parse_tab(args: &[&str]) -> Result<TabAction, clap::Error> {
        let argv = ["pu", "grid", "tab"].iter().chain(args).copied();
        match Cli::try_parse_from(argv)?.command {
            Commands::Grid {
                action: GridAction::Tab { action },
                ..
            } => Ok(action),
            _ => panic!("expected grid tab"),
        }
    }

    #[test]
    fn given_select_with_position_should_parse_index() {
        let action = parse_tab(&["select", "2", "--leaf", "1"]).unwrap();
        assert!(matches!(
            action,
            TabAction::Select {
                index: Some(2),
                next: false,
                prev: false,
                leaf: Some(1)
            }
        ));
    }

    #[test]
    fn given_select_without_target_should_be_rejected() {
        assert!(parse_tab(&["select"]).is_err());
    }

    #[test]
    fn given_select_with_two_targets_should_be_rejected() {
        assert!(parse_tab(&["select", "2", "--next"]).is_err());
        assert!(parse_tab(&["select", "--next", "--prev"]).is_err());
    }

    #[test]
    fn given_select_position_zero_should_be_rejected() {
        assert!(parse_tab(&["select", "0"]).is_err());
    }

    #[test]
    fn given_move_should_require_destination_leaf() {
        assert!(parse_tab(&["move", "4"]).is_err());
        let action = parse_tab(&["move", "4", "--to", "1", "--index", "2"]).unwrap();
        assert!(matches!(
            action,
            TabAction::Move {
                tab_id: 4,
                to: 1,
                index: Some(2)
            }
        ));
    }

    #[test]
    fn given_break_without_axis_should_default_to_vertical() {
        let action = parse_tab(&["break"]).unwrap();
        match action {
            TabAction::Break { tab, axis } => {
                assert_eq!(tab, None);
                assert_eq!(axis, "v");
            }
            _ => panic!("expected Break"),
        }
        assert!(parse_tab(&["break", "--axis", "x"]).is_err());
    }

    #[test]
    fn given_assign_without_leaf_should_leave_leaf_unset() {
        let cli = Cli::try_parse_from(["pu", "grid", "assign", "ag-a"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Grid {
                action: GridAction::Assign { leaf: None, .. },
                workspace: None,
            }
        ));
    }

    #[test]
    fn given_workspace_flag_after_subcommand_should_apply_to_grid() {
        let cli = Cli::try_parse_from([
            "pu",
            "grid",
            "tab",
            "close",
            "--tab",
            "3",
            "--workspace",
            "ws-ag-b",
        ])
        .unwrap();
        match cli.command {
            Commands::Grid { workspace, .. } => assert_eq!(workspace.as_deref(), Some("ws-ag-b")),
            _ => panic!("expected grid"),
        }
    }
}
