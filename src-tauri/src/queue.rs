use crate::{
    follow_up,
    monitoring::{self, OperationState, QueueJob, ScheduleHealth},
    publication::{self, RemoteState},
    review::{self, Decision, Selection},
    storage::{Settings, Store},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    // Safety blockers dominate aggregation; presentation uses a separate priority.
    StaleAfterPublication,
    Stale,
    Failed,
    Blocked,
    WaitingForHuman,
    ConfirmationRequired,
    Reviewing,
    Queued,
    AwaitingPublication,
    WaitingForAuthor,
    MachineSignedOff,
    Closed,
    Merged,
}

impl State {
    fn priority(self) -> u8 {
        match self {
            Self::MachineSignedOff | Self::WaitingForHuman => 0,
            Self::Failed | Self::Blocked | Self::ConfirmationRequired => 1,
            Self::StaleAfterPublication => 2,
            Self::Reviewing | Self::Queued | Self::AwaitingPublication => 3,
            Self::WaitingForAuthor => 4,
            Self::Stale => 5,
            Self::Closed | Self::Merged => 6,
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::MachineSignedOff => "Automated review completed. Ready for your final review and merge decision on GitHub; this is not human approval or a merge.",
            Self::WaitingForAuthor => "Findings or questions were published. The PR author needs to respond or update the change; it is not machine-cleared.",
            Self::WaitingForHuman => "Your attention is needed. Inspect the local findings or conversation; do not assume the PR author has been notified.",
            Self::ConfirmationRequired => "Your confirmation is required before review or publication can proceed. Open evidence and actions.",
            Self::Reviewing => "Automated review or thread analysis is in progress. No final handoff yet.",
            Self::Queued => "Waiting for assigned review work. Every current assignment must finish before this PR can be machine-cleared.",
            Self::AwaitingPublication => "Review output is local; comment publication is pending. No completed handoff yet.",
            Self::Failed => "An operation failed or its remote outcome is unresolved. Inspect the original operation and retry or reconcile; a local sign-off does not clear this failure.",
            Self::Blocked => "Current configuration or access prevents a completed handoff. Inspect evidence, schedule health and Settings.",
            Self::Stale => "This review no longer establishes readiness for the current PR or configuration. Check the latest revision; saved evidence remains available.",
            Self::StaleAfterPublication => "Comments were published, but the reviewed state is now stale. They do not establish readiness for the current PR.",
            Self::Closed => "GitHub reports this PR closed. This iteration is terminal; saved evidence is retained.",
            Self::Merged => "GitHub reports this PR merged. This is provider lifecycle evidence, not a claim that PR Sniper merged it.",
        }
    }
}

#[derive(Serialize)]
pub struct Item {
    pub action_status: Option<crate::actions::Status>,
    pub feedback: Vec<crate::feedback::View>,
    pub id: String,
    pub aliases: Vec<String>,
    pub job: QueueJob,
    pub state: State,
    pub summary: &'static str,
    pub review_keys: Vec<String>,
    pub follow_up_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub feedback: BTreeMap<String, Vec<crate::feedback::View>>,
    pub mentions: Vec<crate::feedback::Mention>,
    pub pending_threads: Vec<crate::feedback::PendingThread>,
    pub global_scan: Option<monitoring::GlobalScan>,
    pub tracked: Vec<monitoring::TrackedPullRequest>,
    pub health: Vec<ScheduleHealth>,
    pub jobs: Vec<QueueJob>,
    pub(crate) reviews: Vec<review::host::Candidate>,
    pub(crate) publications: Vec<publication::host::Candidate>,
    pub(crate) follow_ups: Vec<follow_up::host::Candidate>,
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
    Ok(snapshot)
}

