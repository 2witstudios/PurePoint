use std::path::Path;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, ScheduleTriggerPayload};

pub async fn run_list(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(socket, &Request::ListSchedules { project_root }).await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_create(
    socket: &Path,
    name: &str,
    recurrence: &str,
    start_at: &str,
    trigger_type: &str,
    trigger_name: Option<&str>,
    trigger_prompt: Option<&str>,
    agent: &str,
    vars: Vec<String>,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;

    let start_at_dt = parse_datetime(start_at)?;

    let trigger = match trigger_type {
        "agent-def" => {
            let n = trigger_name
                .ok_or_else(|| CliError::Other("--trigger-name required for agent-def".into()))?;
            ScheduleTriggerPayload::AgentDef {
                name: n.to_string(),
            }
        }
        "swarm-def" => {
            let n = trigger_name
                .ok_or_else(|| CliError::Other("--trigger-name required for swarm-def".into()))?;
            let var_map = commands::parse_vars(&vars)?;
            ScheduleTriggerPayload::SwarmDef {
                name: n.to_string(),
                vars: var_map,
            }
        }
        "inline-prompt" => {
            let p = trigger_prompt.ok_or_else(|| {
                CliError::Other("--trigger-prompt required for inline-prompt".into())
            })?;
            ScheduleTriggerPayload::InlinePrompt {
                prompt: p.to_string(),
                agent: agent.to_string(),
            }
        }
        other => return Err(CliError::Other(format!("unknown trigger type: {other}"))),
    };

    let resp = client::send_request(
        socket,
        &Request::SaveSchedule {
            project_root,
            name: name.to_string(),
            enabled: true,
            recurrence: recurrence.to_string(),
            start_at: start_at_dt,
            trigger,
            target: String::new(),
            scope: scope.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_show(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::GetSchedule {
            project_root,
            name: name.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_delete(
    socket: &Path,
    name: &str,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::DeleteSchedule {
            project_root,
            name: name.to_string(),
            scope: scope.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_enable(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::EnableSchedule {
            project_root,
            name: name.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_disable(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::DisableSchedule {
            project_root,
            name: name.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>, CliError> {
    // Try RFC 3339 first (e.g. "2025-06-15T03:00:00Z")
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Try naive datetime (e.g. "2025-06-15T08:00:00") — treated as local time
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Local
            .from_local_datetime(&naive)
            .earliest()
            .map(|dt| dt.with_timezone(&Utc))
            .ok_or_else(|| CliError::Other(format!("invalid local time: {s}")));
    }
    // Try date only (e.g. "2025-06-15") — treated as midnight local time
    if let Ok(naive) = NaiveDateTime::parse_from_str(&format!("{s}T00:00:00"), "%Y-%m-%dT%H:%M:%S")
    {
        return Local
            .from_local_datetime(&naive)
            .earliest()
            .map(|dt| dt.with_timezone(&Utc))
            .ok_or_else(|| CliError::Other(format!("invalid local time: {s}")));
    }
    Err(CliError::Other(format!(
        "invalid datetime: {s} (expected RFC 3339 or YYYY-MM-DDTHH:MM:SS)"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn given_rfc3339_should_parse() {
        let dt = parse_datetime("2025-06-15T03:00:00Z").unwrap();
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.hour(), 3); // UTC as specified
    }

    #[test]
    fn given_naive_datetime_should_parse_as_local_time() {
        let dt = parse_datetime("2025-06-15T03:00:00").unwrap();
        let local: DateTime<Local> = dt.with_timezone(&Local);
        assert_eq!(local.hour(), 3); // interpreted as local, not UTC
    }

    #[test]
    fn given_date_only_should_parse_as_local_midnight() {
        let dt = parse_datetime("2025-06-15").unwrap();
        let local: DateTime<Local> = dt.with_timezone(&Local);
        assert_eq!(local.day(), 15);
        assert_eq!(local.hour(), 0); // midnight local
    }

    #[test]
    fn given_invalid_should_return_error() {
        assert!(parse_datetime("not-a-date").is_err());
    }
}
