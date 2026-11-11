//! The typed outcome of reviewed work: the exit a run completed, confirmed by
//! the provider effect or publication it caused. Never read from text.

use crate::{
    actions::{packet::Settlement, EffectState, Ledger},
    exit_writes::{ExitWrite, Phase as WritePhase},
    exits::{ExitOutcome, LgtmAction},
    follow_up::{FollowUp, Phase as FollowPhase},
    github::actions::Action,
    monitoring::OperationState,
    publication::{Phase as PublicationPhase, Publication},
    review::ReviewRun,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Confirmed merge (`approve_and_merge`, or `lgtm` with merge).
    ApprovedAndMerged,
    /// Confirmed Approve vote (`approve_pull_request`, or `lgtm` with approve).
    Approved,
    /// `leave_comments` (or a reply) confirmed published.
    Commented,
    /// `ready_for_human`, or a sign-off without approve authority.
    ReadyForHuman,
    /// `wait` on CI, draft or required reviews.
    Waiting,
    /// A teammate's `submit_report`, or findings kept local-only.
    Report,
    /// The exit completed; its provider effect is not confirmed yet.
    Pending,
    /// The exit's provider effect or publication was refused or failed.
    Failed,
}

impl Outcome {
    pub const ALL: [Outcome; 8] = [
        Outcome::ApprovedAndMerged,
        Outcome::Approved,
        Outcome::Commented,
        Outcome::ReadyForHuman,
        Outcome::Waiting,
        Outcome::Report,
        Outcome::Pending,
        Outcome::Failed,
    ];
}

/// Saved records an outcome is confirmed against.
pub struct Evidence<'a> {
    pub publications: &'a [Publication],
    pub exit_writes: &'a [ExitWrite],
    pub actions: &'a Ledger,
}

impl Evidence<'_> {
    /// Outcome of a completed review run. `local_only`: comment publication is
    /// off for this result. `None` until the run completes (a panic never does).
    pub fn run(&self, run: &ReviewRun, local_only: bool) -> Option<Outcome> {
        if run.operation.state != OperationState::Completed {
            return None;
        }
        let output = &run.result.as_ref()?.output;
        Some(match &output.exit {
            Some(ExitOutcome::Wait { .. }) => Outcome::Waiting,
            Some(ExitOutcome::ReadyForHuman { .. }) => Outcome::ReadyForHuman,
            Some(ExitOutcome::SubmitReport) => Outcome::Report,
            Some(ExitOutcome::Approve { .. }) | Some(ExitOutcome::ApproveAndMerge { .. }) => {
                self.approval(run)
            }
            Some(ExitOutcome::Lgtm { action, .. }) => match action {
                LgtmAction::Ready => Outcome::ReadyForHuman,
                LgtmAction::Approve | LgtmAction::Merge => self.approval(run),
            },
            Some(ExitOutcome::Comments { .. }) => self.comments(run, local_only),
            // Legacy result: history only, never an approval.
            None => match self.publication(run).map(|p| &p.phase) {
                Some(PublicationPhase::Published | PublicationPhase::StaleAfterPublication) => {
                    Outcome::Commented
                }
                _ if local_only => Outcome::Report,
                _ => Outcome::ReadyForHuman,
            },
        })
    }

    fn publication(&self, run: &ReviewRun) -> Option<&Publication> {
        self.publications
            .iter()
            .rev()
            .find(|p| p.review.operation.id == run.operation.id)
    }

    fn comments(&self, run: &ReviewRun, local_only: bool) -> Outcome {
        let publication = self.publication(run);
        let write = self
            .exit_writes
            .iter()
            .rev()
            .find(|w| w.review_operation_id == run.operation.id);
        let published = publication.is_some_and(|p| {
            matches!(
                p.phase,
                PublicationPhase::Published | PublicationPhase::StaleAfterPublication
            )
        });
        let written = write.is_some_and(|w| w.published || w.phase == WritePhase::Completed);
        if published || written {
            return Outcome::Commented;
        }
        if publication.is_some_and(|p| p.phase == PublicationPhase::Discarded) {
            return Outcome::Report;
        }
        // Parked like the Queue's failed publication: no retry is scheduled.
        if publication.is_some_and(crate::queue::publication_failed) {
            return Outcome::Failed;
        }
        if publication.is_some_and(|p| {
            matches!(
                p.phase,
                PublicationPhase::Stale | PublicationPhase::Stopped | PublicationPhase::Unresolved
            )
        }) || write.is_some_and(|w| matches!(w.phase, WritePhase::Failed | WritePhase::Tossed))
        {
            return Outcome::Failed;
        }
        if publication.is_none() && write.is_none() && local_only {
            return Outcome::Report;
        }
        Outcome::Pending
    }

    fn approval(&self, run: &ReviewRun) -> Outcome {
        let finals: Vec<_> = self
            .actions
            .finals
            .iter()
            .filter(|f| {
                f.packet
                    .as_ref()
                    .is_some_and(|p| p.run_id == run.operation.id)
            })
            .collect();
        let decided = self.effects(|e| finals.iter().any(|f| f.id == e.final_id));
        if let Some(outcome) = decided.filter(|o| *o != Outcome::Pending) {
            return outcome;
        }
        // An `lgtm` resolving its own threads first.
        let write = self
            .exit_writes
            .iter()
            .rev()
            .find(|w| w.review_operation_id == run.operation.id);
        if write.is_some_and(|w| matches!(w.phase, WritePhase::Failed | WritePhase::Tossed)) {
            return Outcome::Failed;
        }
        let Some(packet) = finals.last().and_then(|f| f.packet.as_ref()) else {
            return Outcome::Pending;
        };
        if !packet.settled {
            return Outcome::Pending;
        }
        match packet.settlement {
            Some(Settlement::Done) => Outcome::Approved,
            Some(Settlement::NoAuthority) => Outcome::ReadyForHuman,
            Some(
                Settlement::Moved
                | Settlement::SignOffStopped
                | Settlement::Revoked
                | Settlement::Refused
                | Settlement::Failed,
            ) => Outcome::Failed,
            // Settled by an older build: only its permissions are on record.
            None if !finals
                .last()
                .is_some_and(|f| f.basis.permissions.approve || f.basis.permissions.merge) =>
            {
                Outcome::ReadyForHuman
            }
            None if write.is_some_and(|w| w.consumed) => Outcome::Approved,
            None => Outcome::Failed,
        }
    }

    /// Outcome of a legacy primary final review record: history only, so with
    /// no confirmed effect it went to a human.
    pub fn final_review(&self, id: &str) -> Outcome {
        self.effects(|e| e.final_id == id)
            .filter(|o| *o != Outcome::Pending)
            .unwrap_or(Outcome::ReadyForHuman)
    }

    /// A PR row: only a confirmed Approve or merge PR Sniper made.
    pub fn item(&self, item_id: &str) -> Option<Outcome> {
        self.effects(|e| e.item_id == item_id)
            .filter(|o| matches!(o, Outcome::Approved | Outcome::ApprovedAndMerged))
    }

    fn effects(&self, select: impl Fn(&crate::actions::Effect) -> bool) -> Option<Outcome> {
        let effects: Vec<_> = self.actions.effects.iter().filter(|e| select(e)).collect();
        let confirmed = |action: Action| {
            effects
                .iter()
                .any(|e| e.action == action && e.state == EffectState::Confirmed)
        };
        if confirmed(Action::Merge) {
            Some(Outcome::ApprovedAndMerged)
        } else if confirmed(Action::Approve) {
            Some(Outcome::Approved)
        } else if effects.iter().any(|e| {
            e.cancelled
                || matches!(
                    e.state,
                    EffectState::Rejected | EffectState::Stale | EffectState::Dequeued
                )
        }) {
            Some(Outcome::Failed)
        } else if effects
            .iter()
            .any(|e| e.state == EffectState::ExternalMerge)
        {
            // A person merged first: the PR went to a human.
            Some(Outcome::ReadyForHuman)
        } else if effects.is_empty() {
            None
        } else {
            Some(Outcome::Pending)
        }
    }
}

