//! Port of `Services/TranscriptParser.swift`: a Claude JSONL transcript as chat messages.
//!
//! Consecutive assistant records merge into one message; a user record holding only
//! `tool_result` blocks folds into the preceding assistant message and marks the matching
//! `tool_use` completed or failed. Assistant text is split into text and fenced code blocks
//! by [`split_content`], a port of `Models/ContentBlockSplitter.swift` (the front end gets
//! its own port in WIN-8.16 for streaming text).
//!
//! Deviation: block and message ids are deterministic (`msg-N`, `blk-N`, `cb-N`) instead of
//! random UUIDs; they only need to be unique within one parse.

use std::path::Path;

use serde_json::Value;

use super::model::{ChatMessage, ChatRole, ContentBlock, ToolUseStatus};
use super::stream_event::tool_input_json;
use super::util::{Object, now_ms, obj_field, parse_json_line, parse_timestamp, str_field};

/// Parses the transcript at `transcript_path`. An empty file is no messages.
pub fn parse(transcript_path: &Path) -> std::io::Result<Vec<ChatMessage>> {
    let bytes = std::fs::read(transcript_path)?;
    Ok(parse_text(&String::from_utf8_lossy(&bytes)))
}

/// Parses transcript text (one JSON record per line; corrupt lines are skipped).
pub fn parse_text(text: &str) -> Vec<ChatMessage> {
    let mut ids = IdSource::default();
    let mut messages: Vec<ChatMessage> = Vec::new();

    for line in text.split('\n').filter(|line| !line.is_empty()) {
        let Some(record) = parse_json_line(line) else {
            continue;
        };
        match str_field(&record, "type") {
            Some("user") => {
                if let Some(results) = extract_tool_result_only_blocks(&record, &mut ids) {
                    if let Some(last) = messages.last_mut()
                        && last.role == ChatRole::Assistant
                    {
                        fold_tool_results(last, results);
                    }
                } else if let Some(message) = parse_user_record(&record, &mut ids) {
                    messages.push(message);
                }
            }
            Some("assistant") => {
                let Some(blocks) = parse_assistant_blocks(&record, &mut ids) else {
                    continue;
                };
                match messages.last_mut() {
                    Some(last) if last.role == ChatRole::Assistant => {
                        last.content_blocks.extend(blocks);
                    }
                    _ => messages.push(ChatMessage {
                        id: ids.message(),
                        role: ChatRole::Assistant,
                        timestamp: timestamp(&record),
                        content_blocks: blocks,
                        is_streaming: false,
                    }),
                }
            }
            _ => {}
        }
    }
    messages
}

/// Appends tool results to an assistant message and settles the tool calls they answer.
fn fold_tool_results(message: &mut ChatMessage, results: Vec<ContentBlock>) {
    for result in &results {
        let ContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } = result
        else {
            continue;
        };
        for block in &mut message.content_blocks {
            if let ContentBlock::ToolUse { id, status, .. } = block
                && id == tool_use_id
            {
                *status = if *is_error {
                    ToolUseStatus::Failed
                } else {
                    ToolUseStatus::Completed
                };
            }
        }
    }
    message.content_blocks.extend(results);
}

#[derive(Default)]
struct IdSource {
    messages: usize,
    blocks: usize,
    /// Next `cb-N` id for split assistant text.
    split: usize,
}

impl IdSource {
    fn message(&mut self) -> String {
        self.messages += 1;
        format!("msg-{}", self.messages)
    }

    fn block(&mut self) -> String {
        self.blocks += 1;
        format!("blk-{}", self.blocks)
    }
}

fn timestamp(record: &Object) -> i64 {
    parse_timestamp(str_field(record, "timestamp")).unwrap_or_else(now_ms)
}

/// `message.content` when `message.role == role`.
fn content_for_role<'a>(record: &'a Object, role: &str) -> Option<&'a Value> {
    let message = obj_field(record, "message")?;
    (str_field(message, "role")? == role).then(|| message.get("content"))?
}

