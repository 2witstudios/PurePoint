use crate::client;
use crate::commands::cwd_string;
use crate::daemon_ctrl;
use crate::error::CliError;
use chrono::Utc;
use owo_colors::OwoColorize;
use pu_core::protocol::{AgentStatusReport, Response};
use pu_core::types::{AgentStatus, WorktreeEntry};
use std::io::Write;
use std::path::Path;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const REFRESH_MS: u64 = 800;

/// RAII guard that restores the terminal on drop (including panics).
struct TermGuard;

impl TermGuard {
    fn enter() -> Self {
        // Hide cursor + enter alt screen
        print!("\x1b[?1049h\x1b[?25l");
        let _ = std::io::stdout().flush();
        Self
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        // Restore cursor + leave alt screen
        print!("\x1b[?25h\x1b[?1049l");
        let _ = std::io::stdout().flush();
    }
}

pub async fn run(socket: &Path, interval: Option<u64>) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let refresh_ms = interval.unwrap_or(REFRESH_MS);
    if refresh_ms == 0 {
        return Err(CliError::Other("--interval must be at least 1ms".into()));
    }
    let refresh = std::time::Duration::from_millis(refresh_ms);
    let mut tick: usize = 0;
    let mut stdout = std::io::stdout();
    let project_root = cwd_string()?;

    let _guard = TermGuard::enter();

    loop {
        match fetch_status(socket, &project_root).await {
            Ok((worktrees, agents)) => {
                render_dashboard(&mut stdout, &worktrees, &agents, tick)?;
            }
            Err(CliError::DaemonNotRunning) => {
                render_no_daemon(&mut stdout)?;
            }
            Err(e) => {
                render_error(&mut stdout, &e.to_string())?;
            }
        }
        tick = tick.wrapping_add(1);

        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(refresh) => {}
        }
    }

    Ok(())
}

async fn fetch_status(
    socket: &Path,
    project_root: &str,
) -> Result<(Vec<WorktreeEntry>, Vec<AgentStatusReport>), CliError> {
    let resp = client::send_request(
        socket,
        &pu_core::protocol::Request::Status {
            project_root: project_root.to_string(),
            agent_id: None,
        },
    )
    .await?;

    match resp {
        Response::StatusReport { worktrees, agents } => Ok((worktrees, agents)),
        Response::Error { code, message } => Err(CliError::DaemonError { code, message }),
        _ => Err(CliError::Other("unexpected response".into())),
    }
}

fn render_dashboard(
    stdout: &mut std::io::Stdout,
    worktrees: &[WorktreeEntry],
    root_agents: &[AgentStatusReport],
    tick: usize,
) -> Result<(), CliError> {
    let mut buf = String::with_capacity(4096);

    // Move cursor to top-left, clear screen
    buf.push_str("\x1b[H\x1b[2J");

    // Header
    let now = Utc::now().format("%H:%M:%S");
    buf.push_str(&format!(
        " {} {}\n",
        "pu watch".bold(),
        now.to_string().dimmed()
    ));

    // Count agents across all worktrees + root
    let mut streaming = 0usize;
    let mut waiting = 0usize;
    let mut done = 0usize;
    let mut broken = 0usize;

    for a in root_agents {
        count_agent(
            a.status,
            a.exit_code,
            &mut streaming,
            &mut waiting,
            &mut done,
            &mut broken,
        );
    }
    for wt in worktrees {
        for a in wt.agents.values() {
            count_agent(
                a.status,
                a.exit_code,
                &mut streaming,
                &mut waiting,
                &mut done,
                &mut broken,
            );
        }
    }

    let total = streaming + waiting + done + broken;

    // Summary bar
    buf.push_str(&format!(
        " {} agents  {}  {}  {}  {}\n\n",
        total.to_string().bold(),
        if streaming > 0 {
            format!("{streaming} streaming").green().to_string()
        } else {
            format!("{streaming} streaming").dimmed().to_string()
        },
        if waiting > 0 {
            format!("{waiting} waiting").cyan().to_string()
        } else {
            format!("{waiting} waiting").dimmed().to_string()
        },
        format!("{done} done").dimmed(),
        if broken > 0 {
            format!("{broken} broken").red().to_string()
        } else {
            format!("{broken} broken").dimmed().to_string()
        },
    ));

    if total == 0 {
        buf.push_str(&format!(
            " {}\n",
            "No agents running. Use `pu spawn` to start one.".dimmed()
        ));
    }

    // Root agents
    if !root_agents.is_empty() {
        buf.push_str(&format!(" {}\n", "Root agents".bold().underline()));
        render_agent_table(
            &mut buf,
            root_agents.iter().map(|a| AgentView {
                id: &a.id,
                name: &a.name,
                status: a.status,
                exit_code: a.exit_code,
                started_at: a.started_at,
                prompt: a.prompt.as_deref(),
                suspended: a.suspended,
            }),
            tick,
        );
        buf.push('\n');
    }

    // Worktrees
    for wt in worktrees {
        let wt_status = format!("{:?}", wt.status);
        buf.push_str(&format!(
            " {} {} {}\n",
            wt.name.bold().underline(),
            wt.branch.dimmed(),
            wt_status.dimmed()
        ));

        if wt.agents.is_empty() {
            buf.push_str(&format!("   {}\n", "(no agents)".dimmed()));
        } else {
            render_agent_table(
                &mut buf,
                wt.agents.values().map(|a| AgentView {
                    id: &a.id,
                    name: &a.name,
                    status: a.status,
                    exit_code: a.exit_code,
                    started_at: a.started_at,
                    prompt: a.prompt.as_deref(),
                    suspended: a.suspended,
                }),
                tick,
            );
        }
        buf.push('\n');
    }

    // Footer
    buf.push_str(&format!(" {}\n", "Press Ctrl+C to exit".dimmed()));

    print!("{buf}");
    stdout.flush().map_err(CliError::Io)
}

