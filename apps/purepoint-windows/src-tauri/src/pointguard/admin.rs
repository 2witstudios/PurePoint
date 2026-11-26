//! The owner admin channel of an app-owned bridge: HTTP/1.1 `POST /admin/v1` with the bearer
//! token over the bridge's owner pipe (`\\.\pipe\purepoint-<hash16>-pointguard-owner`).
//!
//! The Mac posts to a loopback TCP URL; Windows uses the pipe the bridge serves (WIN-9.1).
//! Pipe names are global, so the token is sent only after the pipe's server is confirmed to
//! be the exact owned child (its pid) running as this user.

use std::io::{Read, Write};
use std::time::Duration;

use serde_json::Value;

/// The Mac's request timeout.
pub const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE: usize = 1024 * 1024;
const FAILED: &str = "Point Guard setup request failed. Retry after checking the service.";

/// One request as HTTP/1.1 bytes.
pub fn encode(token: &str, body: &Value) -> Vec<u8> {
    let body = body.to_string();
    let mut out = format!(
        "POST /admin/v1 HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// A whole response (the server closes after one) as the `result`, or the bridge's
/// sanitized error message.
pub fn decode(raw: &[u8]) -> Result<Value, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| FAILED.to_string())?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| FAILED.to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| FAILED.to_string())?;
    if !(200..=499).contains(&status) {
        return Err(FAILED.into());
    }
    let chunked = head.lines().any(|l| {
        l.to_ascii_lowercase()
            .replace(' ', "")
            .starts_with("transfer-encoding:chunked")
    });
    let body = if chunked {
        dechunk(&raw[split + 4..])?
    } else {
        raw[split + 4..].to_vec()
    };
    let value: Value = serde_json::from_slice(&body).map_err(|_| FAILED.to_string())?;
    if value["ok"] == Value::Bool(true) {
        return Ok(value["result"].clone());
    }
    Err(value["error"]["message"]
        .as_str()
        .unwrap_or("Point Guard rejected this setup request.")
        .to_string())
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let bad = || FAILED.to_string();
    let mut out = Vec::new();
    loop {
        let end = data.windows(2).position(|w| w == b"\r\n").ok_or_else(bad)?;
        let size_text = std::str::from_utf8(&data[..end]).map_err(|_| bad())?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| bad())?;
        data = &data[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err(bad());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

/// Send one operation to `pipe`, served by `server_pid`. Blocking; bounded by [`TIMEOUT`].
#[cfg(windows)]
pub fn call(pipe: &str, server_pid: u32, token: &str, body: &Value) -> Result<Value, String> {
    use std::os::windows::io::AsRawHandle;
    const ERROR_PIPE_BUSY: i32 = 231;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut stream = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe)
        {
            Ok(stream) => break stream,
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY)
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return Err(FAILED.into()),
        }
    };
    let server = pu_transport::pipe_server_identity(stream.as_raw_handle())
        .map_err(|_| FAILED.to_string())?;
    if !server.is_same_user() || server.pid != Some(server_pid) {
        return Err("Point Guard's admin pipe is not served by the app-owned runtime; the request was not sent.".into());
    }
    let bytes = encode(token, body);
    // Synchronous pipes have no timeouts: bound the exchange on a thread.
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = stream
            .write_all(&bytes)
            .map_err(|_| FAILED.to_string())
            .and_then(|()| {
                let mut raw = Vec::new();
                (&mut stream)
                    .take(MAX_RESPONSE as u64 + 1)
                    .read_to_end(&mut raw)
                    .map_err(|_| FAILED.to_string())?;
                if raw.len() > MAX_RESPONSE {
                    return Err(FAILED.to_string());
                }
                decode(&raw)
            });
        let _ = send.send(result);
    });
    receive
        .recv_timeout(TIMEOUT)
        .unwrap_or_else(|_| Err(FAILED.into()))
}

#[cfg(not(windows))]
pub fn call(_pipe: &str, _server_pid: u32, _token: &str, _body: &Value) -> Result<Value, String> {
    Err(FAILED.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_request_should_carry_the_bearer_and_length() {
        let bytes = encode("tok", &json!({"operation": "status"}));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("POST /admin/v1 HTTP/1.1\r\n"));
        assert!(text.contains("Authorization: Bearer tok\r\n"));
        assert!(text.contains("Content-Length: 22\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"operation\":\"status\"}"));
    }

    #[test]
    fn given_responses_should_return_result_or_bridge_message() {
        let ok = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"ok\":true,\"result\":{\"a\":1}}";
        assert_eq!(decode(ok), Ok(json!({"a": 1})));
        let rejected = b"HTTP/1.1 400 Bad Request\r\n\r\n{\"ok\":false,\"error\":{\"code\":\"x\",\"message\":\"No.\"}}";
        assert_eq!(decode(rejected), Err("No.".into()));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"ok\"\r\ne\r\n:true,\"result\"\r\n4\r\n:{}}\r\n0\r\n\r\n";
        assert_eq!(decode(chunked), Ok(json!({})));
        assert_eq!(decode(b"HTTP/1.1 500 X\r\n\r\n{}"), Err(FAILED.into()));
        assert_eq!(decode(b"garbage"), Err(FAILED.into()));
    }
}
