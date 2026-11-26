use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use pu_core::trigger_def::TriggerDef;

/// Default timeout for individual gate commands (60 seconds).
const GATE_TIMEOUT: Duration = Duration::from_secs(60);

/// Default timeout for the entire gate evaluation (5 minutes).
/// Prevents git hooks from blocking the user indefinitely when many gates/retries exist.
const GATE_TOTAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Default max retries for gate commands.
pub const DEFAULT_GATE_MAX_RETRIES: u32 = 0;

#[derive(Debug)]
pub struct GateEvalResult {
    pub passed: bool,
    pub output: String,
    /// The interpreter gate commands ran under (`"sh"`, or on Windows without
    /// Git's `sh.exe`, `"pwsh"` / `"powershell"`).
    pub shell: &'static str,
}

/// How gate commands run: `sh -c` everywhere it exists. On Windows that is
/// Git for Windows' `sh.exe` (the same shell git runs hooks with); without
/// it, `pwsh -NoProfile -Command`, then Windows PowerShell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateShell {
    pub name: &'static str,
    program: std::path::PathBuf,
}

impl GateShell {
    fn command(&self, script: &str) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        if self.name == "sh" {
            command.arg("-c");
        } else {
            command.args(["-NoProfile", "-NonInteractive", "-Command"]);
        }
        command.arg(script);
        #[cfg(windows)]
        command.creation_flags(crate::git::HELPER_CREATION_FLAGS);
        command
    }
}

/// The shell gates use on this machine, resolved once.
pub fn gate_shell() -> &'static GateShell {
    static SHELL: std::sync::OnceLock<GateShell> = std::sync::OnceLock::new();
    SHELL.get_or_init(|| {
        resolve_gate_shell(
            std::env::var_os("PATH").unwrap_or_default(),
            std::env::var_os("ProgramFiles"),
        )
    })
}

#[cfg(unix)]
fn resolve_gate_shell(
    _path: std::ffi::OsString,
    _program_files: Option<std::ffi::OsString>,
) -> GateShell {
    GateShell {
        name: "sh",
        program: "sh".into(),
    }
}

/// `sh.exe` on PATH, else beside the `git.exe` on PATH (`<Git>\cmd\git.exe`
/// → `<Git>\bin\sh.exe` or `<Git>\usr\bin\sh.exe`), else the default Git
/// install; then `pwsh.exe` on PATH, then Windows PowerShell.
#[cfg(windows)]
fn resolve_gate_shell(
    path: std::ffi::OsString,
    program_files: Option<std::ffi::OsString>,
) -> GateShell {
    use std::path::PathBuf;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let on_path = |exe: &str| dirs.iter().map(|d| d.join(exe)).find(|p| p.is_file());
    let beside_git = || {
        let git = on_path("git.exe")?;
        git.ancestors()
            .skip(2)
            .take(2)
            .flat_map(|root| [root.join("bin").join("sh.exe"), root.join("usr/bin/sh.exe")])
            .find(|p| p.is_file())
    };
    let program_files = || {
        let root = program_files.clone()?;
        let sh = PathBuf::from(root).join("Git").join("bin").join("sh.exe");
        sh.is_file().then_some(sh)
    };
    if let Some(program) = on_path("sh.exe").or_else(beside_git).or_else(program_files) {
        return GateShell {
            name: "sh",
            program,
        };
    }
    if let Some(program) = on_path("pwsh.exe") {
        return GateShell {
            name: "pwsh",
            program,
        };
    }
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    GateShell {
        name: "powershell",
        program: PathBuf::from(system).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
    }
}

/// Evaluate all gate actions across the given trigger definitions.
/// Returns passed=true only if every gate command exits with its expected code (default 0).
/// The entire evaluation is bounded by `GATE_TOTAL_TIMEOUT` to prevent blocking git hooks.
pub async fn evaluate_trigger_gates(
    triggers: &[TriggerDef],
    worktree_path: &Path,
) -> Result<GateEvalResult, Box<dyn std::error::Error + Send + Sync>> {
    match tokio::time::timeout(
        GATE_TOTAL_TIMEOUT,
        evaluate_trigger_gates_inner(triggers, worktree_path),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Ok(GateEvalResult {
            passed: false,
            output: format!(
                "gate evaluation timed out after {}s\n",
                GATE_TOTAL_TIMEOUT.as_secs()
            ),
            shell: gate_shell().name,
        }),
    }
}

