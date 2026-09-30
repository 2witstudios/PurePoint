//! Confirmed prompt delivery to agents that draw an input box (Claude Code).
//!
//! Every step is decided from the emulated screen, not from timing guesses:
//! wait for the input box, type until the box shows the full text (retyping if
//! characters were dropped), press Enter only then, and confirm the box
//! cleared and the turn was echoed. Anything else is a loud error.

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::output_buffer::OutputBuffer;
use crate::pty_manager::NativePtyHost;
use crate::screen::{ScreenTracker, squash};

#[derive(Debug, Clone)]
pub struct DeliveryOptions {
    /// How long to wait for the input box to appear.
    pub ready_timeout: Duration,
    /// The screen must stay unchanged this long once the box is ready.
    pub ready_settle: Duration,
    /// Typed text that stops changing for this long is judged dropped.
    pub type_stall: Duration,
    /// Hard cap on one typing attempt.
    pub type_timeout: Duration,
    /// Typing attempts before giving up.
    pub max_type_attempts: u32,
    /// How long after Enter before pressing Enter once more.
    pub submit_retry_after: Duration,
    /// How long to wait for the box to clear and the turn to be echoed.
    pub submit_timeout: Duration,
}

impl Default for DeliveryOptions {
    fn default() -> Self {
        Self {
            ready_timeout: Duration::from_secs(60),
            ready_settle: Duration::from_millis(300),
            type_stall: Duration::from_millis(2000),
            type_timeout: Duration::from_secs(30),
            max_type_attempts: 3,
            submit_retry_after: Duration::from_millis(2500),
            submit_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("agent input box never became ready (is the agent waiting on a dialog?)")]
    NotReady,
    #[error(
        "typed text never fully appeared in the input box after {attempts} attempts (last saw {seen} of {expected} characters)"
    )]
    TypeMismatch {
        attempts: u32,
        seen: usize,
        expected: usize,
    },
    #[error("Enter did not submit the prompt: text is still in the input box")]
    NotSubmitted,
    #[error("input box cleared but the submitted turn was not echoed in full")]
    Unconfirmed,
    #[error("pty write failed: {0}")]
    Io(#[from] std::io::Error),
}

const POLL: Duration = Duration::from_millis(25);
const CTRL_U: &[u8] = b"\x15";
const BACKSPACE: u8 = 0x7f;

/// Window size of the PTY, so the emulated screen wraps like the real one.
fn winsize(fd: &OwnedFd) -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) } == 0;
    if ok && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (40, 120)
    }
}

