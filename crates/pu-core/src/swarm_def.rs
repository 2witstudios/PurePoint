use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmDef {
    pub name: String,
    #[serde(default = "crate::serde_defaults::default_worktree_count")]
    pub worktree_count: u32,
    /// Branch name template for spawned worktrees. Use `{index}` as a placeholder
    /// for the worktree iteration index (0-based). E.g. `"feature-{index}"` produces
    /// branches `feature-0`, `feature-1`, etc. Empty string generates automatic names.
    #[serde(default)]
    pub worktree_template: String,
    #[serde(default)]
    pub roster: Vec<SwarmRosterEntry>,
    #[serde(default)]
    pub include_terminal: bool,
    /// "local" or "global" — set at load time
    #[serde(skip)]
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmRosterEntry {
    pub agent_def: String,
    pub role: String,
    #[serde(default = "crate::serde_defaults::default_quantity")]
    pub quantity: u32,
}

/// Scan both local and global swarm definition directories. Local defs take priority.
pub fn list_swarm_defs(project_root: &Path) -> Vec<SwarmDef> {
    let mut seen = HashMap::new();
    let mut result = Vec::new();

    // Local first
    let local_dir = paths::swarms_dir(project_root);
    if local_dir.is_dir() {
        for def in scan_dir(&local_dir, "local") {
            seen.insert(def.name.clone(), result.len());
            result.push(def);
        }
    }

    // Global second (skip duplicates)
    if let Ok(global_dir) = paths::global_swarms_dir() {
        if global_dir.is_dir() {
            for def in scan_dir(&global_dir, "global") {
                if !seen.contains_key(&def.name) {
                    result.push(def);
                }
            }
        }
    }

    result
}

/// Find a swarm definition by name. Checks local first, then global.
pub fn find_swarm_def(project_root: &Path, name: &str) -> Option<SwarmDef> {
    let local_dir = paths::swarms_dir(project_root);
    if local_dir.is_dir() {
        if let Some(def) = find_in_dir(&local_dir, name, "local") {
            return Some(def);
        }
    }
    if let Ok(global_dir) = paths::global_swarms_dir() {
        if global_dir.is_dir() {
            if let Some(def) = find_in_dir(&global_dir, name, "global") {
                return Some(def);
            }
        }
    }
    None
}

/// Save a swarm definition as a YAML file. Creates the directory if needed.
pub fn save_swarm_def(dir: &Path, def: &SwarmDef) -> Result<(), std::io::Error> {
    crate::validation::validate_name(&def.name)?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.yaml", def.name));
    let yaml = serde_yml::to_string(def).map_err(std::io::Error::other)?;
    std::fs::write(path, yaml)
}

/// Delete a swarm definition file. Returns true if the file existed.
pub fn delete_swarm_def(dir: &Path, name: &str) -> Result<bool, std::io::Error> {
    crate::validation::validate_name(name)?;
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        std::fs::remove_file(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn scan_dir(dir: &Path, scope: &str) -> Vec<SwarmDef> {
    let mut defs = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return defs,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                match serde_yml::from_str::<SwarmDef>(&content) {
                    Ok(mut def) => {
                        def.scope = scope.to_string();
                        defs.push(def);
                    }
                    Err(e) => {
                        eprintln!("warning: failed to parse {}: {e}", path.display());
                    }
                }
            }
        }
    }
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    defs
}

