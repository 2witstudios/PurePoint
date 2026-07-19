mod agents;
mod definitions;
mod execution;
mod formatters;
mod system;

#[cfg(test)]
mod tests;

use pu_core::protocol::Response;

use crate::error::CliError;

// Re-imports for test visibility via `use super::*`
#[cfg(test)]
use {
    formatters::{format_duration, status_colored, status_colored_with_suspended},
    pu_core::types::AgentStatus,
};

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
                )?;
            }
            Err(CliError::DaemonError { code, message })
        }
        other => Ok(other),
    }
}

pub fn print_response(response: &Response, json_mode: bool) -> Result<(), CliError> {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(response)?);
        return Ok(());
    }
    match response {
        Response::HealthReport {
            pid,
            uptime_seconds,
            protocol_version,
            projects,
            agent_count,
        } => {
            system::print_health_report(
                *pid,
                *uptime_seconds,
                *protocol_version,
                projects,
                *agent_count,
            );
        }
        Response::InitResult { created } => {
            system::print_init_result(*created);
        }
        Response::SpawnResult {
            worktree_id,
            agent_id,
            status,
        } => {
            agents::print_spawn_result(worktree_id, agent_id, *status);
        }
        Response::StatusReport { worktrees, agents } => {
            agents::print_status_report(worktrees, agents);
        }
        Response::AgentStatus(a) => {
            agents::print_agent_status(a);
        }
        Response::KillResult { killed, .. } => {
            agents::print_kill_result(killed);
        }
        Response::SuspendResult { suspended } => {
            agents::print_suspend_result(suspended);
        }
        Response::ResumeResult { agent_id, status } => {
            agents::print_resume_result(agent_id, *status);
        }
        Response::RenameResult { agent_id, name } => {
            agents::print_rename_result(agent_id, name);
        }
        Response::AssignTriggerResult {
            agent_id,
            trigger_name,
            sequence_len,
        } => {
            agents::print_assign_trigger_result(agent_id, trigger_name, *sequence_len);
        }
        Response::CreateWorktreeResult { worktree_id } => {
            agents::print_create_worktree_result(worktree_id);
        }
        Response::DeleteWorktreeResult {
            worktree_id,
            killed_agents,
            branch_deleted,
            remote_deleted,
            directory_removed,
            error,
        } => {
            agents::print_delete_worktree_result(
                worktree_id,
                killed_agents,
                *branch_deleted,
                *remote_deleted,
                *directory_removed,
                error.as_deref(),
            );
        }
        Response::LogsResult { agent_id, data } => {
            agents::print_logs_result(agent_id, data);
        }
        Response::ShuttingDown => {
            system::print_shutting_down();
        }
        Response::Error { code, message } => {
            system::print_error(code, message);
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
            println!("{}", serde_json::to_string_pretty(layout)?);
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
            definitions::print_template_list(templates);
        }
        Response::TemplateDetail {
            name,
            description,
            agent,
            body,
            source,
            variables,
            command,
        } => {
            definitions::print_template_detail(
                name,
                description,
                agent,
                body,
                source,
                variables,
                command,
            );
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
            command,
        } => {
            definitions::print_agent_def_detail(
                name,
                agent_type,
                template,
                inline_prompt,
                tags,
                scope,
                *available_in_command_dialog,
                icon,
                command,
            );
        }
        Response::AgentDefList { agent_defs } => {
            definitions::print_agent_def_list(agent_defs);
        }
        Response::SwarmDefDetail {
            name,
            worktree_count,
            worktree_template,
            roster,
            include_terminal,
            scope,
        } => {
            definitions::print_swarm_def_detail(
                name,
                *worktree_count,
                worktree_template,
                roster,
                *include_terminal,
                scope,
            );
        }
        Response::SwarmDefList { swarm_defs } => {
            definitions::print_swarm_def_list(swarm_defs);
        }
        Response::RunSwarmResult { spawned_agents } => {
            execution::print_run_swarm_result(spawned_agents);
        }
        Response::RunSwarmPartial {
            spawned_agents,
            error_code,
            error_message,
        } => {
            execution::print_run_swarm_partial(spawned_agents, error_code, error_message);
        }
        Response::DiffResult { diffs } => {
            execution::print_diff_result(diffs);
        }
        Response::PulseReport {
            worktrees,
            root_agents,
        } => {
            execution::print_pulse_report(worktrees, root_agents);
        }
        Response::ScheduleList { schedules } => {
            execution::print_schedule_list(schedules);
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
            execution::print_schedule_detail(
                name, *enabled, recurrence, start_at, next_run, trigger, scope, *root, agent_name,
            );
        }
        Response::TriggerList { triggers } => {
            execution::print_trigger_list(triggers);
        }
        Response::TriggerDetail(t) => {
            execution::print_trigger_detail(t);
        }
        Response::GateResult { passed, output } => {
            execution::print_gate_result(*passed, output);
        }
        Response::ConfigReport {
            default_agent,
            agents,
        } => {
            system::print_config_report(default_agent, agents);
        }
    }
    Ok(())
}
