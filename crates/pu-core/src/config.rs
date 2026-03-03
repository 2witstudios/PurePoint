use std::path::Path;

use crate::error::PuError;
use crate::paths;
use crate::types::{AgentConfig, Config};

pub fn load_config(project_root: &Path) -> Config {
    load_config_result(project_root).unwrap_or_default()
}

pub fn load_config_strict(project_root: &Path) -> Result<Config, PuError> {
    load_config_result(project_root)
}

fn load_config_result(project_root: &Path) -> Result<Config, PuError> {
    let path = paths::config_path(project_root);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let mut config: Config = serde_yml::from_str(&content)?;
            // Fill in any agents missing from file with code defaults
            for (name, agent) in crate::types::default_agents() {
                config.agents.entry(name).or_insert(agent);
            }
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(PuError::Io(e)),
    }
}

pub fn resolve_agent<'a>(config: &'a Config, name: &str) -> Option<&'a AgentConfig> {
    config.agents.get(name)
}

pub fn write_default_config(project_root: &Path) -> Result<(), PuError> {
    let path = paths::config_path(project_root);
    // Only write user-level settings. Agent defaults come from code.
    let yaml = "defaultAgent: claude\nenvFiles:\n- .env\n- .env.local\n";
    std::fs::write(&path, yaml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn given_config_yaml_should_parse_correctly() {
        let yaml = r#"
defaultAgent: codex
agents:
  codex:
    name: codex
    command: "codex --yolo"
    promptFlag: "--prompt"
    interactive: true
envFiles: [".env"]
"#;
        let config: crate::types::Config = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.default_agent, "codex");
        assert!(config.agents.contains_key("codex"));
        let codex = &config.agents["codex"];
        assert_eq!(codex.prompt_flag.as_deref(), Some("--prompt"));
        assert_eq!(config.env_files, vec![".env"]);
    }

    #[test]
    fn given_missing_config_file_should_return_defaults() {
        let tmp = TempDir::new().unwrap();
        let config = load_config(tmp.path());
        assert_eq!(config.default_agent, "claude");
        assert!(config.agents.contains_key("claude"));
    }

    #[test]
    fn given_existing_config_file_should_load_it() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = "defaultAgent: codex\nagents:\n  codex:\n    name: codex\n    command: codex\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        assert_eq!(config.default_agent, "codex");
    }

    #[test]
    fn given_config_should_resolve_agent_by_name() {
        let config = crate::types::Config::default();
        let agent = resolve_agent(&config, "claude");
        assert!(agent.is_some());
        assert_eq!(agent.unwrap().command, "claude");
    }

    #[test]
    fn given_config_should_resolve_default_agent_when_none() {
        let config = crate::types::Config::default();
        let agent = resolve_agent(&config, &config.default_agent);
        assert!(agent.is_some());
    }

    #[test]
    fn given_config_should_return_none_for_unknown_agent() {
        let config = crate::types::Config::default();
        assert!(resolve_agent(&config, "nonexistent").is_none());
    }

    #[test]
    fn given_write_default_config_should_create_minimal_yaml() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();

        write_default_config(root).unwrap();

        let path = crate::paths::config_path(root);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("defaultAgent: claude"));
        assert!(content.contains(".env"));
        // Should NOT contain agents section — defaults come from code
        assert!(!content.contains("agents:"));
    }

    #[test]
    fn given_config_with_one_agent_should_merge_code_defaults() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        // Config file only defines codex — claude, opencode, terminal should be filled from defaults
        let yaml = "defaultAgent: codex\nagents:\n  codex:\n    name: codex\n    command: \"codex --yolo\"\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        assert_eq!(config.default_agent, "codex");
        // codex should keep file value
        assert_eq!(config.agents["codex"].command, "codex --yolo");
        // claude, opencode, terminal should come from code defaults
        assert!(config.agents.contains_key("claude"));
        assert_eq!(config.agents["claude"].command, "claude");
        assert!(config.agents.contains_key("opencode"));
        assert_eq!(config.agents["opencode"].command, "opencode");
        assert!(config.agents.contains_key("terminal"));
        assert_eq!(config.agents["terminal"].command, "shell");
    }

    #[test]
    fn given_config_without_agents_key_should_get_all_defaults() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = "defaultAgent: claude\nenvFiles:\n- .env\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        assert!(config.agents.contains_key("claude"));
        assert!(config.agents.contains_key("codex"));
        assert!(config.agents.contains_key("opencode"));
        assert!(config.agents.contains_key("terminal"));
    }

    #[test]
    fn given_malformed_yaml_should_error_in_strict_mode() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        std::fs::write(crate::paths::config_path(root), "{{invalid yaml").unwrap();

        let result = load_config_strict(root);
        assert!(result.is_err());
    }

    #[test]
    fn given_missing_config_strict_should_return_defaults() {
        let tmp = TempDir::new().unwrap();
        let config = load_config_strict(tmp.path()).unwrap();
        assert_eq!(config.default_agent, "claude");
    }
}