fn tool_result_block(block: &Object, ids: &mut IdSource) -> ContentBlock {
    ContentBlock::ToolResult {
        id: ids.block(),
        tool_use_id: str_field(block, "tool_use_id")
            .unwrap_or_default()
            .to_string(),
        output: str_field(block, "content").unwrap_or_default().to_string(),
        is_error: block
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// The blocks of a user record whose typed blocks are all `tool_result`, else `None`.
fn extract_tool_result_only_blocks(
    record: &Object,
    ids: &mut IdSource,
) -> Option<Vec<ContentBlock>> {
    let blocks = content_for_role(record, "user")?.as_array()?;
    let mut results = Vec::new();
    for block in blocks.iter().filter_map(Value::as_object) {
        match str_field(block, "type") {
            None => continue,
            Some("tool_result") => results.push(tool_result_block(block, ids)),
            Some(_) => return None,
        }
    }
    (!results.is_empty()).then_some(results)
}

fn parse_user_record(record: &Object, ids: &mut IdSource) -> Option<ChatMessage> {
    let content = content_for_role(record, "user")?;
    let content_blocks = match content {
        Value::String(text) => vec![ContentBlock::Text {
            id: ids.block(),
            text: text.clone(),
        }],
        Value::Array(blocks) => {
            let mut content_blocks = Vec::new();
            for block in blocks.iter().filter_map(Value::as_object) {
                match str_field(block, "type") {
                    Some("text") => {
                        if let Some(text) = str_field(block, "text") {
                            content_blocks.push(ContentBlock::Text {
                                id: ids.block(),
                                text: text.to_string(),
                            });
                        }
                    }
                    Some("tool_result") => content_blocks.push(tool_result_block(block, ids)),
                    _ => {}
                }
            }
            if content_blocks.is_empty() {
                return None;
            }
            content_blocks
        }
        _ => return None,
    };
    Some(ChatMessage {
        id: ids.message(),
        role: ChatRole::User,
        timestamp: timestamp(record),
        content_blocks,
        is_streaming: false,
    })
}

fn parse_assistant_blocks(record: &Object, ids: &mut IdSource) -> Option<Vec<ContentBlock>> {
    let content = content_for_role(record, "assistant")?.as_array()?;
    let mut blocks = Vec::new();
    for block in content.iter().filter_map(Value::as_object) {
        match str_field(block, "type") {
            Some("text") => {
                if let Some(text) = str_field(block, "text") {
                    blocks.extend(split_content(text, &mut ids.split));
                }
            }
            Some("tool_use") => blocks.push(ContentBlock::ToolUse {
                id: str_field(block, "id")
                    .map(str::to_string)
                    .unwrap_or_else(|| ids.block()),
                name: str_field(block, "name").unwrap_or("unknown").to_string(),
                input: tool_input_json(block.get("input")),
                status: ToolUseStatus::Completed,
            }),
            // Thinking blocks and the like are not shown.
            _ => {}
        }
    }
    (!blocks.is_empty()).then_some(blocks)
}

/// `ContentBlockSplitter.isClosingFence`.
fn is_closing_fence(line: &str) -> bool {
    line.trim_matches([' ', '\t'])
        .strip_prefix("```")
        .is_some_and(|rest| rest.chars().all(char::is_whitespace))
}

/// Port of `ContentBlockSplitter.split`: Markdown text as text and fenced code blocks, with
/// ids `cb-N` drawn from `counter`. An unclosed fence stays plain text.
pub fn split_content(text: &str, counter: &mut usize) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    if text.is_empty() {
        return blocks;
    }
    let mut current_text: Vec<&str> = Vec::new();
    let mut saved_text: Vec<&str> = Vec::new();
    let mut code_lines: Vec<&str> = Vec::new();
    let mut code_language: Option<String> = None;
    let mut opening_fence: Option<&str> = None;

    let next_id = |counter: &mut usize| {
        let id = format!("cb-{counter}");
        *counter += 1;
        id
    };
    let flush = |lines: &mut Vec<&str>, blocks: &mut Vec<ContentBlock>, counter: &mut usize| {
        let joined = lines.join("\n");
        let trimmed = joined.trim();
        if !trimmed.is_empty() {
            let id = format!("cb-{counter}");
            *counter += 1;
            blocks.push(ContentBlock::Text {
                id,
                text: trimmed.to_string(),
            });
        }
        lines.clear();
    };

    for line in text.split('\n') {
        match opening_fence {
            None => {
                if let Some(language) = line.strip_prefix("```") {
                    saved_text = std::mem::take(&mut current_text);
                    let language = language.trim_matches([' ', '\t']);
                    code_language = (!language.is_empty()).then(|| language.to_string());
                    opening_fence = Some(line);
                    code_lines.clear();
                } else {
                    current_text.push(line);
                }
            }
            Some(_) => {
                if is_closing_fence(line) {
                    current_text = std::mem::take(&mut saved_text);
                    flush(&mut current_text, &mut blocks, counter);
                    blocks.push(ContentBlock::CodeBlock {
                        id: next_id(counter),
                        language: code_language.take(),
                        code: code_lines.join("\n"),
                    });
                    code_lines.clear();
                    opening_fence = None;
                } else {
                    code_lines.push(line);
                }
            }
        }
    }

    if let Some(fence) = opening_fence {
        current_text = std::mem::take(&mut saved_text);
        current_text.push(fence);
        current_text.append(&mut code_lines);
    }
    flush(&mut current_text, &mut blocks, counter);
    blocks
}

/// Ports `TranscriptParserTests.swift`, one fn per Swift `@Test`, with the same
/// `writeTranscript` helper building transcripts in a temporary directory.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::test_support::TempDir;

    enum Line<'a> {
        User(&'a str),
        Assistant(&'a str),
        /// Pre-escaped text.
        AssistantRaw(&'a str),
        AssistantWithToolUse {
            tool_id: &'a str,
            tool_name: &'a str,
            tool_input: &'a str,
        },
        ToolResult {
            tool_use_id: &'a str,
            content: &'a str,
            is_error: bool,
        },
    }

    fn escaped(text: &str) -> String {
        let quoted = serde_json::to_string(text).unwrap();
        quoted[1..quoted.len() - 1].to_string()
    }

    fn user_line(content: &str, timestamp: &str) -> String {
        format!(
            r#"{{"type":"user","sessionId":"test-session","timestamp":"{timestamp}","message":{{"role":"user","content":"{}"}}}}"#,
            escaped(content)
        )
    }

    fn assistant_text_line(content_escaped: &str, timestamp: &str) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"test-session","timestamp":"{timestamp}","message":{{"role":"assistant","content":[{{"type":"text","text":"{content_escaped}"}}]}}}}"#
        )
    }

    const PROGRESS: &str = r#"{"type":"progress","cwd":"/tmp/test","sessionId":"test-session","gitBranch":"main","timestamp":"2026-03-01T09:00:00.000Z"}"#;
    const T0: &str = "2026-03-01T09:00:00.000Z";

    fn write_transcript(dir: &TempDir, lines: &[Line]) -> std::path::PathBuf {
        let mut payloads = vec![PROGRESS.to_string()];
        for (index, line) in lines.iter().enumerate() {
            let timestamp = format!("2026-03-01T09:{:02}:00.000Z", index + 1);
            payloads.push(match line {
                Line::User(content) => user_line(content, &timestamp),
                Line::Assistant(content) => assistant_text_line(&escaped(content), &timestamp),
                Line::AssistantRaw(content) => assistant_text_line(content, &timestamp),
                Line::AssistantWithToolUse {
                    tool_id,
                    tool_name,
                    tool_input,
                } => format!(
                    r#"{{"type":"assistant","sessionId":"test-session","timestamp":"{timestamp}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"{tool_id}","name":"{tool_name}","input":{tool_input}}}]}}}}"#
                ),
                Line::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => format!(
                    r#"{{"type":"user","sessionId":"test-session","timestamp":"{timestamp}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":"{}","is_error":{is_error}}}]}}}}"#,
                    escaped(content)
                ),
            });
        }
        dir.write("session.jsonl", &payloads.join("\n"))
    }

    fn roles(messages: &[ChatMessage]) -> Vec<ChatRole> {
        messages.iter().map(|m| m.role).collect()
    }

    #[test]
    fn given_transcript_with_user_and_assistant_should_parse_messages() {
        let dir = TempDir::new();
        let path = write_transcript(
            &dir,
            &[
                Line::User("Hello, how are you?"),
                Line::Assistant("I'm doing well! How can I help?"),
                Line::User("Tell me a joke"),
                Line::Assistant("Why did the programmer quit? Because they didn't get arrays."),
            ],
        );
        let messages = parse(&path).unwrap();
        use ChatRole::{Assistant, User};
        assert_eq!(roles(&messages), [User, Assistant, User, Assistant]);
        let Some(ContentBlock::Text { text, .. }) = messages[0].content_blocks.first() else {
            panic!("expected text block");
        };
        assert_eq!(text, "Hello, how are you?");
        assert_eq!(
            messages[0].timestamp,
            parse_timestamp(Some("2026-03-01T09:01:00.000Z")).unwrap()
        );
    }

    #[test]
    fn given_transcript_with_tool_use_should_create_tool_blocks() {
        let dir = TempDir::new();
        let path = write_transcript(
            &dir,
            &[
                Line::User("Read my config file"),
                Line::AssistantWithToolUse {
                    tool_id: "toolu_abc",
                    tool_name: "Read",
                    tool_input: r#"{"file_path":"/etc/hosts"}"#,
                },
            ],
        );
        let messages = parse(&path).unwrap();
        assert_eq!(messages.len(), 2);
        let Some(ContentBlock::ToolUse { name, input, .. }) = messages[1].content_blocks.first()
        else {
            panic!("expected tool_use block");
        };
        assert_eq!(name, "Read");
        assert_eq!(input, r#"{"file_path":"/etc/hosts"}"#);
    }

    #[test]
    fn given_transcript_with_tool_result_should_link_to_tool_use() {
        let dir = TempDir::new();
        let path = write_transcript(
            &dir,
            &[
                Line::User("Read the file"),
                Line::AssistantWithToolUse {
                    tool_id: "toolu_xyz",
                    tool_name: "Read",
                    tool_input: "{}",
                },
                Line::ToolResult {
                    tool_use_id: "toolu_xyz",
                    content: "file contents here",
                    is_error: false,
                },
                Line::Assistant("Here is the file content."),
            ],
        );
        let messages = parse(&path).unwrap();
        // Tool result folded into assistant — no separate user message.
        assert_eq!(roles(&messages), [ChatRole::User, ChatRole::Assistant]);
        let blocks = &messages[1].content_blocks;
        assert!(blocks.iter().any(|b| matches!(
            b,
            ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "toolu_xyz"
        )));
        assert!(blocks.iter().any(|b| matches!(
            b,
            ContentBlock::ToolUse { id, status: ToolUseStatus::Completed, .. } if id == "toolu_xyz"
        )));
    }

    #[test]
    fn given_tool_result_only_user_record_should_fold_into_assistant() {
        let dir = TempDir::new();
        let path = write_transcript(
            &dir,
            &[
                Line::User("Do something"),
                Line::AssistantWithToolUse {
                    tool_id: "toolu_001",
                    tool_name: "Bash",
                    tool_input: "{}",
                },
                Line::ToolResult {
                    tool_use_id: "toolu_001",
                    content: "output",
                    is_error: false,
                },
            ],
        );
        let messages = parse(&path).unwrap();
        assert_eq!(roles(&messages), [ChatRole::User, ChatRole::Assistant]);
        let results = messages[1]
            .content_blocks
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
            .count();
        assert_eq!(results, 1);
    }

    #[test]
    fn given_mixed_user_content_should_keep_user_message() {
        let dir = TempDir::new();
        let payloads = [
            PROGRESS.to_string(),
            user_line("Setup", T0),
            r#"{"type":"assistant","sessionId":"test-session","timestamp":"2026-03-01T09:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_mix","name":"Read","input":{}}]}}"#.to_string(),
            r#"{"type":"user","sessionId":"test-session","timestamp":"2026-03-01T09:02:00.000Z","message":{"role":"user","content":[{"type":"text","text":"Here is context"},{"type":"tool_result","tool_use_id":"toolu_mix","content":"result data","is_error":false}]}}"#.to_string(),
        ];
        let path = dir.write("session.jsonl", &payloads.join("\n"));
        let messages = parse(&path).unwrap();
        // Mixed content keeps the user message (not folded).
        let users = messages.iter().filter(|m| m.role == ChatRole::User).count();
        assert_eq!(users, 2);
    }

    #[test]
    fn given_error_tool_result_should_set_failed_status() {
        let dir = TempDir::new();
        let path = write_transcript(
            &dir,
            &[
                Line::User("Run this"),
                Line::AssistantWithToolUse {
                    tool_id: "toolu_err",
                    tool_name: "Bash",
                    tool_input: "{}",
                },
                Line::ToolResult {
                    tool_use_id: "toolu_err",
                    content: "command failed",
                    is_error: true,
                },
            ],
        );
        let messages = parse(&path).unwrap();
        assert_eq!(messages.len(), 2);
        let blocks = &messages[1].content_blocks;
        assert!(blocks.iter().any(|b| matches!(
            b,
            ContentBlock::ToolUse { id, status: ToolUseStatus::Failed, .. } if id == "toolu_err"
        )));
        assert!(blocks.iter().any(|b| matches!(
            b,
            ContentBlock::ToolResult { tool_use_id, is_error: true, .. } if tool_use_id == "toolu_err"
        )));
    }

    #[test]
    fn given_assistant_with_code_blocks_should_split_content() {
        let dir = TempDir::new();
        let code_response = r"Here is the code:\n\n```swift\nlet x = 42\n```\n\nThat should work.";
        let path = write_transcript(
            &dir,
            &[
                Line::User("Write some code"),
                Line::AssistantRaw(code_response),
            ],
        );
        let messages = parse(&path).unwrap();
        assert_eq!(messages.len(), 2);
        // Text + code + text.
        assert_eq!(
            messages[1].content_blocks,
            [
                ContentBlock::Text {
                    id: "cb-0".into(),
                    text: "Here is the code:".into()
                },
                ContentBlock::CodeBlock {
                    id: "cb-1".into(),
                    language: Some("swift".into()),
                    code: "let x = 42".into()
                },
                ContentBlock::Text {
                    id: "cb-2".into(),
                    text: "That should work.".into()
                },
            ]
        );
    }

    #[test]
    fn given_empty_transcript_should_return_empty_array() {
        let dir = TempDir::new();
        let path = dir.write("empty.jsonl", "");
        assert!(parse(&path).unwrap().is_empty());
    }

    #[test]
    fn given_corrupt_line_should_skip_and_continue() {
        let dir = TempDir::new();
        let lines = [
            user_line("Before corrupt", T0),
            "{{not valid json}}".to_string(),
            assistant_text_line(&escaped("After corrupt"), T0),
        ];
        let path = dir.write("session.jsonl", &lines.join("\n"));
        let messages = parse(&path).unwrap();
        assert_eq!(roles(&messages), [ChatRole::User, ChatRole::Assistant]);
    }

    // Beyond the Swift cases.

    #[test]
    fn crlf_transcripts_parse_like_lf() {
        let text = [user_line("hi", T0), assistant_text_line("yo", T0)].join("\r\n");
        assert_eq!(
            roles(&parse_text(&text)),
            [ChatRole::User, ChatRole::Assistant]
        );
    }

    #[test]
    fn unclosed_fence_stays_text() {
        let mut counter = 0;
        let blocks = split_content("intro\n```rust\nlet a = 1;", &mut counter);
        assert_eq!(
            blocks,
            [ContentBlock::Text {
                id: "cb-0".into(),
                text: "intro\n```rust\nlet a = 1;".into()
            }]
        );
        assert_eq!(counter, 1);
    }
}
