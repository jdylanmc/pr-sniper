use crate::{
    follow_up,
    monitoring::{self, OperationState, QueueJob, ScheduleHealth},
    publication::{self, RemoteState},
    review::{self, Decision},
    storage::{Settings, Store},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub mod dismissal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    // Safety blockers dominate aggregation; presentation uses a separate priority.
    StaleAfterPublication,
    Stale,
    Failed,
    WaitingForHuman,
    ConfirmationRequired,
    Reviewing,
    Queued,
    AwaitingPublication,
    WaitingForAuthor,
    WaitingOnCi,
    WaitingOnDraft,
    WaitingOnReviews,
    MachineSignedOff,
    Approved,
    Closed,
    Merged,
}

impl State {
    fn priority(self) -> u8 {
        match self {
            Self::MachineSignedOff | Self::WaitingForHuman => 0,
            Self::Failed | Self::ConfirmationRequired => 1,
            Self::StaleAfterPublication => 2,
            Self::Reviewing | Self::Queued | Self::AwaitingPublication => 3,
            Self::WaitingForAuthor
            | Self::WaitingOnCi
            | Self::WaitingOnDraft
            | Self::WaitingOnReviews => 4,
            Self::Stale => 5,
            Self::Approved | Self::Closed | Self::Merged => 6,
        }
    }

    pub(crate) fn summary(self) -> &'static str {
        match self {
            Self::WaitingOnCi => "Waiting on CI",
            Self::WaitingOnDraft => "Waiting on draft",
            Self::WaitingOnReviews => "Waiting on reviews",
            Self::MachineSignedOff => "Automated review completed. Ready for your final review and merge decision on GitHub; this is not human approval or a merge.",
            Self::Approved => "PR Sniper approved the current revision on GitHub. Nothing is waiting on you in the Queue.",
            Self::WaitingForAuthor => "Findings or questions were published. The PR author needs to respond or update the change; it is not machine-cleared.",
            Self::WaitingForHuman => "Your attention is needed. Inspect the local findings or conversation; do not assume the PR author has been notified.",
            Self::ConfirmationRequired => "Your confirmation is required before review or publication can proceed. Open evidence and actions.",
            Self::Reviewing => "Automated review or thread analysis is in progress. No final handoff yet.",
            Self::Queued => "Waiting for assigned review work. Every current assignment must finish before this PR can be machine-cleared.",
            Self::AwaitingPublication => "Review output is local; comment publication is pending. No completed handoff yet.",
            Self::Failed => "An operation failed or its remote outcome is unresolved. Inspect the original operation and retry or reconcile; a local sign-off does not clear this failure.",
            Self::Stale => "This review no longer establishes readiness for the current PR or configuration. Check the latest revision; saved evidence remains available.",
            Self::StaleAfterPublication => "Comments were published, but the reviewed state is now stale. They do not establish readiness for the current PR.",
            Self::Closed => "GitHub reports this PR closed. This iteration is terminal; saved evidence is retained.",
            Self::Merged => "GitHub reports this PR merged. This is provider lifecycle evidence, not a claim that PR Sniper merged it.",
        }
    }
}

/// Host effect of the primary's exit still in flight. Typed; the UI shows an icon and label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    Approving,
    Merging,
    /// Approved; the merge waits for the author to update the branch.
    MergePendingBehindBase,
    MergePendingConflicts,
    Publishing,
}

/// Typed causes of a failed packet exit (beside `publication_*`).
pub const CAUSE_PANIC: &str = "panic";
pub const CAUSE_TOSSED: &str = "tossed";
pub const CAUSE_EXIT_FAILED: &str = "exit_failed";

#[derive(Clone, Serialize)]
pub struct Item {
    pub dismiss_token: Option<String>,
    pub dismissed: bool,
    pub action_status: Option<crate::actions::Status>,
    /// The exit the item's primary run ended on (`panic_exit` for a panic).
    pub exit: Option<crate::exits::Exit>,
    /// Operation id of that run; actions match their packet final to it.
    #[serde(skip)]
    pub(crate) exit_run_id: Option<String>,
    pub progress: Option<Progress>,
    pub feedback: Vec<crate::feedback::View>,
    pub id: String,
    pub aliases: Vec<String>,
    pub job: QueueJob,
    pub state: State,
    // Set only for failed items; the Queue shows credential failures only.
    pub failure_class: Option<monitoring::FailureClass>,
    // Typed cause of a failed item: `publication_failed`, `publication_unresolved`,
    // `panic`, `tossed` (parked after the toss budget) or `exit_failed`.
    pub failure_cause: Option<&'static str>,
    // A parked teammate holds this PR's primary back; its typed cause rides along.
    pub failed_teammate: Option<review::teammates::FailedTeammate>,
    // The primary's Rubber Duck could not be set up: the duck Agent's name.
    pub failed_duck: Option<String>,
    // The primary is waiting on this teammate's report for the current state.
    pub blocked_by: Option<review::teammates::BlockedBy>,
    // Earliest automatic resume of a parked failure; absent once it waits for a human.
    pub resume_at: Option<i64>,
    pub summary: &'static str,
    pub review_keys: Vec<String>,
    pub follow_up_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub feedback: BTreeMap<String, Vec<crate::feedback::View>>,
    pub mentions: Vec<crate::feedback::Mention>,
    pub pending_threads: Vec<crate::feedback::PendingThread>,
    pub tracked: Vec<monitoring::TrackedPullRequest>,
    pub health: Vec<ScheduleHealth>,
    pub jobs: Vec<QueueJob>,
    pub(crate) reviews: Vec<review::host::Candidate>,
    pub(crate) publications: Vec<publication::host::Candidate>,
    pub(crate) follow_ups: Vec<follow_up::host::Candidate>,
    /// Host execution of `leave_comments` replies and resolutions.
    pub exit_writes: Vec<crate::exit_writes::ExitWrite>,
    #[serde(skip)]
    pub(crate) pr_states: crate::pr_state::PrStates,
    pub items: Vec<Item>,
}