/// Re-syncs the screen until `pred` holds or `deadline` passes.
async fn wait_until(
    tracker: &mut ScreenTracker,
    output: &OutputBuffer,
    deadline: Instant,
    pred: impl Fn(&ScreenTracker) -> bool,
) -> bool {
    let mut rx = output.subscribe();
    loop {
        tracker.sync(output);
        if pred(tracker) {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let wait = POLL.min(deadline - now);
        let _ = tokio::time::timeout(wait, rx.changed()).await;
    }
}

fn box_text(t: &ScreenTracker) -> Option<String> {
    t.input_box().map(|b| squash(&b.text))
}

async fn wait_ready(
    tracker: &mut ScreenTracker,
    output: &OutputBuffer,
    opts: &DeliveryOptions,
) -> Result<(), DeliveryError> {
    let deadline = Instant::now() + opts.ready_timeout;
    loop {
        if !wait_until(tracker, output, deadline, |t| t.is_ready()).await {
            return Err(DeliveryError::NotReady);
        }
        // Ready is only trustworthy once the screen stops redrawing.
        let before = output.current_offset();
        tokio::time::sleep(opts.ready_settle).await;
        tracker.sync(output);
        if output.current_offset() == before && tracker.is_ready() {
            return Ok(());
        }
    }
}

/// Empties the input box after a bad attempt.
async fn clear_box(
    host: &NativePtyHost,
    fd: &Arc<OwnedFd>,
    tracker: &mut ScreenTracker,
    output: &OutputBuffer,
) -> Result<(), DeliveryError> {
    host.write_to_fd(fd, CTRL_U).await?;
    let deadline = Instant::now() + Duration::from_secs(1);
    if wait_until(tracker, output, deadline, |t| {
        box_text(t).is_some_and(|s| s.is_empty())
    })
    .await
    {
        return Ok(());
    }
    let n = box_text(tracker).map_or(0, |s| s.chars().count());
    host.write_to_fd(fd, &vec![BACKSPACE; n]).await?;
    let deadline = Instant::now() + Duration::from_secs(1);
    wait_until(tracker, output, deadline, |t| {
        box_text(t).is_some_and(|s| s.is_empty())
    })
    .await;
    Ok(())
}

/// Types `text` and waits until the box shows all of it.
async fn type_text(
    host: &NativePtyHost,
    fd: &Arc<OwnedFd>,
    tracker: &mut ScreenTracker,
    output: &OutputBuffer,
    text: &str,
    opts: &DeliveryOptions,
) -> Result<(), DeliveryError> {
    let want = squash(text);
    let mut seen = 0;
    for attempt in 1..=opts.max_type_attempts {
        host.write_chunked(fd, text.as_bytes()).await?;
        let hard = Instant::now() + opts.type_timeout;
        let mut last = box_text(tracker);
        let mut last_change = Instant::now();
        loop {
            let deadline = (last_change + opts.type_stall).min(hard);
            let done = wait_until(tracker, output, deadline, |t| {
                box_text(t).as_deref() == Some(want.as_str()) || box_text(t) != last
            })
            .await;
            let now_text = box_text(tracker);
            if now_text.as_deref() == Some(want.as_str()) {
                return Ok(());
            }
            if done {
                last = now_text;
                last_change = Instant::now();
                continue;
            }
            break; // stalled or timed out
        }
        seen = box_text(tracker).map_or(0, |s| s.chars().count());
        if attempt == opts.max_type_attempts {
            break;
        }
        clear_box(host, fd, tracker, output).await?;
    }
    Err(DeliveryError::TypeMismatch {
        attempts: opts.max_type_attempts,
        seen,
        expected: want.chars().count(),
    })
}

/// Delivers `text` to the agent as one submitted turn, or says why not.
pub async fn deliver_prompt(
    host: &NativePtyHost,
    fd: &Arc<OwnedFd>,
    output: &Arc<OutputBuffer>,
    text: &str,
    opts: &DeliveryOptions,
) -> Result<(), DeliveryError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let (rows, cols) = winsize(fd);
    let mut tracker = ScreenTracker::new(output, rows, cols);
    let want = squash(text);

    wait_ready(&mut tracker, output, opts).await?;
    type_text(host, fd, &mut tracker, output, text, opts).await?;

    let submitted = |t: &ScreenTracker| {
        t.input_box().is_some_and(|b| b.is_empty()) && (t.exceeds_screen(text) || t.echoes(text))
    };

    host.write_to_fd(fd, b"\r").await?;
    let first = Instant::now() + opts.submit_retry_after;
    if wait_until(&mut tracker, output, first, submitted).await {
        return Ok(());
    }
    // Enter may have been swallowed. Press it again only while the box still
    // holds the whole text, so a delivered prompt is never submitted twice.
    if box_text(&tracker).as_deref() == Some(want.as_str()) {
        host.write_to_fd(fd, b"\r").await?;
    }
    let last = Instant::now() + opts.submit_timeout;
    if wait_until(&mut tracker, output, last, submitted).await {
        return Ok(());
    }
    if box_text(&tracker).is_some_and(|s| !s.is_empty()) {
        Err(DeliveryError::NotSubmitted)
    } else {
        Err(DeliveryError::Unconfirmed)
    }
}
