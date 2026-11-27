//! Per-launch signal pipe replacing the UnixDatagram intake.
//!
//! One message per connection: the writer connects, writes one JSON frame
//! `{"token": ..., "payload": {...}}`, and closes. EOF delimits the message,
//! so the reader needs no framing. Name: `purepoint-<hash16>-signal-<nonce>`
//! with the same private DACL as the daemon pipe; the token arrives via
//! `PU_SIGNAL_TOKEN` and is compared in constant time.
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::windows::io::AsRawHandle;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

use crate::win::{self, PipeSecurity, Sid};

pub const MAX_SIGNAL: usize = 256 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum Delivery {
    Accepted(serde_json::Value),
    Rejected(String),
}

pub struct SignalPipe {
    name: String,
    token: String,
    security: PipeSecurity,
    expected: Sid,
    listening: NamedPipeServer,
}

impl SignalPipe {
    pub fn bind(name: &str, token: &str) -> io::Result<Self> {
        let security = PipeSecurity::current_user()?;
        let listening = security.create(name, true)?;
        Ok(Self {
            name: name.into(),
            token: token.into(),
            security,
            expected: win::current_user_sid()?,
            listening,
        })
    }

    pub async fn run(mut self, tx: mpsc::Sender<Delivery>) -> io::Result<()> {
        loop {
            let r = self.listening.connect().await;
            let next = self.security.create(&self.name, false)?;
            let pipe = std::mem::replace(&mut self.listening, next);
            if r.is_err() {
                continue;
            }
            if let Err(e) = verify_writer(&pipe, &self.expected) {
                let _ = tx.send(Delivery::Rejected(e.to_string())).await;
                continue;
            }
            let token = self.token.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(read_one(pipe, &token).await).await;
            });
        }
    }
}

fn verify_writer(pipe: &NamedPipeServer, expected: &Sid) -> io::Result<()> {
    let pid = win::pipe_client_pid(pipe.as_raw_handle())?;
    if &win::process_user_sid(pid)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "foreign writer",
        ));
    }
    Ok(())
}

async fn read_one(pipe: NamedPipeServer, token: &str) -> Delivery {
    let mut data = Vec::new();
    let mut limited = pipe.take(MAX_SIGNAL as u64 + 1);
    let read = limited.read_to_end(&mut data);
    match tokio::time::timeout(READ_TIMEOUT, read).await {
        Ok(Ok(_)) => {}
        // A writer's close shows up as ERROR_BROKEN_PIPE, which tokio maps
        // to EOF; anything else is a real failure.
        Ok(Err(e)) => return Delivery::Rejected(format!("read: {e}")),
        Err(_) => return Delivery::Rejected("timeout".into()),
    }
    if data.len() > MAX_SIGNAL {
        return Delivery::Rejected("too large".into());
    }
    let frame: serde_json::Value = match serde_json::from_slice(&data) {
        Ok(v) => v,
        Err(e) => return Delivery::Rejected(format!("json: {e}")),
    };
    let Some(received) = frame["token"].as_str() else {
        return Delivery::Rejected("no token".into());
    };
    if !token_matches(token.as_bytes(), received.as_bytes()) {
        return Delivery::Rejected("bad token".into());
    }
    Delivery::Accepted(frame["payload"].clone())
}

fn token_matches(expected: &[u8], received: &[u8]) -> bool {
    expected.len() == received.len()
        && expected
            .iter()
            .zip(received)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// Synchronous writer for the Rust `__agent-signal` helper: open (waiting
/// out PIPE_BUSY until `deadline`), write one frame, close.
pub fn send_frame(name: &str, frame: &[u8], deadline: Duration) -> io::Result<()> {
    let end = Instant::now() + deadline;
    let wide = win::wide(name);
    loop {
        match OpenOptions::new().write(true).open(name) {
            Ok(mut pipe) => return pipe.write_all(frame),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                unsafe { WaitNamedPipeW(wide.as_ptr(), left.as_millis().max(1) as u32) };
            }
            Err(e) => return Err(e),
        }
    }
}
