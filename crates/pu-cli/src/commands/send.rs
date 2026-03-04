use std::path::Path;
use pu_core::protocol::Request;
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(
    socket: &Path,
    agent_id: &str,
    text: Option<String>,
    no_enter: bool,
    keys: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let data = if let Some(key_name) = keys {
        translate_keys(&key_name)?
    } else if let Some(mut t) = text {
        if !no_enter {
            t.push('\r');
        }
        t.into_bytes()
    } else {
        return Err(CliError::Other("provide text or --keys".into()));
    };

    let resp = client::send_request(
        socket,
        &Request::Input {
            agent_id: agent_id.to_string(),
            data,
        },
    )
    .await?;

    output::check_response(resp, json)?;

    if json {
        println!("{}", serde_json::json!({"status": "ok", "agent_id": agent_id}));
    } else {
        println!("Sent to {agent_id}");
    }
    Ok(())
}

fn translate_keys(keys: &str) -> Result<Vec<u8>, CliError> {
    let mut result = Vec::new();
    for key in keys.split_whitespace() {
        match key {
            "C-c" | "ctrl-c" => result.push(0x03),
            "C-d" | "ctrl-d" => result.push(0x04),
            "C-z" | "ctrl-z" => result.push(0x1a),
            "C-l" | "ctrl-l" => result.push(0x0c),
            "C-a" | "ctrl-a" => result.push(0x01),
            "C-e" | "ctrl-e" => result.push(0x05),
            "C-u" | "ctrl-u" => result.push(0x15),
            "C-k" | "ctrl-k" => result.push(0x0b),
            "C-w" | "ctrl-w" => result.push(0x17),
            "Enter" | "enter" | "CR" => result.push(0x0d),
            "Tab" | "tab" => result.push(0x09),
            "Escape" | "escape" | "ESC" => result.push(0x1b),
            other => {
                // C-x pattern for any letter
                if let Some(letter) = other.strip_prefix("C-").or_else(|| other.strip_prefix("ctrl-")) {
                    if letter.len() == 1 {
                        let ch = letter.chars().next().unwrap();
                        if ch.is_ascii_lowercase() {
                            result.push(ch as u8 - b'a' + 1);
                            continue;
                        }
                    }
                }
                return Err(CliError::Other(format!("unknown key: {other}")));
            }
        }
    }
    Ok(result)
}
