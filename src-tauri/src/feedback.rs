use crate::{
    follow_up::{ConversationTarget, FollowUp, ReplyDecision},
    github::{conversation::TopComment, threads::Thread},
    monitoring::{OperationState, QueueJob, TrackedPullRequest},
    publication::{Publication, RemoteState},
    review::{Failure, Finding, ReviewOutput},
    storage::Store,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub id: String,
    pub publication_id: String,
    pub owner_agent_id: String,
    pub owner_assignment_id: String,
    pub original_head: String,
    pub root_id: String,
    pub path: String,
    pub title: String,
    pub body: String,
    pub thread: Option<Thread>,
    pub closed: bool,
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub context: Context,
    pub job: QueueJob,
    pub observed_head: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub key: String,
    pub work_id: String,
    pub enqueue_order: u64,
    pub enqueued_at: i64,
    pub binding: MentionBinding,
    pub comment: TopComment,
    pub follow_up_id: Option<String>,
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionBinding {
    pub configuration_id: String,
    pub account_id: String,
    pub account_login: String,
    pub repository_id: String,
    pub repository_name: String,
    pub pull_request_id: String,
    pub number: u64,
}

impl MentionBinding {
    pub fn matches(&self, job: &QueueJob) -> bool {
        job.provider == "github"
            && self.configuration_id == job.configuration_id
            && self.account_id == job.account_id
            && self.repository_id == job.repository_id
            && self.pull_request_id == job.pull_request_id
    }
    pub fn matches_tracked(&self, pr: &TrackedPullRequest) -> bool {
        pr.provider == "github"
            && self.configuration_id == pr.configuration_id
            && self.account_id == pr.account_id
            && self.repository_id == pr.repository_id
            && self.pull_request_id == pr.pull_request_id
    }

    pub fn tracked<'a>(
        &self,
        tracked: &'a [TrackedPullRequest],
    ) -> Result<&'a TrackedPullRequest, &'static str> {
        let mut matches = tracked.iter().filter(|pr| self.matches_tracked(pr));
        let pr = matches.next().ok_or("Mention tracking is unavailable.")?;
        if matches.next().is_some() || pr.item_id.is_empty() {
            return Err("Mention tracking is ambiguous; no iteration was selected.");
        }
        Ok(pr)
    }
    pub fn key(&self, comment: &str) -> String {
        serde_json::json!([
            "mention",
            "github",
            self.account_id,
            self.configuration_id,
            self.repository_id,
            self.pull_request_id,
            comment
        ])
        .to_string()
    }
}

impl Mention {
    /// Prefer captured identity or its exact execution. Baseline unlinked intent
    /// has no revision evidence: only an unchanged first tracked iteration can
    /// establish it. FIFO order or timestamps cannot distinguish a revision
    /// observed while no Agents were assigned.
    pub fn association<'a>(
        &self,
        tracked: &[TrackedPullRequest],
        jobs: &[QueueJob],
        runs: impl IntoIterator<Item = &'a FollowUp>,
    ) -> Result<String, &'static str> {
        let mut matches = runs.into_iter().filter(|run| {
            run.key == self.key
                || run.id == self.work_id
                || self.follow_up_id.as_ref() == Some(&run.id)
        });
        if let Some(run) = matches.next() {
            let id = crate::queue::item_id(&run.context.job);
            if matches.next().is_some()
                || run.key != self.key
                || run.id != self.work_id
                || !self.binding.matches(&run.context.job)
                || run.kind() != crate::capacity::Kind::Mention
                || run.enqueue_order != Some(self.enqueue_order)
                || run.enqueued_at != Some(self.enqueued_at)
                || self.item_id.as_ref().is_some_and(|saved| saved != &id)
                || self
                    .follow_up_id
                    .as_ref()
                    .is_some_and(|saved| saved != &run.id)
            {
                return Err("Mention execution conflicts with saved intent; no replacement or clearance inferred.");
            }
            return Ok(id);
        }
        if self.follow_up_id.is_some() {
            return Err(
                "Mention execution history is unavailable; no replacement or clearance inferred.",
            );
        }
        if let Some(id) = &self.item_id {
            if id.is_empty() {
                return Err("Saved mention iteration is invalid; no replacement was selected.");
            }
            return Ok(id.clone());
        }
        let pr = self.binding.tracked(tracked)?;
        if pr.iteration != 1
            || pr.admitted_at > self.enqueued_at
            || jobs.iter().filter(|j| self.binding.matches(j)).any(|j| {
                j.head_sha != pr.head_sha
                    || j.work.is_none() && crate::queue::item_id(j) != pr.item_id
                    || j.work.as_ref().is_some_and(|w| {
                        w.iteration != 1
                            || w.iteration_id != pr.iteration_id
                            || w.item_id != pr.item_id
                    })
            })
        {
            return Err("Legacy mention iteration is unavailable after intervening or ambiguous history; no later iteration was selected.");
        }
        Ok(pr.item_id.clone())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub records: Vec<Record>,
    pub mentions: Vec<Mention>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Open,
    Cleared,
    HumanInputRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub feedback_id: String,
    pub disposition: Disposition,
    pub reason: String,
    pub evidence: Vec<crate::follow_up::Evidence>,
}

