//! OS requests that stop the daemon gracefully, as a `Shutdown` request does.

#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};
#[cfg(windows)]
use tokio::signal::windows::{CtrlBreak, CtrlC, CtrlClose, CtrlLogoff, CtrlShutdown};

/// SIGTERM and SIGINT stop the daemon. The terminal or SSH session that
/// started it may hang up; the daemon must outlive it, so SIGHUP is handled
/// and ignored. A handler (unlike SIG_IGN) resets on exec, so agents still
/// get SIGHUP when their own PTY closes.
#[cfg(unix)]
pub(super) struct ShutdownSignals {
    term: Signal,
    int: Signal,
    hup: Signal,
}

#[cfg(unix)]
impl ShutdownSignals {
    pub(super) fn new() -> std::io::Result<Self> {
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
            hup: signal(SignalKind::hangup())?,
        })
    }

    /// Wait for a stopping signal and name it. Cancel-safe.
    pub(super) async fn recv(&mut self) -> &'static str {
        loop {
            tokio::select! {
                _ = self.term.recv() => return "SIGTERM",
                _ = self.int.recv() => return "SIGINT",
                _ = self.hup.recv() => tracing::info!("received SIGHUP, ignoring"),
            }
        }
    }
}

/// Console Ctrl-C, Ctrl-Break, close, logoff and shutdown events, when one
/// is delivered (a GUI-subsystem daemon has no console, so usually none is).
/// Closing the terminal that started a detached daemon never reaches it.
#[cfg(windows)]
pub(super) struct ShutdownSignals {
    c: CtrlC,
    brk: CtrlBreak,
    close: CtrlClose,
    logoff: CtrlLogoff,
    shutdown: CtrlShutdown,
}

#[cfg(windows)]
impl ShutdownSignals {
    pub(super) fn new() -> std::io::Result<Self> {
        use tokio::signal::windows;
        Ok(Self {
            c: windows::ctrl_c()?,
            brk: windows::ctrl_break()?,
            close: windows::ctrl_close()?,
            logoff: windows::ctrl_logoff()?,
            shutdown: windows::ctrl_shutdown()?,
        })
    }

    /// Wait for a stopping console event and name it. Cancel-safe.
    pub(super) async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.c.recv() => "Ctrl-C",
            _ = self.brk.recv() => "Ctrl-Break",
            _ = self.close.recv() => "console close",
            _ = self.logoff.recv() => "logoff",
            _ = self.shutdown.recv() => "system shutdown",
        }
    }
}
