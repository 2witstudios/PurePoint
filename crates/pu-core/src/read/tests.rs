use super::*;

#[test]
fn given_claude_transcript_should_read_messages_and_tools_without_terminal_noise() {
    // given
    let input = concat!(
        "{\"type\":\"user\",\"uuid\":\"u1\",\"message\":{\"role\":\"user\",\"content\":\"Fix it\"}}\n",
        "{\"type\":\"assistant\",\"uuid\":\"a1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Working\"},{\"type\":\"tool_use\",\"id\":\"call1\",\"name\":\"Bash\",\"input\":{\"command\":\"pwd\"}}]}}\n",
        "{\"type\":\"user\",\"uuid\":\"u2\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"call1\",\"content\":\"\\u001b[31m/path\\u001b[0m\"}]}}\n"
    );
    // when
    let transcript = parse_jsonl(Runtime::Claude, input.as_bytes()).unwrap();
    // then
    assert_eq!(transcript.records.len(), 4);
    assert_eq!(transcript.records[0].kind, RecordKind::User);
    assert_eq!(transcript.records[2].tool_name.as_deref(), Some("Bash"));
    assert_eq!(transcript.records[3].text, "/path");
    assert_eq!(transcript.records[3].tool_call_id.as_deref(), Some("call1"));
}

#[test]
fn given_pi_branches_should_read_only_the_selected_leaf_ancestry() {
    // given
    let input = concat!(
        "{\"type\":\"session\",\"version\":3,\"id\":\"s1\"}\n",
        "{\"type\":\"message\",\"id\":\"u1\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"Question\"}}\n",
        "{\"type\":\"message\",\"id\":\"a1\",\"parentId\":\"u1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Abandoned\"}]}}\n",
        "{\"type\":\"message\",\"id\":\"a2\",\"parentId\":\"u1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Selected\"}]}}\n"
    );
    // when
    let transcript = parse_jsonl(Runtime::Pi, input.as_bytes()).unwrap();
    // then
    assert_eq!(
        transcript
            .records
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Question", "Selected"]
    );
}

#[test]
fn given_cursor_should_read_each_block_once_and_reject_updated_or_wrong_source() {
    // given
    let mut transcript = Transcript {
        records: (0..4)
            .map(|n| record(n.to_string(), RecordKind::Assistant, &format!("Text {n}")))
            .collect(),
        pending_tail: false,
    };
    // when
    let first = paginate("agent:session", &transcript, None, 2).unwrap();
    transcript
        .records
        .push(record("4".into(), RecordKind::Assistant, "Text 4"));
    let next = paginate("agent:session", &transcript, Some(&first.next_cursor), 1).unwrap();
    // then
    assert_eq!(
        first
            .records
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "3"]
    );
    assert_eq!(next.records[0].id, "4");
    assert_eq!(
        paginate("other:session", &transcript, Some(&first.next_cursor), 1).unwrap_err(),
        ReadError::InvalidCursor
    );
    transcript.records[0].text = "updated tool output".into();
    assert_eq!(
        paginate("agent:session", &transcript, Some(&first.next_cursor), 1).unwrap_err(),
        ReadError::StaleCursor
    );
}

#[test]
fn given_live_jsonl_writes_should_wait_for_partial_records_and_reject_complete_corruption() {
    // given
    let first = b"{\"type\":\"user\",\"uuid\":\"u\",\"message\":{\"content\":\"hello\"}}\n";
    let partial = [
        first.as_slice(),
        b"{\"type\":\"assistant\",\"text\":\"\xf0\x9f",
    ]
    .concat();
    // when
    let result = parse_jsonl(Runtime::Claude, &partial).unwrap();
    // then
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.pending_tail, true);
    assert!(matches!(
        parse_jsonl(Runtime::Claude, b"{broken}\n"),
        Err(ReadError::InvalidRecord(_))
    ));
}

