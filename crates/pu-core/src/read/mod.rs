//! Pure projections of native agent transcripts.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Claude,
    Codex,
    Opencode,
    Pi,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    User,
    Assistant,
    ToolCall,
    ToolResult,
    Notice,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadRecord {
    pub id: String,
    pub kind: RecordKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transcript {
    pub records: Vec<ReadRecord>,
    pub pending_tail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("invalid transcript record: {0}")]
    InvalidRecord(String),
    #[error("unsupported transcript schema: {0}")]
    UnsupportedSchema(String),
    #[error("transcript exceeds the bounded read limit")]
    TooLarge,
    #[error("invalid read cursor or cursor belongs to another source")]
    InvalidCursor,
    #[error("transcript changed before the cursor; read again without --since")]
    StaleCursor,
    #[error("read limit must be between 1 and 200")]
    InvalidLimit,
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Remove terminal control sequences without interpreting transcript text as a
/// screen. OSC payloads (titles, links, clipboard data) are never displayed.
pub fn clean_text(text: &str) -> String {
    let mut chars = text.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c == '\u{9b}' {
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        } else if matches!(c, '\u{9d}' | '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}') {
            while let Some(c) = chars.next() {
                if c == '\u{7}' || c == '\u{9c}' {
                    break;
                }
                if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
        } else if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']' | 'P' | '_' | '^' | 'X') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                }
                _ => {}
            }
        } else if !c.is_control() || c == '\n' || c == '\t' {
            out.push(c);
        }
    }
    out
}

fn record(id: String, kind: RecordKind, text: &str) -> ReadRecord {
    ReadRecord {
        id,
        kind,
        text: clean_text(text),
        tool_name: None,
        tool_call_id: None,
        is_error: None,
    }
}

fn content_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(blocks) = value.as_array() {
        return blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
    }
    value.to_string()
}

fn claude_message(value: &Value, index: usize) -> Result<Vec<ReadRecord>, ReadError> {
    let kind = match value["type"].as_str() {
        Some("user") => RecordKind::User,
        Some("assistant") => RecordKind::Assistant,
        _ => return Ok(vec![]),
    };
    let id = string(value, "uuid").unwrap_or_else(|| format!("record:{index}"));
    let content = value
        .get("message")
        .and_then(|m| m.get("content"))
        .ok_or_else(|| ReadError::InvalidRecord(format!("{id}: missing message content")))?;
    if let Some(text) = content.as_str() {
        return Ok(vec![record(id, kind, text)]);
    }
    let blocks = content
        .as_array()
        .ok_or_else(|| ReadError::InvalidRecord(format!("{id}: content is not text or blocks")))?;
    let mut records = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        let block_id = format!("{id}:{i}");
        let r = match block["type"].as_str() {
            Some("text") => record(block_id, kind, block["text"].as_str().unwrap_or_default()),
            Some("tool_use") => {
                let mut r = record(block_id, RecordKind::ToolCall, &block["input"].to_string());
                r.tool_name = string(block, "name");
                r.tool_call_id = string(block, "id");
                r
            }
            Some("tool_result") => {
                let mut r = record(
                    block_id,
                    RecordKind::ToolResult,
                    &content_text(&block["content"]),
                );
                r.tool_call_id = string(block, "tool_use_id");
                r.is_error = block["is_error"].as_bool();
                r
            }
            Some("image" | "document") => record(
                block_id,
                RecordKind::Notice,
                &format!("Attachment: {}", block["type"].as_str().unwrap()),
            ),
            Some("thinking" | "redacted_thinking") => continue,
            other => {
                return Err(ReadError::UnsupportedSchema(format!(
                    "Claude block {other:?}"
                )));
            }
        };
        records.push(r);
    }
    Ok(records)
}

