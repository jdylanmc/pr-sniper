//! The one retry policy: exponential backoff with full jitter, capped, with a
//! bounded attempt budget. Pure and deterministic: waits are persisted as
//! timestamps and dispatched by the scheduler, so pause, cancel and quit stop
//! them without an in-process sleep. Provider-stated waits always win.
//! Non-idempotent writes are never retried through this; they reconcile first.

/// A provider-stated wait (Retry-After, rate-limit reset) is honored, bounded
/// only against absurd values.
pub const PROVIDER_CAP_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub base_seconds: i64,
    pub cap_seconds: i64,
    /// Failed attempts before the work parks.
    pub max_attempts: u8,
}

/// Transient failures: timeouts, connection loss, incomplete reads, 5xx.
pub const TRANSIENT: Policy = Policy {
    base_seconds: 5,
    cap_seconds: 5 * 60,
    max_attempts: 3,
};

/// Uniform sample source for jitter; injectable so tests are deterministic.
pub trait Jitter {
    /// A value in `0..=upper`.
    fn sample(&mut self, upper: u64) -> u64;
}

/// Jitter derived from a stable seed (an operation id) and the retry index, so
/// a restart recomputes the same wait and no global RNG is needed.
pub struct Seeded(u64);

impl Seeded {
    pub fn new(seed: &str, retry: u32) -> Self {
        // FNV-1a over the seed and retry index.
        let hash = seed
            .bytes()
            .chain(retry.to_le_bytes())
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        Self(hash)
    }
}

impl Jitter for Seeded {
    fn sample(&mut self, upper: u64) -> u64 {
        // SplitMix64 step.
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        z % upper.saturating_add(1).max(1)
    }
}

impl Policy {
    /// Upper bound for retry `retry` (0-based): `min(cap, base * 2^retry)`.
    pub fn ceiling(&self, retry: u32) -> i64 {
        self.base_seconds
            .saturating_mul(1_i64.checked_shl(retry.min(62)).unwrap_or(i64::MAX))
            .clamp(1, self.cap_seconds.max(1))
    }

    /// Full jitter: uniform in `[1, ceiling(retry)]` seconds.
    pub fn delay(&self, retry: u32, jitter: &mut impl Jitter) -> i64 {
        let ceiling = self.ceiling(retry);
        1 + jitter.sample((ceiling - 1) as u64) as i64
    }

    /// Seconds to wait before retry `retry`: the provider's stated wait when
    /// present, else the seeded full-jitter delay.
    pub fn wait(&self, seed: &str, retry: u32, provider_seconds: Option<i64>) -> i64 {
        match provider_seconds {
            Some(stated) => stated.clamp(1, PROVIDER_CAP_SECONDS),
            None => self.delay(retry, &mut Seeded::new(seed, retry)),
        }
    }

    pub fn exhausted(&self, failed_attempts: u8) -> bool {
        failed_attempts >= self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u64);
    impl Jitter for Fixed {
        fn sample(&mut self, upper: u64) -> u64 {
            self.0.min(upper)
        }
    }

    #[test]
    fn ceiling_doubles_from_base_and_stops_at_cap() {
        let ceilings: Vec<_> = (0..9).map(|n| TRANSIENT.ceiling(n)).collect();
        assert_eq!(ceilings, [5, 10, 20, 40, 80, 160, 300, 300, 300]);
        assert_eq!(TRANSIENT.ceiling(u32::MAX), 300);
    }

    #[test]
    fn full_jitter_spans_one_second_to_the_ceiling() {
        assert_eq!(TRANSIENT.delay(2, &mut Fixed(0)), 1);
        assert_eq!(TRANSIENT.delay(2, &mut Fixed(u64::MAX)), 20);
        for retry in 0..12 {
            for seed in ["a", "b", "operation-1", ""] {
                let delay = TRANSIENT.wait(seed, retry, None);
                assert!((1..=TRANSIENT.ceiling(retry)).contains(&delay));
            }
        }
    }

    #[test]
    fn seeded_jitter_is_stable_per_seed_and_spreads_across_seeds() {
        assert_eq!(
            TRANSIENT.wait("operation-1", 4, None),
            TRANSIENT.wait("operation-1", 4, None)
        );
        let spread: std::collections::BTreeSet<_> = (0..64)
            .map(|n| TRANSIENT.wait(&format!("operation-{n}"), 4, None))
            .collect();
        assert!(spread.len() > 16, "{spread:?}");
    }

    #[test]
    fn provider_stated_wait_wins_within_bounds() {
        assert_eq!(TRANSIENT.wait("x", 0, Some(90)), 90);
        assert_eq!(TRANSIENT.wait("x", 0, Some(0)), 1);
        assert_eq!(TRANSIENT.wait("x", 0, Some(-5)), 1);
        assert_eq!(TRANSIENT.wait("x", 0, Some(i64::MAX)), PROVIDER_CAP_SECONDS);
    }

    #[test]
    fn attempt_budget_is_exhausted_at_max_attempts() {
        assert!(!TRANSIENT.exhausted(2));
        assert!(TRANSIENT.exhausted(3));
    }
}
