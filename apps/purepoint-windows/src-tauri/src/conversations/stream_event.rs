//! Port of `Models/StreamEvent.swift`: one line of `claude --output-format stream-json`
//! output, parsed into the events the chat stream accumulator consumes.

use serde::Serialize;
use serde_json::Value;

use super::util::{Object, obj_field, parse_json_line, str_field};

/// `StreamContentBlock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
}

/// `StreamEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StreamEvent {
    Assistant {
        content: Vec<StreamContentBlock>,
    },
    ContentBlockStart {
        index: i64,
        id: String,
        name: String,
    },
    ContentBlockDelta {
        index: i64,
        delta: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Result {
        session_id: String,
        duration_ms: Option<i64>,
    },
    Error {
        message: String,
    },
    Unknown,
}

/// A tool call's `input` as JSON text: objects and arrays serialise, anything else is `{}`
/// (`JSONSerialization.data(withJSONObject:)` rejects top-level scalars and null).
pub fn tool_input_json(input: Option<&Value>) -> String {
    match input {
        Some(value @ (Value::Object(_) | Value::Array(_))) => {
            serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
        }
        _ => "{}".to_string(),
    }
}

impl StreamEvent {
    /// Parses one stream-json line; `None` when it is not a JSON object with a `type`.
    pub fn parse(json_line: &str) -> Option<Self> {
        let dict = parse_json_line(json_line)?;
        let event = match str_field(&dict, "type")? {
            "assistant" => parse_assistant(&dict),
            "content_block_delta" => parse_content_block_delta(&dict),
            "stream_event" => parse_stream_event(&dict),
            "tool_result" => parse_tool_result(&dict),
            "result" => parse_result(&dict),
            _ => Self::Unknown,
        };
        Some(event)
    }
}

fn parse_assistant(dict: &Object) -> StreamEvent {
    let Some(content) = obj_field(dict, "message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return StreamEvent::Assistant { content: vec![] };
    };
    let blocks = content
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|block| match str_field(block, "type")? {
            "text" => Some(StreamContentBlock::Text {
                text: str_field(block, "text")?.to_string(),
            }),
            "tool_use" => Some(StreamContentBlock::ToolUse {
                id: str_field(block, "id")?.to_string(),
                name: str_field(block, "name")?.to_string(),
                input: tool_input_json(block.get("input")),
            }),
            _ => None,
        })
        .collect();
    StreamEvent::Assistant { content: blocks }
}

fn parse_stream_event(dict: &Object) -> StreamEvent {
    let Some(event) = obj_field(dict, "event") else {
        return StreamEvent::Unknown;
    };
    match str_field(event, "type") {
        Some("content_block_delta") => parse_content_block_delta(event),
        Some("content_block_start") => {
            let Some(block) = obj_field(event, "content_block") else {
                return StreamEvent::Unknown;
            };
            let (Some("tool_use"), Some(id), Some(name)) = (
                str_field(block, "type"),
                str_field(block, "id"),
                str_field(block, "name"),
            ) else {
                return StreamEvent::Unknown;
            };
            StreamEvent::ContentBlockStart {
                index: event.get("index").and_then(Value::as_i64).unwrap_or(0),
                id: id.to_string(),
                name: name.to_string(),
            }
        }
        _ => StreamEvent::Unknown,
    }
}

fn parse_content_block_delta(dict: &Object) -> StreamEvent {
    let index = dict.get("index").and_then(Value::as_i64).unwrap_or(0);
    match obj_field(dict, "delta").and_then(|delta| str_field(delta, "text")) {
        Some(text) => StreamEvent::ContentBlockDelta {
            index,
            delta: text.to_string(),
        },
        None => StreamEvent::Unknown,
    }
}

