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

fn skill_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".claude")
            .join("skills")
            .join("pu"),
    )
}

/// Write the Claude skill file if it doesn't exist or is stale.
pub fn ensure_skill_current() {
    let dir = match skill_dir() {
        Some(d) => d,
        None => return,
    };
    let skill_path = dir.join("SKILL.md");
    let hash_path = dir.join(".hash");

    let current_hash = skill_hash();

    // Check if hash matches (skip exists() — just try the read)
    if let Ok(stored) = std::fs::read_to_string(&hash_path) {
        if stored.trim() == current_hash {
            return; // Up to date
        }
    }

    // Write skill file atomically via temp + rename
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("pu: failed to create skill directory: {e}");
        return;
    }
    let tmp_path = skill_path.with_extension("md.tmp");
    match std::fs::write(&tmp_path, SKILL_CONTENT) {
        Ok(()) => {
            if let Err(e) = std::fs::rename(&tmp_path, &skill_path) {
                eprintln!("pu: failed to install skill file: {e}");
                let _ = std::fs::remove_file(&tmp_path);
                return;
            }
        }
        Err(e) => {
            eprintln!("pu: failed to write skill file: {e}");
            return;
        }
    }
    if let Err(e) = std::fs::write(&hash_path, &current_hash) {
        eprintln!("pu: failed to write skill hash: {e}");
    }
}
