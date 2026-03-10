use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub prompt_flag: Option<String>,
    #[serde(default = "crate::serde_defaults::default_true")]
    pub interactive: bool,
    #[serde(default)]
    pub launch_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "crate::serde_defaults::default_agent")]
    pub default_agent: String,
    #[serde(default = "default_agents")]
    pub agents: IndexMap<String, AgentConfig>,
    #[serde(default = "default_env_files")]
    pub env_files: Vec<String>,
}

fn default_env_files() -> Vec<String> {
    vec![".env".to_string(), ".env.local".to_string()]
}

pub fn default_agents() -> IndexMap<String, AgentConfig> {
    // (name, command) — command "shell" is a sentinel the engine resolves to $SHELL
    [
        ("claude", "claude"),
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("terminal", "shell"),
    ]
    .into_iter()
    .map(|(name, cmd)| {
        (
            name.to_string(),
            AgentConfig {
                name: name.to_string(),
                command: cmd.to_string(),
                prompt_flag: None,
                interactive: true,
                launch_args: None,
            },
        )
    })
    .collect()
}

/// Resolve launch args for an agent type.
/// - `None` → use built-in defaults per agent type
/// - `Some([])` → no launch args (user explicitly disabled auto-mode)
/// - `Some([...])` → use exactly these args
pub fn resolved_launch_args(agent_type: &str, launch_args: Option<&[String]>) -> Vec<String> {
    match launch_args {
        Some(args) => args.to_vec(),
        None => match agent_type {
            "claude" => vec!["--dangerously-skip-permissions".into()],
            "codex" => vec!["--full-auto".into()],
            _ => vec![],
        },
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_agent: crate::serde_defaults::default_agent(),
            agents: default_agents(),
            env_files: default_env_files(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_default_config_should_have_claude_agent() {
        let config = Config::default();
        assert_eq!(config.default_agent, "claude");
        assert!(config.agents.contains_key("claude"));
        let claude = &config.agents["claude"];
        assert_eq!(claude.command, "claude");
        assert!(claude.prompt_flag.is_none());
        assert!(claude.interactive);
    }

    #[test]
    fn given_default_config_should_have_codex_and_opencode_agents() {
        let config = Config::default();
        assert!(config.agents.contains_key("codex"));
        assert_eq!(config.agents["codex"].command, "codex");
        assert!(config.agents.contains_key("opencode"));
        assert_eq!(config.agents["opencode"].command, "opencode");
    }

    #[test]
    fn given_default_config_should_have_terminal_agent() {
        let config = Config::default();
        assert!(config.agents.contains_key("terminal"));
        let terminal = &config.agents["terminal"];
        assert_eq!(terminal.command, "shell");
        assert!(terminal.prompt_flag.is_none());
        assert!(terminal.interactive);
    }

    #[test]
    fn given_config_should_round_trip_yaml() {
        let config = Config::default();
        let yaml = serde_yml::to_string(&config).unwrap();
        let parsed: Config = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.default_agent, "claude");
        assert!(parsed.agents.contains_key("claude"));
    }

    // --- launch_args ---

    #[test]
    fn given_agent_config_without_launch_args_should_default_to_none() {
        let yaml = r#"
name: claude
command: claude
"#;
        let config: AgentConfig = serde_yml::from_str(yaml).unwrap();
        assert!(config.launch_args.is_none());
    }

    #[test]
    fn given_agent_config_with_empty_launch_args_should_deserialize_as_empty_vec() {
        let yaml = r#"
name: claude
command: claude
launchArgs: []
"#;
        let config: AgentConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.launch_args, Some(vec![]));
    }

    #[test]
    fn given_agent_config_with_launch_args_should_deserialize_flags() {
        let yaml = r#"
name: claude
command: claude
launchArgs:
  - "--dangerously-skip-permissions"
  - "--verbose"
"#;
        let config: AgentConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(
            config.launch_args,
            Some(vec![
                "--dangerously-skip-permissions".to_string(),
                "--verbose".to_string()
            ])
        );
    }

    #[test]
    fn given_agent_config_with_launch_args_should_round_trip_yaml() {
        let config = AgentConfig {
            name: "claude".into(),
            command: "claude".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: Some(vec!["--dangerously-skip-permissions".into()]),
        };
        let yaml = serde_yml::to_string(&config).unwrap();
        let parsed: AgentConfig = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.launch_args, config.launch_args);
    }

    #[test]
    fn given_claude_agent_type_should_resolve_default_launch_args() {
        // When launch_args is None, claude should get --dangerously-skip-permissions
        let args = resolved_launch_args("claude", None);
        assert_eq!(args, vec!["--dangerously-skip-permissions"]);
    }

    #[test]
    fn given_codex_agent_type_should_resolve_default_launch_args() {
        let args = resolved_launch_args("codex", None);
        assert_eq!(args, vec!["--full-auto"]);
    }

    #[test]
    fn given_opencode_agent_type_should_resolve_empty_default_launch_args() {
        let args = resolved_launch_args("opencode", None);
        assert!(args.is_empty());
    }

    #[test]
    fn given_terminal_agent_type_should_resolve_empty_default_launch_args() {
        let args = resolved_launch_args("terminal", None);
        assert!(args.is_empty());
    }

    #[test]
    fn given_explicit_empty_launch_args_should_override_defaults() {
        // User explicitly sets launchArgs: [] to disable auto-mode
        let args = resolved_launch_args("claude", Some(&[]));
        assert!(args.is_empty());
    }

    #[test]
    fn given_explicit_launch_args_should_override_defaults() {
        let custom = vec!["--verbose".to_string()];
        let args = resolved_launch_args("claude", Some(&custom));
        assert_eq!(args, vec!["--verbose"]);
    }
}
