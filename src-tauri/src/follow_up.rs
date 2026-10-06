pub(crate) mod host;
pub(crate) mod task;

use crate::{
    github::conversation::TopComment,
    github::threads::Thread,
    monitoring::{JobOperation, OperationFailure, OperationState},
    publication::Publication,
    review::{Failure, ReviewResult, ReviewRun, Selection},
    storage::Store,
};
use serde::{Deserialize, Serialize};

pub(crate) use task::{ConversationInput, ReplyTask};
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
pub struct ConversationContext {
    pub assignment_id: String,
    pub job: crate::monitoring::QueueJob,
    pub selection: Selection,
    /// Retained for historical decoding only, not a permission.
    #[serde(default)]
    pub trust_confirmed: bool,
    #[serde(default)]
    pub feedback: Vec<crate::feedback::Context>,
    #[serde(default)]
    pub feedback_checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisAttempt {
    pub context: ConversationContext,
    pub operation: JobOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedTarget {
    pub publication_id: String,
    pub review: ReviewRun,
    pub thread: Thread,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedTarget {
    pub proof: crate::github::threads::Ownership,
    pub thread: Thread,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationTarget {
    Owned(Box<OwnedTarget>),
    Retained(Box<RetainedTarget>),
    Thread { thread: Thread },
    Mention { comment: TopComment },
}

#[derive(Debug, Clone)]
pub enum Observation {
    Owned(Thread),
    Mention {
        comment: Option<TopComment>,
        replies: Vec<TopComment>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowUp {
    pub id: String,
    pub key: String,
    pub target: ConversationTarget,
    pub context: ConversationContext,
    pub trigger_id: String,
    pub phase: Phase,
    pub analysis: Option<JobOperation>,
    pub publication: Option<JobOperation>,
    pub history: Vec<JobOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub analysis_history: Vec<AnalysisAttempt>,
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
    #[serde(default)]
    pub discussion: Vec<TopComment>,
}

impl FollowUp {
    pub fn thread(&self) -> Result<&Thread, String> {
        match &self.target {
            ConversationTarget::Owned(origin) => Ok(&origin.thread),
            ConversationTarget::Retained(origin) => Ok(&origin.thread),
            ConversationTarget::Thread { thread } => Ok(thread),
            _ => Err("This is not an owned-thread target.".into()),
        }
    }
    pub fn thread_mut(&mut self) -> Result<&mut Thread, String> {
        match &mut self.target {
            ConversationTarget::Owned(origin) => Ok(&mut origin.thread),
            ConversationTarget::Retained(origin) => Ok(&mut origin.thread),
            ConversationTarget::Thread { thread } => Ok(thread),
            _ => Err("This is not an owned-thread target.".into()),
        }
    }
    pub fn owner_agent_id(&self) -> Option<&str> {
        match &self.target {
            ConversationTarget::Owned(origin) => Some(&origin.review.selection.agent.id),
            ConversationTarget::Retained(origin) => Some(&origin.proof.agent_id),
            _ => None,
        }
    }
    pub(crate) fn owns_feedback(&self, feedback: &crate::feedback::Context) -> bool {
        let (publication, agent, assignment, head) = match &self.target {
            ConversationTarget::Owned(origin) => (
                &origin.publication_id,
                &origin.review.selection.agent.id,
                &origin.review.assignment_id,
                &origin.review.job.head_sha,
            ),
            ConversationTarget::Retained(origin) => (
                &origin.proof.publication_id,
                &origin.proof.agent_id,
                &origin.proof.assignment_id,
                &origin.proof.head_sha,
            ),
            ConversationTarget::Thread { thread } => {
                return thread.root().is_ok_and(|root| root.id == feedback.root_id)
            }
            ConversationTarget::Mention { .. } => return false,
        };
        publication == &feedback.publication_id
            && agent == &feedback.owner_agent_id
            && assignment == &feedback.owner_assignment_id
            && head == &feedback.original_head
            && self
                .thread()
                .is_ok_and(|t| t.root().is_ok_and(|r| r.id == feedback.root_id))
    }
    pub fn owned(&self) -> Result<&OwnedTarget, String> {
        match &self.target {
            ConversationTarget::Owned(value) => Ok(value),
            _ => Err("This is not an owned-thread target.".into()),
        }
    }
    pub fn owned_mut(&mut self) -> Result<&mut OwnedTarget, String> {
        match &mut self.target {
            ConversationTarget::Owned(value) => Ok(value),
            _ => Err("This is not an owned-thread target.".into()),
        }
    }
    pub fn kind(&self) -> crate::capacity::Kind {
        match self.target {
            ConversationTarget::Owned(_)
            | ConversationTarget::Retained(_)
            | ConversationTarget::Thread { .. } => crate::capacity::Kind::Reply,
            ConversationTarget::Mention { .. } => crate::capacity::Kind::Mention,
        }
    }
    pub fn same_result_lens(&self, current: &Selection) -> bool {
        self.context.selection.agent == current.agent
            && self.context.selection.doctrine == current.doctrine
            && self.context.selection.preset == current.preset
            && self.context.selection.policy.prompt == current.policy.prompt
            && self.context.selection.policy.adapter == current.policy.adapter
    }
    pub fn same_analysis_execution(&self, current: &Selection) -> bool {
        self.context.selection.same_execution(current)
    }

    pub(crate) fn retain_analysis_attempt(&mut self, operation: JobOperation) {
        if operation.attempt_count > 0 {
            self.analysis_history.push(AnalysisAttempt {
                context: self.context.clone(),
                operation: operation.clone(),
            });
        }
        self.history.push(operation);
    }
    pub(crate) fn input(&self) -> ConversationInput {
        match &self.target {
            ConversationTarget::Owned(origin) => ConversationInput::Owned {
                thread: origin.thread.clone(),
            },
            ConversationTarget::Retained(origin) => ConversationInput::Owned {
                thread: origin.thread.clone(),
            },
            ConversationTarget::Thread { thread } => ConversationInput::Owned {
                thread: thread.clone(),
            },
            ConversationTarget::Mention { comment } if self.discussion.is_empty() => {
                ConversationInput::Mention {
                    comment: comment.clone(),
                }
            }
            ConversationTarget::Mention { comment } => ConversationInput::General {
                comment: comment.clone(),
                discussion: self.discussion.clone(),
            },
        }
    }
    pub fn fresh_observation(&self, observed: &Observation) -> bool {
        match (&self.target, observed) {
            (
                ConversationTarget::Owned(_)
                | ConversationTarget::Retained(_)
                | ConversationTarget::Thread { .. },
                Observation::Owned(thread),
            ) => self.fresh_thread(thread),
            (
                ConversationTarget::Mention { comment },
                Observation::Mention {
                    comment: current,
                    replies,
                },
            ) => {
                current.as_ref() == Some(comment)
                    && (self.discussion.is_empty()
                        || replies
                            .iter()
                            .filter(|reply| {
                                !(self.body.as_ref() == Some(&reply.body)
                                    && reply.author_id.as_deref()
                                        == Some(&self.context.job.account_id))
                            })
                            .collect::<Vec<_>>()
                            == self.discussion.iter().collect::<Vec<_>>())
            }
            _ => false,
        }
    }
    pub fn validate_current(
        &self,
        settings: &crate::storage::Settings,
        job: &crate::monitoring::QueueJob,
        pull: &crate::github::metadata::PullRequest,
        active: bool,
        can_comment: bool,
    ) -> Result<(), Failure> {
        if !active {
            return Err(Failure::permanent("Monitoring scope is no longer active."));
        }
        crate::monitoring::review_policy(settings, job, Some(pull)).map_err(Failure::permanent)?;
        let current = Selection::resolve(settings, job, &self.context.assignment_id)
            .map_err(Failure::permanent)?;
        if self.publication.is_none() {
            if !self.same_analysis_execution(&current) {
                return Err(Failure::permanent(
                    "Conversation execution configuration changed.",
                ));
            }
        } else if !self.same_result_lens(&current) {
            return Err(Failure::permanent(
                "The conversation review lens changed before publication.",
            ));
        }
        let automatic = self.authority(settings, job).map_err(Failure::permanent)?;
        if self.publication.is_some() && !can_comment {
            return Err(Failure::permanent(
                "The acting account cannot publish to this repository.",
            ));
        }
        if self.publication.is_some() && !automatic {
            return Err(Failure::permanent(
                "Conversation publication permission changed.",
            ));
        }
        if self
            .result
            .as_ref()
            .and_then(|r| r.reviewed_base_sha.as_ref())
            .is_some_and(|base| base != &pull.base_sha)
        {
            return Err(Failure::permanent(
                "Conversation output refers to an older base revision.",
            ));
        }
        if self
            .context
            .job
            .observed_base_sha
            .as_ref()
            .is_some_and(|base| base != &pull.base_sha)
        {
            return Err(Failure::permanent(
                "Conversation analysis context has an older base revision.",
            ));
        }
        Ok(())
    }
    pub fn matches_job(&self, job: &crate::monitoring::QueueJob) -> bool {
        self.context.job.assignment_id == job.assignment_id
            && crate::review::key(
                &self.context.job,
                self.context.job.assignment_id.as_deref().unwrap_or(""),
            ) == crate::review::key(job, job.assignment_id.as_deref().unwrap_or(""))
            && self.context.job.account_id == job.account_id
            && self.context.job.repository_id == job.repository_id
            && self.context.job.head_sha == job.head_sha
    }
    pub fn authority(
        &self,
        settings: &crate::storage::Settings,
        job: &crate::monitoring::QueueJob,
    ) -> Result<bool, String> {
        let repository = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id)
            .ok_or("Repository removed.")?;
        let assignment = repository
            .assignments
            .iter()
            .find(|a| {
                Some(a.id.as_str()) == job.assignment_id.as_deref()
                    && a.agent_id == self.context.selection.agent.id
            })
            .ok_or(
                "The original conversation owner is unavailable; ownership was not transferred.",
            )?;
        if repository.primary_assignment_id() != Some(assignment.id.as_str()) {
            return Err("Only the current primary may assess or reply to conversations. Pending publication cannot use an old secondary.".into());
        }
        crate::monitoring::review_policy(settings, job, None)?;
        Ok(repository.assignment_authority(assignment).reply)
    }

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
            target: ConversationTarget::Owned(Box::new(OwnedTarget {
                publication_id: origin.id.clone(),
                review: origin.review.clone(),
                thread,
            })),
            context: ConversationContext {
                assignment_id: origin.review.assignment_id.clone(),
                job: origin.review.job.clone(),
                selection: origin.review.selection.clone(),
                trust_confirmed: origin.review.trust_confirmed,
                feedback: Vec::new(),
                feedback_checked: false,
            },
            trigger_id,
            phase: Phase::WaitingStart,
            analysis: None,
            publication: None,
            history: vec![],
            analysis_history: Vec::new(),
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
            discussion: Vec::new(),
        })
    }

    pub fn mention(comment: TopComment, context: ConversationContext) -> Self {
        let job = &context.job;
        let key = mention_key(job, &comment.id);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            key,
            trigger_id: comment.id.clone(),
            target: ConversationTarget::Mention { comment },
            context,
            phase: Phase::WaitingStart,
            analysis: None,
            publication: None,
            history: Vec::new(),
            analysis_history: Vec::new(),
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
            discussion: Vec::new(),
        }
    }

    pub fn general_thread(thread: Thread, context: ConversationContext) -> Result<Self, String> {
        if thread.resolved {
            return Err("Resolved discussions cannot create new analysis or replies.".into());
        }
        let trigger = thread
            .latest_other_user(&context.job.account_id)
            .ok_or("No eligible other-user comment.")?;
        let key = serde_json::json!([
            "thread",
            context.job.provider,
            context.job.account_id,
            context.job.configuration_id,
            context.job.repository_id,
            context.job.pull_request_id,
            thread.id,
            trigger.id
        ])
        .to_string();
        let comment = TopComment {
            id: trigger.id.clone(),
            body: trigger.body.clone(),
            author_id: trigger.author_id.clone(),
            author_login: trigger.author_login.clone(),
            created_at: trigger.created_at.clone(),
            updated_at: trigger.published_at.clone(),
        };
        let mut run = Self::mention(comment, context);
        run.key = key;
        run.target = ConversationTarget::Thread { thread };
        Ok(run)
    }

    pub fn retained(
        receipt: &crate::retention::OwnedReceipt,
        thread: Thread,
        context: ConversationContext,
    ) -> Result<Self, String> {
        if thread.resolved
            || !thread.can_reply
            || !thread.owned_by(&receipt.proof)
            || receipt.proof.agent_id != context.selection.agent.id
            || receipt.proof.assignment_id != context.assignment_id
            || !receipt.matches(&context.job)
        {
            return Err("Retained thread ownership or execution binding changed.".into());
        }
        let trigger_id = thread
            .latest_external(&receipt.proof.account_id)
            .ok_or("No new external comment in retained thread.")?
            .id
            .clone();
        let key = serde_json::json!([
            "github",
            receipt.proof.account_id,
            receipt.proof.configuration_id,
            thread.id,
            trigger_id,
            receipt.proof.head_sha
        ])
        .to_string();
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            key,
            target: ConversationTarget::Retained(Box::new(RetainedTarget {
                proof: receipt.proof.clone(),
                thread,
            })),
            context,
            trigger_id,
            phase: Phase::WaitingStart,
            analysis: None,
            publication: None,
            history: Vec::new(),
            analysis_history: Vec::new(),
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
            discussion: Vec::new(),
        })
    }

    pub fn operation(&self, kind: &str, now: i64) -> JobOperation {
        let mut operation = JobOperation::review(&self.context.job, now);
        operation.operation_type = kind.into();
        if let Ok(thread) = self.thread() {
            operation.pending_review_id = thread.comments.first().and_then(|c| c.review_id.clone());
            operation.owned_thread_id = Some(thread.id.clone());
        }
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
        let target = match &self.target {
            ConversationTarget::Mention { comment } => format!(
                "In response to https://github.com/{}/pull/{}#issuecomment-{}\n\n",
                self.context.job.repository_name, self.context.job.number, comment.id
            ),
            _ => String::new(),
        };
        let body = format!("Automated follow-up by PR Sniper / Agent {} / model {}.\n\n{target}{}\n\nEvidence at {}:\n\n{}\n<!-- pr-sniper:reply:{} -->\n\n\u{f05b} PR Sniper",
            self.context.selection.agent.name, result.model, result.output.body, self.context.job.head_sha, citations, self.id);
        if body.chars().count() > 65_536 {
            return Err(Failure::permanent(
                "Reply exceeds GitHub's body limit; nothing was truncated.",
            ));
        }
        Ok(body)
    }

    pub fn check_publication_grant(&self, current: &Self, automatic: bool) -> Result<(), Failure> {
        if self.publication.is_some() && (current.id != self.id || current.cancelled || !automatic)
        {
            return Err(Failure::permanent(
                "Reply Comment permission or cancellation changed; no new provider write is authorized.",
            ));
        }

        Ok(())
    }

    pub(crate) fn assessment_owner(&self) -> &str {
        self.thread()
            .ok()
            .and_then(|thread| thread.root().ok())
            .and_then(|root| {
                self.context
                    .feedback
                    .iter()
                    .find(|feedback| feedback.root_id == root.id)
            })
            .map(|feedback| feedback.owner_agent_id.as_str())
            .unwrap_or(&self.context.selection.agent.id)
    }

    pub fn fresh_thread(&self, current: &Thread) -> bool {
        let Ok(thread) = self.thread() else {
            return false;
        };
        if current.id != thread.id
            || current.resolved
            || (self.publication.is_some() && !current.can_reply)
        {
            return false;
        }
        let comments: Vec<_> = current
            .comments
            .iter()
            .filter(|comment| {
                !(self.body.as_ref().is_some_and(|body| body == &comment.body)
                    && comment.author_id.as_deref() == Some(&self.context.job.account_id))
            })
            .collect();
        comments == thread.comments.iter().collect::<Vec<_>>()
    }

    pub fn reconcile_observation(
        &self,
        observation: &Observation,
    ) -> Result<Option<String>, Failure> {
        match (observation, &self.target) {
            (
                Observation::Owned(thread),
                ConversationTarget::Owned(_)
                | ConversationTarget::Retained(_)
                | ConversationTarget::Thread { .. },
            ) => self.reconcile(thread),
            (Observation::Mention { replies, .. }, ConversationTarget::Mention { .. }) => {
                let matches = replies
                    .iter()
                    .filter(|c| {
                        self.body.as_ref() == Some(&c.body)
                            && c.author_id.as_deref() == Some(&self.context.job.account_id)
                    })
                    .collect::<Vec<_>>();
                if matches.len() > 1 {
                    return Err(Failure::permanent(
                        "Multiple remote replies match this mention intent.",
                    ));
                }
                Ok(matches.first().map(|c| c.id.clone()))
            }
            _ => Err(Failure::permanent(
                "Conversation target does not match its observation.",
            )),
        }
    }
    pub fn reconcile(&self, current: &Thread) -> Result<Option<String>, Failure> {
        let Some(body) = &self.body else {
            return Ok(None);
        };
        let root = self.thread().map_err(Failure::permanent)?.root()?;
        let matches: Vec<_> = current
            .comments
            .iter()
            .filter(|comment| {
                comment.body == *body
                    && comment.author_id.as_deref() == Some(&self.context.job.account_id)
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

pub fn mention_key(job: &crate::monitoring::QueueJob, comment: &str) -> String {
    serde_json::json!([
        "mention",
        job.provider,
        job.account_id,
        job.configuration_id,
        job.repository_id,
        job.pull_request_id,
        comment
    ])
    .to_string()
}

pub fn decode_runs(bytes: &[u8]) -> Result<Vec<FollowUp>, String> {
    decode_with_origins(bytes, &[])
}

pub fn decode_with_origins(bytes: &[u8], origins: &[Publication]) -> Result<Vec<FollowUp>, String> {
    let mut values: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|_| "Invalid conversation history.")?;
    for value in &mut values {
        let object = value
            .as_object_mut()
            .ok_or("Invalid conversation record.")?;
        if !object.contains_key("target") {
            let review = object
                .remove("review")
                .ok_or("Legacy conversation origin missing.")?;
            let publication = object
                .remove("publication_id")
                .ok_or("Legacy publication reference missing.")?;
            let thread = object.remove("thread").ok_or("Legacy thread missing.")?;
            object.insert("context".into(), serde_json::json!({"assignment_id":review["assignment_id"],"job":review["job"],"selection":review["selection"],"trust_confirmed":review["trust_confirmed"],"feedback":[]}));
            let original = origins
                .iter()
                .find(|p| Some(p.id.as_str()) == publication.as_str())
                .map(|p| serde_json::to_value(&p.review))
                .transpose()
                .map_err(|_| "Cannot decode retained conversation provenance.")?
                .unwrap_or(review);
            object.insert("target".into(), serde_json::json!({"kind":"owned","publication_id":publication,"review":original,"thread":thread}));
        }
    }
    serde_json::from_value(serde_json::Value::Array(values))
        .map_err(|_| "Invalid conversation state; no automation allowed.".into())
}

pub fn admit(
    runs: &mut Vec<FollowUp>,
    origin: &Publication,
    thread: Thread,
) -> Result<bool, String> {
    admit_run(runs, FollowUp::new(origin, thread)?)
}

pub fn admit_run(runs: &mut Vec<FollowUp>, mut run: FollowUp) -> Result<bool, String> {
    if runs.iter().any(|previous| previous.key == run.key) {
        return Ok(false);
    }
    let previous: Vec<_> = runs
        .iter()
        .filter(|previous| {
            previous.context.job.account_id == run.context.job.account_id
                && previous.context.job.configuration_id == run.context.job.configuration_id
                && previous.context.job.repository_id == run.context.job.repository_id
                && previous.context.job.pull_request_id == run.context.job.pull_request_id
                && previous.context.selection.agent.id == run.context.selection.agent.id
        })
        .collect();
    run.reply_ordinal = Some(
        previous
            .iter()
            .filter_map(|previous| previous.reply_ordinal)
            .max()
            .unwrap_or(0)
            .max(previous.len() as u64)
            .checked_add(1)
            .ok_or("Conversation ordinal exhausted.")?,
    );
    runs.push(run);
    Ok(true)
}

pub(crate) fn admit_stored(
    store: &Store,
    runs: &mut Vec<FollowUp>,
    run: FollowUp,
) -> Result<bool, String> {
    if crate::retention::known_key(store, &run.key)? {
        return Ok(false);
    }
    let previous = crate::retention::ordinal(
        store,
        &crate::retention::Binding::job(&run.context.job),
        &run.context.selection.agent.id,
        true,
    )?;
    if !admit_run(runs, run)? {
        return Ok(false);
    }
    let latest = runs
        .last_mut()
        .ok_or("Admitted conversation disappeared.")?;
    latest.reply_ordinal = Some(
        latest.reply_ordinal.unwrap_or(0).max(
            previous
                .checked_add(1)
                .ok_or("Conversation ordinal exhausted.")?,
        ),
    );
    Ok(true)
}

pub fn observation_origins(
    store: &Store,
    ticket: &crate::monitoring::PollTicket,
    pulls: &[crate::github::metadata::PullRequest],
) -> Result<Vec<Publication>, String> {
    Ok(store
        .load_publications()?
        .into_iter()
        .filter(|p| {
            (p.review.job.configuration_id == ticket.repository_id
                || p.review.job.configuration_id.is_empty())
                && p.review.job.account_id == ticket.provider_account_id
                && p.review.job.repository_id == ticket.provider_repository_id
                && p.receipts.last().is_some_and(|r| {
                    r.state == crate::publication::RemoteState::Commented
                        && !r.comment_ids.is_empty()
                })
                && pulls.iter().any(|pull| {
                    pull.id == p.review.job.pull_request_id
                        && pull.state == crate::github::metadata::Lifecycle::Open
                })
        })
        .collect())
}

pub fn current_owner_job<'a>(
    jobs: &'a [crate::monitoring::QueueJob],
    origin: &ReviewRun,
) -> Option<&'a crate::monitoring::QueueJob> {
    jobs.iter().rev().find(|j| {
        crate::feedback::same_pr(j, &origin.job)
            && j.assignment_id.as_deref() == Some(&origin.assignment_id)
            && j.work
                .as_ref()
                .is_none_or(|w| w.agent_id == origin.selection.agent.id)
            && crate::monitoring::actionable(j)
    })
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
                    current_owner_job(&jobs, &origin.review).is_some_and(|job| {
                        crate::monitoring::review_policy(&settings, job, Some(pull)).is_ok()
                            && Selection::resolve(&settings, job, &origin.review.assignment_id)
                                .is_ok_and(|s| s.agent.id == origin.review.selection.agent.id)
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
        .find(|job| run.matches_job(job))
        .ok_or_else(|| Failure::permanent("The reviewed revision is no longer available."))?;
    let current = crate::review::Selection::resolve(
        &settings,
        job,
        job.assignment_id
            .as_deref()
            .ok_or_else(|| Failure::permanent("Conversation assignment missing."))?,
    )
    .map_err(Failure::permanent)?;
    if !run.same_analysis_execution(&current) {
        return Err(Failure::permanent(
            "Follow-up selection changed before result persistence.",
        ));
    }
    run.authority(&settings, job).map_err(Failure::permanent)?;
    if run.context.feedback_checked {
        crate::feedback::validate_conversation_context(
            store,
            &run.context.job,
            &run.context.selection.agent.id,
            &run.context.feedback,
        )?;
    }
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
                if operation.interruption.is_some() {
                    operation.requeue_intentional(0);
                    run.result = None;
                    run.phase = Phase::WaitingStart;
                    changed = true;
                    continue;
                }
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
    fn observe(&mut self, run: &FollowUp) -> Result<Observation, Failure>;
    fn gate(
        &mut self,
        run: &FollowUp,
        observation: &Observation,
    ) -> Result<Option<String>, Failure>;
    fn reply(&mut self, run: &FollowUp) -> Result<String, crate::publication::WriteFailure>;
}

pub fn publish(env: &mut impl Environment, run: &mut FollowUp) -> Result<(), Failure> {
    let body_check = run.reply_body().and_then(|body| {
        let legacy = run.result.as_ref().map(|result| {
            body.replacen(
                &format!(
                    "Automated follow-up by PR Sniper / Agent {} / model {}.\n",
                    run.context.selection.agent.name, result.model
                ),
                "Automated follow-up by PR Sniper.\n",
                1,
            )
        });
        // Already-attempted legacy bodies are immutable recovery evidence, not
        // permission to create a fresh unsigned/impersonated response.
        let legacy_recovery =
            (run.uncertain || run.receipt.is_some()) && run.body.as_ref() == legacy.as_ref();
        if run.body.as_deref() == Some(&body) || legacy_recovery {
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
        run.phase = if run.uncertain {
            Phase::Unresolved
        } else {
            Phase::Stopped
        };
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
        let current = env.observe(run)?;
        if let Some(receipt) = run.reconcile_observation(&current)? {
            run.receipt = Some(receipt.clone());
            run.publication.as_mut().unwrap().confirmed_receipt = Some(receipt);
            run.uncertain = false;
            env.save(run)?;
        } else if run.uncertain || run.receipt.is_some() {
            return Err(Failure {
                cancelled: false,
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
                let observation = env
                    .observe(run)
                    .and_then(|observation| env.gate(run, &observation));
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
        let current = env.observe(run)?;
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
