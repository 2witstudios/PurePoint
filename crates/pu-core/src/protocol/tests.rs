use super::*;

// --- Request round-trips ---

#[test]
fn given_health_request_should_round_trip() {
    let req = Request::Health;
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, Request::Health));
}

#[test]
fn given_init_request_should_round_trip() {
    let req = Request::Init {
        project_root: "/test".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Init { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected Init"),
    }
}

#[test]
fn given_spawn_request_should_default_agent_to_claude() {
    let json = r#"{"type":"spawn","project_root":"/test","prompt":"fix bug"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Spawn {
            agent, name, root, ..
        } => {
            assert_eq!(agent, "claude");
            assert!(name.is_none());
            assert!(!root);
        }
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_with_all_fields_should_round_trip() {
    let req = Request::Spawn {
        project_root: "/test".into(),
        prompt: "fix auth".into(),
        agent: "codex".into(),
        name: Some("fix-auth".into()),
        base: Some("develop".into()),
        root: false,
        worktree: None,
        command: None,
        no_auto: false,
        extra_args: vec![],
        plan_mode: false,
        no_trigger: false,
        trigger: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Spawn {
            agent, name, base, ..
        } => {
            assert_eq!(agent, "codex");
            assert_eq!(name.unwrap(), "fix-auth");
            assert_eq!(base.unwrap(), "develop");
        }
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_without_no_auto_should_default_to_false() {
    let json = r#"{"type":"spawn","project_root":"/test","prompt":"fix bug"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Spawn { no_auto, .. } => assert!(!no_auto),
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_should_default_plan_mode_to_false() {
    let json = r#"{"type":"spawn","project_root":"/test","prompt":"fix bug"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Spawn { plan_mode, .. } => assert!(!plan_mode),
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_with_no_auto_true_should_round_trip() {
    let req = Request::Spawn {
        project_root: "/test".into(),
        prompt: "fix".into(),
        agent: "claude".into(),
        name: None,
        base: None,
        root: true,
        worktree: None,
        command: None,
        no_auto: true,
        extra_args: vec![],
        plan_mode: false,
        no_trigger: false,
        trigger: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Spawn { no_auto, .. } => assert!(no_auto),
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_without_extra_args_should_default_to_empty() {
    let json = r#"{"type":"spawn","project_root":"/test","prompt":"fix bug"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Spawn { extra_args, .. } => assert!(extra_args.is_empty()),
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_with_extra_args_should_round_trip() {
    let req = Request::Spawn {
        project_root: "/test".into(),
        prompt: "fix".into(),
        agent: "claude".into(),
        name: None,
        base: None,
        root: true,
        worktree: None,
        command: None,
        no_auto: false,
        extra_args: vec!["--model".into(), "opus".into()],
        plan_mode: false,
        no_trigger: false,
        trigger: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Spawn { extra_args, .. } => {
            assert_eq!(extra_args, vec!["--model", "opus"]);
        }
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_spawn_request_with_plan_mode_should_round_trip() {
    let req = Request::Spawn {
        project_root: "/test".into(),
        prompt: "research auth".into(),
        agent: "claude".into(),
        name: Some("plan-auth".into()),
        base: None,
        root: false,
        worktree: None,
        command: None,
        no_auto: false,
        extra_args: vec![],
        plan_mode: true,
        no_trigger: false,
        trigger: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Spawn { plan_mode, .. } => assert!(plan_mode),
        _ => panic!("expected Spawn"),
    }
}

#[test]
fn given_status_request_with_agent_id_should_round_trip() {
    let req = Request::Status {
        project_root: "/test".into(),
        agent_id: Some("ag-abc".into()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Status { agent_id, .. } => assert_eq!(agent_id.unwrap(), "ag-abc"),
        _ => panic!("expected Status"),
    }
}

#[test]
fn given_kill_request_with_agent_target_should_round_trip() {
    let req = Request::Kill {
        project_root: "/test".into(),
        target: KillTarget::Agent("ag-abc".into()),
        exclude: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Kill {
            target: KillTarget::Agent(id),
            ..
        } => assert_eq!(id, "ag-abc"),
        _ => panic!("expected Kill with Agent target"),
    }
}

#[test]
fn given_kill_target_all_should_round_trip() {
    let target = KillTarget::All;
    let json = serde_json::to_string(&target).unwrap();
    let parsed: KillTarget = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, KillTarget::All));
}

#[test]
fn given_logs_request_should_default_tail_to_500() {
    let json = r#"{"type":"logs","agent_id":"ag-abc"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Logs { tail, .. } => assert_eq!(tail, 500),
        _ => panic!("expected Logs"),
    }
}

#[test]
fn given_attach_request_should_round_trip() {
    let req = Request::Attach {
        agent_id: "ag-abc".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Attach { agent_id } => assert_eq!(agent_id, "ag-abc"),
        _ => panic!("expected Attach"),
    }
}

#[test]
fn given_shutdown_request_should_round_trip() {
    let req = Request::Shutdown;
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, Request::Shutdown));
}

#[test]
fn given_diff_request_should_round_trip() {
    let req = Request::Diff {
        project_root: "/test".into(),
        worktree_id: Some("wt-abc".into()),
        stat: true,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Diff {
            project_root,
            worktree_id,
            stat,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(worktree_id.unwrap(), "wt-abc");
            assert!(stat);
        }
        _ => panic!("expected Diff"),
    }
}

#[test]
fn given_diff_request_with_defaults_should_round_trip() {
    let json = r#"{"type":"diff","project_root":"/test"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Diff {
            worktree_id, stat, ..
        } => {
            assert!(worktree_id.is_none());
            assert!(!stat);
        }
        _ => panic!("expected Diff"),
    }
}

#[test]
fn given_diff_result_should_round_trip() {
    let resp = Response::DiffResult {
        diffs: vec![WorktreeDiffEntry {
            worktree_id: "wt-1".into(),
            worktree_name: "fix-bug".into(),
            branch: "pu/fix-bug".into(),
            base_branch: Some("main".into()),
            diff_output: "+added line\n-removed line\n".into(),
            files_changed: 2,
            insertions: 5,
            deletions: 3,
            error: None,
        }],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::DiffResult { diffs } => {
            assert_eq!(diffs.len(), 1);
            assert_eq!(diffs[0].worktree_id, "wt-1");
            assert_eq!(diffs[0].files_changed, 2);
            assert_eq!(diffs[0].insertions, 5);
            assert_eq!(diffs[0].deletions, 3);
        }
        _ => panic!("expected DiffResult"),
    }
}

#[test]
fn given_empty_diff_result_should_round_trip() {
    let resp = Response::DiffResult { diffs: vec![] };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::DiffResult { diffs } => assert!(diffs.is_empty()),
        _ => panic!("expected DiffResult"),
    }
}

// --- Response round-trips ---

#[test]
fn given_health_report_should_round_trip() {
    let resp = Response::HealthReport {
        pid: 1234,
        uptime_seconds: 3600,
        protocol_version: PROTOCOL_VERSION,
        projects: vec!["/test".into()],
        agent_count: 5,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::HealthReport {
            pid,
            protocol_version,
            ..
        } => {
            assert_eq!(pid, 1234);
            assert_eq!(protocol_version, PROTOCOL_VERSION);
        }
        _ => panic!("expected HealthReport"),
    }
}

#[test]
fn given_error_response_should_round_trip() {
    let resp = Response::Error {
        code: "NOT_INITIALIZED".into(),
        message: "run pu init".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::Error { code, message } => {
            assert_eq!(code, "NOT_INITIALIZED");
            assert_eq!(message, "run pu init");
        }
        _ => panic!("expected Error"),
    }
}

#[test]
fn given_spawn_result_should_round_trip() {
    let resp = Response::SpawnResult {
        worktree_id: Some("wt-abc".into()),
        agent_id: "ag-xyz".into(),
        status: crate::types::AgentStatus::Running,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::SpawnResult {
            worktree_id,
            agent_id,
            ..
        } => {
            assert_eq!(worktree_id.unwrap(), "wt-abc");
            assert_eq!(agent_id, "ag-xyz");
        }
        _ => panic!("expected SpawnResult"),
    }
}

#[test]
fn given_kill_result_should_round_trip() {
    let mut exit_codes = std::collections::HashMap::new();
    exit_codes.insert("ag-abc".to_string(), Some(0i32));
    let resp = Response::KillResult {
        killed: vec!["ag-abc".into()],
        exit_codes,
        skipped: vec![],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::KillResult {
            killed,
            exit_codes,
            skipped,
        } => {
            assert_eq!(killed, vec!["ag-abc"]);
            assert_eq!(exit_codes["ag-abc"], Some(0));
            assert!(skipped.is_empty());
        }
        _ => panic!("expected KillResult"),
    }
}

#[test]
fn given_kill_result_with_skipped_should_round_trip() {
    let resp = Response::KillResult {
        killed: vec!["ag-abc".into()],
        exit_codes: std::collections::HashMap::new(),
        skipped: vec!["ag-root".into()],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::KillResult { skipped, .. } => {
            assert_eq!(skipped, vec!["ag-root"]);
        }
        _ => panic!("expected KillResult"),
    }
}

#[test]
fn given_kill_result_without_skipped_field_should_default_empty() {
    let json = r#"{"type":"kill_result","killed":["ag-abc"],"exit_codes":{}}"#;
    let parsed: Response = serde_json::from_str(json).unwrap();
    match parsed {
        Response::KillResult { skipped, .. } => {
            assert!(skipped.is_empty());
        }
        _ => panic!("expected KillResult"),
    }
}

#[test]
fn given_kill_target_all_worktrees_should_round_trip() {
    let target = KillTarget::AllWorktrees;
    let json = serde_json::to_string(&target).unwrap();
    let parsed: KillTarget = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, KillTarget::AllWorktrees));
}

#[test]
fn given_kill_request_with_exclude_should_round_trip() {
    let req = Request::Kill {
        project_root: "/test".into(),
        target: KillTarget::All,
        exclude: vec!["ag-self".into()],
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Kill { exclude, .. } => {
            assert_eq!(exclude, vec!["ag-self"]);
        }
        _ => panic!("expected Kill"),
    }
}

#[test]
fn given_kill_request_without_exclude_should_default_empty() {
    let json = r#"{"type":"kill","project_root":"/test","target":"all"}"#;
    let parsed: Request = serde_json::from_str(json).unwrap();
    match parsed {
        Request::Kill { exclude, .. } => {
            assert!(exclude.is_empty());
        }
        _ => panic!("expected Kill"),
    }
}

#[test]
fn given_protocol_version_should_be_current() {
    // Intentional hardcoded check — update when the protocol version is bumped.
    assert_eq!(PROTOCOL_VERSION, 5);
}

// --- GridCommand round-trips ---

#[test]
fn given_grid_split_command_should_round_trip() {
    let cmd = GridCommand::Split {
        leaf_id: Some(2),
        axis: "v".into(),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: GridCommand = serde_json::from_str(&json).unwrap();
    match parsed {
        GridCommand::Split { leaf_id, axis } => {
            assert_eq!(leaf_id, Some(2));
            assert_eq!(axis, "v");
        }
        _ => panic!("expected Split"),
    }
}

#[test]
fn given_grid_close_command_should_round_trip() {
    let cmd = GridCommand::Close { leaf_id: None };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: GridCommand = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, GridCommand::Close { leaf_id: None }));
}

