//! Revision sign-off comments: when the primary's review of a revision settles
//! clean (`lgtm`, `ready_for_human`, `approve_pull_request`, `approve_and_merge`),
//! the host says so in one fixed PR-level comment it keeps per PR and acting
//! account. Repository switch `sign_off_comments`, inert without the primary's
//! Publish Comment grant.
//!
//! The first clean revision posts the comment; later revisions update it:
//! GitHub edits it in place, Azure DevOps replies in its closed thread. A later
//! revision whose findings were published adds a fixed "has new findings" line.
//!
//! Once per revision: a ledger record per (PR, head) in `state/feedback.json`, an
//! intent saved before the write, and a remote read for the revision's text
//! before every write. An uncertain write is never repeated; it is only read back.

pub(crate) mod host;

use crate::{
    exit_writes::SIGNATURE,
    exits::Exit,
    monitoring::{OperationState, QueueJob},
    notifications::Category,
    provider::{Lifecycle, PrLevelThread, PullRequest},
    publication::WriteFailure,
    queue::{self, Snapshot, State as ItemState},
    review::Failure,
    storage::Settings,
};
use serde::{Deserialize, Serialize};

const MARKER_PREFIX: &str = "<!-- pr-sniper:signoff:";
const FINDINGS_PREFIX: &str = "<!-- pr-sniper:signoff-findings:";
/// Writes, or read-backs of an uncertain write, before a record gives up.
pub const ATTEMPTS: u8 = 3;
/// Outcomes settled longer ago than this (before the switch was on) get none.
pub const FRESH_SECONDS: i64 = 24 * 60 * 60;
/// Unfinished records older than this give up.
const EXPIRY_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Recheck delay while a gate (pause, connection) holds the write.
const WAIT_SECONDS: i64 = 60;

fn short(head: &str) -> String {
    head.chars().take(7).collect()
}

pub fn marker(head: &str) -> String {
    format!("{MARKER_PREFIX}{head} -->")
}

fn findings_marker(head: &str) -> String {
    format!("{FINDINGS_PREFIX}{head} -->")
}

/// The fixed clean line for `head`. Host text only: no agent prose or doctrine names.
pub fn body(head: &str) -> String {
    format!(
        "Revision `{}` looks good. No issues found.\n\n{}\n\n{SIGNATURE}",
        short(head),
        marker(head)
    )
}

/// The fixed line for a later revision whose findings were published.
pub fn findings_body(head: &str) -> String {
    format!(
        "Revision `{}` has new findings.\n\n{}\n\n{SIGNATURE}",
        short(head),
        findings_marker(head)
    )
}

/// A GitHub sign-off comment edited for a later revision with findings: the
/// last clean revision, then the revision with findings.
pub fn clean_then_findings(clean: &str, findings: &str) -> String {
    format!(
        "Revision `{}` looks good. No issues found.\n\nRevision `{}` has new findings.\n\n{}\n{}\n\n{SIGNATURE}",
        short(clean),
        short(findings),
        marker(clean),
        findings_marker(findings)
    )
}

/// What one exact PR Sniper sign-off text says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub clean: Option<String>,
    pub findings: Option<String>,
}

impl Said {
    fn text(&self) -> Option<String> {
        match (&self.clean, &self.findings) {
            (Some(clean), None) => Some(body(clean)),
            (Some(clean), Some(findings)) => Some(clean_then_findings(clean, findings)),
            (None, Some(findings)) => Some(findings_body(findings)),
            (None, None) => None,
        }
    }

    /// The text says this record's revision and outcome.
    fn covers(&self, kind: Kind, head: &str) -> bool {
        match kind {
            Kind::Clean => self.clean.as_deref() == Some(head) && self.findings.is_none(),
            Kind::Findings => self.findings.as_deref() == Some(head),
        }
    }
}

fn head_after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = &text[text.find(prefix)? + prefix.len()..];
    let head = &rest[..rest.find(" -->")?];
    (!head.is_empty() && head.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(head)
}

/// What `text` says, if it is exactly a sign-off text PR Sniper writes.
pub fn parse(text: &str) -> Option<Said> {
    let text = text.replace("\r\n", "\n");
    let text = text.trim();
    let said = Said {
        clean: head_after(text, MARKER_PREFIX).map(str::to_owned),
        findings: head_after(text, FINDINGS_PREFIX).map(str::to_owned),
    };
    (said.text().as_deref() == Some(text)).then_some(said)
}

