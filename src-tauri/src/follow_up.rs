pub(crate) mod host;
mod task;

use crate::{
    github::threads::Thread,
    monitoring::{JobOperation, OperationFailure, OperationState},
    publication::Publication,
    review::{Failure, ReviewResult, ReviewRun},
    storage::Store,
};
use serde::{Deserialize, Serialize};

pub(crate) use task::ReplyTask;
pub use task::{Evidence, ReplyDecision, ReplyOutput};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    WaitingStart,
    Analyzing,
    Quiet,
    HumanInputRequired,
    WaitingPublication,
    Publishing,
    Published,
    Stopped,
    StaleAfterPublication,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowUp {
    pub id: String,
    pub key: String,
    pub publication_id: String,
    pub review: ReviewRun,
    pub thread: Thread,
    pub trigger_id: String,
    pub phase: Phase,
    pub analysis: Option<JobOperation>,
    pub publication: Option<JobOperation>,
    pub history: Vec<JobOperation>,
    pub manual_start: bool,
    pub confirmed: bool,
    pub automatic_publication: bool,
    pub cancelled: bool,
    pub error: Option<String>,
    pub result: Option<ReviewResult<ReplyOutput>>,
    pub body: Option<String>,
    pub uncertain: bool,
    pub receipt: Option<String>,
    #[serde(default)]
    pub reply_ordinal: Option<u64>,
    #[serde(default)]
    pub enqueue_order: Option<u64>,
    #[serde(default)]
    pub enqueued_at: Option<i64>,
}

impl FollowUp {
    pub fn new(origin: &Publication, thread: Thread) -> Result<Self, String> {
        if thread.resolved || !thread.can_reply || !thread.owned_by(origin) {
            return Err(
                "Only unresolved, verified PR Sniper-owned threads can receive follow-ups.".into(),
            );
        }
        let trigger_id = thread
            .latest_external(&origin.review.job.account_id)
            .ok_or("This thread has no new external comment.")?
            .id
            .clone();
        let job = &origin.review.job;
        let key = serde_json::json!([
            job.provider,
            job.account_id,
            job.configuration_id,
            thread.id,
            trigger_id,
            job.head_sha
        ])
        .to_string();
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            key,
            publication_id: origin.id.clone(),
            review: origin.review.clone(),
            thread,
            trigger_id,
            phase: Phase::WaitingStart,
            analysis: None,
            publication: None,
            history: vec![],
            manual_start: false,
            confirmed: false,
            automatic_publication: false,
            cancelled: false,
            error: None,
            result: None,
            body: None,
            uncertain: false,
            receipt: None,
            reply_ordinal: None,
            enqueue_order: None,
            enqueued_at: None,
        })
    }

    pub fn operation(&self, kind: &str, now: i64) -> JobOperation {
        let mut operation = JobOperation::review(&self.review.job, now);
        operation.operation_type = kind.into();
        operation.pending_review_id = self
            .thread
            .comments
            .first()
            .and_then(|comment| comment.review_id.clone());
        operation.owned_thread_id = Some(self.thread.id.clone());
        operation.triggering_external_comment_id = Some(self.trigger_id.clone());
        operation
    }

    pub fn reply_body(&self) -> Result<String, Failure> {
        let result = self
            .result
            .as_ref()
            .ok_or_else(|| Failure::permanent("No validated follow-up result is available."))?;
        if result.output.decision != ReplyDecision::Reply {
            return Err(Failure::permanent(
                "This follow-up does not authorize a reply.",
            ));
        }

        let citations = result
            .output
            .evidence
            .iter()
            .map(|e| format!("    {} ({} line {}): {}\n", e.path, e.side, e.line, e.quote))
            .collect::<String>();
        let body = format!("Automated follow-up by PR Sniper.\n\n{}\n\nEvidence at {}:\n\n{}\n<!-- pr-sniper:reply:{} -->\n\n\u{f05b} PR Sniper",
            result.output.body, self.review.job.head_sha, citations, self.id);
        if body.chars().count() > 65_536 {
            return Err(Failure::permanent(
                "Reply exceeds GitHub's body limit; nothing was truncated.",
            ));
        }
        Ok(body)
    }

    pub fn check_publication_grant(&self, current: &Self, automatic: bool) -> Result<(), Failure> {
        if self.publication.is_some()
            && (current.id != self.id
                || current.cancelled
                || automatic != self.automatic_publication
                || (!automatic && !current.confirmed))
        {
            return Err(Failure::permanent(
                "Reply publication permission changed; confirm again before retrying.",
            ));
        }
        Ok(())
    }

    pub fn fresh_thread(&self, current: &Thread) -> bool {
        if current.id != self.thread.id || current.resolved || !current.can_reply {
            return false;
        }
        let comments: Vec<_> = current
            .comments
            .iter()
            .filter(|comment| {
                !(self.body.as_ref().is_some_and(|body| body == &comment.body)
                    && comment.author_id.as_deref() == Some(&self.review.job.account_id))
            })
            .collect();
        comments == self.thread.comments.iter().collect::<Vec<_>>()
    }

    pub fn reconcile(&self, current: &Thread) -> Result<Option<String>, Failure> {
        let Some(body) = &self.body else {
            return Ok(None);
        };
        let root = self.thread.root()?;
        let matches: Vec<_> = current
            .comments
            .iter()
            .filter(|comment| {
                comment.body == *body
                    && comment.author_id.as_deref() == Some(&self.review.job.account_id)
                    && comment.reply_to.as_deref() == Some(&root.id)
            })
            .collect();
        if matches.len() > 1 {
            return Err(Failure::permanent(
                "Multiple remote replies match one follow-up; manual reconciliation is required.",
            ));
        }
        Ok(matches.first().map(|comment| comment.id.clone()))
    }
}