pub fn item_id(job: &QueueJob) -> String {
    if let Some(work) = &job.work {
        return work.item_id.clone();
    }
    URL_SAFE_NO_PAD.encode(
        serde_json::json!([
            job.provider,
            job.account_id,
            job.configuration_id,
            job.repository_id,
            job.pull_request_id,
            job.head_sha,
            job.trigger_policy
        ])
        .to_string(),
    )
}

pub fn snapshot(store: &Store, health: Vec<ScheduleHealth>) -> Result<Snapshot, String> {
    let mut snapshot = normal_snapshot(store, health)?;
    crate::actions::project(store, &mut snapshot)?;
    dismissal::project(store, &mut snapshot)?;
    Ok(snapshot)
}

/// Computed once per version of saved state and `health`, then shared: the
/// panel, notifications and scheduler all read the same projection.
pub(crate) fn normal_snapshot(
    store: &Store,
    health: Vec<ScheduleHealth>,
) -> Result<Snapshot, String> {
    let key = serde_json::to_vec(&health)
        .map(|bytes| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut hasher);
            hasher.finish()
        })
        .map_err(|_| "Cannot read schedule health.")?;
    let shared = store.cached_state("normal-snapshot", key, || project_saved(store, health))?;
    Ok(shared.as_ref().clone())
}

fn project_saved(store: &Store, health: Vec<ScheduleHealth>) -> Result<Snapshot, String> {
    let settings = store.load_settings()?;
    let monitoring = store.load_monitoring_state()?;
    let health = if health.is_empty() {
        monitoring.health.into_values().collect()
    } else {
        health
    };
    let ledger = store.load_feedback()?;
    let jobs = store.load_queue()?;
    let mut feedback = BTreeMap::new();
    if !jobs.is_empty() {
        let reviews = store.shared_evidence()?;
        let replies = store.load_follow_ups()?;
        let publications = store.load_publications()?;
        for job in &jobs {
            feedback.insert(
                item_id(job),
                crate::feedback::views_from_evidence(
                    job,
                    &settings,
                    &ledger,
                    &reviews,
                    &replies,
                    &publications,
                ),
            );
        }
    }
    let mut result = Snapshot {
        feedback,
        mentions: ledger.mentions,
        pending_threads: ledger.pending_threads,
        exit_writes: ledger.exit_writes,
        pr_states: store.load_pr_states()?,
        tracked: store.load_queue_state()?.tracked,
        health,
        jobs,
        reviews: review::host::candidates(store)?,
        publications: publication::host::candidates(store)?,
        follow_ups: follow_up::host::candidates(store)?,
        items: vec![],
    };
    result.items = project(&settings, &result);
    Ok(result)
}

fn obsolete(job: &QueueJob) -> bool {
    matches!(
        job.waiting.as_str(),
        monitoring::WAITING_SUPERSEDED
            | monitoring::WAITING_NO_LONGER_CURRENT
            | monitoring::WAITING_INELIGIBLE
            | monitoring::WAITING_SCOPE_EXCLUDED
            | monitoring::WAITING_BINDING_CHANGED
            | monitoring::WAITING_POLICY_CHANGED
            | monitoring::WAITING_REPOSITORY_REMOVED
    )
}

fn publication_cause(
    publications: &[publication::host::Candidate],
    publication_items: &[Option<String>],
    id: &str,
) -> Option<&'static str> {
    let own: Vec<_> = publications
        .iter()
        .zip(publication_items)
        .filter(|(_, item)| item.as_deref() == Some(id))
        .filter_map(|(p, _)| p.publication.as_ref())
        .filter(|p| p.phase != publication::Phase::Discarded)
        .collect();
    if own.iter().any(|p| p.uncertain) {
        Some("publication_unresolved")
    } else if own
        .iter()
        .any(|p| failed(&p.operation) || p.error.is_some())
    {
        Some("publication_failed")
    } else {
        None
    }
}

fn failed(operation: &monitoring::JobOperation) -> bool {
    matches!(
        operation.state,
        OperationState::Failed | OperationState::ManualRetry
    ) && !operation.ended_superseded()
}

/// Packet facts the state of a review needs beyond its own run.
struct ExitContext<'a> {
    exit_writes: &'a [crate::exit_writes::ExitWrite],
    pr_states: &'a crate::pr_state::PrStates,
}