pub fn assessment_schema() -> serde_json::Value {
    serde_json::json!({"type":"array","items":{"type":"object","additionalProperties":false,
    "required":["feedback_id","disposition","reason","evidence"],"properties":{
        "feedback_id":{"type":"string"},"disposition":{"type":"string","enum":["open","cleared","human_input_required"]},
        "reason":{"type":"string","maxLength":2000},"evidence":{"type":"array","maxItems":8,"items":{
            "type":"object","additionalProperties":false,"required":["path","side","line","quote"],
            "properties":{"path":{"type":"string"},"side":{"type":"string","enum":["base","head"]},"line":{"type":"integer","minimum":1},"quote":{"type":"string","maxLength":1000}}
        }}
    }}})
}

#[derive(Debug, Clone, Serialize)]
pub struct View {
    pub context: Context,
    pub state: &'static str,
    pub reason: Option<String>,
}

pub fn same_pr(left: &QueueJob, right: &QueueJob) -> bool {
    left.provider == right.provider
        && left.account_id == right.account_id
        && (left.configuration_id == right.configuration_id
            || left.configuration_id.is_empty()
            || right.configuration_id.is_empty())
        && left.repository_id == right.repository_id
        && left.pull_request_id == right.pull_request_id
}

pub fn root_key(job: &QueueJob, root: &str) -> String {
    serde_json::json!([
        job.provider,
        job.account_id,
        job.repository_id,
        job.pull_request_id,
        root
    ])
    .to_string()
}

impl Ledger {
    pub fn observe(
        &mut self,
        origin: &Publication,
        head: &str,
        threads: &[Thread],
    ) -> Result<(), String> {
        if threads.iter().any(|t| !t.owned_by(origin)) {
            return Err("Owned feedback provenance changed; no clearance recorded.".into());
        }
        let Some(receipt) = origin
            .receipts
            .last()
            .filter(|r| r.state == RemoteState::Commented)
        else {
            return Ok(());
        };
        for root_id in &receipt.comment_ids {
            let thread = threads
                .iter()
                .find(|t| t.root().is_ok_and(|r| &r.id == root_id));
            let id = root_key(&origin.review.job, root_id);
            let old = self.records.iter().find(|r| r.context.id == id);
            if old.is_some_and(|record| {
                record.context.publication_id != origin.id
                    || record.context.owner_agent_id != origin.review.selection.agent.id
                    || record.context.owner_assignment_id != origin.review.assignment_id
            }) {
                return Err("A published root has conflicting ownership provenance; no owner or disposition was replaced.".into());
            }
            let closed =
                old.is_some_and(|r| r.context.closed) || thread.is_some_and(|t| t.resolved);
            let root = thread.and_then(|t| t.root().ok());
            let inline = root.and_then(|root| {
                origin
                    .batch
                    .as_ref()?
                    .comments
                    .iter()
                    .find(|c| c.body == root.body)
            });
            let finding = inline.and_then(|inline| {
                origin
                    .review
                    .result
                    .as_ref()?
                    .output
                    .findings
                    .iter()
                    .find(|f| f.path == inline.path && inline.body.contains(&f.title))
            });
            let context = Context {
                id: id.clone(), publication_id: origin.id.clone(),
                owner_agent_id: origin.review.selection.agent.id.clone(), owner_assignment_id: origin.review.assignment_id.clone(),
                original_head: origin.review.job.head_sha.clone(), root_id: root_id.clone(),
                path: inline.map(|c| c.path.clone()).or_else(|| old.map(|r| r.context.path.clone())).unwrap_or_default(),
                title: finding.map(|f| f.title.clone()).or_else(|| old.map(|r| r.context.title.clone())).unwrap_or_default(),
                body: root.map(|r| r.body.clone()).or_else(|| old.map(|r| r.context.body.clone())).unwrap_or_default(),
                thread: thread.cloned().map(|mut thread| {
                    thread.comments = thread.comments.into_iter().enumerate().filter(|(index,c)|
                        *index == 0 || !(c.author_id.as_deref() == Some(&origin.review.job.account_id) && c.body.contains("<!-- pr-sniper:reply:")))
                        .map(|(_,c)| c).collect();
                    thread
                }).or_else(|| old.and_then(|r| r.context.thread.clone())),
                closed, unavailable: thread.is_none().then(|| "The expected published root is missing or unavailable; no resolution inferred.".into()),
            };
            let record = Record {
                context,
                job: origin.review.job.clone(),
                observed_head: head.into(),
            };
            if let Some(old) = self.records.iter_mut().find(|r| r.context.id == id) {
                *old = record;
            } else {
                self.records.push(record);
            }
        }
        Ok(())
    }
}

