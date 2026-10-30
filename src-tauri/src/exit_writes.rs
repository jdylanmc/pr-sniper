//! Host execution of a Packet-mode `leave_comments` exit's replies and own-thread
//! resolutions. The agent never writes; it requested, the host verifies and executes.
//!
//! Sequence (one exit, one ordered effect): preflight (read-only, every item verified,
//! nothing frozen until all pass) -> publication batch of `new_comments` (existing
//! pipeline, held until preflight passes) -> replies -> resolutions -> confirmed.
//! Any preflight failure tosses the whole exit before the first effect. Each step has
//! an intent saved before its effect and a marker (replies) or idempotent mutation
//! (resolutions) so a crash reconciles instead of duplicating.
//!
//! Why not `FollowUp`: it carries a validated analysis result (model, evidence), a queue
//! kind, primary-only authority and owned-only targets. Pre-filling it means forging
//! that analysis; a direct ledger path keeps the same intent-before-effect and
//! reconcile rules with far less coupling. Legacy follow-ups still load and drain.

pub(crate) mod host;

use crate::{
    exits::ExitOutcome,
    github::threads::{Ownership, Thread},
    monitoring::{JobOperation, OperationState, QueueJob},
    publication::{Phase as PublicationPhase, Publication, RemoteState, WriteFailure},
    review::{Failure, ReviewRun},
    storage::Store,
};
use serde::{Deserialize, Serialize};

/// Default attribution on every machine-authored review body and reply. U+1F52B has
/// default emoji presentation, so it renders on GitHub and Azure DevOps without U+FE0F.
pub const SIGNATURE: &str = "\u{1F52B} PR Sniper automated review";
/// Earlier signature (a Private Use Area icon-font glyph); still recognized as ours.
pub const LEGACY_SIGNATURE: &str = "\u{f05b} PR Sniper";
const MAX_BODY_CHARS: usize = 65_536;
/// Delay before a write waiting on its comment batch checks again.
const WAIT_SECONDS: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Reply,
    Resolve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Planned,
    /// Intent saved; the effect may or may not have landed.
    Attempted,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub kind: StepKind,
    pub thread_id: String,
    /// Agent text until preflight; the frozen provider body afterwards (replies only).
    pub body: String,
    pub root_id: Option<String>,
    /// Local iteration of the thread's own commit, or `unknown` (AC-041).
    pub revision: Option<String>,
    pub state: StepState,
    pub receipt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_evidence: Option<crate::review::findings::Assessment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Frozen request, nothing verified or written.
    Planned,
    /// Preflight passed; writes are authorized.
    Verified,
    /// Every effect confirmed.
    Completed,
    /// Refused before any effect; the whole exit is discarded.
    Tossed,
    /// Stopped after at least one effect; never retried blindly, never re-posted.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExitWrite {
    pub id: String,
    pub review_operation_id: String,
    pub job: QueueJob,
    pub assignment_id: String,
    /// The exit also has `new_comments` or a review `body`: the publication batch goes first.
    pub comments_first: bool,
    pub published: bool,
    pub steps: Vec<Step>,
    pub phase: Phase,
    pub operation: JobOperation,
    pub uncertain: bool,
    pub error: Option<String>,
    /// The consume/baseline hook ran after the confirmed exit.
    pub consumed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_base_sha: Option<String>,
    /// A primary `lgtm` sign-off: its approve or merge final may adopt the state this
    /// write consumed (`actions::packet::adopt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_off: Option<crate::exits::LgtmAction>,
}

impl ExitWrite {
    pub fn effects(&self) -> bool {
        self.published
            || self
                .steps
                .iter()
                .any(|s| matches!(s.state, StepState::Attempted | StepState::Done))
    }

    pub fn terminal(&self) -> bool {
        matches!(self.phase, Phase::Tossed | Phase::Failed)
            || (self.phase == Phase::Completed && self.consumed)
    }

    pub fn reply_marker(&self, index: usize) -> String {
        format!("<!-- pr-sniper:reply:exit-{}-{index} -->", self.id)
    }

    fn request_id(&self, index: usize) -> String {
        format!("exit-{}-{index}", self.id)
    }

    pub fn reply_body(&self, index: usize, text: &str, revision: &str) -> String {
        format!(
            "{}\n\n{}\n<!-- pr-sniper:revision:{revision} -->\n\n{SIGNATURE}",
            text.trim_end(),
            self.reply_marker(index)
        )
    }
}

