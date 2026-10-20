//! Local queue visibility only; never consumes PR state or changes review evidence.
use super::{Item, Snapshot, State};
use crate::{monitoring::OperationState, retention::Binding, storage::Store};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::ErrorKind};

const FILE: &str = "queue-dismissals.json";

fn load(store: &Store) -> Result<BTreeMap<String, String>, String> {
    match store.read_state(FILE) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Queue dismissals are invalid.".into())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(_) => Err("Cannot read queue dismissals.".into()),
    }
}

fn token(item: &Item, snapshot: &Snapshot) -> Result<String, String> {
    let binding = Binding::job(&item.job);
    let current = snapshot.pr_states.get(&binding);
    let runs: BTreeMap<_, _> = snapshot
        .reviews
        .iter()
        .filter(|r| item.review_keys.contains(&r.key))
        .map(|r| (&r.key, r.run.as_ref().map(|run| &run.operation.id)))
        .collect();
    let bytes = serde_json::to_vec(&(
        &binding,
        &item.id,
        item.state,
        &item.job.head_sha,
        &item.job.observed_base_sha,
        &item.job.waiting,
        item.job
            .work
            .as_ref()
            .map(|w| (&w.iteration_id, w.state_seq)),
        current.map(|r| (&r.iteration_id, r.state_seq, &r.hash)),
        runs,
    ))
    .map_err(|_| "Cannot identify stale review state.")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn unsettled(item: &Item, snapshot: &Snapshot, actions: &crate::actions::Ledger) -> bool {
    let binding = Binding::job(&item.job);
    item.progress.is_some()
        || snapshot.reviews.iter().any(|r| {
            binding.matches(&r.job)
                && r.run
                    .as_ref()
                    .is_some_and(|run| run.operation.state == OperationState::Running)
        })
        || snapshot
            .publications
            .iter()
            .filter_map(|p| p.publication.as_ref())
            .any(|p| {
                binding.matches(&p.review.job)
                    && (p.uncertain
                        || p.operation.state == OperationState::Running
                        || matches!(
                            p.phase,
                            crate::publication::Phase::Preparing
                                | crate::publication::Phase::Pending
                                | crate::publication::Phase::Stopped
                                | crate::publication::Phase::Unresolved
                        ))
            })
        || snapshot.follow_ups.iter().any(|f| {
            binding.matches(&f.run.context.job)
                && (f.run.uncertain
                    || [&f.run.analysis, &f.run.publication]
                        .into_iter()
                        .flatten()
                        .any(|operation| operation.state == OperationState::Running)
                    || matches!(
                        f.run.phase,
                        crate::follow_up::Phase::Analyzing
                            | crate::follow_up::Phase::WaitingPublication
                            | crate::follow_up::Phase::Publishing
                            | crate::follow_up::Phase::Unresolved
                    ))
        })
        || snapshot.exit_writes.iter().any(|w| {
            binding.matches(&w.job)
                && !matches!(
                    w.phase,
                    crate::exit_writes::Phase::Completed | crate::exit_writes::Phase::Tossed
                )
        })
        || actions.effects.iter().any(|e| {
            let op = &e.operation;
            op.provider == item.job.provider
                && op.account_id == item.job.account_id
                && op.configuration_id == item.job.configuration_id
                && op.repository_id == item.job.repository_id
                && op.pull_request_id.as_deref() == Some(item.job.pull_request_id.as_str())
                && (e.operation.state == OperationState::Running
                    || e.state == crate::actions::EffectState::Prepared
                    || e.needs_reconciliation())
        })
        || actions.finals.iter().any(|f| {
            binding.matches(&f.basis.job) && f.execution.operation.state == OperationState::Running
        })
}

pub(super) fn project(store: &Store, snapshot: &mut Snapshot) -> Result<(), String> {
    let dismissed = load(store)?;
    let actions = store.load_actions()?;
    let tokens = snapshot
        .items
        .iter()
        .map(|item| {
            if matches!(item.state, State::Stale | State::StaleAfterPublication)
                && !unsettled(item, snapshot, &actions)
            {
                token(item, snapshot).map(Some)
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    for (item, token) in snapshot.items.iter_mut().zip(tokens) {
        item.dismissed = token
            .as_ref()
            .is_some_and(|value| dismissed.get(&item.id) == Some(value));
        item.dismiss_token = token;
    }
    Ok(())
}

/// Caller holds the Store lock through validation and atomic persistence.
pub fn dismiss(store: &Store, item_id: &str, expected_token: &str) -> Result<(), String> {
    let snapshot = super::snapshot(store, Vec::new())?;
    let item = snapshot
        .items
        .iter()
        .find(|item| item.id == item_id)
        .ok_or("Stale review is unavailable.")?;
    if item.dismiss_token.as_deref() != Some(expected_token) {
        return Err("Stale review changed or has unsettled work.".into());
    }
    let mut dismissed = load(store)?;
    dismissed.insert(item.id.clone(), expected_token.into());
    store.write_state(FILE, &dismissed)
}
