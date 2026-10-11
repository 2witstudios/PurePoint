use crate::{client, daemon_ctrl, error::CliError, output};
use pu_core::protocol::Request;
use pu_core::read::{ReadReport, RecordKind, SourceKind};
use std::path::Path;

pub async fn run(
    socket: &Path,
    agent_id: &str,
    limit: usize,
    since: Option<String>,
    session_id: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let response = client::send_request(
        socket,
        &Request::Read {
            project_root: super::agent_project_root(),
            agent_id: agent_id.into(),
            limit,
            since,
            session_id,
        },
    )
    .await?;
    output::print_response(&output::check_response(response, json)?, json)
}

pub fn format_report(report: &ReadReport) -> String {
    let mut text = if report.source_kind == SourceKind::TerminalText {
        format!(
            "Terminal text — {}\n",
            pu_core::read::clean_text(&report.agent_id)
        )
    } else {
        String::new()
    };
    for record in &report.page.records {
        if report.source_kind == SourceKind::TerminalText {
            text.push_str(&record.text);
            text.push('\n');
            continue;
        }
        let label = match record.kind {
            RecordKind::User => "User",
            RecordKind::Assistant => "Assistant",
            RecordKind::ToolCall => "Tool call",
            RecordKind::ToolResult => "Tool result",
            RecordKind::Notice => "Notice",
            RecordKind::Terminal => "Terminal",
        };
        text.push_str(label);
        if let Some(name) = &record.tool_name {
            text.push_str(": ");
            text.push_str(&pu_core::read::clean_text(name));
        }
        if record.is_error == Some(true) {
            text.push_str(" (error)");
        }
        text.push('\n');
        text.push_str(&record.text);
        text.push_str("\n\n");
    }
    text
}
