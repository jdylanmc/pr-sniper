//! Elapsed time that keeps counting while the computer sleeps.
//!
//! `std::time::Instant` and tokio time pause during macOS sleep
//! (`mach_absolute_time`), so a deadline measured on them alone never expires
//! for work whose connection died while the machine slept. A [`Stamp`] pairs
//! monotonic time with wall-clock time: elapsed time is the larger of the two,
//! so sleep counts, and a wall clock set backwards never shortens it.

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::time::Instant;

pub(crate) trait Source: Send + Sync {
    fn monotonic(&self) -> Instant;
    fn wall(&self) -> SystemTime;
}

struct System;

impl Source for System {
    fn monotonic(&self) -> Instant {
        Instant::now()
    }
    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Injectable time source; the default reads the system clocks.
#[derive(Clone)]
pub(crate) struct Clock(Arc<dyn Source>);

impl Default for Clock {
    fn default() -> Self {
        Self(Arc::new(System))
    }
}

impl Clock {
    #[cfg(test)]
    pub(crate) fn with(source: Arc<dyn Source>) -> Self {
        Self(source)
    }

    pub(crate) fn now(&self) -> Stamp {
        Stamp {
            monotonic: self.0.monotonic(),
            wall: self.0.wall(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    monotonic: Instant,
    wall: SystemTime,
}

impl Stamp {
    pub(crate) fn monotonic(&self) -> Instant {
        self.monotonic
    }

    pub(crate) fn since(&self, earlier: Stamp) -> Elapsed {
        let awake = self.monotonic.saturating_duration_since(earlier.monotonic);
        let wall = self.wall.duration_since(earlier.wall).unwrap_or_default();
        Elapsed {
            awake,
            total: awake.max(wall),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Elapsed {
    /// Time the process could run; pauses while the computer sleeps.
    pub awake: Duration,
    /// Time including sleep.
    pub total: Duration,
}

impl Elapsed {
    pub(crate) fn slept(&self) -> Duration {
        self.total.saturating_sub(self.awake)
    }
}

/// Test source: monotonic time follows tokio's (paused) clock, and [`Self::sleep`]
/// advances only the wall clock, as a suspended computer does.
#[cfg(test)]
pub(crate) struct Simulated {
    base_monotonic: Instant,
    base_wall: SystemTime,
    slept: std::sync::Mutex<Duration>,
    wall_offset: std::sync::Mutex<Option<Duration>>,
}

#[cfg(test)]
impl Simulated {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            base_monotonic: Instant::now(),
            base_wall: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
            slept: Default::default(),
            wall_offset: Default::default(),
        })
    }

    pub(crate) fn sleep(&self, duration: Duration) {
        *self.slept.lock().unwrap() += duration;
    }

    /// Sets the wall clock back by `duration` (a manual or NTP correction).
    pub(crate) fn set_back(&self, duration: Duration) {
        *self.wall_offset.lock().unwrap() = Some(duration);
    }
}

#[cfg(test)]
impl Source for Simulated {
    fn monotonic(&self) -> Instant {
        Instant::now()
    }
    fn wall(&self) -> SystemTime {
        let wall = self.base_wall
            + Instant::now().saturating_duration_since(self.base_monotonic)
            + *self.slept.lock().unwrap();
        wall - self.wall_offset.lock().unwrap().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn sleep_counts_and_a_wall_clock_set_back_never_shortens_elapsed_time() {
        let source = Simulated::new();
        let clock = Clock::with(source.clone());
        let start = clock.now();
        tokio::time::advance(Duration::from_secs(5)).await;
        source.sleep(Duration::from_secs(3600));
        let elapsed = clock.now().since(start);
        assert_eq!(elapsed.awake, Duration::from_secs(5));
        assert_eq!(elapsed.total, Duration::from_secs(3605));
        assert_eq!(elapsed.slept(), Duration::from_secs(3600));

        source.set_back(Duration::from_secs(7200));
        tokio::time::advance(Duration::from_secs(2)).await;
        let elapsed = clock.now().since(start);
        assert_eq!(elapsed.total, Duration::from_secs(7));
        assert_eq!(elapsed.slept(), Duration::ZERO);
    }
}
