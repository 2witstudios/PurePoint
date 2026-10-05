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

/// Codex's auto-mode flags. Codex removed the `--full-auto` shortcut; these are the
/// long-form equivalents (workspace-writable sandbox, model decides when to ask).
pub const CODEX_AUTO_ARGS: [&str; 4] = [
    "--sandbox",
    "workspace-write",
    "--ask-for-approval",
    "on-request",
];

/// Rewrite launch args that reference flags the underlying CLI no longer accepts.
/// Configs written before Codex dropped `--full-auto` would otherwise fail to launch.
fn migrate_legacy_args(agent_type: &str, args: &[String]) -> Vec<String> {
    if agent_type != "codex" {
        return args.to_vec();
    }
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        // `--full-auto` and the even older `--approval-mode=full-auto` spelling.
        if arg == "--full-auto" || arg == "--approval-mode=full-auto" {
            out.extend(CODEX_AUTO_ARGS.iter().map(|s| s.to_string()));
        } else {
            out.push(arg.clone());
        }
    }
    out
}

/// Resolve launch args for an agent type.
/// - `None` → use built-in defaults per agent type
/// - `Some([])` → no launch args (user explicitly disabled auto-mode)
/// - `Some([...])` → use exactly these args (with removed flags migrated forward)
pub fn resolved_launch_args(agent_type: &str, launch_args: Option<&[String]>) -> Vec<String> {
    match launch_args {
        Some(args) => migrate_legacy_args(agent_type, args),
        None => match agent_type {
            "claude" => vec!["--dangerously-skip-permissions".into()],
            "codex" => CODEX_AUTO_ARGS.iter().map(|s| s.to_string()).collect(),
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
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        let parsed: Config = serde_yaml_ng::from_str(&yaml).unwrap();
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
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(config.launch_args.is_none());
    }

    #[test]
    fn given_agent_config_with_empty_launch_args_should_deserialize_as_empty_vec() {
        let yaml = r#"
name: claude
command: claude
launchArgs: []
"#;
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
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
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
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
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        let parsed: AgentConfig = serde_yaml_ng::from_str(&yaml).unwrap();
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
        assert_eq!(
            args,
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request"
            ]
        );
    }

    #[test]
    fn given_legacy_codex_full_auto_should_migrate_to_long_form() {
        // Codex removed --full-auto; stored configs must not launch a flag it rejects.
        let args = resolved_launch_args("codex", Some(&["--full-auto".to_string()]));
        assert_eq!(
            args,
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request"
            ]
        );
    }

    #[test]
    fn given_legacy_codex_approval_mode_full_auto_should_migrate_to_long_form() {
        let args = resolved_launch_args("codex", Some(&["--approval-mode=full-auto".to_string()]));
        assert_eq!(
            args,
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request"
            ]
        );
    }

    #[test]
    fn given_legacy_codex_full_auto_with_other_args_should_preserve_them() {
        let args = resolved_launch_args(
            "codex",
            Some(&["--full-auto".to_string(), "--search".to_string()]),
        );
        assert_eq!(
            args,
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request",
                "--search"
            ]
        );
    }

    #[test]
    fn given_full_auto_for_non_codex_agent_should_not_migrate() {
        let args = resolved_launch_args("opencode", Some(&["--full-auto".to_string()]));
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