/// `text` is exactly a sign-off text PR Sniper writes, for any revision.
/// The heartbeat ignores these so the host's own sign-off never retriggers a review.
pub fn is_sign_off(text: &str) -> bool {
    parse(text).is_some()
}

/// What a record says about its revision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Clean,
    /// Published findings on a revision after a clean one.
    Findings,
}

impl Kind {
    fn is_clean(&self) -> bool {
        *self == Kind::Clean
    }
}

/// How a provider updates its tracked sign-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// GitHub: edit the conversation comment; it always holds the latest state.
    Edit,
    /// Azure DevOps: reply in the closed thread; earlier revisions stay readable.
    Reply,
}

pub fn update_style(provider: &str) -> Update {
    if provider == "azure_devops" {
        Update::Reply
    } else {
        Update::Edit
    }
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
    #[serde(default, skip_serializing_if = "Kind::is_clean")]
    pub kind: Kind,
    pub phase: Phase,
    pub attempts: u8,
    pub next_at: i64,
    pub created_at: i64,
    /// The PR's tracked sign-off comment (GitHub comment, Azure DevOps general
    /// thread root) this record wrote to or found.
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

    /// Same pull request and acting account.
    pub fn same_pr(&self, job: &QueueJob) -> bool {
        self.job.provider == job.provider
            && self.job.account_id == job.account_id
            && self.job.repository_id == job.repository_id
            && self.job.pull_request_id == job.pull_request_id
    }

    /// Same pull request and revision.
    pub fn covers(&self, job: &QueueJob, head: &str) -> bool {
        self.head == head && self.same_pr(job)
    }
}

