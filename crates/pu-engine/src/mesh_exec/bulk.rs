//! Output observation over the separately authenticated bulk connection.
//! Subscriptions are requested on control and handed to the bulk session bound
//! to that same control binding. Frames come from the run's bounded ring, so a
//! slow or stalled bulk reader never back-pressures the PTY or the control
//! connection: its bounded send times out, the bulk session closes, and evicted
//! output is later reported as an explicit gap on resubscription.

use super::Executor;
use crate::{mesh_sessions, output_buffer::OutputBuffer};
use pu_core::mesh::{
    response::{ErrorCode, MAX_OUTPUT_CHUNK, OutputFrame},
    wire::AgentTarget,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

/// Pending subscription commands per control binding.
const QUEUE: usize = 8;
/// Idle poll interval while no subscribed run produced output.
const IDLE: Duration = Duration::from_millis(25);
/// An empty frame keeps an idle subscription inside the peer's read deadline and
/// detects a dead or stalled host without waiting for output.
const TICK: Duration = Duration::from_secs(2);
/// After the process exits, the PTY reader may still append its final bytes;
/// output ends only once it has been quiet this long since exit.
const SETTLE: Duration = Duration::from_millis(250);

pub(super) struct Subscribe {
    pub target: AgentTarget,
    pub incarnation: String,
    pub start: usize,
    pub output: Arc<OutputBuffer>,
    pub exit: watch::Receiver<Option<i32>>,
}

pub(super) enum Command {
    /// Replaces any existing subscription for the same run.
    Subscribe(Subscribe),
    Unsubscribe(AgentTarget),
}

struct Slot {
    tx: mpsc::Sender<Command>,
    rx: Option<mpsc::Receiver<Command>>,
}

#[derive(Default)]
pub(super) struct Registry {
    pub(super) owner: Mutex<Weak<Executor>>,
    slots: Mutex<HashMap<String, Slot>>,
}

impl Registry {
    pub(super) fn open(&self, binding: &str) {
        let (tx, rx) = mpsc::channel(QUEUE);
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(binding.to_string(), Slot { tx, rx: Some(rx) });
    }

    /// Dropping the sender ends the paired bulk pump.
    pub(super) fn close(&self, binding: &str) {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(binding);
    }

    pub(super) fn send(&self, binding: &str, command: Command) -> Result<(), ErrorCode> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let slot = slots.get(binding).ok_or(ErrorCode::Unavailable)?;
        slot.tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => ErrorCode::Capacity,
            mpsc::error::TrySendError::Closed(_) => ErrorCode::Unavailable,
        })
    }

    /// Exactly one bulk session may serve a control binding.
    fn take(&self, binding: &str) -> Option<mpsc::Receiver<Command>> {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(binding)?
            .rx
            .take()
    }
}

struct Active {
    request: Subscribe,
    permit: mesh_sessions::Subscription,
    offset: usize,
    sent: Instant,
    /// When exit was first seen with everything sent, and the total then.
    drained: Option<(Instant, usize)>,
}

pub(super) async fn serve(
    executor: Arc<Executor>,
    mut session: mesh_sessions::Session<tokio::net::TcpStream>,
) {
    let binding = session.welcome().control_binding.clone();
    let Some(mut commands) = executor.bulk.take(&binding) else {
        session.close();
        return;
    };
    let mut active: Vec<Active> = Vec::new();
    loop {
        let mut progressed = false;
        let mut index = 0;
        while index < active.len() {
            match pump(&mut session, &mut active[index]).await {
                Ok(Step::Sent) => {
                    progressed = true;
                    index += 1;
                }
                Ok(Step::Idle) => index += 1,
                Ok(Step::Finished) => {
                    progressed = true;
                    active.swap_remove(index);
                }
                // Revocation, expiry, a fenced run or a stalled reader closes
                // the bulk session; control and execution are unaffected.
                Err(_) => return,
            }
        }
        let next = if progressed {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::error::TryRecvError::Empty) => None,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        } else {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => Some(command),
                    None => return,
                },
                _ = tokio::time::sleep(IDLE) => None,
            }
        };
        match next {
            Some(Command::Unsubscribe(target)) => {
                active.retain(|a| a.request.target != target);
            }
            Some(Command::Subscribe(request)) => {
                // Replace first so the permit is released before re-acquiring.
                active.retain(|a| a.request.target != request.target);
                // Session limits bound concurrent subscriptions per peer and total.
                match session.subscribe(request.target.export.id.clone()).await {
                    Ok(permit) => {
                        let offset = request.start;
                        active.push(Active {
                            request,
                            permit,
                            offset,
                            sent: Instant::now(),
                            drained: None,
                        });
                    }
                    Err(_) => return,
                }
            }
            None => {}
        }
    }
}

enum Step {
    Sent,
    Idle,
    Finished,
}

async fn pump(
    session: &mut mesh_sessions::Session<tokio::net::TcpStream>,
    active: &mut Active,
) -> Result<Step, mesh_sessions::Error> {
    let exited = active.request.exit.borrow().is_some();
    let chunk = active
        .request
        .output
        .read_range(active.offset, MAX_OUTPUT_CHUNK)
        .map_err(|_| mesh_sessions::Error::Frame)?;
    let caught_up = exited && chunk.next == chunk.total;
    let end = match (caught_up, active.drained) {
        (false, _) => {
            active.drained = None;
            false
        }
        (true, Some((since, total))) if total == chunk.total => since.elapsed() >= SETTLE,
        (true, _) => {
            active.drained = Some((Instant::now(), chunk.total));
            false
        }
    };
    if chunk.bytes.is_empty() && chunk.gap.is_none() && !end && active.sent.elapsed() < TICK {
        return Ok(Step::Idle);
    }
    let frame = OutputFrame {
        target: active.request.target.clone(),
        incarnation: active.request.incarnation.clone(),
        offset: chunk.start as u64,
        gap: chunk.gap.map(|gap| (gap.end - gap.start) as u64),
        data: chunk.bytes,
        end,
    };
    let raw = frame.data.len();
    let encoded = frame.encode().map_err(|_| mesh_sessions::Error::Frame)?;
    let target = active.request.target.clone();
    session
        .send_output_with(&active.permit, move |ledger, exports, _caller| {
            // Current export generation and run ownership, under the barrier.
            exports
                .resolve(&target.export.id, target.export.generation)
                .map_err(|_| mesh_sessions::Error::Denied)?;
            Executor::ledger_check_run(ledger, &target)?;
            Ok((raw, encoded))
        })
        .await?;
    active.offset = chunk.next;
    active.sent = Instant::now();
    Ok(if end { Step::Finished } else { Step::Sent })
}