pub fn admit(
    runs: &mut Vec<FollowUp>,
    origin: &Publication,
    thread: Thread,
) -> Result<bool, String> {
    let mut run = FollowUp::new(origin, thread)?;
    if runs.iter().any(|previous| previous.key == run.key) {
        return Ok(false);
    }
    run.reply_ordinal = Some(
        runs.iter()
            .filter(|previous| {
                previous.review.job.account_id == run.review.job.account_id
                    && previous.review.job.configuration_id == run.review.job.configuration_id
                    && previous.review.job.repository_id == run.review.job.repository_id
                    && previous.review.job.pull_request_id == run.review.job.pull_request_id
                    && previous.review.selection.agent.id == run.review.selection.agent.id
            })
            .count() as u64
            + 1,
    );
    runs.push(run);
    Ok(true)
}

pub fn polling_origins(
    store: &Store,
    ticket: &crate::monitoring::PollTicket,
    pulls: &[crate::github::metadata::PullRequest],
) -> Result<Vec<Publication>, String> {
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    Ok(store
        .load_publications()?
        .into_iter()
        .filter(|origin| {
            origin.review.job.configuration_id == ticket.repository_id
                && ticket.assignments.iter().any(|a| {
                    a.assignment_id == origin.review.assignment_id
                        && a.agent_id == origin.review.selection.agent.id
                })
                && origin.review.job.account_id == ticket.provider_account_id
                && origin.receipts.last().is_some_and(|r| {
                    r.state == crate::publication::RemoteState::Commented
                        && !r.comment_ids.is_empty()
                })
                && pulls.iter().any(|pull| {
                    jobs.iter()
                        .find(|j| origin.review.matches_job(j))
                        .is_some_and(|job| {
                            crate::monitoring::review_policy(&settings, job, Some(pull)).is_ok()
                        })
                })
        })
        .collect())
}

pub fn validate_analysis_commit(
    store: &Store,
    run: &FollowUp,
    account_allowed: bool,
) -> Result<(), Failure> {
    if !account_allowed {
        return Err(Failure::permanent(
            "The GitHub connection changed before the follow-up result was saved.",
        ));
    }
    let current = store
        .load_follow_ups()
        .map_err(Failure::permanent)?
        .into_iter()
        .find(|current| current.id == run.id)
        .ok_or_else(|| Failure::permanent("Thread follow-up disappeared."))?;
    if current.cancelled {
        return Err(Failure::permanent("Thread follow-up was cancelled."));
    }
    let settings = store.load_settings().map_err(Failure::permanent)?;
    let jobs = store.load_queue().map_err(Failure::permanent)?;
    let job = jobs
        .iter()
        .find(|job| run.review.matches_job(job))
        .ok_or_else(|| Failure::permanent("The reviewed revision is no longer available."))?;
    let current = crate::review::Selection::resolve(&settings, job, &run.review.assignment_id)
        .map_err(Failure::permanent)?;
    if current != run.review.selection
        || (!current.policy.automatic_agent_start && !run.manual_start)
    {
        return Err(Failure::permanent(
            "Follow-up selection or start permission changed before result persistence.",
        ));
    }
    crate::publication::automatic_policy(&settings, &run.review, job)
        .map_err(Failure::permanent)?;
    Ok(())
}

pub fn restore(store: &Store) -> Result<(), String> {
    let mut runs = store.load_follow_ups()?;
    let mut changed = false;
    for run in &mut runs {
        for operation in [&mut run.analysis, &mut run.publication]
            .into_iter()
            .flatten()
        {
            if operation.state == OperationState::Running {
                operation.state = OperationState::Interrupted;
                operation.next_attempt_at = Some(0);
                changed = true;
            }
        }
    }
    if changed {
        store.save_follow_ups(&runs)?;
    }
    Ok(())
}

