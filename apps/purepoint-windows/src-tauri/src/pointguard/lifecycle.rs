//! Port of `Services/PointGuardApplicationDelegate.swift`: Quit waits for the exact owned
//! runtime, and busy work needs a deliberate choice.
//!
//! The Mac answers `applicationShouldTerminate` with `.terminateLater`; Windows prevents the
//! main window's close (Tauri `CloseRequested`) and exits the app once the decision is made:
//! 1. Ask the bridge to stop while idle (`runtime.stop`) and wait for its exit: then quit.
//! 2. When that fails (busy, unreachable, not exiting), show the Mac's warning with
//!    "Stop Point Guard and Quit" (Yes) or Cancel (No). Yes ends the run through the lifetime
//!    pipe and quits once the child has exited.
//! 3. When even that does not end it, "Quit is paused" and the app stays open.
//!
//! Only one decision runs at a time; further close requests wait for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::service::PointGuardService;

/// The two native prompts of the quit flow. Blocking; called off the async runtime.
pub trait QuitPrompt: Send + Sync {
    /// "Point Guard is still running": true for "Stop Point Guard and Quit".
    fn confirm_stop(&self, message: &str) -> bool;
    /// "Quit is paused".
    fn paused(&self, message: &str);
}

/// The Mac's alert text, below the error.
pub const STILL_RUNNING_DETAIL: &str = "Stop Point Guard and Quit ends the current Point Guard run. Your saved conversation and drafts remain available; uncertain prompts will not be resent. Quit and updates wait until the owned runtime exits.";

/// Decide whether the app may quit now (the Swift `applicationShouldTerminate` task).
pub async fn may_terminate(service: &PointGuardService, prompt: Arc<dyn QuitPrompt>) -> bool {
    let error = match service.stop().await {
        Ok(()) => return true,
        Err(error) => error,
    };
    service.set_error(error.clone());
    let message = format!("{error}\n\n{STILL_RUNNING_DETAIL}");
    let ask = prompt.clone();
    let confirmed = tokio::task::spawn_blocking(move || ask.confirm_stop(&message))
        .await
        .unwrap_or(false);
    if !confirmed {
        return false;
    }
    match service.stop_owned_for_quit().await {
        Ok(()) => true,
        Err(error) => {
            service.set_error(error.clone());
            let _ = tokio::task::spawn_blocking(move || prompt.paused(&error)).await;
            false
        }
    }
}

/// Guards the close request: at most one decision at a time, and approval once made.
#[derive(Default)]
pub struct QuitGate {
    pending: AtomicBool,
    approved: AtomicBool,
}

impl QuitGate {
    /// Whether this close must be held back for a decision. `begin` is called only when it
    /// returns true and no decision is already running.
    pub fn intercept(&self, active: bool) -> Intercept {
        if self.approved.load(Ordering::SeqCst) || !active {
            return Intercept::Allow;
        }
        if self.pending.swap(true, Ordering::SeqCst) {
            return Intercept::Wait;
        }
        Intercept::Decide
    }

    /// Record the decision's outcome.
    pub fn finish(&self, approved: bool) {
        self.approved.store(approved, Ordering::SeqCst);
        self.pending.store(false, Ordering::SeqCst);
    }
}

/// What a close request should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intercept {
    /// Nothing to wait for: close.
    Allow,
    /// A decision is already running: keep the window.
    Wait,
    /// Keep the window and run the decision.
    Decide,
}

/// The native prompts: `MessageBoxW`, task-modal, warning icon, Cancel (No) as default.
pub struct MessageBoxPrompt;

#[cfg(windows)]
fn message_box(title: &str, text: &str, style: u32) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (title, text) = (wide(title), wide(text));
    // SAFETY: both strings are NUL-terminated and outlive the call; no owner window.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), style) }
}

impl QuitPrompt for MessageBoxPrompt {
    #[cfg(windows)]
    fn confirm_stop(&self, message: &str) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_SETFOREGROUND, MB_TASKMODAL, MB_YESNO,
        };
        let text = format!("{message}\n\nStop Point Guard and Quit?");
        message_box(
            "Point Guard is still running",
            &text,
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TASKMODAL | MB_SETFOREGROUND,
        ) == IDYES
    }
    #[cfg(windows)]
    fn paused(&self, message: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TASKMODAL,
        };
        message_box(
            "Quit is paused",
            message,
            MB_OK | MB_ICONWARNING | MB_TASKMODAL | MB_SETFOREGROUND,
        );
    }
    #[cfg(not(windows))]
    fn confirm_stop(&self, _message: &str) -> bool {
        false
    }
    #[cfg(not(windows))]
    fn paused(&self, _message: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_close_requests_should_hold_one_decision_at_a_time() {
        let gate = QuitGate::default();
        assert_eq!(gate.intercept(false), Intercept::Allow);
        assert_eq!(gate.intercept(true), Intercept::Decide);
        assert_eq!(gate.intercept(true), Intercept::Wait);
        gate.finish(false);
        assert_eq!(gate.intercept(true), Intercept::Decide);
        gate.finish(true);
        assert_eq!(gate.intercept(true), Intercept::Allow);
    }
}
