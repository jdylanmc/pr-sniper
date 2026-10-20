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
    let states = store.load_pr_states()?;
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
            // A quietly ended run is history: revived work starts a new operation.
            let run = reviews
                .iter()
                .rev()
                .find(|r| r.key == key)
                .filter(|r| !r.operation.ended_superseded())
                .cloned();
            result.push(Candidate {
                agent_name: run
                    .as_ref()
                    .map(|r| r.selection.agent.name.clone())
                    .unwrap_or(agent_name),
                planned_selection: selection.as_ref().ok().cloned(),
                run,
                key,
                assignment_id: assignment.id.clone(),
                blocked: selection.err().or_else(|| {
                    states
                        .get(&crate::retention::Binding::job(job))
                        .and_then(|r| r.external_wait.as_ref())
                        .filter(|w| crate::exits::consume::state_seq(job) <= w.state_seq)
                        .map(|_| "Waiting for external prerequisite".into())
                }),
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
        Some(mut run)
            if run.mode == super::RunMode::Packet
                && matches!(
                    run.operation.state,
                    OperationState::Queued | OperationState::Interrupted
                ) =>
        {
            // The heartbeat raises only the queue job's seq; a reused run reads that state.
            if let (Some(work), Some(fresh)) = (run.job.work.as_mut(), candidate.job.work.as_ref())
            {
                work.state_seq = work.state_seq.max(fresh.state_seq);
            }
            run
        }
        Some(_) if !manual => return Err("Manual retry is required.".into()),
        _ => ReviewRun {
            mode: super::RunMode::Packet,
            packet: None,
            failed_duck: None,
            attempt_ducks: Vec::new(),
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
    if run
        .selection
        .configuration
        .as_ref()
        .is_some_and(|c| c.rubber_duck.is_some())
    {
        let window = store.load_settings()?.rubber_duck.primary_window_minutes;
        run.operation.retry_deadline = run
            .operation
            .retry_deadline
            .max(now.saturating_add(i64::from(window) * 60));
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
        let ducks: runtime::DuckLog = Default::default();
        let outcome = execute(app.clone(), run.clone(), cancelled, ducks.clone()).await;
        let ducks = ducks.lock().map(|d| d.clone()).unwrap_or_default();
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
            // A moved state keeps no duck evidence: its verdicts are gone with it.
            if outcome.as_ref().is_err_and(|error| !moved(error)) {
                record_attempt_ducks(&store, &run.operation.id, ducks)?;
            }
            complete(&store, &run.operation.id, outcome, now_seconds()?)?;
            let diagnostic = store.load_reviews().and_then(|reviews| {
                let current = reviews
                    .iter()
                    .find(|review| review.operation.id == run.operation.id)
                    .ok_or("Review diagnostic operation disappeared.")?;
                store.record_attempt_decision(
                    &current.operation.id,
                    super::diagnostics::retry_event(&current.operation),
                )
            });
            if let Err(error) = diagnostic {
                crate::report(&app, error);
            }
            Ok::<_, String>(())
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
    if current.operation.state != OperationState::Running {
        return Ok(());
    }
    let connectivity = store.load_connectivity()?;
    let offline = connectivity
        .offline(&crate::connectivity::scopes(current))
        .is_some();
    let suspended = current.operation.offline_since.is_some()
        || current.operation.interruption == Some(crate::capacity::Interruption::Offline);
    let suspend = (offline || suspended)
        && outcome.as_ref().err().is_none_or(|e| {
            e.cancelled
                || (connectivity
                    .offline(&[crate::backpressure::Scope::of(
                        e.origin,
                        &current.job.account_id,
                        current
                            .selection
                            .agent
                            .ai_account
                            .as_ref()
                            .map(|a| a.account_id.as_str()),
                    )])
                    .is_some()
                    && matches!(
                        e.kind,
                        monitoring::OperationFailure::Network
                            | monitoring::OperationFailure::Timeout
                    ))
        });
    if suspend {
        let since = connectivity
            .offline(&crate::connectivity::scopes(current))
            .and_then(|s| s.offline_since)
            .or(current.operation.offline_since)
            .unwrap_or(now);
        crate::connectivity::sync_operation(store, &mut current.operation, Some(since), now);
        current.operation.suspend_offline(now);
        current.result = None;
        current.attempt_ducks.clear();
        current.phase = "Offline".into();
        current.error = None;
    } else if crate::capacity::interrupted(&mut current.operation, outcome.as_ref().err(), now) {
        current.result = None;
        current.phase = "Waiting after intentional interruption".into();
        current.error = None;
    } else if let Some(error) = outcome
        .as_ref()
        .err()
        .filter(|e| !e.cancelled && e.kind == monitoring::OperationFailure::Superseded)
    {
        let wait = error.resume.as_deref().map(|resume| match resume {
            super::Resume::NextPoll => None,
            super::Resume::NewBase(base) => Some(base.clone()),
        });
        current.operation.requeue_superseded(
            now,
            wait.map(|base| monitoring::Supersession { at: now, base }),
        );
        current.attempt_ducks.clear();
        current.result = None;
        current.phase = "Waiting for current PR state".into();
        current.error = None;
    } else if crate::backpressure::defer(
        store,
        &mut current.operation,
        outcome.as_ref().err(),
        current
            .selection
            .agent
            .ai_account
            .as_ref()
            .map(|account| account.account_id.as_str()),
        now,
    )? {
        current.result = None;
        current.phase = "Waiting for rate limit".into();
        current.error = None;
    } else {
        let outcome = outcome.and_then(|result| {
            match crate::exits::consume::honour_local(store, current, &result)
                .map_err(Failure::permanent)?
            {
                crate::pr_state::Consumed::Consumed => Ok(result),
                crate::pr_state::Consumed::Moved => Err(Failure::tossed(
                    "PR state moved during the run. Result discarded.",
                )),
            }
        });
        match outcome {
            Ok(result) => {
                crate::backpressure::run_succeeded(
                    store,
                    &current.operation.account_id,
                    current
                        .selection
                        .agent
                        .ai_account
                        .as_ref()
                        .map(|account| account.account_id.as_str()),
                );
                current.failed_duck = None;
                current.attempt_ducks.clear();
                current.operation.ai_attempt = None;
                current.operation.state = OperationState::Completed;
                current.operation.failure = None;
                current.operation.next_attempt_at = None;
                current.phase = if matches!(
                    result.output.exit,
                    Some(crate::exits::ExitOutcome::Wait { .. })
                ) {
                    "Waiting for external prerequisite"
                } else {
                    "Automated review complete; final human review required"
                }
                .into();
                current.result = Some(result);
                current.error = None;
            }
            Err(error) => {
                current.failed_duck = error
                    .duck_setup
                    .then(|| {
                        current
                            .selection
                            .configuration
                            .as_ref()?
                            .rubber_duck
                            .as_ref()
                            .map(|d| d.agent.name.clone())
                    })
                    .flatten();
                if moved(&error) {
                    current.attempt_ducks.clear();
                }
                if error.kind == monitoring::OperationFailure::Tossed {
                    crate::pr_state::toss(store, &crate::retention::Binding::job(&current.job))?;
                }
                current.operation.fail(&error.monitoring(), now);
                current.phase = "Review stopped".into();
                current.error = Some(error.message);
            }
        }
    }
    store.save_reviews(&reviews)
}

/// The PR state moved under the attempt (superseded before the run, tossed after).
fn moved(error: &Failure) -> bool {
    !error.cancelled
        && matches!(
            error.kind,
            monitoring::OperationFailure::Tossed | monitoring::OperationFailure::Superseded
        )
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
    super::validate_execution_selection(&store, run)
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
    pull_gate(&settings, &run.job, &pull)?;
    Ok(pull)
}

/// A moved head, closed or unadmitted PR, or changed assignment supersedes the
/// attempt: no budget spent, no park. The next scan retargets or retires it.
pub(crate) fn pull_gate(
    settings: &crate::storage::Settings,
    job: &QueueJob,
    pull: &crate::github::metadata::PullRequest,
) -> Result<(), Failure> {
    monitoring::check_review_policy(settings, job, Some(pull))
        .map(|_| ())
        .map_err(Failure::policy)
}

/// A new base is a new revision: superseded, never parked. Before inference it
/// waits for the next poll; after it, for a poll that observed a base other than
/// `reviewed`, so each base gets at most one AI attempt.
pub(crate) fn base_changed(reviewed: Option<&str>) -> Failure {
    match reviewed {
        Some(base) => {
            Failure::superseded("Pull request base changed during review; result is stale.")
                .after(super::Resume::NewBase(base.into()))
        }
        None => Failure::superseded("Pull request base changed; review context is stale.")
            .after(super::Resume::NextPoll),
    }
}

/// The fresh snapshot read before any setup: the same eligibility the remote gate
/// applies, so a stale attempt never builds context, a packet or a session.
pub(crate) fn snapshot_gate(
    settings: &crate::storage::Settings,
    job: &QueueJob,
    snapshot: &crate::github::state::PrSnapshot,
) -> Result<(), Failure> {
    let review_drafts = settings
        .repositories
        .iter()
        .find(|r| r.id == job.configuration_id)
        .is_some_and(|r| r.review_drafts);
    let stale = if snapshot.state != "OPEN" {
        Some("The PR is no longer open.")
    } else if snapshot.head != job.head_sha {
        Some("The PR head moved since the work was queued.")
    } else if snapshot.draft && !review_drafts {
        Some("The PR is a draft and Review drafts is off.")
    } else {
        None
    };
    stale.map_or(Ok(()), |message| {
        Err(Failure::superseded(message).after(super::Resume::NextPoll))
    })
}

pub(crate) async fn execute(
    app: tauri::AppHandle,
    run: ReviewRun,
    cancelled: Arc<AtomicBool>,
    ducks: runtime::DuckLog,
) -> Result<(super::ReviewResult, u64), Failure> {
    let diagnostics =
        super::diagnostics::native(&app, &run.job, &run.operation, &run.selection.agent.model);
    diagnostics.emit(crate::storage::diagnostics::Event::Phase {
        phase: crate::storage::diagnostics::Phase::PreparingContext,
    });
    let result = execute_attempt(
        app.clone(),
        run.clone(),
        cancelled,
        diagnostics.clone(),
        ducks,
    )
    .await;
    if let Err(error) = &result {
        if let Err(error) = crate::connectivity::confirm_failure(&app, &run, error).await {
            crate::report(&app, error);
        }
    }
    diagnostics.finish(
        crate::storage::diagnostics::CompletionStage::Attempt,
        result.as_ref().err(),
    );
    result
}

async fn execute_attempt(
    app: tauri::AppHandle,
    run: ReviewRun,
    cancelled: Arc<AtomicBool>,
    diagnostics: Arc<super::diagnostics::Trace>,
    ducks: runtime::DuckLog,
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
        let (identity, client) =
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
        // Packet runs read the PR fresh at consume time. A head that moved since the
        // work was queued is superseded (free); any other change is simply the new state.
        if preparation_run.mode != super::RunMode::Packet {
            return Err(Failure::permanent(super::LEGACY_RETIRED));
        }
        let snapshot = client
            .pr_snapshot(&repo, preparation_run.job.number)
            .map_err(Failure::from)?;
        {
            let settings = host
                .store
                .lock()
                .map_err(|_| Failure::permanent("Review storage unavailable."))?
                .load_settings()
                .map_err(Failure::permanent)?;
            snapshot_gate(&settings, &preparation_run.job, &snapshot)?;
        }
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
        let client = Arc::new(client);
        let packet = {
            let write_capability = client
                .connect(&repo.name, Some(&preparation_run.job.account_id))
                .map_err(Failure::from)?
                .capabilities
                .comment
                == crate::github::provider::CommentCapability::Available;
            let (teammates, iterations, last_consumed_at, ownership, corrections) = {
                let store = host
                    .store
                    .lock()
                    .map_err(|_| Failure::permanent("Review storage unavailable."))?;
                (
                    super::teammates::entries(&store, &preparation_run)?,
                    crate::exit_writes::known_iterations(&store, &preparation_run.job)
                        .unwrap_or_default(),
                    store
                        .load_pr_states()
                        .map_err(Failure::permanent)?
                        .get(&crate::retention::Binding::job(&preparation_run.job))
                        .and_then(|r| r.consumed_at),
                    super::findings::proofs(
                        &store,
                        &preparation_run.job,
                        &preparation_run.assignment_id,
                    )
                    .map_err(Failure::permanent)?,
                    super::findings::corrections(
                        &store
                            .load_feedback()
                            .map_err(Failure::permanent)?
                            .exit_writes,
                        &preparation_run.job,
                        &preparation_run.assignment_id,
                    ),
                )
            };
            let mut prepared = super::packet_run::prepare(
                &client,
                &repo,
                &preparation_run,
                &context,
                &snapshot,
                super::packet_run::Caller {
                    acting_login: &identity.login,
                    write_capability,
                    teammates,
                    iterations: &iterations,
                    last_consumed_at,
                    ownership,
                },
            )?;
            Arc::make_mut(&mut prepared.setup.facts).corrections = corrections;
            prepared
        };
        Ok::<_, Failure>((context, client, packet))
    });
    // Join blocking preparation before releasing its capacity/workspace ownership.
    let (context, client, packet) = prepared
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
        let mut reviews = store.load_reviews().map_err(Failure::permanent)?;
        let current = reviews
            .iter_mut()
            .find(|r| r.operation.id == run.operation.id)
            .ok_or_else(|| Failure::permanent("Review operation disappeared."))?;
        if current.operation.state != OperationState::Running {
            return Err(Failure::permanent("Review is no longer running."));
        }
        current.phase = "Verifying restricted Copilot runtime and reviewing".into();
        current.packet = Some(packet.packet.record.clone());
        store.save_reviews(&reviews).map_err(Failure::permanent)?;
    }
    let remote_app = app.clone();
    let remote_run = run.clone();
    let expected_base = context.pull.base_sha.clone();
    let send_base = expected_base.clone();
    let before_send: runtime::Gate = Arc::new(move || {
        let pull = remote_gate(&remote_app, &remote_run)?;
        if pull.base_sha != send_base {
            return Err(base_changed(None));
        }
        Ok(())
    });
    let duck = duck_setup(&app, &run, packet.duck, ducks)?;
    let integration = app.state::<Host>().copilot.clone();
    let final_packet = packet.packet.record.clone();
    let client_after = client.clone();
    let request = runtime::Request {
        inactivity: crate::storage::review_inactivity_duration(
            app.state::<Host>()
                .store
                .lock()
                .map_err(|_| Failure::permanent("Review storage unavailable."))?
                .load_settings()
                .map_err(Failure::permanent)?
                .review_inactivity_minutes,
        )
        .map_err(Failure::permanent)?,
        exits: packet.setup,
        diagnostics: Some(diagnostics.clone()),
        task: runtime::ExitReview {
            packet_text: packet.packet.text,
        },
        context,
        client,
        repository_name: run.job.repository_name.clone(),
        selection: run.selection.clone(),
        before_send,
        local_gate: gate.clone(),
        duck,
    };
    let mut result = integration.review(request, cancelled, deadline).await?;
    diagnostics.emit(crate::storage::diagnostics::Event::Phase {
        phase: crate::storage::diagnostics::Phase::CheckingFinalEligibility,
    });
    let final_app = app.clone();
    let final_run = run.clone();
    let pull = tauri::async_runtime::spawn_blocking(move || remote_gate(&final_app, &final_run))
        .await
        .map_err(|_| Failure::permanent("Final eligibility check could not finish."))??;
    if pull.base_sha != expected_base {
        return Err(base_changed(Some(&expected_base)));
    }
    // Local exits require fresh state; Wait may absorb only still-pending CI progress.
    if matches!(
        &result.output.exit,
        Some(
            crate::exits::ExitOutcome::ReadyForHuman { .. }
                | crate::exits::ExitOutcome::Wait { .. }
        )
    ) {
        let repo = RemoteRepository {
            id: run.job.repository_id.clone(),
            name: run.job.repository_name.clone(),
        };
        let number = run.job.number;
        result = tauri::async_runtime::spawn_blocking(move || {
            finalize_local_exit(&client_after, &repo, number, &final_packet, &mut result)?;
            Ok::<_, Failure>(result)
        })
        .await
        .map_err(|_| Failure::permanent("State check could not finish."))??;
    }
    gate()?;
    result.reviewed_base_sha = Some(expected_base);
    Ok((result, generation))
}