impl ExitContext<'_> {
    fn write(&self, run: &review::ReviewRun) -> Option<&crate::exit_writes::ExitWrite> {
        self.exit_writes
            .iter()
            .find(|w| w.review_operation_id == run.operation.id)
    }

    /// A tossed write whose PR spent its toss budget: no new run was queued.
    fn parked(&self, run: &review::ReviewRun) -> bool {
        let seq = crate::exits::consume::state_seq(&run.job);
        self.pr_states
            .get(&crate::retention::Binding::job(&run.job))
            .is_some_and(|r| {
                r.error_count >= monitoring::MAX_FAILED_ATTEMPTS
                    && r.exit.as_ref().is_none_or(|e| e.state_seq != seq)
            })
    }
}

fn review_state(
    settings: &Settings,
    candidate: &review::host::Candidate,
    publications: &[publication::host::Candidate],
    feedback: &[crate::feedback::View],
    context: &ExitContext,
) -> State {
    let batch = candidate.run.as_ref().and_then(|run| {
        publications
            .iter()
            .find(|p| p.review_operation_id == run.operation.id)
    });
    // A discarded batch leaves no obligation: the review result decides alone.
    let discarded = batch
        .and_then(|p| p.publication.as_ref())
        .is_some_and(|p| p.phase == publication::Phase::Discarded);
    let tossed_batch = batch
        .and_then(|p| p.publication.as_ref())
        .is_some_and(publication::Publication::tossed);
    let publication = batch
        .and_then(|p| p.publication.as_ref())
        .filter(|p| p.phase != publication::Phase::Discarded);
    let published = publication.is_some_and(|p| {
        p.receipts
            .last()
            .is_some_and(|r| r.state == RemoteState::Commented)
    });
    if obsolete(&candidate.job)
        || publication.is_some_and(|p| {
            matches!(
                p.phase,
                publication::Phase::Stale | publication::Phase::StaleAfterPublication
            )
        })
    {
        return if published {
            State::StaleAfterPublication
        } else {
            State::Stale
        };
    }
    if publication.is_some_and(|p| p.uncertain || failed(&p.operation) || p.error.is_some()) {
        return State::Failed;
    }
    let finished = candidate
        .run
        .as_ref()
        .is_some_and(|r| r.operation.state == OperationState::Completed && r.result.is_some());
    if let Some(wait) = context
        .pr_states
        .get(&crate::retention::Binding::job(&candidate.job))
        .and_then(|r| r.external_wait.as_ref())
        .filter(|w| crate::exits::consume::state_seq(&candidate.job) == w.state_seq)
    {
        return match wait.reason {
            crate::exits::WaitReason::Ci => State::WaitingOnCi,
            crate::exits::WaitReason::Draft => State::WaitingOnDraft,
            crate::exits::WaitReason::RequiredReviews => State::WaitingOnReviews,
        };
    }
    // Finished work stays finished when its assignment or repository goes away.
    if candidate.blocked.is_some() && !finished {
        return State::Queued;
    }
    let Some(run) = &candidate.run else {
        return State::Queued;
    };
    if failed(&run.operation) {
        return State::Failed;
    }
    if run.operation.state != OperationState::Completed {
        return if run.operation.state == OperationState::Running {
            State::Reviewing
        } else {
            State::Queued
        };
    }
    let Some(result) = &run.result else {
        return State::Failed;
    };
    if result.output.feedback_conflict || !result.output.held_findings.is_empty() {
        return State::WaitingForHuman;
    }
    if result.reviewed_base_sha.is_none() {
        return State::Stale;
    }
    match &candidate.job.observed_base_sha {
        Some(base) if Some(base) != result.reviewed_base_sha.as_ref() => {
            return if published {
                State::StaleAfterPublication
            } else {
                State::Stale
            };
        }
        None => return State::Queued,
        Some(_) => {}
    }
    if let Some(exit) = &result.output.exit {
        use crate::exit_writes::Phase;
        use crate::exits::ExitOutcome;
        match exit {
            ExitOutcome::Wait { .. } => return State::Queued,
            // Approve exits are signed off; actions show Approving/Merging, then Approved/Merged.
            ExitOutcome::ReadyForHuman { .. }
            | ExitOutcome::SubmitReport
            | ExitOutcome::Approve { .. }
            | ExitOutcome::ApproveAndMerge { .. } => return State::MachineSignedOff,
            // A sign-off is Ready once signed; its resolutions or actions show progress.
            ExitOutcome::Lgtm { .. } => {
                return match context.write(run).map(|write| write.phase) {
                    Some(Phase::Failed) => State::Failed,
                    Some(Phase::Tossed) if context.parked(run) => State::Failed,
                    Some(Phase::Tossed) => State::Queued,
                    _ => State::MachineSignedOff,
                }
            }
            ExitOutcome::Comments {
                new_comments, body, ..
            } => match context.write(run) {
                Some(write) => match write.phase {
                    Phase::Failed => return State::Failed,
                    Phase::Tossed if context.parked(run) => return State::Failed,
                    // The toss re-queues the PR.
                    Phase::Tossed => return State::Queued,
                    Phase::Completed if write.consumed => {
                        if new_comments.is_empty() && body.is_none() {
                            return State::WaitingForAuthor;
                        }
                    }
                    _ => return State::AwaitingPublication,
                },
                // A content toss of the batch: re-queued, or parked when the budget is spent.
                None if tossed_batch => {
                    return if context.parked(run) {
                        State::Failed
                    } else {
                        State::Queued
                    };
                }
                None if new_comments.is_empty() && body.is_none() => {
                    return State::AwaitingPublication
                }
                None => {}
            },
        }
    }
    let assignment = settings
        .repositories
        .iter()
        .find(|r| r.id == candidate.job.configuration_id && r.enabled)
        .and_then(|r| {
            r.assignments
                .iter()
                .find(|a| a.id == candidate.assignment_id)
        });
    if assignment.is_some()
        && !discarded
        && publication.is_none()
        && batch
            .and_then(|p| p.blocked.as_deref())
            .is_some_and(|reason| reason != "Comments are disabled for this Agent assignment.")
    {
        return State::Failed;
    }
    let comments = assignment.is_some_and(|a| a.comment);
    if !discarded && (comments || publication.is_some()) {
        // A recorded publication outlives later configuration edits.
        if publication.is_none() && batch.is_some_and(|p| p.blocked.is_some()) {
            return State::Failed;
        }
        if let Some(publication) = publication {
            if publication.phase != publication::Phase::Published
                || publication.operation.state != OperationState::Completed
                || !published
            {
                return if publication.cancelled {
                    State::ConfirmationRequired
                } else {
                    State::AwaitingPublication
                };
            }
            if publication
                .batch
                .as_ref()
                .is_some_and(|b| !b.unmappable.is_empty())
            {
                return State::WaitingForHuman;
            }
        } else {
            return if batch.is_some_and(|p| p.automatic) {
                State::AwaitingPublication
            } else {
                State::ConfirmationRequired
            };
        }
    }
    match result.output.decision {
        Decision::MachineSignOff => State::MachineSignedOff,
        Decision::HumanInputRequired if published => {
            let cleared = publication.is_some_and(|p| {
                !result.output.findings.is_empty()
                    && p.receipts.last().is_some_and(|r| {
                        !r.comment_ids.is_empty()
                            && r.comment_ids.iter().all(|id| {
                                feedback.iter().any(|f| {
                                    f.context.root_id == *id
                                        && f.context.publication_id == p.id
                                        && matches!(f.state, "closed" | "cleared")
                                })
                            })
                    })
            });
            if cleared {
                State::MachineSignedOff
            } else {
                State::WaitingForAuthor
            }
        }
        Decision::HumanInputRequired => State::WaitingForHuman,
    }
}

