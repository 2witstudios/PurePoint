//! WIN-1.2 spike: a private NDJSON daemon channel over Windows named pipes.
//! See docs/research/windows-ipc.md for results.
#![cfg(windows)]

pub mod client;
pub mod naming;
pub mod server;
pub mod signal;
pub mod win;

use std::io;
use std::time::{Duration, Instant};

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

pub const MAX_MESSAGE_SIZE: u64 = 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 1024;
pub const FIRST_LINE_TIMEOUT: Duration = Duration::from_secs(30);

/// Opens a client end, retrying while every instance is busy (the server
/// creates the next listening instance right after each connect).
pub async fn open_client(name: &str, deadline: Duration) -> io::Result<NamedPipeClient> {
    let end = Instant::now() + deadline;
    loop {
        match ClientOptions::new().open(name) {
            Ok(c) => return Ok(c),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                if Instant::now() >= end {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
