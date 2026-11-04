use super::{Failure, FailureKind};
use crate::clock::{Clock, Stamp};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

/// Attempt-local clock. Late callbacks cannot revive it or affect a replacement.
/// Inactivity counts sleep ([`Clock`]); the total budget counts awake time only.
pub(super) struct Watchdog {
    clock: Clock,
    started: Stamp,
    total: Duration,
    inactivity: Duration,
    state: Mutex<State>,
}

struct State {
    last_progress: Stamp,
    retired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Expiry {
    pub kind: FailureKind,
    pub idle_ms: u64,
    pub total_ms: u64,
    /// Sleep since the attempt started.
    pub slept_ms: u64,
    /// Inactivity expired only because sleep counted: awake idle time was shorter.
    by_sleep: bool,
}

impl Expiry {
    pub fn failure(&self) -> Failure {
        if self.by_sleep {
            Failure::slept(Duration::from_millis(self.slept_ms))
        } else {
            Failure::timeout()
        }
    }
}

impl Watchdog {
    pub fn new(total_deadline: Instant, inactivity: Duration) -> Self {
        Self::with_clock(Clock::default(), total_deadline, inactivity)
    }

    pub fn with_clock(clock: Clock, total_deadline: Instant, inactivity: Duration) -> Self {
        let started = clock.now();
        Self {
            clock,
            started,
            total: total_deadline.saturating_duration_since(Instant::now()),
            inactivity,
            state: Mutex::new(State {
                last_progress: started,
                retired: false,
            }),
        }
    }

    fn expired(&self, state: &State, now: Stamp) -> Option<Expiry> {
        let idle = now.since(state.last_progress);
        let total = now.since(self.started);
        let kind = if total.awake >= self.total {
            FailureKind::TotalDurationTimeout
        } else if idle.total >= self.inactivity {
            FailureKind::InactivityTimeout
        } else {
            return None;
        };
        Some(Expiry {
            kind,
            idle_ms: idle.total.as_millis() as u64,
            total_ms: total.total.as_millis() as u64,
            slept_ms: total.slept().as_millis() as u64,
            by_sleep: kind == FailureKind::InactivityTimeout && idle.awake < self.inactivity,
        })
    }

    pub fn expiry(&self) -> Option<Expiry> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.expired(&state, self.clock.now())
    }

