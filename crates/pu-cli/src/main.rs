mod client;
mod daemon_ctrl;
mod error;
mod output;
mod commands;

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
    Init,
    /// Spawn an agent in a new worktree
    Spawn {
        /// The prompt for the agent
        prompt: String,
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
    },
    /// Check daemon health
    Health,
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

    let result = match cli.command {
        Commands::Init => commands::init::run(&socket).await,
        Commands::Spawn { prompt, agent, name, base, root, worktree } => {
            commands::spawn::run(&socket, prompt, agent, name, base, root, worktree).await
        }
        Commands::Status { agent, json } => commands::status::run(&socket, agent, json).await,
        Commands::Kill { agent, worktree, all } => {
            commands::kill::run(&socket, agent, worktree, all).await
        }
        Commands::Attach { agent_id } => commands::attach::run(&socket, &agent_id).await,
        Commands::Logs { agent_id, tail } => commands::logs::run(&socket, &agent_id, tail).await,
        Commands::Health => commands::health::run(&socket).await,
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
