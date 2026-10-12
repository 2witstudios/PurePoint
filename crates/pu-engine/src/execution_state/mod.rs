//! Durable worker-local authority and admission, independent of transports and PTYs.
//!
//! Share one `Arc<Ledger>` per daemon and call its synchronous methods via
//! `spawn_blocking`. The engine must authenticate/authorize every entry, including
//! receipt reads. A `Caller` is trusted ingress context, never a request field.
//!
//! Lock order: **authority barrier -> per-agent command boundary -> ledger
//! transaction**. Hold the shared authority barrier and the per-agent boundary
//! from authorization/validation through the actual dispatch/write/transition.
//! Peer grants also require durable canonical policy bytes matching the ledger's
//! authorization digest. Registration starts without an anchor and denies use.
//! Policy changes persist bytes first, then update the ledger anchor/generation
//! under the exclusive authority barrier. A mismatch fails closed. Restart keeps
//! the anchor/revision, so a new generation cannot bless stale or mismatched policy.
//! Revocation, host changes and takeover take the exclusive authority barrier,
//! then fence durably. Never wait for a network or PTY inside a SQLite transaction.
//! `validate_dispatch` alone does not serialize a side effect: the engine must
//! retain the outer barriers and must not retain stale raw writable descriptors.
//!
//! Fresh launch acceptance permits one `begin_start` attempt; receipt replay and
//! unresolved recovery never permit another launch. All local, scheduled and
//! remote launch paths must use this ledger. The engine verifies exact source,
//! runtime and export before admission and supplies explicit supervisor evidence
//! for resolution. This module offers no OS process-tree lifetime guarantee.
//!
//! Reopen fences writers, changes outstanding accepted input to Unknown, and
//! marks live runs Unknown without releasing slots. The engine reconciles those
//! records before resuming. It must never adopt/kill a process from a bare PID,
//! or automatically resend uncertain input under a new writer/operation identity.
//!
//! Reopen closes all operation windows, advances active peer generations and clears
//! peer controllers. Reconnecting peers must obtain the current receiver generation
//! and explicitly acquire control. Retained receipts remain read-only evidence.
//! Backup rollback/restore under the same daemon/trust identity is unsupported:
//! quarantine restored state, reconcile every potentially live run explicitly, and
//! re-enroll under a fresh authority identity. Copying an old database cannot safely
//! rewind replay fences; this is not an anti-rollback hardware store.

mod authority;
mod db;
mod error;
mod input;
mod launch;
mod operations;
mod types;
mod validation;

pub use db::Ledger;
pub use error::{CapacityScope, Error, Result};
pub use types::*;

#[cfg(test)]
mod tests;