    pub fn check(&self) -> Result<(), Failure> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.retired {
            Err(Failure::cancelled())
        } else if let Some(expiry) = self.expired(&state, self.clock.now()) {
            Err(expiry.failure())
        } else {
            Ok(())
        }
    }

    pub fn progress(&self) -> Result<(), Failure> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        if state.retired {
            return Err(Failure::cancelled());
        }
        if let Some(expiry) = self.expired(&state, now) {
            return Err(expiry.failure());
        }
        state.last_progress = now;
        Ok(())
    }

    pub fn event(&self, kind: &str, data: &Value) {
        // Heartbeats, usage, turn notifications and tool liveness are not work.
        let field = match kind {
            "assistant.message_delta" | "assistant.reasoning_delta" => "deltaContent",
            "assistant.message" => "content",
            "assistant.reasoning" => "content",
            _ => return,
        };
        if data[field]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty())
        {
            let _ = self.progress();
        }
    }

    pub fn retire(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).retired = true;
    }

    /// Sleep since the attempt started.
    pub fn slept(&self) -> Duration {
        self.clock.now().since(self.started).slept()
    }

    /// Any failure of an attempt that slept carries the sleep, whatever ended it,
    /// so the retry window never spends it.
    pub fn stamp(&self, mut failure: Failure) -> Failure {
        let slept = u32::try_from(self.slept().as_secs()).unwrap_or(u32::MAX);
        failure.slept_seconds = failure.slept_seconds.max(slept);
        failure
    }

    /// At most 100 ms away, so a wake is noticed promptly.
    pub fn next_check(&self) -> Instant {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        let idle = now.since(state.last_progress).total;
        let total = now.since(self.started).awake;
        Instant::now()
            + self
                .inactivity
                .saturating_sub(idle)
                .min(self.total.saturating_sub(total))
                .min(Duration::from_millis(100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Simulated;
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn independent_timer_expires_without_events_or_hung_tool_completion() {
        let clock = paused(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(10),
        );
        clock.progress().unwrap(); // Tool starts; never completes.
        let operation = super::super::tests::operation(60);
        let gate: super::super::Gate = std::sync::Arc::new(|| Ok(()));
        let failure =
            super::super::monitor_attempt(&clock, &operation, &gate, None, &Default::default())
                .await;
        assert_eq!(failure.kind, crate::monitoring::OperationFailure::Timeout);
        let expiry = clock.expiry().unwrap();
        assert_eq!(
            (
                expiry.kind,
                expiry.idle_ms,
                expiry.total_ms,
                expiry.slept_ms
            ),
            (FailureKind::InactivityTimeout, 10000, 10000, 0)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_wins_without_waiting_for_inactivity() {
        let clock = paused(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(30),
        );
        let operation = super::super::tests::operation(60);
        let gate: super::super::Gate = std::sync::Arc::new(|| Ok(()));
        let cancel = async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            operation
                .cancelled
                .store(true, std::sync::atomic::Ordering::SeqCst);
        };
        let host = super::super::HostFailure::default();
        let (failure, ()) = tokio::join!(
            super::super::monitor_attempt(&clock, &operation, &gate, None, &host),
            cancel,
        );
        assert!(failure.cancelled);
        assert!(clock.expiry().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn meaningful_progress_but_not_heartbeats_resets_idle_without_extending_total() {
        let clock = paused(
            Instant::now() + Duration::from_secs(30),
            Duration::from_secs(10),
        );
        tokio::time::advance(Duration::from_secs(8)).await;
        clock.event(
            "assistant.message_delta",
            &serde_json::json!({"deltaContent":"reviewing"}),
        );
        tokio::time::advance(Duration::from_secs(8)).await;
        assert!(clock.expiry().is_none());
        clock.event("session.heartbeat", &serde_json::json!({"content":"alive"}));
        clock.event(
            "tool.execution_progress",
            &serde_json::json!({"content":"alive"}),
        );
        tokio::time::advance(Duration::from_secs(2)).await;
        let expiry = clock.expiry().unwrap();
        assert_eq!(
            (expiry.kind, expiry.idle_ms, expiry.total_ms),
            (FailureKind::InactivityTimeout, 10000, 18000)
        );
        assert!(clock.progress().is_err());

        let clock = paused(
            Instant::now() + Duration::from_secs(15),
            Duration::from_secs(10),
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        clock.progress().unwrap();
        tokio::time::advance(Duration::from_secs(6)).await;
        let expiry = clock.expiry().unwrap();
        assert_eq!(
            (expiry.kind, expiry.idle_ms, expiry.total_ms),
            (FailureKind::TotalDurationTimeout, 6000, 15000)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retired_and_expired_attempts_reject_late_work_replacement_has_fresh_clock() {
        let old = paused(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(10),
        );
        old.retire();
        assert!(old.check().is_err());
        assert!(old.progress().is_err());
        let fresh = paused(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(10),
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        old.event("assistant.message", &serde_json::json!({"content":"late"}));
        assert!(fresh.check().is_ok());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(fresh.check().is_err());
    }

    /// Follows tokio's paused clock on every platform.
    fn paused(total_deadline: Instant, inactivity: Duration) -> Watchdog {
        Watchdog::with_clock(Clock::with(Simulated::new()), total_deadline, inactivity)
    }

    fn sleeping(inactivity: Duration) -> (Arc<Simulated>, Watchdog) {
        let source = Simulated::new();
        let clock = Watchdog::with_clock(
            Clock::with(source.clone()),
            Instant::now() + Duration::from_secs(60 * 60),
            inactivity,
        );
        (source, clock)
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_longer_than_inactivity_fails_a_silent_attempt_as_retryable_network_on_wake() {
        let (source, clock) = sleeping(Duration::from_secs(15 * 60));
        tokio::time::advance(Duration::from_secs(60)).await;
        clock.progress().unwrap();
        tokio::time::advance(Duration::from_secs(30)).await;
        // Asleep for 4h: awake time stands still, the session's connection dies.
        source.sleep(Duration::from_secs(4 * 3600));
        let operation = super::super::tests::operation(60);
        let gate: super::super::Gate = Arc::new(|| Ok(()));
        let failure = tokio::time::timeout(
            Duration::from_secs(1),
            super::super::monitor_attempt(&clock, &operation, &gate, None, &Default::default()),
        )
        .await
        .expect("noticed within 100 ms of awake time");
        assert_eq!(failure.kind, crate::monitoring::OperationFailure::Network);
        assert!(failure.kind.transient() && !failure.cancelled);
        assert_eq!(failure.slept_seconds, 4 * 3600);
        let expiry = clock.expiry().unwrap();
        assert_eq!(expiry.kind, FailureKind::InactivityTimeout);
        assert_eq!(expiry.slept_ms, 4 * 3600 * 1000);
        // The gate a tool call checks agrees with the monitor.
        assert_eq!(
            clock.check().unwrap_err().kind,
            crate::monitoring::OperationFailure::Network
        );
    }

    #[tokio::test(start_paused = true)]
    async fn short_sleep_keeps_an_active_attempt_and_awake_inactivity_stays_a_timeout() {
        let (source, clock) = sleeping(Duration::from_secs(15 * 60));
        tokio::time::advance(Duration::from_secs(60)).await;
        source.sleep(Duration::from_secs(10 * 60));
        assert!(clock.check().is_ok());
        clock.progress().unwrap();
        tokio::time::advance(Duration::from_secs(14 * 60)).await;
        assert!(clock.check().is_ok());
        tokio::time::advance(Duration::from_secs(60)).await;
        let expiry = clock.expiry().unwrap();
        assert_eq!(expiry.kind, FailureKind::InactivityTimeout);
        assert_eq!(expiry.slept_ms, 10 * 60 * 1000);
        let failure = expiry.failure();
        assert_eq!(failure.kind, crate::monitoring::OperationFailure::Timeout);
        assert_eq!(failure.slept_seconds, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn any_failure_after_a_sleep_carries_the_sleep_whatever_ended_the_attempt() {
        let (source, clock) = sleeping(Duration::from_secs(15 * 60));
        tokio::time::advance(Duration::from_secs(4 * 60)).await;
        source.sleep(Duration::from_secs(10 * 60));
        clock.progress().unwrap();
        tokio::time::advance(Duration::from_secs(3 * 60)).await;
        let failure = clock.stamp(Failure::provider("Session error."));
        assert_eq!(failure.kind, crate::monitoring::OperationFailure::Provider);
        assert_eq!(failure.slept_seconds, 10 * 60);
        assert_eq!(
            clock
                .stamp(Failure::slept(Duration::from_secs(7200)))
                .slept_seconds,
            7200
        );
    }
}
