use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub name: String,
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub inline_prompt: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// "local" or "global" — set at load time
    #[serde(skip)]
    pub scope: String,
    #[serde(default = "default_true")]
    pub available_in_command_dialog: bool,
    #[serde(default)]
    pub icon: Option<String>,
}

fn default_agent_type() -> String {
    "claude".to_string()
}

fn default_true() -> bool {
    true
}

/// Scan both local and global agent definition directories. Local defs take priority.
pub fn list_agent_defs(project_root: &Path) -> Vec<AgentDef> {
    let mut seen = HashMap::new();
    let mut result = Vec::new();

    // Local first
    let local_dir = paths::agents_dir(project_root);
    if local_dir.is_dir() {
        for def in scan_dir(&local_dir, "local") {
            seen.insert(def.name.clone(), result.len());
            result.push(def);
        }
    }

    // Global second (skip duplicates)
    if let Ok(global_dir) = paths::global_agents_dir() {
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

/// Find an agent definition by name. Checks local first, then global.
pub fn find_agent_def(project_root: &Path, name: &str) -> Option<AgentDef> {
    let local_dir = paths::agents_dir(project_root);
    if local_dir.is_dir() {
        if let Some(def) = find_in_dir(&local_dir, name, "local") {
            return Some(def);
        }
    }
    if let Ok(global_dir) = paths::global_agents_dir() {
        if global_dir.is_dir() {
            if let Some(def) = find_in_dir(&global_dir, name, "global") {
                return Some(def);
            }
        }
    }
    None
}

/// Save an agent definition as a YAML file. Creates the directory if needed.
pub fn save_agent_def(dir: &Path, def: &AgentDef) -> Result<(), std::io::Error> {
    crate::validation::validate_name(&def.name)?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.yaml", def.name));
    let yaml = serde_yml::to_string(def).map_err(std::io::Error::other)?;
    std::fs::write(path, yaml)
}

/// Delete an agent definition file. Returns true if the file existed.
pub fn delete_agent_def(dir: &Path, name: &str) -> Result<bool, std::io::Error> {
    crate::validation::validate_name(name)?;
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        std::fs::remove_file(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn scan_dir(dir: &Path, scope: &str) -> Vec<AgentDef> {
    let mut defs = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return defs,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                match serde_yml::from_str::<AgentDef>(&content) {
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

fn find_in_dir(dir: &Path, name: &str, scope: &str) -> Option<AgentDef> {
    // Try exact file name first
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut def) = serde_yml::from_str::<AgentDef>(&content) {
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
    fn given_agent_def_yaml_should_deserialize() {
        let yaml = "name: reviewer\nagent_type: claude\ntags:\n  - review\n  - code\n";
        let def: AgentDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.name, "reviewer");
        assert_eq!(def.agent_type, "claude");
        assert_eq!(def.tags, vec!["review", "code"]);
        assert!(def.available_in_command_dialog);
        assert_eq!(def.scope, ""); // skip_deserializing
    }

    #[test]
    fn given_minimal_yaml_should_use_defaults() {
        let yaml = "name: basic\n";
        let def: AgentDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.agent_type, "claude");
        assert!(def.template.is_none());
        assert!(def.inline_prompt.is_none());
        assert!(def.tags.is_empty());
        assert!(def.available_in_command_dialog);
        assert!(def.icon.is_none());
    }

    #[test]
    fn given_local_and_global_agent_defs_should_list_local_first() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::agents_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("reviewer.yaml"),
            "name: reviewer\nagent_type: claude\n",
        )
        .unwrap();
        std::fs::write(
            local_dir.join("builder.yaml"),
            "name: builder\nagent_type: codex\n",
        )
        .unwrap();

        let defs = list_agent_defs(root);
        assert_eq!(defs.len(), 2);
        // Sorted alphabetically
        assert_eq!(defs[0].name, "builder");
        assert_eq!(defs[1].name, "reviewer");
        assert_eq!(defs[0].scope, "local");
    }

    #[test]
    fn given_agent_def_name_should_find_by_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::agents_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("reviewer.yaml"),
            "name: reviewer\nagent_type: claude\n",
        )
        .unwrap();

        let def = find_agent_def(root, "reviewer");
        assert!(def.is_some());
        assert_eq!(def.unwrap().name, "reviewer");
    }

    #[test]
    fn given_no_agent_defs_should_return_empty_list() {
        let tmp = TempDir::new().unwrap();
        let defs = list_agent_defs(tmp.path());
        assert!(defs.is_empty());
    }

    #[test]
    fn given_agent_def_should_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("agents");
        let def = AgentDef {
            name: "tester".to_string(),
            agent_type: "claude".to_string(),
            template: Some("test-template".to_string()),
            inline_prompt: None,
            tags: vec!["test".to_string()],
            scope: "local".to_string(),
            available_in_command_dialog: true,
            icon: None,
        };

        save_agent_def(&dir, &def).unwrap();

        let path = dir.join("tester.yaml");
        assert!(path.is_file());

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: AgentDef = serde_yml::from_str(&content).unwrap();
        assert_eq!(loaded.name, "tester");
        assert_eq!(loaded.template, Some("test-template".to_string()));
        assert_eq!(loaded.tags, vec!["test"]);
    }

    #[test]
    fn given_existing_agent_def_should_delete_and_return_true() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("reviewer.yaml"), "name: reviewer\n").unwrap();

        let deleted = delete_agent_def(&dir, "reviewer").unwrap();
        assert!(deleted);
        assert!(!dir.join("reviewer.yaml").exists());
    }

    #[test]
    fn given_nonexistent_agent_def_should_return_false() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("agents");
        std::fs::create_dir_all(&dir).unwrap();

        let deleted = delete_agent_def(&dir, "nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn given_duplicate_name_in_local_and_global_should_prefer_local() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create local agent def
        let local_dir = paths::agents_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("reviewer.yaml"),
            "name: reviewer\nagent_type: claude\n",
        )
        .unwrap();

        // find_agent_def checks local first
        let def = find_agent_def(root, "reviewer").unwrap();
        assert_eq!(def.scope, "local");
    }
}
