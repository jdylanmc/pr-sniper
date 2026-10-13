//! Honouring an exit: the state it read is consumed and baselined, so the host's own
//! writes never retrigger the heartbeat. A state that moved is tossed, never consumed.

use super::ExitOutcome;
use crate::{
    github::{
        provider::{GithubClient, RemoteRepository},
        threads::QueryTransport,
    },
    monitoring::QueueJob,
    pr_state::{self, Consumed},
    retention::Binding,
    review::{ReviewResult, ReviewRun, RunMode},
    storage::Store,
};
use std::sync::Mutex;

/// The state sequence the job's packet was built for.
pub fn state_seq(job: &QueueJob) -> u64 {
    job.work.as_ref().map_or(0, |w| w.state_seq)
}

/// Hash of the PR state as it is now.
pub fn fresh_hash<T: QueryTransport>(
    client: &GithubClient<T>,
    repo: &RemoteRepository,
    number: u64,
) -> Result<String, String> {
    client
        .pr_snapshot(repo, number)
        .map(|snapshot| snapshot.normalize().hash())
        .map_err(|_| "PR state unreadable after the write.".to_string())
}

/// Consume the job's state with `baseline`. A moved state is not consumed; the
/// caller decides whether that is a toss (a result) or already counted.
pub fn consume(store: &Store, job: &QueueJob, baseline: &str) -> Result<Consumed, String> {
    pr_state::consume(store, &Binding::job(job), state_seq(job), baseline)
}

/// After a confirmed write: re-read the snapshot, baseline it, consume. A state
/// that moved meanwhile counts one toss (the write itself stays). The read runs
/// without the store lock; only the consume takes it.
pub fn consume_after<T: QueryTransport>(
    store: &Mutex<Store>,
    client: &GithubClient<T>,
    repo: &RemoteRepository,
    job: &QueueJob,
) -> Result<Consumed, String> {
    let baseline = fresh_hash(client, repo, job.number)?;
    let store = store.lock().map_err(|_| "Storage unavailable.")?;
    settle(&store, job, &baseline)
}

/// Store half of [`consume_after`]: consume with a baseline already read after the write.
pub fn settle(store: &Store, job: &QueueJob, baseline: &str) -> Result<Consumed, String> {
    let outcome = consume(store, job, baseline)?;
    if outcome == Consumed::Moved {
        pr_state::toss(store, &Binding::job(job))?;
    }
    Ok(outcome)
}

/// A write that failed after the run completed: the result is discarded, the toss
/// counts, and the review is queued again as a new operation until the count
/// reaches the park limit. A confirmed write never reaches here.
pub fn toss_and_requeue(
    store: &Store,
    job: &QueueJob,
    run: &ReviewRun,
    now: i64,
) -> Result<bool, String> {
    let count = pr_state::toss(store, &Binding::job(job))?;
    if count >= crate::monitoring::MAX_FAILED_ATTEMPTS {
        return Ok(false);
    }
    let mut reviews = store.load_reviews()?;
    reviews.push(ReviewRun {
        operation: crate::monitoring::JobOperation::review(job, now),
        result: None,
        error: None,
        packet: None,
        feedback_context: None,
        phase: "Waiting for shared AI capacity".into(),
        ..run.clone()
    });
    store.save_reviews(&reviews)?;
    Ok(true)
}

/// Explicit Retry of a parked packet exit (`tossed` or `exit_failed`): the PR's toss
/// budget restarts and each of the item's runs whose effect exit was not consumed, or
/// that parked as tossed, is reviewed again on the PR's current state.
pub fn retry_parked(store: &Store, item_id: &str, now: i64) -> Result<usize, String> {
    use crate::{
        monitoring::{OperationFailure, OperationState},
        queue::{self, State},
    };
    let snapshot = queue::snapshot(store, vec![])?;
    let item = snapshot
        .items
        .iter()
        .find(|i| i.id == item_id || i.aliases.iter().any(|a| a == item_id))
        .ok_or("Current PR iteration unavailable.")?;
    if item.state != State::Failed
        || !matches!(
            item.failure_cause,
            Some(queue::CAUSE_TOSSED | queue::CAUSE_EXIT_FAILED)
        )
    {
        return Err("Nothing parked to retry.".into());
    }
    let mut states = store.load_pr_states()?;
    let mut keys = Vec::new();
    for candidate in snapshot
        .reviews
        .iter()
        .filter(|c| item.review_keys.contains(&c.key))
    {
        let Some(run) = &candidate.run else { continue };
        let tossed = matches!(
            run.operation.state,
            OperationState::Failed | OperationState::ManualRetry
        ) && run.operation.failure == Some(OperationFailure::Tossed);
        let effect = run.operation.state == OperationState::Completed
            && matches!(
                run.result.as_ref().and_then(|r| r.output.exit.as_ref()),
                Some(
                    ExitOutcome::Comments { .. }
                        | ExitOutcome::Approve { .. }
                        | ExitOutcome::ApproveAndMerge { .. }
                )
            );
        let binding = Binding::job(&run.job);
        let seq = state_seq(&run.job);
        let consumed = states
            .get(&binding)
            .and_then(|r| r.exit.as_ref())
            .is_some_and(|e| e.state_seq == seq);
        if !(tossed || effect) || consumed {
            continue;
        }
        if let Some(record) = states.records.iter_mut().find(|r| r.binding == binding) {
            record.error_count = 0;
        }
        keys.push(candidate.key.clone());
    }
    if keys.is_empty() {
        return Err("Nothing parked to retry.".into());
    }
    store.save_pr_states(&states)?;
    for key in &keys {
        crate::review::host::request(store, key, true, now)?;
    }
    Ok(keys.len())
}

/// Exits with no provider write are honoured when the run completes: the summary
/// is kept locally and the state the agent read becomes the baseline. Other
/// exits consume when their effect is confirmed.
pub fn honour_local(
    store: &Store,
    run: &ReviewRun,
    result: &ReviewResult,
) -> Result<Consumed, String> {
    if run.mode != RunMode::Packet {
        return Ok(Consumed::Consumed);
    }
    let Some(ExitOutcome::ReadyForHuman { summary }) = &result.output.exit else {
        return Ok(Consumed::Consumed);
    };
    let baseline = run
        .packet
        .as_ref()
        .and_then(|p| p.state_hash.as_deref())
        .ok_or("The packet record has no state hash.")?;
    let outcome = consume(store, &run.job, baseline)?;
    if outcome == Consumed::Consumed {
        pr_state::set_local_summary(store, &Binding::job(&run.job), Some(summary.clone()))?;
    }
    Ok(outcome)
}