pub(crate) fn finalize_local_exit<T: crate::github::threads::QueryTransport>(
    client: &crate::github::provider::GithubClient<T>,
    repo: &RemoteRepository,
    number: u64,
    packet: &crate::packet::PacketRecord,
    result: &mut super::ReviewResult,
) -> Result<(), Failure> {
    let read_hash = packet
        .state_hash
        .as_deref()
        .ok_or_else(|| Failure::permanent("The packet record has no state hash."))?;
    let fresh = client
        .pr_snapshot(repo, number)
        .map_err(Failure::from)?
        .normalize();
    let valid = if let Some(crate::exits::ExitOutcome::Wait { reason }) = &result.output.exit {
        packet.wait_snapshot.as_ref().is_some_and(|read| {
            read.hash() == read_hash
                && crate::pr_state::ExternalWait::pending_snapshot(*reason, read, &fresh)
        })
    } else {
        fresh.hash() == read_hash
    };
    if !valid {
        return Err(Failure::tossed(
            "PR state moved during the run. Result discarded.",
        ));
    }
    if matches!(
        result.output.exit,
        Some(crate::exits::ExitOutcome::Wait { .. })
    ) {
        crate::review::findings::action_clearance(&result.finding_dispositions)
            .map_err(Failure::permanent)?;
        result.wait_snapshot = Some(fresh);
    }
    Ok(())
}

