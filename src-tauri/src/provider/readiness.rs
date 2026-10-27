//! Provider-neutral merge readiness, check status and reviewer outcomes.
//! Each provider maps its review, check and merge state onto these shapes
//! using the shared state vocabulary (`OPEN`, `MERGEABLE`, `SUCCESS`, ...).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use super::threads::TopComment;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Approve,
    Merge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeMethod {
    Merge,
    Rebase,
    Squash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReview {
    pub id: String,
    pub actor_id: String,
    pub head: String,
    pub state: String,
    pub body: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanThread {
    pub id: String,
    pub resolved: bool,
    pub outdated: bool,
    pub resolved_by: Option<String>,
    pub comments: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub write_capability: bool,
    pub node_id: String,
    pub repository_id: String,
    pub pull_request_id: String,
    pub account_id: String,
    pub author_id: String,
    pub head_repository_id: Option<String>,
    pub head: String,
    pub base: String,
    pub base_name: String,
    pub merge_rules: Option<Vec<Value>>,
    pub merge_rules_error: Option<String>,
    pub state: String,
    pub draft: bool,
    pub permission: String,
    pub mergeable: String,
    pub merge_state: String,
    pub review_decision: Option<String>,
    pub checks: Option<String>,
    pub check_contexts: Vec<Value>,
    pub in_merge_queue: bool,
    pub method: Option<MergeMethod>,
    pub protection: Value,
    pub threads: Vec<HumanThread>,
    pub reviews: Vec<ProviderReview>,
    pub comments: Vec<TopComment>,
    pub merged_by: Option<String>,
    pub merged_at: Option<String>,
    pub merge_commit: Option<String>,
}

/// Merge blocker when branch protection needs an up-to-date branch.
pub const BEHIND_BASE: &str =
    "Branch is behind base; the author must update it. PR Sniper never updates branches.";

impl Observation {
    pub fn common_blocker(&self) -> Option<String> {
        if self.state != "OPEN" {
            return Some(format!("GitHub reports {}.", self.state));
        }
        if self.draft {
            return Some("Draft pull requests cannot be approved or merged.".into());
        }
        if !self.write_capability
            || !matches!(self.permission.as_str(), "WRITE" | "MAINTAIN" | "ADMIN")
        {
            return Some(
                "The acting account lacks verified repository write/review authority.".into(),
            );
        }
        self.pending_review_blocker()
    }
    fn pending_review_blocker(&self) -> Option<String> {
        self.reviews.iter().any(|r| r.state == "PENDING").then(|| {
            "An account review is still pending; it will not be submitted or replaced by automation.".into()
        })
    }
    /// Others' threads and requested changes block merge, never approval.
    pub fn discussion_blocker(&self) -> Option<String> {
        if let Some(reason) = self.pending_review_blocker() {
            return Some(reason);
        }
        if self.threads.iter().any(|t| !t.resolved) {
            return Some(
                "Unresolved provider discussions remain, including outdated threads.".into(),
            );
        }
        let mut latest = std::collections::BTreeMap::new();
        for review in &self.reviews {
            if review.state == "APPROVED"
                || review.state == "CHANGES_REQUESTED"
                || review.state == "DISMISSED"
            {
                latest.insert(&review.actor_id, review);
            }
        }
        if latest.values().any(|r| r.state == "CHANGES_REQUESTED") {
            return Some("A provider reviewer is still requesting changes.".into());
        }
        None
    }
    pub fn blocker(&self, action: Action) -> Option<String> {
        if let Some(reason) = self.common_blocker() {
            return Some(reason);
        }
        match action {
            Action::Approve if self.author_id == self.account_id => {
                Some("GitHub does not allow the PR author to approve their own PR.".into())
            }
            Action::Approve => None,
            Action::Merge if self.discussion_blocker().is_some() => self.discussion_blocker(),
            Action::Merge if self.in_merge_queue => {
                Some("This PR is in a provider merge queue; direct merge is unavailable.".into())
            }
            Action::Merge if self.merge_rules_error.is_some() => self.merge_rules_error.clone(),
            Action::Merge if self.merge_rules.is_none() => {
                Some("Active repository rules could not be verified for direct merge.".into())
            }
            Action::Merge
                if self
                    .merge_rules
                    .as_ref()
                    .is_some_and(|rules| rules.iter().any(|r| r["type"] == "merge_queue")) =>
            {
                Some(
                    "Repository rules require a merge queue; direct merge is not permitted.".into(),
                )
            }
            Action::Merge if self.method.is_none() => {
                Some("No provider-selected supported merge method is available.".into())
            }
            Action::Merge if self.checks.as_deref() != Some("SUCCESS") => {
                Some("Current-head CI has not been verified green.".into())
            }
            // PR Sniper never updates a branch; only the author can.
            Action::Merge if self.merge_state == "BEHIND" => Some(BEHIND_BASE.into()),
            Action::Merge if self.mergeable != "MERGEABLE" || self.merge_state != "CLEAN" => {
                Some(format!(
                    "Provider merge readiness is {}/{}; no bypass is permitted.",
                    self.mergeable, self.merge_state
                ))
            }
            Action::Merge
                if self.review_decision.as_deref() == Some("REVIEW_REQUIRED")
                    || self.review_decision.as_deref() == Some("CHANGES_REQUESTED")
                    || (self.protection["requiresApprovingReviews"] == true
                        && self.review_decision.as_deref() != Some("APPROVED")) =>
            {
                Some("Provider-required reviews are not satisfied.".into())
            }
            Action::Merge => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// A reviewer outcome the provider recorded: an approval or a merge.
pub struct OutcomeReceipt {
    pub id: String,
    pub actor_id: String,
    pub head: String,
    pub action: Action,
    pub merge_commit: Option<String>,
}

// PR state for the heartbeat diff. `PrSnapshot` is the raw read.
// `NormalizedSnapshot` keeps only fields that change when a human-visible PR
// state changes: sorted, no timestamps, no transient provider values. Its hash
// is what the heartbeat compares.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCheck {
    pub name: String,
    pub status: String,
    pub conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotComment {
    pub id: String,
    pub body: String,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotThread {
    pub id: String,
    pub resolved: bool,
    pub outdated: bool,
    pub comments: Vec<SnapshotComment>,
    pub comment_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotReview {
    pub actor: String,
    pub state: String,
    pub head: Option<String>,
}

/// Raw provider read of one PR. Public so the packet builder and exits can
/// re-read fresh state at consume time and after their own writes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSnapshot {
    pub number: u64,
    pub pull_request_id: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    /// Source branch name; information for the packet, never part of the state hash.
    #[serde(default)]
    pub head_ref: String,
    pub head: String,
    pub base: String,
    pub state: String,
    pub draft: bool,
    pub mergeable: String,
    pub review_decision: Option<String>,
    pub reviews: Vec<SnapshotReview>,
    pub checks: Vec<SnapshotCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_checks: Vec<String>,
    pub check_total: u64,
    pub threads: Vec<SnapshotThread>,
    pub thread_total: u64,
    pub comments: Vec<SnapshotComment>,
    pub comment_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedComment {
    pub id: String,
    pub body_hash: String,
    pub resolved: bool,
    pub outdated: bool,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedSnapshot {
    pub head: String,
    pub base: String,
    pub text_hash: String,
    pub lifecycle: String,
    pub mergeable: String,
    pub review_decision: Option<String>,
    pub approvals: Vec<(String, String)>,
    pub labels: Vec<String>,
    pub checks: Vec<(String, String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_checks: Vec<String>,
    pub check_total: u64,
    pub comments: Vec<NormalizedComment>,
    pub thread_total: u64,
    pub comment_total: u64,
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

impl PrSnapshot {
    /// Stable view: sorted, deduplicated, no timestamps. A re-read of an
    /// unchanged PR yields an identical value whatever order the provider used.
    pub fn normalize(&self) -> NormalizedSnapshot {
        let lifecycle = match self.state.as_str() {
            "MERGED" => "merged",
            "CLOSED" => "closed",
            _ if self.draft => "draft",
            _ => "open",
        };
        // UNKNOWN is a transient provider computation, not a PR change.
        let mergeable = if self.mergeable == "CONFLICTING" {
            "conflicting"
        } else {
            "ok"
        };
        let mut labels = self.labels.clone();
        labels.sort();
        labels.dedup();
        let mut approvals: Vec<_> = self
            .reviews
            .iter()
            .filter(|r| r.state == "APPROVED")
            .map(|r| (r.actor.clone(), r.head.clone().unwrap_or_default()))
            .collect();
        approvals.sort();
        approvals.dedup();
        let mut checks: Vec<_> = self
            .checks
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    check_status(&c.status).to_string(),
                    c.conclusion.to_ascii_lowercase(),
                )
            })
            .collect();
        checks.sort();
        checks.dedup();
        let mut required_checks = self.required_checks.clone();
        required_checks.sort();
        required_checks.dedup();
        let mut comments: Vec<_> = self
            .comments
            .iter()
            .map(|c| NormalizedComment {
                id: c.id.clone(),
                body_hash: digest(&c.body),
                resolved: false,
                outdated: false,
                commit: c.commit.clone(),
            })
            .collect();
        for thread in &self.threads {
            for c in &thread.comments {
                comments.push(NormalizedComment {
                    id: c.id.clone(),
                    body_hash: digest(&c.body),
                    resolved: thread.resolved,
                    outdated: thread.outdated,
                    commit: c.commit.clone(),
                });
            }
        }
        comments.sort_by(|a, b| a.id.cmp(&b.id));
        comments.dedup_by(|a, b| a.id == b.id);
        NormalizedSnapshot {
            head: self.head.clone(),
            base: self.base.clone(),
            text_hash: digest(&format!("{}\n{}", self.title, self.body)),
            lifecycle: lifecycle.into(),
            mergeable: mergeable.into(),
            review_decision: self.review_decision.clone(),
            approvals,
            labels,
            checks,
            required_checks,
            check_total: self.check_total,
            comments,
            thread_total: self.thread_total,
            comment_total: self.comment_total
                + self.threads.iter().map(|t| t.comment_total).sum::<u64>(),
        }
    }
}

fn check_status(status: &str) -> &'static str {
    match status {
        "COMPLETED" | "SUCCESS" | "FAILURE" | "ERROR" => "done",
        _ => "pending",
    }
}

impl NormalizedSnapshot {
    pub fn ci_pending(&self) -> bool {
        !self.ci_failed()
            && (self.checks.iter().any(|(_, status, _)| status == "pending")
                || self
                    .required_checks
                    .iter()
                    .any(|required| !self.checks.iter().any(|(name, _, _)| name == required)))
    }

    /// No check reported and none required: CI not created yet, or no CI at all.
    /// Not pending: a local sign-off judged on it (conflicts) must not toss.
    pub fn ci_unknown(&self) -> bool {
        self.checks.is_empty() && self.required_checks.is_empty()
    }

    /// CI settled green: checks reported, none pending or failed.
    pub fn ci_green(&self) -> bool {
        !self.checks.is_empty() && !self.ci_failed() && !self.ci_pending()
    }

    /// CI failed on the head: any check concluded as a failure.
    pub fn ci_failed(&self) -> bool {
        self.checks.iter().any(|(_, _, conclusion)| {
            matches!(
                conclusion.as_str(),
                "failure"
                    | "error"
                    | "cancelled"
                    | "timed_out"
                    | "action_required"
                    | "startup_failure"
            )
        })
    }

    /// The review revision: what a review reads. CI progress and results,
    /// mergeability and labels are provider metadata for the same code and
    /// conversation; exit gates read them fresh instead.
    pub fn review_view(&self) -> Self {
        Self {
            mergeable: String::new(),
            labels: Vec::new(),
            checks: Vec::new(),
            required_checks: Vec::new(),
            check_total: 0,
            ..self.clone()
        }
    }

    /// Hash of the review revision ([`Self::review_view`]); what the heartbeat compares.
    pub fn hash(&self) -> String {
        digest(&serde_json::to_string(&self.review_view()).expect("normalized snapshot serializes"))
    }

    /// Hash of the whole snapshot, as baselines were recorded before
    /// [`Self::hash`] left out CI and metadata. Only translates stored hashes.
    pub fn legacy_hash(&self) -> String {
        digest(&serde_json::to_string(self).expect("normalized snapshot serializes"))
    }
}

/// Compact packet-time state: enough to tell the host's own writes from anyone
/// else's change after an exit, without comment authors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMarks {
    /// Acting account login: its approval is an own write.
    pub actor: String,
    /// Digest of the review revision's non-conversation fields except the review
    /// decision and own approval. CI, mergeability and labels are not review changes.
    pub fields: String,
    pub review_decision: Option<String>,
    /// The acting account approved the head.
    pub own_approval: bool,
    /// Comment id -> body digest.
    pub comments: BTreeMap<String, String>,
    pub resolved_threads: Vec<String>,
}

/// Ids the host itself wrote after the packet: comments (batch and replies) and
/// threads it resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnWrites {
    pub comments: std::collections::BTreeSet<String>,
    pub resolved_threads: std::collections::BTreeSet<String>,
}

impl StateMarks {
    pub fn of(snapshot: &PrSnapshot, actor: &str) -> Self {
        let n = snapshot.normalize();
        let approvals: Vec<_> = n.approvals.iter().filter(|(a, _)| a != actor).collect();
        let fields =
            digest(&json!([n.head, n.base, n.text_hash, n.lifecycle, approvals]).to_string());
        let comments = snapshot
            .comments
            .iter()
            .chain(snapshot.threads.iter().flat_map(|t| &t.comments))
            .map(|c| (c.id.clone(), digest(&c.body)))
            .collect();
        Self {
            actor: actor.into(),
            fields,
            review_decision: snapshot.review_decision.clone(),
            own_approval: snapshot.reviews.iter().any(|r| {
                r.actor == actor
                    && r.state == "APPROVED"
                    && r.head.as_deref() == Some(&snapshot.head)
            }),
            comments,
            resolved_threads: snapshot
                .threads
                .iter()
                .filter(|t| t.resolved)
                .map(|t| t.id.clone())
                .collect(),
        }
    }

    /// `fresh` holds a change that is not one of `own`: a new or edited comment,
    /// a thread resolved or reopened by someone else, or any other field.
    pub fn external(&self, fresh: &PrSnapshot, own: &OwnWrites) -> bool {
        let now = Self::of(fresh, &self.actor);
        if now.fields != self.fields
            || (now.review_decision != self.review_decision
                && now.own_approval == self.own_approval)
        {
            return true;
        }
        if now
            .comments
            .iter()
            .any(|(id, body)| match self.comments.get(id) {
                Some(before) => before != body,
                None => !own.comments.contains(id),
            })
        {
            return true;
        }
        fresh.threads.iter().any(|t| {
            t.resolved != self.resolved_threads.contains(&t.id)
                && !own.resolved_threads.contains(&t.id)
        })
    }
}

/// Raw check log bytes; `truncated_head` when only the tail was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLog {
    pub bytes: Vec<u8>,
    pub truncated_head: bool,
}
