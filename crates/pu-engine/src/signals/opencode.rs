//! Read-only observation of the service already used by the native OpenCode TUI.
use super::{LaunchState, Prepared, install, now_ms};
use pu_core::signals::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub(super) struct Observer {
    registration: PathBuf,
}

pub(super) fn prepare(
    project: &str,
    agent: &str,
    worktree: Option<String>,
    session: &str,
    args: &[String],
    env: &[(String, String)],
) -> Option<Prepared> {
    if !crate::read_source::valid_session_id(session)
        || args
            .iter()
            .any(|a| a == "--standalone" || a == "--server" || a.starts_with("--server="))
    {
        return None;
    }
    let get = |key: &str| {
        env.iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| PathBuf::from(v))
    };
    let state_home =
        get("XDG_STATE_HOME").or_else(|| get("HOME").map(|p| p.join(".local/state")))?;
    let launch = pu_core::id::session_id();
    let artifacts = install::Artifacts::new(Path::new(project), &launch).ok()?;
    let socket = tokio::net::UnixDatagram::bind(artifacts.socket_directory.join("s")).ok()?;
    let identity = SignalIdentity {
        agent_id: agent.into(),
        worktree_id: worktree,
        launch_id: launch,
        generation: 1,
        native_session_id: session.into(),
    };
    Some(Prepared {
        socket,
        token: String::new(),
        tool: "opencode".into(),
        version: "2.0.28".into(),
        state: Arc::new(LaunchState {
            reducer: Mutex::new(None),
            identity: Mutex::new(identity),
            native_snapshot: Mutex::new(None),
            macos_qualification: None,
            _artifacts: artifacts,
        }),
        output: None,
        observer: Some(super::NativeObserver::OpenCode(Observer {
            registration: state_home.join("opencode/service.json"),
        })),
    })
}

#[derive(Deserialize)]
struct Registration {
    url: String,
    pid: u32,
    version: String,
    password: Option<String>,
}

fn registration(path: &Path) -> Option<Registration> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file()
        || meta.len() > 16 * 1024
        || meta.mode() & 0o7777 != 0o600
        || meta.uid() != unsafe { libc::geteuid() }
    {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 16 * 1024 {
        return None;
    }
    let r: Registration = serde_json::from_slice(&bytes).ok()?;
    let url = reqwest::Url::parse(&r.url).ok()?;
    if r.pid == 0
        || r.version != "2.0.28"
        || url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return None;
    }
    Some(r)
}

async fn get(client: &reqwest::Client, r: &Registration, path: &str) -> Option<Value> {
    let url = reqwest::Url::parse(&r.url).ok()?.join(path).ok()?;
    let mut request = client.get(url);
    if let Some(password) = &r.password {
        request = request.basic_auth("opencode", Some(password));
    }
    let mut response = request.send().await.ok()?.error_for_status().ok()?;
    if response.content_length().is_some_and(|n| n > 1024 * 1024) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).ok()
}

async fn poll(
    client: &reqwest::Client,
    path: &Path,
    identity: &SignalIdentity,
    cwd: &str,
) -> Option<SignalSnapshot> {
    let path = path.to_owned();
    let r = tokio::task::spawn_blocking(move || registration(&path))
        .await
        .ok()??;
    let health = get(client, &r, "/api/info").await?;
    if health["pid"].as_u64() != Some(u64::from(r.pid)) || health["version"] != r.version {
        return None;
    }
    let session = format!("/api/session/{}", identity.native_session_id);
    let before = get(client, &r, "/api/session/active").await?;
    let info = get(client, &r, &session).await?;
    let permissions = get(client, &r, &format!("{session}/permission")).await?;
    let forms = get(client, &r, &format!("{session}/form")).await?;
    // Never combine the old outcome with a newly started/stopped execution.
    let after = get(client, &r, "/api/session/active").await?;
    let final_info = get(client, &r, &session).await?;
    if before["data"][&identity.native_session_id] != after["data"][&identity.native_session_id]
        || info != final_info
        || get(client, &r, "/api/info").await? != health
    {
        return None;
    }
    snapshot(identity, cwd, &info, &after, &permissions, &forms, now_ms())
}

