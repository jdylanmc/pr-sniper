use super::*;
use crate::{
    feedback::{Context, Record},
    follow_up::{ConversationContext, FollowUp},
    github::{
        metadata::{Lifecycle, PullRequest},
        threads::Thread,
    },
    monitoring::PollTicket,
    review::Selection,
};

pub(crate) struct Observed {
    pub origin: OwnedReceipt,
    pub head: String,
    pub threads: Vec<Thread>,
}

pub(crate) fn scan_origins(
    store: &Store,
    ticket: &PollTicket,
    pulls: &[PullRequest],
) -> Result<Vec<OwnedReceipt>, String> {
    Ok(load(store)?
        .receipts
        .into_iter()
        .flat_map(|r| r.owned)
        .filter(|r| {
            (r.proof.configuration_id == ticket.repository_id
                || r.proof.configuration_id.is_empty())
                && r.proof.account_id == ticket.provider_account_id
                && r.proof.repository_id == ticket.provider_repository_id
                && pulls
                    .iter()
                    .any(|p| p.id == r.proof.pull_request_id && p.state == Lifecycle::Open)
        })
        .collect())
}

pub(crate) fn known_key(store: &Store, key: &str) -> Result<bool, String> {
    Ok(load(store)?
        .receipts
        .iter()
        .any(|r| r.follow_up_keys.contains(key)))
}

pub(crate) fn contexts(store: &Store, job: &QueueJob) -> Result<Vec<Context>, String> {
    let ledger = store.load_feedback()?;
    let mut contexts = Vec::new();
    for origin in load(store)?
        .receipts
        .into_iter()
        .flat_map(|r| r.owned)
        .filter(|o| o.matches(job))
    {
        for root in &origin.proof.root_ids {
            let id = crate::feedback::root_key(job, root);
            let record = ledger
                .records
                .iter()
                .find(|r| r.context.id == id)
                .ok_or("Waiting for verified retained-root observations after reopening.")?;
            if record.observed_head != job.head_sha || record.context.unavailable.is_some() {
                return Err("Retained owned feedback is unavailable at this revision.".into());
            }
            contexts.push(record.context.clone());
        }
    }
    Ok(contexts)
}

pub(crate) fn admit_observations(
    store: &Store,
    observed: Vec<Observed>,
    now: i64,
) -> Result<(), String> {
    if observed.is_empty() {
        return Ok(());
    }
    let jobs = store.load_queue()?;
    let settings = store.load_settings()?;
    let mut receipts = load(store)?;
    let mut feedback = store.load_feedback()?;
    let mut eligible = Vec::new();
    for observation in observed {
        let origin = receipts
            .receipts
            .iter_mut()
            .flat_map(|r| r.owned.iter_mut())
            .find(|o| o.proof == observation.origin.proof)
            .ok_or("Retained feedback provenance changed during the scan.")?;
        if observation
            .threads
            .iter()
            .any(|t| !t.owned_by(&origin.proof))
        {
            return Err("Retained root provenance changed; no observation accepted.".into());
        }
        let Some(job) = jobs
            .iter()
            .filter(|j| {
                origin.matches(j)
                    && j.head_sha == observation.head
                    && !matches!(
                        j.waiting.as_str(),
                        crate::monitoring::WAITING_CLOSED
                            | crate::monitoring::WAITING_MERGED
                            | crate::monitoring::WAITING_SUPERSEDED
                    )
            })
            .max_by_key(|j| j.work.as_ref().map(|w| w.iteration).unwrap_or(0))
        else {
            continue;
        };
        for root_id in &origin.proof.root_ids {
            let thread = observation
                .threads
                .iter()
                .find(|t| t.root().is_ok_and(|r| &r.id == root_id));
            if thread.is_some_and(|t| t.resolved) {
                origin.closed_roots.insert(root_id.clone());
            }
            let closed = origin.closed_roots.contains(root_id);
            let id = crate::feedback::root_key(job, root_id);
            let old = feedback.records.iter().find(|r| r.context.id == id);
            let root = thread.and_then(|t| t.root().ok());
            let hint = root.and_then(|r| origin.hints.get(&hash(&r.body)));
            let context = Context {
                id: id.clone(),
                publication_id: origin.proof.publication_id.clone(),
                owner_agent_id: origin.proof.agent_id.clone(),
                owner_assignment_id: origin.proof.assignment_id.clone(),
                original_head: origin.proof.head_sha.clone(),
                root_id: root_id.clone(),
                path: hint
                    .map(|h| h.0.clone())
                    .or_else(|| old.map(|r| r.context.path.clone()))
                    .unwrap_or_default(),
                title: hint
                    .map(|h| h.1.clone())
                    .or_else(|| old.map(|r| r.context.title.clone()))
                    .unwrap_or_default(),
                body: root.map(|r| r.body.clone()).unwrap_or_default(),
                thread: thread.cloned().map(|mut thread| {
                    thread.comments = thread
                        .comments
                        .into_iter()
                        .enumerate()
                        .filter(|(index, c)| {
                            *index == 0
                                || !(c.author_id.as_deref() == Some(&origin.proof.account_id)
                                    && c.body.contains("<!-- pr-sniper:reply:"))
                        })
                        .map(|(_, c)| c)
                        .collect();
                    thread
                }),
                closed,
                unavailable: (thread.is_none() && !closed)
                    .then(|| "Retained published root is missing; no closure inferred.".into()),
            };
            let record = Record {
                context,
                job: job.clone(),
                observed_head: observation.head.clone(),
            };
            if let Some(old) = feedback.records.iter_mut().find(|r| r.context.id == id) {
                *old = record;
            } else {
                feedback.records.push(record);
            }
        }
        eligible.push((origin.clone(), observation.threads));
    }
    save(store, &receipts)?;
    store.save_feedback(&feedback)?;
    let mut runs = store.load_follow_ups()?;
    for (origin, threads) in eligible {
        let Some(job) = jobs.iter().rev().find(|j| {
            origin.matches(j)
                && j.assignment_id.as_deref() == Some(&origin.proof.assignment_id)
                && j.work
                    .as_ref()
                    .is_some_and(|w| w.agent_id == origin.proof.agent_id)
                && crate::monitoring::review_policy(&settings, j, None).is_ok()
        }) else {
            continue;
        };
        let selection = Selection::resolve(&settings, job, &origin.proof.assignment_id)?;
        for thread in threads {
            if thread.resolved
                || !thread.can_reply
                || thread
                    .root()
                    .is_ok_and(|r| origin.closed_roots.contains(&r.id))
                || thread.latest_external(&job.account_id).is_none()
            {
                continue;
            }
            let context = ConversationContext {
                assignment_id: origin.proof.assignment_id.clone(),
                job: job.clone(),
                selection: selection.clone(),
                trust_confirmed: false,
                feedback: crate::feedback::contexts(store, job, &selection.agent.id)
                    .map_err(|e| e.message)?,
                feedback_checked: true,
            };
            let run = FollowUp::retained(&origin, thread, context)?;
            if known_key(store, &run.key)? {
                continue;
            }
            if crate::follow_up::admit_stored(store, &mut runs, run)? {
                let run = runs
                    .last_mut()
                    .ok_or("Retained follow-up admission disappeared.")?;
                run.enqueue_order = Some(store.allocate_enqueue_order()?);
                run.enqueued_at = Some(now);
            }
        }
    }
    store.save_follow_ups(&runs)
}
