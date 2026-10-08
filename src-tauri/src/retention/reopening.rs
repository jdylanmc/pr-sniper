use super::*;
use crate::{
    feedback::{Context, Record},
    github::{
        metadata::{Lifecycle, PullRequest},
        threads::Thread,
    },
    monitoring::PollTicket,
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

pub(crate) fn contexts_checked(
    store: &Store,
    job: &QueueJob,
) -> Result<Vec<Context>, crate::publication::GateError> {
    use crate::publication::GateError;
    let ledger = store.load_feedback().map_err(GateError::Storage)?;
    let mut contexts = Vec::new();
    for origin in load(store)
        .map_err(GateError::Storage)?
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
                .ok_or_else(|| {
                    GateError::Policy(
                        "Waiting for verified retained-root observations after reopening.".into(),
                    )
                })?;
            if record.observed_head != job.head_sha || record.context.unavailable.is_some() {
                return Err(GateError::Policy(
                    "Retained owned feedback is unavailable at this revision.".into(),
                ));
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
    let queue = store.load_queue_state()?;
    let jobs = &queue.jobs;
    let settings = store.load_settings()?;
    let mut receipts = load(store)?;
    let mut feedback = store.load_feedback()?;
    for observation in observed {
        let (scope, origin) = receipts
            .receipts
            .iter_mut()
            .find_map(|r| {
                r.owned
                    .iter_mut()
                    .find(|o| o.proof == observation.origin.proof)
                    .map(|origin| (&r.scope, origin))
            })
            .ok_or("Retained feedback provenance changed during the scan.")?;
        let tracked =
            crate::feedback::observed_pr(&queue.tracked, &origin.proof, &observation.head)?;
        if Binding::tracked(tracked) != Binding::tracked(scope) {
            return Err("Retained feedback binding changed; no observation accepted.".into());
        }
        if observation
            .threads
            .iter()
            .any(|t| !t.owned_by(&origin.proof))
        {
            return Err("Retained root provenance changed; no observation accepted.".into());
        }
        // Closure is authenticated provider evidence, not Agent work. Keep it
        // even when a reopened iteration has no assignments or jobs.
        origin.closed_roots.extend(
            observation
                .threads
                .iter()
                .filter(|t| t.resolved)
                .filter_map(|t| t.root().ok())
                .map(|root| root.id.clone()),
        );
        let Some(job) = jobs.iter().find(|j| {
            origin.matches(j)
                && crate::queue::item_id(j) == tracked.item_id
                && j.head_sha == observation.head
                && crate::monitoring::review_policy(&settings, j, None).is_ok()
        }) else {
            continue;
        };
        for root_id in &origin.proof.root_ids {
            let thread = observation
                .threads
                .iter()
                .find(|t| t.root().is_ok_and(|r| &r.id == root_id));
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
                unavailable: thread
                    .is_none()
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
    }
    save(store, &receipts)?;
    store.save_feedback(&feedback)?;
    // Conversation admission is primary-routed by the shared scan. Retention
    // observes original ownership and human closure; it must not fan out work
    // to the historical author or replay a reopened conversation.
    let _ = now;
    Ok(())
}