impl Observer {
    pub(super) fn activate(
        self,
        state: Weak<LaunchState>,
        project: String,
        cwd: String,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    ) {
        tokio::spawn(async move {
            let Ok(client) = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_millis(300))
                .timeout(Duration::from_secs(1))
                .build()
            else {
                return;
            };
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {_=interval.tick()=>{},_=exit.changed()=>{}}
                let Some(state) = state.upgrade() else {
                    break;
                };
                let dead = exit.borrow().is_some() || exit.has_changed().is_err();
                let identity = state.identity.lock().unwrap().clone();
                let result = if dead {
                    None
                } else {
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        poll(&client, &self.registration, &identity, &cwd),
                    )
                    .await
                    .ok()
                    .flatten()
                };
                let dead = dead || exit.borrow().is_some() || exit.has_changed().is_err();
                {
                    let mut value = state.native_snapshot.lock().unwrap();
                    if !dead && let Some(current) = result {
                        *value = Some(current);
                    } else if let Some(current) = value.as_mut() {
                        current.observation.state = if dead {
                            ObservationState::ProcessDead
                        } else {
                            ObservationState::Disconnected
                        };
                    }
                }
                if let Some(tx) = channels.lock().await.get(&project) {
                    let _ = tx.send(());
                }
                if dead {
                    break;
                }
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn snapshot(
    identity: &SignalIdentity,
    cwd: &str,
    info: &Value,
    active: &Value,
    permissions: &Value,
    forms: &Value,
    now: i64,
) -> Option<SignalSnapshot> {
    let info = &info["data"];
    if info["id"].as_str() != Some(&identity.native_session_id)
        || !info["parentID"].is_null()
        || info["location"]["directory"].as_str() != Some(cwd)
    {
        return None;
    }
    let running = active["data"].as_object()?.get(&identity.native_session_id);
    if running.is_some_and(|s| s["type"] != "running") {
        return None;
    }
    let mut pending_attention = Vec::new();
    let mut ids = HashSet::new();
    for (list, kind) in [
        (permissions, AttentionKind::Permission),
        (forms, AttentionKind::Question),
    ] {
        for request in list["data"].as_array()? {
            let id = request["id"].as_str()?;
            if !crate::read_source::valid_session_id(id)
                || request["sessionID"].as_str() != Some(&identity.native_session_id)
                || !ids.insert(id.to_owned())
                || pending_attention.len() >= 64
            {
                return None;
            }
            pending_attention.push(PendingAttention {
                request_id: id.into(),
                kind,
                event_id: format!("opencode-request:{id}"),
                turn_id: None,
                confidence: SignalConfidence::Authoritative,
                actionability: AttentionActionability::default(),
            });
        }
    }
    let idle = info["time"]["idle"].as_i64();
    let outcome = if running.is_some() {
        None
    } else {
        match info["outcome"].as_str() {
            Some(kind) => Some(SignalOutcome {
                kind: match kind {
                    "succeeded" => OutcomeKind::Completed,
                    "failed" => OutcomeKind::Errored,
                    "interrupted" => OutcomeKind::Interrupted,
                    _ => return None,
                },
                event_id: format!("opencode-outcome:{}:{}", identity.native_session_id, idle?),
                turn_id: None,
                confidence: SignalConfidence::Authoritative,
            }),
            None => None,
        }
    };
    let event_id = outcome
        .as_ref()
        .map(|o| o.event_id.clone())
        .unwrap_or_else(|| {
            format!(
                "opencode-state:{}:{}",
                identity.native_session_id, info["time"]["updated"]
            )
        });
    Some(SignalSnapshot {
        schema_version: 1,
        identity: identity.clone(),
        activity: if running.is_some() {
            SignalActivity::Working
        } else {
            SignalActivity::Idle
        },
        pending_attention,
        outcome,
        observation: SignalObservation {
            state: ObservationState::Current,
            last_received_at_ms: Some(now),
            expires_at_ms: Some(now + 30_000),
        },
        evidence: Some(SignalEvidence {
            tool: "opencode".into(),
            tool_version: "2.0.28".into(),
            adapter_version: "1".into(),
            confidence: SignalConfidence::Authoritative,
            event_id,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity() -> SignalIdentity {
        SignalIdentity {
            agent_id: "agent".into(),
            worktree_id: Some("worktree".into()),
            launch_id: "launch".into(),
            generation: 1,
            native_session_id: "ses_owned".into(),
        }
    }
    fn info() -> Value {
        json!({"data":{"id":"ses_owned","location":{"directory":"/owned"},"time":{"created":1,"updated":2,"idle":2},"outcome":"succeeded"}})
    }
    fn write_registration(path: &Path, url: &str, version: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            path,
            serde_json::to_vec(
                &json!({"url":url,"pid":123,"version":version,"password":"private-fixture"}),
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[test]
    fn given_registration_should_accept_only_private_owner_file_and_local_pinned_service() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("service.json");
        write_registration(&path, "http://127.0.0.1:1234", "2.0.28");
        assert!(registration(&path).is_some());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(registration(&path).is_none());
        for url in [
            "http://example.com:1234",
            "https://127.0.0.1:1234",
            "http://127.0.0.1:1234/other",
            "http://user@127.0.0.1:1234",
        ] {
            write_registration(&path, url, "2.0.28");
            assert!(registration(&path).is_none());
        }
        write_registration(&path, "http://127.0.0.1:1234", "2.0.29");
        assert!(registration(&path).is_none());
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(registration(&link).is_none());
    }
    #[tokio::test]
    async fn given_existing_service_should_read_native_snapshots_without_starting_server_or_mutating_requests()
     {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let native = Arc::new(Mutex::new(info()));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let server_native = native.clone();
        let server_observed = observed.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = vec![0; 8192];
                let size = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                let path = request
                    .lines()
                    .next()
                    .unwrap()
                    .strip_prefix("GET ")
                    .expect("Observer may only GET")
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_owned();
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: basic b3blbmnvzgu6chjpdmf0zs1maxh0dxjl")
                );
                server_observed.lock().unwrap().push(path.clone());
                let body = match path.as_str() {
                    "/api/info" => json!({"version":"2.0.28","pid":123}),
                    "/api/session/active" => json!({"data":{}}),
                    "/api/session/ses_owned" => server_native.lock().unwrap().clone(),
                    "/api/session/ses_owned/permission" => {
                        json!({"data":[{"id":"perm_one","sessionID":"ses_owned"}]})
                    }
                    "/api/session/ses_owned/form" => {
                        json!({"data":[{"id":"form_one","sessionID":"ses_owned"}]})
                    }
                    _ => panic!("Unexpected observer endpoint"),
                }
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("opencode")).unwrap();
        let path = temp.path().join("opencode/service.json");
        write_registration(&path, &url, "2.0.28");
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let current = poll(&client, &path, &identity(), "/owned").await.unwrap();
        assert_eq!(current.pending_attention.len(), 2);
        assert_eq!(current.outcome.unwrap().kind, OutcomeKind::Completed);
        assert_eq!(observed.lock().unwrap().len(), 8);
        native.lock().unwrap()["data"]["location"]["directory"] = "/foreign".into();
        assert!(poll(&client, &path, &identity(), "/owned").await.is_none());
        *native.lock().unwrap() = info();
        let environment = vec![(
            "XDG_STATE_HOME".into(),
            temp.path().to_string_lossy().into_owned(),
        )];
        let prepared = prepare(
            temp.path().to_str().unwrap(),
            "agent",
            Some("worktree".into()),
            "ses_owned",
            &[],
            &environment,
        )
        .unwrap();
        let states = super::super::SignalStates::default();
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
        prepared.activate(&states, "project", "/owned", 777, exit_rx, Arc::default());
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let current = states
                    .lock()
                    .unwrap()
                    .get("agent")
                    .and_then(|s| s.snapshot());
                if current.is_some_and(|s| s.pending_attention.len() == 2) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        exit_tx.send(Some(0)).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let state = states
                    .lock()
                    .unwrap()
                    .get("agent")
                    .and_then(|s| s.snapshot());
                if state.is_some_and(|s| s.observation.state == ObservationState::ProcessDead) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
    }
    #[test]
    fn given_native_opencode_snapshot_should_project_actual_execution_and_terminal_outcomes() {
        let empty = json!({"data":[]});
        let active = json!({"data":{"ses_owned":{"type":"running"}}});
        let running =
            snapshot(&identity(), "/owned", &info(), &active, &empty, &empty, 100).unwrap();
        assert_eq!(running.activity, SignalActivity::Working);
        assert!(
            running.outcome.is_none(),
            "Old outcome must not reappear during a new execution"
        );
        for (native, expected) in [
            ("succeeded", OutcomeKind::Completed),
            ("failed", OutcomeKind::Errored),
            ("interrupted", OutcomeKind::Interrupted),
        ] {
            let mut session = info();
            session["data"]["outcome"] = native.into();
            let idle = snapshot(
                &identity(),
                "/owned",
                &session,
                &json!({"data":{}}),
                &empty,
                &empty,
                101,
            )
            .unwrap();
            assert_eq!(idle.activity, SignalActivity::Idle);
            assert_eq!(idle.outcome.unwrap().kind, expected);
        }
    }
    #[test]
    fn given_native_pending_lists_should_keep_exact_permission_and_question_ids_and_remove_resolved_requests()
     {
        let permissions = json!({"data":[{"id":"perm_one","sessionID":"ses_owned"}]});
        let forms = json!({"data":[{"id":"form_one","sessionID":"ses_owned"}]});
        let active = json!({"data":{"ses_owned":{"type":"running"}}});
        let pending = snapshot(
            &identity(),
            "/owned",
            &info(),
            &active,
            &permissions,
            &forms,
            100,
        )
        .unwrap();
        assert_eq!(pending.pending_attention.len(), 2);
        assert_eq!(pending.pending_attention[0].kind, AttentionKind::Permission);
        assert_eq!(pending.pending_attention[1].kind, AttentionKind::Question);
        assert_eq!(pending.pending_attention[1].request_id, "form_one");
        let cleared = snapshot(
            &identity(),
            "/owned",
            &info(),
            &active,
            &json!({"data":[]}),
            &json!({"data":[]}),
            101,
        )
        .unwrap();
        assert!(cleared.pending_attention.is_empty());
        assert!(
            pending
                .pending_attention
                .iter()
                .all(|r| !r.actionability.macos && !r.actionability.ios)
        );
    }
    #[test]
    fn given_foreign_child_or_malformed_snapshots_should_refuse_to_attribute_them_to_owned_root() {
        let empty = json!({"data":[]});
        let inactive = json!({"data":{}});
        for field in ["id", "parentID", "location"] {
            let mut session = info();
            session["data"][field] = match field {
                "location" => json!({"directory":"/other"}),
                _ => json!("foreign"),
            };
            assert!(
                snapshot(
                    &identity(),
                    "/owned",
                    &session,
                    &inactive,
                    &empty,
                    &empty,
                    100
                )
                .is_none()
            );
        }
        assert!(
            snapshot(
                &identity(),
                "/owned",
                &info(),
                &inactive,
                &json!({"data":[{"id":"wrong","sessionID":"foreign"}]}),
                &empty,
                100
            )
            .is_none()
        );
        assert!(
            snapshot(
                &identity(),
                "/owned",
                &info(),
                &inactive,
                &json!({"data":null}),
                &empty,
                100
            )
            .is_none()
        );
    }
}
