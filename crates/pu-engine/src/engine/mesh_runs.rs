//! Engine primitives for worker-owned mesh runs. Authorization, admission and
//! durable state transitions happen in `mesh_exec` under the authority barrier
//! and per-agent boundary; these helpers only perform the local effects.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use pu_core::{
    config, manifest, paths,
    types::{AgentEntry, AgentStatus, WorktreeEntry, WorktreeStatus},
};
use tokio::sync::watch;

use super::Engine;
use crate::output_buffer::OutputBuffer;
use crate::pty_manager::SpawnConfig;

/// Receiver-resolved launch for one admitted run. Nothing here comes from the
/// caller except the runtime ID (checked against receiver grants), the exact,
/// verified commit and the prompt text.
pub struct MeshSpawn {
    pub agent_id: String,
    pub run_id: String,
    pub project_root: PathBuf,
    pub runtime: String,
    pub prompt: String,
    pub source_commit: String,
    pub worktree_id: String,
    pub branch: String,
    /// The fully wrapped guardian command, arguments and extra environment.
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum MeshSpawnError {
    /// No process could have been created for this run.
    #[error("prelaunch failure: {0}")]
    Prelaunch(String),
}

pub struct MeshSpawned {
    pub exit_rx: watch::Receiver<Option<i32>>,
}

/// Attachable output/exit state of a live mesh run's PTY session.
pub struct MeshSession {
    pub output: Arc<OutputBuffer>,
    pub exit_rx: watch::Receiver<Option<i32>>,
    master: Arc<OwnedFd>,
}

/// Exact single-attempt delivery evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Written,
    Partial(u32),
    NotWritten,
    Unknown,
}

/// Bounds one PTY input attempt; a stalled reader never blocks the bridge.
pub const INPUT_DEADLINE: Duration = Duration::from_millis(500);

impl Engine {
    /// Resolve the receiver's own runtime configuration for an exported project.
    /// The project must already be an initialized PurePoint project.
    pub(crate) fn mesh_runtime_command(
        project_root: &Path,
        runtime: &str,
        prompt: &str,
    ) -> Result<(String, Vec<String>, Option<String>, bool), MeshSpawnError> {
        if !paths::manifest_path(project_root).exists() {
            return Err(MeshSpawnError::Prelaunch(
                "exported project is not initialized".into(),
            ));
        }
        let cfg = config::load_config_strict(project_root)
            .map_err(|_| MeshSpawnError::Prelaunch("receiver configuration".into()))?;
        let agent_cfg = config::resolve_agent(&cfg, runtime)
            .ok_or_else(|| MeshSpawnError::Prelaunch("runtime not configured".into()))?
            .clone();
        Self::agent_command(&agent_cfg, runtime, prompt, false, &[])
            .map_err(|_| MeshSpawnError::Prelaunch("runtime command".into()))
    }