pub trait Environment {
    fn now(&self) -> Result<i64, Failure>;
    fn save(&mut self, run: &mut FollowUp) -> Result<(), Failure>;
    fn thread(&mut self, run: &FollowUp) -> Result<Thread, Failure>;
    fn gate(&mut self, run: &FollowUp, thread: &Thread) -> Result<Option<String>, Failure>;
    fn reply(&mut self, run: &FollowUp) -> Result<String, crate::publication::WriteFailure>;
}

pub fn publish(env: &mut impl Environment, run: &mut FollowUp) -> Result<(), Failure> {
    let body_check = run.reply_body().and_then(|body| {
        if run.body.as_deref() == Some(&body) {
            Ok(())
        } else {
            Err(Failure::permanent(
                "The frozen reply does not match the validated follow-up result.",
            ))
        }
    });
    if let Err(error) = body_check {
        if let Some(operation) = run.publication.as_mut() {
            operation.fail(&error.monitoring(), env.now()?);
        }
        run.phase = Phase::Stopped;
        run.error = Some(error.message.clone());
        env.save(run)?;
        return Err(error);
    }
    let operation = run
        .publication
        .as_mut()
        .ok_or_else(|| Failure::permanent("Publication has not been authorized."))?;
    if operation.state == OperationState::Completed {
        return Err(Failure::permanent("This reply is already complete."));
    }
    if let Err(error) = operation.begin_attempt(env.now()?) {
        run.error = Some(error.clone());
        env.save(run)?;
        return Err(Failure::permanent(error));
    }
    let result = (|| {
        run.phase = Phase::Publishing;
        run.error = None;
        env.save(run)?;
        let current = env.thread(run)?;
        if let Some(receipt) = run.reconcile(&current)? {
            run.receipt = Some(receipt.clone());
            run.publication.as_mut().unwrap().confirmed_receipt = Some(receipt);
            run.uncertain = false;
            env.save(run)?;
        } else if run.uncertain || run.receipt.is_some() {
            return Err(Failure {
                kind: OperationFailure::Network,
                message: "Reply outcome is unresolved; no replacement reply will be posted.".into(),
                retry_after_seconds: None,
            });
        }
        let gate = env.gate(run, &current)?;
        if run.receipt.is_some() {
            return finish(env, run, gate);
        }
        if let Some(reason) = gate {
            return Err(Failure::permanent(reason));
        }
        if env.now()? >= run.publication.as_ref().unwrap().retry_deadline {
            return Err(Failure::timeout());
        }
        run.uncertain = true;
        run.publication.as_mut().unwrap().attempted_mutation = Some("thread_reply".into());
        env.save(run)?;
        match env.reply(run) {
            Ok(receipt) => {
                run.receipt = Some(receipt.clone());
                run.publication.as_mut().unwrap().confirmed_receipt = Some(receipt);
                run.uncertain = false;
                env.save(run)?;
            }
            Err(error) => {
                run.uncertain = error.uncertain;
                env.save(run)?;
                let observation = env.thread(run).and_then(|thread| env.gate(run, &thread));
                return Err(match observation {
                    Ok(_) => error.failure,
                    Err(observation) => Failure {
                        message: format!(
                            "{} Post-reply verification also failed: {}",
                            error.failure.message, observation.message
                        ),
                        ..error.failure
                    },
                });
            }
        }
        let current = env.thread(run)?;
        let gate = env.gate(run, &current)?;
        finish(env, run, gate)
    })();
    if let Err(error) = result {
        run.publication
            .as_mut()
            .unwrap()
            .fail(&error.monitoring(), env.now()?);
        run.phase = if run.uncertain {
            Phase::Unresolved
        } else if run.receipt.is_some() {
            Phase::Published
        } else {
            Phase::Stopped
        };
        run.error = Some(error.message.clone());
        env.save(run)?;
        return Err(error);
    }
    env.save(run)
}

fn finish(
    env: &mut impl Environment,
    run: &mut FollowUp,
    gate: Option<String>,
) -> Result<(), Failure> {
    let receipt = run
        .receipt
        .as_ref()
        .ok_or_else(|| Failure::permanent("No confirmed reply receipt."))?;
    let operation = run.publication.as_mut().unwrap();
    operation.confirmed_receipt = Some(receipt.clone());
    operation.state = OperationState::Completed;
    operation.next_attempt_at = None;
    operation.failure = None;
    run.phase = if gate.is_some() {
        Phase::StaleAfterPublication
    } else {
        Phase::Published
    };
    run.error = gate;
    env.save(run)
}