fn follow_up_state(candidate: &follow_up::host::Candidate) -> Option<State> {
    use follow_up::Phase;
    let run = &candidate.run;
    if run.has_unsettled_analysis_failure() {
        return Some(State::Failed);
    }
    if matches!(run.phase, Phase::Quiet | Phase::Published) && !run.uncertain && run.error.is_none()
    {
        return None;
    }
    if run.phase == Phase::StaleAfterPublication {
        return Some(State::StaleAfterPublication);
    }
    if run.uncertain
        || run.error.is_some()
        || run.analysis.as_ref().is_some_and(failed)
        || run.publication.as_ref().is_some_and(failed)
    {
        return Some(State::Failed);
    }
    if candidate.blocked.is_some() {
        return Some(State::Queued);
    }
    if candidate.human_gate || run.phase == Phase::HumanInputRequired {
        return Some(State::WaitingForHuman);
    }
    match run.phase {
        Phase::WaitingStart => Some(State::Queued),
        Phase::Analyzing => Some(State::Reviewing),
        Phase::WaitingPublication
            if run.publication.is_none()
                && !candidate.automatic_publication
                && run
                    .analysis
                    .as_ref()
                    .is_some_and(|operation| operation.state == OperationState::Completed)
                && run.result.as_ref().is_some_and(|result| {
                    result.output.decision == crate::follow_up::ReplyDecision::Reply
                }) =>
        {
            None
        }
        Phase::WaitingPublication => Some(State::AwaitingPublication),
        Phase::Publishing => Some(State::AwaitingPublication),
        Phase::Stopped | Phase::Unresolved => Some(State::Failed),
        Phase::Quiet | Phase::Published => None,
        Phase::HumanInputRequired | Phase::StaleAfterPublication => unreachable!(),
    }
}

