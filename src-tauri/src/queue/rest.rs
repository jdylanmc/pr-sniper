//! Why a stale item rests outside Needs attention. A stale review asks
//! nothing of the human: the PR's next change starts a new Iteration, and a
//! closed PR, removed repository or unbound account retires it. Projection
//! only; dismissals, history and receipts are untouched.
use super::{Item, Snapshot, State};
use crate::{monitoring, provider::Lifecycle, retention::Binding, storage::Settings};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rest {
    /// A newer Iteration of the same PR exists.
    Superseded,
    /// Waiting for the PR to change (new head or meaningful state).
    AwaitingChange,
    Closed,
    Merged,
    RepositoryRemoved,
    RepositoryDisabled,
    AccountUnbound,
    /// The PR no longer passes admission or its path gate.
    NotAdmitted,
}

fn iteration(item: &Item) -> Option<u64> {
    item.job.work.as_ref().map(|w| w.iteration)
}

fn classify(settings: &Settings, snapshot: &Snapshot, item: &Item) -> Rest {
    let job = &item.job;
    let binding = Binding::job(job);
    let lifecycle = |waiting: &str| match waiting {
        monitoring::WAITING_MERGED => Some(Rest::Merged),
        monitoring::WAITING_CLOSED => Some(Rest::Closed),
        _ => None,
    };
    let tracked = snapshot
        .tracked
        .iter()
        .filter(|pr| Binding::tracked(pr) == binding);
    if let Some(rest) = snapshot
        .jobs
        .iter()
        .filter(|other| binding.matches(other))
        .find_map(|other| lifecycle(&other.waiting))
        .or_else(|| {
            tracked.clone().find_map(|pr| match pr.lifecycle {
                Lifecycle::Merged => Some(Rest::Merged),
                Lifecycle::Closed => Some(Rest::Closed),
                Lifecycle::Open => None,
            })
        })
    {
        return rest;
    }
    let repository = settings
        .repositories
        .iter()
        .find(|r| r.id == job.configuration_id);
    match (repository, job.waiting.as_str()) {
        (None, _) | (_, monitoring::WAITING_REPOSITORY_REMOVED) => return Rest::RepositoryRemoved,
        (Some(r), _) if !r.enabled => return Rest::RepositoryDisabled,
        (_, monitoring::WAITING_REPOSITORY_DISABLED) => return Rest::RepositoryDisabled,
        (Some(r), _) if r.provider_account_id.as_deref() != Some(job.account_id.as_str()) => {
            return Rest::AccountUnbound
        }
        (_, monitoring::WAITING_BINDING_CHANGED) => return Rest::AccountUnbound,
        _ => {}
    }
    let newer = snapshot.items.iter().any(|other| {
        other.id != item.id
            && Binding::job(&other.job) == binding
            && iteration(other) > iteration(item)
    }) || tracked.clone().any(|pr| pr.item_id != item.id);
    if newer
        || matches!(
            job.waiting.as_str(),
            monitoring::WAITING_SUPERSEDED | monitoring::WAITING_NO_LONGER_CURRENT
        )
    {
        return Rest::Superseded;
    }
    match job.waiting.as_str() {
        monitoring::WAITING_INELIGIBLE
        | monitoring::WAITING_SCOPE_EXCLUDED
        | monitoring::WAITING_POLICY_CHANGED => Rest::NotAdmitted,
        _ => Rest::AwaitingChange,
    }
}

pub(super) fn project(settings: &Settings, snapshot: &mut Snapshot) {
    let rests: Vec<_> = snapshot
        .items
        .iter()
        .map(|item| {
            matches!(item.state, State::Stale | State::StaleAfterPublication)
                .then(|| classify(settings, snapshot, item))
        })
        .collect();
    for (item, rest) in snapshot.items.iter_mut().zip(rests) {
        item.rest = rest;
    }
}
