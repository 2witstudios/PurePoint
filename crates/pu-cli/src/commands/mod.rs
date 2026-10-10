pub mod agent_def;
pub mod attach;
pub mod bench;
pub mod channel;
pub mod clean;
pub mod diff;
pub mod gate;
pub mod grid;
pub mod health;
pub mod init;
pub mod inventory;
pub mod kill;
pub mod logs;
pub mod prompt;
pub mod pulse;
pub mod schedule;
pub mod send;
pub mod spawn;
pub mod status;
pub mod swarm;
pub mod trigger;
pub mod watch;

use std::collections::HashMap;

use crate::error::CliError;

static PROJECT_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn set_project_override(root: String) {
    let _ = PROJECT_OVERRIDE.set(root);
}
pub fn explicit_project() -> Option<String> {
    PROJECT_OVERRIDE.get().cloned()
}

/// Agent IDs are daemon-wide; only --project constrains targeted operations.
pub fn agent_project_root() -> String {
    explicit_project().unwrap_or_default()
}

pub fn cwd_string() -> Result<String, CliError> {
    Ok(std::env::current_dir()?.to_string_lossy().to_string())
}

/// Resolve the project root directory.
/// Checks --project first, then PU_PROJECT_ROOT (set for worktree agents),
/// then the current working directory.
pub fn project_root_string() -> Result<String, CliError> {
    if let Some(root) = explicit_project() {
        return Ok(root);
    }
    if let Ok(root) = std::env::var("PU_PROJECT_ROOT")
        && !root.is_empty()
    {
        return Ok(root);
    }
    cwd_string()
}

/// Parse --var KEY=VALUE pairs into a HashMap.
pub fn parse_vars(vars: &[String]) -> Result<HashMap<String, String>, CliError> {
    let mut map = HashMap::new();
    for v in vars {
        let (key, value) = v.split_once('=').ok_or_else(|| {
            CliError::Other(format!("invalid --var format: {v} (expected KEY=VALUE)"))
        })?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vars_valid_input() {
        let input = vec!["FOO=bar".to_string(), "BAZ=qux".to_string()];
        let result = parse_vars(&input).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["FOO"], "bar");
        assert_eq!(result["BAZ"], "qux");
    }

    #[test]
    fn parse_vars_value_with_equals() {
        let input = vec!["URL=http://host?a=b".to_string()];
        let result = parse_vars(&input).unwrap();
        assert_eq!(result["URL"], "http://host?a=b");
    }

    #[test]
    fn parse_vars_missing_equals() {
        let input = vec!["NOEQUALS".to_string()];
        let result = parse_vars(&input);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("NOEQUALS"),
            "error should mention the bad input"
        );
    }

    #[test]
    fn parse_vars_empty_input() {
        let input: Vec<String> = vec![];
        let result = parse_vars(&input).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_vars_duplicate_keys_last_wins() {
        let input = vec!["KEY=first".to_string(), "KEY=second".to_string()];
        let result = parse_vars(&input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["KEY"], "second");
    }

    #[test]
    fn parse_vars_empty_key() {
        let input = vec!["=value".to_string()];
        let result = parse_vars(&input).unwrap();
        assert_eq!(result[""], "value");
    }

    #[test]
    fn parse_vars_empty_value() {
        let input = vec!["KEY=".to_string()];
        let result = parse_vars(&input).unwrap();
        assert_eq!(result["KEY"], "");
    }
}
