use super::{Error, Result};
use crate::mesh_transport::HANDSHAKE_DEADLINE;
use pu_core::mesh::wire;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr},
    sync::{Arc, Mutex},
    time::Duration,
};

/// Small bounded defaults are configuration, not tested fleet capacity.
#[derive(Clone, Debug)]
pub struct SessionLimits {
    pub control_total: usize,
    pub bulk_total: usize,
    pub control_per_peer: usize,
    pub bulk_per_peer: usize,
    pub subscriptions_total: usize,
    pub subscriptions_per_peer: usize,
    pub control_frame: usize,
    pub bulk_frame: usize,
    pub output_chunk: usize,
    pub send_deadline: Duration,
    pub read_deadline: Duration,
    /// Concurrent UNAUTHENTICATED TLS handshakes across all sources. Separate
    /// from the session budgets above, which are reserved only after the exact
    /// enrolled certificate pin is verified, so a LAN flood cannot hold them.
    pub preauth_total: usize,
    /// Concurrent unauthenticated handshakes from one source address (an IPv6
    /// source is keyed by its /64), so one host cannot take the whole budget.
    pub preauth_per_source: usize,
    /// Server-side deadline for an unauthenticated peer to complete TLS and the
    /// pin check; never above the transport's overall `HANDSHAKE_DEADLINE`.
    pub handshake_deadline: Duration,
}
impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            control_total: 8,
            bulk_total: 8,
            control_per_peer: 2,
            bulk_per_peer: 2,
            subscriptions_total: 16,
            subscriptions_per_peer: 4,
            control_frame: wire::MAX_CONTROL_FRAME,
            bulk_frame: wire::MAX_BULK_FRAME,
            output_chunk: 32 * 1024,
            send_deadline: Duration::from_millis(250),
            read_deadline: Duration::from_secs(5),
            preauth_total: 32,
            preauth_per_source: 4,
            handshake_deadline: Duration::from_secs(1),
        }
    }
}
impl SessionLimits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=64).contains(&self.control_total)
            || !(1..=64).contains(&self.bulk_total)
            || self.control_per_peer == 0
            || self.control_per_peer > self.control_total
            || self.bulk_per_peer == 0
            || self.bulk_per_peer > self.bulk_total
            || !(1..=256).contains(&self.subscriptions_total)
            || self.subscriptions_per_peer == 0
            || self.subscriptions_per_peer > self.subscriptions_total
            || !(2048..=wire::MAX_CONTROL_FRAME).contains(&self.control_frame)
            || !(2048..=wire::MAX_BULK_FRAME).contains(&self.bulk_frame)
            || !(1..=32 * 1024).contains(&self.output_chunk)
            || self.output_chunk > self.bulk_frame
            || self.send_deadline.is_zero()
            || self.send_deadline > Duration::from_secs(1)
            || self.read_deadline.is_zero()
            || self.read_deadline > Duration::from_secs(30)
            || !(1..=256).contains(&self.preauth_total)
            || self.preauth_per_source == 0
            || self.preauth_per_source > self.preauth_total
            || self.handshake_deadline.is_zero()
            || self.handshake_deadline > HANDSHAKE_DEADLINE
        {
            return Err(Error::Limits);
        }
        Ok(())
    }
}

/// Receiver-owned budget for UNAUTHENTICATED handshakes, shared across
/// listener reloads like the session permits. Bounded total plus a per-source
/// share; holds no session permit, so it can never starve an enrolled peer of
/// one. The registry mutex is never held across an await.
pub(super) struct PreAuthGate {
    total: usize,
    per_source: usize,
    state: Mutex<PreAuthState>,
}
#[derive(Default)]
struct PreAuthState {
    held: usize,
    sources: HashMap<IpAddr, usize>,
}
impl PreAuthGate {
    pub(super) fn new(limits: &SessionLimits) -> Arc<Self> {
        Arc::new(Self {
            total: limits.preauth_total,
            per_source: limits.preauth_per_source,
            state: Mutex::default(),
        })
    }
    pub(super) fn admit(self: &Arc<Self>, source: Option<IpAddr>) -> Result<PreAuth> {
        let source = source.map(source_key);
        let mut state = self.state.lock().map_err(|_| Error::Operation)?;
        if state.held >= self.total
            || source
                .is_some_and(|key| state.sources.get(&key).copied().unwrap_or(0) >= self.per_source)
        {
            return Err(Error::Capacity);
        }
        state.held += 1;
        if let Some(key) = source {
            *state.sources.entry(key).or_default() += 1;
        }
        Ok(PreAuth {
            gate: self.clone(),
            source,
        })
    }
}

/// One admitted unauthenticated handshake; released on drop, at the latest
/// when the session authority has reserved a session permit for the peer.
pub struct PreAuth {
    gate: Arc<PreAuthGate>,
    source: Option<IpAddr>,
}
impl PreAuth {
    pub(super) fn issued_by(&self, gate: &Arc<PreAuthGate>) -> bool {
        Arc::ptr_eq(&self.gate, gate)
    }
}
impl Drop for PreAuth {
    fn drop(&mut self) {
        if let Ok(mut state) = self.gate.state.lock() {
            state.held -= 1;
            if let Some(key) = self.source
                && let Some(count) = state.sources.get_mut(&key)
            {
                *count -= 1;
                if *count == 0 {
                    state.sources.remove(&key);
                }
            }
        }
    }
}

/// IPv4 (including IPv4-mapped IPv6) per address; native IPv6 per /64, since
/// one host can trivially rotate addresses within its own prefix.
fn source_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let prefix = u128::from(v6) & !((1u128 << 64) - 1);
            IpAddr::V6(Ipv6Addr::from(prefix))
        }
        v4 => v4,
    }
}
