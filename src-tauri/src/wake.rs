//! Idle gate for the scheduler tick. Coordinators that only react to saved
//! state run when that state changed or when the heartbeat elapses, never on
//! every tick. A pass that writes wakes the next tick once; an idempotent
//! pass then settles, so no coordinator can keep itself or another busy.

/// Upper bound on how late a purely time-based transition (retry clock,
/// cooldown, backpressure resume) is noticed without any saved change.
pub(crate) const HEARTBEAT_SECONDS: i64 = 60;

#[derive(Debug, Default)]
pub(crate) struct Wake {
    settled: Option<(u64, i64)>,
}

impl Wake {
    /// Whether a pass is due for `generation` at `now`. Records the pass, so
    /// call it once per tick, before the coordinators run.
    pub(crate) fn due(&mut self, generation: u64, now: i64) -> bool {
        if let Some((seen, at)) = self.settled {
            if seen == generation && (at..at.saturating_add(HEARTBEAT_SECONDS)).contains(&now) {
                return false;
            }
        }
        self.settled = Some((generation, now));
        true
    }
}

#[cfg(test)]
pub(crate) mod tests;
