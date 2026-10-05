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
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Serialize)]
pub(crate) struct Candidate {
    pub key: String,
    pub assignment_id: String,
    pub agent_name: String,
    pub job: QueueJob,
    pub blocked: Option<String>,
    pub planned_selection: Option<Selection>,
    pub run: Option<ReviewRun>,
}

pub(crate) fn candidates(store: &crate::storage::Store) -> Result<Vec<Candidate>, String> {
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    let reviews = store.review_evidence()?;
    let mut result = Vec::new();
    for job in &jobs {
        let Some(repository) = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id)
        else {
            continue;
        };
        for assignment in &repository.assignments {
            if job
                .work
                .as_ref()
                .is_some_and(|w| w.agent_id != assignment.agent_id)
                || job
                    .assignment_id
                    .as_deref()
                    .is_some_and(|id| id != assignment.id)
            {
                continue;
            }
            let key = key(job, &assignment.id);
            let selection = Selection::resolve(&settings, job, &assignment.id);
            let agent_name = settings
                .agents
                .iter()
                .find(|a| a.id == assignment.agent_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Missing Agent".into());
            let run = reviews.iter().rev().find(|r| r.key == key).cloned();
            result.push(Candidate {
                agent_name: run
                    .as_ref()
                    .map(|r| r.selection.agent.name.clone())
                    .unwrap_or(agent_name),
                planned_selection: selection.as_ref().ok().cloned(),
                run,
                key,
                assignment_id: assignment.id.clone(),
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
                job: jobs
                    .iter()
                    .find(|j| run.matches_job(j))
                    .cloned()
                    .unwrap_or_else(|| run.job.clone()),
                blocked: Some(
                    "Historical review; assignment or repository is no longer available.".into(),
                ),
                run: Some(run),
                planned_selection: None,
            });
        }
    }
    result.sort_by_key(|c| {
        (
            c.job.work.as_ref().map(|w| w.enqueue_order).unwrap_or(0),
            c.job.detected_at,
            c.key.clone(),
        )
    });
    Ok(result)
}

pub(crate) fn request(
    store: &crate::storage::Store,
    candidate_key: &str,
    manual: bool,
    now: i64,
) -> Result<ReviewRun, String> {
    let settings = store.load_settings()?;
    let candidate = candidates(store)?
        .into_iter()
        .find(|c| c.key == candidate_key)
        .ok_or("This review candidate is no longer available.")?;
    if let Some(error) = candidate.blocked {
        return Err(error);
    }
    let selection = Selection::resolve(&settings, &candidate.job, &candidate.assignment_id)?;
    if !manual
        && !selection.policy.automatic_agent_start
        && candidate.run.as_ref().is_none_or(|r| !r.manual_start)
    {
        return Err("Automatic start is disabled; explicitly start this review.".into());
    }
    let mut reviews = store.load_reviews()?;
    if candidate
        .run
        .as_ref()
        .is_some_and(|r| r.operation.state == OperationState::Running)
    {
        return Err("This review is running or stopping; wait for teardown.".into());
    }
    if candidate
        .run
        .as_ref()
        .is_some_and(|run| run.operation.state == OperationState::Completed)
        && store.load_publications()?.iter().any(|publication| {
            publication.review.key == candidate.key && publication.reserves_revision()
        })
    {
        return Err("Reconcile the original publication before reviewing this revision again; a confirmed published batch cannot be replaced.".into());
    }
    let mut run = match candidate.run {
        Some(run) if run.operation.state == OperationState::Completed && !manual => {
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
            feedback_context: None,
            key: candidate.key,
            assignment_id: candidate.assignment_id,
            operation: monitoring::JobOperation::review(&candidate.job, now),
            job: candidate.job,
            selection: selection.clone(),
            manual_start: manual,
            trust_confirmed: false,
            phase: "Waiting for shared AI capacity".into(),
            error: None,
            result: None,
        },
    };
    if !run.selection.same_execution(&selection) {
        run.error = Some(
            "Review configuration changed. Explicitly retry with the new configuration.".into(),
        );
        run.operation.fail(
            &Failure::permanent(run.error.clone().unwrap()).monitoring(),
            now,
        );
        replace_run(&mut reviews, &run);
        store.save_reviews(&reviews)?;
        return Err(run.error.unwrap());
    }
    run.manual_start |= manual;
    replace_run(&mut reviews, &run);
    store.save_reviews(&reviews)?;
    Ok(run)
}

