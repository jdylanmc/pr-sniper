//! Idle and active-load gate for the scheduler tick. Each coordinator runs
//! only when a saved file it reads recorded a transition, when a deadline
//! saved in those files arrives, or when the heartbeat elapses. A pass that
//! writes wakes the next tick once; an idempotent pass then settles, so no
//! coordinator keeps itself or another busy, and progress writes of running
//! reviews (see `Store::save_reviews`) wake nobody.

use crate::storage::{Counter, Store};
use std::collections::BTreeMap;

/// Upper bound for transitions that leave no saved trace at all (in-memory
/// account or capacity state). Saved deadlines are honored on the next tick.
pub(crate) const HEARTBEAT_SECONDS: i64 = 60;

/// Queue compaction cadence after the startup pass.
pub(crate) const COMPACTION_SECONDS: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Coordinator {
    Maintenance,
    Resume,
    Capacity,
    Publication,
    ExitWrites,
    FollowUps,
    Actions,
    Notifications,
}

const OPERATIONAL: &[&str] = &["settings.json", "retention.json", "automation.json"];

impl Coordinator {
    pub(crate) const ALL: [Coordinator; 8] = [
        Coordinator::Maintenance,
        Coordinator::Resume,
        Coordinator::Capacity,
        Coordinator::Publication,
        Coordinator::ExitWrites,
        Coordinator::FollowUps,
        Coordinator::Actions,
        Coordinator::Notifications,
    ];

    /// Saved files whose transitions or deadlines can give this coordinator
    /// work, beyond `OPERATIONAL`. `None`: the whole Queue projection.
    /// `wake::tests` checks these cover every file each pass reads.
    pub(crate) fn inputs(self) -> Option<&'static [&'static str]> {
        Some(match self {
            Coordinator::Maintenance => &[],
            Coordinator::Resume => &[
                "monitoring.json",
                "reviews.json",
                "publications.json",
                "follow-ups.json",
                "feedback.json",
                "actions.json",
                "queue.json",
                "connectivity.json",
                "backpressure.json",
            ],
            Coordinator::Capacity => &[
                "reviews.json",
                "queue.json",
                "publications.json",
                "follow-ups.json",
                "feedback.json",
                "actions.json",
                "pr-state.json",
                "connectivity.json",
                "backpressure.json",
                "monitoring.json",
            ],
            Coordinator::Publication => &[
                "reviews.json",
                "queue.json",
                "publications.json",
                "follow-ups.json",
                "feedback.json",
                "pr-state.json",
                "connectivity.json",
                "backpressure.json",
            ],
            Coordinator::ExitWrites => &[
                "reviews.json",
                "publications.json",
                "follow-ups.json",
                "feedback.json",
            ],
            Coordinator::FollowUps => &[
                "reviews.json",
                "queue.json",
                "publications.json",
                "follow-ups.json",
                "feedback.json",
                "monitoring.json",
                "pr-state.json",
                "connectivity.json",
                "backpressure.json",
            ],
            Coordinator::Actions | Coordinator::Notifications => return None,
        })
    }

    fn reads(self, name: &str) -> bool {
        OPERATIONAL.contains(&name) || self.inputs().is_none_or(|inputs| inputs.contains(&name))
    }

    fn generation(self, generations: &BTreeMap<String, Counter>) -> u64 {
        generations
            .iter()
            .filter(|(name, _)| self.reads(name))
            .map(|(_, counter)| counter.transitions)
            .sum()
    }
}

#[derive(Debug, Clone, Copy)]
struct Settled {
    generation: u64,
    at: i64,
    deadline: Option<i64>,
}

#[derive(Debug, Default)]
pub(crate) struct Gates {
    settled: BTreeMap<Coordinator, Settled>,
    /// Future instants saved in each file, keyed by that file's write count.
    deadlines: BTreeMap<String, (Option<u64>, Vec<i64>)>,
    compacted_at: Option<i64>,
}

