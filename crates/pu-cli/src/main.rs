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
        /// Variable substitution (KEY=VALUE), repeatable
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
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
        #[command(subcommand)]
        action: GridAction,
    },
    /// Manage scheduled tasks
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
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
    /// Assign an agent to a pane
    Assign {
        /// Agent ID
        agent_id: String,
        /// Leaf ID (default: focused pane)
        #[arg(long)]
        leaf: Option<u32>,
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

    // Background skill freshness check (non-blocking)
    std::thread::spawn(skill::ensure_skill_current);

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
            vars,
            json,
        } => {
            commands::spawn::run(
                &socket, prompt, agent, name, base, root, worktree, template, file, vars, json,
            )
            .await
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
        Commands::Grid { action } => commands::grid::run(&socket, action).await,
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
