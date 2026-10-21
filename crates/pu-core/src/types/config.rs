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
    /// Extra args appended after `command`. Written as one shell-style string;
    /// the older list form is still accepted.
    #[serde(default, with = "launch_args_serde")]
    pub launch_args: Option<Vec<String>>,
    /// Machine-level launch args from `~/.pu/agent-settings.yaml`, used only when
    /// the project leaves `launch_args` unset. Filled in at config resolution.
    #[serde(skip)]
    pub machine_launch_args: Option<Vec<String>>,
}

impl AgentConfig {
    /// The launch args in effect before resolution: project, else machine,
    /// else none.
    pub fn configured_launch_args(&self) -> Option<&[String]> {
        self.launch_args
            .as_deref()
            .or(self.machine_launch_args.as_deref())
    }

    /// Drop every configured launch arg so the agent launches its bare command.
    pub fn clear_launch_args(&mut self) {
        self.launch_args = Some(Vec::new());
        self.machine_launch_args = None;
    }
}

/// Split a free-form launch args string into argv, shell-style (quotes respected).
pub fn parse_launch_args(text: &str) -> Result<Vec<String>, String> {
    shell_words::split(text).map_err(|e| format!("invalid launch args {text:?}: {e}"))
}

/// Canonical string form of launch args; `parse_launch_args` inverts it.
pub fn format_launch_args(args: &[String]) -> String {
    shell_words::join(args)
}

/// Launch args that deserialize from a shell-style string or a list of
/// strings, and serialize as the string form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchArgs(pub Vec<String>);

impl Serialize for LaunchArgs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format_launch_args(&self.0))
    }
}

impl<'de> Deserialize<'de> for LaunchArgs {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = LaunchArgs;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a launch args string or a list of strings")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<LaunchArgs, E> {
                parse_launch_args(v).map(LaunchArgs).map_err(E::custom)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<LaunchArgs, A::Error> {
                let mut args = Vec::new();
                while let Some(arg) = seq.next_element::<String>()? {
                    args.push(arg);
                }
                Ok(LaunchArgs(args))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// `with` adapter so `Option<Vec<String>>` fields use the `LaunchArgs` format.
pub mod launch_args_serde {
    use super::LaunchArgs;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        args: &Option<Vec<String>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        args.clone().map(LaunchArgs).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<String>>, D::Error> {
        Ok(Option::<LaunchArgs>::deserialize(deserializer)?.map(|args| args.0))
    }
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
                machine_launch_args: None,
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
/// Only flags the user wrote are rewritten; nothing is added otherwise.
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
/// - `None` → no launch args: agents launch with their bare command
/// - `Some([...])` → exactly these args (with removed flags migrated forward)
pub fn resolved_launch_args(agent_type: &str, launch_args: Option<&[String]>) -> Vec<String> {
    launch_args
        .map(|args| migrate_legacy_args(agent_type, args))
        .unwrap_or_default()
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
            machine_launch_args: None,
        };
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        let parsed: AgentConfig = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed.launch_args, config.launch_args);
    }

    #[test]
    fn given_launch_args_string_should_split_shell_style() {
        // given
        let yaml = r#"
name: claude
command: claude
launchArgs: '--model opus --append-system-prompt "be terse"'
"#;
        // when
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
        // then
        assert_eq!(
            config.launch_args,
            Some(vec![
                "--model".to_string(),
                "opus".to_string(),
                "--append-system-prompt".to_string(),
                "be terse".to_string(),
            ])
        );
    }

    #[test]
    fn given_empty_launch_args_string_should_deserialize_as_empty_vec() {
        let yaml = "name: claude\ncommand: claude\nlaunchArgs: \"\"\n";
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.launch_args, Some(vec![]));
    }

    #[test]
    fn given_unbalanced_quote_in_launch_args_should_fail_to_deserialize() {
        let yaml = "name: claude\ncommand: claude\nlaunchArgs: '--append-system-prompt \"oops'\n";
        let err = serde_yaml_ng::from_str::<AgentConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("invalid launch args"), "{err}");
    }

    #[test]
    fn given_list_launch_args_should_write_back_as_string() {
        // given — legacy list form with a value containing a space
        let yaml =
            "name: claude\ncommand: claude\nlaunchArgs: [--append-system-prompt, be terse]\n";
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
        // when
        let written = serde_yaml_ng::to_string(&config).unwrap();
        let reparsed: AgentConfig = serde_yaml_ng::from_str(&written).unwrap();
        // then
        assert!(
            written.contains("launchArgs: --append-system-prompt 'be terse'"),
            "{written}"
        );
        assert_eq!(reparsed.launch_args, config.launch_args);
    }

    #[test]
    fn given_string_launch_args_should_round_trip_yaml() {
        let yaml = "name: codex\ncommand: codex\nlaunchArgs: \"-c 'model=\\\"o3\\\"' --search\"\n";
        let config: AgentConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            config.launch_args,
            Some(vec!["-c".into(), "model=\"o3\"".into(), "--search".into()])
        );
        let written = serde_yaml_ng::to_string(&config).unwrap();
        let reparsed: AgentConfig = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(reparsed.launch_args, config.launch_args);
    }

    #[test]
    fn given_empty_launch_args_should_round_trip_yaml() {
        let config: AgentConfig =
            serde_yaml_ng::from_str("name: claude\ncommand: claude\nlaunchArgs: []\n").unwrap();
        let written = serde_yaml_ng::to_string(&config).unwrap();
        let reparsed: AgentConfig = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(reparsed.launch_args, Some(vec![]));
    }

    #[test]
    fn given_unset_launch_args_should_resolve_bare_for_every_agent_type() {
        for agent_type in ["claude", "codex", "opencode", "terminal"] {
            assert_eq!(
                resolved_launch_args(agent_type, None),
                Vec::<String>::new(),
                "{agent_type}"
            );
        }
    }

    #[test]
    fn given_project_launch_args_should_take_precedence_over_machine() {
        // given
        let mut config = default_agents()["claude"].clone();
        config.machine_launch_args = Some(vec!["--dangerously-skip-permissions".into()]);
        config.launch_args = Some(vec![]);
        // then — an explicit empty project value still wins
        assert_eq!(config.configured_launch_args(), Some(&[][..]));
    }

    #[test]
    fn given_no_project_launch_args_should_use_machine_launch_args() {
        let mut config = default_agents()["claude"].clone();
        config.machine_launch_args = Some(vec!["--model".into(), "opus".into()]);
        assert_eq!(
            config.configured_launch_args(),
            Some(&["--model".to_string(), "opus".to_string()][..])
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
    fn given_codex_args_without_legacy_flags_should_not_gain_any() {
        let custom = vec!["--search".to_string()];
        assert_eq!(
            resolved_launch_args("codex", Some(&custom)),
            vec!["--search"]
        );
        assert!(resolved_launch_args("codex", Some(&[])).is_empty());
    }

    #[test]
    fn given_explicit_launch_args_should_override_defaults() {
        let custom = vec!["--verbose".to_string()];
        let args = resolved_launch_args("claude", Some(&custom));
        assert_eq!(args, vec!["--verbose"]);
    }
}
