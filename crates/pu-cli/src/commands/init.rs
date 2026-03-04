use pu_core::{config, manifest, paths};
use pu_core::types::Manifest;

use crate::error::CliError;
use crate::skill;

pub async fn run(_socket: &std::path::Path, json: bool) -> Result<(), CliError> {
    let cwd = std::env::current_dir()?;
    let project_root = &cwd;

    if paths::manifest_path(project_root).exists() {
        if json {
            println!("{{\"created\":false}}");
        } else {
            println!("Already initialized");
        }
        return Ok(());
    }

    std::fs::create_dir_all(paths::pu_dir(project_root))?;

    let m = Manifest::new(project_root.to_string_lossy().to_string());
    manifest::write_manifest(project_root, &m).map_err(|e| CliError::Other(e.to_string()))?;

    config::write_default_config(project_root).map_err(|e| CliError::Other(e.to_string()))?;

    // Register skills
    skill::register_all_skills();

    // Write agent-context.md for non-Claude tools
    write_agent_context(project_root);

    if json {
        println!("{{\"created\":true}}");
    } else {
        println!("Initialized PurePoint workspace");
    }
    Ok(())
}

fn write_agent_context(project_root: &std::path::Path) {
    let pu_dir = paths::pu_dir(project_root);
    let path = pu_dir.join("agent-context.md");
    if path.exists() {
        return;
    }
    // Write a stripped-down version of the skill content for non-Claude tools
    let content = skill::skill_content();
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("warning: failed to write agent-context.md: {e}");
    }
}
