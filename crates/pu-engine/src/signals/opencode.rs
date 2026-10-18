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
    outcome_fence: OutcomeFence,
}

#[derive(Clone)]
struct NativeObservation {
    snapshot: SignalSnapshot,
    idle_revision: Option<i64>,
    materialized: bool,
}

#[derive(Clone)]
struct Root {
    identity: SignalIdentity,
    cwd: String,
}
struct Service {
    client: reqwest::Client,
    sampler: Arc<super::batch::Sampler<Root, NativeObservation>>,
}
/// Groups only observers of the same private native service registration.
/// Weak entries and registration guards prevent retaining finished launches.
#[derive(Default)]
pub(super) struct Services(Mutex<HashMap<PathBuf, Weak<Service>>>);
impl Services {
    fn group(&self, path: &Path) -> Option<Arc<Service>> {
        let mut services = self.0.lock().unwrap();
        services.retain(|_, service| service.strong_count() > 0);
        if let Some(service) = services.get(path).and_then(Weak::upgrade) {
            return Some(service);
        }
        if services.len() >= 256 {
            return None;
        }
        let service = Arc::new(Service {
            client: client()?,
            sampler: Arc::new(super::batch::Sampler::new(Duration::from_millis(500))),
        });
        services.insert(path.to_owned(), Arc::downgrade(&service));
        Some(service)
    }
}

