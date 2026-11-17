//! Revision sign-off comments: when the primary's review of a revision settles
//! clean (`lgtm`, `ready_for_human`, `approve_pull_request`, `approve_and_merge`),
//! the host posts one fixed PR-level comment naming that revision. Repository
//! switch `sign_off_comments`, inert without the primary's Publish Comment grant.
//!
//! Once per revision: a ledger record per (PR, head) in `state/feedback.json`, an
//! intent saved before the write, and a remote read for the revision's marker
//! before every post. An uncertain write is never re-posted; it is only read back.

pub(crate) mod host;

use crate::{
    exit_writes::SIGNATURE,
    exits::Exit,
    monitoring::{OperationState, QueueJob},
    notifications::Category,
    provider::{Lifecycle, PullRequest, TopComment},
    publication::WriteFailure,
    queue::{self, Snapshot, State as ItemState},
    review::Failure,
    storage::Settings,
};
use serde::{Deserialize, Serialize};

const MARKER_PREFIX: &str = "<!-- pr-sniper:signoff:";
/// Posts, or read-backs of an uncertain post, before a record gives up.
pub const ATTEMPTS: u8 = 3;
/// Outcomes settled longer ago than this (before the switch was on) get none.
pub const FRESH_SECONDS: i64 = 24 * 60 * 60;
/// Unfinished records older than this give up.
const EXPIRY_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Recheck delay while a gate (pause, connection) holds the write.
const WAIT_SECONDS: i64 = 60;

pub fn marker(head: &str) -> String {
    format!("{MARKER_PREFIX}{head} -->")
}

/// The fixed comment for `head`. Host text only: no agent prose or doctrine names.
pub fn body(head: &str) -> String {
    let short: String = head.chars().take(7).collect();
    format!(
        "Revision `{short}` looks good. No issues found.\n\n{}\n\n{SIGNATURE}",
        marker(head)
    )
}

/// `text` is exactly a sign-off comment PR Sniper writes, for any revision.
/// The heartbeat ignores these so the host's own sign-off never retriggers a review.
pub fn is_sign_off(text: &str) -> bool {
    let Some(start) = text.find(MARKER_PREFIX) else {
        return false;
    };
    let rest = &text[start + MARKER_PREFIX.len()..];
    let Some(end) = rest.find(" -->") else {
        return false;
    };
    let head = &rest[..end];
    !head.is_empty()
        && head.bytes().all(|b| b.is_ascii_alphanumeric())
        && text.replace("\r\n", "\n").trim() == body(head)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Planned,
    /// Intent saved; the post may or may not have landed. Never posted again.
    Attempted,
    Done,
    /// Not posted: the revision moved, the PR closed, or the switch or grant is off.
    Skipped,
    /// Gave up without a confirmed comment.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignOff {
    pub id: String,
    pub job: QueueJob,
    pub head: String,
    /// Planned from a PR PR Sniper merged: the merged PR still gets its comment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merged: bool,
    pub phase: Phase,
    pub attempts: u8,
    pub next_at: i64,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

impl SignOff {
    pub fn terminal(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Skipped | Phase::Failed)
    }

    pub fn due(&self, now: i64) -> bool {
        !self.terminal() && self.next_at <= now
    }

    /// Same pull request and revision.
    pub fn covers(&self, job: &QueueJob, head: &str) -> bool {
        self.head == head
            && self.job.provider == job.provider
            && self.job.account_id == job.account_id
            && self.job.repository_id == job.repository_id
            && self.job.pull_request_id == job.pull_request_id
    }
}

/// When the item's clean outcome settled: its primary run, or a later approve or merge.
fn settled_at(item: &queue::Item, snapshot: &Snapshot) -> Option<i64> {
    let run = snapshot
        .reviews
        .iter()
        .filter_map(|r| r.run.as_ref())
        .find(|r| Some(&r.operation.id) == item.exit_run_id.as_ref())?;
    let effects = item.action_status.iter().flat_map(|s| &s.effects).map(|e| {
        e.operation
            .completed_at
            .unwrap_or(e.operation.initial_attempt_at)
    });
    run.operation.completed_at.into_iter().chain(effects).max()
}

/// The item's primary run settled clean on this revision; `Some(merged)`.
fn clean(item: &queue::Item, snapshot: &Snapshot, primary: &str) -> Option<bool> {
    if !matches!(
        item.exit,
        Some(Exit::Lgtm | Exit::ReadyForHuman | Exit::ApprovePullRequest | Exit::ApproveAndMerge)
    ) {
        return None;
    }
    let run = snapshot
        .reviews
        .iter()
        .find(|r| {
            r.run
                .as_ref()
                .is_some_and(|run| Some(&run.operation.id) == item.exit_run_id.as_ref())
        })
        .filter(|r| r.assignment_id == primary)?;
    if run
        .run
        .as_ref()
        .is_none_or(|r| r.operation.state != OperationState::Completed)
    {
        return None;
    }
    if item.state == ItemState::Merged {
        // Only a merge PR Sniper confirmed; someone else's merge settles nothing here.
        return item
            .action_status
            .iter()
            .flat_map(|s| &s.effects)
            .any(|e| e.state == crate::actions::EffectState::Confirmed)
            .then_some(true);
    }
    if item.rest.is_some() {
        return None;
    }
    matches!(
        crate::notifications::item_category(item),
        Some(Category::Ready | Category::FinalReviewCleared | Category::Approved)
    )
    .then_some(false)
}