fn parse_tool_result(dict: &Object) -> StreamEvent {
    StreamEvent::ToolResult {
        tool_use_id: str_field(dict, "tool_use_id")
            .unwrap_or_default()
            .to_string(),
        content: str_field(dict, "content").unwrap_or_default().to_string(),
        is_error: dict
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_result(dict: &Object) -> StreamEvent {
    StreamEvent::Result {
        session_id: str_field(dict, "session_id")
            .unwrap_or_default()
            .to_string(),
        duration_ms: dict.get("duration_ms").and_then(Value::as_i64),
    }
}

/// Ports `StreamEventTests.swift`, one fn per Swift `@Test`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_assistant_text_event_should_parse_message_content() {
        let json = r#"{"type":"assistant","message":{"id":"msg_01","type":"message","role":"assistant","content":[{"type":"text","text":"Hello, world!"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn"}}"#;
        let Some(StreamEvent::Assistant { content }) = StreamEvent::parse(json) else {
            panic!("expected .assistant");
        };
        assert_eq!(
            content,
            [StreamContentBlock::Text {
                text: "Hello, world!".into()
            }]
        );
    }

    #[test]
    fn given_tool_use_event_should_parse_name_and_input() {
        let json = r#"{"type":"assistant","message":{"id":"msg_02","type":"message","role":"assistant","content":[{"type":"text","text":"Let me read that."},{"type":"tool_use","id":"toolu_abc123","name":"Read","input":{"file_path":"/etc/hosts"}}],"model":"claude-sonnet-4-20250514","stop_reason":"tool_use"}}"#;
        let Some(StreamEvent::Assistant { content }) = StreamEvent::parse(json) else {
            panic!("expected .assistant");
        };
        assert_eq!(content.len(), 2);
        let StreamContentBlock::ToolUse { id, name, input } = &content[1] else {
            panic!("expected .toolUse block");
        };
        assert_eq!(id, "toolu_abc123");
        assert_eq!(name, "Read");
        assert_eq!(input, r#"{"file_path":"/etc/hosts"}"#);
    }

    #[test]
    fn given_tool_result_event_should_parse_content_and_error_flag() {
        let json = r#"{"type":"tool_result","tool_use_id":"toolu_abc123","content":"127.0.0.1 localhost","is_error":false}"#;
        assert_eq!(
            StreamEvent::parse(json),
            Some(StreamEvent::ToolResult {
                tool_use_id: "toolu_abc123".into(),
                content: "127.0.0.1 localhost".into(),
                is_error: false,
            })
        );
    }

    #[test]
    fn given_result_event_should_parse_session_id_and_duration() {
        let json = r#"{"type":"result","subtype":"success","session_id":"sess-abc-123","duration_ms":4567,"is_error":false,"num_turns":3}"#;
        assert_eq!(
            StreamEvent::parse(json),
            Some(StreamEvent::Result {
                session_id: "sess-abc-123".into(),
                duration_ms: Some(4567),
            })
        );
    }

    #[test]
    fn given_unknown_event_type_should_return_unknown() {
        let json = r#"{"type":"system","message":"some internal event"}"#;
        assert_eq!(StreamEvent::parse(json), Some(StreamEvent::Unknown));
    }

    #[test]
    fn given_malformed_json_should_return_nil() {
        assert_eq!(StreamEvent::parse("not valid json {{{"), None);
    }

    // MARK: - stream_event wrapping tests

    #[test]
    fn given_stream_event_with_content_block_delta_should_parse_text_delta() {
        let json = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}}"#;
        assert_eq!(
            StreamEvent::parse(json),
            Some(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: "Hello".into(),
            })
        );
    }

    #[test]
    fn given_stream_event_with_tool_use_content_block_start_should_parse() {
        let json = r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_xyz789","name":"Write","input":{}}}}"#;
        assert_eq!(
            StreamEvent::parse(json),
            Some(StreamEvent::ContentBlockStart {
                index: 1,
                id: "toolu_xyz789".into(),
                name: "Write".into(),
            })
        );
    }

    #[test]
    fn given_stream_event_with_text_content_block_start_should_return_unknown() {
        let json = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#;
        assert_eq!(StreamEvent::parse(json), Some(StreamEvent::Unknown));
    }

    #[test]
    fn given_stream_event_with_unknown_subtype_should_return_unknown() {
        let json =
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_01"}}}"#;
        assert_eq!(StreamEvent::parse(json), Some(StreamEvent::Unknown));
    }

    #[test]
    fn tool_input_json_rejects_scalars() {
        assert_eq!(tool_input_json(None), "{}");
        assert_eq!(tool_input_json(Some(&Value::Null)), "{}");
        assert_eq!(tool_input_json(Some(&Value::from("x"))), "{}");
        assert_eq!(tool_input_json(Some(&serde_json::json!([1]))), "[1]");
    }
}