pub(crate) fn prepare_dispatch(
    store: &crate::storage::Store,
    key: &str,
    now: i64,
) -> Result<ReviewRun, String> {
    let mut run = request(store, key, false, now)?;
    run.feedback_context = Some(
        crate::feedback::contexts(store, &run.job, &run.selection.agent.id)
            .map_err(|e| e.message)?,
    );
    let mut reviews = store.load_reviews()?;
    if let Err(error) = run.operation.begin_ai_attempt(now) {
        run.error = Some(error.clone());
        replace_run(&mut reviews, &run);
        store.save_reviews(&reviews)?;
        return Err(error);
    }
    run.phase = "Preparing immutable review context".into();
    run.error = None;
    replace_run(&mut reviews, &run);
    store.save_reviews(&reviews)?;
    Ok(run)
}

pub(crate) fn launch_worker(app: &tauri::AppHandle, run: ReviewRun, cancelled: Arc<AtomicBool>) {
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
                if generations.get(&run.job.account_id).copied().unwrap_or(0) != generation {
                    return Err(Failure::permanent(
                        "Review cancelled or GitHub account connection changed.",
                    ));
                }
                if host.quitting.load(Ordering::SeqCst) {
                    return Err(Failure::cancelled());
                }
                auth.account_session_allowed(&run.job.account_id)
                    .map_err(Failure::from)?;
                super::validate_execution_selection(&store, &run)?;
                Ok(result)
            });
            complete(&store, &run.operation.id, outcome, now_seconds()?)
        })();
        if let Err(error) = saved {
            crate::report(&app, error);
            if let Err(error) = host.ai.persistence_failed(&run.operation.id) {
                crate::report(&app, error);
            }
            return;
        }
        crate::capacity::refill(
            &app,
            &crate::capacity::WorkId {
                kind: crate::capacity::Kind::Normal,
                id: run.key,
            },
            &run.operation.id,
        );
    });
}

pub(crate) fn complete(
    store: &crate::storage::Store,
    id: &str,
    outcome: Result<super::ReviewResult, Failure>,
    now: i64,
) -> Result<(), String> {
    let mut reviews = store.load_reviews()?;
    let current = reviews
        .iter_mut()
        .find(|r| r.operation.id == id)
        .ok_or("Review operation disappeared.")?;
    let outcome = outcome.and_then(|result| {
        if let Some(contexts) = &current.feedback_context {
            crate::feedback::validate_review(
                &result.output,
                contexts,
                &current.selection.agent.id,
            )?;
        }
        Ok(result)
    });
    if current.operation.state != OperationState::Running {
        return Ok(());
    }
    if crate::capacity::interrupted(&mut current.operation, outcome.as_ref().err(), now) {
        current.result = None;
        current.phase = "Waiting after intentional interruption".into();
        current.error = None;
    } else {
        match outcome {
            Ok(result) => {
                current.operation.ai_attempt = None;
                current.operation.state = OperationState::Completed;
                current.operation.failure = None;
                current.operation.next_attempt_at = None;
                current.phase = "Automated review complete; final human review required".into();
                current.result = Some(result);
                current.error = None;
            }
            Err(error) => {
                current.operation.fail(&error.monitoring(), now);
                current.phase = "Review stopped".into();
                current.error = Some(error.message);
            }
        }
    }
    store.save_reviews(&reviews)
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
        return Err(Failure::cancelled());
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
    if run.operation.operation_type == "primary_final_review" {
        crate::actions::host::validate_execution(&store, run)
    } else {
        super::validate_execution_selection(&store, run)
    }
}