pub fn contexts(store: &Store, job: &QueueJob, agent: &str) -> Result<Vec<Context>, Failure> {
    contexts_excluding(store, job, agent, None)
}

fn contexts_excluding(
    store: &Store,
    job: &QueueJob,
    agent: &str,
    own_operation: Option<&str>,
) -> Result<Vec<Context>, Failure> {
    let ledger = store.load_feedback().map_err(Failure::permanent)?;
    let publications = store.load_publications().map_err(Failure::permanent)?;
    let mut contexts = Vec::new();
    for origin in publications.iter().filter(|p| same_pr(&p.review.job, job)) {
        if own_operation == Some(origin.review.operation.id.as_str()) {
            continue;
        }
        let Some(receipt) = origin
            .receipts
            .last()
            .filter(|r| r.state == RemoteState::Commented)
        else {
            continue;
        };
        let earlier = origin.review.job.work.as_ref().map(|w| &w.iteration_id)
            != job.work.as_ref().map(|w| &w.iteration_id);
        for root in &receipt.comment_ids {
            let id = root_key(job, root);
            let record = ledger.records.iter().find(|r| r.context.id == id);
            if !earlier
                && origin.review.selection.agent.id != agent
                && !record.is_some_and(|r| r.context.closed)
            {
                continue;
            }
            let record = record.ok_or_else(|| {
                Failure::permanent("Waiting for verified prior-feedback observations.")
            })?;
            if record.observed_head != job.head_sha || record.context.unavailable.is_some() {
                return Err(Failure::permanent(
                    "Prior feedback is unavailable or not observed at this revision.",
                ));
            }
            contexts.push(record.context.clone());
        }
    }
    contexts.extend(crate::retention::retained_contexts(store, job).map_err(Failure::permanent)?);
    contexts.sort_by(|a, b| a.id.cmp(&b.id));
    contexts.dedup_by(|a, b| a.id == b.id);
    if serde_json::to_vec(&contexts)
        .map_err(|_| Failure::permanent("Cannot encode feedback context."))?
        .len()
        > 1024 * 1024
    {
        return Err(Failure::permanent(
            "Verified feedback context exceeds the bounded analysis size; nothing was truncated.",
        ));
    }
    Ok(contexts)
}

pub fn validate_context(
    store: &Store,
    job: &QueueJob,
    agent: &str,
    captured: &[Context],
) -> Result<(), Failure> {
    if contexts(store, job, agent)? != captured {
        return Err(Failure::permanent(
            "Owned feedback changed during analysis; no stale assessment accepted.",
        ));
    }
    if !captured.is_empty()
        && store
            .load_monitoring_state()
            .map_err(Failure::permanent)?
            .health
            .values()
            .any(|h| {
                h.repository_id == job.configuration_id
                    && (h.last_failure.is_some() || h.conversation_admission_pending)
            })
    {
        return Err(Failure::permanent(
            "Feedback observation failed; no clearance accepted.",
        ));
    }
    Ok(())
}

