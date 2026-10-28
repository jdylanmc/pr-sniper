//! Bulk Retry for Needs attention: one plan for the Queue pill and control.
//! It re-arms through the per-item Retry paths and leaves human decisions
//! (permanent failures, user cancellation, panic exits, provider actions)
//! parked.
use super::{Item, Snapshot, State, CAUSE_EXIT_FAILED, CAUSE_PANIC, CAUSE_TOSSED};
use crate::{
    monitoring::{FailureClass, OperationFailure, OperationState, ScheduleHealth},
    publication::{Phase, RemoteState},
    storage::Store,
};
use std::collections::BTreeSet;

/// What Retry all re-arms for one failed item.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// Parked toss or failed exit write: restart the PR's toss budget.
    pub packet: bool,
    pub reviews: Vec<String>,
    /// Review operation ids whose parked publication reconciles.
    pub publications: Vec<String>,
    /// Parked repository poll operations.
    pub polls: Vec<String>,
}

impl Recovery {
    pub fn is_empty(&self) -> bool {
        !self.packet
            && self.reviews.is_empty()
            && self.publications.is_empty()
            && self.polls.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct Plan {
    pub items: Vec<(String, Recovery)>,
    /// Failed items left parked for a human.
    pub skipped: usize,
}

const PARKED: [OperationState; 2] = [OperationState::Failed, OperationState::ManualRetry];

fn human(failure: &Option<OperationFailure>) -> bool {
    matches!(
        failure,
        Some(OperationFailure::Permanent | OperationFailure::Panic)
    )
}

pub(crate) fn recovery(item: &Item, snapshot: &Snapshot, paused: bool) -> Recovery {
    let mut recovery = Recovery::default();
    if item.state != State::Failed
        || item.failure_cause == Some(CAUSE_PANIC)
        || item
            .action_status
            .as_ref()
            .is_some_and(|status| status.effects.iter().any(|e| e.needs_reconciliation()))
    {
        return recovery;
    }
    if item.failure_class != Some(FailureClass::Credentials)
        && matches!(item.failure_cause, Some(CAUSE_TOSSED | CAUSE_EXIT_FAILED))
    {
        recovery.packet = true;
        return recovery;
    }
    // A parked repository poll retries too, so reviews restart on fresh data.
    recovery.polls = snapshot
        .health
        .iter()
        .filter(|health| {
            health.repository_id == item.job.configuration_id
                && health.provider_account_id.as_deref() == Some(item.job.account_id.as_str())
        })
        .filter_map(|health| health.operation.as_ref())
        .filter(|operation| PARKED.contains(&operation.state) && !human(&operation.failure))
        .map(|operation| operation.id.clone())
        .collect();
    let runs: Vec<_> = snapshot
        .reviews
        .iter()
        .filter(|candidate| item.review_keys.contains(&candidate.key))
        .filter_map(|candidate| candidate.run.as_ref().map(|run| (&candidate.key, run)))
        .collect();
    // A revision is reviewed once: only a run that itself failed, with
    // nothing published or pending for it, starts again.
    for (key, run) in &runs {
        if PARKED.contains(&run.operation.state)
            && !human(&run.operation.failure)
            && run.result.is_none()
            && !snapshot
                .publications
                .iter()
                .any(|p| p.publication.is_some() && p.review_operation_id == run.operation.id)
        {
            recovery.reviews.push((*key).clone());
        }
    }
    // Publications refuse to start while paused; they wait for a resume.
    if !paused {
        let ids: BTreeSet<_> = runs.iter().map(|(_, run)| &run.operation.id).collect();
        recovery.publications = snapshot
            .publications
            .iter()
            .filter(|candidate| ids.contains(&candidate.review_operation_id))
            .filter(|candidate| {
                // A withdrawn batch never re-arms: manual retry would un-cancel it.
                candidate.publication.as_ref().is_some_and(|p| {
                    !p.cancelled
                        && !matches!(p.phase, Phase::Discarded | Phase::Stale)
                        && p.receipts
                            .last()
                            .is_none_or(|r| r.state != RemoteState::Deleted)
                        && p.operation.state != OperationState::Completed
                        && (p.uncertain || PARKED.contains(&p.operation.state))
                        && !human(&p.operation.failure)
                })
            })
            .map(|candidate| candidate.review_operation_id.clone())
            .collect();
    }
    recovery
}

/// Marks the items Retry all would re-arm (`Item::retry_all`).
pub(crate) fn project(store: &Store, snapshot: &mut Snapshot) -> Result<(), String> {
    let paused = store.load_automation()?.paused;
    let flags: Vec<bool> = snapshot
        .items
        .iter()
        .map(|item| !recovery(item, snapshot, paused).is_empty())
        .collect();
    for (item, flag) in snapshot.items.iter_mut().zip(flags) {
        item.retry_all = flag;
    }
    Ok(())
}

/// Every failed item's recovery; `health` is the live monitor's (empty: saved).
pub fn plan(store: &Store, health: Vec<ScheduleHealth>) -> Result<Plan, String> {
    let paused = store.load_automation()?.paused;
    let snapshot = super::snapshot(store, health)?;
    let mut plan = Plan::default();
    for item in snapshot.items.iter().filter(|i| i.state == State::Failed) {
        let recovery = recovery(item, &snapshot, paused);
        if recovery.is_empty() {
            plan.skipped += 1;
        } else {
            plan.items.push((item.id.clone(), recovery));
        }
    }
    Ok(plan)
}

/// Re-arms the plan's packet states and review runs in `store`; returns the
/// items with at least one re-armed recovery. Dispatch keeps pause, capacity
/// and backpressure gates, so paused work waits queued for a resume.
pub fn rearm(store: &Store, plan: &Plan, now: i64) -> BTreeSet<String> {
    let mut done = BTreeSet::new();
    for (id, recovery) in &plan.items {
        let mut ok = recovery.packet && crate::exits::consume::retry_parked(store, id, now).is_ok();
        for key in &recovery.reviews {
            ok |= crate::review::host::request(store, key, true, now).is_ok();
        }
        if ok {
            done.insert(id.clone());
        }
    }
    done
}

#[cfg(test)]
mod tests;
