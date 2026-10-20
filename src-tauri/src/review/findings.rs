//! Existing provider threads are finding identities. The model establishes truth;
//! the host enforces identity, revision, evidence and independent-verification contracts.

use super::duck::{DuckCall, DuckOutcome, EvidenceRef, TargetKind, VerdictKind};
use crate::{
    exits::{Exit, ExitCall},
    github::threads::{Ownership, Provenance},
    monitoring::QueueJob,
    packet::ThreadFact,
    storage::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const CORRECTION: &str = "Correction: my earlier resolution was mistaken.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    thread_id: String,
    root_id: Option<String>,
    body: String,
    text: String,
    receipt: Option<String>,
    pending: bool,
}

impl Correction {
    fn confirmed_in(&self, thread: &ThreadFact) -> bool {
        !self.pending
            && thread.comments.first().map(|c| &c.id) == self.root_id.as_ref()
            && self.receipt.as_ref().is_some_and(|receipt| {
                thread
                    .comments
                    .iter()
                    .any(|comment| comment.id == *receipt && comment.body == self.body)
            })
    }
}

/// A compact view of existing write intents, not another persisted ledger.
pub(crate) fn corrections(
    writes: &[crate::exit_writes::ExitWrite],
    job: &QueueJob,
    assignment: &str,
) -> Vec<Correction> {
    use crate::exit_writes::{StepKind, StepState, SIGNATURE};
    let mut ordered: Vec<_> = writes
        .iter()
        .enumerate()
        .filter(|(_, write)| {
            crate::feedback::same_pr(&write.job, job) && write.assignment_id == assignment
        })
        .collect();
    ordered.sort_by_key(|(index, write)| (write.operation.initial_attempt_at, *index));
    let mut latest: BTreeMap<String, Vec<Correction>> = BTreeMap::new();
    for (_, write) in ordered {
        for (index, step) in write.steps.iter().enumerate() {
            if step.state == StepState::Planned {
                continue;
            }
            if step.kind == StepKind::Resolve {
                if step.state == StepState::Done && step.receipt.as_deref() == Some("already") {
                    continue;
                }
                // A later resolution is a new episode, even at the same head/base.
                latest.remove(&step.thread_id);
                if step.state == StepState::Done {
                    continue;
                }
                latest.insert(
                    step.thread_id.clone(),
                    vec![Correction {
                        thread_id: step.thread_id.clone(),
                        root_id: step.root_id.clone(),
                        body: String::new(),
                        text: String::new(),
                        receipt: None,
                        pending: true,
                    }],
                );
                continue;
            }
            if !step.body.starts_with(CORRECTION) {
                continue;
            }
            let Some(revision) = &step.revision else {
                continue;
            };
            let suffix = format!(
                "\n\n{}\n<!-- pr-sniper:revision:{revision} -->\n\n{SIGNATURE}",
                write.reply_marker(index)
            );
            let Some(text) = step.body.strip_suffix(&suffix) else {
                continue;
            };
            latest
                .entry(step.thread_id.clone())
                .or_default()
                .push(Correction {
                    thread_id: step.thread_id.clone(),
                    root_id: step.root_id.clone(),
                    body: step.body.clone(),
                    text: text.to_string(),
                    receipt: step.receipt.clone(),
                    pending: step.state != StepState::Done,
                });
        }
    }
    latest.into_values().flatten().collect()
}

