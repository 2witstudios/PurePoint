//! Client-side server verification.
//!
//! The DACL stops other users from *connecting* to our pipe, but not from
//! *creating* a pipe with our name first. A client must therefore check who
//! it is talking to: GetNamedPipeServerProcessId -> that process's token
//! user SID must be the caller's. That stops other users (including
//! services) squatting the name. It cannot stop a same-user impostor: only
//! the expected PID (from `~/.pu/daemon.sock.pid`) and/or the server's
//! image path do that.
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use tokio::net::windows::named_pipe::NamedPipeClient;

use crate::win::{self, Sid};

#[derive(Debug, Clone)]
pub struct ServerIdentity {
    pub pid: u32,
    pub sid: Sid,
    pub image: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ServerPolicy {
    pub expected_sid: Sid,
    pub expected_pid: Option<u32>,
    pub expected_image: Option<PathBuf>,
}

impl ServerPolicy {
    pub fn current_user() -> io::Result<Self> {
        Ok(Self {
            expected_sid: win::current_user_sid()?,
            expected_pid: None,
            expected_image: None,
        })
    }

    /// Adds the PID recorded in a `daemon.sock.pid` file.
    pub fn with_pid_file(mut self, path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let pid = text
            .trim()
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.expected_pid = Some(pid);
        Ok(self)
    }
}

#[derive(Debug)]
pub enum Refusal {
    Lookup(io::Error),
    Sid {
        expected: Sid,
        actual: Sid,
        pid: u32,
    },
    Pid {
        expected: u32,
        actual: u32,
    },
    Image {
        expected: PathBuf,
        actual: PathBuf,
    },
}

pub fn identify_server(client: &NamedPipeClient) -> io::Result<ServerIdentity> {
    let pid = win::pipe_server_pid(client.as_raw_handle())?;
    Ok(ServerIdentity {
        pid,
        sid: win::process_user_sid(pid)?,
        image: win::process_image(pid)?,
    })
}

pub fn verify_server(
    client: &NamedPipeClient,
    policy: &ServerPolicy,
) -> Result<ServerIdentity, Refusal> {
    let id = identify_server(client).map_err(Refusal::Lookup)?;
    if id.sid != policy.expected_sid {
        return Err(Refusal::Sid {
            expected: policy.expected_sid.clone(),
            actual: id.sid,
            pid: id.pid,
        });
    }
    if let Some(expected) = policy.expected_pid
        && expected != id.pid
    {
        return Err(Refusal::Pid {
            expected,
            actual: id.pid,
        });
    }
    if let Some(expected) = &policy.expected_image
        && !same_path(expected, &id.image)
    {
        return Err(Refusal::Image {
            expected: expected.clone(),
            actual: id.image,
        });
    }
    Ok(id)
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => a.as_os_str().eq_ignore_ascii_case(b.as_os_str()),
        _ => false,
    }
}