#[test]
fn given_grid_focus_command_should_round_trip() {
    let cmd = GridCommand::Focus {
        leaf_id: None,
        direction: Some("right".into()),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: GridCommand = serde_json::from_str(&json).unwrap();
    match parsed {
        GridCommand::Focus { leaf_id, direction } => {
            assert!(leaf_id.is_none());
            assert_eq!(direction.unwrap(), "right");
        }
        _ => panic!("expected Focus"),
    }
}

#[test]
fn given_grid_set_agent_command_should_round_trip() {
    let cmd = GridCommand::SetAgent {
        leaf_id: 3,
        agent_id: "ag-abc".into(),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: GridCommand = serde_json::from_str(&json).unwrap();
    match parsed {
        GridCommand::SetAgent { leaf_id, agent_id } => {
            assert_eq!(leaf_id, 3);
            assert_eq!(agent_id, "ag-abc");
        }
        _ => panic!("expected SetAgent"),
    }
}

#[test]
fn given_grid_get_layout_command_should_round_trip() {
    let cmd = GridCommand::GetLayout;
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: GridCommand = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, GridCommand::GetLayout));
}

#[test]
fn given_subscribe_grid_request_should_round_trip() {
    let req = Request::SubscribeGrid {
        project_root: "/test".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::SubscribeGrid { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected SubscribeGrid"),
    }
}

#[test]
fn given_grid_command_request_should_round_trip() {
    let req = Request::GridCommand {
        project_root: "/test".into(),
        command: GridCommand::Split {
            leaf_id: Some(1),
            axis: "h".into(),
        },
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::GridCommand {
            project_root,
            command,
        } => {
            assert_eq!(project_root, "/test");
            assert!(matches!(command, GridCommand::Split { .. }));
        }
        _ => panic!("expected GridCommand"),
    }
}

#[test]
fn given_grid_event_response_should_round_trip() {
    let resp = Response::GridEvent {
        project_root: "/test".into(),
        command: GridCommand::Focus {
            leaf_id: Some(2),
            direction: None,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::GridEvent {
            project_root,
            command,
        } => {
            assert_eq!(project_root, "/test");
            assert!(matches!(command, GridCommand::Focus { .. }));
        }
        _ => panic!("expected GridEvent"),
    }
}

// --- Suspend/Resume round-trips ---

#[test]
fn given_suspend_request_with_all_target_should_round_trip() {
    let req = Request::Suspend {
        project_root: "/test".into(),
        target: SuspendTarget::All,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Suspend {
            project_root,
            target,
        } => {
            assert_eq!(project_root, "/test");
            assert!(matches!(target, SuspendTarget::All));
        }
        _ => panic!("expected Suspend"),
    }
}

#[test]
fn given_suspend_request_with_agent_target_should_round_trip() {
    let req = Request::Suspend {
        project_root: "/test".into(),
        target: SuspendTarget::Agent("ag-abc".into()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Suspend {
            target: SuspendTarget::Agent(id),
            ..
        } => {
            assert_eq!(id, "ag-abc");
        }
        _ => panic!("expected Suspend with Agent target"),
    }
}

#[test]
fn given_resume_request_should_round_trip() {
    let req = Request::Resume {
        project_root: "/test".into(),
        agent_id: "ag-abc".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Resume {
            project_root,
            agent_id,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(agent_id, "ag-abc");
        }
        _ => panic!("expected Resume"),
    }
}

#[test]
fn given_suspend_result_should_round_trip() {
    let resp = Response::SuspendResult {
        suspended: vec!["ag-abc".into(), "ag-def".into()],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::SuspendResult { suspended } => {
            assert_eq!(suspended, vec!["ag-abc", "ag-def"]);
        }
        _ => panic!("expected SuspendResult"),
    }
}

#[test]
fn given_resume_result_should_round_trip() {
    let resp = Response::ResumeResult {
        agent_id: "ag-abc".into(),
        status: crate::types::AgentStatus::Running,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::ResumeResult { agent_id, status } => {
            assert_eq!(agent_id, "ag-abc");
            assert_eq!(status, crate::types::AgentStatus::Running);
        }
        _ => panic!("expected ResumeResult"),
    }
}

// --- Rename round-trips ---

#[test]
fn given_rename_request_should_round_trip() {
    let req = Request::Rename {
        project_root: "/test".into(),
        agent_id: "ag-abc".into(),
        name: "new-name".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Rename {
            project_root,
            agent_id,
            name,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(agent_id, "ag-abc");
            assert_eq!(name, "new-name");
        }
        _ => panic!("expected Rename"),
    }
}

#[test]
fn given_rename_result_should_round_trip() {
    let resp = Response::RenameResult {
        agent_id: "ag-abc".into(),
        name: "new-name".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::RenameResult { agent_id, name } => {
            assert_eq!(agent_id, "ag-abc");
            assert_eq!(name, "new-name");
        }
        _ => panic!("expected RenameResult"),
    }
}

// --- AssignTrigger round-trips ---

#[test]
fn given_assign_trigger_request_should_round_trip() {
    let req = Request::AssignTrigger {
        project_root: "/test".into(),
        agent_id: "ag-abc123".into(),
        trigger_name: "review-bot".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::AssignTrigger {
            project_root,
            agent_id,
            trigger_name,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(agent_id, "ag-abc123");
            assert_eq!(trigger_name, "review-bot");
        }
        _ => panic!("expected AssignTrigger"),
    }
}

#[test]
fn given_assign_trigger_result_should_round_trip() {
    let resp = Response::AssignTriggerResult {
        agent_id: "ag-abc".into(),
        trigger_name: "review-bot".into(),
        sequence_len: 3,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::AssignTriggerResult {
            agent_id,
            trigger_name,
            sequence_len,
        } => {
            assert_eq!(agent_id, "ag-abc");
            assert_eq!(trigger_name, "review-bot");
            assert_eq!(sequence_len, 3);
        }
        _ => panic!("expected AssignTriggerResult"),
    }
}

// --- DeleteWorktree round-trips ---

#[test]
fn given_delete_worktree_request_should_round_trip() {
    let req = Request::DeleteWorktree {
        project_root: "/test".into(),
        worktree_id: "wt-abc".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::DeleteWorktree {
            project_root,
            worktree_id,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(worktree_id, "wt-abc");
        }
        _ => panic!("expected DeleteWorktree"),
    }
}

#[test]
fn given_delete_worktree_result_should_round_trip() {
    let resp = Response::DeleteWorktreeResult {
        worktree_id: "wt-abc".into(),
        killed_agents: vec!["ag-1".into(), "ag-2".into()],
        branch_deleted: true,
        remote_deleted: false,
        directory_removed: true,
        error: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::DeleteWorktreeResult {
            worktree_id,
            killed_agents,
            branch_deleted,
            remote_deleted,
            directory_removed,
            error,
        } => {
            assert_eq!(worktree_id, "wt-abc");
            assert_eq!(killed_agents, vec!["ag-1", "ag-2"]);
            assert!(branch_deleted);
            assert!(!remote_deleted);
            assert!(directory_removed);
            assert!(error.is_none());
        }
        _ => panic!("expected DeleteWorktreeResult"),
    }
}

#[test]
fn given_delete_worktree_result_with_directory_failure_should_round_trip() {
    let resp = Response::DeleteWorktreeResult {
        worktree_id: "wt-abc".into(),
        killed_agents: vec![],
        branch_deleted: false,
        remote_deleted: false,
        directory_removed: false,
        error: Some("permission denied".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::DeleteWorktreeResult {
            directory_removed,
            error,
            ..
        } => {
            assert!(!directory_removed);
            assert_eq!(error.as_deref(), Some("permission denied"));
        }
        _ => panic!("expected DeleteWorktreeResult"),
    }
}

#[test]
fn given_suspend_target_all_should_round_trip() {
    let target = SuspendTarget::All;
    let json = serde_json::to_string(&target).unwrap();
    let parsed: SuspendTarget = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, SuspendTarget::All));
}

// --- SwarmRosterEntryPayload ---

#[test]
fn given_swarm_roster_entry_payload_should_round_trip() {
    // given
    let entry = SwarmRosterEntryPayload {
        agent_def: "reviewer".into(),
        role: "review".into(),
        quantity: 2,
    };

    // when
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: SwarmRosterEntryPayload = serde_json::from_str(&json).unwrap();

    // then
    assert_eq!(parsed.agent_def, "reviewer");
    assert_eq!(parsed.role, "review");
    assert_eq!(parsed.quantity, 2);
}

#[test]
fn given_swarm_roster_entry_payload_should_default_quantity_to_1() {
    // given
    let json = r#"{"agent_def":"builder","role":"build"}"#;

    // when
    let parsed: SwarmRosterEntryPayload = serde_json::from_str(json).unwrap();

    // then
    assert_eq!(parsed.quantity, 1);
}

// --- TemplateInfo ---

#[test]
fn given_template_info_should_round_trip() {
    // given
    let info = TemplateInfo {
        name: "review".into(),
        description: "Code review".into(),
        agent: "claude".into(),
        source: "local".into(),
        variables: vec!["BRANCH".into()],
        command: None,
    };

    // when
    let json = serde_json::to_string(&info).unwrap();
    let parsed: TemplateInfo = serde_json::from_str(&json).unwrap();

    // then
    assert_eq!(parsed.name, "review");
    assert_eq!(parsed.description, "Code review");
    assert_eq!(parsed.agent, "claude");
    assert_eq!(parsed.source, "local");
    assert_eq!(parsed.variables, vec!["BRANCH"]);
}

// --- AgentDefInfo ---

#[test]
fn given_agent_def_info_should_round_trip() {
    // given
    let info = AgentDefInfo {
        name: "reviewer".into(),
        agent_type: "claude".into(),
        template: Some("review-template".into()),
        inline_prompt: None,
        tags: vec!["review".into()],
        scope: "local".into(),
        available_in_command_dialog: true,
        icon: Some("magnifyingglass".into()),
        command: None,
    };

    // when
    let json = serde_json::to_string(&info).unwrap();
    let parsed: AgentDefInfo = serde_json::from_str(&json).unwrap();

    // then
    assert_eq!(parsed.name, "reviewer");
    assert_eq!(parsed.agent_type, "claude");
    assert_eq!(parsed.template, Some("review-template".into()));
    assert_eq!(parsed.tags, vec!["review"]);
    assert_eq!(parsed.scope, "local");
    assert!(parsed.available_in_command_dialog);
    assert_eq!(parsed.icon, Some("magnifyingglass".into()));
}

// --- SwarmDefInfo ---

#[test]
fn given_swarm_def_info_should_round_trip() {
    // given
    let info = SwarmDefInfo {
        name: "full-stack".into(),
        worktree_count: 3,
        worktree_template: "feature".into(),
        roster: vec![SwarmRosterEntryPayload {
            agent_def: "reviewer".into(),
            role: "review".into(),
            quantity: 2,
        }],
        include_terminal: true,
        scope: "local".into(),
    };

    // when
    let json = serde_json::to_string(&info).unwrap();
    let parsed: SwarmDefInfo = serde_json::from_str(&json).unwrap();

    // then
    assert_eq!(parsed.name, "full-stack");
    assert_eq!(parsed.worktree_count, 3);
    assert_eq!(parsed.worktree_template, "feature");
    assert_eq!(parsed.roster.len(), 1);
    assert_eq!(parsed.roster[0].agent_def, "reviewer");
    assert!(parsed.include_terminal);
    assert_eq!(parsed.scope, "local");
}

// --- New Request round-trips ---

#[test]
fn given_list_templates_request_should_round_trip() {
    // given
    let req = Request::ListTemplates {
        project_root: "/test".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::ListTemplates { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected ListTemplates"),
    }
}

#[test]
fn given_get_template_request_should_round_trip() {
    // given
    let req = Request::GetTemplate {
        project_root: "/test".into(),
        name: "review".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::GetTemplate { project_root, name } => {
            assert_eq!(project_root, "/test");
            assert_eq!(name, "review");
        }
        _ => panic!("expected GetTemplate"),
    }
}

#[test]
fn given_save_template_request_should_round_trip() {
    // given
    let req = Request::SaveTemplate {
        project_root: "/test".into(),
        name: "review".into(),
        description: "Code review".into(),
        agent: "claude".into(),
        body: "Review {{BRANCH}}.".into(),
        scope: "local".into(),
        command: None,
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::SaveTemplate {
            project_root,
            name,
            description,
            agent,
            body,
            scope,
            ..
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(name, "review");
            assert_eq!(description, "Code review");
            assert_eq!(agent, "claude");
            assert_eq!(body, "Review {{BRANCH}}.");
            assert_eq!(scope, "local");
        }
        _ => panic!("expected SaveTemplate"),
    }
}

#[test]
fn given_delete_template_request_should_round_trip() {
    // given
    let req = Request::DeleteTemplate {
        project_root: "/test".into(),
        name: "review".into(),
        scope: "local".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::DeleteTemplate {
            project_root,
            name,
            scope,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(name, "review");
            assert_eq!(scope, "local");
        }
        _ => panic!("expected DeleteTemplate"),
    }
}

#[test]
fn given_list_agent_defs_request_should_round_trip() {
    // given
    let req = Request::ListAgentDefs {
        project_root: "/test".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::ListAgentDefs { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected ListAgentDefs"),
    }
}

#[test]
fn given_save_agent_def_request_should_round_trip() {
    // given
    let req = Request::SaveAgentDef {
        project_root: "/test".into(),
        name: "reviewer".into(),
        agent_type: "claude".into(),
        template: Some("review-tpl".into()),
        inline_prompt: None,
        tags: vec!["review".into()],
        scope: "local".into(),
        available_in_command_dialog: true,
        icon: Some("magnifyingglass".into()),
        command: None,
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::SaveAgentDef {
            name,
            agent_type,
            template,
            tags,
            scope,
            available_in_command_dialog,
            icon,
            ..
        } => {
            assert_eq!(name, "reviewer");
            assert_eq!(agent_type, "claude");
            assert_eq!(template, Some("review-tpl".into()));
            assert_eq!(tags, vec!["review"]);
            assert_eq!(scope, "local");
            assert!(available_in_command_dialog);
            assert_eq!(icon, Some("magnifyingglass".into()));
        }
        _ => panic!("expected SaveAgentDef"),
    }
}

#[test]
fn given_save_agent_def_request_should_default_available_in_command_dialog_to_true() {
    // given
    let json = r#"{"type":"save_agent_def","project_root":"/test","name":"x","agent_type":"claude","scope":"local"}"#;

    // when
    let parsed: Request = serde_json::from_str(json).unwrap();

    // then
    match parsed {
        Request::SaveAgentDef {
            available_in_command_dialog,
            ..
        } => {
            assert!(available_in_command_dialog);
        }
        _ => panic!("expected SaveAgentDef"),
    }
}

#[test]
fn given_delete_agent_def_request_should_round_trip() {
    // given
    let req = Request::DeleteAgentDef {
        project_root: "/test".into(),
        name: "reviewer".into(),
        scope: "local".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::DeleteAgentDef {
            project_root,
            name,
            scope,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(name, "reviewer");
            assert_eq!(scope, "local");
        }
        _ => panic!("expected DeleteAgentDef"),
    }
}

#[test]
fn given_list_swarm_defs_request_should_round_trip() {
    // given
    let req = Request::ListSwarmDefs {
        project_root: "/test".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::ListSwarmDefs { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected ListSwarmDefs"),
    }
}

#[test]
fn given_save_swarm_def_request_should_round_trip() {
    // given
    let req = Request::SaveSwarmDef {
        project_root: "/test".into(),
        name: "full-stack".into(),
        worktree_count: 3,
        worktree_template: "feature".into(),
        roster: vec![SwarmRosterEntryPayload {
            agent_def: "reviewer".into(),
            role: "review".into(),
            quantity: 2,
        }],
        include_terminal: true,
        scope: "local".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::SaveSwarmDef {
            name,
            worktree_count,
            roster,
            include_terminal,
            scope,
            ..
        } => {
            assert_eq!(name, "full-stack");
            assert_eq!(worktree_count, 3);
            assert_eq!(roster.len(), 1);
            assert_eq!(roster[0].agent_def, "reviewer");
            assert!(include_terminal);
            assert_eq!(scope, "local");
        }
        _ => panic!("expected SaveSwarmDef"),
    }
}

#[test]
fn given_save_swarm_def_request_should_default_worktree_count_to_1() {
    // given
    let json = r#"{"type":"save_swarm_def","project_root":"/test","name":"x","scope":"local"}"#;

    // when
    let parsed: Request = serde_json::from_str(json).unwrap();

    // then
    match parsed {
        Request::SaveSwarmDef {
            worktree_count,
            worktree_template,
            roster,
            include_terminal,
            ..
        } => {
            assert_eq!(worktree_count, 1);
            assert_eq!(worktree_template, "");
            assert!(roster.is_empty());
            assert!(!include_terminal);
        }
        _ => panic!("expected SaveSwarmDef"),
    }
}

#[test]
fn given_delete_swarm_def_request_should_round_trip() {
    // given
    let req = Request::DeleteSwarmDef {
        project_root: "/test".into(),
        name: "full-stack".into(),
        scope: "local".into(),
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::DeleteSwarmDef {
            project_root,
            name,
            scope,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(name, "full-stack");
            assert_eq!(scope, "local");
        }
        _ => panic!("expected DeleteSwarmDef"),
    }
}

#[test]
fn given_run_swarm_request_should_round_trip() {
    // given
    let mut vars = std::collections::HashMap::new();
    vars.insert("ENV".into(), "staging".into());
    let req = Request::RunSwarm {
        project_root: "/test".into(),
        swarm_name: "full-stack".into(),
        vars,
    };

    // when
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Request::RunSwarm {
            project_root,
            swarm_name,
            vars,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(swarm_name, "full-stack");
            assert_eq!(vars["ENV"], "staging");
        }
        _ => panic!("expected RunSwarm"),
    }
}

#[test]
fn given_run_swarm_request_should_default_vars_to_empty() {
    // given
    let json = r#"{"type":"run_swarm","project_root":"/test","swarm_name":"x"}"#;

    // when
    let parsed: Request = serde_json::from_str(json).unwrap();

    // then
    match parsed {
        Request::RunSwarm { vars, .. } => {
            assert!(vars.is_empty());
        }
        _ => panic!("expected RunSwarm"),
    }
}

// --- New Response round-trips ---

#[test]
fn given_template_list_response_should_round_trip() {
    // given
    let resp = Response::TemplateList {
        templates: vec![TemplateInfo {
            name: "review".into(),
            description: "Code review".into(),
            agent: "claude".into(),
            source: "local".into(),
            variables: vec!["BRANCH".into()],
            command: None,
        }],
    };

    // when
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Response::TemplateList { templates } => {
            assert_eq!(templates.len(), 1);
            assert_eq!(templates[0].name, "review");
            assert_eq!(templates[0].variables, vec!["BRANCH"]);
        }
        _ => panic!("expected TemplateList"),
    }
}

#[test]
fn given_template_detail_response_should_round_trip() {
    // given
    let resp = Response::TemplateDetail {
        name: "review".into(),
        description: "Code review".into(),
        agent: "claude".into(),
        body: "Review {{BRANCH}}.".into(),
        source: "local".into(),
        variables: vec!["BRANCH".into()],
        command: None,
    };

    // when
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Response::TemplateDetail {
            name,
            description,
            agent,
            body,
            source,
            variables,
            ..
        } => {
            assert_eq!(name, "review");
            assert_eq!(description, "Code review");
            assert_eq!(agent, "claude");
            assert_eq!(body, "Review {{BRANCH}}.");
            assert_eq!(source, "local");
            assert_eq!(variables, vec!["BRANCH"]);
        }
        _ => panic!("expected TemplateDetail"),
    }
}

#[test]
fn given_agent_def_list_response_should_round_trip() {
    // given
    let resp = Response::AgentDefList {
        agent_defs: vec![AgentDefInfo {
            name: "reviewer".into(),
            agent_type: "claude".into(),
            template: None,
            inline_prompt: None,
            tags: vec![],
            scope: "local".into(),
            available_in_command_dialog: true,
            icon: None,
            command: None,
        }],
    };

    // when
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Response::AgentDefList { agent_defs } => {
            assert_eq!(agent_defs.len(), 1);
            assert_eq!(agent_defs[0].name, "reviewer");
        }
        _ => panic!("expected AgentDefList"),
    }
}

#[test]
fn given_swarm_def_list_response_should_round_trip() {
    // given
    let resp = Response::SwarmDefList {
        swarm_defs: vec![SwarmDefInfo {
            name: "full-stack".into(),
            worktree_count: 3,
            worktree_template: "feature".into(),
            roster: vec![],
            include_terminal: false,
            scope: "local".into(),
        }],
    };

    // when
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Response::SwarmDefList { swarm_defs } => {
            assert_eq!(swarm_defs.len(), 1);
            assert_eq!(swarm_defs[0].name, "full-stack");
            assert_eq!(swarm_defs[0].worktree_count, 3);
        }
        _ => panic!("expected SwarmDefList"),
    }
}

