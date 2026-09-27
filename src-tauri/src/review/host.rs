use super::{key, runtime, Failure, ReviewRun, Selection};
use crate::{
    github::provider::RemoteRepository,
    monitoring::{self, OperationState, QueueJob},
    now_seconds, Host,
};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Default)]
pub(crate) struct Coordinator {
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

#[derive(Serialize)]
pub(crate) struct Candidate {
    pub key: String,
    pub assignment_id: String,
    pub agent_name: String,
    pub job: QueueJob,
    pub trust_required: bool,
    pub blocked: Option<String>,
    pub run: Option<ReviewRun>,
}

pub(crate) fn candidates(store: &crate::storage::Store) -> Result<Vec<Candidate>, String> {
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    let reviews = store.load_reviews()?;
    let mut result = Vec::new();
    for job in jobs {
        let Some(repository) = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id)
        else {
            continue;
        };
        for assignment in &repository.assignments {
            if job
                .assignment_id
                .as_deref()
                .is_some_and(|id| id != assignment.id)
            {
                continue;
            }
            let key = key(&job, &assignment.id);
            let selection = Selection::resolve(&settings, &job, &assignment.id);
            let agent_name = settings
                .agents
                .iter()
                .find(|a| a.id == assignment.agent_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Missing Agent".into());
            result.push(Candidate {
                run: reviews.iter().rev().find(|r| r.key == key).cloned(),
                key,
                assignment_id: assignment.id.clone(),
                agent_name,
                trust_required: job.waiting == monitoring::WAITING_TRUST_CONFIRMATION,
                blocked: selection.err(),
                job: job.clone(),
            });
        }
    }
    for run in reviews.into_iter().rev() {
        if !result.iter().any(|c| c.key == run.key) {
            result.push(Candidate {
                key: run.key.clone(),
                assignment_id: run.assignment_id.clone(),
                agent_name: run.selection.agent.name.clone(),
                job: run.job.clone(),
                trust_required: false,
                blocked: Some(
                    "Historical review; assignment or repository is no longer available.".into(),
                ),
                run: Some(run),
            });
        }
    }
    Ok(result)
}

impl Coordinator {
    pub(crate) fn cancel_all(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some((_, cancelled)) = active.as_ref() {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    pub(crate) fn finished(&self) -> bool {
        self.active.lock().is_ok_and(|a| a.is_none())
    }

    pub(crate) fn pump(app: &tauri::AppHandle) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) || !host.reviews.finished() {
            return Ok(());
        }
        let next = {
            let active = host
                .reviews
                .active
                .lock()
                .map_err(|_| "Review execution is unavailable.")?;
            if active.is_some() {
                return Ok(());
            }
            let store = host
                .store
                .lock()
                .map_err(|_| "Review storage is unavailable.")?;
            super::restore(&store)?;
            let settings = store.load_settings()?;
            candidates(&store)?.into_iter().find(|candidate| {
                if candidate.blocked.is_some() {
                    return false;
                }
                let Ok(selection) =
                    Selection::resolve(&settings, &candidate.job, &candidate.assignment_id)
                else {
                    return false;
                };
                match &candidate.run {
                    None => selection.policy.automatic_agent_start && !candidate.trust_required,
                    Some(run) => {
                        matches!(
                            run.operation.state,
                            OperationState::Queued | OperationState::Interrupted
                        ) && (selection.policy.automatic_agent_start || run.manual_start)
                            && run
                                .operation
                                .next_attempt_at
                                .is_some_and(|t| now_seconds().is_ok_and(|now| now >= t))
                    }
                }
            })
        };
        if let Some(candidate) = next {
            Self::launch(app, &candidate.key, false, false)?;
        }
        Ok(())
    }

