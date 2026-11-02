use std::path::Path;

use crate::error::PuError;
use crate::paths;
use crate::types::{AgentConfig, Config, LaunchArgs};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalAgentSettings {
    /// Observe agent status (working/idle/question) through the native adapters.
    #[serde(default = "crate::serde_defaults::default_true")]
    pub agent_status_tracking: bool,
    /// Per-agent command for every project on this machine, used when a
    /// project's `.pu/config.yaml` leaves `command` unset. An agent type that
    /// is not built in is an agent the user added.
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub commands: indexmap::IndexMap<String, String>,
    /// Per-agent-type launch args for every project on this machine, used when a
    /// project's `.pu/config.yaml` leaves `launchArgs` unset.
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub launch_args: indexmap::IndexMap<String, LaunchArgs>,
}

impl Default for GlobalAgentSettings {
    fn default() -> Self {
        Self {
            agent_status_tracking: true,
            commands: indexmap::IndexMap::new(),
            launch_args: indexmap::IndexMap::new(),
        }
    }
}

/// Changes to apply in `save_global_agent_settings`; `None` keeps the stored value.
#[derive(Debug, Default)]
pub struct GlobalAgentSettingsUpdate {
    pub agent_status_tracking: Option<bool>,
    /// Machine-level command per agent type; a blank string removes it.
    pub commands: indexmap::IndexMap<String, String>,
    /// Machine-level launch args string per agent type; a blank string removes it.
    pub launch_args: indexmap::IndexMap<String, String>,
    /// Agent types whose machine-level command and launch args are removed.
    pub remove_agents: Vec<String>,
}

fn check_agent_name(name: &str) -> Result<(), PuError> {
    if crate::types::is_valid_agent_name(name) {
        Ok(())
    } else {
        Err(PuError::InvalidArgument(format!(
            "invalid agent name {name:?}: use lowercase letters, digits, - and _"
        )))
    }
}

/// A blank string means "unset"; anything else is trimmed.
fn non_blank(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub fn load_global_agent_settings() -> Result<GlobalAgentSettings, PuError> {
    let path = paths::global_pu_dir()?.join("agent-settings.yaml");
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(serde_yaml_ng::from_str(&content)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GlobalAgentSettings::default()),
        Err(e) => Err(e.into()),
    }
}

/// Update the given settings, keeping the stored value of any left as `None`.
pub fn save_global_agent_settings(
    update: GlobalAgentSettingsUpdate,
) -> Result<GlobalAgentSettings, PuError> {
    use fs4::FileExt;
    // Validate before touching the file so a bad value never replaces a good one.
    for agent in update
        .commands
        .keys()
        .chain(update.launch_args.keys())
        .chain(&update.remove_agents)
    {
        check_agent_name(agent)?;
    }
    let mut launch_args = Vec::with_capacity(update.launch_args.len());
    for (agent, text) in update.launch_args {
        let args = if text.trim().is_empty() {
            None
        } else {
            Some(crate::types::parse_launch_args(&text).map_err(PuError::InvalidArgument)?)
        };
        launch_args.push((agent, args));
    }
    let dir = paths::global_pu_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("agent-settings.yaml");
    let lock = std::fs::File::create(path.with_extension("yaml.lock"))?;
    FileExt::lock(&lock)?;
    // A malformed file is an error, never silently replaced: it would erase every stored setting.
    let mut settings = load_global_agent_settings()?;
    if let Some(enabled) = update.agent_status_tracking {
        settings.agent_status_tracking = enabled;
    }
    for (agent, command) in update.commands {
        match non_blank(&command) {
            Some(command) => {
                settings.commands.insert(agent, command);
            }
            None => {
                settings.commands.shift_remove(&agent);
            }
        }
    }
    for (agent, args) in launch_args {
        match args {
            Some(args) => {
                settings.launch_args.insert(agent, LaunchArgs(args));
            }
            None => {
                settings.launch_args.shift_remove(&agent);
            }
        }
    }
    for agent in update.remove_agents {
        settings.commands.shift_remove(&agent);
        settings.launch_args.shift_remove(&agent);
    }
    let yaml = serde_yaml_ng::to_string(&settings)?;
    let tmp_path = path.with_extension("yaml.tmp");
    let mut tmp = std::fs::File::create(&tmp_path)?;
    std::io::Write::write_all(&mut tmp, yaml.as_bytes())?;
    tmp.sync_all()?;
    std::fs::rename(tmp_path, path)?;
    Ok(settings)
}

