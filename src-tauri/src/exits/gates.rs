//! Pure gate table: which exits the host offers for a PR state, and why not.
//! Run at packet build (offered set + reasons in the packet) and again at exit time.

use super::{Exit, ExitCall, LgtmAction, WaitReason};
use crate::github::actions::{Action, Observation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ci {
    Red,
    Pending,
    Green,
    /// No checks reported, or not verifiable.
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    pub comment: bool,
    pub reply: bool,
    pub approve: bool,
    pub merge: bool,
    pub include_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub open: bool,
    pub draft: bool,
    pub ci: Ci,
    pub conflicts: bool,
    pub required_reviews_unmet: bool,
    pub changes_requested: bool,
    pub unresolved_threads: bool,
    pub author_is_acting: bool,
    pub already_approved: bool,
    /// Provider reason approve is unavailable beyond the table (e.g. no write access).
    pub approve_blocker: Option<String>,
    /// Provider reason merge is unavailable beyond the table (queue, rules, method).
    pub merge_blocker: Option<String>,
    /// Gates remaining after only observable external prerequisites are satisfied.
    pub wait_approve_blocker: Option<String>,
    pub wait_merge_blocker: Option<String>,
    /// The facts once a sign-off resolved its own open threads ([`sign_off_facts`]).
    /// `None`: unknown, so a sign-off is judged on the facts as they are.
    pub sign_off: Option<Box<Facts>>,
}

impl Default for Facts {
    fn default() -> Self {
        Self {
            open: true,
            draft: false,
            ci: Ci::Green,
            conflicts: false,
            required_reviews_unmet: false,
            changes_requested: false,
            unresolved_threads: false,
            author_is_acting: false,
            already_approved: false,
            approve_blocker: None,
            merge_blocker: None,
            wait_approve_blocker: None,
            wait_merge_blocker: None,
            sign_off: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateInput {
    /// The assignment's configured tool ids.
    pub tools: Vec<String>,
    pub primary: bool,
    pub grants: Grants,
    pub review_drafts: bool,
    pub facts: Facts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "offer", content = "reason", rename_all = "snake_case")]
pub enum Offer {
    Offered,
    Denied(String),
}

impl Offer {
    pub fn is_offered(&self) -> bool {
        matches!(self, Offer::Offered)
    }
}

impl Facts {
    /// Normalize a provider observation. Unknown CI is never green.
    pub fn from_observation(o: &Observation) -> Self {
        // Test only whether other gates forbid waiting. Action gates always use `o`.
        let mut actionable = o.clone();
        actionable.draft = false;
        actionable.checks = Some("SUCCESS".into());
        actionable.mergeable = "MERGEABLE".into();
        actionable.merge_state = "CLEAN".into();
        actionable.review_decision = Some("APPROVED".into());
        let mut latest = std::collections::BTreeMap::new();
        for review in &o.reviews {
            if matches!(
                review.state.as_str(),
                "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED"
            ) {
                latest.insert(review.actor_id.as_str(), review);
            }
        }
        Self {
            open: o.state == "OPEN",
            draft: o.draft,
            ci: ci(o),
            conflicts: o.mergeable == "CONFLICTING" || o.merge_state == "DIRTY",
            required_reviews_unmet: o.review_decision.as_deref() == Some("REVIEW_REQUIRED")
                || (o.protection["requiresApprovingReviews"] == true
                    && !matches!(
                        o.review_decision.as_deref(),
                        Some("APPROVED" | "CHANGES_REQUESTED")
                    )),
            changes_requested: o.review_decision.as_deref() == Some("CHANGES_REQUESTED")
                || latest
                    .iter()
                    .any(|(actor, r)| *actor != o.account_id && r.state == "CHANGES_REQUESTED"),
            unresolved_threads: o.threads.iter().any(|t| !t.resolved),
            author_is_acting: o.author_id == o.account_id,
            already_approved: latest
                .get(o.account_id.as_str())
                .is_some_and(|r| r.state == "APPROVED" && r.head == o.head),
            approve_blocker: o.blocker(Action::Approve),
            merge_blocker: o.blocker(Action::Merge),
            wait_approve_blocker: actionable.blocker(Action::Approve),
            wait_merge_blocker: actionable.blocker(Action::Merge),
            sign_off: None,
        }
    }
}

/// The facts as they will be once a sign-off resolved `own` (this assignment's open
/// threads): those threads resolved, and a provider merge state `BLOCKED` while they
/// were open read as clean, since resolving them may be what unblocks it. Every other
/// gate is unchanged; the action re-reads the real state before approving or merging.
pub fn sign_off_facts(o: &Observation, own: &[String]) -> Facts {
    let mut after = o.clone();
    let mut resolved = false;
    for thread in after
        .threads
        .iter_mut()
        .filter(|t| !t.resolved && own.contains(&t.id))
    {
        thread.resolved = true;
        resolved = true;
    }
    if resolved && after.merge_state == "BLOCKED" && after.threads.iter().all(|t| t.resolved) {
        after.merge_state = "CLEAN".into();
    }
    Facts::from_observation(&after)
}

fn ci(o: &Observation) -> Ci {
    let failed = |c: &serde_json::Value| match c["__typename"].as_str() {
        Some("CheckRun") => matches!(
            c["conclusion"].as_str(),
            Some("FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE")
        ),
        Some("StatusContext") => matches!(c["state"].as_str(), Some("FAILURE" | "ERROR")),
        _ => false,
    };
    match o.checks.as_deref() {
        Some("SUCCESS") => Ci::Green,
        Some("FAILURE" | "ERROR") => Ci::Red,
        Some(_) if o.check_contexts.iter().any(failed) => Ci::Red,
        Some(_) => Ci::Pending,
        None => Ci::Unknown,
    }
}

/// The exits this assignment is offered, in a stable order, each offered or denied with a
/// short reason. Exits outside the assignment's tool set or role are absent.
pub fn offered(input: &GateInput) -> Vec<(Exit, Offer)> {
    let f = &input.facts;
    Exit::ALL
        .into_iter()
        .filter(|exit| *exit == Exit::Lgtm || input.tools.iter().any(|tool| tool == exit.id()))
        .filter(|exit| match exit {
            Exit::SubmitReport => !input.primary,
            Exit::SubmitVerdict => false,
            Exit::PanicExit => true,
            _ => input.primary,
        })
        .map(|exit| {
            let offer = match exit {
                Exit::SubmitReport
                | Exit::SubmitVerdict
                | Exit::PanicExit
                | Exit::ReadyForHuman => Offer::Offered,
                Exit::LeaveComments => {
                    if !input.grants.comment {
                        Offer::Denied("No Publish Comment grant.".into())
                    } else if !f.open {
                        Offer::Denied("PR is closed or merged.".into())
                    } else if f.draft && !input.review_drafts {
                        Offer::Denied("PR is draft and Review drafts is off.".into())
                    } else {
                        Offer::Offered
                    }
                }
                Exit::ApprovePullRequest => {
                    approve_denial(input).map_or(Offer::Offered, Offer::Denied)
                }
                Exit::ApproveAndMerge => merge_denial(input).map_or(Offer::Offered, Offer::Denied),
                // Judged on the state after its own resolutions; ownership is
                // re-verified on a fresh read when it is called.
                Exit::Lgtm => {
                    let mut after = input.clone();
                    if let Some(facts) = &f.sign_off {
                        after.facts = (**facts).clone();
                    }
                    lgtm_denial(&after).map_or(Offer::Offered, Offer::Denied)
                }
                Exit::Wait => {
                    if wait_reasons(input).is_empty() {
                        Offer::Denied(
                            "No observable external prerequisite to an enabled action.".into(),
                        )
                    } else {
                        Offer::Offered
                    }
                }
            };
            (exit, offer)
        })
        .collect()
}

pub fn wait_reasons(input: &GateInput) -> Vec<WaitReason> {
    let f = &input.facts;
    if !input.primary
        || !f.open
        || f.conflicts
        || f.ci == Ci::Red
        || f.changes_requested
        || f.unresolved_threads
    {
        return vec![];
    }
    let merge = input.grants.merge
        && input.tools.iter().any(|t| t == Exit::ApproveAndMerge.id())
        && (!f.author_is_acting || input.grants.include_self)
        && f.wait_merge_blocker.is_none();
    let approve = (input.grants.approve || input.grants.merge)
        && input
            .tools
            .iter()
            .any(|t| t == Exit::ApprovePullRequest.id())
        && !f.author_is_acting
        && f.wait_approve_blocker.is_none();
    let mut reasons = Vec::new();
    if merge && f.ci == Ci::Pending {
        reasons.push(WaitReason::Ci);
    }
    if (approve || merge) && f.draft {
        reasons.push(WaitReason::Draft);
    }
    // Do not wait for a vote this assignment can supply itself.
    if merge && f.required_reviews_unmet && (f.already_approved || !approve) {
        reasons.push(WaitReason::RequiredReviews);
    }
    reasons
}

fn approve_denial(input: &GateInput) -> Option<String> {
    let f = &input.facts;
    Some(if !(input.grants.approve || input.grants.merge) {
        "No Approve grant.".into()
    } else if !f.open {
        "PR is closed or merged.".into()
    } else if f.draft {
        "PR is draft. Do not approve.".into()
    } else if f.conflicts {
        // PR-state reasons first: they tell the agent what to comment.
        "Merge conflicts. Do not approve. Say so with resolution advice.".into()
    } else if f.ci == Ci::Red {
        "CI is red. Do not approve. Comment the fix if you know it.".into()
    } else if f.author_is_acting {
        // GitHub refuses self-approval: "Can not approve your own pull request".
        "Acting account authored this PR. GitHub refuses self-approval.".into()
    } else {
        return f.approve_blocker.clone();
    })
}

fn merge_denial(input: &GateInput) -> Option<String> {
    let f = &input.facts;
    if !input.grants.merge {
        return Some("No Approve & Merge grant.".into());
    }
    if !f.author_is_acting {
        if let Some(reason) = approve_denial(input) {
            return Some(reason);
        }
    }
    Some(if !f.open {
        "PR is closed or merged.".into()
    } else if f.draft {
        "PR is draft. Do not merge.".into()
    } else if f.conflicts {
        "Merge conflicts. Do not merge.".into()
    } else if f.ci == Ci::Red {
        "CI is red. Do not merge.".into()
    } else if f.author_is_acting && !input.grants.include_self {
        // Approval cannot be posted on an own PR; merge alone needs Include self.
        "Acting account authored this PR. Include self is off.".into()
    } else if f.ci == Ci::Pending {
        "CI is pending. Cannot merge yet.".into()
    } else if f.ci == Ci::Unknown {
        "CI not verified green. Cannot merge.".into()
    } else if f.required_reviews_unmet {
        "Required reviews unmet. Cannot merge.".into()
    } else if f.changes_requested {
        "Changes requested by a reviewer. Cannot merge.".into()
    } else if f.unresolved_threads {
        "Unresolved threads remain. Cannot merge.".into()
    } else {
        return f.merge_blocker.clone();
    })
}

/// The sign-off's action: the assignment's strongest granted authority. Grants alone
/// decide it; the tool list only gates the legacy explicit approve exits.
pub fn lgtm_action(input: &GateInput) -> LgtmAction {
    if input.grants.merge {
        LgtmAction::Merge
    } else if input.grants.approve {
        LgtmAction::Approve
    } else {
        LgtmAction::Ready
    }
}

/// Why a sign-off is refused on these facts: the facts after its own resolutions.
pub fn lgtm_denial(input: &GateInput) -> Option<String> {
    let f = &input.facts;
    Some(if !input.primary {
        "lgtm is primary-only.".into()
    } else if !f.open {
        "PR is closed or merged.".into()
    } else if f.draft {
        "PR is draft. Do not sign off.".into()
    } else if f.conflicts {
        "Merge conflicts. Do not sign off.".into()
    } else if f.ci == Ci::Red {
        "CI is red. Do not sign off.".into()
    } else if f.ci == Ci::Pending {
        "CI is pending. Do not sign off until it is green.".into()
    } else if f.ci != Ci::Green {
        "CI not verified green. Do not sign off.".into()
    } else if f.changes_requested {
        "Changes requested by a reviewer. Do not sign off.".into()
    } else if f.unresolved_threads {
        "Unresolved threads not rooted in your own findings remain. They need the author's fix or reply; lgtm cannot clear them.".into()
    } else {
        return match lgtm_action(input) {
            LgtmAction::Ready => None,
            LgtmAction::Approve => approve_denial(input),
            LgtmAction::Merge => merge_denial(input),
        };
    })
}

/// Approving a head we already approved is a successful no-op.
pub fn approve_is_idempotent(input: &GateInput) -> bool {
    input.facts.already_approved
}

/// Exit-time check: the call must still be offered and its parts granted.
pub fn check_call(input: &GateInput, call: &ExitCall) -> Result<(), String> {
    match offered(input).into_iter().find(|(e, _)| *e == call.exit()) {
        None => return Err("Exit not available. Pick another.".into()),
        Some((_, Offer::Denied(reason))) => return Err(reason),
        Some((_, Offer::Offered)) => {}
    }
    if let ExitCall::Wait { reason } = call {
        if !wait_reasons(input).contains(reason) {
            return Err(
                "That external prerequisite is not currently observed for an enabled action."
                    .into(),
            );
        }
    }
    if let ExitCall::LeaveComments {
        reply_comments,
        resolve_own_threads,
        ..
    } = call
    {
        if !reply_comments.is_empty() && !input.grants.reply {
            return Err("No Reply Comment grant. Drop reply_comments.".into());
        }
        if !resolve_own_threads.is_empty() && !input.grants.reply {
            return Err("No grant to resolve threads. Drop resolve_own_threads.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