fn project(settings: &Settings, snapshot: &Snapshot) -> Vec<Item> {
    // Indexes keep projection linear in history; each lookup matches the
    // former first-match scan exactly.
    let index = CurrentJobs::new(&snapshot.jobs);
    let current = |job| index.current(job);
    let publication_items: Vec<Option<String>> = snapshot
        .publications
        .iter()
        .map(|p| {
            p.publication
                .as_ref()
                .map(|p| item_id(current(&p.review.job)))
        })
        .collect();
    let mut publications_by_item: BTreeMap<&str, Vec<&publication::Publication>> = BTreeMap::new();
    for (candidate, item) in snapshot.publications.iter().zip(&publication_items) {
        if let (Some(publication), Some(item)) = (candidate.publication.as_ref(), item) {
            publications_by_item
                .entry(item.as_str())
                .or_default()
                .push(publication);
        }
    }
    let own_publications = |id: &str| {
        publications_by_item
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .copied()
    };
    let mut reviews_by_item: BTreeMap<String, Vec<&review::host::Candidate>> = BTreeMap::new();
    for review in &snapshot.reviews {
        reviews_by_item
            .entry(item_id(&review.job))
            .or_default()
            .push(review);
    }
    let mut aliases_by_item: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for job in &snapshot.jobs {
        let aliases = aliases_by_item.entry(item_id(job)).or_default();
        if let Some(alias) = job.work.as_ref().and_then(|w| w.legacy_item_id.clone()) {
            aliases.push(alias);
        }
    }
    let mut groups = BTreeMap::new();
    for job in snapshot
        .jobs
        .iter()
        .chain(snapshot.reviews.iter().map(|r| &r.job))
        .chain(snapshot.follow_ups.iter().map(|f| &f.run.context.job))
    {
        let job = current(job);
        groups
            .entry(item_id(job))
            .and_modify(|previous: &mut &QueueJob| {
                if previous.waiting == monitoring::WAITING_ASSIGNMENT_REMOVED
                    && job.waiting != monitoring::WAITING_ASSIGNMENT_REMOVED
                    || previous.work.is_none() && job.work.is_some()
                    || previous
                        .work
                        .as_ref()
                        .zip(job.work.as_ref())
                        .is_some_and(|(old, new)| new.state_seq > old.state_seq)
                {
                    *previous = job;
                }
            })
            .or_insert(job);
    }
    let mut items = Vec::new();
    for (id, job) in groups {
        let mut reviews: Vec<_> = reviews_by_item.remove(&id).unwrap_or_default();
        // One entry per PR: an older state's result is history and must not
        // mask the newest state's wait.
        let newest = |r: &review::host::Candidate| r.job.work.as_ref().map_or(0, |w| w.state_seq);
        let latest: BTreeMap<_, _> = reviews.iter().fold(BTreeMap::new(), |mut latest, r| {
            let seq = latest.entry(r.assignment_id.clone()).or_insert(0);
            *seq = (*seq).max(newest(r));
            latest
        });
        reviews.retain(|r| newest(r) >= latest[&r.assignment_id]);
        let follow_ups: Vec<_> = snapshot
            .follow_ups
            .iter()
            .filter(|f| crate::feedback::same_pr(&f.run.context.job, job))
            .collect();
        let context = ExitContext {
            exit_writes: &snapshot.exit_writes,
            pr_states: &snapshot.pr_states,
        };
        let mut states: Vec<_> = reviews
            .iter()
            .filter(|r| r.job.waiting != monitoring::WAITING_ASSIGNMENT_REMOVED)
            .map(|r| {
                review_state(
                    settings,
                    r,
                    &snapshot.publications,
                    snapshot.feedback.get(&id).map(Vec::as_slice).unwrap_or(&[]),
                    &context,
                )
            })
            .collect();
        let mut warnings = BTreeSet::new();
        let mut failures: Vec<monitoring::FailureClass> = Vec::new();
        failures.extend(
            reviews
                .iter()
                .filter_map(|r| r.run.as_ref()?.operation.failure_class()),
        );
        failures.extend(
            reviews
                .iter()
                .filter_map(|r| context.write(r.run.as_ref()?))
                .filter(|w| w.phase == crate::exit_writes::Phase::Failed)
                .filter_map(|w| w.operation.failure_class()),
        );
        let resume_at = reviews
            .iter()
            .filter_map(|r| r.run.as_ref())
            .map(|r| &r.operation)
            .chain(own_publications(&id).map(|p| &p.operation))
            .filter(|operation| operation.parked())
            .filter_map(|operation| operation.resume_at)
            .min();
        failures.extend(own_publications(&id).filter_map(|p| p.operation.failure_class()));
        failures.extend(follow_ups.iter().flat_map(|f| {
            [&f.run.analysis, &f.run.publication]
                .into_iter()
                .flatten()
                .filter_map(|op| op.failure_class())
        }));
        for review in &reviews {
            if let Some(error) = &review.blocked {
                warnings.insert(error.clone());
            }
            if let Some(error) = review.run.as_ref().and_then(|r| r.error.as_ref()) {
                warnings.insert(error.clone());
            }
            if let Some(result) = review.run.as_ref().and_then(|r| r.result.as_ref()) {
                match &review.job.observed_base_sha {
                    Some(base) if Some(base) != result.reviewed_base_sha.as_ref() => {
                        warnings.insert("The target base changed since this review. Saved results do not cover the latest base.".into());
                    }
                    None => {
                        warnings.insert("Waiting for a successful poll to confirm the target base after upgrading.".into());
                    }
                    Some(_) => {}
                }
            }
            if let Some(batch) = review.run.as_ref().and_then(|r| {
                snapshot
                    .publications
                    .iter()
                    .find(|p| p.review_operation_id == r.operation.id)
            }) {
                if let Some(error) = &batch.blocked {
                    warnings.insert(error.clone());
                }
            }
        }
        for follow_up in &follow_ups {
            if follow_up.superseded {
                continue;
            }
            let prior_iteration = follow_up.run.context.job.head_sha != job.head_sha
                || follow_up
                    .run
                    .context
                    .job
                    .work
                    .as_ref()
                    .map(|w| &w.iteration_id)
                    != job.work.as_ref().map(|w| &w.iteration_id);
            if prior_iteration
                && follow_up.run.can_retire_assessment()
                && follow_up.run.phase != follow_up::Phase::HumanInputRequired
            {
                continue;
            }
            if let Some(state) = follow_up_state(follow_up) {
                states.push(state);
            }
            if follow_up.run.has_unsettled_analysis_failure() {
                warnings.insert("A prior analysis failure requires validated explicit recovery; queuing a retry is not settlement.".into());
            }
            for error in [&follow_up.blocked, &follow_up.run.error]
                .into_iter()
                .flatten()
            {
                warnings.insert(error.clone());
            }
        }
        let feedback = snapshot.feedback.get(&id).cloned().unwrap_or_default();
        for concern in &feedback {
            match concern.state {
                "open" => states.push(State::WaitingForAuthor),
                "human_input_required" => states.push(State::WaitingForHuman),
                "unavailable" | "owner_unavailable" => states.push(State::Queued),
                _ => {}
            }
        }
        for mention in snapshot.mentions.iter().filter(|m| m.binding.matches(job)) {
            match mention.association(
                &snapshot.tracked,
                &snapshot.jobs,
                snapshot.follow_ups.iter().map(|f| &f.run),
            ) {
                Ok(mention_item) if mention_item != id => continue,
                Ok(_) => {}
                Err(reason) => {
                    // Missing execution or ambiguous legacy evidence is not
                    // proof that an old operation or human concern is settled.
                    states.push(State::Queued);
                    warnings.insert(reason.into());
                }
            }
            if mention.follow_up_id.is_none() {
                states.push(State::Queued);
                warnings.insert("Observed mention is awaiting durable execution admission.".into());
            }
            if let Some(reason) = &mention.blocked {
                // Admission still pending is autonomous waiting; a blocked
                // executed mention cannot proceed on its own.
                states.push(if mention.follow_up_id.is_some() {
                    State::Failed
                } else {
                    State::Queued
                });
                warnings.insert(reason.clone());
            }
            if mention
                .follow_up_id
                .as_ref()
                .is_some_and(|id| !snapshot.follow_ups.iter().any(|f| &f.run.id == id))
            {
                states.push(State::Failed);
                warnings.insert(
                    "Mention execution history is unavailable; no replay or clearance inferred."
                        .into(),
                );
            }
        }
        for intent in snapshot
            .pending_threads
            .iter()
            .filter(|intent| intent.binding.matches(job) && intent.item_id == id)
        {
            if intent.follow_up_id.is_none()
                || intent.blocked.is_some()
                || intent.follow_up_id.as_ref().is_some_and(|id| {
                    !snapshot
                        .follow_ups
                        .iter()
                        .any(|candidate| &candidate.run.id == id)
                })
            {
                states.push(State::Queued);
                warnings.insert(intent.blocked.clone().unwrap_or_else(|| "Observed discussion awaits durable primary assessment; no clearance inferred.".into()));
            }
        }
        for publication in own_publications(&id) {
            if let Some(error) = &publication.error {
                warnings.insert(error.clone());
            }
            if publication.uncertain {
                warnings.insert("Publication outcome unresolved; reconcile the original batch, never a replacement.".into());
                states.push(State::Failed);
            }
        }
        if obsolete(job) {
            let published = own_publications(&id).any(|p| {
                p.receipts
                    .last()
                    .is_some_and(|r| r.state == RemoteState::Commented)
            }) || follow_ups.iter().any(|f| f.run.receipt.is_some());
            states.push(if published {
                State::StaleAfterPublication
            } else {
                State::Stale
            });
            warnings.insert(format!(
                "Detection state: {}. No live head or mergeability guarantee.",
                job.waiting
            ));
        } else {
            let repository = settings
                .repositories
                .iter()
                .find(|r| r.id == job.configuration_id);
            let covered: BTreeSet<_> = reviews.iter().map(|r| &r.assignment_id).collect();
            if repository.is_some_and(|r| r.assignments.iter().any(|a| !covered.contains(&a.id))) {
                states.push(State::Queued);
                warnings.insert("Waiting for every currently assigned Agent to detect and review this revision.".into());
            }
            for health in snapshot.health.iter().filter(|h| {
                h.repository_id == job.configuration_id
                    && h.provider_account_id.as_deref() == Some(&job.account_id)
            }) {
                if health.conversation_admission_pending {
                    states.push(State::Queued);
                    warnings.insert("Conversation observations are not fully admitted; retry the repository check before relying on readiness.".into());
                }
                if let Some(failure) = &health.last_failure {
                    let credentials = matches!(
                        failure.as_str(),
                        "account_disconnected" | "account_binding_required"
                    );
                    // A check that is still backing off retries on its own.
                    if !credentials && health.operation.as_ref().is_some_and(|op| !op.parked()) {
                        continue;
                    }
                    states.push(State::Failed);
                    failures.push(if credentials {
                        monitoring::FailureClass::Credentials
                    } else {
                        health
                            .operation
                            .as_ref()
                            .and_then(|op| op.failure.as_ref())
                            .map_or(monitoring::FailureClass::Other, |f| f.class())
                    });
                    warnings.insert(format!("Monitoring failure: {failure}. Check schedule health before relying on this saved revision."));
                }
            }
        }
        let state = match job.waiting.as_str() {
            monitoring::WAITING_CLOSED => State::Closed,
            monitoring::WAITING_MERGED => State::Merged,
            _ => states.into_iter().min().unwrap_or(State::Queued),
        };
        let repository = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id);
        let primary_id = repository.and_then(|r| r.primary_assignment_id());
        let failed_teammate = reviews
            .iter()
            .filter(|r| primary_id.is_some_and(|id| id != r.assignment_id))
            .filter(|r| r.run.as_ref().is_some_and(|run| failed(&run.operation)))
            .find_map(|r| {
                Some(review::teammates::FailedTeammate {
                    assignment_id: r.assignment_id.clone(),
                    agent_name: r.agent_name.clone(),
                    cause: r
                        .run
                        .as_ref()?
                        .operation
                        .failure
                        .clone()
                        .unwrap_or(monitoring::OperationFailure::Provider),
                })
            });
        let failed_duck = reviews
            .iter()
            .filter_map(|r| r.run.as_ref())
            .filter(|run| failed(&run.operation))
            .find_map(|run| run.failed_duck.clone());
        let blocked_by = (state != State::Failed || failed_teammate.is_some())
            .then(|| {
                let primary = reviews
                    .iter()
                    .find(|r| primary_id == Some(r.assignment_id.as_str()))?;
                review::teammates::blocker(repository?, &reviews, primary)
                    .map(review::teammates::BlockedBy::Teammate)
            })
            .flatten();
        let exit_cause = reviews
            .iter()
            .filter_map(|r| r.run.as_ref())
            .find_map(|run| {
                use crate::exit_writes::Phase;
                if failed(&run.operation) {
                    return match run.operation.failure {
                        Some(monitoring::OperationFailure::Panic) => Some(CAUSE_PANIC),
                        Some(monitoring::OperationFailure::Tossed) => Some(CAUSE_TOSSED),
                        _ => None,
                    };
                }
                if context.parked(run)
                    && snapshot.publications.iter().any(|p| {
                        p.review_operation_id == run.operation.id
                            && p.publication
                                .as_ref()
                                .is_some_and(publication::Publication::tossed)
                    })
                {
                    return Some(CAUSE_TOSSED);
                }
                match context.write(run)?.phase {
                    Phase::Failed => Some(CAUSE_EXIT_FAILED),
                    Phase::Tossed if context.parked(run) => Some(CAUSE_TOSSED),
                    _ => None,
                }
            });
        let failure_cause = if state == State::Failed {
            exit_cause
                .or_else(|| publication_cause(&snapshot.publications, &publication_items, &id))
        } else {
            None
        };
        // The primary's exit leads; a teammate's report is never the item's exit.
        let exit_run = reviews
            .iter()
            .filter_map(|r| Some((r, r.run.as_ref()?, run_exit(r.run.as_ref()?)?)))
            .filter(|(_, _, exit)| *exit != crate::exits::Exit::SubmitReport)
            .min_by_key(|(r, _, _)| primary_id != Some(r.assignment_id.as_str()));
        let exit = exit_run.map(|(_, _, exit)| exit);
        let progress = match (exit, state) {
            (
                Some(crate::exits::Exit::ApprovePullRequest | crate::exits::Exit::ApproveAndMerge),
                State::MachineSignedOff,
            ) => Some(Progress::Approving),
            (Some(crate::exits::Exit::Lgtm), State::MachineSignedOff)
                if exit_run.is_some_and(|(_, run, _)| {
                    matches!(
                        run.result.as_ref().and_then(|r| r.output.exit.as_ref()),
                        Some(crate::exits::ExitOutcome::Lgtm { action, .. })
                            if *action != crate::exits::LgtmAction::Ready
                    )
                }) =>
            {
                Some(Progress::Approving)
            }
            (Some(crate::exits::Exit::LeaveComments), State::AwaitingPublication) => {
                Some(Progress::Publishing)
            }
            _ => None,
        };
        items.push(Item {
            dismiss_token: None,
            dismissed: false,
            action_status: None,
            exit,
            exit_run_id: exit_run.map(|(_, run, _)| run.operation.id.clone()),
            progress,
            feedback,
            aliases: aliases_by_item.get(&id).cloned().unwrap_or_default(),
            id,
            job: job.clone(),
            state,
            resume_at: if state == State::Failed {
                resume_at
            } else {
                None
            },
            failure_cause,
            failure_class: (state == State::Failed).then(|| {
                failed_teammate
                    .as_ref()
                    .map(|teammate| teammate.cause.class())
                    .or_else(|| failures.into_iter().min())
                    .unwrap_or(monitoring::FailureClass::Other)
            }),
            failed_teammate,
            failed_duck,
            blocked_by,
            summary: state.summary(),
            review_keys: reviews.iter().map(|r| r.key.clone()).collect(),
            follow_up_ids: follow_ups.iter().map(|f| f.run.id.clone()).collect(),
            warnings: warnings.into_iter().collect(),
        });
    }
    items.sort_by(|a, b| {
        (
            a.state.priority(),
            &a.job.repository_name,
            a.job.number,
            &a.id,
        )
            .cmp(&(
                b.state.priority(),
                &b.job.repository_name,
                b.job.number,
                &b.id,
            ))
    });
    items
}