pub(crate) fn normal_snapshot(
    store: &Store,
    health: Vec<ScheduleHealth>,
) -> Result<Snapshot, String> {
    let settings = store.load_settings()?;
    let monitoring = store.load_monitoring_state()?;
    let health = if health.is_empty() {
        monitoring.health.into_values().collect()
    } else {
        health
    };
    let mut result = Snapshot {
        feedback: BTreeMap::new(),
        mentions: store.load_feedback()?.mentions,
        pending_threads: store.load_feedback()?.pending_threads,
        global_scan: monitoring.global_scan,
        tracked: store.load_queue_state()?.tracked,
        health,
        jobs: store.load_queue()?,
        reviews: review::host::candidates(store)?,
        publications: publication::host::candidates(store)?,
        follow_ups: follow_up::host::candidates(store)?,
        items: vec![],
    };
    for job in &result.jobs {
        result
            .feedback
            .insert(item_id(job), crate::feedback::views(store, job)?);
    }
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

fn failed(operation: &monitoring::JobOperation) -> bool {
    matches!(
        operation.state,
        OperationState::Failed | OperationState::ManualRetry
    )
}

fn review_state(
    settings: &Settings,
    candidate: &review::host::Candidate,
    publications: &[publication::host::Candidate],
    feedback: &[crate::feedback::View],
) -> State {
    let batch = candidate.run.as_ref().and_then(|run| {
        publications
            .iter()
            .find(|p| p.review_operation_id == run.operation.id)
    });
    let publication = batch.and_then(|p| p.publication.as_ref());
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
    if candidate.blocked.is_some() {
        return State::Blocked;
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
    let Ok(current) = Selection::resolve(settings, &candidate.job, &candidate.assignment_id) else {
        return State::Blocked;
    };
    if current.agent != run.selection.agent
        || current.doctrine != run.selection.doctrine
        || current.preset != run.selection.preset
        || current.policy.prompt != run.selection.policy.prompt
        || current.policy.adapter != run.selection.policy.adapter
        || result.reviewed_base_sha.is_none()
    {
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
    if publication.is_none()
        && batch
            .and_then(|p| p.blocked.as_deref())
            .is_some_and(|reason| reason != "Comments are disabled for this Agent assignment.")
    {
        return State::Blocked;
    }
    let comments = settings
        .repositories
        .iter()
        .find(|r| r.id == candidate.job.configuration_id)
        .and_then(|r| {
            r.assignments
                .iter()
                .find(|a| a.id == candidate.assignment_id)
        })
        .is_some_and(|a| a.comment);
    if comments || publication.is_some() {
        if batch.is_some_and(|p| p.blocked.is_some()) {
            return State::Blocked;
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
        return Some(State::Blocked);
    }
    if candidate.human_gate || run.phase == Phase::HumanInputRequired {
        return Some(State::WaitingForHuman);
    }
    match run.phase {
        Phase::WaitingStart => Some(State::Queued),
        Phase::Analyzing => Some(State::Reviewing),
        Phase::WaitingPublication => Some(if candidate.automatic_publication {
            State::AwaitingPublication
        } else {
            State::ConfirmationRequired
        }),
        Phase::Publishing => Some(State::AwaitingPublication),
        Phase::Stopped | Phase::Unresolved => Some(State::Failed),
        Phase::Quiet | Phase::Published => None,
        Phase::HumanInputRequired | Phase::StaleAfterPublication => unreachable!(),
    }
}

fn project(settings: &Settings, snapshot: &Snapshot) -> Vec<Item> {
    let current = |job| current_job(job, &snapshot.jobs);
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
                {
                    *previous = job;
                }
            })
            .or_insert(job);
    }
    let mut items = Vec::new();
    for (id, job) in groups {
        let reviews: Vec<_> = snapshot
            .reviews
            .iter()
            .filter(|r| item_id(&r.job) == id)
            .collect();
        let follow_ups: Vec<_> = snapshot
            .follow_ups
            .iter()
            .filter(|f| crate::feedback::same_pr(&f.run.context.job, job))
            .collect();
        let mut states: Vec<_> = reviews
            .iter()
            .filter(|r| r.job.waiting != monitoring::WAITING_ASSIGNMENT_REMOVED)
            .map(|r| {
                review_state(
                    settings,
                    r,
                    &snapshot.publications,
                    snapshot.feedback.get(&id).map(Vec::as_slice).unwrap_or(&[]),
                )
            })
            .collect();
        let mut warnings = BTreeSet::new();
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
                && follow_up.run.publication.is_none()
                && !follow_up.run.uncertain
                && follow_up.run.phase != follow_up::Phase::HumanInputRequired
            {
                continue;
            }
            if follow_up.run.thread().is_ok() {
                let feedback = snapshot.feedback.get(&id).and_then(|values| {
                    values
                        .iter()
                        .find(|f| follow_up.run.owns_feedback(&f.context))
                });
                let needs_reconciliation = follow_up.run.uncertain
                    || follow_up
                        .run
                        .publication
                        .as_ref()
                        .is_some_and(|op| op.state != OperationState::Completed);
                if !needs_reconciliation
                    && feedback.is_some_and(|f| {
                        f.context.closed
                            || f.context
                                .thread
                                .as_ref()
                                .and_then(|t| t.latest_external(&job.account_id))
                                .is_some_and(|c| c.id != follow_up.run.trigger_id)
                    })
                {
                    continue;
                }
            }
            if follow_up.run.cancelled
                && follow_up.run.result.is_none()
                && follow_up.run.publication.is_none()
            {
                continue;
            }
            if let Some(state) = follow_up_state(follow_up) {
                states.push(state);
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
                "unavailable" | "owner_unavailable" => states.push(State::Blocked),
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
                    states.push(State::Blocked);
                    warnings.insert(reason.into());
                }
            }
            if mention.follow_up_id.is_none() {
                states.push(State::Blocked);
                warnings.insert("Observed mention is awaiting durable execution admission.".into());
            }
            if let Some(reason) = &mention.blocked {
                states.push(State::Blocked);
                warnings.insert(reason.clone());
            }
            if mention
                .follow_up_id
                .as_ref()
                .is_some_and(|id| !snapshot.follow_ups.iter().any(|f| &f.run.id == id))
            {
                states.push(State::Blocked);
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
                states.push(State::Blocked);
                warnings.insert(intent.blocked.clone().unwrap_or_else(|| "Observed discussion awaits durable primary assessment; no clearance inferred.".into()));
            }
        }
        for publication in snapshot
            .publications
            .iter()
            .filter_map(|p| p.publication.as_ref())
            .filter(|p| item_id(current(&p.review.job)) == id)
        {
            if let Some(error) = &publication.error {
                warnings.insert(error.clone());
            }
            if publication.uncertain {
                warnings.insert("Publication outcome unresolved; reconcile the original batch, never a replacement.".into());
                states.push(State::Failed);
            }
        }
        if obsolete(job) {
            let published = snapshot
                .publications
                .iter()
                .filter_map(|p| p.publication.as_ref())
                .any(|p| {
                    item_id(current(&p.review.job)) == id
                        && p.receipts
                            .last()
                            .is_some_and(|r| r.state == RemoteState::Commented)
                })
                || follow_ups.iter().any(|f| f.run.receipt.is_some());
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
            if monitoring::review_policy(settings, job, None).is_err() {
                states.push(State::Blocked);
            }
            for health in snapshot.health.iter().filter(|h| {
                h.repository_id == job.configuration_id
                    && h.provider_account_id.as_deref() == Some(&job.account_id)
            }) {
                if health.conversation_admission_pending {
                    states.push(State::Blocked);
                    warnings.insert("Conversation observations are not fully admitted; retry the repository check before relying on readiness.".into());
                }
                if let Some(failure) = &health.last_failure {
                    states.push(State::Failed);
                    warnings.insert(format!("Monitoring failure: {failure}. Check schedule health before relying on this saved revision."));
                }
            }
        }
        let state = match job.waiting.as_str() {
            monitoring::WAITING_CLOSED => State::Closed,
            monitoring::WAITING_MERGED => State::Merged,
            _ => states.into_iter().min().unwrap_or(State::Blocked),
        };
        items.push(Item {
            action_status: None,
            feedback,
            aliases: snapshot
                .jobs
                .iter()
                .filter(|j| item_id(j) == id)
                .filter_map(|j| j.work.as_ref()?.legacy_item_id.clone())
                .collect(),
            id,
            job: job.clone(),
            state,
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

fn current_job<'a>(job: &'a QueueJob, jobs: &'a [QueueJob]) -> &'a QueueJob {
    let Some(assignment_id) = &job.assignment_id else {
        return job;
    };
    jobs.iter()
        .find(|j| {
            j.assignment_id == job.assignment_id
                && review::key(j, assignment_id) == review::key(job, assignment_id)
        })
        .unwrap_or(job)
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