/// Add the machine's agents and settings to a project config, then resolve each
/// agent's command: project, else machine, else the built-in default.
fn apply_global_agent_settings(config: &mut Config, settings: &GlobalAgentSettings) {
    for name in settings.commands.keys().chain(settings.launch_args.keys()) {
        if !config.agents.contains_key(name) {
            config.agents.insert(name.clone(), AgentConfig::named(name));
        }
    }
    for (name, agent) in config.agents.iter_mut() {
        agent.machine_command = settings.commands.get(name).cloned();
        agent.machine_launch_args = settings.launch_args.get(name).map(|args| args.0.clone());
        agent.command = agent
            .project_command
            .clone()
            .unwrap_or_else(|| agent.inherited_command(name));
    }
}

pub fn load_config(project_root: &Path) -> Config {
    load_config_result(project_root).unwrap_or_default()
}

pub fn load_config_strict(project_root: &Path) -> Result<Config, PuError> {
    load_config_result(project_root)
}

fn load_config_result(project_root: &Path) -> Result<Config, PuError> {
    let path = paths::config_path(project_root);
    let mut config = match std::fs::read_to_string(&path) {
        Ok(content) => {
            let mut config: Config = serde_yaml_ng::from_str(&content)?;
            for agent in config.agents.values_mut() {
                agent.project_command = non_blank(&std::mem::take(&mut agent.command));
            }
            // Fill in any agents missing from file with code defaults
            for (name, agent) in crate::types::default_agents() {
                config.agents.entry(name).or_insert(agent);
            }
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(PuError::Io(e)),
    }?;
    apply_global_agent_settings(&mut config, &load_global_agent_settings()?);
    Ok(config)
}

pub fn resolve_agent<'a>(config: &'a Config, name: &str) -> Option<&'a AgentConfig> {
    config.agents.get(name)
}

/// Changes to one agent in a project config.
#[derive(Debug, Default)]
pub struct AgentConfigUpdate {
    /// `None` keeps the stored command; a blank string clears it.
    pub command: Option<String>,
    /// The launch args to store; `None` clears them.
    pub launch_args: Option<Vec<String>>,
    /// Drop the agent's entry from the project config.
    pub remove: bool,
}

