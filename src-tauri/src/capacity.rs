use crate::{
    follow_up::{self, FollowUp},
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
    Reply(Box<FollowUp>, Arc<AtomicBool>),
}

pub struct Batch {
    pub dispatched: Vec<Dispatch>,
    pub errors: Vec<String>,
}

impl Dispatch {
    pub fn key(&self) -> WorkId {
        match self {
            Self::Review(run, _) => WorkId {
                kind: if run.operation.operation_type == "primary_final_review" {
                    Kind::PrimaryFinal
                } else {
                    Kind::Normal
                },
                id: run.key.clone(),
            },
            Self::Reply(run, _) => WorkId {
                kind: run.kind(),
                id: run.id.clone(),
            },
        }
    }
}

fn due(operation: &JobOperation, now: i64) -> bool {
    matches!(
        operation.state,
        OperationState::Queued | OperationState::Interrupted
    ) && operation.next_attempt_at.is_some_and(|n| n <= now)
}

pub fn candidates(store: &Store, now: i64) -> Result<Vec<Work>, String> {
    let mut result = crate::actions::host::candidates(store, now)?;
    let follow_ups = follow_up::host::candidates(store)?;
    for mention in store.load_feedback()?.mentions.into_iter().filter(|m| {
        m.follow_up_id.is_none()
            && !follow_ups
                .iter()
                .any(|f| f.run.id == m.work_id && f.run.key == m.key)
    }) {
        result.push(Work {
            key: WorkId {
                kind: Kind::Mention,
                id: mention.work_id,
            },
            enqueue_order: mention.enqueue_order,
            state: "blocked",
            reason: Some(
                mention
                    .blocked
                    .unwrap_or_else(|| "Mention routing is unavailable.".into()),
            ),
        });
    }
    for candidate in review::host::candidates(store)? {
        if candidate
            .run
            .as_ref()
            .is_some_and(|r| r.operation.state == OperationState::Completed)
        {
            continue;
        }
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
                    if !run.manual_start
                        && candidate
                            .planned_selection
                            .as_ref()
                            .is_none_or(|s| !s.policy.automatic_agent_start) =>
                {
                    Some("Manual start required.".into())
                }
                Some(run)
                    if run.operation.state != OperationState::Running
                        && !due(&run.operation, now) =>
                {
                    Some("Waiting for retry backoff.".into())
                }
                None if candidate
                    .planned_selection
                    .as_ref()
                    .is_none_or(|s| !s.policy.automatic_agent_start) =>
                {
                    Some("Manual start required.".into())
                }
                _ => None,
            };
        }
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
        });
    }
    for candidate in follow_ups {
        let run = candidate.run;
        if run.result.is_some() || run.publication.is_some() {
            continue;
        }
        let mut reason = candidate.blocked;
        if reason.is_none() {
            reason = if run.cancelled {
                Some("Follow-up cancelled; manual retry required.".into())
            } else if run.analysis.as_ref().is_some_and(|op| {
                matches!(
                    op.state,
                    OperationState::Failed | OperationState::ManualRetry
                )
            }) {
                Some(
                    run.error
                        .clone()
                        .unwrap_or_else(|| "Manual retry required.".into()),
                )
            } else if !run.manual_start && !candidate.automatic_start {
                Some("Manual follow-up start required.".into())
            } else if run
                .analysis
                .as_ref()
                .is_some_and(|op| op.state != OperationState::Running && !due(op, now))
            {
                Some("Waiting for retry backoff.".into())
            } else {
                None
            };
        }
        result.push(Work {
            key: WorkId {
                kind: run.kind(),
                id: run.id,
            },
            enqueue_order: run.enqueue_order.unwrap_or(0),
            state: if reason.is_some() {
                "blocked"
            } else {
                "waiting"
            },
            reason,
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
        if store.load_automation()?.paused || work.reason.is_some() {
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
    /// The same reservation map must also admit future primary-final/mention work.
    pub fn dispatch(&self, store: &Store, now: i64) -> Result<Batch, String> {
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
                    Kind::Reply | Kind::Mention => {
                        let run = follow_up::host::prepare_dispatch(store, &candidate.key.id, now)?;
                        let id = run
                            .analysis
                            .as_ref()
                            .ok_or("Reply analysis missing.")?
                            .id
                            .clone();
                        (Dispatch::Reply(Box::new(run), cancelled.clone()), id)
                    }
                    Kind::PrimaryFinal => {
                        let run =
                            crate::actions::host::prepare_dispatch(store, &candidate.key.id, now)?;
                        let id = run.operation.id.clone();
                        (Dispatch::Review(Box::new(run), cancelled.clone()), id)
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
        work.extend(
            store
                .load_follow_ups()?
                .into_iter()
                .filter(|f| matches(&f.context.job))
                .map(|f| WorkId {
                    kind: f.kind(),
                    id: f.id,
                }),
        );
        work.extend(
            store
                .load_actions()?
                .finals
                .into_iter()
                .filter(|f| matches(&f.basis.job))
                .map(|f| WorkId {
                    kind: Kind::PrimaryFinal,
                    id: f.id,
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
            Dispatch::Review(run, cancelled)
                if run.operation.operation_type == "primary_final_review" =>
            {
                crate::actions::host::launch_worker(app, *run, cancelled)
            }
            Dispatch::Review(run, cancelled) => review::host::launch_worker(app, *run, cancelled),
            Dispatch::Reply(run, cancelled) => {
                follow_up::host::launch_analysis_worker(app, *run, cancelled)
            }
        }
    }
}

fn request_interruption(store: &Store, id: &str, reason: Interruption) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    if let Some(run) = ledger
        .finals
        .iter_mut()
        .find(|f| f.execution.operation.id == id)
    {
        run.execution.operation.interruption = Some(reason);
        return store.save_actions(&ledger);
    }
    let mut reviews = store.load_reviews()?;
    if let Some(run) = reviews.iter_mut().find(|r| r.operation.id == id) {
        run.operation.interruption = Some(reason);
        return store.save_reviews(&reviews);
    }
    let mut replies = store.load_follow_ups()?;
    let run = replies
        .iter_mut()
        .find(|r| r.analysis.as_ref().is_some_and(|op| op.id == id))
        .ok_or("The active AI operation is missing.")?;
    run.analysis
        .as_mut()
        .ok_or("Reply analysis missing.")?
        .interruption = Some(reason);
    store.save_follow_ups(&replies)
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
    if store.load_automation()?.paused {
        Err(
            "Automation paused; reconcile any already-started provider mutation after resume."
                .into(),
        )
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
pub(crate) fn automation_snapshot(app: tauri::AppHandle) -> Result<Snapshot, String> {
    let host = app.state::<crate::Host>();
    let store = host
        .store
        .lock()
        .map_err(|_| "Automation storage unavailable.")?;
    host.ai.snapshot(&store, crate::now_seconds()?)
}

#[cfg(test)]
pub(crate) mod tests;
