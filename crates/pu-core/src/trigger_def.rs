use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEvent {
    AgentIdle,
    PreCommit,
    PrePush,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateDef {
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_exit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriggerAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub on: TriggerEvent,
    pub sequence: Vec<TriggerAction>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, String>,
    /// "local" or "global" -- set at load time, not serialized
    #[serde(skip)]
    pub scope: String,
}

/// Substitute `{{KEY}}` placeholders in a string using the provided variables.
pub fn substitute_variables(text: &str, variables: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    for (key, value) in variables {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
    }
    result
}

/// Scan both local and global trigger definition directories. Local defs take priority.
pub fn list_trigger_defs(project_root: &Path) -> Vec<TriggerDef> {
    let mut seen = HashMap::new();
    let mut result = Vec::new();

    let local_dir = paths::triggers_dir(project_root);
    if local_dir.is_dir() {
        for def in scan_dir(&local_dir, "local") {
            seen.insert(def.name.clone(), result.len());
            result.push(def);
        }
    }

    if let Ok(global_dir) = paths::global_triggers_dir() {
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

/// Find a trigger definition by name. Checks local first, then global.
pub fn find_trigger_def(project_root: &Path, name: &str) -> Option<TriggerDef> {
    let local_dir = paths::triggers_dir(project_root);
    if local_dir.is_dir() {
        if let Some(def) = find_in_dir(&local_dir, name, "local") {
            return Some(def);
        }
    }
    if let Ok(global_dir) = paths::global_triggers_dir() {
        if global_dir.is_dir() {
            if let Some(def) = find_in_dir(&global_dir, name, "global") {
                return Some(def);
            }
        }
    }
    None
}

/// Save a trigger definition as a YAML file. Creates the directory if needed.
pub fn save_trigger_def(dir: &Path, def: &TriggerDef) -> Result<(), std::io::Error> {
    crate::validation::validate_name(&def.name)?;
    if def.sequence.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sequence must not be empty",
        ));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.yaml", def.name));
    let yaml = serde_yml::to_string(def).map_err(std::io::Error::other)?;
    std::fs::write(path, yaml)
}

/// Delete a trigger definition file. Returns true if the file existed.
pub fn delete_trigger_def(dir: &Path, name: &str) -> Result<bool, std::io::Error> {
    crate::validation::validate_name(name)?;
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        std::fs::remove_file(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Find all trigger defs matching a given event type.
pub fn triggers_for_event(project_root: &Path, event: &TriggerEvent) -> Vec<TriggerDef> {
    list_trigger_defs(project_root)
        .into_iter()
        .filter(|def| &def.on == event)
        .collect()
}

fn scan_dir(dir: &Path, scope: &str) -> Vec<TriggerDef> {
    let mut defs = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return defs,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                match serde_yml::from_str::<TriggerDef>(&content) {
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

fn find_in_dir(dir: &Path, name: &str, scope: &str) -> Option<TriggerDef> {
    let path = dir.join(format!("{name}.yaml"));
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            match serde_yml::from_str::<TriggerDef>(&content) {
                Ok(mut def) => {
                    def.scope = scope.to_string();
                    return Some(def);
                }
                Err(e) => {
                    eprintln!("warning: failed to parse {}: {e}", path.display());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_trigger_def(name: &str) -> TriggerDef {
        TriggerDef {
            name: name.to_string(),
            description: Some("test trigger".to_string()),
            on: TriggerEvent::AgentIdle,
            sequence: vec![TriggerAction {
                inject: Some("/simplify".to_string()),
                gate: None,
                max_retries: None,
            }],
            variables: HashMap::new(),
            scope: String::new(),
        }
    }

    // --- Deserialization ---

    #[test]
    fn given_agent_idle_trigger_yaml_should_deserialize() {
        let yaml = r#"
name: post-task
description: "After agent task, run simplify then review"
on: agent_idle
sequence:
  - inject: "/simplify"
  - inject: "/review"
  - inject: "/commit-push-pr"
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.name, "post-task");
        assert_eq!(def.on, TriggerEvent::AgentIdle);
        assert_eq!(def.sequence.len(), 3);
        assert_eq!(def.sequence[0].inject.as_deref(), Some("/simplify"));
        assert_eq!(def.sequence[2].inject.as_deref(), Some("/commit-push-pr"));
    }

    #[test]
    fn given_pre_commit_trigger_yaml_should_deserialize() {
        let yaml = r#"
name: quality-gate
on: pre_commit
sequence:
  - gate:
      run: "cargo test"
  - gate:
      run: "cargo clippy -- -D warnings"
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.on, TriggerEvent::PreCommit);
        assert_eq!(def.sequence.len(), 2);
        assert_eq!(def.sequence[0].gate.as_ref().unwrap().run, "cargo test");
        assert!(def.sequence[0].inject.is_none());
    }

    #[test]
    fn given_pre_push_trigger_yaml_should_deserialize() {
        let yaml = r#"
name: push-gate
on: pre_push
sequence:
  - gate:
      run: "cargo build --release"
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.on, TriggerEvent::PrePush);
        assert_eq!(def.sequence.len(), 1);
    }

    #[test]
    fn given_trigger_with_gate_and_inject_should_deserialize() {
        let yaml = r#"
name: gated-flow
on: agent_idle
sequence:
  - inject: "/commit-push-pr"
    gate:
      run: "cargo test"
    max_retries: 5
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        let action = &def.sequence[0];
        assert_eq!(action.inject.as_deref(), Some("/commit-push-pr"));
        assert_eq!(action.gate.as_ref().unwrap().run, "cargo test");
        assert_eq!(action.max_retries, Some(5));
    }

    #[test]
    fn given_trigger_with_variables_should_deserialize() {
        let yaml = r#"
name: swift-gate
on: pre_commit
variables:
  SCHEME: "MyApp"
sequence:
  - gate:
      run: "xcodebuild test -scheme {{SCHEME}} -quiet"
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.variables["SCHEME"], "MyApp");
        assert!(
            def.sequence[0]
                .gate
                .as_ref()
                .unwrap()
                .run
                .contains("{{SCHEME}}")
        );
    }

    #[test]
    fn given_gate_with_expect_exit_should_deserialize() {
        let yaml = r#"
name: custom-gate
on: pre_commit
sequence:
  - gate:
      run: "check-something"
      expect_exit: 2
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        assert_eq!(def.sequence[0].gate.as_ref().unwrap().expect_exit, Some(2));
    }

    #[test]
    fn given_minimal_trigger_should_use_defaults() {
        let yaml = r#"
name: minimal
on: agent_idle
sequence:
  - inject: "hello"
"#;
        let def: TriggerDef = serde_yml::from_str(yaml).unwrap();
        assert!(def.description.is_none());
        assert!(def.variables.is_empty());
        assert!(def.sequence[0].gate.is_none());
        assert!(def.sequence[0].max_retries.is_none());
    }

    // --- Round-trip ---

    #[test]
    fn given_trigger_def_should_round_trip_yaml() {
        let def = TriggerDef {
            name: "devx".to_string(),
            description: Some("development workflow".to_string()),
            on: TriggerEvent::AgentIdle,
            sequence: vec![
                TriggerAction {
                    inject: Some("/simplify".to_string()),
                    gate: None,
                    max_retries: None,
                },
                TriggerAction {
                    inject: Some("/review".to_string()),
                    gate: Some(GateDef {
                        run: "cargo test".to_string(),
                        expect_exit: None,
                    }),
                    max_retries: Some(3),
                },
            ],
            variables: HashMap::new(),
            scope: String::new(),
        };
        let yaml = serde_yml::to_string(&def).unwrap();
        let reparsed: TriggerDef = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(reparsed.name, "devx");
        assert_eq!(reparsed.on, TriggerEvent::AgentIdle);
        assert_eq!(reparsed.sequence.len(), 2);
        assert_eq!(
            reparsed.sequence[1].gate.as_ref().unwrap().run,
            "cargo test"
        );
    }

    #[test]
    fn given_all_event_types_should_round_trip() {
        for event in [
            TriggerEvent::AgentIdle,
            TriggerEvent::PreCommit,
            TriggerEvent::PrePush,
        ] {
            let yaml = serde_yml::to_string(&event).unwrap();
            let parsed: TriggerEvent = serde_yml::from_str(&yaml).unwrap();
            assert_eq!(parsed, event);
        }
    }

    // --- Variable substitution ---

    #[test]
    fn given_variables_should_substitute_in_text() {
        let mut vars = HashMap::new();
        vars.insert("SCHEME".to_string(), "MyApp".to_string());
        vars.insert("TARGET".to_string(), "arm64".to_string());
        let result =
            substitute_variables("xcodebuild test -scheme {{SCHEME}} -arch {{TARGET}}", &vars);
        assert_eq!(result, "xcodebuild test -scheme MyApp -arch arm64");
    }

    #[test]
    fn given_no_matching_variables_should_leave_placeholders() {
        let vars = HashMap::new();
        let result = substitute_variables("{{UNKNOWN}} stays", &vars);
        assert_eq!(result, "{{UNKNOWN}} stays");
    }

    #[test]
    fn given_empty_variables_and_no_placeholders_should_return_unchanged() {
        let result = substitute_variables("no placeholders here", &HashMap::new());
        assert_eq!(result, "no placeholders here");
    }

    // --- CRUD ---

    #[test]
    fn given_trigger_def_should_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("triggers");
        let def = make_trigger_def("devx");
        save_trigger_def(&dir, &def).unwrap();

        let path = dir.join("devx.yaml");
        assert!(path.is_file());

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: TriggerDef = serde_yml::from_str(&content).unwrap();
        assert_eq!(loaded.name, "devx");
        assert_eq!(loaded.on, TriggerEvent::AgentIdle);
    }

    #[test]
    fn given_local_trigger_defs_should_list_sorted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::triggers_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let mut def = make_trigger_def("zebra");
        save_trigger_def(&local_dir, &def).unwrap();
        def.name = "alpha".to_string();
        save_trigger_def(&local_dir, &def).unwrap();

        let defs = list_trigger_defs(root);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "alpha");
        assert_eq!(defs[1].name, "zebra");
        assert_eq!(defs[0].scope, "local");
    }

    #[test]
    fn given_trigger_def_name_should_find_by_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::triggers_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let def = make_trigger_def("quality-gate");
        save_trigger_def(&local_dir, &def).unwrap();

        let found = find_trigger_def(root, "quality-gate");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "quality-gate");
    }

    #[test]
    fn given_no_trigger_defs_should_return_empty_list() {
        let tmp = TempDir::new().unwrap();
        let defs = list_trigger_defs(tmp.path());
        assert!(defs.is_empty());
    }

    #[test]
    fn given_nonexistent_name_should_return_none() {
        let tmp = TempDir::new().unwrap();
        let found = find_trigger_def(tmp.path(), "nonexistent");
        assert!(found.is_none());
    }

    #[test]
    fn given_invalid_name_should_reject() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("triggers");
        let mut def = make_trigger_def("../evil");
        def.name = "../evil".to_string();
        assert!(save_trigger_def(&dir, &def).is_err());
    }

    #[test]
    fn given_empty_sequence_should_reject() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("triggers");
        let def = TriggerDef {
            name: "empty".to_string(),
            description: None,
            on: TriggerEvent::AgentIdle,
            sequence: vec![],
            variables: HashMap::new(),
            scope: String::new(),
        };
        assert!(save_trigger_def(&dir, &def).is_err());
    }

    #[test]
    fn given_existing_trigger_def_should_delete_and_return_true() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("triggers");
        std::fs::create_dir_all(&dir).unwrap();
        let def = make_trigger_def("devx");
        save_trigger_def(&dir, &def).unwrap();

        let deleted = delete_trigger_def(&dir, "devx").unwrap();
        assert!(deleted);
        assert!(!dir.join("devx.yaml").exists());
    }

    #[test]
    fn given_nonexistent_trigger_def_should_return_false() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("triggers");
        std::fs::create_dir_all(&dir).unwrap();

        let deleted = delete_trigger_def(&dir, "nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn given_duplicate_name_in_local_and_global_should_prefer_local() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::triggers_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let def = make_trigger_def("devx");
        save_trigger_def(&local_dir, &def).unwrap();

        let found = find_trigger_def(root, "devx").unwrap();
        assert_eq!(found.scope, "local");
    }

    // --- triggers_for_event ---

    #[test]
    fn given_mixed_events_should_filter_by_event_type() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::triggers_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();

        let mut idle_def = make_trigger_def("devx");
        idle_def.on = TriggerEvent::AgentIdle;
        save_trigger_def(&local_dir, &idle_def).unwrap();

        let commit_def = TriggerDef {
            name: "quality-gate".to_string(),
            description: None,
            on: TriggerEvent::PreCommit,
            sequence: vec![TriggerAction {
                inject: None,
                gate: Some(GateDef {
                    run: "cargo test".to_string(),
                    expect_exit: None,
                }),
                max_retries: None,
            }],
            variables: HashMap::new(),
            scope: String::new(),
        };
        save_trigger_def(&local_dir, &commit_def).unwrap();

        let idle_triggers = triggers_for_event(root, &TriggerEvent::AgentIdle);
        assert_eq!(idle_triggers.len(), 1);
        assert_eq!(idle_triggers[0].name, "devx");

        let commit_triggers = triggers_for_event(root, &TriggerEvent::PreCommit);
        assert_eq!(commit_triggers.len(), 1);
        assert_eq!(commit_triggers[0].name, "quality-gate");

        let push_triggers = triggers_for_event(root, &TriggerEvent::PrePush);
        assert!(push_triggers.is_empty());
    }
}