    fn launch(
        app: &tauri::AppHandle,
        candidate_key: &str,
        manual: bool,
        confirm_trust: bool,
    ) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err("PR Sniper is quitting.".into());
        }
        let mut active = host
            .reviews
            .active
            .lock()
            .map_err(|_| "Review execution is unavailable.")?;
        if active.is_some() {
            if !manual {
                return Ok(());
            }
            return Err("A review is already running. Wait or cancel it first.".into());
        }
        let run = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Review storage is unavailable.")?;
            let settings = store.load_settings()?;
            let candidate = candidates(&store)?
                .into_iter()
                .find(|c| c.key == candidate_key)
                .ok_or("This review candidate is no longer available.")?;
            if let Some(error) = candidate.blocked {
                return Err(error);
            }
            let selection =
                Selection::resolve(&settings, &candidate.job, &candidate.assignment_id)?;
            if !manual
                && !selection.policy.automatic_agent_start
                && candidate.run.as_ref().is_none_or(|r| !r.manual_start)
            {
                return Err("Automatic start is disabled; explicitly start this review.".into());
            }
            let mut reviews = store.load_reviews()?;
            let now = now_seconds()?;
            let mut run = match candidate.run {
                Some(run) if run.operation.state == OperationState::Completed => {
                    return Err("This Agent already reviewed this revision.".into())
                }
                Some(run)
                    if matches!(
                        run.operation.state,
                        OperationState::Queued | OperationState::Interrupted
                    ) =>
                {
                    run
                }
                Some(_) if !manual => return Err("Manual retry is required.".into()),
                _ => ReviewRun {
                    key: candidate.key,
                    assignment_id: candidate.assignment_id,
                    operation: monitoring::JobOperation::review(&candidate.job, now),
                    job: candidate.job,
                    selection: selection.clone(),
                    manual_start: manual,
                    trust_confirmed: confirm_trust,
                    phase: "Preparing immutable review context".into(),
                    error: None,
                    result: None,
                },
            };
            if run.selection != selection {
                run.error = Some(
                    "Review configuration changed. Explicitly retry with the new configuration."
                        .into(),
                );
                run.operation.fail(
                    &Failure::permanent(run.error.clone().unwrap()).monitoring(),
                    now,
                );
                replace_run(&mut reviews, &run);
                store.save_reviews(&reviews)?;
                return Ok(());
            }
            run.manual_start |= manual;
            run.trust_confirmed |= confirm_trust;
            if candidate.trust_required && !run.trust_confirmed {
                return Err("Confirm trust for this exact head revision before starting.".into());
            }
            if manual {
                run.operation.next_attempt_at = Some(now);
            }
            if let Err(error) = run.operation.begin_attempt(now) {
                run.error = Some(error);
                replace_run(&mut reviews, &run);
                store.save_reviews(&reviews)?;
                return Ok(());
            }
            run.phase = "Preparing immutable review context".into();
            run.error = None;
            replace_run(&mut reviews, &run);
            store.save_reviews(&reviews)?;
            run
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        *active = Some((run.operation.id.clone(), cancelled.clone()));
        drop(active);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = execute(app.clone(), run.clone(), cancelled).await;
            let host = app.state::<Host>();
            let saved = (|| {
                let generations = host
                    .github_generations
                    .lock()
                    .map_err(|_| "GitHub coordination unavailable.")?;
                let auth = host
                    .github_auth
                    .lock()
                    .map_err(|_| "GitHub account state unavailable.")?;
                let store = host
                    .store
                    .lock()
                    .map_err(|_| "Review storage is unavailable.")?;
                let outcome = outcome.and_then(|(result, generation)| {
                    if host.quitting.load(Ordering::SeqCst)
                        || generations.get(&run.job.account_id).copied().unwrap_or(0) != generation
                    {
                        return Err(Failure::permanent(
                            "Review cancelled or GitHub account connection changed.",
                        ));
                    }
                    auth.account_session_allowed(&run.job.account_id)
                        .map_err(Failure::from)?;
                    validate_saved_selection(&store, &run)?;
                    Ok(result)
                });
                let mut reviews = store.load_reviews()?;
                let current = reviews
                    .iter_mut()
                    .find(|r| r.operation.id == run.operation.id)
                    .ok_or("Review operation disappeared.")?;
                if current.operation.state != OperationState::Running {
                    return Ok::<_, String>(());
                }
                match outcome {
                    Ok(result) => {
                        current.operation.state = OperationState::Completed;
                        current.operation.failure = None;
                        current.operation.next_attempt_at = None;
                        current.phase =
                            "Automated review complete; final human review required".into();
                        current.result = Some(result);
                        current.error = None;
                    }
                    Err(error) => {
                        current.operation.fail(&error.monitoring(), now_seconds()?);
                        current.phase = "Review stopped".into();
                        current.error = Some(error.message);
                    }
                }
                store.save_reviews(&reviews)
            })();
            if let Err(error) = saved {
                crate::report(&app, error);
            }
            match host.reviews.active.lock() {
                Ok(mut active) => {
                    *active = None;
                }
                Err(_) => crate::report(&app, "Review execution state is unavailable.".into()),
            };
        });
        Ok(())
    }
}