pub fn validate_assessments(
    values: &[Assessment],
    contexts: &[Context],
    agent: &str,
    complete: bool,
) -> Result<(), Failure> {
    let expected: HashSet<_> = contexts
        .iter()
        .filter(|c| c.owner_agent_id == agent && !c.closed)
        .map(|c| c.id.as_str())
        .collect();
    let mut actual = HashSet::new();
    for assessment in values {
        if !expected.contains(assessment.feedback_id.as_str())
            || !actual.insert(assessment.feedback_id.as_str())
            || assessment.reason.trim().is_empty()
            || assessment.reason.chars().count() > 2000
            || (assessment.disposition == Disposition::Cleared && assessment.evidence.is_empty())
        {
            return Err(Failure::permanent("Feedback assessments must reference unique open concerns owned by this Agent, with evidence for clearance."));
        }
    }
    if complete && actual != expected {
        return Err(Failure::permanent(
            "Review did not reassess every earlier open owned concern.",
        ));
    }
    Ok(())
}

pub fn duplicates(finding: &Finding, context: &Context) -> bool {
    finding.feedback_id.as_deref() == Some(&context.id)
        || (!context.title.is_empty()
            && finding.path == context.path
            && finding
                .title
                .trim()
                .eq_ignore_ascii_case(context.title.trim()))
}

pub fn validate_review(
    output: &ReviewOutput,
    contexts: &[Context],
    agent: &str,
) -> Result<(), Failure> {
    validate_assessments(&output.feedback_assessments, contexts, agent, true)?;
    if output
        .findings
        .iter()
        .any(|f| f.feedback_id.is_some() || contexts.iter().any(|c| duplicates(f, c)))
    {
        return Err(Failure::permanent("Existing or closed concerns must not be republished as new findings. Use their feedback assessment identities."));
    }
    Ok(())
}

pub fn suppress_closed_overlap(
    output: &mut ReviewOutput,
    contexts: &[Context],
    files: &[crate::github::review::ReviewFile],
) {
    // A paraphrase on the same/renamed file is not evidence of a new concern.
    // Keep the result human-gated instead of automatically reviving a closed root.
    let findings = std::mem::take(&mut output.findings);
    for finding in findings {
        if contexts.iter().any(|context| {
            context.closed
                && (finding.path == context.path
                    || files.iter().any(|file| {
                        file.path == finding.path
                            && file.previous_path.as_deref() == Some(&context.path)
                    }))
        }) {
            output.held_findings.push(finding);
        } else {
            output.findings.push(finding);
        }
    }
    if !output.held_findings.is_empty() {
        output.decision = crate::review::Decision::HumanInputRequired;
        output.feedback_conflict = true;
    }
}

pub fn publication_gate(store: &Store, review: &crate::review::ReviewRun) -> Result<(), String> {
    if review
        .result
        .as_ref()
        .is_some_and(|r| r.output.feedback_conflict)
    {
        return Err("Possible reintroduction of human-closed feedback requires human judgment; no automatic or manual machine batch is created.".into());
    }
    let ledger = store.load_feedback()?;
    let publications = store.load_publications()?;
    if let Some(captured) = &review.feedback_context {
        let current = contexts_excluding(
            store,
            &review.job,
            &review.selection.agent.id,
            Some(&review.operation.id),
        )
        .map_err(|e| e.message)?;
        if &current != captured {
            return Err("Prior feedback changed after this review; reconcile its original publication but do not send new stale feedback.".into());
        }
    }
    let duplicate = review.result.as_ref().is_some_and(|result| {
        result.output.findings.iter().any(|finding| {
            ledger.records.iter().any(|record| {
                same_pr(&record.job, &review.job)
                    && record.context.closed
                    && (duplicates(finding, &record.context) || finding.path == record.context.path)
                    && !publications.iter().any(|p| {
                        p.id == record.context.publication_id
                            && p.review.operation.id == review.operation.id
                    })
            })
        })
    });
    if duplicate {
        return Err(
            "A human-closed concern must not be republished, even from an earlier saved result."
                .into(),
        );
    }
    Ok(())
}