struct AgentView<'a> {
    id: &'a str,
    name: &'a str,
    status: AgentStatus,
    exit_code: Option<i32>,
    started_at: chrono::DateTime<Utc>,
    prompt: Option<&'a str>,
    suspended: bool,
}

fn render_agent_table<'a>(
    buf: &mut String,
    agents: impl Iterator<Item = AgentView<'a>>,
    tick: usize,
) {
    for agent in agents {
        let spinner = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];

        let (indicator, status_str) = match agent.status {
            AgentStatus::Streaming if agent.suspended => {
                ("⏸".to_string(), "suspended".yellow().to_string())
            }
            AgentStatus::Streaming => {
                (spinner.green().to_string(), "streaming".green().to_string())
            }
            AgentStatus::Waiting if agent.suspended => {
                ("⏸".to_string(), "suspended".yellow().to_string())
            }
            AgentStatus::Waiting => ("●".cyan().to_string(), "waiting".cyan().to_string()),
            AgentStatus::Broken => match agent.exit_code {
                Some(0) => ("✓".dimmed().to_string(), "done".dimmed().to_string()),
                _ => ("✗".red().to_string(), "broken".red().to_string()),
            },
        };

        let elapsed = format_elapsed(agent.started_at);

        let prompt_preview = agent
            .prompt
            .map(|p| {
                let sanitized = sanitize_prompt(p);
                if sanitized.chars().count() > 50 {
                    let truncated: String = sanitized.chars().take(49).collect();
                    format!("{truncated}…")
                } else {
                    sanitized
                }
            })
            .unwrap_or_default();

        buf.push_str(&format!(
            "   {} {:<12} {:<12} {:<10} {:<8} {}\n",
            indicator,
            agent.id.dimmed(),
            agent.name,
            status_str,
            elapsed.dimmed(),
            prompt_preview.dimmed(),
        ));
    }
}

