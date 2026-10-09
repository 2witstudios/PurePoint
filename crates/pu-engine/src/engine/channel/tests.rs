use super::*;
use crate::engine::Engine;
use pu_core::types::{AgentEntry, Manifest, WorktreeEntry};

fn setup() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".pu")).unwrap();
    let root = temp.path().to_string_lossy().into_owned();
    let mut manifest = Manifest::new(root.clone());
    let agent: AgentEntry = serde_json::from_value(serde_json::json!({"id":"ag-one", "name":"Builder", "agentType":"codex", "status":"running", "startedAt":"2026-10-09T00:00:00Z"})).unwrap();
    let worktree: WorktreeEntry = serde_json::from_value(serde_json::json!({"id":"wt-one", "name":"feature", "path":"/unused", "branch":"pu/feature", "status":"active", "agents":{"ag-one":agent}, "createdAt":"2026-10-09T00:00:00Z"})).unwrap();
    manifest.worktrees.insert("wt-one".into(), worktree);
    pu_core::manifest::write_manifest(temp.path(), &manifest).unwrap();
    (temp, root)
}
fn read(root: &str, agent_id: Option<&str>) -> Request {
    Request::ChannelRead {
        project_root: root.into(),
        agent_id: agent_id.map(str::to_owned),
        after: None,
        before: None,
        limit: 100,
        query: None,
        parent_id: None,
        known_revision: None,
    }
}
#[tokio::test]
async fn given_registered_agent_should_snapshot_context_and_preserve_removed_author_after_restart()
{
    let (temp, root) = setup();
    let engine = Engine::new();
    let sent = engine
        .handle_request(Request::ChannelSend {
            project_root: root.clone(),
            agent_id: Some("ag-one".into()),
            text: "hello".into(),
            parent_id: None,
            references: vec![],
        })
        .await;
    let message = match sent {
        Response::ChannelMessage { message, revision } => {
            assert_eq!(revision, 1);
            message
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(message.author.id, "ag-one");
    assert_eq!(message.author.name, "Builder");
    assert_eq!(message.author.agent_type.as_deref(), Some("codex"));
    assert_eq!(message.author.worktree_id.as_deref(), Some("wt-one"));
    assert_eq!(message.author.branch.as_deref(), Some("pu/feature"));
    assert!(
        matches!(engine.handle_request(read(&root, Some("ag-one"))).await, Response::ChannelHistory { self_author_id, .. } if self_author_id == "ag-one")
    );
    pu_core::manifest::update_manifest(temp.path(), |mut m| {
        m.worktrees.clear();
        m
    })
    .unwrap();
    let restarted = Engine::new();
    match restarted.handle_request(read(&root, None)).await {
        Response::ChannelHistory {
            messages,
            self_author_id,
            ..
        } => {
            assert_eq!(messages, vec![message.clone()]);
            assert!(self_author_id.starts_with("human:"));
            let human = restarted
                .handle_request(Request::ChannelSend {
                    project_root: root.clone(),
                    agent_id: None,
                    text: "human".into(),
                    parent_id: None,
                    references: vec![],
                })
                .await;
            assert!(
                matches!(human, Response::ChannelMessage { message, .. } if message.author.id == self_author_id)
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        restarted.handle_request(read(&root, Some("ag-one"))).await,
        Response::Error { .. }
    ));
    assert!(
        matches!(restarted.handle_request(Request::ChannelEdit { project_root: root, agent_id: None, message_id: message.id, text: "changed".into() }).await, Response::Error { code, .. } if code == "CHANNEL_OWNERSHIP")
    );
}
#[tokio::test]
async fn given_wire_requests_should_apply_defaults_and_return_exact_contract() {
    let (_temp, root) = setup();
    let engine = Engine::new();
    let send: Request = serde_json::from_value(
        serde_json::json!({"type":"channel_send", "project_root":root, "text":"hello"}),
    )
    .unwrap();
    let response = engine.handle_request(send).await;
    let message_id = match response {
        Response::ChannelMessage { message, .. } => message.id,
        other => panic!("{other:?}"),
    };
    let react: Request = serde_json::from_value(serde_json::json!({"type":"channel_react", "project_root":root, "message_id":message_id, "active":true})).unwrap();
    assert!(
        matches!(engine.handle_request(react).await, Response::ChannelMessage { message, revision: 2 } if message.reactions[0].emoji == "👍")
    );
    let read: Request =
        serde_json::from_value(serde_json::json!({"type":"channel_read", "project_root":root}))
            .unwrap();
    let json = serde_json::to_value(engine.handle_request(read).await).unwrap();
    assert_eq!(json["type"], "channel_history");
    for field in [
        "messages",
        "revision",
        "latest_sequence",
        "has_more",
        "oldest_sequence",
        "unchanged",
        "self_author_id",
        "reply_counts",
    ] {
        assert!(json.get(field).is_some(), "missing {field}");
    }
    assert_eq!(json["reply_counts"][&message_id], 0);
    assert!(json["messages"][0].get("created_at").is_some());
    assert!(json["messages"][0]["author"].get("agent_type").is_some());
}
#[tokio::test]
async fn given_invalid_identity_or_corrupt_store_should_report_error() {
    let (temp, root) = setup();
    let engine = Engine::new();
    assert!(
        matches!(engine.handle_request(read(&root, Some("unknown"))).await, Response::Error { code, .. } if code == "CHANNEL_INVALID")
    );
    assert!(matches!(
        engine.handle_request(read(&root, Some(""))).await,
        Response::Error { .. }
    ));
    std::fs::write(temp.path().join(".pu/channel.json"), "partial").unwrap();
    assert!(
        matches!(engine.handle_request(read(&root, None)).await, Response::Error { code, .. } if code == "CHANNEL_CORRUPT")
    );
    std::fs::write(
        temp.path().join(".pu/channel.json"),
        r#"{"version":999,"revision":0,"latest_sequence":0,"messages":[]}"#,
    )
    .unwrap();
    assert!(
        matches!(engine.handle_request(read(&root, None)).await, Response::Error { code, .. } if code == "CHANNEL_VERSION")
    );
}
