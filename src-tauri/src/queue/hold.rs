//! Why a signed-off PR is not ready to merge right now. The heartbeat keeps
//! reading every tracked open PR; a Ready item whose latest provider facts
//! (same head) show conflicts, a required update, red CI, requested changes or
//! a draft waits for its author instead. Projection only: no review, comment,
//! notification or write. The next read that clears the fact returns it to
//! Ready; a new head is a new Iteration.
use super::{Progress, Snapshot, State};
use crate::{provider::NormalizedSnapshot, retention::Binding};
use serde::Serialize;

pub const SUMMARY: &str = "Signed off, but the PR is not ready to merge. It returns to Ready once it is; this is not a finding.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    Draft,
    Conflicts,
    ChangesRequested,
    CiFailed,
    /// The base requires an up-to-date branch and the head is behind it.
    BehindBase,
}

/// The first fact that keeps an open PR from merging, if any.
pub fn of(snapshot: &NormalizedSnapshot) -> Option<Hold> {
    if snapshot.lifecycle == "draft" {
        Some(Hold::Draft)
    } else if snapshot.lifecycle != "open" {
        None
    } else if snapshot.mergeable == "conflicting" {
        Some(Hold::Conflicts)
    } else if snapshot.review_decision.as_deref() == Some("CHANGES_REQUESTED") {
        Some(Hold::ChangesRequested)
    } else if snapshot.ci_failed() {
        Some(Hold::CiFailed)
    } else if snapshot.mergeable == "behind" {
        Some(Hold::BehindBase)
    } else {
        None
    }
}

/// Ready items, and approvals whose merge waits on the branch, whose latest
/// read of the same head is not mergeable wait for the author. An approve or
/// merge in flight keeps its progress until it settles; a settled approval
/// with nothing pending stays Approved.
pub(super) fn project(snapshot: &mut Snapshot) {
    for item in &mut snapshot.items {
        let merge_pending = matches!(
            item.progress,
            Some(Progress::MergePendingBehindBase | Progress::MergePendingConflicts)
        );
        let eligible = match item.state {
            State::MachineSignedOff => {
                !matches!(item.progress, Some(Progress::Approving | Progress::Merging))
            }
            State::Approved => merge_pending,
            _ => false,
        };
        if !eligible {
            continue;
        }
        let Some(hold) = snapshot
            .pr_states
            .get(&Binding::job(&item.job))
            .and_then(|record| record.snapshot.as_ref())
            .filter(|read| read.head == item.job.head_sha)
            .and_then(of)
        else {
            continue;
        };
        item.state = State::WaitingForAuthor;
        item.summary = SUMMARY;
        item.progress = None;
        item.hold = Some(hold);
    }
}