struct OutcomeFence {
    minimum_idle: Option<i64>,
    high_water: Option<i64>,
    initialized: bool,
    materialized: bool,
}
impl Default for OutcomeFence {
    fn default() -> Self {
        Self {
            minimum_idle: None,
            high_water: None,
            initialized: true,
            materialized: false,
        }
    }
}
impl OutcomeFence {
    fn resumed(revision: Option<i64>, initialized: bool) -> Self {
        Self {
            minimum_idle: revision,
            high_water: revision,
            initialized,
            materialized: true,
        }
    }
    fn apply(&mut self, value: NativeObservation) -> Option<SignalSnapshot> {
        if self.materialized && !value.materialized {
            return None;
        }
        // Native projector.ts advances time.idle strictly on every terminal,
        // including same-millisecond completion and wall-clock rollback. It is
        // a native revision, not our receipt timestamp or metadata updated time.
        if value.idle_revision < self.high_water {
            return None;
        }
        self.high_water = value.idle_revision;
        self.materialized |= value.materialized;
        if !self.initialized {
            self.minimum_idle = value.idle_revision;
            self.initialized = true;
        }
        let mut snapshot = value.snapshot;
        // This is a fresh, authenticated full view of the same native session.
        // The managed service can keep executing after its TUI is suspended;
        // live activity/requests therefore remain valid when that TUI resumes.
        // They are current conditions, unlike a persisted past terminal event.
        if snapshot.activity == SignalActivity::Working {
            self.minimum_idle = self.minimum_idle.max(value.idle_revision);
        }
        if snapshot.outcome.is_some() && value.idle_revision <= self.minimum_idle {
            snapshot.outcome = None;
            if let Some(evidence) = snapshot.evidence.as_mut() {
                evidence.event_id = format!(
                    "opencode-state:{}:idle:{}",
                    snapshot.identity.native_session_id, value.idle_revision?
                );
            }
        }
        Some(snapshot)
    }
}

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(1))
        .build()
        .ok()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn prepare_launch(
    project: &str,
    cwd: &str,
    agent: &str,
    worktree: Option<String>,
    session: &str,
    args: &[String],
    env: &[(String, String)],
    resuming: bool,
) -> Option<Prepared> {
    let mut prepared = prepare(project, agent, worktree, session, args, env)?;
    if resuming {
        let identity = prepared.state.identity.lock().unwrap().clone();
        let Some(super::NativeObserver::OpenCode(observer)) = prepared.observer.as_mut() else {
            return None;
        };
        let baseline = tokio::time::timeout(Duration::from_millis(500), async {
            poll(&client()?, &observer.registration, &identity, cwd).await
        })
        .await
        .ok()
        .flatten();
        observer.outcome_fence = match baseline {
            Some(value) => OutcomeFence::resumed(value.idle_revision, true),
            None => OutcomeFence::resumed(None, false),
        };
    }
    Some(prepared)
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
            outcome_fence: OutcomeFence::default(),
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

async fn fetch_json(
    client: &reqwest::Client,
    r: &Registration,
    path: &str,
) -> Option<(reqwest::StatusCode, Value)> {
    let url = reqwest::Url::parse(&r.url).ok()?.join(path).ok()?;
    let mut request = client.get(url);
    if let Some(password) = &r.password {
        request = request.basic_auth("opencode", Some(password));
    }
    let mut response = request.send().await.ok()?;
    let status = response.status();
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
    Some((status, serde_json::from_slice(&bytes).ok()?))
}

async fn get(client: &reqwest::Client, r: &Registration, path: &str) -> Option<Value> {
    let (status, value) = fetch_json(client, r, path).await?;
    status.is_success().then_some(value)
}

async fn get_session(
    client: &reqwest::Client,
    r: &Registration,
    path: &str,
    session: &str,
) -> Option<Option<Value>> {
    let (status, value) = fetch_json(client, r, path).await?;
    session_response(status, value, session)
}

fn session_response(
    status: reqwest::StatusCode,
    value: Value,
    session: &str,
) -> Option<Option<Value>> {
    if status.is_success() {
        return Some(Some(value));
    }
    if status == reqwest::StatusCode::NOT_FOUND
        && value["_tag"] == "SessionNotFoundError"
        && value["sessionID"].as_str() == Some(session)
    {
        return Some(None);
    }
    None
}

async fn poll(
    client: &reqwest::Client,
    path: &Path,
    identity: &SignalIdentity,
    cwd: &str,
) -> Option<NativeObservation> {
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
    Some(NativeObservation {
        snapshot: snapshot(identity, cwd, &info, &after, &permissions, &forms, now_ms())?,
        idle_revision: info["data"]["time"]["idle"].as_i64(),
        materialized: true,
    })
}

// Per-service bracketing avoids downloading the global active map per agent.
// Each root still has its own metadata bracket and exact identity/cwd checks.
// A changing unrelated root does not invalidate another root's current view.
async fn poll_batch(
    client: &reqwest::Client,
    path: &Path,
    roots: Vec<(String, Root)>,
) -> HashMap<String, Option<NativeObservation>> {
    async fn sample(
        client: &reqwest::Client,
        path: &Path,
        roots: Vec<(String, Root)>,
    ) -> Option<HashMap<String, Option<NativeObservation>>> {
        let path = path.to_owned();
        let r = tokio::task::spawn_blocking(move || registration(&path))
            .await
            .ok()??;
        let health = get(client, &r, "/api/info").await?;
        if health["pid"].as_u64() != Some(u64::from(r.pid)) || health["version"] != r.version {
            return None;
        }
        let before = get(client, &r, "/api/session/active").await?;
        before["data"].as_object()?;
        let r = Arc::new(r);
        let mut pending = roots.into_iter();
        let mut tasks = tokio::task::JoinSet::new();
        let mut observed = Vec::new();
        loop {
            while tasks.len() < 32 {
                let Some((id, root)) = pending.next() else {
                    break;
                };
                let (client, r) = (client.clone(), r.clone());
                tasks.spawn(async move {
                    let value = async {
                        let session = format!("/api/session/{}", root.identity.native_session_id);
                        let sid = &root.identity.native_session_id;
                        let info = get_session(&client, &r, &session, sid).await?;
                        let permissions =
                            get_session(&client, &r, &format!("{session}/permission"), sid).await?;
                        let forms =
                            get_session(&client, &r, &format!("{session}/form"), sid).await?;
                        Some((info, permissions, forms))
                    }
                    .await;
                    (id, root, value)
                });
            }
            if tasks.is_empty() {
                break;
            }
            observed.push(tasks.join_next().await?.ok()?);
        }
        let after = get(client, &r, "/api/session/active").await?;
        after["data"].as_object()?;
        let mut tasks = tokio::task::JoinSet::new();
        let mut pending = observed.into_iter();
        let mut observed = Vec::new();
        loop {
            while tasks.len() < 32 {
                let Some((id, root, value)) = pending.next() else {
                    break;
                };
                let (client, r) = (client.clone(), r.clone());
                tasks.spawn(async move {
                    let value = async {
                        let (info, permissions, forms) = value?;
                        let session = format!("/api/session/{}", root.identity.native_session_id);
                        if get_session(&client, &r, &session, &root.identity.native_session_id)
                            .await?
                            != info
                        {
                            return None;
                        }
                        Some((info, permissions, forms))
                    }
                    .await;
                    (id, root, value)
                });
            }
            if tasks.is_empty() {
                break;
            }
            observed.push(tasks.join_next().await?.ok()?);
        }

        if get(client, &r, "/api/info").await? != health {
            return None;
        }
        let now = now_ms();
        Some(
            observed
                .into_iter()
                .map(|(id, root, value)| {
                    let sid = &root.identity.native_session_id;
                    let value = value.and_then(|(info, permissions, forms)| {
                        if before["data"][sid] != after["data"][sid] {
                            return None;
                        }
                        observation(
                            &root.identity,
                            &root.cwd,
                            info.as_ref(),
                            &after,
                            permissions.as_ref(),
                            forms.as_ref(),
                            now,
                        )
                    });
                    (id, value)
                })
                .collect(),
        )
    }
    if roots.is_empty() || roots.len() > 256 {
        return HashMap::new();
    }
    tokio::time::timeout(Duration::from_secs(2), sample(client, path, roots))
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

impl Observer {
    pub(super) fn activate(
        self,
        state: Weak<LaunchState>,
        project: String,
        cwd: String,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
        services: &Services,
    ) {
        let Some(service) = services.group(&self.registration) else {
            return;
        };
        let Some(identity) = state.upgrade().map(|s| s.identity.lock().unwrap().clone()) else {
            return;
        };
        let launch = identity.launch_id.clone();
        let Some(registration) = service
            .sampler
            .register(launch.clone(), Root { identity, cwd })
        else {
            return;
        };
        tokio::spawn(async move {
            let _registration = registration;
            let mut outcome_fence = self.outcome_fence;
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            let mut publication = super::NativePublication::default();
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {_=interval.tick()=>{},_=exit.changed()=>{}}
                let Some(state) = state.upgrade() else {
                    break;
                };
                let dead = exit.borrow().is_some() || exit.has_changed().is_err();
                let result = if dead {
                    None
                } else {
                    service
                        .sampler
                        .sample(&launch, false, |roots| {
                            poll_batch(&service.client, &self.registration, roots)
                        })
                        .await
                        .and_then(|value| outcome_fence.apply(value))
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
                if publication.should_publish(state.snapshot(), tokio::time::Instant::now())
                    && let Some(tx) = channels.lock().await.get(&project)
                {
                    let _ = tx.send(());
                }
                if dead {
                    break;
                }
            }
        });
    }
}

fn observation(
    identity: &SignalIdentity,
    cwd: &str,
    info: Option<&Value>,
    active: &Value,
    permissions: Option<&Value>,
    forms: Option<&Value>,
    now: i64,
) -> Option<NativeObservation> {
    match (info, permissions, forms) {
        (Some(info), Some(permissions), Some(forms)) => Some(NativeObservation {
            snapshot: snapshot(identity, cwd, info, active, permissions, forms, now)?,
            idle_revision: info["data"]["time"]["idle"].as_i64(),
            materialized: true,
        }),
        (None, None, None)
            if !active["data"]
                .as_object()?
                .contains_key(&identity.native_session_id) =>
        {
            // Before the first prompt, all three native endpoints report the
            // assigned session as not found and the service has no execution.
            // This establishes only initial idle; disappearance of an observed
            // or resumed root is rejected by the per-launch outcome fence.
            Some(NativeObservation {
                materialized: false,
                idle_revision: None,
                snapshot: SignalSnapshot {
                    schema_version: 1,
                    identity: identity.clone(),
                    activity: SignalActivity::Idle,
                    pending_attention: Vec::new(),
                    outcome: None,
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
                        event_id: format!("opencode-draft:{}", identity.native_session_id),
                    }),
                },
            })
        }
        _ => None,
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

    #[test]
    fn only_exact_native_session_not_found_is_an_initial_draft_observation() {
        let missing = json!({"_tag":"SessionNotFoundError","sessionID":"ses_owned"});
        assert_eq!(
            session_response(reqwest::StatusCode::NOT_FOUND, missing.clone(), "ses_owned"),
            Some(None)
        );
        assert!(
            session_response(
                reqwest::StatusCode::NOT_FOUND,
                missing.clone(),
                "ses_foreign"
            )
            .is_none()
        );
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert!(session_response(status, missing.clone(), "ses_owned").is_none());
        }
        assert!(
            session_response(
                reqwest::StatusCode::NOT_FOUND,
                json!({"message":"Not found"}),
                "ses_owned"
            )
            .is_none()
        );
    }

    #[test]
    fn missing_initial_root_is_idle_but_disappearing_or_resumed_root_is_unavailable() {
        let inactive = json!({"data":{}});
        let draft = observation(&identity(), "/owned", None, &inactive, None, None, 100).unwrap();
        assert_eq!(draft.snapshot.activity, SignalActivity::Idle);
        assert!(draft.snapshot.outcome.is_none() && draft.snapshot.pending_attention.is_empty());
        assert!(
            observation(
                &identity(),
                "/owned",
                None,
                &json!({"data":{"ses_owned":{"type":"running"}}}),
                None,
                None,
                100
            )
            .is_none()
        );
        assert!(
            observation(
                &identity(),
                "/owned",
                None,
                &inactive,
                Some(&json!({"data":[]})),
                None,
                100
            )
            .is_none()
        );
        assert!(observation(&identity(), "/owned", None, &json!({}), None, None, 100).is_none());
        let mut fence = OutcomeFence::default();
        assert!(fence.apply(draft.clone()).is_some());
        let mut root = info();
        root["data"]["outcome"] = Value::Null;
        root["data"]["time"]["idle"] = Value::Null;
        let empty = json!({"data":[]});
        let existing = observation(
            &identity(),
            "/owned",
            Some(&root),
            &inactive,
            Some(&empty),
            Some(&empty),
            200,
        )
        .unwrap();
        assert!(fence.apply(existing).is_some());
        assert!(fence.apply(draft.clone()).is_none());
        assert!(OutcomeFence::resumed(None, false).apply(draft).is_none());
    }

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
    fn observed(idle: i64, running: bool) -> NativeObservation {
        let mut native = info();
        native["data"]["time"]["idle"] = json!(idle);
        NativeObservation {
            snapshot: snapshot(
                &identity(),
                "/owned",
                &native,
                &if running {
                    json!({"data":{"ses_owned":{"type":"running"}}})
                } else {
                    json!({"data":{}})
                },
                &json!({"data":[]}),
                &json!({"data":[]}),
                100,
            )
            .unwrap(),
            idle_revision: Some(idle),
            materialized: true,
        }
    }
    #[test]
    fn given_resumed_opencode_should_require_a_new_native_terminal_revision() {
        let mut fence = OutcomeFence::resumed(Some(2), true);
        assert!(fence.apply(observed(2, false)).unwrap().outcome.is_none());
        // The native projector advances time.idle strictly on every terminal,
        // even same-millisecond completions and wall-clock rollback.
        assert_eq!(
            fence
                .apply(observed(3, false))
                .unwrap()
                .outcome
                .unwrap()
                .kind,
            OutcomeKind::Completed
        );
        assert_eq!(
            fence.apply(observed(3, true)).unwrap().activity,
            SignalActivity::Working
        );
        assert!(
            fence.apply(observed(3, false)).unwrap().outcome.is_none(),
            "prior outcome must not reappear after a new execution"
        );
        assert!(fence.apply(observed(4, false)).unwrap().outcome.is_some());
        assert!(
            fence.apply(observed(2, false)).is_none(),
            "reverted native counter is unavailable, not a new terminal"
        );
    }
    #[test]
    fn given_missing_prelaunch_opencode_view_should_baseline_the_first_complete_observation() {
        let mut fence = OutcomeFence::resumed(None, false);
        assert!(fence.apply(observed(50, false)).unwrap().outcome.is_none());
        assert!(fence.apply(observed(50, false)).unwrap().outcome.is_none());
        assert!(fence.apply(observed(51, false)).unwrap().outcome.is_some());
        let mut fresh = OutcomeFence::default();
        assert!(
            fresh.apply(observed(2, false)).unwrap().outcome.is_some(),
            "new root sessions have no inherited outcome"
        );
    }
    #[test]
    fn given_resumed_native_session_still_live_should_preserve_current_work_and_requests() {
        let mut fence = OutcomeFence::resumed(Some(2), true);
        let current = NativeObservation {
            snapshot: snapshot(
                &identity(),
                "/owned",
                &info(),
                &json!({"data":{"ses_owned":{"type":"running"}}}),
                &json!({"data":[{"id":"per_live","sessionID":"ses_owned"}]}),
                &json!({"data":[]}),
                100,
            )
            .unwrap(),
            idle_revision: Some(2),
            materialized: true,
        };
        let current = fence.apply(current).unwrap();
        assert_eq!(current.activity, SignalActivity::Working);
        assert_eq!(current.pending_attention[0].request_id, "per_live");
        assert_eq!(
            current.pending_attention[0].confidence,
            SignalConfidence::Authoritative
        );
        assert!(current.outcome.is_none());
        // Once the live service clears those conditions, do not retain them or
        // resurrect the unchanged terminal from before this observer launch.
        let ended = fence.apply(observed(2, false)).unwrap();
        assert_eq!(ended.activity, SignalActivity::Idle);
        assert!(ended.pending_attention.is_empty());
        assert!(ended.outcome.is_none());
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
    async fn given_multiple_roots_should_share_service_reads_and_preserve_each_root() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let requests = observed.clone();
        let racing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let active_reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_racing = racing.clone();
        let server_reads = active_reads.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 8192];
                let n = stream.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]);
                let line = request.lines().next().unwrap();
                assert!(line.starts_with("GET "));
                let path = line.split_whitespace().nth(1).unwrap().to_owned();
                requests.lock().unwrap().push(path.clone());
                let body = match path.as_str() {
                    "/api/info" => json!({"version":"2.0.28","pid":123}),
                    "/api/session/active" => {
                        let n = server_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let mut value = json!({"data":{"ses_1":{"type":"running"}}});
                        if server_racing.load(std::sync::atomic::Ordering::SeqCst) && n > 0 {
                            value["data"]["unrelated"] = json!({"type":"running"});
                        }
                        value
                    }
                    p if p.ends_with("/permission") || p.ends_with("/form") => json!({"data":[]}),
                    p => {
                        let sid = p.strip_prefix("/api/session/").unwrap();
                        let mut value = info();
                        value["data"]["id"] = sid.into();
                        if sid == "ses_1"
                            && server_racing.load(std::sync::atomic::Ordering::SeqCst)
                            && server_reads.load(std::sync::atomic::Ordering::SeqCst) >= 2
                        {
                            value["data"]["time"]["idle"] = 99999.into();
                        }
                        value
                    }
                }
                .to_string();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("opencode")).unwrap();
        let path = temp.path().join("opencode/service.json");
        write_registration(&path, &url, "2.0.28");
        let roots: Vec<_> = (0..3)
            .map(|n| {
                let mut identity = identity();
                identity.native_session_id = format!("ses_{n}");
                identity.launch_id = format!("launch-{n}");
                (
                    n.to_string(),
                    Root {
                        identity,
                        cwd: "/owned".into(),
                    },
                )
            })
            .collect();
        let values = poll_batch(&client().unwrap(), &path, roots.clone()).await;
        assert_eq!(values.len(), 3);
        assert_eq!(
            values["1"].as_ref().unwrap().snapshot.activity,
            SignalActivity::Working
        );
        assert_eq!(
            values["0"]
                .as_ref()
                .unwrap()
                .snapshot
                .identity
                .native_session_id,
            "ses_0"
        );
        assert_eq!(
            values["2"].as_ref().unwrap().snapshot.identity.launch_id,
            "launch-2"
        );
        {
            let requests = observed.lock().unwrap();
            assert_eq!(requests.len(), 4 + 4 * 3);
            assert_eq!(
                requests
                    .iter()
                    .filter(|p| *p == "/api/session/active")
                    .count(),
                2
            );
            assert_eq!(requests.iter().filter(|p| *p == "/api/info").count(), 2);
        }
        racing.store(true, std::sync::atomic::Ordering::SeqCst);
        active_reads.store(0, std::sync::atomic::Ordering::SeqCst);
        let raced = poll_batch(&client().unwrap(), &path, roots).await;
        assert!(
            raced["1"].is_none(),
            "Metadata changed across the active-map bracket"
        );
        assert!(
            raced["0"].is_some(),
            "Unrelated activity must not invalidate root 0"
        );
        assert!(
            raced["2"].is_some(),
            "Unrelated activity must not invalidate root 2"
        );
        racing.store(false, std::sync::atomic::Ordering::SeqCst);
        observed.lock().unwrap().clear();
        let sources = Arc::new(super::super::NativeSources::default());
        let states = super::super::SignalStates::default();
        let (exit, receiver) = tokio::sync::watch::channel(None);
        let environment = vec![(
            "XDG_STATE_HOME".into(),
            temp.path().to_string_lossy().into_owned(),
        )];
        for n in 0..3 {
            prepare(
                temp.path().to_str().unwrap(),
                &format!("agent{n}"),
                None,
                &format!("ses_{n}"),
                &[],
                &environment,
            )
            .unwrap()
            .activate_with_sources(
                &states,
                "project",
                "/owned",
                777,
                receiver.clone(),
                Arc::default(),
                sources.clone(),
            );
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if states
                    .lock()
                    .unwrap()
                    .values()
                    .filter(|state| state.snapshot().is_some())
                    .count()
                    == 3
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        {
            let requests = observed.lock().unwrap();
            let first = &requests[..16];
            assert_eq!(
                first.iter().filter(|p| *p == "/api/session/active").count(),
                2,
                "Production launch activation must share its service sampler"
            );
            assert_eq!(first.iter().filter(|p| *p == "/api/info").count(), 2);
            for n in 0..3 {
                assert_eq!(
                    first
                        .iter()
                        .filter(|p| *p == &format!("/api/session/ses_{n}"))
                        .count(),
                    2
                );
            }
        }
        exit.send(Some(0)).unwrap();
        task.abort();
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
        assert_eq!(current.snapshot.pending_attention.len(), 2);
        assert_eq!(
            current.snapshot.outcome.unwrap().kind,
            OutcomeKind::Completed
        );
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
    #[tokio::test]
    #[ignore = "requires sandbox native OpenCode probe context"]
    async fn native_interactive_observer_probe() {
        let path =
            std::env::var("PUREPOINT_OPENCODE_PROBE_CONTEXT").expect("sandbox context required");
        let context: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let cwd = context["cwd"].as_str().unwrap();
        let env: Vec<(String, String)> = serde_json::from_value(context["env"].clone()).unwrap();
        let prepared = prepare(
            cwd,
            "probe",
            None,
            context["session"].as_str().unwrap(),
            &[],
            &env,
        )
        .unwrap();
        let states = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = tokio::sync::watch::channel(None);
        prepared.activate(
            &states,
            cwd,
            cwd,
            context["pid"].as_u64().unwrap() as u32,
            rx,
            Arc::default(),
        );
        let mut captures = Vec::new();
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Some(snapshot) = states
                .lock()
                .unwrap()
                .get("probe")
                .and_then(|s| s.snapshot())
            {
                captures.push(snapshot);
            }
        }
        tx.send(Some(0)).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            states
                .lock()
                .unwrap()
                .get("probe")
                .unwrap()
                .snapshot()
                .unwrap()
                .observation
                .state,
            ObservationState::ProcessDead
        );
        std::fs::write(
            Path::new(&path).with_file_name("observer-captures.json"),
            serde_json::to_vec(&captures).unwrap(),
        )
        .unwrap();
        let current: Vec<_> = captures
            .iter()
            .filter(|s| s.is_current(s.observation.last_received_at_ms.unwrap()))
            .collect();
        assert!(
            current
                .iter()
                .any(|s| s.activity == SignalActivity::Working)
        );
        let expected = context["expected"].as_str().unwrap();
        if expected == "error" {
            assert!(current.iter().any(|s| {
                s.outcome
                    .as_ref()
                    .is_some_and(|o| o.kind == OutcomeKind::Errored)
            }));
        } else {
            if expected != "completion" {
                let kind = if expected == "question" {
                    AttentionKind::Question
                } else {
                    AttentionKind::Permission
                };
                assert!(
                    current
                        .iter()
                        .any(|s| s.pending_attention.iter().any(|p| p.kind == kind))
                );
            }
            assert!(current.iter().any(|s| {
                s.pending_attention.is_empty()
                    && s.outcome
                        .as_ref()
                        .is_some_and(|o| o.kind == OutcomeKind::Completed)
            }));
        }
    }
}
