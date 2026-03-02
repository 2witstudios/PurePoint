use pu_core::{config, manifest, paths};
use pu_core::types::Manifest;

use crate::error::CliError;

pub async fn run(_socket: &std::path::Path) -> Result<(), CliError> {
    let cwd = std::env::current_dir()?;
    let project_root = &cwd;

    if paths::manifest_path(project_root).exists() {
        println!("Already initialized");
        return Ok(());
    }

    std::fs::create_dir_all(paths::pu_dir(project_root))?;

    let m = Manifest::new(project_root.to_string_lossy().to_string());
    manifest::write_manifest(project_root, &m).map_err(|e| CliError::Other(e.to_string()))?;

    config::write_default_config(project_root).map_err(|e| CliError::Other(e.to_string()))?;

    println!("Initialized PurePoint workspace");
    Ok(())
}
