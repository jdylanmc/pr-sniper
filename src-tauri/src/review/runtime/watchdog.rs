use super::{Failure, FailureKind};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

/// Attempt-local clock. Late callbacks cannot revive it or affect a replacement.
pub(super) struct Watchdog {
    started: Instant,
    total_deadline: Instant,
    inactivity: Duration,
    state: Mutex<State>,
}

struct State {
    last_progress: Instant,
    retired: bool,
}

impl Watchdog {
    pub fn new(total_deadline: Instant, inactivity: Duration) -> Self {
        let started = Instant::now();
        Self {
            started,
            total_deadline,
            inactivity,
            state: Mutex::new(State {
                last_progress: started,
                retired: false,
            }),
        }
    }

    fn expired(&self, state: &State, now: Instant) -> Option<FailureKind> {
        if now >= self.total_deadline {
            Some(FailureKind::TotalDurationTimeout)
        } else if now.duration_since(state.last_progress) >= self.inactivity {
            Some(FailureKind::InactivityTimeout)
        } else {
            None
        }
    }

    pub fn expiry(&self) -> Option<(FailureKind, u64, u64)> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        self.expired(&state, now).map(|kind| {
            (
                kind,
                now.duration_since(state.last_progress).as_millis() as u64,
                now.duration_since(self.started).as_millis() as u64,
            )
        })
    }

    pub fn check(&self) -> Result<(), Failure> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.retired {
            Err(Failure::cancelled())
        } else if self.expired(&state, Instant::now()).is_some() {
            Err(Failure::timeout())
        } else {
            Ok(())
        }
    }

    pub fn progress(&self) -> Result<(), Failure> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if state.retired {
            return Err(Failure::cancelled());
        }
        if self.expired(&state, now).is_some() {
            return Err(Failure::timeout());
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

    pub fn next_check(&self) -> Instant {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.last_progress + self.inactivity)
            .min(self.total_deadline)
            .min(Instant::now() + Duration::from_millis(100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn independent_timer_expires_without_events_or_hung_tool_completion() {
        let clock = Watchdog::new(
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
        assert!(matches!(
            clock.expiry(),
            Some((FailureKind::InactivityTimeout, 10000, 10000))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_wins_without_waiting_for_inactivity() {
        let clock = Watchdog::new(
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
        let clock = Watchdog::new(
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
        assert!(matches!(
            clock.expiry(),
            Some((FailureKind::InactivityTimeout, 10000, 18000))
        ));
        assert!(clock.progress().is_err());

        let clock = Watchdog::new(
            Instant::now() + Duration::from_secs(15),
            Duration::from_secs(10),
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        clock.progress().unwrap();
        tokio::time::advance(Duration::from_secs(6)).await;
        assert!(matches!(
            clock.expiry(),
            Some((FailureKind::TotalDurationTimeout, 6000, 15000))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn retired_and_expired_attempts_reject_late_work_replacement_has_fresh_clock() {
        let old = Watchdog::new(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(10),
        );
        old.retire();
        assert!(old.check().is_err());
        assert!(old.progress().is_err());
        let fresh = Watchdog::new(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(10),
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        old.event("assistant.message", &serde_json::json!({"content":"late"}));
        assert!(fresh.check().is_ok());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(fresh.check().is_err());
    }
}
