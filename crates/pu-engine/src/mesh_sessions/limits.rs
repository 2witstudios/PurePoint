use super::{Error, Result};
use pu_core::mesh::wire;
use std::time::Duration;

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
        {
            return Err(Error::Limits);
        }
        Ok(())
    }
}
