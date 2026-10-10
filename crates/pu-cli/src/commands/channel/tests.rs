use super::*;
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn given_root_subdirectory_and_linked_worktree_should_route_to_primary_project() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init"]);
    std::fs::create_dir(root.join(".pu")).unwrap();
    pu_core::manifest::write_manifest(
        &root,
        &pu_core::types::Manifest::new(root.to_string_lossy().into_owned()),
    )
    .unwrap();
    git(
        &root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    let sub = root.join("deep/path");
    std::fs::create_dir_all(&sub).unwrap();
    let linked = temp.path().join("linked");
    git(
        &root,
        &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
    );
    let linked_sub = linked.join("sub");
    std::fs::create_dir(&linked_sub).unwrap();
    let expected = std::fs::canonicalize(&root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for cwd in [&root, &sub, &linked, &linked_sub] {
        assert_eq!(resolve_root(None, None, cwd).unwrap(), expected);
    }
    assert_eq!(
        resolve_root(
            Some(root.to_str().unwrap()),
            Some("/does/not/exist"),
            &linked
        )
        .unwrap(),
        expected
    );
    assert_eq!(
        resolve_root(None, Some(root.to_str().unwrap()), temp.path()).unwrap(),
        expected
    );
    assert!(resolve_root(None, None, temp.path()).is_err());
}
#[test]
fn given_separate_git_dir_should_route_to_primary_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let metadata = temp.path().join("metadata/.git");
    std::fs::create_dir(metadata.parent().unwrap()).unwrap();
    git(
        &root,
        &["init", "--separate-git-dir", metadata.to_str().unwrap()],
    );
    let expected = std::fs::canonicalize(&root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(resolve_root(None, None, &root).unwrap(), expected);
    let sub = root.join("nested");
    std::fs::create_dir(&sub).unwrap();
    assert_eq!(resolve_root(None, None, &sub).unwrap(), expected);
    git(
        &root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    let linked = temp.path().join("linked");
    git(
        &root,
        &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
    );
    let unavailable = resolve_root(None, None, &linked).unwrap_err().to_string();
    assert!(unavailable.contains("use --project-root or PU_PROJECT_ROOT"));
    // Git does not record the primary path in separate metadata. An explicit
    // project path remains authoritative for agents in such linked worktrees.
    assert_eq!(
        resolve_root(Some(root.to_str().unwrap()), None, &linked).unwrap(),
        expected
    );
    assert_eq!(
        resolve_root(None, Some(root.to_str().unwrap()), &linked).unwrap(),
        expected
    );
}
#[test]
fn given_response_should_display_ids_cursors_references_and_reactions() {
    let message = pu_core::channel::ChannelMessage {
        id: "msg-1".into(),
        sequence: 3,
        parent_id: Some("msg-parent".into()),
        author: pu_core::channel::ChannelAuthor {
            id: "human:1".into(),
            name: "Developer".into(),
            kind: "human".into(),
            agent_type: None,
            branch: None,
            worktree_id: None,
        },
        text: "hello".into(),
        created_at: "2026-10-09T00:00:00Z".into(),
        edited_at: Some("2026-10-09T00:01:00Z".into()),
        references: vec![ChannelReference {
            kind: "pr".into(),
            value: "42".into(),
            label: None,
        }],
        reactions: vec![pu_core::channel::ChannelReaction {
            emoji: "👍".into(),
            author_ids: vec!["human:1".into()],
        }],
    };
    let response = Response::ChannelHistory {
        messages: vec![message],
        revision: 2,
        latest_sequence: 9,
        has_more: true,
        oldest_sequence: Some(3),
        unchanged: false,
        self_author_id: "human:1".into(),
        reply_counts: Default::default(),
    };
    let text = format_response(&response);
    for expected in [
        "msg-1",
        "reply-to=msg-parent",
        "hello",
        "(edited)",
        "pr: 42",
        "👍 ×1",
        "Latest sequence: 9",
        "oldest in this window: 3",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["type"], "channel_history");
    assert_eq!(json["self_author_id"], "human:1");
}

#[tokio::test]
async fn given_real_ipc_server_should_persist_cli_send_reply_edit_and_reaction() {
    use pu_core::types::Manifest;
    let temp = tempfile::tempdir().unwrap();
    // Keep the socket path under sockaddr_un's platform length bound.
    let socket = temp.path().join("ipc.sock");
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join(".pu")).unwrap();
    let root_text = root.to_string_lossy().into_owned();
    pu_core::manifest::write_manifest(&root, &Manifest::new(root_text.clone())).unwrap();
    let server =
        pu_engine::ipc_server::IpcServer::bind(&socket, pu_engine::engine::Engine::new()).unwrap();
    let task = tokio::spawn(async move {
        server.run().await.unwrap();
    });
    let sent = client::send_request(
        &socket,
        &Request::ChannelSend {
            project_root: root_text.clone(),
            agent_id: None,
            text: "human parent".into(),
            parent_id: None,
            references: vec![],
        },
    )
    .await
    .unwrap();
    let (parent, self_id) = match sent {
        Response::ChannelMessage {
            message,
            revision: 1,
        } => (message.id, message.author.id),
        other => panic!("{other:?}"),
    };
    let reply = client::send_request(
        &socket,
        &Request::ChannelSend {
            project_root: root_text.clone(),
            agent_id: None,
            text: "reply".into(),
            parent_id: Some(parent.clone()),
            references: vec![],
        },
    )
    .await
    .unwrap();
    let reply_id = match reply {
        Response::ChannelMessage {
            message,
            revision: 2,
        } => message.id,
        other => panic!("{other:?}"),
    };
    let edited = client::send_request(
        &socket,
        &Request::ChannelEdit {
            project_root: root_text.clone(),
            agent_id: None,
            message_id: reply_id.clone(),
            text: "edited reply".into(),
        },
    )
    .await
    .unwrap();
    assert!(
        matches!(edited, Response::ChannelMessage { revision: 3, message } if message.text == "edited reply" && message.sequence == 2)
    );
    let reacted = client::send_request(
        &socket,
        &Request::ChannelReact {
            project_root: root_text.clone(),
            agent_id: None,
            message_id: parent.clone(),
            emoji: "👍".into(),
            active: true,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        reacted,
        Response::ChannelMessage { revision: 4, .. }
    ));
    let response = client::send_request(
        &socket,
        &Request::ChannelRead {
            project_root: root_text.clone(),
            agent_id: None,
            after: Some(1),
            before: None,
            limit: 1,
            query: None,
            parent_id: Some(parent.clone()),
            known_revision: Some(4),
        },
    )
    .await
    .unwrap();
    match response {
        Response::ChannelHistory {
            messages,
            revision,
            oldest_sequence,
            latest_sequence,
            reply_counts,
            self_author_id,
            unchanged,
            has_more,
        } => {
            assert_eq!(revision, 4);
            assert_eq!(oldest_sequence, Some(2));
            assert_eq!(latest_sequence, 2);
            assert_eq!(reply_counts[&parent], 1);
            assert_eq!(self_author_id, self_id);
            assert!(!unchanged);
            assert!(!has_more);
            assert_eq!(
                messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
                [parent.as_str(), reply_id.as_str()]
            );
            assert_eq!(messages[0].reactions[0].author_ids, [self_id]);
        }
        other => panic!("{other:?}"),
    }
    // Exercise the actual channel action-to-request path against the same server.
    run_with_context(
        &socket,
        root_text.clone(),
        None,
        ChannelAction::React {
            id: parent.clone(),
            remove: true,
            json: true,
        },
    )
    .await
    .unwrap();
    run_with_context(
        &socket,
        root_text,
        None,
        ChannelAction::Read {
            since: None,
            before: None,
            limit: 1,
            search: Some("edited".into()),
            thread: Some(parent),
            json: true,
        },
    )
    .await
    .unwrap();
    let history = pu_core::channel::ChannelStore::new(&root)
        .read(&pu_core::channel::ReadOptions::default())
        .unwrap();
    assert_eq!(history.revision, 5);
    assert!(history.messages[0].reactions.is_empty());
    task.abort(); // only this test's temporary in-process server, never a daemon session
}

#[tokio::test]
async fn given_incompatible_daemon_should_not_post_channel_message() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("old.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut line = String::new();
        tokio::io::BufReader::new(read)
            .read_line(&mut line)
            .await
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<Request>(&line).unwrap(),
            Request::Health
        ));
        let response = Response::HealthReport {
            pid: 1,
            uptime_seconds: 0,
            protocol_version: 6,
            projects: vec![],
            agent_count: 1,
        };
        write
            .write_all((serde_json::to_string(&response).unwrap() + "\n").as_bytes())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let result = run_with_context(
        &socket,
        "/unused".into(),
        None,
        ChannelAction::Send {
            text: "must not post".into(),
            reply_to: None,
            commit: vec![],
            pr: vec![],
            json: false,
        },
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("protocol"));
    server.await.unwrap();
}
