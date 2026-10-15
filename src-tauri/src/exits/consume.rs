//! Honouring an exit: the state it read is consumed and baselined, so the host's own
//! writes never retrigger the heartbeat. A state that moved is tossed, never consumed.

use super::ExitOutcome;
use crate::{
    github::{
        provider::{GithubClient, RemoteRepository},
        state::{OwnWrites, PrSnapshot},
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

/// After a confirmed write: re-read the snapshot and consume (see [`settle_fresh`]).
/// The read runs without the store lock; only the consume takes it.
pub fn consume_after<T: QueryTransport>(
    store: &Mutex<Store>,
    client: &GithubClient<T>,
    repo: &RemoteRepository,
    job: &QueueJob,
    review_operation_id: &str,
) -> Result<Consumed, String> {
    let fresh = client
        .pr_snapshot(repo, job.number)
        .map_err(|_| "PR state unreadable after the write.".to_string())?;
    let store = store.lock().map_err(|_| "Storage unavailable.")?;
    settle_fresh(&store, job, review_operation_id, &fresh)
}

/// Store half of [`consume_after`]. The fresh post-write snapshot differs from the
/// packet state only by the exit's own writes: baseline it and consume the current
/// state (a heartbeat that already saw those writes is absorbed). Any other change:
/// consume the packet state with the packet hash as baseline, so the next heartbeat
/// queues one review of that change. Runs without packet marks keep the plain rule.
pub fn settle_fresh(
    store: &Store,
    job: &QueueJob,
    review_operation_id: &str,
    fresh: &PrSnapshot,
) -> Result<Consumed, String> {
    let fresh_hash = fresh.normalize().hash();
    let packet = store
        .review_evidence()?
        .into_iter()
        .find(|r| r.operation.id == review_operation_id)
        .and_then(|r| r.packet);
    let Some((marks, read_hash)) = packet.and_then(|p| Some((p.state_marks?, p.state_hash?)))
    else {
        return settle(store, job, &fresh_hash);
    };
    if marks.external(fresh, &own_writes(store, review_operation_id)?) {
        return settle(store, job, &read_hash);
    }
    let binding = Binding::job(job);
    let seq = store
        .load_pr_states()?
        .state_seq(&binding)
        .max(state_seq(job));
    let outcome = pr_state::consume(store, &binding, seq, &fresh_hash)?;
    if outcome == Consumed::Moved {
        pr_state::toss(store, &binding)?;
    }
    Ok(outcome)
}

/// Comment ids and resolved thread ids the exit of `review_operation_id` wrote.
fn own_writes(store: &Store, review_operation_id: &str) -> Result<OwnWrites, String> {
    use crate::exit_writes::{StepKind, StepState};
    let mut own = OwnWrites::default();
    for publication in store
        .load_publications()?
        .iter()
        .filter(|p| p.review.operation.id == review_operation_id)
    {
        for receipt in &publication.receipts {
            own.comments.extend(receipt.comment_ids.iter().cloned());
        }
    }
    for write in store
        .load_feedback()?
        .exit_writes
        .iter()
        .filter(|w| w.review_operation_id == review_operation_id)
    {
        for step in &write.steps {
            match (step.kind, step.state) {
                (StepKind::Reply, _) => own.comments.extend(step.receipt.clone()),
                (StepKind::Resolve, StepState::Attempted | StepState::Done) => {
                    own.resolved_threads.insert(step.thread_id.clone());
                }
                (StepKind::Resolve, StepState::Planned) => {}
            }
        }
    }
    Ok(own)
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
        failed_duck: None,
        attempt_ducks: Vec::new(),
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

/// PRs with an exit effect still being written (comment batch, replies and
/// resolutions, approve or merge). The heartbeat records no new state for them:
/// their own writes are absorbed by the consume that follows.
pub fn writing(store: &Store) -> Result<std::collections::BTreeSet<Binding>, String> {
    use crate::{actions::EffectState, publication::Phase};
    let mut bindings = std::collections::BTreeSet::new();
    bindings.extend(
        store
            .load_publications()?
            .iter()
            .filter(|p| {
                p.review.mode == RunMode::Packet
                    && matches!(
                        p.phase,
                        Phase::Preparing | Phase::Pending | Phase::Unresolved
                    )
                    && !p.operation.parked()
            })
            .map(|p| Binding::job(&p.review.job)),
    );
    bindings.extend(
        store
            .load_feedback()?
            .exit_writes
            .iter()
            .filter(|w| !w.terminal())
            .map(|w| Binding::job(&w.job)),
    );
    let actions = store.load_actions()?;
    bindings.extend(
        actions
            .effects
            .iter()
            .filter(|e| matches!(e.state, EffectState::Prepared | EffectState::Uncertain))
            .filter_map(|e| actions.finals.iter().find(|f| f.id == e.final_id))
            .filter(|f| f.packet.is_some())
            .map(|f| Binding::job(&f.basis.job)),
    );
    Ok(bindings)
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