/// Update one agent's command and launch args in the project config.
/// Loads existing config, modifies the named agent, writes back to YAML.
/// Returns the updated (merged) config.
pub fn update_agent_config(
    project_root: &Path,
    agent_name: &str,
    update: AgentConfigUpdate,
) -> Result<Config, PuError> {
    check_agent_name(agent_name)?;
    let path = paths::config_path(project_root);

    // Hold an exclusive lock across the read-modify-write so overlapping updates
    // (the daemon serves each request on its own blocking thread) can't lose or
    // interleave writes. The lock file is left in place: unlinking it on release
    // would let a waiter and a new caller lock different inodes.
    use fs4::FileExt;
    let lock_file = std::fs::File::create(path.with_extension("yaml.lock"))?;
    FileExt::lock(&lock_file)?;

    // Load raw config from file (without merging code defaults)
    let mut raw_config: Config = match std::fs::read_to_string(&path) {
        Ok(content) => serde_yaml_ng::from_str(&content)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config {
            default_agent: "claude".into(),
            agents: indexmap::IndexMap::new(),
            env_files: vec![".env".into(), ".env.local".into()],
        },
        Err(e) => return Err(PuError::Io(e)),
    };

    if update.remove {
        raw_config.agents.shift_remove(agent_name);
    } else {
        // Only what the user sets is written: an unset command stays inherited.
        let agent = raw_config
            .agents
            .entry(agent_name.to_string())
            .or_insert_with(|| AgentConfig::named(agent_name));
        if let Some(command) = update.command {
            agent.command = non_blank(&command).unwrap_or_default();
        }
        agent.launch_args = update.launch_args;
    }

    // Write back to YAML atomically: temp file, fsync, rename. A plain truncating
    // write can leave a torn file (e.g. a duplicated `envFiles:` tail) that fails
    // every later load.
    let yaml = serde_yaml_ng::to_string(&raw_config)?;
    let tmp_path = path.with_extension("yaml.tmp");
    let mut tmp = std::fs::File::create(&tmp_path)?;
    std::io::Write::write_all(&mut tmp, yaml.as_bytes())?;
    tmp.sync_all()?;
    std::fs::rename(&tmp_path, &path)?;
    drop(lock_file);

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
# Agents launch with their bare command (claude, codex, opencode) and no
# permission or auto-approve flags unless you add them here. launchArgs is one
# free-form string, split like a shell command line (quotes are respected).
#
# To set your own default for every project on this machine instead, put it in
# ~/.pu/agent-settings.yaml:
#   commands:
#     claude: "claude"
#   launchArgs:
#     claude: "--dangerously-skip-permissions"
# A project command or launchArgs (even "") takes precedence over the machine default.
# agents:
#   claude:
#     name: claude
#     command: claude
#     launchArgs: "--model opus --append-system-prompt 'Be terse.'"
#   codex:
#     name: codex
#     command: codex
#     launchArgs: "--sandbox workspace-write --ask-for-approval on-request"
#   opencode:
#     name: opencode
#     command: opencode
#     launchArgs: "--auto"
"#;
    std::fs::write(&path, yaml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TestHome(TempDir);
    impl TestHome {
        fn new() -> Self {
            let home = TempDir::new().unwrap();
            paths::set_home_override(Some(home.path().into()));
            Self(home)
        }
    }
    impl Drop for TestHome {
        fn drop(&mut self) {
            paths::set_home_override(None);
        }
    }

    #[test]
    fn status_tracking_defaults_on_and_updates() {
        let _home = TestHome::new();
        assert!(load_global_agent_settings().unwrap().agent_status_tracking);
        let settings = save_global_agent_settings(GlobalAgentSettingsUpdate {
            agent_status_tracking: Some(false),
            ..Default::default()
        })
        .unwrap();
        assert!(!settings.agent_status_tracking);
        assert!(!load_global_agent_settings().unwrap().agent_status_tracking);
    }

    #[test]
    fn given_legacy_codex_yolo_key_should_load_and_drop_it_on_save() {
        // given — a settings file written before Codex YOLO was removed
        let home = TestHome::new();
        let path = home.0.path().join(".pu/agent-settings.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "codexYolo: true\nagentStatusTracking: false\nlaunchArgs:\n  codex: --search\n",
        )
        .unwrap();
        // when
        let settings = load_global_agent_settings().unwrap();
        // then — the stale key is ignored and the other settings survive
        assert!(!settings.agent_status_tracking);
        assert_eq!(settings.launch_args["codex"].0, ["--search"]);
        let project = TempDir::new().unwrap();
        let config = load_config_strict(project.path()).unwrap();
        assert_eq!(resolved(&config, "codex"), ["--search"]);
        // when — any save rewrites the file without it
        save_global_agent_settings(GlobalAgentSettingsUpdate::default()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("codexYolo"), "{content}");
        assert!(content.contains("agentStatusTracking: false"), "{content}");
    }

    #[test]
    fn malformed_global_settings_fail_strict_config_resolution() {
        let home = TestHome::new();
        std::fs::create_dir_all(home.0.path().join(".pu")).unwrap();
        std::fs::write(
            home.0.path().join(".pu/agent-settings.yaml"),
            "agentStatusTracking: [broken",
        )
        .unwrap();
        let project = TempDir::new().unwrap();
        assert!(load_config_strict(project.path()).is_err());
    }

    #[test]
    fn failed_global_save_retains_confirmed_setting() {
        let home = TestHome::new();
        let tracking = |enabled: bool| GlobalAgentSettingsUpdate {
            agent_status_tracking: Some(enabled),
            ..Default::default()
        };
        save_global_agent_settings(tracking(false)).unwrap();
        std::fs::create_dir(home.0.path().join(".pu/agent-settings.yaml.tmp")).unwrap();
        assert!(save_global_agent_settings(tracking(true)).is_err());
        assert!(!load_global_agent_settings().unwrap().agent_status_tracking);
    }

    #[test]
    fn given_malformed_machine_settings_should_refuse_save_and_keep_file() {
        for content in [
            "agentStatusTracking: true\nlaunchArgs: [broken",
            "agentStatusTracking: true\nlaunchArgs:\n  claude: \"--append-system-prompt 'oops\"\n",
        ] {
            // given
            let home = TestHome::new();
            let path = home.0.path().join(".pu/agent-settings.yaml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            // when
            let result = save_global_agent_settings(GlobalAgentSettingsUpdate {
                launch_args: [("codex".to_string(), "--search".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            });
            // then
            assert!(result.is_err(), "save should fail for {content:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
    }

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
        let config: crate::types::Config = serde_yaml_ng::from_str(yaml).unwrap();
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
        assert!(content.contains("--ask-for-approval"));
        assert!(content.contains("agent-settings.yaml"));
        assert!(content.contains("--model"));
        assert!(!content.contains("# Default: ["));
    }

    #[test]
    fn given_config_with_one_agent_should_merge_code_defaults() {
        let _home = TestHome::new();
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
    fn given_config_with_empty_launch_args_should_launch_bare() {
        let _home = TestHome::new();
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
    fn given_config_without_launch_args_should_launch_bare() {
        let _home = TestHome::new();
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml =
            "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = load_config(root);
        let claude = &config.agents["claude"];
        assert!(claude.launch_args.is_none());
        // then — no permission flags are added by default
        let args = crate::types::resolved_launch_args("claude", claude.configured_launch_args());
        assert!(args.is_empty());
    }

    fn write_machine_settings(home: &TestHome, yaml: &str) {
        std::fs::create_dir_all(home.0.path().join(".pu")).unwrap();
        std::fs::write(home.0.path().join(".pu/agent-settings.yaml"), yaml).unwrap();
    }

    fn resolved(config: &Config, agent: &str) -> Vec<String> {
        crate::types::resolved_launch_args(agent, config.agents[agent].configured_launch_args())
    }

    #[test]
    fn given_machine_launch_args_should_apply_to_projects_without_their_own() {
        // given
        let home = TestHome::new();
        write_machine_settings(
            &home,
            "launchArgs:\n  claude: --dangerously-skip-permissions --model opus\n",
        );
        let project = TempDir::new().unwrap();
        // when
        let config = load_config_strict(project.path()).unwrap();
        // then
        assert_eq!(
            resolved(&config, "claude"),
            ["--dangerously-skip-permissions", "--model", "opus"]
        );
        assert!(config.agents["claude"].launch_args.is_none());
        assert!(resolved(&config, "codex").is_empty());
    }

    #[test]
    fn given_project_launch_args_should_override_machine_launch_args() {
        // given
        let home = TestHome::new();
        write_machine_settings(
            &home,
            "launchArgs:\n  claude: --dangerously-skip-permissions\n",
        );
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(paths::pu_dir(project.path())).unwrap();
        std::fs::write(
            paths::config_path(project.path()),
            "agents:\n  claude:\n    name: claude\n    command: claude\n    launchArgs: \"\"\n  opencode:\n    name: opencode\n    command: opencode\n    launchArgs: --model x/y\n",
        )
        .unwrap();
        // when
        let config = load_config_strict(project.path()).unwrap();
        // then — an explicit empty project value wins over the machine default
        assert!(resolved(&config, "claude").is_empty());
        assert_eq!(resolved(&config, "opencode"), ["--model", "x/y"]);
    }

    #[test]
    fn given_machine_command_should_apply_where_the_project_leaves_it_unset() {
        // given
        let home = TestHome::new();
        write_machine_settings(
            &home,
            "commands:\n  claude: scripts/agent-launch.sh claude\n  codex: scripts/agent-launch.sh codex\n",
        );
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(paths::pu_dir(project.path())).unwrap();
        std::fs::write(
            paths::config_path(project.path()),
            "agents:\n  claude:\n    name: claude\n    launchArgs: --model opus\n  codex:\n    name: codex\n    command: codex\n",
        )
        .unwrap();
        // when
        let config = load_config_strict(project.path()).unwrap();
        // then — the project's command wins, a missing one inherits
        let claude = &config.agents["claude"];
        assert_eq!(claude.command, "scripts/agent-launch.sh claude");
        assert_eq!(claude.project_command, None);
        let codex = &config.agents["codex"];
        assert_eq!(codex.command, "codex");
        assert_eq!(codex.project_command.as_deref(), Some("codex"));
        assert_eq!(
            codex.inherited_command("codex"),
            "scripts/agent-launch.sh codex"
        );
        assert_eq!(config.agents["opencode"].command, "opencode");
    }

    #[test]
    fn given_added_machine_agent_should_load_in_every_project() {
        // given
        let _home = TestHome::new();
        save_global_agent_settings(GlobalAgentSettingsUpdate {
            commands: [("aider".to_string(), "aider".to_string())]
                .into_iter()
                .collect(),
            launch_args: [("aider".to_string(), "--yes".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        })
        .unwrap();
        let project = TempDir::new().unwrap();
        // when
        let config = load_config_strict(project.path()).unwrap();
        // then
        assert_eq!(config.agents["aider"].command, "aider");
        assert_eq!(resolved(&config, "aider"), ["--yes"]);
        // when — the agent is removed
        save_global_agent_settings(GlobalAgentSettingsUpdate {
            remove_agents: vec!["aider".into()],
            ..Default::default()
        })
        .unwrap();
        // then
        let config = load_config_strict(project.path()).unwrap();
        assert!(!config.agents.contains_key("aider"));
    }

    #[test]
    fn given_invalid_agent_name_should_refuse_machine_save() {
        let _home = TestHome::new();
        let result = save_global_agent_settings(GlobalAgentSettingsUpdate {
            commands: [("My Agent".to_string(), "x".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        });
        assert_eq!(result.unwrap_err().code(), "INVALID_ARGUMENT");
    }

    #[test]
    fn given_update_agent_config_command_should_set_clear_and_remove() {
        // given
        let _home = TestHome::new();
        let project = TempDir::new().unwrap();
        let root = project.path();
        std::fs::create_dir_all(paths::pu_dir(root)).unwrap();
        let update = |command: Option<&str>, remove: bool| AgentConfigUpdate {
            command: command.map(str::to_string),
            launch_args: Some(vec!["--yes".into()]),
            remove,
        };
        // when — only args are set, the command stays inherited
        update_agent_config(root, "claude", update(None, false)).unwrap();
        let written = std::fs::read_to_string(paths::config_path(root)).unwrap();
        assert!(!written.contains("command:"), "{written}");
        // when — a command is set
        let config = update_agent_config(root, "aider", update(Some(" aider "), false)).unwrap();
        assert_eq!(config.agents["aider"].command, "aider");
        assert_eq!(
            config.agents["aider"].project_command.as_deref(),
            Some("aider")
        );
        // when — the command is cleared
        let config = update_agent_config(root, "aider", update(Some(""), false)).unwrap();
        assert_eq!(config.agents["aider"].project_command, None);
        assert_eq!(config.agents["aider"].command, "aider");
        // when — the agent is removed
        let config = update_agent_config(root, "aider", update(None, true)).unwrap();
        assert!(!config.agents.contains_key("aider"));
        assert_eq!(resolved(&config, "claude"), ["--yes"]);
    }

    #[test]
    fn given_malformed_machine_launch_args_should_fail_config_resolution() {
        let home = TestHome::new();
        write_machine_settings(
            &home,
            "launchArgs:\n  claude: \"--append-system-prompt 'oops\"\n",
        );
        let project = TempDir::new().unwrap();
        let err = load_config_strict(project.path()).unwrap_err();
        assert!(err.to_string().contains("invalid launch args"), "{err}");
        // and the non-strict loader falls back to the bare defaults
        assert!(resolved(&load_config(project.path()), "claude").is_empty());
    }

    #[test]
    fn given_machine_launch_args_update_should_set_and_remove_per_agent() {
        // given
        let _home = TestHome::new();
        let update = |agent: &str, text: &str| GlobalAgentSettingsUpdate {
            launch_args: [(agent.to_string(), text.to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        // when
        save_global_agent_settings(update("claude", "--model 'claude opus'")).unwrap();
        save_global_agent_settings(update("codex", "--search")).unwrap();
        let settings = save_global_agent_settings(update("codex", "  ")).unwrap();
        // then
        assert_eq!(settings.launch_args["claude"].0, ["--model", "claude opus"]);
        assert!(!settings.launch_args.contains_key("codex"));
        assert_eq!(
            load_global_agent_settings().unwrap().launch_args["claude"].0,
            ["--model", "claude opus"]
        );
    }

    #[test]
    fn given_invalid_machine_launch_args_update_should_keep_stored_value() {
        let _home = TestHome::new();
        let update = |agent: &str, text: &str| GlobalAgentSettingsUpdate {
            launch_args: [(agent.to_string(), text.to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        save_global_agent_settings(update("claude", "--verbose")).unwrap();
        let bad_quote = save_global_agent_settings(update("claude", "--x 'oops")).unwrap_err();
        let bad_name = save_global_agent_settings(update("No Pe", "--verbose")).unwrap_err();
        assert_eq!(bad_quote.code(), "INVALID_ARGUMENT");
        assert_eq!(bad_name.code(), "INVALID_ARGUMENT");
        assert_eq!(
            load_global_agent_settings().unwrap().launch_args["claude"].0,
            ["--verbose"]
        );
    }

    fn args(launch_args: Option<Vec<String>>) -> AgentConfigUpdate {
        AgentConfigUpdate {
            launch_args,
            ..Default::default()
        }
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
            args(Some(vec![
                "--verbose".to_string(),
                "--model".to_string(),
                "opus".to_string(),
            ])),
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
    fn given_update_agent_config_should_write_launch_args_as_string() {
        let _home = TestHome::new();
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();

        update_agent_config(
            root,
            "claude",
            args(Some(vec![
                "--append-system-prompt".into(),
                "be terse".into(),
            ])),
        )
        .unwrap();

        let written = std::fs::read_to_string(crate::paths::config_path(root)).unwrap();
        assert!(
            written.contains("launchArgs: --append-system-prompt 'be terse'"),
            "{written}"
        );
    }

    #[test]
    fn given_update_agent_config_with_none_should_reset_to_defaults() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n    launchArgs:\n      - '--verbose'\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config = update_agent_config(root, "claude", args(None)).unwrap();
        let claude = &config.agents["claude"];
        assert!(claude.launch_args.is_none());
    }

    #[test]
    fn given_update_agent_config_should_preserve_other_agents() {
        let _home = TestHome::new();
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();
        let yaml = "defaultAgent: claude\nagents:\n  claude:\n    name: claude\n    command: claude\n  codex:\n    name: codex\n    command: codex\n    launchArgs:\n      - '--search'\n";
        std::fs::write(crate::paths::config_path(root), yaml).unwrap();

        let config =
            update_agent_config(root, "claude", args(Some(vec!["--verbose".to_string()]))).unwrap();

        // Claude should have new args
        assert_eq!(
            config.agents["claude"].launch_args,
            Some(vec!["--verbose".to_string()])
        );
        // Codex should be preserved
        assert_eq!(
            config.agents["codex"].launch_args,
            Some(vec!["--search".to_string()])
        );
    }

    #[test]
    fn given_update_agent_config_with_invalid_agent_name_should_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::pu_dir(root)).unwrap();

        let result =
            update_agent_config(root, "Not An Agent", args(Some(vec!["--verbose".into()])));
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
            args(Some(vec![
                "--permission-mode".to_string(),
                "plan".to_string(),
            ])),
        )
        .unwrap();

        assert!(crate::paths::config_path(root).exists());
        assert_eq!(
            config.agents["claude"].launch_args,
            Some(vec!["--permission-mode".to_string(), "plan".to_string()])
        );
    }

    #[test]
    fn given_concurrent_update_agent_config_calls_should_leave_parseable_config() {
        // Overlapping read-modify-write calls (the settings UI fires one per change)
        // used to interleave truncating writes, leaving a stale tail such as a
        // second `envFiles:` key that made every later config load fail.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(crate::paths::pu_dir(&root)).unwrap();

        let handles: Vec<_> = (0..16)
            .map(|i| {
                let root = root.clone();
                std::thread::spawn(move || {
                    let agent = ["claude", "codex", "opencode", "terminal"][i % 4];
                    let flags: Vec<String> = (0..=i).map(|n| format!("--flag-{n}")).collect();
                    for _ in 0..20 {
                        update_agent_config(&root, agent, args(Some(flags.clone()))).unwrap();
                        update_agent_config(&root, agent, args(None)).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        load_config_strict(&root).unwrap();
    }
}
