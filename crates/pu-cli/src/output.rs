use owo_colors::OwoColorize;
use pu_core::protocol::Response;
use pu_core::types::AgentStatus;

use crate::error::CliError;

/// Check a daemon response for errors. On error, print JSON if requested, then return Err.
pub fn check_response(resp: Response, json: bool) -> Result<Response, CliError> {
    match resp {
        Response::Error { code, message } => {
            if json {
                print_response(
                    &Response::Error {
                        code: code.clone(),
                        message: message.clone(),
                    },
                    true,
                );
            }
            Err(CliError::DaemonError { code, message })
        }
        other => Ok(other),
    }
}

fn status_colored(status: AgentStatus, exit_code: Option<i32>) -> String {
    match status {
        AgentStatus::Streaming => "streaming".green().to_string(),
        AgentStatus::Waiting => "waiting".cyan().to_string(),
        AgentStatus::Broken => match exit_code {
            Some(0) => "done".dimmed().to_string(),
            _ => "broken".red().to_string(),
        },
    }
}

pub fn print_response(response: &Response, json_mode: bool) {
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(response).expect("response JSON serialization failed")
        );
        return;
    }
    match response {
        Response::HealthReport {
            pid,
            uptime_seconds,
            protocol_version,
            projects,
            agent_count,
        } => {
            println!("{}", "Daemon healthy".green().bold());
            println!("  PID:      {pid}");
            println!("  Uptime:   {uptime_seconds}s");
            println!("  Protocol: v{protocol_version}");
            println!("  Projects: {}", projects.len());
            println!("  Agents:   {agent_count}");
        }
        Response::InitResult { created } => {
            if *created {
                println!("{}", "Initialized PurePoint workspace".green());
            } else {
                println!("Already initialized");
            }
        }
        Response::SpawnResult {
            worktree_id,
            agent_id,
            status,
        } => {
            println!(
                "Spawned agent {} ({})",
                agent_id.bold(),
                status_colored(*status, None)
            );
            if let Some(wt) = worktree_id {
                println!("  Worktree: {wt}");
            }
        }
        Response::StatusReport { worktrees, agents } => {
            if worktrees.is_empty() && agents.is_empty() {
                println!("No active agents");
                return;
            }
            if !agents.is_empty() {
                println!(
                    "{:<14} {:<16} {}",
                    "ID".bold(),
                    "NAME".bold(),
                    "STATUS".bold()
                );
                for a in agents {
                    println!(
                        "{:<14} {:<16} {}",
                        a.id.dimmed(),
                        a.name,
                        status_colored(a.status, a.exit_code)
                    );
                }
            }
            for wt in worktrees {
                println!(
                    "\n{} {} ({}) — {:?}",
                    "Worktree".bold(),
                    wt.id.dimmed(),
                    wt.branch,
                    wt.status,
                );
                if !wt.agents.is_empty() {
                    println!(
                        "  {:<14} {:<16} {}",
                        "ID".bold(),
                        "NAME".bold(),
                        "STATUS".bold()
                    );
                    for a in wt.agents.values() {
                        println!(
                            "  {:<14} {:<16} {}",
                            a.id.dimmed(),
                            a.name,
                            status_colored(a.status, a.exit_code),
                        );
                    }
                }
            }
        }
        Response::AgentStatus(a) => {
            println!(
                "{} {} {}",
                a.id.dimmed(),
                a.name.bold(),
                status_colored(a.status, a.exit_code)
            );
            if let Some(pid) = a.pid {
                println!("  PID:    {pid}");
            }
            if let Some(code) = a.exit_code {
                println!("  Exit:   {code}");
            }
            if let Some(idle) = a.idle_seconds {
                println!("  Idle:   {idle}s");
            }
            if let Some(ref wt) = a.worktree_id {
                println!("  Worktree: {wt}");
            }
        }
        Response::KillResult { killed, .. } => {
            println!("Killed {} agent(s)", killed.len());
        }
        Response::SuspendResult { suspended } => {
            println!("Suspended {} agent(s)", suspended.len());
        }
        Response::ResumeResult { agent_id, status } => {
            println!(
                "Resumed agent {} ({})",
                agent_id.bold(),
                status_colored(*status, None)
            );
        }
        Response::RenameResult { agent_id, name } => {
            println!("Renamed agent {} to {}", agent_id.bold(), name.green());
        }
        Response::CreateWorktreeResult { worktree_id } => {
            println!("Created worktree {}", worktree_id.bold());
        }
        Response::DeleteWorktreeResult {
            worktree_id,
            killed_agents,
            branch_deleted,
            remote_deleted,
        } => {
            println!("Deleted worktree {}", worktree_id.bold());
            if !killed_agents.is_empty() {
                println!("  Killed {} agent(s)", killed_agents.len());
            }
            println!(
                "  Branch deleted: {}",
                if *branch_deleted {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                }
            );
            println!(
                "  Remote deleted: {}",
                if *remote_deleted {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                }
            );
        }
        Response::LogsResult { agent_id, data } => {
            println!("{}", format!("--- Logs for {agent_id} ---").dimmed());
            print!("{data}");
        }
        Response::ShuttingDown => {
            println!("Daemon shutting down");
        }
        Response::Error { code, message } => {
            eprintln!("{} [{}]: {}", "error".red().bold(), code, message);
        }
        Response::Ok => {}
        Response::Output { data, .. } => {
            print!("{}", String::from_utf8_lossy(data));
        }
        Response::AttachReady { buffered_bytes } => {
            println!("Attached ({buffered_bytes} bytes buffered)");
        }
        Response::GridSubscribed => {
            println!("Grid subscription active");
        }
        Response::GridLayout { layout } => {
            println!(
                "{}",
                serde_json::to_string_pretty(layout).expect("layout JSON serialization failed")
            );
        }
        Response::GridEvent {
            project_root,
            command,
        } => {
            println!("Grid event for {project_root}: {command:?}");
        }
        Response::StatusSubscribed => {
            println!("Status subscription active");
        }
        Response::StatusEvent { agents, worktrees } => {
            println!(
                "Status update: {} agents, {} worktrees",
                agents.len(),
                worktrees.len()
            );
        }
        Response::TemplateList { templates } => {
            if templates.is_empty() {
                println!("No templates");
                return;
            }
            println!(
                "{:<20} {:<12} {:<10} {}",
                "NAME".bold(),
                "AGENT".bold(),
                "SOURCE".bold(),
                "VARIABLES".bold()
            );
            for t in templates {
                println!(
                    "{:<20} {:<12} {:<10} {}",
                    t.name,
                    t.agent,
                    t.source,
                    t.variables.join(", ")
                );
            }
        }
        Response::TemplateDetail {
            name,
            description,
            agent,
            body,
            source,
            variables,
        } => {
            println!("{} ({})", name.bold(), source.dimmed());
            if !description.is_empty() {
                println!("  {description}");
            }
            println!("  Agent: {agent}");
            if !variables.is_empty() {
                println!("  Variables: {}", variables.join(", "));
            }
            println!("---");
            print!("{body}");
        }
        Response::AgentDefDetail {
            name,
            agent_type,
            template,
            inline_prompt,
            tags,
            scope,
            available_in_command_dialog,
            icon,
        } => {
            println!("{} ({})", name.bold(), scope.dimmed());
            println!("  Type: {agent_type}");
            if let Some(tpl) = template {
                println!("  Template: {tpl}");
            }
            if let Some(prompt) = inline_prompt {
                println!("  Inline prompt:");
                for line in prompt.lines() {
                    println!("    {line}");
                }
            }
            if !tags.is_empty() {
                println!("  Tags: {}", tags.join(", "));
            }
            if let Some(ic) = icon {
                println!("  Icon: {ic}");
            }
            println!("  Command dialog: {available_in_command_dialog}");
        }
        Response::AgentDefList { agent_defs } => {
            if agent_defs.is_empty() {
                println!("No agent definitions");
                return;
            }
            println!(
                "{:<20} {:<12} {:<10}",
                "NAME".bold(),
                "TYPE".bold(),
                "SCOPE".bold()
            );
            for d in agent_defs {
                println!("{:<20} {:<12} {:<10}", d.name, d.agent_type, d.scope);
            }
        }
        Response::SwarmDefDetail {
            name,
            worktree_count,
            worktree_template,
            roster,
            include_terminal,
            scope,
        } => {
            println!("{} ({})", name.bold(), scope.dimmed());
            println!("  Worktrees: {worktree_count}");
            if !worktree_template.is_empty() {
                println!("  Template: {worktree_template}");
            }
            println!("  Terminal: {include_terminal}");
            if !roster.is_empty() {
                println!("  Roster:");
                for r in roster {
                    println!("    {} ({}) x{}", r.agent_def, r.role, r.quantity);
                }
            }
        }
        Response::SwarmDefList { swarm_defs } => {
            if swarm_defs.is_empty() {
                println!("No swarm definitions");
                return;
            }
            println!(
                "{:<20} {:<10} {:<10} {}",
                "NAME".bold(),
                "WORKTREES".bold(),
                "SCOPE".bold(),
                "ROSTER".bold()
            );
            for d in swarm_defs {
                let roster_summary: Vec<String> = d
                    .roster
                    .iter()
                    .map(|r| format!("{}x{}", r.agent_def, r.quantity))
                    .collect();
                println!(
                    "{:<20} {:<10} {:<10} {}",
                    d.name,
                    d.worktree_count,
                    d.scope,
                    roster_summary.join(", ")
                );
            }
        }
        Response::RunSwarmResult { spawned_agents } => {
            println!("Spawned {} agent(s)", spawned_agents.len());
            for id in spawned_agents {
                println!("  {}", id.dimmed());
            }
        }
        Response::RunSwarmPartial {
            spawned_agents,
            error_code,
            error_message,
        } => {
            println!(
                "{}: {error_message} ({error_code})",
                "Swarm partially failed".red().bold()
            );
            if !spawned_agents.is_empty() {
                println!("Spawned {} agent(s) before failure:", spawned_agents.len());
                for id in spawned_agents {
                    println!("  {}", id.dimmed());
                }
            }
        }
        Response::DiffResult { diffs } => {
            if diffs.is_empty() {
                println!("No worktree diffs");
                return;
            }
            for (i, d) in diffs.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                let base = d.base_branch.as_deref().unwrap_or("(unknown)");
                println!(
                    "{} {} ({} -> {})",
                    "Worktree".bold(),
                    d.worktree_name.bold(),
                    base.dimmed(),
                    d.branch.green()
                );
                if let Some(ref err) = d.error {
                    println!("  {}: {}", "error".red().bold(), err);
                } else if d.files_changed == 0 && d.diff_output.trim().is_empty() {
                    println!("  {}", "No changes".dimmed());
                } else {
                    println!(
                        "  {} file(s) changed, {} insertion(s), {} deletion(s)",
                        d.files_changed, d.insertions, d.deletions
                    );
                    if !d.diff_output.trim().is_empty() {
                        println!();
                        print!("{}", d.diff_output);
                    }
                }
            }
        }
        Response::ScheduleList { schedules } => {
            if schedules.is_empty() {
                println!("No schedules");
                return;
            }
            println!(
                "{:<20} {:<10} {:<10} {:<10} {}",
                "NAME".bold(),
                "RECURRENCE".bold(),
                "ENABLED".bold(),
                "SCOPE".bold(),
                "NEXT RUN".bold()
            );
            for s in schedules {
                let next = s
                    .next_run
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string());
                let enabled_str = if s.enabled {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                };
                println!(
                    "{:<20} {:<10} {:<10} {:<10} {}",
                    s.name, s.recurrence, enabled_str, s.scope, next
                );
            }
        }
        Response::ScheduleDetail {
            name,
            enabled,
            recurrence,
            start_at,
            next_run,
            trigger,
            scope,
            root,
            agent_name,
            ..
        } => {
            println!("{} ({})", name.bold(), scope.dimmed());
            println!("  Enabled:    {enabled}");
            println!("  Recurrence: {recurrence}");
            println!("  Root:       {root}");
            if let Some(an) = agent_name {
                println!("  Agent name: {an}");
            }
            println!("  Start at:   {}", start_at.format("%Y-%m-%d %H:%M UTC"));
            if let Some(nr) = next_run {
                println!("  Next run:   {}", nr.format("%Y-%m-%d %H:%M UTC"));
            }
            match trigger {
                pu_core::protocol::ScheduleTriggerPayload::AgentDef { name } => {
                    println!("  Trigger:    agent-def ({name})");
                }
                pu_core::protocol::ScheduleTriggerPayload::SwarmDef { name, vars } => {
                    println!("  Trigger:    swarm-def ({name})");
                    if !vars.is_empty() {
                        for (k, v) in vars {
                            println!("    {k}={v}");
                        }
                    }
                }
                pu_core::protocol::ScheduleTriggerPayload::InlinePrompt { prompt, agent } => {
                    println!("  Trigger:    inline-prompt ({agent})");
                    println!("  Prompt:     {prompt}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pu_core::protocol::{AgentStatusReport, GridCommand, PROTOCOL_VERSION};

    fn make_agent_report(id: &str, status: AgentStatus) -> AgentStatusReport {
        AgentStatusReport {
            id: id.into(),
            name: format!("{id}-name"),
            agent_type: "claude".into(),
            status,
            pid: Some(1234),
            exit_code: None,
            idle_seconds: None,
            worktree_id: None,
            started_at: chrono::Utc::now(),
            session_id: None,
            prompt: None,
            suspended: false,
        }
    }

    // --- check_response ---

    #[test]
    fn given_ok_response_check_response_should_return_ok() {
        let resp = Response::Ok;
        let result = check_response(resp, false);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Response::Ok));
    }

    #[test]
    fn given_error_response_check_response_should_return_err() {
        let resp = Response::Error {
            code: "NOT_FOUND".into(),
            message: "agent not found".into(),
        };
        let result = check_response(resp, false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("NOT_FOUND"));
        assert!(err.to_string().contains("agent not found"));
    }

    #[test]
    fn given_non_error_response_check_response_should_pass_through() {
        let resp = Response::ShuttingDown;
        let result = check_response(resp, false);
        assert!(matches!(result.unwrap(), Response::ShuttingDown));
    }

    #[test]
    fn given_error_response_in_json_mode_check_response_should_print_and_return_err() {
        let resp = Response::Error {
            code: "TEST_ERR".into(),
            message: "json mode error".into(),
        };
        let result = check_response(resp, true);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("TEST_ERR"));
    }

    // --- print_response (json mode) ---

    #[test]
    fn given_json_mode_should_produce_valid_json() {
        // Exercise the print_response JSON path (which calls serde internally)
        let resp = Response::InitResult { created: true };
        print_response(&resp, true);
        // Verify it round-trips through serde correctly
        let json = serde_json::to_string_pretty(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "init_result");
        assert_eq!(parsed["created"], true);
    }

    // --- print_response (human mode, smoke tests that they don't panic) ---

    #[test]
    fn given_health_report_should_not_panic() {
        let resp = Response::HealthReport {
            pid: 42,
            uptime_seconds: 3600,
            protocol_version: PROTOCOL_VERSION,
            projects: vec!["/test".into()],
            agent_count: 3,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_init_result_created_should_not_panic() {
        print_response(&Response::InitResult { created: true }, false);
    }

    #[test]
    fn given_init_result_already_should_not_panic() {
        print_response(&Response::InitResult { created: false }, false);
    }

    #[test]
    fn given_spawn_result_with_worktree_should_not_panic() {
        let resp = Response::SpawnResult {
            worktree_id: Some("wt-abc".into()),
            agent_id: "ag-xyz".into(),
            status: AgentStatus::Streaming,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_spawn_result_without_worktree_should_not_panic() {
        let resp = Response::SpawnResult {
            worktree_id: None,
            agent_id: "ag-xyz".into(),
            status: AgentStatus::Waiting,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_empty_status_report_should_not_panic() {
        let resp = Response::StatusReport {
            worktrees: vec![],
            agents: vec![],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_status_report_with_agents_should_not_panic() {
        let resp = Response::StatusReport {
            worktrees: vec![],
            agents: vec![
                make_agent_report("ag-1", AgentStatus::Streaming),
                make_agent_report("ag-2", AgentStatus::Broken),
            ],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_status_report_with_worktree_should_not_panic() {
        let now = chrono::Utc::now();
        // Deserialize a worktree entry from JSON to avoid needing indexmap dependency
        let wt_json = serde_json::json!({
            "id": "wt-1",
            "name": "test",
            "path": "/tmp",
            "branch": "pu/test",
            "baseBranch": null,
            "status": "active",
            "agents": {
                "ag-1": {
                    "id": "ag-1",
                    "name": "claude",
                    "agentType": "claude",
                    "status": "streaming",
                    "prompt": null,
                    "startedAt": now.to_rfc3339()
                }
            },
            "createdAt": now.to_rfc3339(),
            "mergedAt": null
        });
        let wt: pu_core::types::WorktreeEntry = serde_json::from_value(wt_json).unwrap();
        let resp = Response::StatusReport {
            worktrees: vec![wt],
            agents: vec![],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_agent_status_should_not_panic() {
        let resp = Response::AgentStatus(make_agent_report("ag-1", AgentStatus::Waiting));
        print_response(&resp, false);
    }

    #[test]
    fn given_kill_result_should_not_panic() {
        let resp = Response::KillResult {
            killed: vec!["ag-1".into(), "ag-2".into()],
            exit_codes: std::collections::HashMap::new(),
            skipped: vec![],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_suspend_result_should_not_panic() {
        let resp = Response::SuspendResult {
            suspended: vec!["ag-1".into()],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_resume_result_should_not_panic() {
        let resp = Response::ResumeResult {
            agent_id: "ag-1".into(),
            status: AgentStatus::Streaming,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_rename_result_should_not_panic() {
        let resp = Response::RenameResult {
            agent_id: "ag-1".into(),
            name: "new-name".into(),
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_delete_worktree_result_should_not_panic() {
        let resp = Response::DeleteWorktreeResult {
            worktree_id: "wt-1".into(),
            killed_agents: vec!["ag-1".into()],
            branch_deleted: true,
            remote_deleted: false,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_logs_result_should_not_panic() {
        let resp = Response::LogsResult {
            agent_id: "ag-1".into(),
            data: "some log output\n".into(),
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_shutting_down_should_not_panic() {
        print_response(&Response::ShuttingDown, false);
    }

    #[test]
    fn given_error_response_should_not_panic() {
        let resp = Response::Error {
            code: "ERR".into(),
            message: "something failed".into(),
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_ok_response_should_not_panic() {
        print_response(&Response::Ok, false);
    }

    #[test]
    fn given_output_response_should_not_panic() {
        let resp = Response::Output {
            agent_id: "ag-1".into(),
            data: b"hello world".to_vec(),
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_attach_ready_should_not_panic() {
        let resp = Response::AttachReady {
            buffered_bytes: 1024,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_grid_subscribed_should_not_panic() {
        print_response(&Response::GridSubscribed, false);
    }

    #[test]
    fn given_grid_layout_should_not_panic() {
        let resp = Response::GridLayout {
            layout: serde_json::json!({"root": {"type": "leaf", "id": 1}}),
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_grid_event_should_not_panic() {
        let resp = Response::GridEvent {
            project_root: "/test".into(),
            command: GridCommand::GetLayout,
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_status_subscribed_should_not_panic() {
        print_response(&Response::StatusSubscribed, false);
    }

    #[test]
    fn given_status_event_should_not_panic() {
        let resp = Response::StatusEvent {
            agents: vec![],
            worktrees: vec![],
        };
        print_response(&resp, false);
    }

    // --- status_colored ---

    #[test]
    fn given_broken_with_exit_0_should_show_done() {
        let s = status_colored(AgentStatus::Broken, Some(0));
        assert!(s.contains("done"));
    }

    #[test]
    fn given_broken_with_nonzero_exit_should_show_broken() {
        let s = status_colored(AgentStatus::Broken, Some(1));
        assert!(s.contains("broken"));
    }

    #[test]
    fn given_broken_with_no_exit_should_show_broken() {
        let s = status_colored(AgentStatus::Broken, None);
        assert!(s.contains("broken"));
    }

    #[test]
    fn given_streaming_should_show_streaming() {
        let s = status_colored(AgentStatus::Streaming, None);
        assert!(s.contains("streaming"));
    }

    #[test]
    fn given_waiting_should_show_waiting() {
        let s = status_colored(AgentStatus::Waiting, None);
        assert!(s.contains("waiting"));
    }

    // --- diff output ---

    #[test]
    fn given_diff_result_should_not_panic() {
        let resp = Response::DiffResult {
            diffs: vec![pu_core::protocol::WorktreeDiffEntry {
                worktree_id: "wt-1".into(),
                worktree_name: "fix-bug".into(),
                branch: "pu/fix-bug".into(),
                base_branch: Some("main".into()),
                diff_output: "+line\n".into(),
                files_changed: 1,
                insertions: 1,
                deletions: 0,
                error: None,
            }],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_empty_diff_result_should_not_panic() {
        let resp = Response::DiffResult { diffs: vec![] };
        print_response(&resp, false);
    }

    #[test]
    fn given_diff_result_no_changes_should_not_panic() {
        let resp = Response::DiffResult {
            diffs: vec![pu_core::protocol::WorktreeDiffEntry {
                worktree_id: "wt-1".into(),
                worktree_name: "clean".into(),
                branch: "pu/clean".into(),
                base_branch: None,
                diff_output: String::new(),
                files_changed: 0,
                insertions: 0,
                deletions: 0,
                error: None,
            }],
        };
        print_response(&resp, false);
    }

    // --- schedule output ---

    #[test]
    fn given_schedule_list_response_should_not_panic() {
        let resp = Response::ScheduleList {
            schedules: vec![pu_core::protocol::ScheduleInfo {
                name: "nightly".into(),
                enabled: true,
                recurrence: "daily".into(),
                start_at: chrono::Utc::now(),
                next_run: Some(chrono::Utc::now()),
                trigger: pu_core::protocol::ScheduleTriggerPayload::AgentDef {
                    name: "reviewer".into(),
                },
                project_root: "/test".into(),
                target: String::new(),
                scope: "local".into(),
                root: true,
                agent_name: None,
                created_at: chrono::Utc::now(),
            }],
        };
        print_response(&resp, false);
    }

    #[test]
    fn given_empty_schedule_list_should_not_panic() {
        let resp = Response::ScheduleList { schedules: vec![] };
        print_response(&resp, false);
    }

    #[test]
    fn given_schedule_detail_response_should_not_panic() {
        let resp = Response::ScheduleDetail {
            name: "nightly".into(),
            enabled: true,
            recurrence: "daily".into(),
            start_at: chrono::Utc::now(),
            next_run: None,
            trigger: pu_core::protocol::ScheduleTriggerPayload::InlinePrompt {
                prompt: "Review deps".into(),
                agent: "claude".into(),
            },
            project_root: "/test".into(),
            target: String::new(),
            scope: "local".into(),
            root: true,
            agent_name: None,
            created_at: chrono::Utc::now(),
        };
        print_response(&resp, false);
    }
}
