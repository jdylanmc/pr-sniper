//! Persisted PR state (`pr-state.json`): the heartbeat baseline per PR.
//!
//! The heartbeat only compares hashes. A different hash records a `pending`
//! state and bumps `state_seq`; a successful exit consumes it into a new
//! baseline. There is one record per PR, so a state that moves while queued
//! changes nothing but `pending`.

use crate::{github::state::NormalizedSnapshot, retention::Binding, storage::Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub seq: u64,
    /// Unknown until the first snapshot after a head change or reopen.
    pub hash: Option<String>,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumedExit {
    pub state_seq: u64,
    pub baseline_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrStateRecord {
    pub binding: Binding,
    pub iteration_id: String,
    #[serde(default)]
    pub snapshot: Option<NormalizedSnapshot>,
    #[serde(default)]
    pub hash: Option<String>,
    /// Hash of the last consumed state, including the host's own writes.
    #[serde(default)]
    pub baseline_hash: Option<String>,
    #[serde(default)]
    pub pending: Option<Pending>,
    #[serde(default)]
    pub state_seq: u64,
    #[serde(default)]
    pub error_count: u8,
    /// Local `ready_for_human` summary; never published.
    #[serde(default)]
    pub local_summary: Option<String>,
    #[serde(default)]
    pub exit: Option<ConsumedExit>,
    /// When a state of this PR was last consumed (unix seconds); kept across heads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrStates {
    pub records: Vec<PrStateRecord>,
}

/// What a heartbeat did to one PR's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Same as baseline or already pending.
    None,
    /// First read of a PR that was already reviewed: baseline set, no work.
    Adopted,
    /// New pending state with this `state_seq`.
    Pending(u64),
}

pub struct Observation<'a> {
    pub iteration_id: &'a str,
    /// Head change or reopen: a new iteration always needs a pass.
    pub new_iteration: bool,
    /// Absent when the poll worker read no snapshot for this PR.
    pub snapshot: Option<&'a NormalizedSnapshot>,
    /// A completed normal pass already covers this head (adopt-on-upgrade).
    pub already_reviewed: bool,
    pub now: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumed {
    Consumed,
    /// The state moved since the run read it: toss the result.
    Moved,
}

impl PrStates {
    pub fn get(&self, binding: &Binding) -> Option<&PrStateRecord> {
        self.records.iter().find(|r| &r.binding == binding)
    }

    fn entry(&mut self, binding: &Binding, iteration_id: &str) -> &mut PrStateRecord {
        let index = self
            .records
            .iter()
            .position(|r| &r.binding == binding)
            .unwrap_or_else(|| {
                self.records.push(PrStateRecord {
                    binding: binding.clone(),
                    iteration_id: iteration_id.into(),
                    snapshot: None,
                    hash: None,
                    baseline_hash: None,
                    pending: None,
                    state_seq: 0,
                    error_count: 0,
                    local_summary: None,
                    exit: None,
                    consumed_at: None,
                });
                self.records.len() - 1
            });
        &mut self.records[index]
    }

    pub fn remove(&mut self, binding: &Binding) -> bool {
        let before = self.records.len();
        self.records.retain(|r| &r.binding != binding);
        before != self.records.len()
    }

    /// Pure heartbeat step for a tracked open PR.
    pub fn observe(&mut self, binding: &Binding, observation: Observation) -> Change {
        let record = self.entry(binding, observation.iteration_id);
        record.iteration_id = observation.iteration_id.into();
        let hash = observation.snapshot.map(NormalizedSnapshot::hash);
        if let Some(snapshot) = observation.snapshot {
            record.snapshot = Some(snapshot.clone());
            record.hash = hash.clone();
        }
        if observation.new_iteration {
            record.baseline_hash = None;
            record.error_count = 0;
            record.exit = None;
            record.local_summary = None;
            return Self::bump(record, hash, observation.now);
        }
        let Some(hash) = hash else {
            return Change::None;
        };
        if record.baseline_hash.as_deref() == Some(&hash) {
            record.pending = None;
            return Change::None;
        }
        match &mut record.pending {
            Some(pending) if pending.hash.as_deref() == Some(&hash) => Change::None,
            Some(pending) if pending.hash.is_none() => {
                pending.hash = Some(hash);
                Change::None
            }
            None if record.baseline_hash.is_none()
                && record.state_seq == 0
                && observation.already_reviewed =>
            {
                record.baseline_hash = Some(hash);
                Change::Adopted
            }
            _ => Self::bump(record, Some(hash), observation.now),
        }
    }

    fn bump(record: &mut PrStateRecord, hash: Option<String>, now: i64) -> Change {
        record.state_seq += 1;
        record.pending = Some(Pending {
            seq: record.state_seq,
            hash,
            observed_at: now,
        });
        Change::Pending(record.state_seq)
    }

    /// A successful exit consumes the state it read. `new_baseline` is the
    /// hash of the state after the host's own writes, so those writes never
    /// retrigger. A state that moved meanwhile is not consumed.
    pub fn consume(&mut self, binding: &Binding, state_seq: u64, new_baseline: &str) -> Consumed {
        if state_seq != 0 && self.get(binding).is_none() {
            return Consumed::Moved;
        }
        let record = self.entry(binding, "");
        if record.state_seq != state_seq {
            return Consumed::Moved;
        }
        record.baseline_hash = Some(new_baseline.into());
        record.pending = None;
        record.error_count = 0;
        record.exit = Some(ConsumedExit {
            state_seq,
            baseline_hash: new_baseline.into(),
        });
        Consumed::Consumed
    }

    /// The run result is discarded. Returns the new error count; the caller
    /// re-queues through the `JobOperation` budget and parks at
    /// `MAX_FAILED_ATTEMPTS`.
    pub fn toss(&mut self, binding: &Binding) -> u8 {
        self.records
            .iter_mut()
            .find(|r| &r.binding == binding)
            .map_or(0, |record| {
                record.error_count = record.error_count.saturating_add(1);
                record.error_count
            })
    }

    pub fn set_local_summary(&mut self, binding: &Binding, summary: Option<String>) {
        if let Some(record) = self.records.iter_mut().find(|r| &r.binding == binding) {
            record.local_summary = summary;
        }
    }

    /// Seq a run starting now would read.
    pub fn state_seq(&self, binding: &Binding) -> u64 {
        self.get(binding).map_or(0, |r| r.state_seq)
    }
}

/// Consumes under the Store lock the caller already holds.
pub fn consume(
    store: &Store,
    binding: &Binding,
    state_seq: u64,
    new_baseline: &str,
) -> Result<Consumed, String> {
    let mut states = store.load_pr_states()?;
    let outcome = states.consume(binding, state_seq, new_baseline);
    if outcome == Consumed::Consumed {
        if let Some(record) = states.records.iter_mut().find(|r| &r.binding == binding) {
            record.consumed_at = Some(chrono::Utc::now().timestamp());
        }
        store.save_pr_states(&states)?;
    }
    Ok(outcome)
}

pub fn toss(store: &Store, binding: &Binding) -> Result<u8, String> {
    let mut states = store.load_pr_states()?;
    let count = states.toss(binding);
    store.save_pr_states(&states)?;
    Ok(count)
}

pub fn set_local_summary(
    store: &Store,
    binding: &Binding,
    summary: Option<String>,
) -> Result<(), String> {
    let mut states = store.load_pr_states()?;
    states.set_local_summary(binding, summary);
    store.save_pr_states(&states)
}

#[cfg(test)]
mod tests;
