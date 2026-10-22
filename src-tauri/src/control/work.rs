use crate::{
    capacity,
    monitoring::{OperationState, QueueJob},
    review,
    storage::Store,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestOverride {
    pub agent_id: Option<String>,
    pub doctrine_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct WatchResult {
    pub state: &'static str,
    pub reason: Option<&'static str>,
    pub paused: bool,
    pub prioritized: usize,
    pub item_id: Option<String>,
}

pub(crate) fn holds(store: &Store) -> Result<Vec<(String, u64)>, String> {
    match store.read_state("control-watch-holds.json") {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid pending watch requests.".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("Pending watch requests unavailable.".into()),
    }
}

pub(crate) fn hold(
    store: &Store,
    repository_id: &str,
    number: u64,
    held: bool,
) -> Result<(), String> {
    let mut values = holds(store)?;
    values.retain(|(id, n)| id != repository_id || *n != number);
    if held {
        values.push((repository_id.into(), number));
    }
    store.write_state("control-watch-holds.json", &values)
}

fn matches(job: &QueueJob, repository_id: &str, number: u64) -> bool {
    job.configuration_id == repository_id
        && job.number == number
        && !crate::monitoring::retired(job)
}

pub(crate) fn references(store: &Store, id: &str, doctrine: bool) -> Result<bool, String> {
    let reviews = store.review_evidence()?;
    Ok(store
        .load_queue()?
        .iter()
        .filter(|job| !crate::monitoring::retired(job))
        .any(|job| {
            job.work.as_ref().is_some_and(|work| {
                !reviews.iter().any(|run| {
                    run.key == work.id && run.operation.state == OperationState::Completed
                }) && work.request_override.as_ref().is_some_and(|request| {
                    if doctrine {
                        request
                            .doctrine_ids
                            .as_ref()
                            .is_some_and(|ids| ids.iter().any(|entry| entry == id))
                    } else {
                        request.agent_id.as_deref() == Some(id)
                    }
                })
            })
        }))
}

/// Changes queue order only; admission, Wait, backpressure and teardown are
/// still decided by the shared capacity coordinator.
pub fn prioritize(
    store: &Store,
    repository_id: &str,
    number: u64,
    now: i64,
) -> Result<usize, String> {
    let eligible: HashSet<_> = capacity::candidates(store, now)?
        .into_iter()
        .filter(|w| w.reason.is_none() && w.offline.is_none() && w.delayed_until.is_none())
        .map(|w| w.key.id)
        .collect();
    let running: HashSet<_> = store
        .load_reviews()?
        .into_iter()
        .filter(|r| r.operation.state == OperationState::Running)
        .map(|r| r.key)
        .collect();
    let mut queue = store.load_queue_state()?;
    let selected: HashSet<_> = queue
        .jobs
        .iter()
        .filter(|j| matches(j, repository_id, number))
        .filter_map(|j| j.work.as_ref())
        .filter(|w| eligible.contains(&w.id) && !running.contains(&w.id))
        .map(|w| w.id.clone())
        .collect();
    if selected.is_empty() {
        return Ok(0);
    }
    for job in &mut queue.jobs {
        if let Some(work) = &mut job.work {
            if running.contains(&work.id) || !eligible.contains(&work.id) {
                continue;
            }
            work.enqueue_order = if selected.contains(&work.id) {
                0
            } else {
                work.enqueue_order
                    .checked_add(1)
                    .ok_or("Queue ordering exhausted.")?
            };
            queue.next_enqueue_order = queue.next_enqueue_order.max(work.enqueue_order);
        }
    }
    store.save_queue_state(&queue)?;
    Ok(selected.len())
}

/// Start a new pass without replacing completed evidence or clearing receipts.
/// An unchanged pending/running/Wait state is never restarted by this request.
pub fn request_again(
    store: &Store,
    repository_id: &str,
    number: u64,
    now: i64,
) -> Result<bool, String> {
    let mut queue = store.load_queue_state()?;
    let mut latest = BTreeMap::new();
    for job in queue
        .jobs
        .iter()
        .filter(|j| matches(j, repository_id, number))
    {
        if let Some(work) = &job.work {
            let old: Option<&&QueueJob> = latest.get(&job.assignment_id);
            if old.is_none_or(|old| old.work.as_ref().unwrap().state_seq < work.state_seq) {
                latest.insert(job.assignment_id.clone(), job);
            }
        }
    }
    let selected: Vec<_> = latest.into_values().cloned().collect();
    let first = selected.first().ok_or("No current work for this PR.")?;
    let binding = crate::retention::Binding::job(first);
    let reviews = store.review_evidence()?;
    if selected.iter().any(|job| {
        !reviews
            .iter()
            .any(|run| run.matches_job(job) && run.operation.state == OperationState::Completed)
    }) {
        return Ok(false);
    }
    if crate::exits::consume::writing(store)?.contains(&binding)
        || store.load_publications()?.iter().any(|p| {
            crate::retention::Binding::job(&p.review.job) == binding && unsettled_publication(p)
        })
    {
        return Err("Reconcile provider writes before reviewing again.".into());
    }

    fn unsettled_publication(p: &crate::publication::Publication) -> bool {
        use crate::publication::RemoteState;
        p.uncertain
            || p.receipts
                .last()
                .is_some_and(|r| r.state == RemoteState::Pending)
            || (p.operation.pending_review_id.is_some()
                && p.receipts.last().is_none_or(|r| {
                    !matches!(r.state, RemoteState::Commented | RemoteState::Deleted)
                }))
            || (!p.cancelled
                && matches!(
                    p.operation.state,
                    OperationState::Queued | OperationState::Running | OperationState::Interrupted
                ))
    }
    let mut states = store.load_pr_states()?;
    let seq = states.review_again(&binding, now)?;
    for mut job in selected {
        let order = queue.allocate_order()?;
        let work = job.work.as_mut().unwrap();
        work.id = uuid::Uuid::new_v4().to_string();
        work.request_override = None;
        work.state_seq = seq;
        work.pass_ordinal = work
            .pass_ordinal
            .checked_add(1)
            .ok_or("Pass counter exhausted.")?;
        work.enqueue_order = order;
        queue.jobs.push(job);
    }
    // A crash after this write leaves pending state that the next heartbeat
    // materializes, never reuses a completed run's operation/receipt identity.
    store.save_pr_states(&states)?;
    store.save_queue_state(&queue)?;
    Ok(true)
}

pub(crate) fn apply_overrides(
    store: &Store,
    repository_id: &str,
    number: u64,
    overrides: &BTreeMap<String, RequestOverride>,
) -> Result<(), String> {
    if overrides.is_empty() {
        return Ok(());
    }
    let settings = store.load_settings()?;
    let reviews = store.review_evidence()?;
    let mut queue = store.load_queue_state()?;
    let mut found = HashSet::new();
    for job in queue
        .jobs
        .iter_mut()
        .filter(|j| matches(j, repository_id, number))
    {
        let Some(assignment) = &job.assignment_id else {
            continue;
        };
        let Some(value) = overrides.get(assignment) else {
            continue;
        };
        let Some(work) = &mut job.work else { continue };
        if reviews.iter().any(|r| r.key == work.id) {
            continue;
        }
        if value
            .doctrine_ids
            .as_ref()
            .is_some_and(|ids| ids.iter().collect::<HashSet<_>>().len() != ids.len())
        {
            return Err("Request doctrines must be unique.".into());
        }
        work.request_override = Some(value.clone());
        review::Selection::resolve(&settings, job, assignment)?;
        found.insert(assignment.clone());
    }
    if found.len() != overrides.len() {
        return Err("Overrides require never-started work for each addressed assignment. Use explicit review again for completed work.".into());
    }
    store.save_queue_state(&queue)
}

pub(crate) fn result(
    store: &Store,
    repository_id: &str,
    number: u64,
    prioritized: usize,
) -> Result<WatchResult, String> {
    let paused = store.load_automation()?.paused;
    let queue = store.load_queue_state()?;
    let jobs: Vec<_> = queue
        .jobs
        .iter()
        .filter(|j| matches(j, repository_id, number))
        .collect();
    let keys: HashSet<_> = jobs
        .iter()
        .filter_map(|j| j.work.as_ref().map(|w| &w.id))
        .collect();
    let runs = store.load_reviews()?;
    let states = store.load_pr_states()?;
    let external_wait = jobs.iter().any(|job| {
        states
            .get(&crate::retention::Binding::job(job))
            .is_some_and(|state| state.external_wait.is_some())
    });
    let held = holds(store)?
        .iter()
        .any(|(id, n)| id == repository_id && *n == number);
    let running = runs
        .iter()
        .any(|r| keys.contains(&r.key) && r.operation.state == OperationState::Running);
    let pending = capacity::candidates(store, crate::now_seconds()?)?
        .into_iter()
        .filter(|w| keys.contains(&w.key.id))
        .collect::<Vec<_>>();
    let waiting = paused
        || pending
            .iter()
            .all(|w| w.reason.is_some() || w.delayed_until.is_some() || w.offline.is_some());
    Ok(WatchResult {
        state: if running {
            "running"
        } else if jobs.is_empty() {
            "not_queued"
        } else if external_wait || held {
            "waiting"
        } else if pending.is_empty() {
            "completed"
        } else if waiting {
            "waiting"
        } else {
            "queued"
        },
        reason: if external_wait {
            Some("external_prerequisite")
        } else if held {
            Some("pending_configuration")
        } else if paused && !pending.is_empty() {
            Some("paused")
        } else if waiting
            && pending
                .iter()
                .any(|w| w.reason.as_deref() == Some(crate::feedback::WAITING_FOR_OBSERVATION))
        {
            // Retries automatically after the next clean feedback observation.
            Some("feedback_observation")
        } else if waiting && !pending.is_empty() {
            Some("execution_gates")
        } else {
            None
        },
        paused,
        prioritized,
        item_id: jobs.first().map(|j| crate::queue::item_id(j)),
    })
}
