use clap::Subcommand;
use pu_core::machines::{self, Machine};
use pu_core::paths;
use serde_json::json;

use crate::error::CliError;
use crate::machine_target::{self, MachineState, core_error};

#[derive(Subcommand)]
pub enum MachineAction {
    /// List registered machines
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Register a machine reachable over SSH
    Add {
        /// Name used with --machine (letters, digits, '.', '_', '-')
        name: String,
        /// SSH destination: an ~/.ssh/config alias or user@host[:port]
        ssh_host: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Forget a machine (its daemon and agents keep running)
    Remove {
        /// Machine name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check machines' daemons over SSH (never starts them)
    Status {
        /// Machine name (default: every registered machine)
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the daemon on a machine (runs `pu health` there over ssh)
    Start {
        /// Machine name
        name: String,
    },
}

pub async fn run(action: MachineAction) -> Result<(), CliError> {
    match action {
        MachineAction::List { json } => run_list(json),
        MachineAction::Add {
            name,
            ssh_host,
            json,
        } => run_add(&name, &ssh_host, json),
        MachineAction::Remove { name, json } => run_remove(&name, json),
        MachineAction::Status { name, json } => run_status(name.as_deref(), json).await,
        MachineAction::Start { name } => run_start(&name).await,
    }
}

fn machine_json(machine: &Machine) -> serde_json::Value {
    json!({ "id": machine.id, "name": machine.name, "ssh_host": machine.ssh_host })
}

fn run_list(json: bool) -> Result<(), CliError> {
    let registry = machine_target::load_registry()?;
    if json {
        let list: Vec<_> = registry.machines.iter().map(machine_json).collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    if registry.machines.is_empty() {
        println!("No machines registered — add one with `pu machine add <name> <ssh_host>`");
        return Ok(());
    }
    let width = registry
        .machines
        .iter()
        .map(|m| m.name.len())
        .max()
        .unwrap_or(0);
    for machine in &registry.machines {
        println!("  {:width$}  {}", machine.name, machine.ssh_host);
    }
    Ok(())
}

fn run_add(name: &str, ssh_host: &str, json: bool) -> Result<(), CliError> {
    let path = paths::machines_registry_path()?;
    let machine = machines::update(&path, |r| r.add(name, ssh_host)).map_err(core_error)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&machine_json(&machine))?);
    } else {
        println!(
            "Added machine {} ({}) — check it with `pu machine status {}`",
            machine.name, machine.ssh_host, machine.name
        );
    }
    Ok(())
}

fn run_remove(name: &str, json: bool) -> Result<(), CliError> {
    let path = paths::machines_registry_path()?;
    let machine = machines::update(&path, |r| {
        r.remove(name).ok_or_else(|| {
            pu_core::error::PuError::InvalidArgument(format!("unknown machine {name:?}"))
        })
    })
    .map_err(core_error)?;
    // Best effort: drop dead CLI sockets and the directory if nothing else
    // (such as the app's live forward) still uses it.
    if let Ok(dir) = paths::machine_dir(&machine.id) {
        machine_target::sweep_stale_sockets(&dir);
        let _ = std::fs::remove_dir(&dir);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&machine_json(&machine))?);
    } else {
        println!(
            "Removed machine {} — its daemon and agents keep running",
            machine.name
        );
    }
    Ok(())
}

async fn run_status(name: Option<&str>, json: bool) -> Result<(), CliError> {
    let targets = match name {
        Some(name) => vec![machine_target::find_machine(name)?],
        None => machine_target::load_registry()?.machines,
    };
    let mut rows = Vec::with_capacity(targets.len());
    for machine in targets {
        let state = machine_target::probe(&machine).await;
        rows.push((machine, state));
    }
    if json {
        let list: Vec<_> = rows
            .iter()
            .map(|(machine, state)| {
                let mut value = machine_json(machine);
                value["status"] = json!(state.label());
                value["detail"] = json!(state.detail());
                value
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No machines registered — add one with `pu machine add <name> <ssh_host>`");
        return Ok(());
    }
    let width = rows.iter().map(|(m, _)| m.name.len()).max().unwrap_or(0);
    for (machine, state) in &rows {
        let hint = match state {
            MachineState::Stopped => {
                format!(" — start it with `pu machine start {}`", machine.name)
            }
            MachineState::Incompatible { .. } => " — update PurePoint on both machines".to_string(),
            _ => String::new(),
        };
        let detail = state
            .detail()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        println!(
            "  {:width$}  {}  {}{detail}{hint}",
            machine.name,
            machine.ssh_host,
            state.label()
        );
    }
    Ok(())
}

/// Longest `pu machine start` waits for ssh (the app waits as long).
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

async fn run_start(name: &str) -> Result<(), CliError> {
    let machine = machine_target::find_machine(name)?;
    machines::validate_ssh_host(&machine.ssh_host).map_err(core_error)?;
    let program = machine_target::ssh_program();
    let run = tokio::process::Command::new(&program)
        .args(machine_target::start_args(&machine.ssh_host))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    let status = tokio::time::timeout(START_TIMEOUT, run)
        .await
        .map_err(|_| {
            CliError::Other(format!(
                "starting the daemon on {} did not finish within {}s",
                machine.name,
                START_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| {
            CliError::Other(format!(
                "failed to run {}: {e}",
                std::path::Path::new(&program).display()
            ))
        })?;
    if !status.success() {
        return Err(CliError::Other(format!(
            "failed to start the daemon on {} ({status}) — is PurePoint installed there?",
            machine.name
        )));
    }
    Ok(())
}
