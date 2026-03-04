use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

const SKILL_CONTENT: &str = include_str!("../assets/SKILL.md");

pub fn skill_content() -> &'static str {
    SKILL_CONTENT
}

fn skill_hash() -> String {
    let mut h = DefaultHasher::new();
    SKILL_CONTENT.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn claude_skill_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".claude").join("skills").join("pu").join("SKILL.md"))
}

fn hash_marker_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".claude").join("skills").join("pu").join(".hash"))
}

/// Write the Claude skill file if it doesn't exist or is stale.
pub fn ensure_claude_skill() {
    let skill_path = match claude_skill_path() {
        Some(p) => p,
        None => return,
    };
    let hash_path = match hash_marker_path() {
        Some(p) => p,
        None => return,
    };

    let current_hash = skill_hash();

    // Check if hash matches
    if hash_path.exists() {
        if let Ok(stored) = std::fs::read_to_string(&hash_path) {
            if stored.trim() == current_hash {
                return; // Up to date
            }
        }
    }

    // Write skill file atomically via temp + rename
    if let Some(parent) = skill_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp_path = skill_path.with_extension("md.tmp");
    if std::fs::write(&tmp_path, SKILL_CONTENT).is_ok() {
        std::fs::rename(&tmp_path, &skill_path).ok();
    }
    std::fs::write(&hash_path, &current_hash).ok();
}

/// Register all skills (called from `pu init`).
pub fn register_all_skills() {
    ensure_claude_skill();
}

/// Background freshness check (called on every `pu` command).
pub fn ensure_skill_current() {
    ensure_claude_skill();
}
