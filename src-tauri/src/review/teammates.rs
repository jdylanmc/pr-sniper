//! Teammate barrier: the primary of a PR state runs only after every
//! teammate has a successful report for that same state.

use super::{host::Candidate, Failure};
use crate::{
    exits::TeammateReport,
    monitoring::{self, OperationState},
    packet::TeammateEntry,
    storage::{Repository, Settings, Store},
};
use serde::Serialize;
use std::borrow::Borrow;

/// Which teammate holds the primary back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeammateRef {
    pub assignment_id: String,
    pub agent_name: String,
}

/// A parked teammate and why: typed, never prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedTeammate {
    pub assignment_id: String,
    pub agent_name: String,
    pub cause: monitoring::OperationFailure,
}

/// Typed reason a piece of work cannot start yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedBy {
    Teammate(TeammateRef),
}

pub const WAITING_FOR_TEAMMATES: &str = "Waiting for teammates";

/// A teammate's finished report for the state: stored on its own run.
pub(crate) fn report(candidate: &Candidate) -> Option<&TeammateReport> {
    let run = candidate.run.as_ref()?;
    if run.operation.state != OperationState::Completed {
        return None;
    }
    run.result.as_ref()?.output.report.as_ref()
}

fn state_seq(candidate: &Candidate) -> Option<u64> {
    candidate.job.work.as_ref().map(|w| w.state_seq)
}

/// The primary's teammates for its state, in assignment order: each is the
/// newest entry of that assignment on the same PR iteration. `None` when the
/// primary has no barrier (sole assignment, no designated primary).
pub(crate) fn peers<'a, C: Borrow<Candidate>>(
    repository: &Repository,
    candidates: &'a [C],
    primary: &Candidate,
) -> Option<Vec<(String, Option<&'a Candidate>)>> {
    let work = primary.job.work.as_ref()?;
    let assignment = repository
        .assignments
        .iter()
        .find(|a| a.id == primary.assignment_id)?;
    if repository.primary_assignment_id() != Some(assignment.id.as_str())
        || repository.assignments.len() < 2
    {
        return None;
    }
    Some(
        repository
            .assignments
            .iter()
            .filter(|a| a.id != assignment.id)
            .map(|a| {
                let newest = candidates
                    .iter()
                    .map(Borrow::borrow)
                    .filter(|c| {
                        c.assignment_id == a.id
                            && c.job.waiting != monitoring::WAITING_ASSIGNMENT_REMOVED
                            && c.job.work.as_ref().is_some_and(|w| {
                                w.item_id == work.item_id && w.iteration_id == work.iteration_id
                            })
                    })
                    .max_by_key(|c| state_seq(c));
                (a.id.clone(), newest)
            })
            .collect(),
    )
}

/// First teammate without a successful report for the primary's state.
pub(crate) fn blocker<C: Borrow<Candidate>>(
    repository: &Repository,
    candidates: &[C],
    primary: &Candidate,
) -> Option<TeammateRef> {
    let seq = state_seq(primary)?;
    let peers = peers(repository, candidates, primary)?;
    peers.into_iter().find_map(|(id, newest)| {
        let ready = newest.is_some_and(|c| state_seq(c) == Some(seq) && report(c).is_some());
        if ready {
            return None;
        }
        // An assignment with no entry yet is not enqueued for this PR: nothing to wait for.
        let candidate = newest?;
        Some(TeammateRef {
            assignment_id: id,
            agent_name: candidate.agent_name.clone(),
        })
    })
}

/// Reports for the packet of the primary run, or a failure when one is missing:
/// the barrier should have kept the primary from starting.
pub(crate) fn entries(
    store: &Store,
    run: &super::ReviewRun,
) -> Result<Vec<TeammateEntry>, Failure> {
    let settings: Settings = store.load_settings().map_err(Failure::permanent)?;
    let Some(repository) = settings
        .repositories
        .iter()
        .find(|r| r.id == run.job.configuration_id)
    else {
        return Ok(vec![]);
    };
    let candidates = super::host::candidates(store).map_err(Failure::permanent)?;
    let Some(primary) = candidates.iter().find(|c| {
        c.run
            .as_ref()
            .is_some_and(|r| r.operation.id == run.operation.id)
    }) else {
        return Ok(vec![]);
    };
    let Some(seq) = state_seq(primary) else {
        return Ok(vec![]);
    };
    let Some(peers) = peers(repository, &candidates, primary) else {
        return Ok(vec![]);
    };
    let mut entries = Vec::new();
    for (_, newest) in peers {
        let Some(candidate) = newest else { continue };
        let report = report(candidate).filter(|_| state_seq(candidate) == Some(seq));
        let Some(report) = report else {
            return Err(Failure::superseded(&format!(
                "Teammate report for {} is not ready for this state.",
                candidate.agent_name
            )));
        };
        let lens = candidate
            .run
            .as_ref()
            .and_then(|r| r.selection.configuration.as_ref())
            .map(|c| c.doctrines.iter().map(|d| d.title.clone()).collect())
            .unwrap_or_default();
        entries.push(TeammateEntry {
            agent: candidate.agent_name.clone(),
            lens,
            report: report.clone(),
        });
    }
    Ok(entries)
}
