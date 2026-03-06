pub mod agent_def;
pub mod attach;
pub mod grid;
pub mod health;
pub mod init;
pub mod kill;
pub mod logs;
pub mod prompt;
pub mod send;
pub mod spawn;
pub mod status;
pub mod swarm;

use std::collections::HashMap;

use crate::error::CliError;

pub fn cwd_string() -> Result<String, CliError> {
    Ok(std::env::current_dir()?.to_string_lossy().to_string())
}

/// Parse --var KEY=VALUE pairs into a HashMap.
pub fn parse_vars(vars: &[String]) -> Result<HashMap<String, String>, CliError> {
    let mut map = HashMap::new();
    for v in vars {
        let (key, value) = v.split_once('=').ok_or_else(|| {
            CliError::Other(format!("invalid --var format: {v} (expected KEY=VALUE)"))
        })?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}