/// The PR's tracked sign-off comment: the newest other record's receipt.
/// Per-revision records from before tracking migrate as they are.
pub fn tracked(records: &[SignOff], record: &SignOff) -> Option<String> {
    records
        .iter()
        .filter(|r| r.id != record.id && r.same_pr(&record.job))
        .filter_map(|r| Some((r.created_at, r.receipt.clone()?)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, id)| id)
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

/// The item's primary run published new findings on this revision: when.
fn published_findings(item: &queue::Item, snapshot: &Snapshot, primary: &str) -> Option<i64> {
    if item.state != ItemState::WaitingForAuthor || item.rest.is_some() {
        return None;
    }
    let run = snapshot
        .reviews
        .iter()
        .filter(|r| r.assignment_id == primary)
        .filter_map(|r| r.run.as_ref())
        .find(|r| Some(&r.operation.id) == item.exit_run_id.as_ref())?;
    if run.operation.state != OperationState::Completed || run.job.head_sha != item.job.head_sha {
        return None;
    }
    // Only a confirmed publication with comments: local-only findings say nothing.
    let publication = snapshot
        .publications
        .iter()
        .filter(|c| c.review_operation_id == run.operation.id && !c.local_only)
        .find_map(|c| c.publication.as_ref())?;
    let confirmed = publication.phase == crate::publication::Phase::Published
        && publication.operation.state == OperationState::Completed
        && publication.receipts.iter().any(|r| {
            r.state == crate::publication::RemoteState::Commented && !r.comment_ids.is_empty()
        });
    confirmed.then(|| {
        publication
            .operation
            .completed_at
            .unwrap_or(publication.operation.initial_attempt_at)
    })
}

/// New records for revisions that settled since the last pass: clean ones,
/// and ones with published findings on a PR that already has a sign-off.
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
        let (kind, merged, at) = match clean(item, snapshot, primary) {
            Some(merged) => (Kind::Clean, merged, settled_at(item, snapshot)),
            None => {
                let signed = existing
                    .iter()
                    .any(|r| r.same_pr(job) && r.phase == Phase::Done && r.receipt.is_some());
                if !signed {
                    continue;
                }
                let Some(at) = published_findings(item, snapshot, primary) else {
                    continue;
                };
                (Kind::Findings, false, Some(at))
            }
        };
        if at.is_none_or(|at| at < now.saturating_sub(FRESH_SECONDS)) {
            continue;
        }
        planned.push(SignOff {
            id: uuid::Uuid::new_v4().to_string(),
            job: job.clone(),
            head: job.head_sha.clone(),
            merged,
            kind,
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
    /// The PR's tracked sign-off comment from the ledger.
    fn tracked(&mut self, record: &SignOff) -> Result<Option<String>, Failure>;
    fn gate(&mut self, record: &SignOff) -> Result<Gate, Failure>;
    fn pull(&mut self, record: &SignOff) -> Result<PullRequest, Failure>;
    /// PR-level conversations: GitHub conversation comments, Azure DevOps general threads.
    fn threads(&mut self, record: &SignOff) -> Result<Vec<PrLevelThread>, Failure>;
    fn post(&mut self, record: &SignOff, body: &str) -> Result<String, WriteFailure>;
    /// GitHub edits comment `id`; Azure DevOps replies in its thread, kept closed.
    fn update(&mut self, record: &SignOff, id: &str, body: &str) -> Result<(), WriteFailure>;
    /// Closes the thread of comment `id` (Azure DevOps).
    fn close(&mut self, record: &SignOff, id: &str) -> Result<(), WriteFailure>;
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

/// What the acting account's own sign-off texts in `thread` say, oldest first.
/// A quoted or pasted marker, or another author's copy, is none.
fn own<'a>(thread: &'a PrLevelThread, account: &'a str) -> impl Iterator<Item = Said> + 'a {
    thread
        .comments
        .iter()
        .filter(move |c| c.author_id.as_deref() == Some(account))
        .filter_map(|c| parse(&c.body))
}

/// A thread PR Sniper started: its root is the acting account's sign-off.
fn ours(thread: &PrLevelThread, account: &str) -> bool {
    thread.comments.first().is_some_and(|root| {
        root.author_id.as_deref() == Some(account) && parse(&root.body).is_some()
    })
}

/// The text that brings `anchor` up to this record.
fn update_text(record: &SignOff, anchor: &PrLevelThread) -> Option<String> {
    match (update_style(&record.job.provider), record.kind) {
        (Update::Reply, Kind::Clean) | (Update::Edit, Kind::Clean) => Some(body(&record.head)),
        (Update::Reply, Kind::Findings) => Some(findings_body(&record.head)),
        (Update::Edit, Kind::Findings) => {
            let clean = parse(&anchor.comments.first()?.body)?.clean?;
            Some(clean_then_findings(&clean, &record.head))
        }
    }
}

/// One step of one record: gate, fresh read, read-back, then at most one write.
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
    let threads = match env.threads(record) {
        Ok(threads) => threads,
        Err(_) => return retry(env, record, now),
    };
    let account = record.job.account_id.clone();
    let tracked = match &record.receipt {
        Some(id) => Some(id.clone()),
        None => env.tracked(record)?,
    };
    // The tracked comment; else, never tracked, the newest own sign-off is adopted.
    let anchor = match &tracked {
        Some(id) => threads.iter().find(|t| &t.id == id && ours(t, &account)),
        None => threads
            .iter()
            .filter(|t| ours(t, &account))
            .max_by(|a, b| a.comments[0].created_at.cmp(&b.comments[0].created_at)),
    };
    // Already said: the tracked comment first, then any own one (a landed post).
    let said = anchor.into_iter().chain(&threads).find(|t| {
        ours(t, &account) && own(t, &account).any(|s| s.covers(record.kind, &record.head))
    });
    if let Some(found) = said {
        record.receipt = Some(found.id.clone());
        // A sign-off never holds a comment-resolution policy: an open thread is closed again.
        if found.open && env.close(record, &found.id).is_err() {
            return retry(env, record, now);
        }
        return finish(env, record, Phase::Done);
    }
    if record.phase == Phase::Attempted {
        // The earlier write may still land; read it back, never write again.
        return retry(env, record, now);
    }
    let Some(anchor) = anchor else {
        if record.kind == Kind::Findings {
            // Nothing tracked to update; findings never start a sign-off.
            return finish(env, record, Phase::Skipped);
        }
        // First sign-off, or the tracked one was deleted: one new comment.
        record.receipt = None;
        record.phase = Phase::Attempted;
        env.save(record)?;
        return match env.post(record, &body(&record.head)) {
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
        };
    };
    let Some(text) = update_text(record, anchor) else {
        return finish(env, record, Phase::Skipped);
    };
    record.receipt = Some(anchor.id.clone());
    record.phase = Phase::Attempted;
    env.save(record)?;
    match env.update(record, &anchor.id, &text) {
        Ok(()) => finish(env, record, Phase::Done),
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