#[test]
fn given_opencode_current_and_legacy_messages_should_read_text_and_tool_results() {
    // given
    let current = serde_json::json!({"messages":[
        {"id":"m1","type":"user","text":"Fix it"},
        {"id":"m2","type":"assistant","content":[
            {"type":"text","text":"Working"},
            {"type":"tool","id":"call1","name":"bash","state":{"status":"completed","input":{"command":"pwd"},"output":"/path"}}
        ]}
    ]});
    let legacy = serde_json::json!({"messages":[
        {"info":{"id":"m1","role":"user"},"parts":[{"id":"p1","type":"text","text":"Fix it"}]},
        {"info":{"id":"m2","role":"assistant"},"parts":[{"id":"p2","type":"tool","callID":"call1","tool":"bash","state":{"status":"completed","input":{"command":"pwd"},"output":"/path"}}]}
    ]});
    // when
    let current = parse_opencode(&current).unwrap();
    let legacy = parse_opencode(&legacy).unwrap();
    // then
    assert_eq!(current.records.len(), 4);
    assert_eq!(current.records[3].text, "/path");
    assert_eq!(legacy.records.len(), 3);
    assert_eq!(legacy.records[2].tool_call_id.as_deref(), Some("call1"));
}

#[test]
fn given_codex_rollout_should_read_native_items_without_duplicate_event_messages() {
    // given
    let input = concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread1\",\"source\":\"cli\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Fix it\"}]}}\n",
        "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"Fix it\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"exec\",\"arguments\":\"{\\\"cmd\\\":\\\"pwd\\\"}\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call1\",\"output\":\"/path\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Done\"}]}}\n"
    );
    // when
    let transcript = parse_jsonl(Runtime::Codex, input.as_bytes()).unwrap();
    // then
    assert_eq!(transcript.records.len(), 4);
    assert_eq!(transcript.records[1].tool_name.as_deref(), Some("exec"));
    assert_eq!(transcript.records[2].text, "/path");
    assert_eq!(transcript.records[3].text, "Done");
}

#[test]
fn given_claude_branches_should_exclude_sidechains_and_abandoned_messages() {
    // given
    let lines = [
        serde_json::json!({"type":"user","uuid":"u","parentUuid":null,"message":{"content":"Question"}}),
        serde_json::json!({"type":"assistant","uuid":"old","parentUuid":"u","message":{"content":"Abandoned"}}),
        serde_json::json!({"type":"assistant","uuid":"new","parentUuid":"u","message":{"content":"Selected"}}),
        serde_json::json!({"type":"assistant","uuid":"side","parentUuid":"new","isSidechain":true,"message":{"content":"Other agent"}}),
    ];
    let input = lines.iter().map(|v| format!("{v}\n")).collect::<String>();
    // when
    let result = parse_jsonl(Runtime::Claude, input.as_bytes()).unwrap();
    // then
    assert_eq!(
        result
            .records
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["Question", "Selected"]
    );
}

#[test]
fn given_attachments_or_new_schema_should_report_content_instead_of_silently_dropping_it() {
    // given
    let input = b"{\"type\":\"user\",\"uuid\":\"u\",\"message\":{\"content\":[{\"type\":\"image\",\"source\":{\"type\":\"base64\",\"data\":\"PRIVATE\"}}]}}\n";
    // when
    let result = parse_jsonl(Runtime::Claude, input).unwrap();
    // then
    assert_eq!(result.records[0].text, "Attachment: image");
    assert_eq!(result.records[0].kind, RecordKind::Notice);
    assert!(matches!(
        parse_jsonl(
            Runtime::Claude,
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"future_block\"}]}}\n"
        ),
        Err(ReadError::UnsupportedSchema(_))
    ));
}

#[test]
fn given_transcript_c1_controls_should_remove_payloads_and_preserve_unicode() {
    // given / when
    let cleaned = clean_text("Hi\u{9b}31m red\u{9b}0m\u{9d}title secret\u{9c} 😀\n");
    // then
    assert_eq!(cleaned, "Hi red 😀\n");
}

#[test]
fn given_codex_agent_messages_and_images_should_retain_visible_content() {
    // given
    let input = concat!(
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"agent_message\",\"author\":\"worker\",\"content\":\"Finding\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_image\",\"image_url\":\"PRIVATE\"}]}}\n"
    );
    // when
    let result = parse_jsonl(Runtime::Codex, input.as_bytes()).unwrap();
    // then
    assert_eq!(
        result
            .records
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["Finding", "Attachment: image"]
    );
    assert!(matches!(
        parse_jsonl(
            Runtime::Codex,
            b"{\"type\":\"response_item\",\"payload\":{\"type\":\"unknown_future\"}}\n"
        ),
        Err(ReadError::UnsupportedSchema(_))
    ));
}
