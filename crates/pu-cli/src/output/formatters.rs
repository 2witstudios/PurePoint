use owo_colors::OwoColorize;
use pu_core::protocol::AgentStatusReport;
use pu_core::types::{AgentStatus, TriggerState};

/// Return a colored status string for display (delegates with `suspended = false`).
pub(crate) fn status_colored(status: AgentStatus, exit_code: Option<i32>) -> String {
    status_colored_with_suspended(status, exit_code, false)
}

/// Return a colored status string, showing "benched" (yellow) for suspended alive agents.
pub(crate) fn status_colored_with_suspended(
    status: AgentStatus,
    exit_code: Option<i32>,
    suspended: bool,
) -> String {
    if suspended && status.is_alive() {
        return "benched".yellow().to_string();
    }
    match status {
        AgentStatus::Running => "running".green().to_string(),
        AgentStatus::Broken => match exit_code {
            Some(0) => "done".dimmed().to_string(),
            _ => "broken".red().to_string(),
        },
    }
}

pub(crate) fn trigger_progress(report: &AgentStatusReport) -> String {
    match (
        report.trigger_state,
        report.trigger_seq_index,
        report.trigger_total,
    ) {
        (Some(TriggerState::Active), Some(idx), Some(total)) => {
            format!(" [{}{}{}]", idx.to_string().cyan(), "/".dimmed(), total)
        }
        (Some(TriggerState::Gating), Some(idx), Some(total)) => {
            format!(
                " [{}{}{} {}]",
                idx.to_string().cyan(),
                "/".dimmed(),
                total,
                "gating".yellow()
            )
        }
        (Some(TriggerState::Completed), _, Some(total)) => {
            format!(" [{}{}{} {}]", total, "/".dimmed(), total, "done".green())
        }
        (Some(TriggerState::Failed), Some(idx), Some(total)) => {
            format!(" [{}{}{} {}]", idx, "/".dimmed(), total, "failed".red())
        }
        _ => String::new(),
    }
}

pub(crate) fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        format!("{h}h {m}m")
    }
}
