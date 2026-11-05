//! Elapsed time that keeps counting while the computer sleeps.
//!
//! A [`Stamp`] pairs awake time (pauses during sleep) with wall-clock time.
//! Elapsed time is the larger of the two, so sleep counts, and a wall clock set
//! backwards never shortens it; their difference is the sleep. `Instant` alone
//! is not portable for either: it pauses during macOS and Linux sleep
//! (`mach_absolute_time`, `CLOCK_MONOTONIC`) but keeps counting on Windows
//! (`QueryPerformanceCounter`), so Windows reads unbiased interrupt time.

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

pub(crate) trait Source: Send + Sync {
    /// Awake time since an arbitrary fixed origin.
    fn awake(&self) -> Duration;
    fn wall(&self) -> SystemTime;
}

struct System {
    #[cfg(not(windows))]
    origin: tokio::time::Instant,
}

impl Source for System {
    #[cfg(not(windows))]
    fn awake(&self) -> Duration {
        self.origin.elapsed()
    }

    #[cfg(windows)]
    fn awake(&self) -> Duration {
        let mut ticks = 0u64;
        // SAFETY: writes one u64 through a valid pointer; cannot fail on Windows 7+.
        unsafe {
            windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime(&mut ticks)
        };
        Duration::from_nanos(ticks.saturating_mul(100))
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Injectable time source; the default reads the system clocks. Compare only
/// stamps from the same `Clock`: awake time has a per-clock origin.
#[derive(Clone)]
pub(crate) struct Clock(Arc<dyn Source>);

impl Default for Clock {
    fn default() -> Self {
        Self(Arc::new(System {
            #[cfg(not(windows))]
            origin: tokio::time::Instant::now(),
        }))
    }
}

impl Clock {
    #[cfg(test)]
    pub(crate) fn with(source: Arc<dyn Source>) -> Self {
        Self(source)
    }

    pub(crate) fn now(&self) -> Stamp {
        Stamp {
            awake: self.0.awake(),
            wall: self.0.wall(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    awake: Duration,
    wall: SystemTime,
}

impl Stamp {
    pub(crate) fn since(&self, earlier: Stamp) -> Elapsed {
        let awake = self.awake.saturating_sub(earlier.awake);
        let wall = self.wall.duration_since(earlier.wall).unwrap_or_default();
        Elapsed {
            awake,
            total: awake.max(wall),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Elapsed {
    /// Time the computer was awake.
    pub awake: Duration,
    /// Time including sleep.
    pub total: Duration,
}

impl Elapsed {
    pub(crate) fn slept(&self) -> Duration {
        self.total.saturating_sub(self.awake)
    }
}

/// Test source: awake time follows tokio's (paused) clock, and [`Self::sleep`]
/// advances only the wall clock, as a suspended computer does.
#[cfg(test)]
pub(crate) struct Simulated {
    base: tokio::time::Instant,
    base_wall: SystemTime,
    slept: std::sync::Mutex<Duration>,
    wall_offset: std::sync::Mutex<Duration>,
}

#[cfg(test)]
impl Simulated {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            base: tokio::time::Instant::now(),
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
        *self.wall_offset.lock().unwrap() = duration;
    }
}

#[cfg(test)]
impl Source for Simulated {
    fn awake(&self) -> Duration {
        self.base.elapsed()
    }
    fn wall(&self) -> SystemTime {
        self.base_wall + self.base.elapsed() + *self.slept.lock().unwrap()
            - *self.wall_offset.lock().unwrap()
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

    #[test]
    fn the_system_source_advances_awake_and_wall_time() {
        let clock = Clock::default();
        let start = clock.now();
        std::thread::sleep(Duration::from_millis(50));
        let elapsed = clock.now().since(start);
        assert!(elapsed.awake >= Duration::from_millis(20), "{elapsed:?}");
        assert!(elapsed.total >= elapsed.awake);
    }
}