#[test]
fn given_run_swarm_result_response_should_round_trip() {
    // given
    let resp = Response::RunSwarmResult {
        spawned_agents: vec!["ag-abc".into(), "ag-def".into()],
    };

    // when
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Response::RunSwarmResult { spawned_agents } => {
            assert_eq!(spawned_agents, vec!["ag-abc", "ag-def"]);
        }
        _ => panic!("expected RunSwarmResult"),
    }
}

// --- Schedule round-trips ---

#[test]
fn given_list_schedules_request_should_round_trip() {
    let req = Request::ListSchedules {
        project_root: "/test".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::ListSchedules { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected ListSchedules"),
    }
}

#[test]
fn given_save_schedule_request_should_round_trip() {
    let req = Request::SaveSchedule {
        project_root: "/test".into(),
        name: "nightly".into(),
        enabled: true,
        recurrence: "daily".into(),
        start_at: Utc::now(),
        trigger: ScheduleTriggerPayload::AgentDef {
            name: "reviewer".into(),
        },
        target: String::new(),
        scope: "local".into(),
        root: true,
        agent_name: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::SaveSchedule {
            name, recurrence, ..
        } => {
            assert_eq!(name, "nightly");
            assert_eq!(recurrence, "daily");
        }
        _ => panic!("expected SaveSchedule"),
    }
}

#[test]
fn given_delete_schedule_request_should_round_trip() {
    let req = Request::DeleteSchedule {
        project_root: "/test".into(),
        name: "nightly".into(),
        scope: "local".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::DeleteSchedule { name, scope, .. } => {
            assert_eq!(name, "nightly");
            assert_eq!(scope, "local");
        }
        _ => panic!("expected DeleteSchedule"),
    }
}

#[test]
fn given_enable_schedule_request_should_round_trip() {
    let req = Request::EnableSchedule {
        project_root: "/test".into(),
        name: "nightly".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::EnableSchedule { name, .. } => assert_eq!(name, "nightly"),
        _ => panic!("expected EnableSchedule"),
    }
}

#[test]
fn given_disable_schedule_request_should_round_trip() {
    let req = Request::DisableSchedule {
        project_root: "/test".into(),
        name: "nightly".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::DisableSchedule { name, .. } => assert_eq!(name, "nightly"),
        _ => panic!("expected DisableSchedule"),
    }
}

#[test]
fn given_schedule_list_response_should_round_trip() {
    let resp = Response::ScheduleList {
        schedules: vec![ScheduleInfo {
            name: "nightly".into(),
            enabled: true,
            recurrence: "daily".into(),
            start_at: Utc::now(),
            next_run: Some(Utc::now()),
            trigger: ScheduleTriggerPayload::AgentDef {
                name: "reviewer".into(),
            },
            project_root: "/test".into(),
            target: String::new(),
            scope: "local".into(),
            root: true,
            agent_name: None,
            created_at: Utc::now(),
        }],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::ScheduleList { schedules } => {
            assert_eq!(schedules.len(), 1);
            assert_eq!(schedules[0].name, "nightly");
        }
        _ => panic!("expected ScheduleList"),
    }
}

#[test]
fn given_schedule_detail_response_should_round_trip() {
    let resp = Response::ScheduleDetail {
        name: "nightly".into(),
        enabled: true,
        recurrence: "daily".into(),
        start_at: Utc::now(),
        next_run: None,
        trigger: ScheduleTriggerPayload::InlinePrompt {
            prompt: "Review deps".into(),
            agent: "claude".into(),
        },
        project_root: "/test".into(),
        target: String::new(),
        scope: "local".into(),
        root: false,
        agent_name: Some("overnight-build".into()),
        created_at: Utc::now(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::ScheduleDetail { name, trigger, .. } => {
            assert_eq!(name, "nightly");
            assert!(matches!(
                trigger,
                ScheduleTriggerPayload::InlinePrompt { .. }
            ));
        }
        _ => panic!("expected ScheduleDetail"),
    }
}

#[test]
fn given_run_swarm_partial_response_should_round_trip() {
    // given
    let resp = Response::RunSwarmPartial {
        spawned_agents: vec!["ag-abc".into()],
        error_code: "SPAWN_FAILED".into(),
        error_message: "could not spawn agent ag-def".into(),
    };

    // when
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();

    // then
    match parsed {
        Response::RunSwarmPartial {
            spawned_agents,
            error_code,
            error_message,
        } => {
            assert_eq!(spawned_agents, vec!["ag-abc"]);
            assert_eq!(error_code, "SPAWN_FAILED");
            assert_eq!(error_message, "could not spawn agent ag-def");
        }
        _ => panic!("expected RunSwarmPartial"),
    }
}

#[test]
fn given_create_worktree_request_should_round_trip() {
    let req = Request::CreateWorktree {
        project_root: "/test".into(),
        name: Some("feature-x".into()),
        base: Some("develop".into()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::CreateWorktree {
            project_root,
            name,
            base,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(name.as_deref(), Some("feature-x"));
            assert_eq!(base.as_deref(), Some("develop"));
        }
        _ => panic!("expected CreateWorktree"),
    }
}

#[test]
fn given_create_worktree_request_should_default_optional_fields() {
    let json = r#"{"type":"create_worktree","project_root":"/test"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::CreateWorktree { name, base, .. } => {
            assert!(name.is_none());
            assert!(base.is_none());
        }
        _ => panic!("expected CreateWorktree"),
    }
}

#[test]
fn given_create_worktree_result_should_round_trip() {
    let resp = Response::CreateWorktreeResult {
        worktree_id: "wt-abc".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::CreateWorktreeResult { worktree_id } => {
            assert_eq!(worktree_id, "wt-abc");
        }
        _ => panic!("expected CreateWorktreeResult"),
    }
}

#[test]
fn given_get_config_request_should_round_trip() {
    let req = Request::GetConfig {
        project_root: "/test".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::GetConfig { project_root } => assert_eq!(project_root, "/test"),
        _ => panic!("expected GetConfig"),
    }
}

#[test]
fn given_update_agent_config_request_with_args_should_round_trip() {
    let req = Request::UpdateAgentConfig {
        project_root: "/test".into(),
        agent_name: "claude".into(),
        launch_args: Some(vec!["--verbose".into(), "--model".into(), "opus".into()]),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::UpdateAgentConfig {
            project_root,
            agent_name,
            launch_args,
        } => {
            assert_eq!(project_root, "/test");
            assert_eq!(agent_name, "claude");
            assert_eq!(launch_args.unwrap(), vec!["--verbose", "--model", "opus"]);
        }
        _ => panic!("expected UpdateAgentConfig"),
    }
}

// --- Input submit field ---

#[test]
fn given_input_with_submit_should_round_trip() {
    let req = Request::Input {
        agent_id: "ag-abc".into(),
        data: b"hello world".to_vec(),
        submit: true,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Input {
            agent_id,
            data,
            submit,
        } => {
            assert_eq!(agent_id, "ag-abc");
            assert_eq!(data, b"hello world");
            assert!(submit);
        }
        _ => panic!("expected Input"),
    }
}

#[test]
fn given_update_agent_config_request_with_none_should_round_trip() {
    let req = Request::UpdateAgentConfig {
        project_root: "/test".into(),
        agent_name: "codex".into(),
        launch_args: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::UpdateAgentConfig { launch_args, .. } => {
            assert!(launch_args.is_none());
        }
        _ => panic!("expected UpdateAgentConfig"),
    }
}

#[test]
fn given_config_report_response_should_round_trip() {
    let resp = Response::ConfigReport {
        default_agent: "claude".into(),
        agents: vec![
            AgentConfigInfo {
                name: "claude".into(),
                command: "claude".into(),
                launch_args: Some(vec!["--verbose".into()]),
                resolved_launch_args: vec!["--verbose".into()],
                interactive: true,
            },
            AgentConfigInfo {
                name: "codex".into(),
                command: "codex".into(),
                launch_args: None,
                resolved_launch_args: vec!["--full-auto".into()],
                interactive: true,
            },
        ],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&json).unwrap();
    match parsed {
        Response::ConfigReport {
            default_agent,
            agents,
        } => {
            assert_eq!(default_agent, "claude");
            assert_eq!(agents.len(), 2);
            assert_eq!(agents[0].name, "claude");
            assert_eq!(agents[0].launch_args, Some(vec!["--verbose".to_string()]));
            assert_eq!(agents[1].name, "codex");
            assert!(agents[1].launch_args.is_none());
            assert_eq!(agents[1].resolved_launch_args, vec!["--full-auto"]);
        }
        _ => panic!("expected ConfigReport"),
    }
}

#[test]
fn given_input_without_submit_field_should_default_false() {
    let json = r#"{"type":"input","agent_id":"ag-abc","data":"68656c6c6f"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Input { submit, .. } => assert!(!submit),
        _ => panic!("expected Input"),
    }
}

#[test]
fn given_input_with_submit_false_should_round_trip() {
    let req = Request::Input {
        agent_id: "ag-abc".into(),
        data: b"raw keys".to_vec(),
        submit: false,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: Request = serde_json::from_str(&json).unwrap();
    match parsed {
        Request::Input { submit, .. } => assert!(!submit),
        _ => panic!("expected Input"),
    }
}
