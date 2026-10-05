pub(crate) mod host;
mod plan;

use crate::{
    monitoring::{JobOperation, OperationFailure, OperationState, QueueJob},
    review::{Failure, ReviewRun, Selection},
    storage::{Settings, Store},
};
pub use plan::{Batch, InlineComment};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Pending,
    Published,
    Stale,
    StaleAfterPublication,
    Stopped,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutation {
    Create,
    Submit,
    Discard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteState {
    Pending,
    Commented,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub review_id: String,
    pub state: RemoteState,
    pub comment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub id: String,
    pub review: ReviewRun,
    pub operation: JobOperation,
    pub history: Vec<JobOperation>,
    pub automatic: bool,
    pub confirmed: bool,
    pub cancelled: bool,
    pub phase: Phase,
    pub error: Option<String>,
    pub batch: Option<Batch>,
    pub mutation: Option<Mutation>,
    pub uncertain: bool,
    pub receipts: Vec<Receipt>,
}

impl Publication {
    pub fn new(
        review: ReviewRun,
        automatic: bool,
        confirmed: bool,
        now: i64,
    ) -> Result<Self, String> {
        if review.operation.state != OperationState::Completed
            || review
                .result
                .as_ref()
                .and_then(|r| r.reviewed_base_sha.as_ref())
                .is_none()
        {
            return Err(
                "Run a new review before publication; this result has no reviewed base revision."
                    .into(),
            );
        }
        if !automatic && !confirmed {
            return Err("Explicit publication confirmation is required.".into());
        }
        let mut operation = JobOperation::review(&review.job, now);
        operation.operation_type = "github_comment".into();
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            review,
            operation,
            history: vec![],
            automatic,
            confirmed,
            cancelled: false,
            phase: Phase::Preparing,
            error: None,
            batch: None,
            mutation: None,
            uncertain: false,
            receipts: vec![],
        })
    }

    pub fn retry(&mut self, automatic: bool, now: i64) -> Result<(), String> {
        if self.phase == Phase::Stale
            || self
                .receipts
                .last()
                .is_some_and(|r| r.state == RemoteState::Deleted)
            || self.operation.state == OperationState::Completed
        {
            return Err("This publication is terminal; it cannot create another batch.".into());
        }
        self.history.push(self.operation.clone());
        let mut operation = JobOperation::review(&self.review.job, now);
        operation.operation_type = "github_comment".into();
        operation.attempted_mutation = self.operation.attempted_mutation.clone();
        operation.pending_review_id = self.operation.pending_review_id.clone();
        operation.confirmed_receipt = self.operation.confirmed_receipt.clone();
        self.operation = operation;
        self.automatic = automatic;
        self.confirmed = true;
        self.cancelled = false;
        self.error = None;
        // Preserve the publication identity and uncertain intent across new retry budgets.
        Ok(())
    }

    pub fn marker(&self) -> String {
        format!("<!-- pr-sniper:publication:{} -->", self.id)
    }

    pub fn conflicts_with(&self, review: &ReviewRun) -> bool {
        self.review.key == review.key
            && self.review.operation.id != review.operation.id
            && self.reserves_revision()
    }

    pub fn reserves_revision(&self) -> bool {
        self.uncertain
            || self
                .receipts
                .last()
                .is_some_and(|r| r.state != RemoteState::Deleted)
            || (!self.cancelled
                && matches!(
                    self.operation.state,
                    OperationState::Queued | OperationState::Running | OperationState::Interrupted
                ))
    }

    fn accept(&mut self, receipt: Receipt) -> Result<(), Failure> {
        if self
            .operation
            .pending_review_id
            .as_ref()
            .is_some_and(|id| id != &receipt.review_id)
        {
            return Err(Failure::permanent(
                "GitHub returned a different review identity.",
            ));
        }
        self.operation.pending_review_id = Some(receipt.review_id.clone());
        self.operation.confirmed_receipt = Some(
            serde_json::to_string(&receipt)
                .map_err(|_| Failure::permanent("Cannot encode publication receipt."))?,
        );
        self.phase = match receipt.state {
            RemoteState::Pending => Phase::Pending,
            RemoteState::Commented => Phase::Published,
            RemoteState::Deleted => Phase::Stopped,
        };
        if self.receipts.last() != Some(&receipt) {
            self.receipts.push(receipt);
        }
        self.uncertain = false;
        Ok(())
    }
}