/// Plan the replies and resolutions of a completed run. `None` when the exit has none.
pub fn plan(review: &ReviewRun, now: i64) -> Option<ExitWrite> {
    if review.operation.state != OperationState::Completed {
        return None;
    }
    if let Some(ExitOutcome::Lgtm {
        resolve_own_threads,
        action,
        ..
    }) = review.result.as_ref()?.output.exit.as_ref()
    {
        return sign_off(review, resolve_own_threads, *action, now);
    }
    let Some(ExitOutcome::Comments {
        new_comments,
        reply_comments,
        resolve_own_threads,
        ..
    }) = review.result.as_ref()?.output.exit.as_ref()
    else {
        return None;
    };
    if reply_comments.is_empty() && resolve_own_threads.is_empty() {
        return None;
    }
    let mut steps: Vec<Step> = reply_comments
        .iter()
        .map(|reply| Step {
            kind: StepKind::Reply,
            thread_id: reply.thread_id.clone(),
            body: reply.body.clone(),
            root_id: None,
            revision: None,
            state: StepState::Planned,
            receipt: None,
            resolution_evidence: None,
        })
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let result = review.result.as_ref()?;
    let duck_required = review
        .selection
        .configuration
        .as_ref()
        .is_some_and(|c| c.rubber_duck.is_some());
    steps.extend(
        resolve_own_threads
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .map(|id| Step {
                kind: StepKind::Resolve,
                thread_id: id.clone(),
                body: String::new(),
                root_id: None,
                revision: None,
                state: StepState::Planned,
                receipt: None,
                resolution_evidence: result
                    .finding_dispositions
                    .iter()
                    .find(|a| {
                        a.thread_id == *id
                            && (a.disposition
                                == crate::review::findings::Disposition::AuthorWontFix
                                || !duck_required
                                || review
                                    .packet
                                    .as_ref()
                                    .is_some_and(|p| a.duck_confirms(&p.state_id, &result.ducks)))
                    })
                    .cloned(),
            }),
    );
    let mut operation = JobOperation::review(&review.job, now);
    operation.operation_type = "exit_writes".into();
    Some(ExitWrite {
        id: uuid::Uuid::new_v4().to_string(),
        review_operation_id: review.operation.id.clone(),
        job: review.job.clone(),
        assignment_id: review.assignment_id.clone(),
        comments_first: !new_comments.is_empty(),
        published: false,
        steps,
        phase: Phase::Planned,
        operation,
        uncertain: false,
        error: None,
        consumed: false,
        reviewed_base_sha: review.result.as_ref()?.reviewed_base_sha.clone(),
        sign_off: None,
    })
}