fn replace_run(reviews: &mut Vec<ReviewRun>, run: &ReviewRun) {
    if let Some(previous) = reviews
        .iter_mut()
        .find(|r| r.operation.id == run.operation.id)
    {
        *previous = run.clone();
    } else {
        reviews.push(run.clone());
    }
}

fn local_gate(app: &tauri::AppHandle, run: &ReviewRun) -> Result<(), Failure> {
    let host = app.state::<Host>();
    if host.quitting.load(Ordering::SeqCst) {
        return Err(Failure::permanent("PR Sniper is quitting."));
    }
    host.github_auth
        .lock()
        .map_err(|_| Failure::permanent("GitHub account state unavailable."))?
        .account_session_allowed(&run.job.account_id)
        .map_err(Failure::from)?;
    let store = host
        .store
        .lock()
        .map_err(|_| Failure::permanent("Review storage is unavailable."))?;
    validate_saved_selection(&store, run)
}

fn validate_saved_selection(store: &crate::storage::Store, run: &ReviewRun) -> Result<(), Failure> {
    let settings = store.load_settings().map_err(Failure::permanent)?;
    let jobs = store.load_queue().map_err(Failure::permanent)?;
    let job = jobs
        .iter()
        .find(|j| key(j, &run.assignment_id) == run.key)
        .ok_or_else(|| Failure::permanent("Review detection is no longer available."))?;
    let current =
        Selection::resolve(&settings, job, &run.assignment_id).map_err(Failure::permanent)?;
    if current != run.selection || (!current.policy.automatic_agent_start && !run.manual_start) {
        return Err(Failure::permanent(
            "Agent configuration or start gate changed; explicitly retry.",
        ));
    }
    Ok(())
}

fn remote_gate(
    app: &tauri::AppHandle,
    run: &ReviewRun,
) -> Result<crate::github::metadata::PullRequest, Failure> {
    local_gate(app, run)?;
    let host = app.state::<Host>();
    let (_, client) = crate::github_session(&host, &run.job.account_id).map_err(Failure::from)?;
    let repo = RemoteRepository {
        id: run.job.repository_id.clone(),
        name: run.job.repository_name.clone(),
    };
    let pull = client.review_pull(&repo, run.job.number)?;
    let store = host
        .store
        .lock()
        .map_err(|_| Failure::permanent("Review storage is unavailable."))?;
    let settings = store.load_settings().map_err(Failure::permanent)?;
    monitoring::review_policy(&settings, &run.job, Some(&pull)).map_err(Failure::permanent)?;
    if super::requires_trust(&run.job, &pull) && !run.trust_confirmed {
        return Err(Failure::permanent(
            "This fork or author requires explicit trust confirmation.",
        ));
    }
    Ok(pull)
}