    /// Create the run's worktree at the exact commit and start the guarded
    /// process. Every error is returned before any process exists.
    pub(crate) async fn spawn_mesh_run(
        &self,
        spec: MeshSpawn,
        inject_prompt: bool,
        session_id: Option<String>,
    ) -> Result<MeshSpawned, MeshSpawnError> {
        let root = spec.project_root.clone();
        let wt_path = paths::worktree_path(&root, &spec.worktree_id);
        crate::git::create_worktree(&root, &wt_path, &spec.branch, &spec.source_commit)
            .await
            .map_err(|_| MeshSpawnError::Prelaunch("worktree creation".into()))?;
        let cwd = wt_path.to_string_lossy().to_string();
        let mut env = self.agent_env().await;
        env.push(("PU_AGENT_ID".into(), spec.agent_id.clone()));
        env.push(("PU_PROJECT_ROOT".into(), root.to_string_lossy().to_string()));
        env.push(("PU_MESH_RUN_ID".into(), spec.run_id.clone()));
        env.extend(spec.env.iter().cloned());
        let config = SpawnConfig {
            command: spec.command.clone(),
            args: spec.args.clone(),
            cwd: cwd.clone(),
            env,
            env_remove: vec!["CLAUDECODE".into()],
            cols: 120,
            rows: 40,
        };
        let mut handle = match self.pty_host.spawn(config).await {
            Ok(handle) => handle,
            Err(error) => {
                self.rollback_worktree(&root, Some(&spec.worktree_id), Some(&spec.branch))
                    .await;
                return Err(MeshSpawnError::Prelaunch(format!("process spawn: {error}")));
            }
        };
        let project_root = root.to_string_lossy().to_string();
        handle.project_root = Some(project_root.clone());
        handle.name = format!("mesh-{}", spec.worktree_id);
        handle.agent_type = spec.runtime.clone();
        handle.worktree_id = Some(spec.worktree_id.clone());
        handle.screen_aware = spec.runtime == "claude";
        if inject_prompt && !spec.prompt.is_empty() {
            self.inject_prompt_after_quiet_period(
                &handle,
                &spec.agent_id,
                spec.prompt.as_bytes().to_vec(),
            )
            .await;
        }
        let pid = handle.pid;
        let exit_rx = handle.exit_rx.clone();
        self.watch_natural_exit(&project_root, &spec.agent_id, pid, exit_rx.clone());
        self.sessions
            .lock()
            .await
            .insert(spec.agent_id.clone(), handle);
        // The manifest makes the worker-owned run visible to local `pu status`.
        // It is observation only; the ledger remains the authority.
        let entry = AgentEntry {
            id: spec.agent_id.clone(),
            name: format!("mesh-{}", spec.worktree_id),
            agent_type: spec.runtime.clone(),
            status: AgentStatus::Running,
            prompt: Some(spec.prompt.clone()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(pid),
            session_id,
            suspended_at: None,
            suspended: false,
            command: None,
            plan_mode: false,
            trigger_seq_index: None,
            trigger_state: None,
            trigger_total: None,
            gate_attempts: None,
            no_trigger: true,
            trigger_name: None,
        };
        let manifest_root = root.clone();
        let wt_id = spec.worktree_id.clone();
        let branch = spec.branch.clone();
        let base = spec.source_commit.clone();
        let name = format!("mesh-{}", spec.worktree_id);
        let agent_id = spec.agent_id.clone();
        let recorded = tokio::task::spawn_blocking(move || {
            manifest::update_manifest(&manifest_root, move |mut m| {
                let worktree = m
                    .worktrees
                    .entry(wt_id.clone())
                    .or_insert_with(|| WorktreeEntry {
                        id: wt_id.clone(),
                        name,
                        path: cwd,
                        branch,
                        base_branch: Some(base),
                        status: WorktreeStatus::Active,
                        agents: IndexMap::new(),
                        created_at: chrono::Utc::now(),
                        merged_at: None,
                        error: None,
                    });
                worktree.agents.insert(agent_id, entry);
                m
            })
        })
        .await;
        if !matches!(recorded, Ok(Ok(_))) {
            // The process exists; only local visibility failed. The guardian and
            // ledger still own its lifetime, so this is logged, not rolled back.
            tracing::warn!(agent_id = %spec.agent_id, "mesh run manifest entry not recorded");
        }
        self.notify_status_change(&project_root).await;
        Ok(MeshSpawned { exit_rx })
    }

    pub(crate) async fn mesh_session(&self, agent_id: &str) -> Option<MeshSession> {
        let sessions = self.sessions.lock().await;
        let handle = sessions.get(agent_id)?;
        Some(MeshSession {
            output: handle.output_buffer.clone(),
            exit_rx: handle.exit_rx.clone(),
            master: handle.master_fd(),
        })
    }

    /// Ask the guardian (the PTY session leader) to terminate its whole run tree.
    /// Termination proof arrives later from its outcome record, never from here.
    pub(crate) async fn mesh_terminate(&self, agent_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        let Some(handle) = sessions.get(agent_id) else {
            return false;
        };
        if handle.exit_rx.borrow().is_some() {
            return false;
        }
        let Ok(pid) = i32::try_from(handle.pid) else {
            return false;
        };
        // SAFETY: plain signal delivery to the guardian's own process group.
        unsafe { libc::killpg(pid, libc::SIGTERM) == 0 }
    }

    pub(crate) async fn mesh_resize(&self, agent_id: &str, cols: u16, rows: u16) -> bool {
        let Some(session) = self.mesh_session(agent_id).await else {
            return false;
        };
        session.output.resize_terminal(rows, cols);
        self.pty_host
            .resize_fd(&session.master, cols, rows)
            .await
            .is_ok()
    }
}

impl MeshSession {
    /// One bounded write attempt with an exact byte count. Never retried.
    pub fn write_once(&self, data: &[u8], deadline: Duration) -> Delivery {
        write_once(&self.master, data, deadline)
    }
}

pub(crate) fn write_once(master: &Arc<OwnedFd>, data: &[u8], deadline: Duration) -> Delivery {
    let fd = master.as_raw_fd();
    let started = Instant::now();
    let mut written = 0usize;
    while written < data.len() {
        let Some(remaining) = deadline.checked_sub(started.elapsed()) else {
            break;
        };
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let timeout = i32::try_from(remaining.as_millis())
            .unwrap_or(i32::MAX)
            .max(1);
        // SAFETY: one valid pollfd for a descriptor kept alive by `master`.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return outcome(written, data.len(), true);
        }
        if ready == 0 {
            break;
        }
        if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return outcome(written, data.len(), true);
        }
        // Small chunks keep each write below the tty queue so POLLOUT means it
        // completes without blocking past the deadline.
        let chunk = (data.len() - written).min(256);
        // SAFETY: the slice is valid for `chunk` bytes; fd is open.
        let n = unsafe { libc::write(fd, data[written..].as_ptr().cast(), chunk) };
        if n < 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.kind(),
                std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return outcome(written, data.len(), true);
        }
        written += n as usize;
    }
    outcome(written, data.len(), false)
}