/// A sign-off's resolution of each accounted own open thread. Silent, never status
/// chatter: only an accepted author won't-fix gets its acceptance reply first,
/// answering the author like `leave_comments` must.
fn sign_off(
    review: &ReviewRun,
    threads: &[String],
    action: crate::exits::LgtmAction,
    now: i64,
) -> Option<ExitWrite> {
    use crate::review::findings::Disposition;
    if threads.is_empty() {
        return None;
    }
    let result = review.result.as_ref()?;
    let duck_required = review
        .selection
        .configuration
        .as_ref()
        .is_some_and(|c| c.rubber_duck.is_some());
    let evidence = |id: &String| {
        result
            .finding_dispositions
            .iter()
            .find(|a| {
                a.thread_id == *id
                    && match a.disposition {
                        Disposition::Fixed => {
                            !duck_required
                                || review
                                    .packet
                                    .as_ref()
                                    .is_some_and(|p| a.duck_confirms(&p.state_id, &result.ducks))
                        }
                        Disposition::SignedOff => true,
                        other => other.signs_off(),
                    }
            })
            .cloned()
    };
    let step = |kind, id: &String, body: String| Step {
        kind,
        thread_id: id.clone(),
        body,
        root_id: None,
        revision: None,
        state: StepState::Planned,
        receipt: None,
        resolution_evidence: (kind == StepKind::Resolve).then(|| evidence(id)).flatten(),
    };
    let mut steps: Vec<Step> = threads
        .iter()
        .filter_map(|id| {
            evidence(id)
                .filter(|a| a.disposition == Disposition::AuthorWontFix)
                .map(|a| step(StepKind::Reply, id, a.rationale))
        })
        .collect();
    steps.extend(
        threads
            .iter()
            .map(|id| step(StepKind::Resolve, id, String::new())),
    );
    let mut operation = JobOperation::review(&review.job, now);
    operation.operation_type = "exit_writes".into();
    Some(ExitWrite {
        id: uuid::Uuid::new_v4().to_string(),
        review_operation_id: review.operation.id.clone(),
        job: review.job.clone(),
        assignment_id: review.assignment_id.clone(),
        comments_first: false,
        published: false,
        steps,
        phase: Phase::Planned,
        operation,
        uncertain: false,
        error: None,
        consumed: false,
        reviewed_base_sha: result.reviewed_base_sha.clone(),
        sign_off: Some(action),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dependency {
    Pending,
    Confirmed,
    Failed(String),
}

/// State of the exit's publication batch, from saved publications only.
/// `batch_expected`: the review still has a batch to publish
/// ([`crate::publication::writes_comments`]). A write saved before body-only batches
/// stopped publishing (status chatter) has none coming, so it never waits for one.
pub fn publication_dependency(
    publications: &[Publication],
    review_operation_id: &str,
    batch_expected: bool,
) -> Dependency {
    let Some(p) = publications
        .iter()
        .find(|p| p.review.operation.id == review_operation_id)
    else {
        return if batch_expected {
            Dependency::Pending
        } else {
            Dependency::Confirmed
        };
    };
    if p.phase == PublicationPhase::Published
        && p.operation.state == OperationState::Completed
        && !p.uncertain
        && p.error.is_none()
        && p.receipts
            .last()
            .is_some_and(|r| r.state == RemoteState::Commented)
    {
        return Dependency::Confirmed;
    }
    // Parked or stopped batches never land on their own: the exit is all or nothing.
    match p.phase {
        PublicationPhase::Stale
        | PublicationPhase::Discarded
        | PublicationPhase::Stopped
        | PublicationPhase::StaleAfterPublication => Dependency::Failed(
            "The comment batch was not published; the exit was discarded.".into(),
        ),
        _ if p.operation.parked() => Dependency::Failed(
            "The comment batch was not published; the exit was discarded.".into(),
        ),
        _ => Dependency::Pending,
    }
}

pub trait Environment {
    fn now(&self) -> Result<i64, Failure>;
    fn save(&mut self, write: &mut ExitWrite) -> Result<(), Failure>;
    /// Grants, scope and connection. An error here is a refusal, never a write.
    fn gate(&mut self, write: &ExitWrite) -> Result<(), Failure>;
    fn dependency(&mut self, write: &ExitWrite) -> Result<Dependency, Failure>;
    /// Current head of the pull request.
    fn head(&mut self, write: &ExitWrite) -> Result<String, Failure>;
    /// Saved ownership proofs of every comment batch PR Sniper published to this PR.
    fn proofs(&mut self, write: &ExitWrite) -> Result<Vec<Ownership>, Failure>;
    fn thread(&mut self, write: &ExitWrite, id: &str) -> Result<Option<Thread>, Failure>;
    /// Provider account id of the PR author. Unknown refuses author won't-fix resolutions.
    fn pr_author(&mut self, _write: &ExitWrite) -> Result<Option<String>, Failure> {
        Ok(None)
    }
    /// Local iteration whose head is `commit`, else `unknown`.
    fn revision(&mut self, write: &ExitWrite, commit: Option<&str>) -> String;
    fn reply(
        &mut self,
        write: &ExitWrite,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure>;
    fn resolve(
        &mut self,
        write: &ExitWrite,
        thread_id: &str,
        request_id: &str,
    ) -> Result<(), WriteFailure>;
    /// Undo PR Sniper's own resolution. Unsupported: an uncertain failure.
    fn unresolve(
        &mut self,
        _write: &ExitWrite,
        _thread_id: &str,
        _request_id: &str,
    ) -> Result<(), WriteFailure> {
        Err(WriteFailure {
            failure: Failure::permanent("Unresolving is unavailable."),
            uncertain: true,
        })
    }
    /// The confirmed exit consumes the PR state and re-baselines
    /// (`exits::consume::consume_after`). Runs once; retried when it fails.
    fn confirmed(&mut self, write: &ExitWrite) -> Result<(), Failure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing due: waiting on retry time or the publication batch.
    Waiting,
    Progressed,
}

pub fn run(env: &mut impl Environment, write: &mut ExitWrite) -> Result<Outcome, Failure> {
    match write.phase {
        Phase::Tossed | Phase::Failed => return Ok(Outcome::Progressed),
        Phase::Completed => return finish(env, write).map(|()| Outcome::Progressed),
        _ => (),
    }
    let now = env.now()?;
    // One runner at a time (the mutation owner), so a saved Running state is a crashed attempt.
    if write.operation.state == OperationState::Running {
        write.operation.state = OperationState::Interrupted;
        write.operation.next_attempt_at = Some(now);
    }
    if write.operation.parked() {
        settle(write);
        env.save(write)?;
        return Ok(Outcome::Progressed);
    }
    if write
        .operation
        .next_attempt_at
        .is_some_and(|next| now < next)
    {
        return Ok(Outcome::Waiting);
    }
    if write.operation.begin_attempt(now).is_err() {
        settle(write);
        env.save(write)?;
        return Ok(Outcome::Progressed);
    }
    match advance(env, write) {
        Ok(Outcome::Waiting) => {
            // Waiting spends no attempt, only the retry window: a batch that never
            // lands parks the write (tossed) instead of polling forever.
            write.operation.state = OperationState::Queued;
            write.operation.attempt_count = write.operation.attempt_count.saturating_sub(1);
            write.operation.next_attempt_at = Some(env.now()? + WAIT_SECONDS);
            env.save(write)?;
            Ok(Outcome::Waiting)
        }
        Ok(Outcome::Progressed) => {
            env.save(write)?;
            if write.phase == Phase::Completed {
                finish(env, write)?;
            }
            Ok(Outcome::Progressed)
        }
        Err(error) => {
            write.operation.fail(&error.monitoring(), env.now()?);
            write.error = (!error.rate_limited() || write.uncertain).then(|| error.message.clone());
            settle(write);
            env.save(write)?;
            Err(error)
        }
    }
}

/// A parked write is terminal: tossed when nothing was written, failed otherwise.
fn settle(write: &mut ExitWrite) {
    if write.operation.parked() && !matches!(write.phase, Phase::Completed) {
        write.phase = if write.effects() {
            Phase::Failed
        } else {
            Phase::Tossed
        };
    }
}

fn finish(env: &mut impl Environment, write: &mut ExitWrite) -> Result<(), Failure> {
    if !write.consumed {
        env.confirmed(write)?;
        write.consumed = true;
        env.save(write)?;
    }
    Ok(())
}

fn advance(env: &mut impl Environment, write: &mut ExitWrite) -> Result<Outcome, Failure> {
    env.gate(write)?;
    if write.phase == Phase::Planned {
        preflight(env, write)?;
        write.phase = Phase::Verified;
        write.operation.requeue_fresh(env.now()?);
        write
            .operation
            .begin_attempt(env.now()?)
            .map_err(Failure::permanent)?;
        env.save(write)?;
    }
    if write.comments_first && !write.published {
        match env.dependency(write)? {
            Dependency::Pending => return Ok(Outcome::Waiting),
            Dependency::Failed(reason) => {
                write.error = Some(reason.clone());
                return Err(Failure::permanent(reason));
            }
            Dependency::Confirmed => {
                write.published = true;
                env.save(write)?;
            }
        }
    }
    if !write.effects() {
        // First effect of an exit without a comment batch: the revision must still match.
        if env.head(write)? != write.job.head_sha {
            return Err(Failure::permanent(
                "PR state moved. The exit was discarded.",
            ));
        }
    }
    for index in 0..write.steps.len() {
        if write.steps[index].state != StepState::Done {
            execute(env, write, index)?;
        }
    }
    write.phase = Phase::Completed;
    write.operation.state = OperationState::Completed;
    write.operation.next_attempt_at = None;
    write.operation.failure = None;
    write.error = None;
    write.uncertain = false;
    Ok(Outcome::Progressed)
}

/// Read-only verification of every item. Nothing is frozen unless all items pass.
fn preflight(env: &mut impl Environment, write: &mut ExitWrite) -> Result<(), Failure> {
    if env.head(write)? != write.job.head_sha {
        return Err(Failure::permanent(
            "PR state moved. The exit was discarded.",
        ));
    }
    let proofs = if write.steps.iter().any(|s| s.kind == StepKind::Resolve) {
        own_proofs(env, write)?
    } else {
        Vec::new()
    };
    let mut verified = write.steps.clone();
    for (index, step) in verified.iter_mut().enumerate() {
        let thread = env.thread(write, &step.thread_id)?.ok_or_else(|| {
            Failure::permanent("A thread is not in this pull request. The exit was discarded.")
        })?;
        let root = thread.root().map_err(|_| {
            Failure::permanent("A thread has no verifiable root. The exit was discarded.")
        })?;
        match step.kind {
            StepKind::Reply => {
                if !thread.can_reply {
                    return Err(Failure::permanent(
                        "A thread cannot take a reply. The exit was discarded.",
                    ));
                }
                let revision = env.revision(write, root.original_commit.as_deref());
                let body = write.reply_body(index, &step.body, &revision);
                if body.chars().count() > MAX_BODY_CHARS {
                    return Err(Failure::permanent(
                        "A reply exceeds GitHub's body limit; nothing was truncated.",
                    ));
                }
                step.body = body;
                step.revision = Some(revision);
            }
            StepKind::Resolve => {
                resolution_proof(write, step)?;
                if let Some(evidence) = settled(step) {
                    settled_holds(env, write, evidence, &thread)?;
                }
                if !thread.owned_by_any(&proofs) {
                    return Err(Failure::permanent(
                        "Only threads rooted in this agent's own comments can be resolved. The exit was discarded.",
                    ));
                }
                step.revision = Some(env.revision(write, root.original_commit.as_deref()));
            }
        }
        step.root_id = Some(root.id.clone());
    }
    write.steps = verified;
    Ok(())
}

fn resolution_proof(write: &ExitWrite, step: &Step) -> Result<(), Failure> {
    use crate::review::findings::Disposition;
    let valid = step.resolution_evidence.as_ref().is_some_and(|a| {
        a.thread_id == step.thread_id
            && match a.disposition {
                Disposition::Fixed => !a.evidence.is_empty(),
                Disposition::AuthorWontFix => {
                    a.author_comment_id.is_some()
                        && a.anchor.as_ref().is_some_and(|x| x.author_id.is_some())
                }
                Disposition::SignedOff | Disposition::Withdrawn | Disposition::NoLongerApplies => {
                    write.sign_off.is_some() && a.anchor.is_some()
                }
                _ => false,
            }
            && a.head_sha == write.job.head_sha
            && Some(a.base_sha.as_str()) == write.reviewed_base_sha.as_deref()
            && !a.rationale.trim().is_empty()
            && a.evidence.iter().all(|e| {
                !e.path.trim().is_empty()
                    && e.side.as_deref().unwrap_or("head") == "head"
                    // No line: the whole head file was served (see `evidence_served`).
                    && e.line.map_or(e.end_line.is_none(), |line| {
                        line > 0 && e.end_line.unwrap_or(line) >= line
                    })
            })
    });
    if valid {
        Ok(())
    } else {
        Err(Failure::permanent("Resolution needs a fixed disposition with exact head/base and current-code evidence, a verified author won't-fix, or a disposition the primary's sign-off accounted for; outdated is not proof."))
    }
}

/// Re-read before resolving: the accepted won't-fix snapshot still holds on the
/// provider thread for the current PR author (see `findings::author_reply_holds`).
fn author_reply_holds(
    env: &mut impl Environment,
    write: &ExitWrite,
    evidence: &crate::review::findings::Assessment,
    thread: &Thread,
) -> Result<(), Failure> {
    let author = env.pr_author(write)?;
    let comments: Vec<crate::review::findings::Seen<'_>> =
        thread.comments.iter().map(Into::into).collect();
    if crate::review::findings::author_reply_holds(
        evidence,
        &comments,
        author.as_deref(),
        &write.job.account_id,
    ) {
        Ok(())
    } else {
        Err(Failure::permanent(
            "The accepted won't-fix thread or PR author changed since the review. Nothing was resolved.",
        ))
    }
}

/// Resolution evidence that relies on an accepted thread snapshot (author won't-fix,
/// sign-off) rather than code: re-read before and after the effect.
fn settled(step: &Step) -> Option<&crate::review::findings::Assessment> {
    use crate::review::findings::Disposition;
    step.resolution_evidence.as_ref().filter(|a| {
        matches!(
            a.disposition,
            Disposition::AuthorWontFix
                | Disposition::SignedOff
                | Disposition::Withdrawn
                | Disposition::NoLongerApplies
        )
    })
}

fn settled_holds(
    env: &mut impl Environment,
    write: &ExitWrite,
    evidence: &crate::review::findings::Assessment,
    thread: &Thread,
) -> Result<(), Failure> {
    if evidence.disposition == crate::review::findings::Disposition::AuthorWontFix {
        return author_reply_holds(env, write, evidence, thread);
    }
    let comments: Vec<crate::review::findings::Seen<'_>> =
        thread.comments.iter().map(Into::into).collect();
    let holds = evidence.anchor.as_ref().is_some_and(|anchor| {
        Some(anchor.base_sha.as_str()) == write.reviewed_base_sha.as_deref()
            && crate::review::findings::snapshot_holds(anchor, &comments, |c| {
                crate::review::findings::own_marked(c, &write.job.account_id)
            })
    });
    if holds {
        Ok(())
    } else {
        Err(Failure::permanent(
            "A signed-off thread changed since the review. Nothing was resolved.",
        ))
    }
}

fn own_proofs(env: &mut impl Environment, write: &ExitWrite) -> Result<Vec<Ownership>, Failure> {
    Ok(env
        .proofs(write)?
        .into_iter()
        .filter(|p| {
            p.assignment_id == write.assignment_id
                && p.account_id == write.job.account_id
                && p.repository_id == write.job.repository_id
                && p.pull_request_id == write.job.pull_request_id
                && p.number == write.job.number
        })
        .collect())
}

fn done(write: &mut ExitWrite, index: usize, receipt: String) {
    write.steps[index].state = StepState::Done;
    write.steps[index].receipt = Some(receipt);
    write.uncertain = false;
}

fn found_reply(write: &ExitWrite, thread: &Thread, index: usize) -> Option<String> {
    let marker = write.reply_marker(index);
    thread
        .comments
        .iter()
        .find(|c| {
            c.body.contains(&marker)
                && c.author_id.as_deref() == Some(write.job.account_id.as_str())
        })
        .map(|c| c.id.clone())
}

fn execute(env: &mut impl Environment, write: &mut ExitWrite, index: usize) -> Result<(), Failure> {
    env.gate(write)?;
    // Fence every effect, including resumed writes and steps after a published batch.
    if env.head(write)? != write.job.head_sha {
        return Err(Failure::permanent(
            "PR state moved; nothing further was written.",
        ));
    }
    let step = write.steps[index].clone();
    let thread = env
        .thread(write, &step.thread_id)?
        .ok_or_else(|| Failure::permanent("A thread disappeared before its write."))?;
    if thread.root().ok().map(|r| &r.id) != step.root_id.as_ref() {
        return Err(Failure::permanent(
            "A thread root changed; nothing further was written.",
        ));
    }
    match step.kind {
        StepKind::Reply => {
            if let Some(receipt) = found_reply(write, &thread, index) {
                done(write, index, receipt);
                return env.save(write);
            }
            if step.state == StepState::Attempted {
                // An earlier attempt may still land; never post a replacement.
                return Err(Failure {
                    cancelled: false,
                    kind: crate::monitoring::OperationFailure::Network,
                    message: "Reply outcome is unresolved; no replacement reply will be posted."
                        .into(),
                    retry_after_seconds: None,
                    origin: crate::backpressure::Origin::Github,
                    duck_setup: false,
                    model: None,
                    resume: Default::default(),
                });
            }
            if !thread.can_reply {
                return Err(Failure::permanent("A thread cannot take a reply."));
            }
            write.steps[index].state = StepState::Attempted;
            write.uncertain = true;
            env.save(write)?;
            let root = step.root_id.clone().unwrap_or_default();
            match env.reply(write, &root, &step.body) {
                Ok(receipt) => {
                    done(write, index, receipt);
                    env.save(write)
                }
                Err(error) => {
                    write.uncertain = error.uncertain;
                    if !error.uncertain {
                        write.steps[index].state = StepState::Planned;
                    }
                    env.save(write)?;
                    if error.uncertain {
                        if let Ok(Some(current)) = env.thread(write, &step.thread_id) {
                            if let Some(receipt) = found_reply(write, &current, index) {
                                done(write, index, receipt);
                                return env.save(write);
                            }
                        }
                    }
                    Err(error.failure)
                }
            }
        }
        StepKind::Resolve => {
            // Reconcile an already-landed effect before enforcing the new contract on
            // pending legacy work. Never re-resolve an uncertain legacy request.
            if !thread.resolved {
                resolution_proof(write, &step)?;
            }
            if thread.resolved {
                let receipt = if step.state == StepState::Attempted {
                    write.request_id(index)
                } else {
                    "already".into()
                };
                done(write, index, receipt);
                return env.save(write);
            }
            let proofs = own_proofs(env, write)?;
            if !thread.owned_by_any(&proofs) {
                return Err(Failure::permanent(
                    "Thread ownership could not be verified; nothing was resolved.",
                ));
            }
            write.steps[index].state = StepState::Attempted;
            write.uncertain = true;
            env.save(write)?;
            let request = write.request_id(index);
            let won_t = settled(&step);
            // Last check before the effect: after every local write, a fresh thread
            // and PR author read. A refusal here sent nothing.
            if let Some(evidence) = won_t {
                let fresh = env.thread(write, &step.thread_id).and_then(|thread| {
                    let thread = thread.ok_or_else(|| {
                        Failure::permanent("A thread disappeared before its write.")
                    })?;
                    settled_holds(env, write, evidence, &thread)
                });
                if let Err(error) = fresh {
                    write.steps[index].state = step.state;
                    write.uncertain = false;
                    env.save(write)?;
                    return Err(error);
                }
            }
            match env.resolve(write, &step.thread_id, &request) {
                Ok(()) => {
                    if let Some(evidence) = won_t {
                        return confirm_won_t_fix(env, write, index, evidence, request);
                    }
                    done(write, index, request);
                    env.save(write)
                }
                Err(error) => {
                    write.uncertain = error.uncertain || step.state == StepState::Attempted;
                    if env
                        .thread(write, &step.thread_id)
                        .is_ok_and(|t| t.is_some_and(|t| t.resolved))
                    {
                        done(write, index, request);
                        return env.save(write);
                    }
                    // Resolving twice is a no-op, so an unconfirmed attempt may be retried.
                    write.steps[index].state = if write.uncertain {
                        StepState::Attempted
                    } else {
                        StepState::Planned
                    };
                    env.save(write)?;
                    Err(error.failure)
                }
            }
        }
    }
}

/// After an accepted won't-fix resolution lands, re-read the thread and PR author. If the
/// accepted snapshot changed during the write, undo PR Sniper's own resolution and do
/// not complete the step. Only the GitHub-side gap between this read and the resolve
/// (sub-second) remains; see ADR 0021.
fn confirm_won_t_fix(
    env: &mut impl Environment,
    write: &mut ExitWrite,
    index: usize,
    evidence: &crate::review::findings::Assessment,
    request: String,
) -> Result<(), Failure> {
    let thread_id = write.steps[index].thread_id.clone();
    let holds = env.thread(write, &thread_id).and_then(|thread| {
        let thread =
            thread.ok_or_else(|| Failure::permanent("A thread disappeared after its write."))?;
        settled_holds(env, write, evidence, &thread)
    });
    let Err(changed) = holds else {
        done(write, index, request);
        return env.save(write);
    };
    match env.unresolve(write, &thread_id, &format!("{request}-undo")) {
        Ok(()) => {
            write.steps[index].state = StepState::Planned;
            write.uncertain = false;
            env.save(write)?;
            Err(Failure::permanent(format!(
                "{} The resolution was undone.",
                changed.message
            )))
        }
        Err(error) => {
            // Leave the step attempted and uncertain: a human must reconcile it.
            env.save(write)?;
            Err(Failure::permanent(format!(
                "{} Undoing the resolution failed: {}",
                changed.message, error.failure.message
            )))
        }
    }
}

/// Store half of a toss: count it and queue the review again under budget. The
/// write's run must still be its key's newest run; anything newer already re-ran.
pub fn requeue(store: &Store, write: &ExitWrite, now: i64) -> Result<(), String> {
    if write.phase != Phase::Tossed {
        return Ok(());
    }
    let reviews = store.load_reviews()?;
    let Some(run) = reviews
        .iter()
        .find(|r| r.operation.id == write.review_operation_id)
    else {
        return Ok(());
    };
    if reviews
        .iter()
        .rev()
        .find(|r| r.key == run.key)
        .is_some_and(|newest| newest.operation.id != run.operation.id)
    {
        return Ok(());
    }
    crate::exits::consume::toss_and_requeue(store, &write.job, run, now).map(|_| ())
}

/// Saved exit writes of a review, for the publication hold and the coordinator.
pub fn for_review(store: &Store, review_operation_id: &str) -> Result<Option<ExitWrite>, String> {
    Ok(store
        .load_feedback()?
        .exit_writes
        .into_iter()
        .find(|w| w.review_operation_id == review_operation_id))
}

/// Why the publication batch must wait for this review, if it must. Replies and
/// resolutions are verified before the first effect so the exit stays all-or-nothing.
pub fn holds_publication(store: &Store, review: &ReviewRun) -> Result<Option<String>, String> {
    if plan(review, 0).is_none() {
        return Ok(None);
    }
    Ok(holds_publication_with(
        &store.load_feedback()?.exit_writes,
        review,
    ))
}

/// `holds_publication` over already-loaded exit writes.
pub(crate) fn holds_publication_with(writes: &[ExitWrite], review: &ReviewRun) -> Option<String> {
    let planned = plan(review, 0)?;
    match writes
        .iter()
        .find(|w| w.review_operation_id == review.operation.id)
    {
        None if planned.comments_first => {
            Some("Replies and resolutions are not verified yet.".into())
        }
        None => None,
        Some(w) => match w.phase {
            Phase::Planned => Some("Replies and resolutions are not verified yet.".into()),
            Phase::Tossed => Some("The exit was discarded; nothing is published.".into()),
            _ => None,
        },
    }
}

/// Admit writes for completed runs that have none yet. Returns how many were added.
pub fn admit(store: &Store, now: i64) -> Result<usize, String> {
    let (ledger, added) = with_admissions(store, now)?;
    if added > 0 {
        store.save_feedback(&ledger)?;
    }
    Ok(added)
}

/// How many writes `admit` would add; read-only.
pub(crate) fn admissions(store: &Store, now: i64) -> Result<usize, String> {
    Ok(with_admissions(store, now)?.1)
}

fn with_admissions(store: &Store, now: i64) -> Result<(crate::feedback::Ledger, usize), String> {
    let reviews = store.review_evidence()?;
    let mut ledger = store.load_feedback()?;
    let latest: std::collections::BTreeMap<_, _> = reviews
        .iter()
        .map(|r| (r.key.as_str(), r.operation.id.as_str()))
        .collect();
    let mut added = 0;
    for review in &reviews {
        if latest.get(review.key.as_str()) != Some(&review.operation.id.as_str())
            || ledger
                .exit_writes
                .iter()
                .any(|w| w.review_operation_id == review.operation.id)
        {
            continue;
        }
        if let Some(write) = plan(review, now) {
            ledger.exit_writes.push(write);
            added += 1;
        }
    }
    Ok((ledger, added))
}

/// Whether `next_write` could admit or start anything; read-only.
pub(crate) fn write_pending(store: &Store, now: i64) -> Result<bool, String> {
    if store.load_automation()?.paused {
        return Ok(false);
    }
    Ok(admissions(store, now)? > 0 || next_due(store, now)?.is_some())
}

/// Admits new exits, then returns the next due write unless automation is paused.
pub(crate) fn next_write(store: &Store, now: i64) -> Result<Option<ExitWrite>, String> {
    if store.load_automation()?.paused {
        return Ok(None);
    }
    admit(store, now)?;
    Ok(match next_due(store, now)? {
        Some(id) => store
            .load_feedback()?
            .exit_writes
            .into_iter()
            .find(|w| w.id == id),
        None => None,
    })
}

/// Due write with the oldest retry time, so one write cannot hold back the rest.
pub fn next_due(store: &Store, now: i64) -> Result<Option<String>, String> {
    Ok(store
        .load_feedback()?
        .exit_writes
        .iter()
        .filter(|w| {
            !w.terminal()
                && (w.phase == Phase::Completed
                    || w.operation.state == OperationState::Running
                    || w.operation.next_attempt_at.is_some_and(|next| now >= next))
        })
        .min_by_key(|w| w.operation.next_attempt_at.unwrap_or(i64::MIN))
        .map(|w| w.id.clone()))
}

/// Local iterations (head, iteration) known for this PR, newest information wins nothing:
/// a head maps to the iteration of any job that carried it.
pub fn known_iterations(store: &Store, job: &QueueJob) -> Result<Vec<(String, u64)>, String> {
    let mut known: Vec<(String, u64)> = store
        .load_queue()?
        .iter()
        .filter(|j| crate::feedback::same_pr(j, job))
        .filter_map(|j| Some((j.head_sha.clone(), j.work.as_ref()?.iteration)))
        .collect();
    known.extend(
        store
            .review_evidence()?
            .iter()
            .filter(|r| crate::feedback::same_pr(&r.job, job))
            .filter_map(|r| Some((r.job.head_sha.clone(), r.job.work.as_ref()?.iteration))),
    );
    known.sort();
    known.dedup();
    Ok(known)
}

/// Targeted revision of a comment (AC-041): the local iteration whose head is the
/// comment's `original_commit_id`, else `unknown`. Ambiguous heads are `unknown`.
pub fn targeted_revision(commit: Option<&str>, known: &[(String, u64)]) -> String {
    let Some(commit) = commit else {
        return "unknown".into();
    };
    let mut matches = known.iter().filter(|(head, _)| head == commit);
    match (matches.next(), matches.next()) {
        (Some((_, iteration)), None) => iteration.to_string(),
        (Some((_, first)), Some(_)) => {
            let all: std::collections::BTreeSet<_> = known
                .iter()
                .filter(|(head, _)| head == commit)
                .map(|(_, n)| *n)
                .collect();
            if all.len() == 1 {
                first.to_string()
            } else {
                "unknown".into()
            }
        }
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod tests;
