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

use crate::error::CliError;

pub fn cwd_string() -> Result<String, CliError> {
    Ok(std::env::current_dir()?.to_string_lossy().to_string())
}
