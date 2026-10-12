//! A discarded, unpublished batch is re-reviewed once per lens (ADR 0008).

use super::{ReviewRun, Selection};
use crate::{
    monitoring::{JobOperation, OperationState},
    publication::{Phase, RemoteState},
    storage::Store,
};

/// Queues one fresh review for each current head whose latest batch was
/// discarded, or whose publishable result was never batched and whose lens
/// changed, and that was never published. Returns the number of runs queued.
/// `JobOperation::rereview_lens` is the durable once-per-(head, lens) marker, so
/// a restart, a poll or a second discard under the same lens never repeats it.
pub(crate) fn enqueue_discarded(store: &Store, now: i64) -> Result<usize, String> {
    let publications = store.load_publications()?;
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    let mut reviews = store.load_reviews()?;
    let mut queued = 0;
    let mut keys: Vec<String> = Vec::new();
    for run in reviews.iter().rev() {
        if !keys.contains(&run.key) {
            keys.push(run.key.clone());
        }
    }
    for key in &keys {
        // Only the newest attempt for the head counts; a queued or running
        // successor already supersedes it.
        let Some(run) = reviews.iter().rev().find(|r| &r.key == key).cloned() else {
            continue;
        };
        if run.operation.state != OperationState::Completed || run.result.is_none() {
            continue;
        }
        let own = publications
            .iter()
            .find(|p| p.review.operation.id == run.operation.id);
        let discarded = own.is_some_and(|p| p.phase == Phase::Discarded);
        // Without any batch, only a lens change makes the result unpublishable.
        if own.is_some() && !discarded {
            continue;
        }
        // A confirmed, pending, uncertain or in-flight batch for the head keeps it.
        if publications.iter().any(|other| {
            &other.review.key == key
                && (other.reserves_revision()
                    || other
                        .receipts
                        .iter()
                        .any(|r| r.state == RemoteState::Commented))
        }) {
            continue;
        }
        // Same head, open PR and still reviewable: resolving the current lens
        // fails for superseded, closed, merged or ineligible work.
        let Some(job) = jobs.iter().find(|job| run.matches_job(job)) else {
            continue;
        };
        let Ok(selection) = Selection::resolve(&settings, job, &run.assignment_id) else {
            continue;
        };
        let lens = selection.lens_hash();
        // Local-only results are never blocked by the lens, so they stay put.
        let publishes = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id)
            .is_some_and(|r| {
                r.assignments
                    .iter()
                    .any(|a| a.id == run.assignment_id && a.comment)
            });
        if !discarded && (!publishes || lens == run.selection.lens_hash()) {
            continue;
        }
        if reviews
            .iter()
            .any(|r| &r.key == key && r.operation.rereview_lens.as_deref() == Some(&lens))
        {
            continue;
        }
        let mut operation = JobOperation::review(job, now);
        operation.rereview_lens = Some(lens);
        reviews.push(ReviewRun {
            feedback_context: None,
            key: key.clone(),
            assignment_id: run.assignment_id.clone(),
            job: job.clone(),
            selection,
            operation,
            manual_start: false,
            trust_confirmed: false,
            phase: "Waiting for shared AI capacity".into(),
            error: None,
            result: None,
        });
        queued += 1;
    }
    if queued > 0 {
        store.save_reviews(&reviews)?;
    }
    Ok(queued)
}

#[cfg(test)]
mod tests;