fn correction_text(body: &str) -> String {
    if body.starts_with(CORRECTION) {
        body.trim_end().to_string()
    } else {
        format!("{CORRECTION}\n\n{}", body.trim_end())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    StillApplies,
    Fixed,
    Withdrawn,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub thread_id: String,
    pub disposition: Disposition,
    pub head_sha: String,
    pub base_sha: String,
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duck_call: Option<u32>,
}

impl Assessment {
    pub(crate) fn duck_confirms(&self, state_id: &str, ducks: &[DuckCall]) -> bool {
        ducks.iter().any(|d| {
            Some(d.ordinal) == self.duck_call
                && d.state_id == state_id
                && d.input.kind == TargetKind::Thread
                && d.input.thread_id.as_deref() == Some(&self.thread_id)
                && d.input.verify_fixed
                && d.input.claim == self.rationale
                && d.input.evidence == self.evidence
                && matches!(&d.outcome, DuckOutcome::Verdict { verdict }
                    if verdict.verdict == VerdictKind::Confirmed && !verdict.evidence.is_empty())
        })
    }
}

pub(crate) fn action_clearance(assessments: &[Assessment]) -> Result<(), String> {
    if assessments.iter().any(|a| {
        matches!(
            a.disposition,
            Disposition::StillApplies | Disposition::Unverifiable
        )
    }) {
        Err("Applicable or unverifiable findings prevent approval and merge; use ready_for_human or leave_comments.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn proofs(
    store: &Store,
    job: &QueueJob,
    assignment: &str,
) -> Result<Vec<Ownership>, String> {
    let mut proofs: Vec<_> = store
        .load_publications()?
        .iter()
        .filter(|p| crate::feedback::same_pr(&p.review.job, job))
        .filter_map(|p| p.ownership().ok())
        .collect();
    proofs.extend(
        crate::retention::load(store)?
            .receipts
            .into_iter()
            .flat_map(|r| r.owned)
            .filter(|o| o.matches(job))
            .map(|o| o.proof),
    );
    proofs.retain(|p| p.assignment_id == assignment && p.account_id == job.account_id);
    Ok(proofs)
}

pub(crate) fn stamp_ownership(
    threads: &mut [ThreadFact],
    proofs: &[Ownership],
    assignment: &str,
    account: &str,
) {
    // Identity survives edits; resolution still needs the full ownership proof.
    for thread in threads {
        thread.mine = thread.comments.first().is_some_and(|root| {
            proofs.iter().any(|proof| {
                proof.assignment_id == assignment
                    && proof.account_id == account
                    && proof.root_ids.contains(&root.id)
            })
        });
    }
}

pub fn assessment_schema() -> Value {
    json!({"type":"array","items":{"type":"object","additionalProperties":false,
        "properties":{
            "thread_id":{"type":"string"},
            "disposition":{"type":"string","enum":["still_applies","fixed","withdrawn","unverifiable"]},
            "head_sha":{"type":"string"},"base_sha":{"type":"string"},
            "rationale":{"type":"string","maxLength":4096},
            "evidence":{"type":"array","maxItems":10,"items":super::duck::reference_schema()},
            "duck_call":{"type":"integer","minimum":1}},
        "required":["thread_id","disposition","head_sha","base_sha","rationale","evidence"]}})
}

/// Remove host metadata before the legacy exit parser. Existing identities become
/// replies before diff mapping or publication can ever create another root.
pub(crate) fn extract(args: &Value) -> Result<(Value, Vec<Assessment>), String> {
    let mut args = args.clone();
    let object = args
        .as_object_mut()
        .ok_or("Exit arguments must be an object.")?;
    let assessments = object
        .remove("finding_dispositions")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "Malformed finding_dispositions.")?
        .unwrap_or_default();
    if let Some(comments) = object.get_mut("new_comments").and_then(Value::as_array_mut) {
        let mut replies = Vec::new();
        let mut fresh = Vec::new();
        for mut comment in comments.drain(..) {
            match comment.as_object_mut().and_then(|c| c.remove("finding_id")) {
                Some(Value::String(id)) if !id.trim().is_empty() => {
                    let body = comment
                        .get("body")
                        .cloned()
                        .ok_or("Finding update needs body.")?;
                    // Validate the full original comment too; do not silently accept unknown fields.
                    serde_json::from_value::<crate::exits::NewComment>(comment)
                        .map_err(|_| "Malformed finding update.")?;
                    replies.push(json!({"thread_id":id,"body":body}));
                }
                Some(Value::Null) | None => fresh.push(comment),
                _ => return Err("finding_id must be null (new) or an existing thread id.".into()),
            }
        }
        *comments = fresh;
        if !replies.is_empty() {
            let existing = object.entry("reply_comments").or_insert_with(|| json!([]));
            existing
                .as_array_mut()
                .ok_or("reply_comments must be an array.")?
                .extend(replies);
        }
    }
    Ok((args, assessments))
}

pub(crate) struct Contract {
    pub head: String,
    pub base: String,
    pub state_id: String,
    pub threads: Vec<ThreadFact>,
    pub duck_required: bool,
    pub corrections: Vec<Correction>,
}

impl Contract {
    pub fn prepare(
        &self,
        call: &mut ExitCall,
        assessments: &[Assessment],
        ducks: &[DuckCall],
        served: impl Fn(&EvidenceRef) -> bool,
    ) -> Result<(), String> {
        self.validate(call, assessments, ducks, served)?;
        if let ExitCall::LeaveComments { reply_comments, .. } = call {
            for a in assessments.iter().filter(|a| {
                a.disposition == Disposition::StillApplies
                    && self
                        .threads
                        .iter()
                        .any(|t| t.id == a.thread_id && t.resolved)
            }) {
                let Some(reply) = reply_comments
                    .iter_mut()
                    .find(|r| r.thread_id == a.thread_id && r.body.contains(&a.rationale))
                else {
                    continue;
                };
                reply.body = correction_text(&reply.body);
                if reply.body.len() > crate::exits::MAX_COMMENT_BYTES {
                    return Err("Correction reply exceeds 16384 bytes; shorten it.".into());
                }
            }
        }
        Ok(())
    }

    pub fn validate(
        &self,
        call: &ExitCall,
        assessments: &[Assessment],
        ducks: &[DuckCall],
        served: impl Fn(&EvidenceRef) -> bool,
    ) -> Result<(), String> {
        if call.exit() == Exit::Wait
            && ducks.iter().any(|d| match &d.outcome {
                DuckOutcome::Failure { .. } => true,
                DuckOutcome::Verdict { verdict } => verdict.verdict == VerdictKind::Unverifiable,
            })
        {
            return Err("Unsettled Rubber Duck evidence cannot become an external wait.".into());
        }
        if matches!(
            call.exit(),
            Exit::PanicExit | Exit::SubmitReport | Exit::SubmitVerdict
        ) {
            return if assessments.is_empty() {
                Ok(())
            } else {
                Err("Finding dispositions are primary-only.".into())
            };
        }
        if let ExitCall::LeaveComments { reply_comments, .. } = call {
            for reply in reply_comments {
                let previous: Vec<_> = self
                    .corrections
                    .iter()
                    .filter(|c| c.thread_id == reply.thread_id)
                    .collect();
                if previous.is_empty() {
                    continue;
                }
                let thread = self.threads.iter().find(|t| t.id == reply.thread_id);
                if previous
                    .iter()
                    .any(|c| !thread.is_some_and(|t| c.confirmed_in(t)))
                {
                    return Err("Earlier correction outcome is unresolved or its receipt is unreadable; no replacement reply will be posted. Omit that reply or choose ready_for_human.".into());
                }
                if previous
                    .iter()
                    .any(|c| correction_text(&reply.body) == c.text)
                {
                    return Err("That correction is already confirmed on the original thread. Omit the duplicate reply or choose ready_for_human; revise only for new information.".into());
                }
            }
        }
        let own: BTreeSet<_> = self
            .threads
            .iter()
            .filter(|t| t.mine)
            .map(|t| t.id.as_str())
            .collect();
        let mut seen = BTreeSet::new();
        for a in assessments {
            if !own.contains(a.thread_id.as_str()) || !seen.insert(a.thread_id.as_str()) {
                return Err("Classify each own finding once, using its exact thread id.".into());
            }
            if a.head_sha != self.head || a.base_sha != self.base {
                return Err("Finding evidence must name this packet's exact head and base.".into());
            }
            if a.rationale.trim().is_empty() || a.rationale.len() > 4096 || a.evidence.len() > 10 {
                return Err(
                    "Finding rationale is required (<=4096 bytes), evidence <=10 refs.".into(),
                );
            }
            super::duck::evidence(&a.evidence)?;
            if a.duck_call == Some(0) {
                return Err("duck_call must be a positive call ordinal.".into());
            }
            if a.disposition == Disposition::Fixed {
                if a.evidence.is_empty() || !a.evidence.iter().all(&served) {
                    return Err("Fixed needs current-head code evidence actually read in this attempt; outdated alone is not evidence.".into());
                }
                if self.duck_required && !a.duck_confirms(&self.state_id, ducks) {
                    return Err("Fixed needs a confirmed rubber_duck verify_fixed call for this thread, rationale and evidence on this state.".into());
                }
            }
            let thread = self.threads.iter().find(|t| t.id == a.thread_id).unwrap();
            if thread.resolved
                && a.disposition == Disposition::StillApplies
                && !self.corrections.iter().any(|c| c.thread_id == a.thread_id)
            {
                let corrected = matches!(call, ExitCall::LeaveComments { reply_comments, .. }
                    if reply_comments.iter().any(|r| r.thread_id == a.thread_id && r.body.contains(&a.rationale)));
                if !corrected {
                    return Err("Earlier resolution was mistaken: reply to that existing thread with the correction rationale, never post a new root.".into());
                }
            }
        }
        if seen != own {
            return Err("Classify every own finding, including resolved/outdated threads: still_applies, fixed, withdrawn or unverifiable.".into());
        }
        if let ExitCall::LeaveComments {
            resolve_own_threads,
            ..
        } = call
        {
            if resolve_own_threads.iter().any(|id| {
                !assessments
                    .iter()
                    .any(|a| a.thread_id == *id && a.disposition == Disposition::Fixed)
            }) {
                return Err("Only own findings classified fixed with current-code evidence may be resolved.".into());
            }
        }
        if matches!(
            call.exit(),
            Exit::ApprovePullRequest | Exit::ApproveAndMerge | Exit::Wait
        ) {
            action_clearance(assessments)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
