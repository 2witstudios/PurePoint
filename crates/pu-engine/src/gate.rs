use std::path::Path;
use std::process::Stdio;

use pu_core::trigger_def::TriggerDef;

#[derive(Debug)]
pub struct GateEvalResult {
    pub passed: bool,
    pub output: String,
}

/// Evaluate all gate actions across the given trigger definitions.
/// Returns passed=true only if every gate command exits with its expected code (default 0).
pub async fn evaluate_trigger_gates(
    triggers: &[TriggerDef],
    worktree_path: &Path,
) -> Result<GateEvalResult, Box<dyn std::error::Error + Send + Sync>> {
    let mut all_output = String::new();

    for trigger in triggers {
        for action in &trigger.sequence {
            if let Some(ref gate) = action.gate {
                let max_retries = action.max_retries.unwrap_or(0);
                let expect_exit = gate.expect_exit.unwrap_or(0);
                let mut passed = false;

                for attempt in 0..=max_retries {
                    if attempt > 0 {
                        all_output
                            .push_str(&format!("  retry {attempt}/{max_retries}: {}\n", gate.run));
                    }

                    match run_gate_command(&gate.run, worktree_path).await {
                        Ok((exit_code, stdout, stderr)) => {
                            if exit_code == expect_exit {
                                passed = true;
                                break;
                            }
                            all_output.push_str(&format!(
                                "gate '{}' exited {} (expected {})\n",
                                gate.run, exit_code, expect_exit
                            ));
                            if !stdout.is_empty() {
                                all_output.push_str(&stdout);
                            }
                            if !stderr.is_empty() {
                                all_output.push_str(&stderr);
                            }
                        }
                        Err(e) => {
                            all_output.push_str(&format!("gate '{}' error: {e}\n", gate.run));
                        }
                    }
                }

                if !passed {
                    return Ok(GateEvalResult {
                        passed: false,
                        output: all_output,
                    });
                }
            }
        }
    }

    Ok(GateEvalResult {
        passed: true,
        output: all_output,
    })
}

pub async fn run_gate_command(
    command: &str,
    cwd: &Path,
) -> Result<(i32, String, String), Box<dyn std::error::Error + Send + Sync>> {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((exit_code, stdout, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pu_core::trigger_def::{GateDef, TriggerAction, TriggerEvent};

    fn make_gate_trigger(command: &str, expect_exit: Option<i32>) -> TriggerDef {
        TriggerDef {
            name: "test-gate".into(),
            description: None,
            on: TriggerEvent::PreCommit,
            sequence: vec![TriggerAction {
                inject: None,
                gate: Some(GateDef {
                    run: command.into(),
                    expect_exit,
                }),
                max_retries: None,
            }],
            variables: std::collections::HashMap::new(),
            scope: String::new(),
        }
    }

    #[tokio::test]
    async fn given_passing_gate_should_return_passed() {
        let trigger = make_gate_trigger("true", None);
        let result = evaluate_trigger_gates(&[trigger], Path::new("/tmp"))
            .await
            .unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn given_failing_gate_should_return_not_passed() {
        let trigger = make_gate_trigger("false", None);
        let result = evaluate_trigger_gates(&[trigger], Path::new("/tmp"))
            .await
            .unwrap();
        assert!(!result.passed);
        assert!(result.output.contains("exited 1"));
    }

    #[tokio::test]
    async fn given_custom_expect_exit_should_match() {
        let trigger = make_gate_trigger("exit 2", Some(2));
        let result = evaluate_trigger_gates(&[trigger], Path::new("/tmp"))
            .await
            .unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn given_no_gates_should_pass() {
        let trigger = TriggerDef {
            name: "inject-only".into(),
            description: None,
            on: TriggerEvent::PreCommit,
            sequence: vec![TriggerAction {
                inject: Some("text".into()),
                gate: None,
                max_retries: None,
            }],
            variables: std::collections::HashMap::new(),
            scope: String::new(),
        };
        let result = evaluate_trigger_gates(&[trigger], Path::new("/tmp"))
            .await
            .unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn given_retry_should_attempt_multiple_times() {
        let trigger = TriggerDef {
            name: "retry-gate".into(),
            description: None,
            on: TriggerEvent::PreCommit,
            sequence: vec![TriggerAction {
                inject: None,
                gate: Some(GateDef {
                    run: "false".into(),
                    expect_exit: None,
                }),
                max_retries: Some(2),
            }],
            variables: std::collections::HashMap::new(),
            scope: String::new(),
        };
        let result = evaluate_trigger_gates(&[trigger], Path::new("/tmp"))
            .await
            .unwrap();
        assert!(!result.passed);
        // Should have retry output
        assert!(result.output.contains("retry"));
    }

    #[tokio::test]
    async fn given_empty_triggers_should_pass() {
        let result = evaluate_trigger_gates(&[], Path::new("/tmp"))
            .await
            .unwrap();
        assert!(result.passed);
    }
}