fn outcome(written: usize, total: usize, failed: bool) -> Delivery {
    match written {
        0 if failed => Delivery::NotWritten,
        // A deadline before any byte is also a certain non-delivery.
        0 => Delivery::NotWritten,
        n if n >= total => Delivery::Written,
        n => u32::try_from(n).map_or(Delivery::Unknown, Delivery::Partial),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipe() -> (Arc<OwnedFd>, OwnedFd) {
        let (read, write) = nix::unistd::pipe().unwrap();
        (Arc::new(write), read)
    }

    #[test]
    fn given_ready_reader_should_report_exact_written_input() {
        // given
        let (write, _read) = pipe();
        // when / then
        assert_eq!(
            write_once(&write, b"hello", INPUT_DEADLINE),
            Delivery::Written
        );
    }

    #[test]
    fn given_stalled_reader_should_report_partial_count_without_retry() {
        // given: fill the pipe so only part of the next input fits
        let (write, read) = pipe();
        let fd = write.as_raw_fd();
        // SAFETY: test-owned descriptor.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        let filler = vec![0u8; 4096];
        loop {
            // SAFETY: test-owned descriptor and buffer.
            let n = unsafe { libc::write(fd, filler.as_ptr().cast(), filler.len()) };
            if n < filler.len() as isize {
                break;
            }
        }
        // when
        let delivery = write_once(&write, &[1u8; 8192], Duration::from_millis(50));
        // then
        assert!(
            matches!(delivery, Delivery::NotWritten | Delivery::Partial(_)),
            "{delivery:?}"
        );
        drop(read);
    }

    #[test]
    fn given_closed_reader_should_report_not_written() {
        // given
        let (write, read) = pipe();
        drop(read);
        // SAFETY: ignore SIGPIPE for this process-wide test write.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
        // when / then
        assert_eq!(
            write_once(&write, b"lost", INPUT_DEADLINE),
            Delivery::NotWritten
        );
    }
}