/// The exit a packet run ended on; a panic leaves no outcome, only its typed failure.
pub(crate) fn run_exit(run: &review::ReviewRun) -> Option<crate::exits::Exit> {
    use crate::exits::{Exit, ExitOutcome};
    if run.operation.failure == Some(monitoring::OperationFailure::Panic) && failed(&run.operation)
    {
        return Some(Exit::PanicExit);
    }
    if run.operation.state != OperationState::Completed {
        return None;
    }
    Some(match run.result.as_ref()?.output.exit.as_ref()? {
        ExitOutcome::Comments { .. } => Exit::LeaveComments,
        ExitOutcome::ReadyForHuman { .. } => Exit::ReadyForHuman,
        ExitOutcome::Wait { .. } => Exit::Wait,
        ExitOutcome::Approve { .. } => Exit::ApprovePullRequest,
        ExitOutcome::ApproveAndMerge { .. } => Exit::ApproveAndMerge,
        ExitOutcome::SubmitReport => Exit::SubmitReport,
        ExitOutcome::Lgtm { .. } => Exit::Lgtm,
    })
}

/// The first saved job for the same assignment and review key, if any.
struct CurrentJobs<'a>(std::collections::HashMap<(&'a str, String), &'a QueueJob>);

impl<'a> CurrentJobs<'a> {
    fn new(jobs: &'a [QueueJob]) -> Self {
        let mut index = std::collections::HashMap::new();
        for job in jobs {
            if let Some(assignment_id) = job.assignment_id.as_deref() {
                index
                    .entry((assignment_id, review::key(job, assignment_id)))
                    .or_insert(job);
            }
        }
        Self(index)
    }

