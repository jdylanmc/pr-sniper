use crate::{
    backpressure::{Backpressure, Scope},
    monitoring::{JobOperation, OperationState},
    review::{self, ReviewRun},
    storage::Store,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interruption {
    Pause,
    CapacityReduction,
    Shutdown,
}

/// Only `Normal` runs. The other kinds name historical work (legacy primary final,
/// Reply and Mention analysis) kept in old records and retention receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Normal,
    PrimaryFinal,
    Reply,
    Mention,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkId {
    pub kind: Kind,
    pub id: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Automation {
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Work {
    pub key: WorkId,
    pub enqueue_order: u64,
    pub state: &'static str,
    pub reason: Option<String>,
    pub failure: Option<crate::monitoring::FailureClass>,
    /// Rate-limit backpressure: runnable work that waits until this time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delayed_until: Option<i64>,
    /// Typed cause when `reason` holds the work back behind another run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<review::teammates::BlockedBy>,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub paused: bool,
    pub capacity: u32,
    pub active: usize,
    pub stopping: usize,
    pub waiting: usize,
    pub blocked: usize,
    pub work: Vec<Work>,
}

pub struct Reservation {
    pub key: WorkId,
    pub operation_id: String,
    pub enqueue_order: u64,
    pub cancelled: Arc<AtomicBool>,
    pub interruption: Option<Interruption>,
    pub persistence_error: Option<String>,
}

/// One owner for every AI kind. Reservations include stopping workers until
/// runtime and blocking work have been joined, not merely signalled to cancel.
#[derive(Default)]
pub struct Coordinator {
    active: Mutex<BTreeMap<WorkId, Reservation>>,
}

pub enum Dispatch {
    Review(Box<ReviewRun>, Arc<AtomicBool>),
}

pub struct Batch {
    pub dispatched: Vec<Dispatch>,
    pub errors: Vec<String>,
}

impl Dispatch {
    pub fn key(&self) -> WorkId {
        match self {
            Self::Review(run, _) => WorkId {
                kind: Kind::Normal,
                id: run.key.clone(),
            },
        }
    }
}

/// Accounts whose rate limits can delay a piece of AI work: the GitHub
/// account it reads and writes through, and the Agent's Copilot account.
fn scopes(github_account: &str, selection: Option<&review::Selection>) -> Vec<Scope> {
    let mut scopes = vec![Scope::github(github_account)];
    if let Some(account) = selection.and_then(|s| s.agent.ai_account.as_ref()) {
        scopes.push(Scope::copilot(&account.account_id));
    }
    scopes
}

/// When runnable work may next start: the later of its own rate-limit wait
/// and any gate on its provider accounts.
pub(crate) fn pacing(
    gates: &Backpressure,
    scopes: &[Scope],
    operation: Option<&JobOperation>,
    now: i64,
) -> Option<i64> {
    let own = operation
        .filter(|op| op.state != OperationState::Running)
        .and_then(|op| op.backpressure_until)
        .filter(|until| *until > now);
    own.max(gates.until_any(scopes, now))
}

fn due(operation: &JobOperation, now: i64) -> bool {
    matches!(
        operation.state,
        OperationState::Queued | OperationState::Interrupted
    ) && operation.next_attempt_at.is_some_and(|n| n <= now)
}

/// A superseded run waits for its repository's next successful poll, never a timer.
pub const WAITING_FOR_POLL: &str = "Waiting for the next poll.";

pub fn candidates(store: &Store, now: i64) -> Result<Vec<Work>, String> {
    let mut result = Vec::new();
    let gates = store.load_backpressure()?;
    let reviews = review::host::candidates(store)?;
    let repositories = store.load_settings()?.repositories;
    let tracked = store.load_queue_state()?.tracked;
    let barriers: BTreeMap<String, review::teammates::TeammateRef> = reviews
        .iter()
        .filter(|c| {
            c.run
                .as_ref()
                .is_none_or(|r| r.operation.state != OperationState::Completed)
        })
        .filter_map(|c| {
            let repository = repositories
                .iter()
                .find(|r| r.id == c.job.configuration_id)?;
            Some((
                c.key.clone(),
                review::teammates::blocker(repository, &reviews, c)?,
            ))
        })
        .collect();
    for candidate in reviews {
        if candidate.run.as_ref().is_some_and(|r| {
            r.operation.state == OperationState::Completed || r.operation.ended_superseded()
        }) {
            continue;
        }
        // Running lists only real work: a retired entry can never run. A
        // superseded entry stays listed, distinct, until its PR ends or is cleaned.
        if crate::monitoring::retired(&candidate.job)
            && candidate.job.waiting != crate::monitoring::WAITING_SUPERSEDED
            && candidate
                .run
                .as_ref()
                .is_none_or(|r| r.operation.state != OperationState::Running)
        {
            continue;
        }
        let mut blocked_by = None;
        let mut reason = candidate.blocked;
        if reason.is_none() {
            if let Some(selection) = &candidate.planned_selection {
                reason = crate::feedback::contexts(store, &candidate.job, &selection.agent.id)
                    .err()
                    .map(|e| e.message);
            }
        }
        if reason.is_none() {
            reason = match &candidate.run {
                Some(run)
                    if matches!(
                        run.operation.state,
                        OperationState::Failed | OperationState::ManualRetry
                    ) =>
                {
                    Some(
                        run.error
                            .clone()
                            .unwrap_or_else(|| "Manual retry required.".into()),
                    )
                }
                Some(run)
                    if run.operation.state != OperationState::Running
                        && run
                            .operation
                            .superseded
                            .as_ref()
                            .is_some_and(|s| !s.cleared(&candidate.job, &tracked)) =>
                {
                    Some(WAITING_FOR_POLL.into())
                }
                Some(run)
                    if run.operation.state != OperationState::Running
                        && run.operation.backpressure_until.is_none()
                        && !due(&run.operation, now) =>
                {
                    Some("Waiting for retry backoff.".into())
                }
                _ => None,
            };
        }
        if reason.is_none()
            && candidate
                .run
                .as_ref()
                .is_none_or(|run| run.operation.state != OperationState::Running)
        {
            if let Some(teammate) = barriers.get(&candidate.key) {
                reason = Some(review::teammates::WAITING_FOR_TEAMMATES.into());
                blocked_by = Some(review::teammates::BlockedBy::Teammate(teammate.clone()));
            }
        }
        let scopes = scopes(
            &candidate.job.account_id,
            candidate
                .run
                .as_ref()
                .map(|run| &run.selection)
                .or(candidate.planned_selection.as_ref()),
        );
        let delayed_until = reason.is_none().then(|| {
            pacing(
                &gates,
                &scopes,
                candidate.run.as_ref().map(|run| &run.operation),
                now,
            )
        });
        result.push(Work {
            key: WorkId {
                kind: Kind::Normal,
                id: candidate.key,
            },
            enqueue_order: candidate
                .job
                .work
                .as_ref()
                .map(|w| w.enqueue_order)
                .unwrap_or(0),
            state: if reason.is_some() {
                "blocked"
            } else {
                "waiting"
            },
            reason,
            failure: candidate
                .run
                .as_ref()
                .and_then(|run| run.operation.failure_class()),
            delayed_until: delayed_until.flatten(),
            blocked_by,
        });
    }
    result.sort_by(|a, b| (a.enqueue_order, &a.key).cmp(&(b.enqueue_order, &b.key)));
    Ok(result)
}

impl Coordinator {
    /// Execution adapters reserve through this same owner after merging their
    /// candidates into global FIFO. `prepare` durably starts an attempt only
    /// after a slot is available. Call while holding the Host Store lock.
    pub fn reserve<T>(
        &self,
        store: &Store,
        work: &Work,
        prepare: impl FnOnce(Arc<AtomicBool>) -> Result<(String, T), String>,
    ) -> Result<Option<T>, String> {
        if store.load_automation()?.paused || work.reason.is_some() || work.delayed_until.is_some()
        {
            return Ok(None);
        }
        let capacity = store.load_settings()?.capacity;
        let mut active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        if active.len() >= capacity as usize || active.contains_key(&work.key) {
            return Ok(None);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (operation_id, prepared) = prepare(cancelled.clone())?;
        active.insert(
            work.key.clone(),
            Reservation {
                key: work.key.clone(),
                operation_id,
                enqueue_order: work.enqueue_order,
                cancelled,
                interruption: None,
                persistence_error: None,
            },
        );
        Ok(Some(prepared))
    }

    pub fn snapshot(&self, store: &Store, now: i64) -> Result<Snapshot, String> {
        let automation = store.load_automation()?;
        let capacity = store.load_settings()?.capacity;
        let mut work = candidates(store, now)?;
        let active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        for reservation in active.values() {
            let entry = Work {
                key: reservation.key.clone(),
                enqueue_order: reservation.enqueue_order,
                state: if reservation.cancelled.load(Ordering::SeqCst) {
                    "stopping"
                } else {
                    "active"
                },
                reason: reservation.persistence_error.clone().or_else(|| {
                    reservation
                        .interruption
                        .map(|r| format!("{r:?}; waiting for worker teardown."))
                }),
                failure: None,
                delayed_until: None,
                blocked_by: None,
            };
            if let Some(old) = work.iter_mut().find(|w| w.key == entry.key) {
                *old = entry;
            } else {
                work.push(entry);
            }
        }
        work.sort_by(|a, b| (a.enqueue_order, &a.key).cmp(&(b.enqueue_order, &b.key)));
        Ok(Snapshot {
            paused: automation.paused,
            capacity,
            active: active.len(),
            stopping: work.iter().filter(|w| w.state == "stopping").count(),
            waiting: work.iter().filter(|w| w.state == "waiting").count(),
            blocked: work.iter().filter(|w| w.state == "blocked").count(),
            work,
        })
    }

    /// Called under the Host Store lock, including by deterministic worker tests.
    pub fn dispatch(&self, store: &Store, now: i64) -> Result<Batch, String> {
        review::end_retired(store)?;
        let settings = store.load_settings()?;
        let paused = store.load_automation()?.paused;
        let mut active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        let mut ordered: Vec<_> = active
            .values()
            .map(|r| (r.enqueue_order, r.key.clone()))
            .collect();
        ordered.sort();
        let keep = if paused {
            0
        } else {
            settings.capacity as usize
        };
        for (_, key) in ordered.into_iter().skip(keep) {
            let reservation = active.get_mut(&key).ok_or("AI reservation disappeared.")?;
            if reservation.interruption.is_none() {
                let reason = if paused {
                    Interruption::Pause
                } else {
                    Interruption::CapacityReduction
                };
                request_interruption(store, &reservation.operation_id, reason)?;
                reservation.interruption = Some(reason);
                reservation.cancelled.store(true, Ordering::SeqCst);
            }
        }
        if paused {
            return Ok(Batch {
                dispatched: Vec::new(),
                errors: Vec::new(),
            });
        }
        drop(active);
        let mut dispatched = Vec::new();
        let mut errors = Vec::new();
        for candidate in candidates(store, now)? {
            let prepared = self.reserve(store, &candidate, |cancelled| {
                let (dispatch, operation_id) = match candidate.key.kind {
                    Kind::Normal => {
                        let run = review::host::prepare_dispatch(store, &candidate.key.id, now)?;
                        let id = run.operation.id.clone();
                        (Dispatch::Review(Box::new(run), cancelled.clone()), id)
                    }
                    Kind::PrimaryFinal | Kind::Reply | Kind::Mention => {
                        return Err("Historical work kind cannot run.".into())
                    }
                };
                Ok((operation_id, dispatch))
            });
            let dispatch = match prepared {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            dispatched.push(dispatch);
        }
        Ok(Batch { dispatched, errors })
    }

    pub fn release(&self, key: &WorkId, operation_id: &str) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        if !active
            .get(key)
            .is_some_and(|r| r.operation_id == operation_id)
        {
            return Err("AI completion does not own this reservation.".into());
        }
        active.remove(key);
        Ok(())
    }

    pub fn cancel(&self, operation_id: &str) -> Result<(), String> {
        let active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        if let Some(reservation) = active.values().find(|r| r.operation_id == operation_id) {
            reservation.cancelled.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub(crate) fn persistence_failed(&self, operation_id: &str) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        let reservation = active
            .values_mut()
            .find(|r| r.operation_id == operation_id)
            .ok_or("AI reservation disappeared.")?;
        reservation.cancelled.store(true, Ordering::SeqCst);
        reservation.persistence_error = Some("Worker ended, but its outcome could not be persisted. Slot retained; repair storage and restart.".into());
        Ok(())
    }

    pub fn finished(&self) -> bool {
        self.active.lock().is_ok_and(|a| a.is_empty())
    }

    pub(crate) fn settle_terminal(&self, store: &Store) -> Result<bool, String> {
        let terminal: Vec<_> = store
            .load_queue_state()?
            .tracked
            .into_iter()
            .filter(|p| {
                p.terminal_observed && p.lifecycle != crate::github::metadata::Lifecycle::Open
            })
            .map(|p| crate::retention::Binding::tracked(&p))
            .collect();
        let matches = |job: &crate::monitoring::QueueJob| terminal.iter().any(|b| b.matches(job));
        let mut work = std::collections::BTreeSet::new();
        work.extend(
            store
                .load_reviews()?
                .into_iter()
                .filter(|r| matches(&r.job))
                .map(|r| WorkId {
                    kind: Kind::Normal,
                    id: r.key,
                }),
        );
        let active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        let mut settled = true;
        for reservation in active.values().filter(|r| work.contains(&r.key)) {
            reservation.cancelled.store(true, Ordering::SeqCst);
            settled = false;
        }
        Ok(settled)
    }

    pub(crate) fn shutdown(&self, store: &Store) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|_| "AI capacity unavailable.")?;
        for reservation in active.values_mut() {
            request_interruption(store, &reservation.operation_id, Interruption::Shutdown)?;
            reservation.interruption = Some(Interruption::Shutdown);
            reservation.cancelled.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub(crate) fn pump(app: &tauri::AppHandle) -> Result<(), String> {
        let host = app.state::<crate::Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Ok(());
        }
        let dispatches = {
            let store = host.store.lock().map_err(|_| "AI storage unavailable.")?;
            host.ai.dispatch(&store, crate::now_seconds()?)?
        };
        launch_batch(app, dispatches);
        Ok(())
    }
}

pub(crate) fn launch_batch(app: &tauri::AppHandle, batch: Batch) {
    for error in batch.errors {
        crate::report(app, error);
    }
    for dispatch in batch.dispatched {
        match dispatch {
            Dispatch::Review(run, cancelled) => review::host::launch_worker(app, *run, cancelled),
        }
    }
}

fn request_interruption(store: &Store, id: &str, reason: Interruption) -> Result<(), String> {
    let mut reviews = store.load_reviews()?;
    let run = reviews
        .iter_mut()
        .find(|r| r.operation.id == id)
        .ok_or("The active AI operation is missing.")?;
    run.operation.interruption = Some(reason);
    store.save_reviews(&reviews)
}

/// A genuine failure wins a race with pause. Only successful-but-discarded
/// output or an explicit cancellation restores the pre-attempt budget.
pub fn interrupted(
    operation: &mut JobOperation,
    error: Option<&review::Failure>,
    now: i64,
) -> bool {
    if operation.interruption.is_some() && error.is_none_or(|e| e.cancelled) {
        operation.requeue_intentional(now);
        true
    } else {
        false
    }
}

pub fn publication_gate(store: &Store) -> Result<(), String> {
    publication_gate_checked(store).map_err(crate::publication::GateError::message)
}

pub(crate) fn publication_gate_checked(store: &Store) -> Result<(), crate::publication::GateError> {
    use crate::publication::GateError;
    if store.load_automation().map_err(GateError::Storage)?.paused {
        Err(GateError::Policy(
            "Automation paused; reconcile any already-started provider mutation after resume."
                .into(),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn refill(app: &tauri::AppHandle, key: &WorkId, operation_id: &str) {
    if let Err(error) = app
        .state::<crate::Host>()
        .ai
        .release(key, operation_id)
        .and_then(|()| Coordinator::pump(app))
    {
        crate::report(app, error);
    }
}

#[tauri::command]
pub(crate) fn set_automation_paused(app: tauri::AppHandle, paused: bool) -> Result<(), String> {
    let host = app.state::<crate::Host>();
    let batch = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Automation storage unavailable.")?;
        store.save_automation(&Automation { paused })?;
        host.ai.dispatch(&store, crate::now_seconds()?)?
    };
    launch_batch(&app, batch);
    Ok(())
}

#[tauri::command]
pub(crate) async fn automation_snapshot(app: tauri::AppHandle) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<crate::Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| "Automation storage unavailable.")?;
        host.ai.snapshot(&store, crate::now_seconds()?)
    })
    .await
    .map_err(|_| "Automation state could not be read.".to_string())?
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod barrier_tests;