/// Outcome of a completed reply or mention conversation.
pub fn follow_up(follow: &FollowUp) -> Option<Outcome> {
    follow.result.as_ref()?;
    Some(match follow.phase {
        FollowPhase::Published | FollowPhase::StaleAfterPublication => Outcome::Commented,
        FollowPhase::Quiet => Outcome::Report,
        FollowPhase::HumanInputRequired => Outcome::ReadyForHuman,
        FollowPhase::Stopped | FollowPhase::Unresolved => Outcome::Failed,
        FollowPhase::WaitingStart
        | FollowPhase::Analyzing
        | FollowPhase::WaitingPublication
        | FollowPhase::Publishing => Outcome::Pending,
    })
}

/// Each Reviewed row's outcome, keyed `<kind>:<id>` like the panel's rows.
pub(crate) fn project(
    store: &crate::storage::Store,
    snapshot: &mut crate::queue::Snapshot,
) -> Result<(), String> {
    let publications = store.load_publications()?;
    let actions = store.load_actions()?;
    let evidence = Evidence {
        publications: &publications,
        exit_writes: &snapshot.exit_writes,
        actions: &actions,
    };
    let mut outcomes = std::collections::BTreeMap::new();
    for candidate in &snapshot.reviews {
        let Some(run) = &candidate.run else { continue };
        let local_only = snapshot
            .publications
            .iter()
            .find(|p| p.review_operation_id == run.operation.id)
            .is_some_and(|p| p.local_only);
        if let Some(outcome) = evidence.run(run, local_only) {
            outcomes.insert(format!("normal:{}", candidate.key), outcome);
        }
    }
    for candidate in &snapshot.follow_ups {
        let kind = match candidate.run.kind() {
            crate::capacity::Kind::Mention => "mention",
            _ => "reply",
        };
        if let Some(outcome) = follow_up(&candidate.run) {
            outcomes.insert(format!("{kind}:{}", candidate.run.id), outcome);
        }
    }
    for item in &snapshot.items {
        if let Some(final_review) = item
            .action_status
            .as_ref()
            .and_then(|s| s.final_review.as_ref())
        {
            outcomes.insert(
                format!("primary_final:{}", final_review.id),
                evidence.final_review(&final_review.id),
            );
        }
        if let Some(outcome) = evidence.item(&item.id) {
            outcomes.insert(format!("item:{}", item.id), outcome);
        }
    }
    snapshot.outcomes = outcomes;
    Ok(())
}