    fn current(&self, job: &'a QueueJob) -> &'a QueueJob {
        let Some(assignment_id) = job.assignment_id.as_deref() else {
            return job;
        };
        self.0
            .get(&(assignment_id, review::key(job, assignment_id)))
            .copied()
            .unwrap_or(job)
    }
}

pub fn destination(store: &Store, id: &str, file: Option<&str>) -> Result<url::Url, String> {
    if let Some(message) =
        crate::retention::cleaned(store, &crate::panel::Detail::Item { item_id: id.into() })?
    {
        return Err(message.into());
    }
    let snapshot = snapshot(store, vec![])?;
    let item = snapshot
        .items
        .iter()
        .find(|item| item.id == id || item.aliases.iter().any(|alias| alias == id))
        .ok_or("This exact queue item is no longer available; no other PR was opened.")?;
    if item.job.provider != "github" || item.job.number == 0 {
        return Err("This queue item's provider destination is unsupported.".into());
    }
    let name = crate::storage::canonical_repository(&item.job.repository_name)?;
    let mut url = url::Url::parse(&format!(
        "https://github.com/{name}/pull/{}",
        item.job.number
    ))
    .map_err(|_| "Invalid saved GitHub destination.")?;
    if let Some(file) = file {
        let present = snapshot
            .reviews
            .iter()
            .filter(|r| item.review_keys.contains(&r.key))
            .filter_map(|r| r.run.as_ref())
            .filter_map(|r| r.result.as_ref())
            .any(|r| r.output.files.iter().any(|f| f.path == file))
            || item
                .action_status
                .as_ref()
                .and_then(|s| s.final_review.as_ref())
                .and_then(|f| f.execution.result.as_ref())
                .is_some_and(|r| r.output.files.iter().any(|f| f.path == file));
        if !present {
            return Err("This file is not in the exact review's complete guide.".into());
        }
        url.set_path(&format!("{}/files", url.path()));
        url.set_fragment(Some(&format!("diff-{:x}", Sha256::digest(file.as_bytes()))));
    }
    Ok(url)
}

pub fn select(store: &Store, id: Option<&str>) -> Result<(), String> {
    if let Some(id) = id {
        destination(store, id, None)?;
    }
    store.save_queue_selection(id)
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn feedback_history_reads_do_not_grow_with_queue_size() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().into());
        let jobs: Vec<QueueJob> = (1..=64)
            .map(|number| {
                serde_json::from_value(serde_json::json!({
                    "provider": "github", "account_id": "22", "account_login": "operator",
                    "configuration_id": "fixture", "repository_id": "100",
                    "repository_name": "example/repo", "pull_request_id": number.to_string(),
                    "number": number, "title": "Change", "head_sha": "a".repeat(40),
                    "trigger_policy": "fixture", "author_id": "11", "author_login": "author",
                    "watched_author": true, "requested_reviewer": false,
                    "waiting": "human_start", "detected_at": 100
                }))
                .unwrap()
            })
            .collect();
        store.save_queue(&jobs[..1]).unwrap();
        store.take_state_reads();
        let single = normal_snapshot(&store, vec![]).unwrap();
        assert_eq!(single.feedback.len(), 1);
        let single_reads = store.take_state_reads();

        store.save_queue(&jobs).unwrap();
        store.take_state_reads();
        let many = normal_snapshot(&store, vec![]).unwrap();
        assert_eq!(many.feedback.len(), 64);
        let many_reads = store.take_state_reads();
        for name in [
            "feedback.json",
            "reviews.json",
            "publications.json",
            "follow-ups.json",
        ] {
            assert_eq!(
                many_reads.get(name),
                single_reads.get(name),
                "{name} must be loaded a fixed number of times per snapshot"
            );
        }
    }
}
