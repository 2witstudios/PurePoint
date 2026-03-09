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

/// Update a specific agent's launch_args in the project config.
/// Loads existing config, modifies the named agent, writes back to YAML.
/// Returns the updated (merged) config.
///
/// Only known agent types (from `default_agents()`) are accepted.
/// Unknown agent names return an error to prevent config pollution.
pub fn update_agent_config(
    project_root: &Path,
    agent_name: &str,
    launch_args: Option<Vec<String>>,
) -> Result<Config, PuError> {
    let defaults = crate::types::default_agents();
    if !defaults.contains_key(agent_name) {
        return Err(PuError::InvalidArgument(format!(
            "unknown agent type: {agent_name}"
        )));
    }
    let path = paths::config_path(project_root);

    // Load raw config from file (without merging code defaults)
    let mut raw_config: Config = match std::fs::read_to_string(&path) {
        Ok(content) => serde_yml::from_str(&content)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config {
            default_agent: "claude".into(),
            agents: indexmap::IndexMap::new(),
            env_files: vec![".env".into(), ".env.local".into()],
        },
        Err(e) => return Err(PuError::Io(e)),
    };

    // Ensure the agent entry exists
    let agent = raw_config
        .agents
        .entry(agent_name.to_string())
        .or_insert_with(|| {
            let defaults = crate::types::default_agents();
            defaults.get(agent_name).cloned().unwrap_or(AgentConfig {
                name: agent_name.to_string(),
                command: agent_name.to_string(),
                prompt_flag: None,
                interactive: true,
                launch_args: None,
            })
        });

    agent.launch_args = launch_args;

    // Write back to YAML
    let yaml = serde_yml::to_string(&raw_config)?;
    std::fs::write(&path, yaml)?;

    // Return fully merged config (with code defaults filled in)
    load_config_strict(project_root)
}

pub fn write_default_config(project_root: &Path) -> Result<(), PuError> {
    let path = paths::config_path(project_root);
    // Only write user-level settings. Agent defaults come from code.
    // Commented-out agents section documents available flags per agent type.
    let yaml = r#"defaultAgent: claude
envFiles:
  - .env
  - .env.local

# Agent launch configuration — uncomment and customize as needed.
# Agents not listed here use built-in defaults.
# agents:
#   claude:
#     name: claude
#     command: claude
#     launchArgs:                  # Default: ["--dangerously-skip-permissions"]
#       - "--dangerously-skip-permissions"
#       # --permission-mode <default|acceptEdits|plan|bypassPermissions>
#       # --model <sonnet|opus|haiku>
#       # --effort <low|medium|high>
#       # --allowedTools <tools...>
#       # --append-system-prompt <prompt>
#       # --max-budget-usd <amount>
#   codex:
#     name: codex
#     command: codex
#     launchArgs:                  # Default: ["--full-auto"]
#       - "--full-auto"
#       # -a <untrusted|on-request|never>  (approval mode)
#       # --model <model>
#   opencode:
#     name: opencode
#     command: opencode
#     launchArgs: []               # Default: [] (auto-approves in run mode)
#       # --model <provider/model>
#       # --variant <effort>
"#;
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
        // agents section is only in comments — no uncommented "agents:" key
        // (parser fills defaults but that's from code, not from the file)
        assert!(
            !content
                .lines()
                .any(|l| !l.starts_with('#') && l.contains("agents:"))
        );
        // Comments should document key flags for discoverability
        assert!(content.contains("--dangerously-skip-permissions"));
        assert!(content.contains("--full-auto"));
        assert!(content.contains("--permission-mode"));
        assert!(content.contains("--model"));
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

    #[test]
    fn given_config_with_launch_args_should_load_them() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = r#"
defaultAgent: claude
agents:
  claude:
    name: claude
    command: claude
    launchArgs:
      - "--verbose"
"#;
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        let claude = &config.agents["claude"];
        assert_eq!(claude.launch_args, Some(vec!["--verbose".to_string()]));
    }

    #[test]
    fn given_config_with_empty_launch_args_should_disable_auto_mode() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = r#"
defaultAgent: claude
agents:
  claude:
    name: claude
    command: claude
    launchArgs: []
"#;
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        let claude = &config.agents["claude"];
        assert_eq!(claude.launch_args, Some(vec![]));
        // resolved_launch_args should return empty when explicitly set
        let args = crate::types::resolved_launch_args("claude", claude.launch_args.as_deref());
        assert!(args.is_empty());
    }

    #[test]
    fn given_config_without_launch_args_should_use_defaults() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml =
            "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        let claude = &config.agents["claude"];
        assert!(claude.launch_args.is_none());
        // resolved_launch_args should return defaults when not set
        let args = crate::types::resolved_launch_args("claude", claude.launch_args.as_deref());
        assert_eq!(args, vec!["--dangerously-skip-permissions"]);
    }

    #[test]
    fn given_update_agent_config_should_write_launch_args() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml =
            "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = update_agent_config(
            root,
            "claude",
            Some(vec![
                "--verbose".to_string(),
                "--model".to_string(),
                "opus".to_string(),
            ]),
        )
        .unwrap();

        let claude = &config.agents["claude"];
        assert_eq!(
            claude.launch_args,
            Some(vec![
                "--verbose".to_string(),
                "--model".to_string(),
                "opus".to_string()
            ])
        );

        // Re-read from disk to verify persistence
        let reloaded = load_config(root);
        assert_eq!(
            reloaded.agents["claude"].launch_args,
            Some(vec![
                "--verbose".to_string(),
                "--model".to_string(),
                "opus".to_string()
            ])
        );
    }

    #[test]
    fn given_update_agent_config_with_none_should_reset_to_defaults() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n    launchArgs:\n      - '--verbose'\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = update_agent_config(root, "claude", None).unwrap();
        let claude = &config.agents["claude"];
        assert!(claude.launch_args.is_none());
    }

    #[test]
    fn given_update_agent_config_should_preserve_other_agents() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n  codex:\n    name: codex\n    command: codex\n    launchArgs:\n      - '--full-auto'\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config =
            update_agent_config(root, "claude", Some(vec!["--verbose".to_string()])).unwrap();

        // Claude should have new args
        assert_eq!(
            config.agents["claude"].launch_args,
            Some(vec!["--verbose".to_string()])
        );
        // Codex should be preserved
        assert_eq!(
            config.agents["codex"].launch_args,
            Some(vec!["--full-auto".to_string()])
        );
    }

    #[test]
    fn given_update_agent_config_with_unknown_agent_should_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();

        let result = update_agent_config(root, "unknown-agent", Some(vec!["--verbose".into()]));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), "INVALID_ARGUMENT");
    }

    #[test]
    fn given_update_agent_config_with_no_config_file_should_create_one() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();

        let config = update_agent_config(
            root,
            "claude",
            Some(vec!["--permission-mode".to_string(), "plan".to_string()]),
        )
        .unwrap();

        assert!(crate::paths::config_path(root).exists());
        assert_eq!(
            config.agents["claude"].launch_args,
            Some(vec!["--permission-mode".to_string(), "plan".to_string()])
        );
    }
}