pub fn parse_jsonl(runtime: Runtime, bytes: &[u8]) -> Result<Transcript, ReadError> {
    if !matches!(runtime, Runtime::Claude | Runtime::Codex | Runtime::Pi) {
        return Err(ReadError::UnsupportedSchema(format!("{runtime:?}")));
    }
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(ReadError::TooLarge);
    }
    let mut result = Transcript::default();
    let mut values = Vec::new();
    let mut offset = 0;
    for (index, line) in bytes.split(|b| *b == b'\n').enumerate() {
        let complete = offset + line.len() < bytes.len();
        offset += line.len() + 1;
        if line.len() > 1024 * 1024 {
            return Err(ReadError::TooLarge);
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match serde_json::from_slice(line) {
            Ok(value) => value,
            Err(error)
                if !complete
                    && (error.is_eof()
                        || std::str::from_utf8(line).is_err_and(|e| e.error_len().is_none())) =>
            {
                result.pending_tail = true;
                break;
            }
            Err(error) => {
                return Err(ReadError::InvalidRecord(format!(
                    "line {}: {error}",
                    index + 1
                )));
            }
        };
        values.push(value);
    }
    if runtime == Runtime::Claude {
        values.retain(|v| v["isSidechain"] != true);
    }
    let selected = if runtime == Runtime::Pi {
        branch_entries(&values, "id", "parentId")?
    } else if runtime == Runtime::Claude && values.iter().any(|v| v.get("parentUuid").is_some()) {
        branch_entries(&values, "uuid", "parentUuid")?
    } else {
        values.iter().enumerate().collect()
    };
    for (index, value) in selected {
        result.records.extend(match runtime {
            Runtime::Claude => claude_message(&value, index)?,
            Runtime::Codex => codex_item(&value, index)?,
            Runtime::Pi => pi_message(value, index)?,
            _ => unreachable!(),
        });
    }
    Ok(result)
}

fn branch_entries<'a>(
    values: &'a [Value],
    id_key: &str,
    parent_key: &str,
) -> Result<Vec<(usize, &'a Value)>, ReadError> {
    let by_id: std::collections::HashMap<_, _> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.get(parent_key).is_some())
        .filter_map(|(i, v)| v[id_key].as_str().map(|id| (id, (i, v))))
        .collect();
    let mut next = values
        .iter()
        .rev()
        .find(|v| v.get(parent_key).is_some())
        .and_then(|v| v[id_key].as_str());
    let mut selected = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = next {
        if !seen.insert(id) {
            return Err(ReadError::InvalidRecord("cyclic session ancestry".into()));
        }
        let &(index, value) = by_id
            .get(id)
            .ok_or_else(|| ReadError::InvalidRecord(format!("missing branch parent {id}")))?;
        selected.push((index, value));
        next = value[parent_key].as_str();
    }
    selected.reverse();
    Ok(selected)
}