/// The primary's Rubber Duck for this attempt: the job-start snapshot, current limits and
/// the duck account's rate-limit gate. The credential is fetched with the primary's.
fn duck_setup(
    app: &tauri::AppHandle,
    run: &ReviewRun,
    prep: Option<runtime::DuckPrep>,
    log: runtime::DuckLog,
) -> Result<Option<runtime::DuckSetup>, Failure> {
    let Some(prep) = prep else {
        return Ok(None);
    };
    let snapshot = run
        .selection
        .configuration
        .as_ref()
        .and_then(|c| c.rubber_duck.clone())
        .ok_or_else(|| Failure::permanent("Rubber duck snapshot is missing.").duck_setup())?;
    let host = app.state::<Host>();
    let limits = host
        .store
        .lock()
        .map_err(|_| Failure::permanent("Review storage unavailable."))?
        .load_settings()
        .map_err(Failure::permanent)?
        .rubber_duck;
    let scope = snapshot
        .agent
        .ai_account
        .as_ref()
        .map(|a| crate::backpressure::Scope::copilot(&a.account_id));
    let (gate_app, gate_scope) = (app.clone(), scope.clone());
    let gated: runtime::Gated = Arc::new(move || {
        let Some(scope) = &gate_scope else {
            return false;
        };
        let host = gate_app.state::<Host>();
        let Ok(store) = host.store.lock() else {
            return false;
        };
        let now = now_seconds().unwrap_or(0);
        store
            .load_backpressure()
            .is_ok_and(|gates| gates.until(scope, now).is_some())
    });
    let (pace_app, seed) = (app.clone(), run.operation.id.clone());
    let rate_limited: runtime::Pacing = Arc::new(move |retry_after| {
        let Some(scope) = scope.clone() else {
            return;
        };
        let host = pace_app.state::<Host>();
        let recorded = host.store.lock().map(|store| {
            let now = now_seconds().unwrap_or(0);
            crate::backpressure::record(&store, scope, retry_after, &seed, now)
        });
        if let Ok(Err(error)) = recorded {
            crate::report(&pace_app, error);
        }
    });
    Ok(Some(runtime::DuckSetup {
        prep,
        selection: super::Selection {
            agent: snapshot.agent,
            policy: run.selection.policy.clone(),
            doctrine: None,
            preset: None,
            configuration: None,
        },
        limits,
        launch: runtime::DuckLaunch::Pending,
        rate_limited,
        gated,
        account: None,
        log,
    }))
}

/// Keeps the duck calls of an attempt that ended without a result, replacing those of
/// an earlier attempt. Evidence only; a completed result carries its own.
pub(crate) fn record_attempt_ducks(
    store: &crate::storage::Store,
    id: &str,
    ducks: Vec<crate::review::duck::DuckCall>,
) -> Result<(), String> {
    let mut reviews = store.load_reviews()?;
    let Some(current) = reviews.iter_mut().find(|r| r.operation.id == id) else {
        return Ok(());
    };
    if current.operation.state != OperationState::Running || current.attempt_ducks == ducks {
        return Ok(());
    }
    current.attempt_ducks = ducks;
    store.save_reviews(&reviews)
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
pub(crate) async fn retry_packet_state(
    app: tauri::AppHandle,
    item_id: String,
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
            crate::exits::consume::retry_parked(&store, &item_id, now_seconds()?)?;
        }
        crate::capacity::Coordinator::pump(&app)
    })
    .await
    .map_err(|_| "Retry could not finish.".to_string())?
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
mod stale_revision_tests;
#[cfg(test)]
mod tests;
