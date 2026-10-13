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