fn find_in_dir(dir: &Path, name: &str, scope: &str) -> Option<SwarmDef> {
    crate::validation::validate_name(name).ok()?;
    // Try exact file name first
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut def) = serde_yml::from_str::<SwarmDef>(&content) {
                def.scope = scope.to_string();
                return Some(def);
            }
        }
    }
    // Scan all files and match by name field
    scan_dir(dir, scope)
        .into_iter()
        .find(|def| def.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn given_swarm_def_yaml_should_deserialize() {
        let yaml = "name: full-stack\nworktree_count: 3\nworktree_template: feature\nroster:\n  - agent_def: reviewer\n    role: review\n    quantity: 2\ninclude_terminal: true\n";
        let def: SwarmDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.name, "full-stack");
        assert_eq!(def.worktree_count, 3);
        assert_eq!(def.worktree_template, "feature");
        assert_eq!(def.roster.len(), 1);
        assert_eq!(def.roster[0].agent_def, "reviewer");
        assert_eq!(def.roster[0].role, "review");
        assert_eq!(def.roster[0].quantity, 2);
        assert!(def.include_terminal);
        assert_eq!(def.scope, ""); // skip_deserializing
    }

    #[test]
    fn given_minimal_yaml_should_use_defaults() {
        let yaml = "name: basic\n";
        let def: SwarmDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.worktree_count, 1);
        assert_eq!(def.worktree_template, "");
        assert!(def.roster.is_empty());
        assert!(!def.include_terminal);
    }

    #[test]
    fn given_roster_entry_should_default_quantity() {
        let yaml = "agent_def: builder\nrole: build\n";
        let entry: SwarmRosterEntry = serde_yml::from_str(yaml).unwrap();
        assert_eq!(entry.quantity, 1);
    }

    #[test]
    fn given_local_and_global_swarm_defs_should_list_local_first() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::swarms_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("full-stack.yaml"),
            "name: full-stack\nworktree_count: 3\n",
        )
        .unwrap();
        std::fs::write(
            local_dir.join("backend.yaml"),
            "name: backend\nworktree_count: 2\n",
        )
        .unwrap();

        let defs = list_swarm_defs(root);
        assert_eq!(defs.len(), 2);
        // Sorted alphabetically
        assert_eq!(defs[0].name, "backend");
        assert_eq!(defs[1].name, "full-stack");
        assert_eq!(defs[0].scope, "local");
    }

    #[test]
    fn given_swarm_def_name_should_find_by_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::swarms_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("full-stack.yaml"),
            "name: full-stack\nworktree_count: 3\n",
        )
        .unwrap();

        let def = find_swarm_def(root, "full-stack");
        assert!(def.is_some());
        assert_eq!(def.unwrap().name, "full-stack");
    }

    #[test]
    fn given_no_swarm_defs_should_return_empty_list() {
        let tmp = TempDir::new().unwrap();
        let defs = list_swarm_defs(tmp.path());
        assert!(defs.is_empty());
    }

    #[test]
    fn given_swarm_def_should_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("swarms");
        let def = SwarmDef {
            name: "test-swarm".to_string(),
            worktree_count: 2,
            worktree_template: "feature".to_string(),
            roster: vec![SwarmRosterEntry {
                agent_def: "reviewer".to_string(),
                role: "review".to_string(),
                quantity: 1,
            }],
            include_terminal: true,
            scope: "local".to_string(),
        };

        save_swarm_def(&dir, &def).unwrap();

        let path = dir.join("test-swarm.yaml");
        assert!(path.is_file());

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: SwarmDef = serde_yml::from_str(&content).unwrap();
        assert_eq!(loaded.name, "test-swarm");
        assert_eq!(loaded.worktree_count, 2);
        assert_eq!(loaded.roster.len(), 1);
        assert!(loaded.include_terminal);
    }

    #[test]
    fn given_existing_swarm_def_should_delete_and_return_true() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("swarms");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("full-stack.yaml"), "name: full-stack\n").unwrap();

        let deleted = delete_swarm_def(&dir, "full-stack").unwrap();
        assert!(deleted);
        assert!(!dir.join("full-stack.yaml").exists());
    }

    #[test]
    fn given_nonexistent_swarm_def_should_return_false() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("swarms");
        std::fs::create_dir_all(&dir).unwrap();

        let deleted = delete_swarm_def(&dir, "nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn given_duplicate_name_in_local_and_global_should_prefer_local() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create local swarm def
        let local_dir = paths::swarms_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("full-stack.yaml"),
            "name: full-stack\nworktree_count: 3\n",
        )
        .unwrap();

        // find_swarm_def checks local first
        let def = find_swarm_def(root, "full-stack").unwrap();
        assert_eq!(def.scope, "local");
    }

    #[test]
    fn given_path_traversal_name_should_return_none() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::swarms_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(local_dir.join("legit.yaml"), "name: legit\n").unwrap();

        // Path traversal attempts should return None
        assert!(find_swarm_def(root, "../../etc/passwd").is_none());
        assert!(find_swarm_def(root, "../evil").is_none());
        assert!(find_swarm_def(root, "foo/bar").is_none());
        assert!(find_swarm_def(root, ".hidden").is_none());
    }
}