fn remote_gate(
    app: &tauri::AppHandle,
    run: &ReviewRun,
) -> Result<crate::github::metadata::PullRequest, Failure> {
    local_gate(app, run)?;
    if run.operation.operation_type == "primary_final_review" {
        crate::actions::host::remote_gate(app, run)?;
    }
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
    Ok(pull)
}

pub(crate) async fn execute(
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
            return Err(Failure::cancelled());
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
        let interrupted = Arc::new(AtomicBool::new(false));
        let saw_interruption = interrupted.clone();
        let client = client.guarded(Arc::new(move || {
            preparation_gate().map_err(|error| {
                if error.cancelled {
                    saw_interruption.store(true, Ordering::SeqCst);
                }
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
        let context = client
            .review_context(
                &repo,
                preparation_run.job.number,
                &preparation_run.job.head_sha,
            )
            .map_err(|error| {
                if error == crate::github::ConnectionError::Configuration
                    && interrupted.load(Ordering::SeqCst)
                {
                    Failure::cancelled()
                } else {
                    Failure::from(error)
                }
            })?;
        Ok::<_, Failure>((context, Arc::new(client)))
    });
    // Join blocking preparation before releasing its capacity/workspace ownership.
    let (context, client) = prepared
        .await
        .map_err(|_| Failure::permanent("Review preparation could not finish."))??;
    gate()?;
    if cancelled.load(Ordering::SeqCst) {
        return Err(Failure::cancelled());
    }
    {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Review storage unavailable."))?;
        if run.operation.operation_type == "primary_final_review" {
            crate::actions::host::phase(
                &store,
                &run,
                "Verifying restricted runtime for primary final full review",
            )
            .map_err(Failure::permanent)?;
        } else {
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
    }
    let remote_app = app.clone();
    let remote_run = run.clone();
    let expected_base = context.pull.base_sha.clone();
    let send_base = expected_base.clone();
    let request = runtime::Request {
        task: runtime::FullReview {
            feedback: run.feedback_context.clone().unwrap_or_default(),
            owner_agent_id: run.selection.agent.id.clone(),
            final_context: if run.operation.operation_type == "primary_final_review" {
                let host = app.state::<Host>();
                let store = host
                    .store
                    .lock()
                    .map_err(|_| Failure::permanent("Final storage unavailable."))?;
                Some(
                    crate::actions::host::prompt_context(&store, &run.key)
                        .map_err(Failure::permanent)?,
                )
            } else {
                None
            },
        },
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
    let mut result = integration.review(request, cancelled, deadline).await?;
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
    result.reviewed_base_sha = Some(expected_base);
    Ok((result, generation))
}

#[tauri::command]
pub(crate) async fn start_review(
    app: tauri::AppHandle,
    candidate_key: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err("PR Sniper is quitting.".into());
        }
        {
            let store = host
                .store
                .lock()
                .map_err(|_| "Review storage unavailable.")?;
            request(&store, &candidate_key, true, now_seconds()?)?;
        }
        crate::capacity::Coordinator::pump(&app)
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
        let store = host
            .store
            .lock()
            .map_err(|_| "Review storage unavailable.")?;
        let mut reviews = store.load_reviews()?;
        let run = reviews
            .iter_mut()
            .find(|r| r.operation.id == operation_id)
            .ok_or("Review operation disappeared.")?;
        if !matches!(
            run.operation.state,
            OperationState::Running | OperationState::Queued | OperationState::Interrupted
        ) {
            return Err("This review is not running or waiting.".into());
        }
        run.operation.fail(
            &Failure::permanent("Review cancelled by user.").monitoring(),
            now_seconds()?,
        );
        run.error = Some("Review cancelled by user.".into());
        run.phase = "Cancelled".into();
        store.save_reviews(&reviews)?;
        host.ai.cancel(&operation_id)
    })
    .await
    .map_err(|_| "Review cancellation could not finish.".to_string())?
}

#[cfg(test)]
mod tests;
