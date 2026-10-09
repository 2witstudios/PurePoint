use super::*;
#[test]
fn given_reopened_store_should_preserve_history() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".pu")).unwrap();
    let author = ChannelAuthor {
        id: "human:1".into(),
        name: "Developer".into(),
        kind: "human".into(),
        agent_type: None,
        worktree_id: None,
        branch: None,
    };
    let message = ChannelStore::new(dir.path())
        .send(author.clone(), "hello".into(), None, vec![])
        .unwrap()
        .0;
    let history = ChannelStore::new(dir.path())
        .read(&ReadOptions::default())
        .unwrap();
    assert_eq!(history.messages[0], message);
    assert_eq!(history.revision, 1);
}

fn setup() -> (tempfile::TempDir, ChannelAuthor) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".pu")).unwrap();
    (
        dir,
        ChannelAuthor {
            id: "human:1".into(),
            name: "Developer".into(),
            kind: "human".into(),
            agent_type: None,
            worktree_id: None,
            branch: None,
        },
    )
}
#[test]
fn given_concurrent_writers_should_keep_all_ordered_messages() {
    let (dir, author) = setup();
    std::thread::scope(|scope| {
        for writer in 0..8 {
            let author = author.clone();
            let root = dir.path();
            scope.spawn(move || {
                for i in 0..20 {
                    ChannelStore::new(root)
                        .send(author.clone(), format!("{writer}/{i}"), None, vec![])
                        .unwrap();
                }
            });
        }
    });
    let history = ChannelStore::new(dir.path())
        .read(&ReadOptions {
            limit: 200,
            ..ReadOptions::default()
        })
        .unwrap();
    assert_eq!(history.messages.len(), 160);
    assert_eq!(history.revision, 160);
    assert_eq!(history.latest_sequence, 160);
    assert_eq!(
        history
            .messages
            .iter()
            .map(|m| m.id.clone())
            .collect::<HashSet<_>>()
            .len(),
        160
    );
    assert!(dir.path().join(".pu/channel.json.lock").exists());
    assert_eq!(
        std::fs::read_dir(dir.path().join(".pu")).unwrap().count(),
        2
    );
}
#[test]
fn given_corrupt_or_future_store_should_fail_without_overwriting() {
    let (dir, author) = setup();
    let path = dir.path().join(".pu/channel.json");
    for bytes in [
        "broken",
        r#"{"version":2,"revision":0,"latest_sequence":0,"messages":[]}"#,
        r#"{"version":1,"revision":0,"latest_sequence":9,"messages":[]}"#,
        r#"{"version":1}"#,
    ] {
        std::fs::write(&path, bytes).unwrap();
        let store = ChannelStore::new(dir.path());
        assert!(store.read(&ReadOptions::default()).is_err());
        assert!(
            store
                .send(author.clone(), "hello".into(), None, vec![])
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }
}
#[test]
fn given_replies_should_keep_primary_cursor_and_full_reply_count() {
    let (dir, author) = setup();
    let store = ChannelStore::new(dir.path());
    let parent = store
        .send(author.clone(), "parent".into(), None, vec![])
        .unwrap()
        .0;
    let other = store
        .send(author.clone(), "other".into(), None, vec![])
        .unwrap()
        .0;
    for i in 0..3 {
        store
            .send(
                author.clone(),
                format!("reply {i}"),
                Some(parent.id.clone()),
                vec![],
            )
            .unwrap();
    }
    let latest = store
        .read(&ReadOptions {
            limit: 2,
            ..ReadOptions::default()
        })
        .unwrap();
    assert_eq!(
        latest
            .messages
            .iter()
            .map(|m| m.sequence)
            .collect::<Vec<_>>(),
        [1, 4, 5]
    );
    assert_eq!(latest.oldest_sequence, Some(4));
    assert!(latest.has_more);
    assert_eq!(latest.reply_counts[&parent.id], 3);
    let older = store
        .read(&ReadOptions {
            before: latest.oldest_sequence,
            limit: 2,
            ..ReadOptions::default()
        })
        .unwrap();
    assert_eq!(
        older
            .messages
            .iter()
            .map(|m| m.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(older.oldest_sequence, Some(2));
    let newer = store
        .read(&ReadOptions {
            after: Some(1),
            limit: 2,
            ..ReadOptions::default()
        })
        .unwrap();
    assert_eq!(newer.oldest_sequence, Some(2));
    assert_eq!(
        newer
            .messages
            .iter()
            .map(|m| m.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let thread = store
        .read(&ReadOptions {
            parent_id: Some(parent.id.clone()),
            limit: 2,
            ..ReadOptions::default()
        })
        .unwrap();
    assert_eq!(thread.oldest_sequence, Some(4));
    assert_eq!(thread.messages.len(), 3);
    assert!(
        thread
            .messages
            .iter()
            .all(|m| m.id == parent.id || m.parent_id.as_ref() == Some(&parent.id))
    );
    let empty = store
        .read(&ReadOptions {
            parent_id: Some(other.id),
            ..ReadOptions::default()
        })
        .unwrap();
    assert!(empty.messages.is_empty());
    assert_eq!(empty.oldest_sequence, None);
}
#[test]
fn given_search_should_match_identity_context_references_and_inject_parent() {
    let (dir, mut author) = setup();
    author.name = "Builder".into();
    author.branch = Some("pu/Feature".into());
    author.worktree_id = Some("wt-Alpha".into());
    let store = ChannelStore::new(dir.path());
    let parent = store
        .send(author.clone(), "parent".into(), None, vec![])
        .unwrap()
        .0;
    let reply = store
        .send(
            author.clone(),
            "Résumé READY".into(),
            Some(parent.id.clone()),
            vec![ChannelReference {
                kind: "commit".into(),
                value: "abcdef012345".into(),
                label: Some("Fix Label".into()),
            }],
        )
        .unwrap()
        .0;
    for query in [
        "builder",
        "FEATURE",
        "alpha",
        "résumé",
        "ABCDEF",
        "fix label",
    ] {
        let history = store
            .read(&ReadOptions {
                query: Some(query.into()),
                limit: 1,
                ..ReadOptions::default()
            })
            .unwrap();
        assert_eq!(
            history.messages.iter().map(|m| &m.id).collect::<Vec<_>>(),
            [&parent.id, &reply.id]
        );
        assert_eq!(history.oldest_sequence, Some(2));
    }
    assert!(
        store
            .read(&ReadOptions {
                query: Some("absent".into()),
                ..ReadOptions::default()
            })
            .unwrap()
            .messages
            .is_empty()
    );
}
#[test]
fn given_edits_and_reactions_should_refresh_revision_preserve_sequence_and_enforce_owner() {
    let (dir, author) = setup();
    let store = ChannelStore::new(dir.path());
    let sent = store
        .send(author.clone(), "original".into(), None, vec![])
        .unwrap()
        .0;
    assert!(matches!(
        store.edit("human:2", &sent.id, "bad".into()),
        Err(ChannelError::Ownership)
    ));
    let (edited, revision) = store.edit(&author.id, &sent.id, "changed".into()).unwrap();
    assert_eq!(revision, 2);
    assert_eq!(edited.sequence, 1);
    assert_eq!(edited.created_at, sent.created_at);
    assert!(edited.edited_at.is_some());
    let (_, revision) = store.react(&author.id, &sent.id, "👍", true).unwrap();
    assert_eq!(revision, 3);
    assert_eq!(store.react(&author.id, &sent.id, "👍", true).unwrap().1, 3);
    assert_eq!(store.react("human:2", &sent.id, "👍", false).unwrap().1, 3);
    assert_eq!(store.react("human:2", &sent.id, "👍", true).unwrap().1, 4);
    let (removed, revision) = store.react(&author.id, &sent.id, "👍", false).unwrap();
    assert_eq!(revision, 5);
    assert_eq!(removed.reactions[0].author_ids, ["human:2"]);
    assert_eq!(
        store
            .react("human:2", &sent.id, "👍", false)
            .unwrap()
            .0
            .reactions
            .len(),
        0
    );
    let history = store.read(&ReadOptions::default()).unwrap();
    assert!(
        store
            .read(&ReadOptions {
                known_revision: Some(history.revision),
                ..ReadOptions::default()
            })
            .unwrap()
            .unchanged
    );
    assert!(
        !store
            .read(&ReadOptions {
                known_revision: Some(1),
                ..ReadOptions::default()
            })
            .unwrap()
            .unchanged
    );
    for options in [
        ReadOptions {
            after: Some(0),
            ..ReadOptions::default()
        },
        ReadOptions {
            before: Some(2),
            ..ReadOptions::default()
        },
        ReadOptions {
            query: Some("".into()),
            ..ReadOptions::default()
        },
    ] {
        assert!(
            !store
                .read(&ReadOptions {
                    known_revision: Some(history.revision),
                    ..options
                })
                .unwrap()
                .unchanged
        );
    }
}
#[test]
fn given_invalid_inputs_should_reject_without_mutation() {
    let (dir, author) = setup();
    let store = ChannelStore::new(dir.path());
    for text in ["".to_string(), " \n\t".into(), "é".repeat(8001)] {
        assert!(store.send(author.clone(), text, None, vec![]).is_err());
    }
    assert!(
        store
            .send(
                author.clone(),
                "hello".into(),
                Some("missing".into()),
                vec![]
            )
            .is_err()
    );
    for reference in [
        ChannelReference {
            kind: "other".into(),
            value: "abc".into(),
            label: None,
        },
        ChannelReference {
            kind: "commit".into(),
            value: "xyzxyzx".into(),
            label: None,
        },
        ChannelReference {
            kind: "pr".into(),
            value: "0".into(),
            label: None,
        },
        ChannelReference {
            kind: "pr".into(),
            value: "1".into(),
            label: Some("x".repeat(257)),
        },
    ] {
        assert!(
            store
                .send(author.clone(), "hello".into(), None, vec![reference])
                .is_err()
        );
    }
    assert!(
        store
            .send(
                author.clone(),
                "hello".into(),
                None,
                vec![
                    ChannelReference {
                        kind: "pr".into(),
                        value: "1".into(),
                        label: None
                    };
                    9
                ]
            )
            .is_err()
    );
    let parent = store
        .send(author.clone(), "hello".into(), None, vec![])
        .unwrap()
        .0;
    let reply = store
        .send(
            author.clone(),
            "reply".into(),
            Some(parent.id.clone()),
            vec![],
        )
        .unwrap()
        .0;
    assert!(
        store
            .send(
                author.clone(),
                "nested".into(),
                Some(reply.id.clone()),
                vec![]
            )
            .is_err()
    );
    assert!(
        store
            .read(&ReadOptions {
                parent_id: Some(reply.id),
                ..ReadOptions::default()
            })
            .is_err()
    );
    for options in [
        ReadOptions {
            after: Some(1),
            before: Some(2),
            ..ReadOptions::default()
        },
        ReadOptions {
            limit: 0,
            ..ReadOptions::default()
        },
        ReadOptions {
            limit: 201,
            ..ReadOptions::default()
        },
        ReadOptions {
            query: Some("é".repeat(257)),
            ..ReadOptions::default()
        },
    ] {
        assert!(store.read(&options).is_err());
    }
    assert!(store.edit(&author.id, "missing", "hello".into()).is_err());
    assert!(store.react(&author.id, &parent.id, "", true).is_err());
    assert_eq!(store.read(&ReadOptions::default()).unwrap().revision, 2);
}

#[test]
#[ignore = "invoked as a subprocess by the cross-process locking proof"]
fn channel_process_writer() {
    let root = std::env::var("PU_CHANNEL_TEST_ROOT").expect("isolated test root");
    let writer = std::env::var("PU_CHANNEL_TEST_WRITER").expect("writer ID");
    let author = ChannelAuthor {
        id: format!("human:{writer}"),
        name: format!("Writer {writer}"),
        kind: "human".into(),
        agent_type: None,
        worktree_id: None,
        branch: None,
    };
    let store = ChannelStore::new(Path::new(&root));
    for i in 0..20 {
        store
            .send(author.clone(), format!("{writer}/{i}"), None, vec![])
            .unwrap();
    }
}
#[test]
fn given_concurrent_processes_should_serialize_durable_updates() {
    let (dir, _) = setup();
    let executable = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..4)
        .map(|writer| {
            std::process::Command::new(&executable)
                .args([
                    "--exact",
                    "channel::tests::channel_process_writer",
                    "--ignored",
                ])
                .env("PU_CHANNEL_TEST_ROOT", dir.path())
                .env("PU_CHANNEL_TEST_WRITER", writer.to_string())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let history = ChannelStore::new(dir.path())
        .read(&ReadOptions::default())
        .unwrap();
    assert_eq!(history.messages.len(), 80);
    assert_eq!(history.latest_sequence, 80);
    assert_eq!(history.revision, 80);
    assert_eq!(
        history
            .messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<HashSet<_>>()
            .len(),
        80
    );
}
