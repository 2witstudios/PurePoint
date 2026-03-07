pub mod agent_def;
pub mod attach;
pub mod clean;
pub mod diff;
pub mod grid;
pub mod health;
pub mod init;
pub mod kill;
pub mod logs;
pub mod prompt;
pub mod schedule;
pub mod send;
pub mod spawn;
pub mod status;
pub mod swarm;

use std::collections::HashMap;

use crate::error::CliError;

pub fn cwd_string() -> Result<String, CliError> {
    Ok(std::env::current_dir()?.to_string_lossy().to_string())
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