async fn execute(
    app: tauri::AppHandle,
    run: ReviewRun,
    cancelled: Arc<AtomicBool>,
) -> Result<(super::ReviewResult, u64), Failure> {
    let remaining = run
        .operation
        .retry_deadline
        .saturating_sub(now_seconds().map_err(Failure::permanent)?);
    if remaining <= 0 {
        return Err(Failure::timeout());
    }
    let deadline = Instant::now() + Duration::from_secs(remaining as u64);
    let generation = app
        .state::<Host>()
        .github_generations
        .lock()
        .map_err(|_| Failure::permanent("GitHub coordination unavailable."))?
        .get(&run.job.account_id)
        .copied()
        .unwrap_or(0);
    let gate_app = app.clone();
    let gate_run = run.clone();
    let gate_cancelled = cancelled.clone();
    let gate: runtime::Gate = Arc::new(move || {
        if Instant::now() >= deadline {
            return Err(Failure::timeout());
        }
        if gate_cancelled.load(Ordering::SeqCst) {
            return Err(Failure::permanent("Review cancelled."));
        }
        let host = gate_app.state::<Host>();
        if host
            .github_generations
            .lock()
            .map_err(|_| Failure::permanent("GitHub coordination unavailable."))?
            .get(&gate_run.job.account_id)
            .copied()
            .unwrap_or(0)
            != generation
        {
            return Err(Failure::permanent(
                "GitHub account connection changed during review.",
            ));
        }
        local_gate(&gate_app, &gate_run)
    });
    gate()?;
    let preparation_app = app.clone();
    let preparation_run = run.clone();
    let preparation_gate = gate.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        preparation_gate()?;
        remote_gate(&preparation_app, &preparation_run)?;
        let host = preparation_app.state::<Host>();
        let (_, client) =
            crate::github_session(&host, &preparation_run.job.account_id).map_err(Failure::from)?;
        let client = client.guarded(Arc::new(move || {
            preparation_gate().map_err(|error| {
                if error.kind == monitoring::OperationFailure::Timeout {
                    crate::github::ConnectionError::Timeout
                } else {
                    crate::github::ConnectionError::Configuration
                }
            })
        }));
        let repo = RemoteRepository {
            id: preparation_run.job.repository_id.clone(),
            name: preparation_run.job.repository_name.clone(),
        };
        let context = client.review_context(
            &repo,
            preparation_run.job.number,
            &preparation_run.job.head_sha,
        )?;
        Ok::<_, Failure>((context, Arc::new(client)))
    });
    let (context, client) = tokio::time::timeout_at(deadline.into(), prepared)
        .await
        .map_err(|_| Failure::timeout())?
        .map_err(|_| Failure::permanent("Review preparation could not finish."))??;
    gate()?;
    if cancelled.load(Ordering::SeqCst) {
        return Err(Failure::permanent("Review cancelled."));
    }
    {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Review storage unavailable."))?;
        let mut reviews = store.load_reviews().map_err(Failure::permanent)?;
        let current = reviews
            .iter_mut()
            .find(|r| r.operation.id == run.operation.id)
            .ok_or_else(|| Failure::permanent("Review operation disappeared."))?;
        if current.operation.state != OperationState::Running {
            return Err(Failure::permanent("Review is no longer running."));
        }
        current.phase = "Verifying restricted Copilot runtime and reviewing".into();
        store.save_reviews(&reviews).map_err(Failure::permanent)?;
    }
    let remote_app = app.clone();
    let remote_run = run.clone();
    let expected_base = context.pull.base_sha.clone();
    let send_base = expected_base.clone();
    let request = runtime::Request {
        context,
        client,
        repository_name: run.job.repository_name.clone(),
        selection: run.selection.clone(),
        before_send: Arc::new(move || {
            let pull = remote_gate(&remote_app, &remote_run)?;
            if pull.base_sha != send_base {
                return Err(Failure::permanent(
                    "Pull request base changed; review context is stale.",
                ));
            }
            Ok(())
        }),
        local_gate: gate.clone(),
    };
    let integration = app.state::<Host>().copilot.clone();
    let result = integration.review(request, cancelled, deadline).await?;
    let final_app = app.clone();
    let pull = tauri::async_runtime::spawn_blocking(move || remote_gate(&final_app, &run))
        .await
        .map_err(|_| Failure::permanent("Final eligibility check could not finish."))??;
    if pull.base_sha != expected_base {
        return Err(Failure::permanent(
            "Pull request base changed during review; result is stale.",
        ));
    }
    gate()?;
    Ok((result, generation))
}

#[tauri::command]
pub(crate) async fn start_review(
    app: tauri::AppHandle,
    candidate_key: String,
    confirm_trust: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        Coordinator::launch(&app, &candidate_key, true, confirm_trust)
    })
    .await
    .map_err(|_| "Review start could not finish.".to_string())?
}

#[tauri::command]
pub(crate) async fn cancel_review(
    app: tauri::AppHandle,
    operation_id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let active = host
            .reviews
            .active
            .lock()
            .map_err(|_| "Review execution is unavailable.")?;
        let (_, cancelled) = active
            .as_ref()
            .filter(|(id, _)| id == &operation_id)
            .ok_or("This review is not running.")?;
        cancelled.store(true, Ordering::SeqCst);
        let store = host
            .store
            .lock()
            .map_err(|_| "Review storage unavailable.")?;
        let mut reviews = store.load_reviews()?;
        let run = reviews
            .iter_mut()
            .find(|r| r.operation.id == operation_id)
            .ok_or("Review operation disappeared.")?;
        run.operation.fail(
            &Failure::permanent("Review cancelled by user.").monitoring(),
            now_seconds()?,
        );
        run.error = Some("Review cancelled by user.".into());
        run.phase = "Cancelled".into();
        store.save_reviews(&reviews)
    })
    .await
    .map_err(|_| "Review cancellation could not finish.".to_string())?
}

#[cfg(test)]
mod tests;