pub fn views(store: &Store, job: &QueueJob) -> Result<Vec<View>, String> {
    let ledger = store.load_feedback()?;
    let reviews = store.review_evidence()?;
    let replies = store.load_follow_ups()?;
    let settings = store.load_settings()?;
    let mut views = Vec::new();
    for record in ledger.records.iter().filter(|r| same_pr(&r.job, job)) {
        let context = &record.context;
        let owner = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id)
            .is_some_and(|r| {
                r.assignments.iter().any(|a| {
                    a.id == context.owner_assignment_id && a.agent_id == context.owner_agent_id
                })
            })
            && settings.agents.iter().any(|a| {
                a.id == context.owner_agent_id
                    && a.ai_account.is_some()
                    && !a.model.trim().is_empty()
            });
        let mut state = if context.unavailable.is_some() {
            "unavailable"
        } else if context.closed {
            "closed"
        } else if !owner {
            "owner_unavailable"
        } else {
            "open"
        };
        let mut reason = context.unavailable.clone();
        if state == "open" && record.observed_head == job.head_sha {
            let mut assessed = Vec::new();
            for review in reviews.iter().filter(|r| {
                same_pr(&r.job, job)
                    && r.job.head_sha == job.head_sha
                    && r.job.work.as_ref().map(|w| &w.iteration_id)
                        == job.work.as_ref().map(|w| &w.iteration_id)
                    && r.operation.state == OperationState::Completed
                    && r.feedback_context.iter().flatten().any(|c| c == context)
            }) {
                if let Some(result) = &review.result {
                    assessed.extend(
                        result
                            .output
                            .feedback_assessments
                            .iter()
                            .filter(|a| a.feedback_id == context.id)
                            .map(|a| (review.operation.initial_attempt_at, a)),
                    );
                }
            }
            for reply in replies.iter().filter(|r| {
                r.context.job.head_sha == job.head_sha
                    && same_pr(&r.context.job, job)
                    && r.context.job.work.as_ref().map(|w| &w.iteration_id)
                        == job.work.as_ref().map(|w| &w.iteration_id)
                    && r.context.feedback.iter().any(|c| c == context)
                    && r.analysis
                        .as_ref()
                        .is_some_and(|o| o.state == OperationState::Completed)
            }) {
                if let Some(result) = &reply.result {
                    assessed.extend(
                        result
                            .output
                            .feedback_assessments
                            .iter()
                            .filter(|a| a.feedback_id == context.id)
                            .map(|a| (reply.analysis.as_ref().unwrap().initial_attempt_at, a)),
                    );
                }
            }
            if let Some((_, assessment)) = assessed.into_iter().max_by_key(|(at, _)| *at) {
                state = match assessment.disposition {
                    Disposition::Open => "open",
                    Disposition::Cleared => "cleared",
                    Disposition::HumanInputRequired => "human_input_required",
                };
                reason = Some(assessment.reason.clone());
            }
        }
        views.push(View {
            context: context.clone(),
            state,
            reason,
        });
    }
    for origin in store
        .load_publications()?
        .iter()
        .filter(|p| same_pr(&p.review.job, job))
    {
        if let Some(receipt) = origin
            .receipts
            .last()
            .filter(|r| r.state == RemoteState::Commented)
        {
            for root in &receipt.comment_ids {
                let id = root_key(job, root);
                if views.iter().any(|v| v.context.id == id) {
                    continue;
                }
                views.push(View {
                    context: Context {
                        id,
                        publication_id: origin.id.clone(),
                        owner_agent_id: origin.review.selection.agent.id.clone(),
                        owner_assignment_id: origin.review.assignment_id.clone(),
                        original_head: origin.review.job.head_sha.clone(),
                        root_id: root.clone(),
                        path: String::new(),
                        title: String::new(),
                        body: String::new(),
                        thread: None,
                        closed: false,
                        unavailable: Some("Published feedback has not been observed.".into()),
                    },
                    state: "unavailable",
                    reason: Some("Waiting for verified feedback observations.".into()),
                });
            }
        }
    }
    Ok(views)
}

pub fn owned_reply_assessment(run: &FollowUp) -> Result<(), Failure> {
    let Some(result) = &run.result else {
        return Ok(());
    };
    validate_assessments(
        &result.output.feedback_assessments,
        &run.context.feedback,
        &run.context.selection.agent.id,
        false,
    )?;
    if matches!(run.target, ConversationTarget::Mention { .. })
        && !result.output.feedback_assessments.is_empty()
    {
        return Err(Failure::permanent(
            "A top-level mention cannot clear another thread's concern.",
        ));
    }
    if result.output.decision == ReplyDecision::HumanInputRequired
        && result
            .output
            .feedback_assessments
            .iter()
            .any(|a| a.disposition == Disposition::Cleared)
    {
        return Err(Failure::permanent(
            "Human judgment cannot be converted into automated clearance.",
        ));
    }
    if let Ok(thread) = run.thread() {
        let id = root_key(&run.context.job, &thread.root()?.id);
        if result
            .output
            .feedback_assessments
            .iter()
            .any(|a| a.feedback_id != id)
        {
            return Err(Failure::permanent(
                "A reply can reassess only its own concern.",
            ));
        }
    }
    Ok(())
}