async fn evaluate_trigger_gates_inner(
    triggers: &[TriggerDef],
    worktree_path: &Path,
) -> Result<GateEvalResult, Box<dyn std::error::Error + Send + Sync>> {
    let mut all_output = String::new();

    for trigger in triggers {
        for action in &trigger.sequence {
            if let Some(ref gate) = action.gate {
                let resolved_run =
                    pu_core::trigger_def::substitute_variables(&gate.run, &trigger.variables);
                let max_retries = action.max_retries.unwrap_or(DEFAULT_GATE_MAX_RETRIES);
                let expect_exit = gate.expect_exit.unwrap_or(0);
                let mut passed = false;

                for attempt in 0..=max_retries {
                    if attempt > 0 {
                        all_output.push_str(&format!(
                            "  retry {attempt}/{max_retries}: {resolved_run}\n"
                        ));
                    }

                    match run_with(gate_shell(), &resolved_run, worktree_path).await {
                        Ok((exit_code, stdout, stderr)) => {
                            if exit_code == expect_exit {
                                passed = true;
                                break;
                            }
                            all_output.push_str(&format!(
                                "gate '{resolved_run}' exited {exit_code} (expected {expect_exit})\n"
                            ));
                            if !stdout.is_empty() {
                                all_output.push_str(&stdout);
                            }
                            if !stderr.is_empty() {
                                all_output.push_str(&stderr);
                            }
                        }
                        Err(e) => {
                            all_output.push_str(&format!("gate '{resolved_run}' error: {e}\n"));
                        }
                    }
                }

                if !passed {
                    return Ok(GateEvalResult {
                        passed: false,
                        output: all_output,
                        shell: gate_shell().name,
                    });
                }
            }
        }
    }

    Ok(GateEvalResult {
        passed: true,
        output: all_output,
        shell: gate_shell().name,
    })
}

pub async fn run_gate_command(
    command: &str,
    cwd: &Path,
) -> Result<(i32, String, String), Box<dyn std::error::Error + Send + Sync>> {
    run_with(gate_shell(), command, cwd).await
}

async fn run_with(
    shell: &GateShell,
    command: &str,
    cwd: &Path,
) -> Result<(i32, String, String), Box<dyn std::error::Error + Send + Sync>> {
    let output = tokio::time::timeout(
        GATE_TIMEOUT,
        shell
            .command(command)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        format!(
            "gate command timed out after {}s: {command}",
            GATE_TIMEOUT.as_secs()
        )
    })??;

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
        let result = evaluate_trigger_gates(&[trigger], &std::env::temp_dir())
            .await
            .unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn given_failing_gate_should_return_not_passed() {
        let trigger = make_gate_trigger("false", None);
        let result = evaluate_trigger_gates(&[trigger], &std::env::temp_dir())
            .await
            .unwrap();
        assert!(!result.passed);
        assert!(result.output.contains("exited 1"));
    }

    #[tokio::test]
    async fn given_custom_expect_exit_should_match() {
        let trigger = make_gate_trigger("exit 2", Some(2));
        let result = evaluate_trigger_gates(&[trigger], &std::env::temp_dir())
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
        let result = evaluate_trigger_gates(&[trigger], &std::env::temp_dir())
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
        let result = evaluate_trigger_gates(&[trigger], &std::env::temp_dir())
            .await
            .unwrap();
        assert!(!result.passed);
        // Should have retry output
        assert!(result.output.contains("retry"));
    }

    #[tokio::test]
    async fn given_gate_should_record_the_shell_it_ran_under() {
        let trigger = make_gate_trigger("exit 0", None);
        let result = evaluate_trigger_gates(&[trigger], &std::env::temp_dir())
            .await
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.shell, gate_shell().name);
        // Unix always has sh; Windows CI and dev machines have Git's sh.exe.
        assert_eq!(result.shell, "sh");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn given_no_git_sh_should_fall_back_to_powershell() {
        let empty = tempfile::tempdir().unwrap();
        let shell = resolve_gate_shell(empty.path().as_os_str().to_owned(), None);
        assert_eq!(shell.name, "powershell");
        let (code, _, _) = run_with(&shell, "exit 3", &std::env::temp_dir())
            .await
            .unwrap();
        assert_eq!(code, 3);
    }

    #[cfg(windows)]
    #[test]
    fn given_git_on_path_should_find_its_sh() {
        let Some(git) = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|d| d.join("git.exe"))
            .find(|p| p.is_file())
        else {
            return;
        };
        let dir = git.parent().unwrap().as_os_str().to_owned();
        let shell = resolve_gate_shell(dir, None);
        assert_eq!(shell.name, "sh", "{shell:?}");
        assert!(shell.program.is_file());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn given_pwsh_should_run_gate_with_no_profile_command() {
        let shell = GateShell {
            name: "pwsh",
            program: "pwsh.exe".into(),
        };
        if std::process::Command::new("pwsh.exe")
            .args(["-NoProfile", "-Command", "exit 0"])
            .status()
            .is_err()
        {
            eprintln!("pwsh not installed; skipping");
            return;
        }
        let (code, stdout, _) = run_with(&shell, "Write-Output ok; exit 4", &std::env::temp_dir())
            .await
            .unwrap();
        assert_eq!(code, 4);
        assert_eq!(stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn given_empty_triggers_should_pass() {
        let result = evaluate_trigger_gates(&[], &std::env::temp_dir())
            .await
            .unwrap();
        assert!(result.passed);
    }
}
