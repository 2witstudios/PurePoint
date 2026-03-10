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
        trigger_seq_index: None,
        trigger_state: None,
        trigger_total: None,
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
    print_response(&resp, true).unwrap();
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
    print_response(&resp, false).unwrap();
}

#[test]
fn given_init_result_created_should_not_panic() {
    print_response(&Response::InitResult { created: true }, false).unwrap();
}

#[test]
fn given_init_result_already_should_not_panic() {
    print_response(&Response::InitResult { created: false }, false).unwrap();
}

#[test]
fn given_spawn_result_with_worktree_should_not_panic() {
    let resp = Response::SpawnResult {
        worktree_id: Some("wt-abc".into()),
        agent_id: "ag-xyz".into(),
        status: AgentStatus::Streaming,
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_spawn_result_without_worktree_should_not_panic() {
    let resp = Response::SpawnResult {
        worktree_id: None,
        agent_id: "ag-xyz".into(),
        status: AgentStatus::Waiting,
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_empty_status_report_should_not_panic() {
    let resp = Response::StatusReport {
        worktrees: vec![],
        agents: vec![],
    };
    print_response(&resp, false).unwrap();
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
    print_response(&resp, false).unwrap();
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
    print_response(&resp, false).unwrap();
}

#[test]
fn given_agent_status_should_not_panic() {
    let resp = Response::AgentStatus(make_agent_report("ag-1", AgentStatus::Waiting));
    print_response(&resp, false).unwrap();
}

#[test]
fn given_kill_result_should_not_panic() {
    let resp = Response::KillResult {
        killed: vec!["ag-1".into(), "ag-2".into()],
        exit_codes: std::collections::HashMap::new(),
        skipped: vec![],
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_suspend_result_should_not_panic() {
    let resp = Response::SuspendResult {
        suspended: vec!["ag-1".into()],
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_empty_suspend_result_should_not_panic() {
    let resp = Response::SuspendResult { suspended: vec![] };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_resume_result_should_not_panic() {
    let resp = Response::ResumeResult {
        agent_id: "ag-1".into(),
        status: AgentStatus::Streaming,
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_rename_result_should_not_panic() {
    let resp = Response::RenameResult {
        agent_id: "ag-1".into(),
        name: "new-name".into(),
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_delete_worktree_result_should_not_panic() {
    let resp = Response::DeleteWorktreeResult {
        worktree_id: "wt-1".into(),
        killed_agents: vec!["ag-1".into()],
        branch_deleted: true,
        remote_deleted: false,
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_logs_result_should_not_panic() {
    let resp = Response::LogsResult {
        agent_id: "ag-1".into(),
        data: "some log output\n".into(),
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_shutting_down_should_not_panic() {
    print_response(&Response::ShuttingDown, false).unwrap();
}

#[test]
fn given_error_response_should_not_panic() {
    let resp = Response::Error {
        code: "ERR".into(),
        message: "something failed".into(),
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_ok_response_should_not_panic() {
    print_response(&Response::Ok, false).unwrap();
}

#[test]
fn given_output_response_should_not_panic() {
    let resp = Response::Output {
        agent_id: "ag-1".into(),
        data: b"hello world".to_vec(),
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_attach_ready_should_not_panic() {
    let resp = Response::AttachReady {
        buffered_bytes: 1024,
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_grid_subscribed_should_not_panic() {
    print_response(&Response::GridSubscribed, false).unwrap();
}

#[test]
fn given_grid_layout_should_not_panic() {
    let resp = Response::GridLayout {
        layout: serde_json::json!({"root": {"type": "leaf", "id": 1}}),
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_grid_event_should_not_panic() {
    let resp = Response::GridEvent {
        project_root: "/test".into(),
        command: GridCommand::GetLayout,
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_status_subscribed_should_not_panic() {
    print_response(&Response::StatusSubscribed, false).unwrap();
}

#[test]
fn given_status_event_should_not_panic() {
    let resp = Response::StatusEvent {
        agents: vec![],
        worktrees: vec![],
    };
    print_response(&resp, false).unwrap();
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

// --- status_colored_with_suspended (bench) ---

#[test]
fn given_suspended_streaming_should_show_benched() {
    let s = status_colored_with_suspended(AgentStatus::Streaming, None, true);
    assert!(s.contains("benched"));
}

#[test]
fn given_suspended_waiting_should_show_benched() {
    let s = status_colored_with_suspended(AgentStatus::Waiting, None, true);
    assert!(s.contains("benched"));
}

#[test]
fn given_suspended_broken_should_not_show_benched() {
    let s = status_colored_with_suspended(AgentStatus::Broken, Some(0), true);
    assert!(!s.contains("benched"));
    assert!(s.contains("done"));
}

#[test]
fn given_not_suspended_should_show_normal_status() {
    let s = status_colored_with_suspended(AgentStatus::Streaming, None, false);
    assert!(s.contains("streaming"));
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
    print_response(&resp, false).unwrap();
}

#[test]
fn given_empty_diff_result_should_not_panic() {
    let resp = Response::DiffResult { diffs: vec![] };
    print_response(&resp, false).unwrap();
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
    print_response(&resp, false).unwrap();
}

// --- pulse output ---

#[test]
fn given_pulse_report_should_not_panic() {
    let resp = Response::PulseReport {
        worktrees: vec![pu_core::protocol::WorktreePulseEntry {
            worktree_id: "wt-1".into(),
            worktree_name: "feature-5".into(),
            branch: "pu/feature-5".into(),
            elapsed_seconds: 3661,
            agents: vec![pu_core::protocol::AgentPulseEntry {
                id: "ag-1".into(),
                name: "claude".into(),
                agent_type: "claude".into(),
                status: AgentStatus::Streaming,
                exit_code: None,
                runtime_seconds: 120,
                idle_seconds: Some(5),
                prompt_snippet: Some("Add pulse command to CLI".into()),
            }],
            files_changed: 3,
            insertions: 42,
            deletions: 7,
            diff_error: None,
        }],
        root_agents: vec![pu_core::protocol::AgentPulseEntry {
            id: "ag-2".into(),
            name: "point-guard".into(),
            agent_type: "claude".into(),
            status: AgentStatus::Waiting,
            exit_code: None,
            runtime_seconds: 7200,
            idle_seconds: Some(30),
            prompt_snippet: None,
        }],
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_empty_pulse_report_should_not_panic() {
    let resp = Response::PulseReport {
        worktrees: vec![],
        root_agents: vec![],
    };
    print_response(&resp, false).unwrap();
}

#[test]
fn given_pulse_report_json_should_produce_valid_json() {
    let resp = Response::PulseReport {
        worktrees: vec![pu_core::protocol::WorktreePulseEntry {
            worktree_id: "wt-1".into(),
            worktree_name: "test".into(),
            branch: "pu/test".into(),
            elapsed_seconds: 60,
            agents: vec![],
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            diff_error: None,
        }],
        root_agents: vec![],
    };
    let json = serde_json::to_string_pretty(&resp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "pulse_report");
}

#[test]
fn given_format_duration_under_60s() {
    assert_eq!(format_duration(45), "45s");
}

#[test]
fn given_format_duration_minutes() {
    assert_eq!(format_duration(125), "2m 5s");
}

#[test]
fn given_format_duration_hours() {
    assert_eq!(format_duration(3661), "1h 1m");
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
    print_response(&resp, false).unwrap();
}

#[test]
fn given_empty_schedule_list_should_not_panic() {
    let resp = Response::ScheduleList { schedules: vec![] };
    print_response(&resp, false).unwrap();
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
    print_response(&resp, false).unwrap();
}