/// New records for revisions that settled clean since the last pass.
pub fn plan(
    settings: &Settings,
    snapshot: &Snapshot,
    existing: &[SignOff],
    now: i64,
) -> Vec<SignOff> {
    let mut planned: Vec<SignOff> = Vec::new();
    for item in &snapshot.items {
        let job = &item.job;
        if job.head_sha.is_empty()
            || existing
                .iter()
                .chain(&planned)
                .any(|r| r.covers(job, &job.head_sha))
        {
            continue;
        }
        let Some(repository) = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id && r.enabled && r.sign_off_comments_active())
        else {
            continue;
        };
        let Some(primary) = repository.primary_assignment_id() else {
            continue;
        };
        let Some(merged) = clean(item, snapshot, primary) else {
            continue;
        };
        if settled_at(item, snapshot).is_none_or(|at| at < now.saturating_sub(FRESH_SECONDS)) {
            continue;
        }
        planned.push(SignOff {
            id: uuid::Uuid::new_v4().to_string(),
            job: job.clone(),
            head: job.head_sha.clone(),
            merged,
            phase: Phase::Planned,
            attempts: 0,
            next_at: now,
            created_at: now,
            receipt: None,
        });
    }
    planned
}

/// The record due next, oldest retry time first.
pub fn next_due(records: &[SignOff], now: i64) -> Option<&SignOff> {
    records
        .iter()
        .filter(|r| r.due(now))
        .min_by_key(|r| (r.next_at, r.created_at))
}

/// What the gates allow right now.
pub enum Gate {
    /// Write allowed; `review_drafts` is the repository's draft rule.
    Open { review_drafts: bool },
    /// The switch, grant, repository or scope is off: never posted.
    Refused,
    /// Pause or a connection change: check again later.
    Hold,
}

pub trait Environment {
    fn now(&self) -> Result<i64, Failure>;
    fn save(&mut self, record: &SignOff) -> Result<(), Failure>;
    fn gate(&mut self, record: &SignOff) -> Result<Gate, Failure>;
    fn pull(&mut self, record: &SignOff) -> Result<PullRequest, Failure>;
    /// PR-level comments: GitHub conversation comments, Azure DevOps general threads.
    fn comments(&mut self, record: &SignOff) -> Result<Vec<TopComment>, Failure>;
    fn post(&mut self, record: &SignOff, body: &str) -> Result<String, WriteFailure>;
}

fn backoff(attempts: u8) -> i64 {
    60 << attempts.min(6)
}

/// A read or a write failed: try again later, until the attempts run out.
fn retry(env: &mut dyn Environment, record: &mut SignOff, now: i64) -> Result<(), Failure> {
    record.attempts = record.attempts.saturating_add(1);
    if record.attempts >= ATTEMPTS {
        record.phase = Phase::Failed;
    } else {
        record.next_at = now + backoff(record.attempts);
    }
    env.save(record)
}

fn finish(env: &mut dyn Environment, record: &mut SignOff, phase: Phase) -> Result<(), Failure> {
    record.phase = phase;
    env.save(record)
}

/// One step of one record: gate, fresh read, marker check, then at most one post.
pub fn run(env: &mut dyn Environment, record: &mut SignOff) -> Result<(), Failure> {
    if record.terminal() {
        return Ok(());
    }
    let now = env.now()?;
    if now.saturating_sub(record.created_at) > EXPIRY_SECONDS {
        return finish(env, record, Phase::Failed);
    }
    let review_drafts = match env.gate(record)? {
        Gate::Open { review_drafts } => review_drafts,
        Gate::Refused => return finish(env, record, Phase::Skipped),
        Gate::Hold => {
            record.next_at = now + WAIT_SECONDS;
            return env.save(record);
        }
    };
    let pull = match env.pull(record) {
        Ok(pull) => pull,
        Err(_) => return retry(env, record, now),
    };
    let open = pull.state == Lifecycle::Open && (!pull.draft || review_drafts);
    let merged = record.merged && pull.state == Lifecycle::Merged;
    if pull.head_sha != record.head || !(open || merged) {
        return finish(env, record, Phase::Skipped);
    }
    let comments = match env.comments(record) {
        Ok(comments) => comments,
        Err(_) => return retry(env, record, now),
    };
    // Only our account's exact sign-off for this head; a quoted or pasted marker is not one.
    let marker = marker(&record.head);
    let own = |c: &&TopComment| {
        c.author_id.as_deref() == Some(record.job.account_id.as_str())
            && c.body.contains(&marker)
            && is_sign_off(&c.body)
    };
    if let Some(found) = comments.iter().find(own) {
        record.receipt = Some(found.id.clone());
        return finish(env, record, Phase::Done);
    }
    if record.phase == Phase::Attempted {
        // The earlier post may still land; read it back, never post again.
        return retry(env, record, now);
    }
    record.phase = Phase::Attempted;
    env.save(record)?;
    match env.post(record, &body(&record.head)) {
        Ok(id) => {
            record.receipt = Some(id);
            finish(env, record, Phase::Done)
        }
        Err(failure) => {
            if !failure.uncertain {
                record.phase = Phase::Planned;
            }
            retry(env, record, now)
        }
    }
}

#[cfg(test)]
mod tests;