fn format_elapsed(started_at: chrono::DateTime<Utc>) -> String {
    let elapsed = Utc::now().signed_duration_since(started_at);
    let secs = elapsed.num_seconds().max(0);

    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn count_agent(
    status: AgentStatus,
    exit_code: Option<i32>,
    streaming: &mut usize,
    waiting: &mut usize,
    done: &mut usize,
    broken: &mut usize,
) {
    match status {
        AgentStatus::Streaming => *streaming += 1,
        AgentStatus::Waiting => *waiting += 1,
        AgentStatus::Broken => match exit_code {
            Some(0) => *done += 1,
            _ => *broken += 1,
        },
    }
}

/// Strip ANSI escape sequences and control characters, collapse whitespace.
fn sanitize_prompt(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip CSI sequences: ESC [ <params> <letter>
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            result.push(' ');
        } else if c.is_control() {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_no_daemon(stdout: &mut std::io::Stdout) -> Result<(), CliError> {
    print!("\x1b[H\x1b[2J");
    print!(
        " {} {}\n\n {}\n",
        "pu watch".bold(),
        Utc::now().format("%H:%M:%S").to_string().dimmed(),
        "Daemon not running. Waiting...".yellow(),
    );
    stdout.flush().map_err(CliError::Io)
}

fn render_error(stdout: &mut std::io::Stdout, err: &str) -> Result<(), CliError> {
    print!("\x1b[H\x1b[2J");
    print!(
        " {} {}\n\n {} {}\n",
        "pu watch".bold(),
        Utc::now().format("%H:%M:%S").to_string().dimmed(),
        "Error:".red().bold(),
        err,
    );
    stdout.flush().map_err(CliError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_recent_start_time_format_elapsed_should_show_seconds() {
        let started = Utc::now() - chrono::Duration::seconds(42);
        let result = format_elapsed(started);
        assert_eq!(result, "42s");
    }

    #[test]
    fn given_minutes_elapsed_should_show_minutes_and_seconds() {
        let started = Utc::now() - chrono::Duration::seconds(125);
        let result = format_elapsed(started);
        assert_eq!(result, "2m5s");
    }

    #[test]
    fn given_hours_elapsed_should_show_hours_and_minutes() {
        let started = Utc::now() - chrono::Duration::seconds(7380);
        let result = format_elapsed(started);
        assert_eq!(result, "2h3m");
    }

    #[test]
    fn given_zero_seconds_should_show_0s() {
        let started = Utc::now();
        let result = format_elapsed(started);
        assert_eq!(result, "0s");
    }

    #[test]
    fn given_streaming_status_should_count_as_streaming() {
        let (mut s, mut w, mut d, mut b) = (0, 0, 0, 0);
        count_agent(AgentStatus::Streaming, None, &mut s, &mut w, &mut d, &mut b);
        assert_eq!((s, w, d, b), (1, 0, 0, 0));
    }

    #[test]
    fn given_waiting_status_should_count_as_waiting() {
        let (mut s, mut w, mut d, mut b) = (0, 0, 0, 0);
        count_agent(AgentStatus::Waiting, None, &mut s, &mut w, &mut d, &mut b);
        assert_eq!((s, w, d, b), (0, 1, 0, 0));
    }

    #[test]
    fn given_broken_with_exit_0_should_count_as_done() {
        let (mut s, mut w, mut d, mut b) = (0, 0, 0, 0);
        count_agent(AgentStatus::Broken, Some(0), &mut s, &mut w, &mut d, &mut b);
        assert_eq!((s, w, d, b), (0, 0, 1, 0));
    }

    #[test]
    fn given_broken_with_nonzero_exit_should_count_as_broken() {
        let (mut s, mut w, mut d, mut b) = (0, 0, 0, 0);
        count_agent(AgentStatus::Broken, Some(1), &mut s, &mut w, &mut d, &mut b);
        assert_eq!((s, w, d, b), (0, 0, 0, 1));
    }

    #[test]
    fn given_broken_with_no_exit_code_should_count_as_broken() {
        let (mut s, mut w, mut d, mut b) = (0, 0, 0, 0);
        count_agent(AgentStatus::Broken, None, &mut s, &mut w, &mut d, &mut b);
        assert_eq!((s, w, d, b), (0, 0, 0, 1));
    }

    #[test]
    fn given_prompt_with_control_chars_sanitize_should_strip_them() {
        let input = "hello\x1b[31mworld\rfoo\nbar";
        let result = sanitize_prompt(input);
        assert_eq!(result, "hello world foo bar");
    }

    #[test]
    fn given_prompt_with_only_whitespace_sanitize_should_return_empty() {
        assert_eq!(sanitize_prompt("  \n\t\r  "), "");
    }

    #[test]
    fn given_clean_prompt_sanitize_should_preserve_content() {
        assert_eq!(sanitize_prompt("fix the auth bug"), "fix the auth bug");
    }
}
