//! Accepted effects outlive transport cancellation. Only transport waiters are
//! aborted; operation/service tasks keep awaiting their blocking work and carry
//! singleton ownership until their actual outcome is recorded.
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::{Notify, watch};
use tokio::task::{JoinError, JoinHandle};
use tracing::Instrument;

use super::DaemonLock;

tokio::task_local! {
    static CURRENT_WORK: WorkLease;
}

/// Shutdown/drain handle. A timeout reports pending work and never cancels it.
/// Idle engine handles do not retain the lock; each running effect/service does.
pub struct WorkOwner {
    lifecycle: Weak<DaemonLock>,
    next_id: AtomicU64,
    pending: AtomicUsize,
    requests: AtomicUsize,
    completed: AtomicU64,
    changed: Notify,
    stopping: watch::Sender<bool>,
}

struct WorkLease {
    owner: Arc<WorkOwner>,
    lifecycle: Option<Arc<DaemonLock>>,
    id: u64,
    finished: std::cell::Cell<bool>,
    request: bool,
}

/// Waiter only: dropping it detaches the accepted work. No abort handle escapes.
pub(crate) struct WorkHandle<T> {
    join: JoinHandle<T>,
}

impl<T> Future for WorkHandle<T> {
    type Output = Result<T, JoinError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().join).poll(cx)
    }
}

impl<T> WorkHandle<T> {
    #[cfg(test)]
    pub(crate) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }
}

impl WorkOwner {
    pub(crate) fn new(lifecycle: &Arc<DaemonLock>) -> Arc<Self> {
        Arc::new(Self {
            lifecycle: Arc::downgrade(lifecycle),
            next_id: AtomicU64::new(1),
            pending: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            completed: AtomicU64::new(0),
            changed: Notify::new(),
            stopping: watch::channel(false).0,
        })
    }

    /// Stop new admissions and wake idle services. Already accepted chains finish.
    pub fn begin_shutdown(&self) {
        self.stopping.send_replace(true);
    }

    pub(crate) fn is_stopping(&self) -> bool {
        *self.stopping.borrow()
    }

    pub(crate) async fn stopping(&self) {
        let mut receiver = self.stopping.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }

    /// Await actual completion, including descendants started by accepted work.
    pub async fn wait_idle(&self) {
        self.wait_pending(false).await;
    }

    /// Await admitted request chains and current scheduler ticks before agent
    /// teardown. Exit observers are drained after the agent shutdown phase.
    pub async fn wait_requests(&self) {
        self.wait_pending(true).await;
    }

    async fn wait_pending(&self, requests: bool) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let pending = if requests {
                &self.requests
            } else {
                &self.pending
            };
            if pending.load(Ordering::Acquire) == 0 {
                return;
            }
            changed.await;
        }
    }

    /// Bound the wait, preserving the singleton and files on timeout.
    pub async fn drain_for(&self, duration: Duration) -> io::Result<()> {
        tokio::time::timeout(duration, self.wait_idle())
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "{} accepted effects/services still pending; singleton ownership retained",
                        self.pending.load(Ordering::Acquire)
                    ),
                )
            })
    }

    pub(crate) async fn drain_requests_for(&self, duration: Duration) -> io::Result<()> {
        tokio::time::timeout(duration, self.wait_requests())
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "{} accepted requests/services still pending; singleton ownership retained",
                        self.requests.load(Ordering::Acquire)
                    ),
                )
            })
    }

    pub(crate) fn spawn<F, T>(self: &Arc<Self>, future: F) -> io::Result<WorkHandle<T>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.spawn_inner(future, false)
    }

    fn spawn_inner<F, T>(self: &Arc<Self>, future: F, descendant: bool) -> io::Result<WorkHandle<T>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        if !descendant && self.is_stopping() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "daemon is stopping",
            ));
        }
        let lifecycle = self
            .lifecycle
            .upgrade()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "daemon ownership ended"))?;
        lifecycle.active_work.fetch_add(1, Ordering::AcqRel);
        self.pending.fetch_add(1, Ordering::AcqRel);
        if !descendant {
            self.requests.fetch_add(1, Ordering::AcqRel);
        }
        let lease = WorkLease {
            owner: self.clone(),
            lifecycle: Some(lifecycle),
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            finished: std::cell::Cell::new(false),
            request: !descendant,
        };
        // Dropping the returned handle detaches the waiter; no abort handle is
        // put in the connection JoinSet. The operation itself runs to completion.
        let span = tracing::info_span!("accepted_work", work_id = lease.id);
        Ok(WorkHandle {
            join: tokio::spawn(
                CURRENT_WORK
                    .scope(lease, async move {
                        let outcome = future.await;
                        CURRENT_WORK.with(|lease| lease.finished.set(true));
                        outcome
                    })
                    .instrument(span),
            ),
        })
    }

    #[cfg(test)]
    pub(crate) fn completed(&self) -> u64 {
        self.completed.load(Ordering::Acquire)
    }
}

impl Drop for WorkLease {
    fn drop(&mut self) {
        if self.finished.get() {
            self.owner.completed.fetch_add(1, Ordering::Release);
            tracing::debug!(work_id = self.id, "accepted effect/service completed");
        } else {
            tracing::error!(
                work_id = self.id,
                "accepted effect/service ended without an outcome"
            );
        }
        let lifecycle = self
            .lifecycle
            .take()
            .expect("work lease owns its lifecycle guard");
        lifecycle.active_work.fetch_sub(1, Ordering::AcqRel);
        // Finish guard cleanup/release before publishing idle to another task.
        // Merely decrementing the counter before the Arc field drops lets a
        // waiter observe idle while the last singleton guard is still dropping.
        drop(lifecycle);
        if self.request {
            self.owner.requests.fetch_sub(1, Ordering::AcqRel);
        }
        self.owner.pending.fetch_sub(1, Ordering::AcqRel);
        self.owner.changed.notify_waiters();
    }
}

/// Propagate ownership to detached descendants of an accepted operation/tick.
/// Unbound in-memory engines retain their existing embedding behavior.
pub(crate) fn spawn_inherited<F, T>(future: F) -> WorkHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    match CURRENT_WORK.try_with(|lease| lease.owner.clone()) {
        Ok(owner) => owner
            .spawn_inner(future, true)
            .expect("parent lease retains daemon ownership"),
        Err(_) => WorkHandle {
            join: tokio::spawn(future),
        },
    }
}