pub fn automatic_policy(
    settings: &Settings,
    review: &ReviewRun,
    job: &QueueJob,
) -> Result<bool, String> {
    let current = Selection::resolve(settings, job, &review.assignment_id)?;
    if current.agent != review.selection.agent
        || current.doctrine != review.selection.doctrine
        || current.preset != review.selection.preset
    {
        return Err("The review lens changed; run a new review before publishing.".into());
    }
    let assignment = settings
        .repositories
        .iter()
        .find(|r| r.id == job.configuration_id)
        .and_then(|r| r.assignments.iter().find(|a| a.id == review.assignment_id))
        .ok_or("The review assignment was removed.")?;
    if !assignment.comment {
        return Err("Comments are disabled for this Agent assignment.".into());
    }
    Ok(current.policy.automatic_comment_publication)
}

pub fn evaluate_gate(
    settings: &Settings,
    run: &Publication,
    current_job: Option<&QueueJob>,
    pull: &crate::github::metadata::PullRequest,
    active: bool,
    can_comment: bool,
    cancelled: bool,
) -> Gate {
    evaluate_review_gate(
        settings,
        &run.review,
        current_job,
        pull,
        GatePermissions {
            active,
            can_comment,
            cancelled,
            automatic: run.automatic,
            confirmed: run.confirmed,
        },
    )
}

pub struct GatePermissions {
    pub active: bool,
    pub can_comment: bool,
    pub cancelled: bool,
    pub automatic: bool,
    pub confirmed: bool,
}

pub fn evaluate_review_gate(
    settings: &Settings,
    review: &ReviewRun,
    current_job: Option<&QueueJob>,
    pull: &crate::github::metadata::PullRequest,
    permissions: GatePermissions,
) -> Gate {
    let GatePermissions {
        active,
        can_comment,
        cancelled,
        automatic,
        confirmed,
    } = permissions;
    let head_changed = pull.head_sha != review.job.head_sha;
    let stale = head_changed
        || review
            .result
            .as_ref()
            .and_then(|r| r.reviewed_base_sha.as_ref())
            != Some(&pull.base_sha);
    let requeue = head_changed
        && active
        && crate::monitoring::new_revision_eligible(settings, &review.job, pull);
    let policy = current_job
        .ok_or_else(|| "Review detection is no longer available.".to_string())
        .and_then(|job| automatic_policy(settings, review, job));
    let mut stop = match policy {
        Err(error) => Some(error),
        Ok(current) if current != automatic => {
            Some("The publication gate changed; confirm again before retrying.".into())
        }
        _ => None,
    };
    if cancelled || (!confirmed && !automatic) {
        stop = Some("Publication confirmation was withdrawn.".into());
    } else if !active {
        stop = Some("Monitoring scope is no longer active.".into());
    }
    if stop.is_none() {
        stop = crate::monitoring::review_policy(
            settings,
            current_job.unwrap_or(&review.job),
            Some(pull),
        )
        .err();
    }
    if stop.is_none() && stale {
        stop = Some("The reviewed diff changed; this output is stale.".into());
    }
    if stop.is_none() && !can_comment {
        stop = Some("The acting GitHub account cannot publish to this repository.".into());
    }
    Gate {
        stop,
        stale,
        requeue,
    }
}

pub fn restore(store: &Store) -> Result<(), String> {
    let mut publications = store.load_publications()?;
    let mut changed = false;
    for publication in &mut publications {
        if publication.operation.state == OperationState::Running {
            publication.operation.state = OperationState::Interrupted;
            publication.operation.next_attempt_at = Some(0);
            changed = true;
        }
    }
    if changed {
        store.save_publications(&publications)?;
    }
    Ok(())
}

pub struct Gate {
    pub stop: Option<String>,
    pub stale: bool,
    pub requeue: bool,
}

pub struct WriteFailure {
    pub failure: Failure,
    pub uncertain: bool,
}