impl Gates {
    /// Coordinators whose inputs changed, whose saved deadline arrived, or
    /// whose heartbeat elapsed.
    pub(crate) fn due(
        &self,
        generations: &BTreeMap<String, Counter>,
        now: i64,
    ) -> Vec<Coordinator> {
        Coordinator::ALL
            .into_iter()
            .filter(|coordinator| match self.settled.get(coordinator) {
                None => true,
                Some(settled) => {
                    settled.generation != coordinator.generation(generations)
                        || settled.deadline.is_some_and(|deadline| now >= deadline)
                        || !(settled.at..settled.at.saturating_add(HEARTBEAT_SECONDS))
                            .contains(&now)
                }
            })
            .collect()
    }

    /// Queue compaction runs at most hourly; the first pass compacts.
    pub(crate) fn compaction_due(&mut self, now: i64) -> bool {
        let due = self
            .compacted_at
            .is_none_or(|at| !(at..at.saturating_add(COMPACTION_SECONDS)).contains(&now));
        if due {
            self.compacted_at = Some(now);
        }
        due
    }

    /// Records the passes that ran against `generations` (sampled before
    /// they ran) and the next saved deadline each one must wake for, read
    /// from `view`, a frozen copy taken after sampling.
    pub(crate) fn settle(
        &mut self,
        ran: &[Coordinator],
        generations: &BTreeMap<String, Counter>,
        view: &Store,
        now: i64,
    ) {
        // Probe cadence is not work; polls already wake through their writes.
        for name in view
            .frozen_state_files()
            .into_iter()
            .filter(|name| *name != "connectivity.json")
        {
            // All writes go through this Store, so an unwritten file is unchanged.
            let writes = generations.get(name).map(|c| c.writes);
            let stale = self
                .deadlines
                .get(name)
                .is_none_or(|(seen, _)| *seen != writes);
            if stale {
                let instants = view.saved_instants(name, now);
                self.deadlines.insert(name.to_string(), (writes, instants));
            }
        }
        for &coordinator in ran {
            let deadline = self
                .deadlines
                .iter()
                .filter(|(name, _)| coordinator.reads(name))
                .filter_map(|(_, (_, instants))| instants.iter().copied().find(|&at| at > now))
                .min();
            self.settled.insert(
                coordinator,
                Settled {
                    generation: coordinator.generation(generations),
                    at: now,
                    deadline,
                },
            );
        }
    }
}

/// Sorted future values of saved instant fields (`*_at`, `*until`,
/// `*deadline`) in a JSON document. A false match only wakes a pass early;
/// a missed one would delay due work, so matching is deliberately loose.
pub(crate) fn future_instants(bytes: &[u8], now: i64) -> Vec<i64> {
    const KEYS: [&[u8]; 3] = [b"_at", b"until", b"deadline"];
    let mut instants = Vec::new();
    for colon in memchr_iter(b':', bytes) {
        if colon == 0 || bytes[colon - 1] != b'"' {
            continue;
        }
        let key_end = colon - 1;
        let start = bytes[..key_end]
            .iter()
            .rposition(|&b| b == b'"')
            .unwrap_or(0);
        let key = &bytes[start..key_end];
        if key.len() > 64 || !KEYS.iter().any(|suffix| key.ends_with(suffix)) {
            continue;
        }
        let digits: Vec<u8> = bytes[colon + 1..]
            .iter()
            .copied()
            .skip_while(|b| *b == b' ')
            .take_while(u8::is_ascii_digit)
            .take(19)
            .collect();
        if let Some(value) = std::str::from_utf8(&digits)
            .ok()
            .and_then(|digits| digits.parse::<i64>().ok())
            .filter(|&value| value > now)
        {
            instants.push(value);
        }
    }
    instants.sort_unstable();
    instants.dedup();
    instants
}

fn memchr_iter(needle: u8, bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    bytes
        .iter()
        .enumerate()
        .filter(move |(_, &b)| b == needle)
        .map(|(i, _)| i)
}

#[cfg(test)]
pub(crate) mod tests;
