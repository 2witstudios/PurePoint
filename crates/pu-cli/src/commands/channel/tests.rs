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
    let metadata = temp.path().join("metadata");
    git(
        &root,
        &["init", "--separate-git-dir", metadata.to_str().unwrap()],
    );
    let expected = std::fs::canonicalize(&root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(resolve_root(None, None, &root).unwrap(), expected);
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