fn pi_message(value: &Value, index: usize) -> Result<Vec<ReadRecord>, ReadError> {
    if value["type"] != "message" {
        return Ok(vec![]);
    }
    let message = &value["message"];
    let id = string(value, "id").unwrap_or_else(|| format!("entry:{index}"));
    if message["role"] == "toolResult" {
        let mut r = record(
            id,
            RecordKind::ToolResult,
            &content_text(&message["content"]),
        );
        r.tool_name = string(message, "toolName");
        r.tool_call_id = string(message, "toolCallId");
        r.is_error = message["isError"].as_bool();
        return Ok(vec![r]);
    }
    let kind = match message["role"].as_str() {
        Some("user") => RecordKind::User,
        Some("assistant") => RecordKind::Assistant,
        _ => return Ok(vec![]),
    };
    if let Some(text) = message["content"].as_str() {
        return Ok(vec![record(id, kind, text)]);
    }
    let blocks = message["content"]
        .as_array()
        .ok_or_else(|| ReadError::InvalidRecord(format!("{id}: missing content")))?;
    let mut result = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        let id = format!("{id}:{i}");
        match block["type"].as_str() {
            Some("text") => {
                result.push(record(id, kind, block["text"].as_str().unwrap_or_default()))
            }
            Some("toolCall") => {
                let mut r = record(id, RecordKind::ToolCall, &block["arguments"].to_string());
                r.tool_name = string(block, "name");
                r.tool_call_id = string(block, "id");
                result.push(r);
            }
            _ => {}
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadPage {
    pub records: Vec<ReadRecord>,
    pub next_cursor: String,
    pub has_older: bool,
    pub has_more: bool,
    pub pending_tail: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Transcript,
    TerminalText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadReport {
    pub agent_id: String,
    pub runtime: Runtime,
    pub session_id: String,
    pub source_kind: SourceKind,
    #[serde(flatten)]
    pub page: ReadPage,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    version: u8,
    source: String,
    position: usize,
    prefix: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn prefix_digest(records: &[ReadRecord]) -> String {
    digest(&serde_json::to_vec(records).expect("read records serialize"))
}

/// Initial reads show the newest window; a cursor reads forward from the last
/// delivered block. Rewritten branches or mutable results never silently skip.
pub fn paginate(
    source: &str,
    transcript: &Transcript,
    since: Option<&str>,
    limit: usize,
) -> Result<ReadPage, ReadError> {
    if !(1..=200).contains(&limit) {
        return Err(ReadError::InvalidLimit);
    }
    let source = digest(source.as_bytes());
    let start = if let Some(since) = since {
        if since.len() > 1024 {
            return Err(ReadError::InvalidCursor);
        }
        let cursor: Cursor = serde_json::from_str(since).map_err(|_| ReadError::InvalidCursor)?;
        if cursor.version != 1 || cursor.source != source {
            return Err(ReadError::InvalidCursor);
        }
        if cursor.position > transcript.records.len()
            || prefix_digest(&transcript.records[..cursor.position]) != cursor.prefix
        {
            return Err(ReadError::StaleCursor);
        }
        cursor.position
    } else {
        transcript.records.len().saturating_sub(limit)
    };
    let end = (start + limit).min(transcript.records.len());
    let cursor = Cursor {
        version: 1,
        source,
        position: end,
        prefix: prefix_digest(&transcript.records[..end]),
    };
    Ok(ReadPage {
        records: transcript.records[start..end].to_vec(),
        next_cursor: serde_json::to_string(&cursor).expect("read cursor serializes"),
        has_older: start > 0,
        has_more: end < transcript.records.len(),
        pending_tail: transcript.pending_tail,
    })
}

fn codex_item(value: &Value, index: usize) -> Result<Vec<ReadRecord>, ReadError> {
    if value["type"] != "response_item" {
        return Ok(vec![]);
    }
    let item = &value["payload"];
    let id = string(item, "id").unwrap_or_else(|| format!("item:{index}"));
    match item["type"].as_str() {
        Some("message") => {
            let kind = match item["role"].as_str() {
                Some("user") => RecordKind::User,
                Some("assistant") => RecordKind::Assistant,
                _ => return Ok(vec![]),
            };
            let blocks = item["content"].as_array().ok_or_else(|| {
                ReadError::InvalidRecord(format!("{id}: missing message content"))
            })?;
            blocks
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let id = format!("{id}:{i}");
                    match b["type"].as_str() {
                        Some("input_text" | "output_text" | "text") => Ok(record(
                            id,
                            kind,
                            b["text"].as_str().ok_or_else(|| {
                                ReadError::InvalidRecord("missing Codex text".into())
                            })?,
                        )),
                        Some("input_image" | "image") => {
                            Ok(record(id, RecordKind::Notice, "Attachment: image"))
                        }
                        Some("input_audio" | "output_audio" | "audio") => {
                            Ok(record(id, RecordKind::Notice, "Attachment: audio"))
                        }
                        other => Err(ReadError::UnsupportedSchema(format!(
                            "Codex content {other:?}"
                        ))),
                    }
                })
                .collect()
        }
        Some("function_call" | "custom_tool_call") => {
            let input = item
                .get("arguments")
                .or_else(|| item.get("input"))
                .unwrap_or(&Value::Null);
            let mut r = record(id, RecordKind::ToolCall, &content_text(input));
            r.tool_name = string(item, "name");
            r.tool_call_id = string(item, "call_id");
            Ok(vec![r])
        }
        Some("function_call_output" | "custom_tool_call_output") => {
            let mut r = record(id, RecordKind::ToolResult, &content_text(&item["output"]));
            r.tool_call_id = string(item, "call_id");
            Ok(vec![r])
        }
        Some("agent_message") => Ok(vec![record(
            id,
            RecordKind::Assistant,
            &content_text(&item["content"]),
        )]),
        Some("reasoning" | "compaction") => Ok(vec![]),
        other => Err(ReadError::UnsupportedSchema(format!(
            "Codex item {other:?}"
        ))),
    }
}

/// Normalize both OpenCode v2 exports and v1 message/part snapshots.
pub fn parse_opencode(snapshot: &Value) -> Result<Transcript, ReadError> {
    let messages = snapshot["messages"]
        .as_array()
        .ok_or_else(|| ReadError::InvalidRecord("OpenCode messages missing".into()))?;
    let mut result = Transcript::default();
    for (index, message) in messages.iter().enumerate() {
        let info = message.get("info").unwrap_or(message);
        let id = string(info, "id").unwrap_or_else(|| format!("message:{index}"));
        let kind = match info["role"].as_str().or_else(|| info["type"].as_str()) {
            Some("user") => RecordKind::User,
            Some("assistant") => RecordKind::Assistant,
            Some(
                "system" | "synthetic" | "idle" | "compaction" | "agent-switched"
                | "model-switched",
            ) => continue,
            other => {
                return Err(ReadError::UnsupportedSchema(format!(
                    "OpenCode message {other:?}"
                )));
            }
        };
        if let Some(text) = info["text"].as_str() {
            result.records.push(record(id.clone(), kind, text));
        }
        let blocks = message
            .get("parts")
            .or_else(|| info.get("content"))
            .and_then(Value::as_array);
        if let Some(blocks) = blocks {
            for (i, block) in blocks.iter().enumerate() {
                let block_id = string(block, "id").unwrap_or_else(|| format!("{id}:{i}"));
                match block["type"].as_str() {
                    Some("text") => result.records.push(record(
                        block_id,
                        kind,
                        block["text"].as_str().unwrap_or_default(),
                    )),
                    Some("tool") => {
                        let state = &block["state"];
                        let tool_name = string(block, "tool").or_else(|| string(block, "name"));
                        let call_id = string(block, "callID").or_else(|| string(block, "id"));
                        let mut call = record(
                            format!("{block_id}:call"),
                            RecordKind::ToolCall,
                            &state["input"].to_string(),
                        );
                        call.tool_name = tool_name.clone();
                        call.tool_call_id = call_id.clone();
                        result.records.push(call);
                        if matches!(state["status"].as_str(), Some("completed" | "error")) {
                            let error = state["status"] == "error";
                            let output = if error {
                                &state["error"]
                            } else {
                                &state["output"]
                            };
                            let mut r = record(
                                format!("{block_id}:result"),
                                RecordKind::ToolResult,
                                &content_text(output),
                            );
                            r.tool_name = tool_name;
                            r.tool_call_id = call_id;
                            r.is_error = Some(error);
                            result.records.push(r);
                        }
                    }
                    Some("file") => result.records.push(record(
                        block_id,
                        RecordKind::Notice,
                        &format!(
                            "Attachment: {}",
                            block["filename"].as_str().unwrap_or("file")
                        ),
                    )),
                    Some(
                        "reasoning" | "step-start" | "step-finish" | "snapshot" | "patch" | "agent"
                        | "retry" | "compaction",
                    ) => {}
                    other => {
                        return Err(ReadError::UnsupportedSchema(format!(
                            "OpenCode part {other:?}"
                        )));
                    }
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
