use pu_core::protocol::{InventoryKind, KillTarget, Request, Response, SuspendTarget};
use pu_core::signals::{AttentionKind, OutcomeKind, SignalActivity, SignalSnapshot};
use pu_engine::engine::Engine;
use serde_json::json;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
fn save(dir: &Path, name: &str, v: &impl serde::Serialize) -> Result<(), String> {
    std::fs::write(
        dir.join(name),
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
async fn capture(
    e: &Engine,
    project: &str,
    dir: &Path,
    tag: &str,
    id: &str,
) -> Result<Option<SignalSnapshot>, String> {
    let status = e
        .handle_request(Request::Status {
            project_root: project.into(),
            agent_id: None,
        })
        .await;
    let inventory = e
        .handle_request(Request::Inventory {
            project_root: Some(project.into()),
            kind: InventoryKind::Agents,
            state: None,
        })
        .await;
    let (worktrees, agents) = e
        .compute_full_status(project)
        .await
        .map_err(|e| e.to_string())?;
    let event = Response::StatusEvent { worktrees, agents };
    let frame = json!({"captured_at_ms":chrono::Utc::now().timestamp_millis(),"status":status,"inventory":inventory,"status_event":event});
    save(dir, &format!("{tag}.json"), &frame)?;
    let agent = frame["inventory"]["agents"]
        .as_array()
        .ok_or("no inventory agents")?
        .iter()
        .find(|a| a["id"] == id)
        .ok_or("owned agent absent")?;
    if agent["state"] != "running" {
        return Err(format!("owned agent not Running: {}", agent["state"]));
    }
    if let Some(pid) = agent["pid"].as_u64() {
        std::fs::write(dir.join("owned-pid.txt"), pid.to_string()).map_err(|e| e.to_string())?;
    }
    if agent["signal"].is_null() {
        return Ok(None);
    }
    let snapshot: SignalSnapshot =
        serde_json::from_value(agent["signal"].clone()).map_err(|e| e.to_string())?;
    if snapshot.identity.agent_id != id || snapshot.identity.native_session_id.is_empty() {
        return Err("native identity mismatch".into());
    }
    if snapshot
        .pending_attention
        .iter()
        .any(|p| p.actionability.macos || p.actionability.ios)
    {
        return Err("unqualified attention activated".into());
    }
    Ok(Some(snapshot))
}
async fn raw(e: &Engine, id: &str, data: &[u8]) -> Result<(), String> {
    let response = e
        .handle_request(Request::Input {
            agent_id: id.into(),
            data: data.to_vec(),
            submit: false,
            cancel_on_disconnect: false,
        })
        .await;
    if !matches!(response, Response::Ok) {
        return Err(format!("input failed: {response:?}"));
    }
    Ok(())
}
async fn stages(
    e: &Engine,
    project: &str,
    id: &str,
    dir: &Path,
    tool: &str,
    scenario: &str,
) -> Result<(), String> {
    tokio::time::sleep(Duration::from_secs(3)).await;
    if tool == "opencode" {
        let until = Instant::now() + Duration::from_secs(25);
        loop {
            let logs = e
                .handle_request(Request::Logs {
                    agent_id: id.into(),
                    tail: 65536,
                })
                .await;
            let text = serde_json::to_value(logs).map_err(|e| e.to_string())?;
            if text["data"]
                .as_str()
                .is_some_and(|s| s.contains("Native fixture"))
            {
                break;
            }
            if Instant::now() > until {
                return Err("native OpenCode input UI not ready".into());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    if tool == "codex" && scenario.starts_with("question") {
        raw(e, id, b"/plan").await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        raw(e, id, b"\r").await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    raw(e, id, b"Native test hello").await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    raw(e, id, b"\r").await?;
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut working = false;
    let mut request = None;
    let mut answered = false;
    let mut recovered = false;
    let negative = matches!(scenario, "permission_deny" | "question_cancel");
    let mut recovery_working = false;
    loop {
        if let Some(s) = capture(e, project, dir, "latest", id).await? {
            if s.is_current(chrono::Utc::now().timestamp_millis()) {
                if s.activity == SignalActivity::Working
                    && s.pending_attention.is_empty()
                    && !working
                {
                    working = true;
                    capture(e, project, dir, "working", id).await?;
                    if scenario == "interrupt" {
                        raw(e, id, b"\x1b").await?;
                        if tool == "opencode" {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            raw(e, id, b"\x1b").await?;
                        }
                        answered = true;
                    }
                }
                if (scenario.starts_with("question") || scenario.starts_with("permission"))
                    && request.is_none()
                {
                    let kind = if scenario.starts_with("question") {
                        AttentionKind::Question
                    } else {
                        AttentionKind::Permission
                    };
                    if let Some(pending) = s.pending_attention.iter().find(|p| p.kind == kind) {
                        request = Some(pending.request_id.clone());
                        capture(e, project, dir, "pending", id).await?;
                        save(
                            dir,
                            "pending-logs.json",
                            &e.handle_request(Request::Logs {
                                agent_id: id.into(),
                                tail: 65536,
                            })
                            .await,
                        )?;
                        if scenario == "permission_suspend" {
                            return Ok(());
                        }
                        if scenario == "question_hold" {
                            let mut receiver = e.subscribe_status(project).await;
                            let began = Instant::now();
                            let mut delivered = s.clone();
                            let mut notifications = 0;
                            while began.elapsed() < Duration::from_secs(35) {
                                if let Ok(Ok(())) =
                                    tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                                        .await
                                {
                                    notifications += 1;
                                    delivered = capture(e, project, dir, "held-delivered", id)
                                        .await?
                                        .ok_or("missing held snapshot")?;
                                }
                                save(
                                    dir,
                                    "status-heartbeat.json",
                                    &json!({"elapsed_ms":began.elapsed().as_millis(),"notifications":notifications,"last_delivered":delivered.observation}),
                                )?;
                                if !delivered.is_current(chrono::Utc::now().timestamp_millis()) {
                                    return Err(
                                        "subscribed question lease expired without heartbeat"
                                            .into(),
                                    );
                                }
                                if notifications > 6 {
                                    return Err(
                                        "unchanged native state flooded status subscribers".into(),
                                    );
                                }
                            }
                            if notifications < 2 {
                                return Err("no recurring lease renewal delivered".into());
                            }
                        }
                        if negative {
                            if scenario == "permission_deny" && tool == "claude" {
                                raw(e, id, b"\x1b[B\x1b[B").await?;
                                tokio::time::sleep(Duration::from_millis(250)).await;
                                raw(e, id, b"\r").await?;
                            } else if scenario == "permission_deny" && tool == "opencode" {
                                raw(e, id, b"\x1b[C").await?;
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                raw(e, id, b"\x1b[C").await?;
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                raw(e, id, b"\r").await?;
                                tokio::time::sleep(Duration::from_millis(300)).await;
                                raw(e, id, b"\r").await?;
                            } else {
                                raw(e, id, b"\x1b").await?;
                            }
                        } else {
                            raw(e, id, b"\r").await?;
                            if tool != "claude" {
                                tokio::time::sleep(Duration::from_millis(300)).await;
                                raw(e, id, b"\r").await?;
                            }
                        }
                        answered = true;
                    }
                }
                if negative
                    && answered
                    && !recovered
                    && s.activity == SignalActivity::Idle
                    && s.pending_attention.is_empty()
                {
                    capture(e, project, dir, "negative-resolved", id).await?;
                    raw(e, id, b"Recover after negative native response").await?;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    raw(e, id, b"\r").await?;
                    recovered = true;
                }
                if negative
                    && recovered
                    && s.activity == SignalActivity::Working
                    && s.pending_attention.is_empty()
                {
                    recovery_working = true;
                    capture(e, project, dir, "recovery-working", id).await?;
                }
                if tool == "claude"
                    && scenario == "interrupt"
                    && working
                    && answered
                    && !recovered
                    && s.activity == SignalActivity::Idle
                    && s.outcome.is_none()
                    && s.pending_attention.is_empty()
                {
                    capture(e, project, dir, "interrupted-unknown", id).await?;
                    raw(e, id, b"Recover after native cancellation").await?;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    raw(e, id, b"\r").await?;
                    recovered = true;
                }
                let expected = if scenario == "error" {
                    OutcomeKind::Errored
                } else if scenario == "interrupt" && tool != "claude" {
                    OutcomeKind::Interrupted
                } else {
                    OutcomeKind::Completed
                };
                if working
                    && s.outcome.as_ref().is_some_and(|o| o.kind == expected)
                    && (!negative || (recovered && recovery_working))
                {
                    if scenario == "interrupt" && tool == "claude" && !recovered {
                        return Err("no actual native cancel/recovery".into());
                    }
                    if !working {
                        return Err("no actual Working stage captured".into());
                    }
                    if (scenario == "question" || scenario == "permission")
                        && (!answered || request.is_none())
                    {
                        return Err("terminal outcome without native request/input".into());
                    }
                    if !s.pending_attention.is_empty() {
                        return Err("native request retained after terminal outcome".into());
                    }
                    capture(e, project, dir, "resolved", id).await?;
                    save(
                        dir,
                        "resolved-logs.json",
                        &e.handle_request(Request::Logs {
                            agent_id: id.into(),
                            tail: 65536,
                        })
                        .await,
                    )?;
                    save(
                        dir,
                        "result.json",
                        &json!({"result":"PASS","tool":tool,"scenario":scenario,"agent_id":id,"native_session_id":s.identity.native_session_id,"request_id":request,"working":working,"actual_public_spawn_input_status_inventory_event":true,"app_device_notification":"NOT RUN"}),
                    )?;
                    return Ok(());
                }
            }
        }
        if Instant::now() > deadline {
            return Err("native stages timeout; inspect latest frame/logs".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dir = Path::new(&args[1]);
    let tool = &args[2];
    let cli = &args[3];
    let scenario = &args[4];
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let outcome: Result<(), String> = rt.block_on(async {
        let e = Arc::new(Engine::new());
        let project = dir.join("project");
        std::fs::create_dir_all(&project).map_err(|e| e.to_string())?;
        let project = project
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        save(
            dir,
            "init.json",
            &e.handle_request(Request::Init {
                project_root: project.clone(),
            })
            .await,
        )?;
        let mut agents = serde_json::Map::new();
        agents.insert(
            tool.clone(),
            json!({"name":tool,"command":cli,"interactive":true,"launchArgs":[]}),
        );
        let config = json!({"defaultAgent":tool,"agents":agents,"envFiles":[]});
        std::fs::write(
            Path::new(&project).join(".pu/config.yaml"),
            serde_json::to_vec(&config).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        let mut extra = Vec::new();
        if tool == "claude" {
            extra.extend([
                "--setting-sources".into(),
                "".into(),
                "--model".into(),
                "claude-sonnet-4-6".into(),
            ]);
            if scenario.starts_with("permission") {
                extra.extend(["--permission-mode".into(), "manual".into()]);
            }
        }
        if tool == "codex" {
            extra.push("--no-alt-screen".into());
        }
        let response = e
            .handle_request(Request::Spawn {
                project_root: project.clone(),
                prompt: String::new(),
                agent: tool.clone(),
                name: Some("native-owned-proof".into()),
                base: None,
                root: scenario != "worktree",
                worktree: None,
                command: None,
                no_auto: true,
                extra_args: extra,
                plan_mode: false,
                no_trigger: true,
                trigger: None,
            })
            .await;
        save(dir, "spawn.json", &response)?;
        let id = if let Response::SpawnResult { agent_id, .. } = response {
            agent_id
        } else {
            return Err(format!("spawn failed: {response:?}"));
        };
        let result = async {
            stages(&e, &project, &id, dir, tool, if scenario == "resume_pending" { "permission_suspend" } else { scenario }).await?;
            if scenario == "worktree" {
                let signal = capture(&e, &project, dir, "worktree", &id)
                    .await?
                    .ok_or("no worktree signal")?;
                if signal.identity.worktree_id.is_none() {
                    return Err("missing scoped worktree identity".into());
                }
            }

            if matches!(scenario, "resume" | "resume_pending") {
                let before = capture(&e, &project, dir, "before-suspend", &id)
                    .await?
                    .ok_or("no initial signal")?;
                let suspended = e
                    .handle_request(Request::Suspend {
                        project_root: project.clone(),
                        target: SuspendTarget::Agent(id.clone()),
                    })
                    .await;
                save(dir, "suspended.json", &suspended)?;
                if !matches!(suspended, Response::SuspendResult { .. }) {
                    return Err(format!("suspend failed: {suspended:?}"));
                }
                let inventory = e
                    .handle_request(Request::Inventory {
                        project_root: Some(project.clone()),
                        kind: InventoryKind::Agents,
                        state: None,
                    })
                    .await;
                let wire = serde_json::to_value(&inventory).map_err(|e| e.to_string())?;
                save(dir, "suspended-inventory.json", &inventory)?;
                if wire["agents"][0]["state"] == "running" || !wire["agents"][0]["signal"].is_null()
                {
                    return Err("suspended agent retained live signal".into());
                }
                let resumed = e
                    .handle_request(Request::Resume {
                        project_root: project.clone(),
                        agent_id: id.clone(),
                    })
                    .await;
                save(dir, "resumed.json", &resumed)?;
                if !matches!(resumed, Response::ResumeResult { .. }) {
                    return Err(format!("resume failed: {resumed:?}"));
                }
                if scenario == "resume_pending" {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    let startup = capture(&e, &project, dir, "resume-before-submit", &id).await?.ok_or("resumed native idle unavailable")?;
                    save(dir, "resume-startup-logs.json", &e.handle_request(Request::Logs { agent_id: id.clone(), tail: 65536 }).await)?;
                    if !startup.is_current(chrono::Utc::now().timestamp_millis()) || startup.activity != SignalActivity::Idle || !startup.pending_attention.is_empty() || startup.outcome.is_some() {
                        return Err("resumed launch revived historical work/request/outcome before new input".into());
                    }
                }
                stages(&e, &project, &id, dir, tool, scenario).await?;
                let after = capture(&e, &project, dir, "after-resume", &id)
                    .await?
                    .ok_or("no resumed signal")?;
                if before.identity.native_session_id != after.identity.native_session_id
                    || before.identity.launch_id == after.identity.launch_id
                {
                    return Err("resume identity did not preserve native session/new launch".into());
                }
            }
            Ok(())
        }
        .await;
        save(
            dir,
            "final-logs.json",
            &e.handle_request(Request::Logs {
                agent_id: id.clone(),
                tail: 65536,
            })
            .await,
        )?;
        save(
            dir,
            "cleanup.json",
            &e.handle_request(Request::Kill {
                project_root: project.clone(),
                target: KillTarget::Agent(id),
                exclude: vec![],
            })
            .await,
        )?;
        save(
            dir,
            "shutdown.json",
            &e.handle_request(Request::Shutdown).await,
        )?;
        result
    });
    match outcome {
        Ok(()) => println!("PASS public Engine {tool}/{scenario}"),
        Err(error) => {
            eprintln!("FAIL: {error}");
            std::process::exit(1)
        }
    }
}
