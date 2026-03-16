use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

const PLUGIN_JSON: &str = include_str!("../assets/plugin/.claude-plugin/plugin.json");
const SKILL_CONTENT: &str = include_str!("../assets/plugin/skills/pu/SKILL.md");
const COMMAND_PU: &str = include_str!("../assets/plugin/commands/pu.md");
const COMMAND_ORCHESTRATE: &str = include_str!("../assets/plugin/commands/orchestrate.md");

/// Returns the skill content (used for .pu/agent-context.md for non-Claude tools).
pub fn skill_content() -> &'static str {
    SKILL_CONTENT
}

fn plugin_hash() -> String {
    let mut h = DefaultHasher::new();
    PLUGIN_JSON.hash(&mut h);
    SKILL_CONTENT.hash(&mut h);
    COMMAND_PU.hash(&mut h);
    COMMAND_ORCHESTRATE.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn plugin_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".claude")
            .join("plugins")
            .join("purepoint"),
    )
}

fn old_skill_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".claude")
            .join("skills")
            .join("pu"),
    )
}

/// Write the Claude plugin files if they don't exist or are stale.
pub fn ensure_plugin_current() {
    let dir = match plugin_dir() {
        Some(d) => d,
        None => return,
    };
    let hash_path = dir.join(".hash");
    let current_hash = plugin_hash();

    // Check if hash matches (skip exists() -- just try the read)
    if let Ok(stored) = std::fs::read_to_string(&hash_path) {
        if stored.trim() == current_hash {
            return; // Up to date
        }
    }

    // Write plugin directory structure
    let files: &[(&str, &str)] = &[
        (".claude-plugin/plugin.json", PLUGIN_JSON),
        ("skills/pu/SKILL.md", SKILL_CONTENT),
        ("commands/pu.md", COMMAND_PU),
        ("commands/orchestrate.md", COMMAND_ORCHESTRATE),
    ];

    for (rel_path, content) in files {
        let path = dir.join(rel_path);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("pu: failed to create plugin directory: {e}");
                return;
            }
        }
        let tmp_path = path.with_extension("tmp");
        match std::fs::write(&tmp_path, content) {
            Ok(()) => {
                if let Err(e) = std::fs::rename(&tmp_path, &path) {
                    eprintln!("pu: failed to install plugin file {rel_path}: {e}");
                    let _ = std::fs::remove_file(&tmp_path);
                    return;
                }
            }
            Err(e) => {
                eprintln!("pu: failed to write plugin file {rel_path}: {e}");
                return;
            }
        }
    }

    if let Err(e) = std::fs::write(&hash_path, &current_hash) {
        eprintln!("pu: failed to write plugin hash: {e}");
    }

    // Migrate: remove old skill directory
    if let Some(old_dir) = old_skill_dir() {
        if old_dir.exists() {
            let _ = std::fs::remove_dir_all(&old_dir);
        }
    }
}