// The host owns credentials and live gates; the state machine owns effect ordering.
pub trait Environment {
    fn paused(&self) -> Result<bool, Failure> {
        Ok(false)
    }
    fn now(&self) -> Result<i64, Failure>;
    fn save(&mut self, publication: &mut Publication) -> Result<(), Failure>;
    fn inspect(&mut self, publication: &Publication) -> Result<Gate, Failure>;
    fn prepare(&mut self, publication: &Publication) -> Result<Batch, Failure>;
    fn reconcile(&mut self, publication: &Publication) -> Result<Option<Receipt>, Failure>;
    fn mutate(
        &mut self,
        publication: &Publication,
        mutation: Mutation,
    ) -> Result<Receipt, WriteFailure>;
    fn requeue(&mut self, publication: &Publication) -> Result<(), Failure>;
}

pub fn execute(
    environment: &mut impl Environment,
    publication: &mut Publication,
) -> Result<(), Failure> {
    if publication.operation.state == OperationState::Completed {
        return Err(Failure::permanent("This publication is already complete."));
    }
    if let Err(error) = publication.operation.begin_attempt(environment.now()?) {
        publication.error = Some(error.clone());
        environment.save(publication)?;
        return Err(Failure::permanent(error));
    }
    let outcome = (|| {
        publication.error = None;
        environment.save(publication)?;
        drive(environment, publication)
    })();
    if let Err(error) = outcome {
        publication
            .operation
            .fail(&error.monitoring(), environment.now()?);
        publication.error = Some(error.message.clone());
        if publication.uncertain {
            publication.phase = Phase::Unresolved;
        }
        environment.save(publication)?;
        return Err(error);
    }
    environment.save(publication)
}

fn drive(env: &mut impl Environment, run: &mut Publication) -> Result<(), Failure> {
    if defer_paused(env, run)? {
        return Ok(());
    }
    let gate = env.inspect(run)?;
    if run.batch.is_none() {
        if gate.stop.is_some() {
            return stop(env, run, gate);
        }
        run.batch = Some(match env.prepare(run) {
            Ok(batch) => batch,
            Err(error) => {
                let current = env.inspect(run)?;
                if current.stop.is_some() {
                    return stop(env, run, current);
                }
                return Err(error);
            }
        });
        env.save(run)?;
    }
    let remote = env.reconcile(run)?;
    if let Some(receipt) = remote {
        run.accept(receipt)?;
        env.save(run)?;
    } else if run.uncertain || !run.receipts.is_empty() {
        if run.mutation == Some(Mutation::Discard) && run.operation.pending_review_id.is_some() {
            run.accept(Receipt {
                review_id: run.operation.pending_review_id.clone().unwrap(),
                state: RemoteState::Deleted,
                comment_ids: vec![],
            })?;
            env.save(run)?;
            return stop(env, run, gate);
        }
        run.uncertain = true;
        return Err(unresolved());
    }
    let gate = env.inspect(run)?;
    if run
        .receipts
        .last()
        .is_some_and(|r| r.state == RemoteState::Commented)
    {
        return complete(env, run, gate);
    }
    if run.mutation == Some(Mutation::Discard) {
        return stop(env, run, gate);
    }
    if gate.stop.is_some()
        || run
            .receipts
            .last()
            .is_some_and(|r| r.state == RemoteState::Deleted)
    {
        return stop(env, run, gate);
    }
    if run.operation.pending_review_id.is_none() {
        effect(env, run, Mutation::Create)?;
        if defer_paused(env, run)? {
            return Ok(());
        }
        let gate = env.inspect(run)?;
        if gate.stop.is_some() {
            return stop(env, run, gate);
        }
    }
    // Re-read the entire owned pending batch, including comments, before submitting.
    let receipt = env.reconcile(run)?.ok_or_else(unresolved)?;
    run.accept(receipt)?;
    env.save(run)?;
    let gate = env.inspect(run)?;
    if run
        .receipts
        .last()
        .is_some_and(|r| r.state == RemoteState::Commented)
    {
        return complete(env, run, gate);
    }
    if gate.stop.is_some() {
        return stop(env, run, gate);
    }
    effect(env, run, Mutation::Submit)?;
    if defer_paused(env, run)? {
        return Ok(());
    }
    let gate = env.inspect(run)?;
    complete(env, run, gate)
}

fn defer_paused(env: &mut impl Environment, run: &mut Publication) -> Result<bool, Failure> {
    if !env.paused()? {
        return Ok(false);
    }
    run.operation.state = OperationState::Interrupted;
    run.operation.next_attempt_at = Some(env.now()?);
    run.error =
        Some("Automation paused; original provider state will be reconciled after resume.".into());
    env.save(run)?;
    Ok(true)
}

fn effect(
    env: &mut impl Environment,
    run: &mut Publication,
    mutation: Mutation,
) -> Result<(), Failure> {
    if env.now()? >= run.operation.retry_deadline {
        return Err(Failure::timeout());
    }
    run.mutation = Some(mutation);
    run.operation.attempted_mutation = Some(
        serde_json::to_string(&mutation)
            .map_err(|_| Failure::permanent("Cannot persist mutation intent."))?,
    );
    run.uncertain = true;
    env.save(run)?;
    let outcome = env.mutate(run, mutation);
    match outcome {
        Ok(receipt) => {
            run.accept(receipt)?;
            env.save(run)?;
            Ok(())
        }
        Err(error) => {
            run.uncertain = error.uncertain;
            env.save(run)?;
            // Even rejected/uncertain effects get a post-mutation eligibility observation.
            match env.inspect(run) {
                Ok(gate) => {
                    if gate.stop.is_some() && !run.uncertain && mutation != Mutation::Discard {
                        stop(env, run, gate)?;
                    } else if gate.requeue {
                        env.requeue(run)?;
                    }
                    Err(error.failure)
                }
                Err(observation) => Err(Failure {
                    message: format!(
                        "{} Post-mutation verification also failed: {}",
                        error.failure.message, observation.message
                    ),
                    ..error.failure
                }),
            }
        }
    }
}

fn stop(env: &mut impl Environment, run: &mut Publication, gate: Gate) -> Result<(), Failure> {
    if defer_paused(env, run)? {
        return Ok(());
    }
    if run
        .receipts
        .last()
        .is_some_and(|r| r.state == RemoteState::Pending)
        && !run.uncertain
    {
        // Cleanup is not a grant to publish: only this exact owned pending review can be deleted.
        env.inspect(run)?;
        if defer_paused(env, run)? {
            return Ok(());
        }
        effect(env, run, Mutation::Discard)?;
        env.inspect(run)?;
        if run
            .receipts
            .last()
            .is_some_and(|r| r.state == RemoteState::Commented)
        {
            return complete(env, run, gate);
        }
    }
    run.phase = if gate.stale {
        Phase::Stale
    } else {
        Phase::Stopped
    };
    run.error = Some(gate.stop.unwrap_or_else(|| {
        "Pending review was discarded; no repeat publication is allowed.".into()
    }));
    run.operation.state = OperationState::Failed;
    run.operation.failure = Some(OperationFailure::Permanent);
    run.operation.next_attempt_at = None;
    env.save(run)?;
    if gate.requeue {
        env.requeue(run)?;
    }
    Ok(())
}

fn complete(env: &mut impl Environment, run: &mut Publication, gate: Gate) -> Result<(), Failure> {
    if run
        .receipts
        .last()
        .is_none_or(|r| r.state != RemoteState::Commented)
    {
        return Err(unresolved());
    }
    run.phase = if gate.stop.is_some() {
        Phase::StaleAfterPublication
    } else {
        Phase::Published
    };
    run.error = gate.stop;
    run.operation.state = OperationState::Completed;
    run.operation.next_attempt_at = None;
    run.operation.failure = None;
    env.save(run)?;
    if gate.requeue {
        env.requeue(run)?;
    }
    Ok(())
}

fn unresolved() -> Failure {
    Failure {
        cancelled: false,
        message: "GitHub mutation outcome is unresolved. Reconcile the original review; no replacement batch will be created.".into(),
        kind: OperationFailure::Network,
        retry_after_seconds: None,
    }
}
