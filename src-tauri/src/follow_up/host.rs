use super::*;
use crate::{
    github::{
        self,
        http::HttpTransport,
        metadata::PullRequest,
        provider::{CommentCapability, GithubClient, RemoteRepository},
        review::GuardedTransport,
    },
    monitoring::{self, PollTicket},
    now_seconds,
    publication::{self, GatePermissions, WriteFailure},
    review::{runtime, Selection},
    Host,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Manager;

pub(crate) struct Observed {
    origin: Publication,
    thread: Thread,
}

pub(crate) fn scan(
    app: &tauri::AppHandle,
    ticket: &PollTicket,
    pulls: &[PullRequest],
) -> Result<Vec<Observed>, github::ConnectionError> {
    let origins = {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| github::ConnectionError::Configuration)?;
        super::polling_origins(&store, ticket, pulls)
            .map_err(|_| github::ConnectionError::Configuration)?
    };
    if origins.is_empty() {
        return Ok(vec![]);
    }
    let guard_app = app.clone();
    let guard_ticket = ticket.clone();
    let guard = Arc::new(move || {
        let host = guard_app.state::<Host>();
        connection_gate(
            &host,
            &guard_ticket.provider_account_id,
            guard_ticket.account_generation,
        )
        .map_err(|_| github::ConnectionError::Configuration)?;
        let store = host
            .store
            .lock()
            .map_err(|_| github::ConnectionError::Configuration)?;
        let settings = store
            .load_settings()
            .map_err(|_| github::ConnectionError::Configuration)?;
        let context =
            monitoring::Monitor::activation_context(&settings, &guard_ticket.repository_id)?;
        if context.account_id != guard_ticket.provider_account_id
            || context.provider_repository_id != guard_ticket.provider_repository_id
            || context.trigger_policy != guard_ticket.trigger_policy
        {
            return Err(github::ConnectionError::Configuration);
        }
        let health = host
            .monitor
            .lock()
            .map_err(|_| github::ConnectionError::Configuration)?
            .snapshot();
        let operation = health
            .iter()
            .find(|h| h.repository_id == guard_ticket.repository_id)
            .and_then(|h| h.operation.as_ref())
            .ok_or(github::ConnectionError::Configuration)?;
        if now_seconds().map_err(|_| github::ConnectionError::Configuration)?
            >= operation.retry_deadline
        {
            return Err(github::ConnectionError::Timeout);
        }
        Ok(())
    });
    guard()?;
    let (_, client) = crate::github_session(&app.state::<Host>(), &ticket.provider_account_id)?;
    let client = client.guarded(guard);
    let mut result = Vec::new();
    for origin in origins {
        for thread in client.owned_threads(&origin)? {
            if !thread.resolved
                && thread.can_reply
                && thread
                    .latest_external(&origin.review.job.account_id)
                    .is_some()
            {
                result.push(Observed {
                    origin: origin.clone(),
                    thread,
                });
            }
        }
    }
    Ok(result)
}

pub(crate) fn admit(
    app: &tauri::AppHandle,
    ticket: &PollTicket,
    observations: Vec<Observed>,
) -> Result<(), String> {
    let host = app.state::<Host>();
    let generations = host
        .github_generations
        .lock()
        .map_err(|_| "GitHub coordination unavailable.")?;
    if generations
        .get(&ticket.provider_account_id)
        .copied()
        .unwrap_or(0)
        != ticket.account_generation
    {
        return Err(
            "Thread observations were discarded because the GitHub connection changed.".into(),
        );
    }
    host.github_auth
        .lock()
        .map_err(|_| "GitHub account unavailable.")?
        .account_session_allowed(&ticket.provider_account_id)
        .map_err(|_| "Thread account is no longer available.")?;
    let store = host
        .store
        .lock()
        .map_err(|_| "Thread storage unavailable.")?;
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    let mut runs = store.load_follow_ups()?;
    let before = runs.len();
    for observation in observations {
        let run = FollowUp::new(&observation.origin, observation.thread)?;
        if jobs.iter().any(|job| {
            run.review.matches_job(job) && monitoring::review_policy(&settings, job, None).is_ok()
        }) && super::admit(&mut runs, &observation.origin, run.thread)?
        {
            let work = runs
                .last_mut()
                .ok_or("The admitted follow-up was not retained.")?;
            work.enqueue_order = Some(store.allocate_enqueue_order()?);
            work.enqueued_at = Some(now_seconds()?);
        }
    }
    if runs.len() != before {
        store.save_follow_ups(&runs)?;
    }
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct Candidate {
    pub(crate) run: FollowUp,
    pub(crate) blocked: Option<String>,
    pub(crate) automatic_start: bool,
    pub(crate) automatic_publication: bool,
    pub(crate) human_gate: bool,
}

pub(crate) fn candidates(store: &Store) -> Result<Vec<Candidate>, String> {
    let runs = store.load_follow_ups()?;
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    Ok(runs
        .iter()
        .map(|run| {
            let policy = jobs
                .iter()
                .find(|j| run.review.matches_job(j))
                .ok_or("This revision is no longer detected.".to_string())
                .and_then(|job| {
                    let selection = Selection::resolve(&settings, job, &run.review.assignment_id)?;
                    let mut review = run.review.clone();
                    if run.result.is_none() && run.publication.is_none() {
                        review.selection = selection.clone();
                    }
                    publication::automatic_policy(&settings, &review, job)?;
                    Ok(selection)
                });
            let human_gate = runs.iter().any(|other| {
                other.thread.id == run.thread.id
                    && other.review.job.account_id == run.review.job.account_id
                    && other.phase == Phase::HumanInputRequired
            });
            Candidate {
                run: run.clone(),
                blocked: policy.as_ref().err().cloned(),
                automatic_start: policy
                    .as_ref()
                    .is_ok_and(|s| s.policy.automatic_agent_start)
                    && !human_gate,
                automatic_publication: policy
                    .as_ref()
                    .is_ok_and(|s| s.policy.automatic_comment_publication),
                human_gate,
            }
        })
        .collect())
}

#[derive(Default)]
pub(crate) struct Coordinator {
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

impl Coordinator {
    pub(crate) fn finished(&self) -> bool {
        self.active.lock().is_ok_and(|v| v.is_none())
    }
    pub(crate) fn cancel_all(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some((_, cancelled)) = active.as_ref() {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
    }
    pub(crate) fn pump(app: &tauri::AppHandle) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) || !host.follow_ups.finished() {
            return Ok(());
        }
        let next = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Thread storage unavailable.")?;
            let now = now_seconds()?;
            candidates(&store)?.into_iter().find_map(|candidate| {
                let run = &candidate.run;
                let due = |op: &JobOperation| {
                    matches!(
                        op.state,
                        OperationState::Queued | OperationState::Interrupted
                    ) && op.next_attempt_at.is_some_and(|n| n <= now)
                };
                if run.publication.as_ref().is_some_and(due) {
                    return Some((run.id.clone(), true));
                }
                if run.analysis.as_ref().is_some_and(due) {
                    return Some((run.id.clone(), false));
                }
                if candidate.blocked.is_some() || run.cancelled {
                    return None;
                }
                if run.analysis.is_none() && candidate.automatic_start {
                    return Some((run.id.clone(), false));
                }
                if run.phase == Phase::WaitingPublication
                    && run.publication.is_none()
                    && candidate.automatic_publication
                {
                    return Some((run.id.clone(), true));
                }
                None
            })
        };
        if let Some((id, publish)) = next {
            Self::launch(app, &id, publish, false)?;
        }
        Ok(())
    }

    fn launch(app: &tauri::AppHandle, id: &str, publish: bool, manual: bool) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err("PR Sniper is quitting.".into());
        }
        let mut active = host
            .follow_ups
            .active
            .lock()
            .map_err(|_| "Thread coordination unavailable.")?;
        if active.is_some() {
            return Err("Another thread follow-up is running.".into());
        }
        let generations = host
            .github_generations
            .lock()
            .map_err(|_| "GitHub coordination unavailable.")?;
        let mut run = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Thread storage unavailable.")?;
            let candidate = candidates(&store)?
                .into_iter()
                .find(|c| c.run.id == id)
                .ok_or("Thread follow-up disappeared.")?;
            let mut run = candidate.run;
            let now = now_seconds()?;
            if publish {
                if run
                    .result
                    .as_ref()
                    .is_none_or(|r| r.output.decision != ReplyDecision::Reply)
                {
                    return Err("This follow-up has no validated reply.".into());
                }
                if run
                    .publication
                    .as_ref()
                    .is_some_and(|p| p.state == OperationState::Completed)
                {
                    return Err("The reply was already published.".into());
                }
                if run.publication.is_none() || manual {
                    if !candidate.automatic_publication && !manual {
                        return Err("Explicit reply publication confirmation is required.".into());
                    }
                    if let Some(previous) = run.publication.take() {
                        run.history.push(previous);
                    }
                    let mut operation = run.operation("thread_reply", now);
                    operation.confirmed_receipt = run.receipt.clone();
                    if run.uncertain {
                        operation.attempted_mutation = Some("thread_reply".into());
                    }
                    run.publication = Some(operation);
                    run.automatic_publication = candidate.automatic_publication;
                    run.confirmed = manual;
                    if run.body.is_none() {
                        run.body = Some(run.reply_body().map_err(|e| e.message)?);
                    }
                }
            } else {
                if run.result.is_some() || run.publication.is_some() {
                    return Err("This follow-up was already analyzed; a later external comment creates new work.".into());
                }
                if let Some(error) = candidate.blocked {
                    return Err(error);
                }
                if run.analysis.is_none() || manual {
                    if !candidate.automatic_start && !manual {
                        return Err("Explicit follow-up start is required.".into());
                    }
                    let settings = store.load_settings()?;
                    let jobs = store.load_queue()?;
                    let job = jobs
                        .iter()
                        .find(|j| run.review.matches_job(j))
                        .ok_or("The reviewed revision is no longer available.")?;
                    run.review.selection =
                        Selection::resolve(&settings, job, &run.review.assignment_id)?;
                    if let Some(previous) = run.analysis.take() {
                        run.history.push(previous);
                    }
                    run.analysis = Some(run.operation("thread_analysis", now));
                    run.manual_start = manual;
                }
            }
            if manual {
                run.cancelled = false;
            }
            run.error = None;
            save_to_store(&store, &run)?;
            run
        };
        let generation = generations
            .get(&run.review.job.account_id)
            .copied()
            .unwrap_or(0);
        drop(generations);
        let cancelled = Arc::new(AtomicBool::new(false));
        *active = Some((run.id.clone(), cancelled.clone()));
        drop(active);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if publish {
                let worker = app.clone();
                let outcome = tauri::async_runtime::spawn_blocking(move || {
                    let mut native = Native {
                        app: worker,
                        generation,
                        cancelled,
                    };
                    super::publish(&mut native, &mut run)
                })
                .await;
                match outcome {
                    Ok(Err(error)) => crate::report(&app, error.message),
                    Err(_) => {
                        crate::report(&app, "Thread publication worker could not finish.".into())
                    }
                    _ => {}
                }
            } else {
                let mut native = Native {
                    app: app.clone(),
                    generation,
                    cancelled,
                };
                if let Err(error) = analyze(&mut native, &mut run).await {
                    crate::report(&app, error.message);
                }
            }
            match app.state::<Host>().follow_ups.active.lock() {
                Ok(mut active) => *active = None,
                Err(_) => crate::report(&app, "Thread coordination unavailable.".into()),
            };
        });
        Ok(())
    }
}

fn save_to_store(store: &Store, run: &FollowUp) -> Result<(), String> {
    let mut runs = store.load_follow_ups()?;
    if let Some(previous) = runs.iter_mut().find(|r| r.id == run.id) {
        *previous = run.clone();
    } else {
        return Err("Thread follow-up disappeared.".into());
    }
    store.save_follow_ups(&runs)
}

fn connection_gate(host: &Host, account: &str, generation: u64) -> Result<(), Failure> {
    if host.quitting.load(Ordering::SeqCst) {
        return Err(Failure::permanent("Thread work interrupted by shutdown."));
    }
    if host
        .github_generations
        .lock()
        .map_err(|_| Failure::permanent("GitHub coordination unavailable."))?
        .get(account)
        .copied()
        .unwrap_or(0)
        != generation
    {
        return Err(Failure::permanent("GitHub account connection changed."));
    }
    host.github_auth
        .lock()
        .map_err(|_| Failure::permanent("GitHub account unavailable."))?
        .account_session_allowed(account)?;
    Ok(())
}

#[derive(Clone)]
struct Native {
    app: tauri::AppHandle,
    generation: u64,
    cancelled: Arc<AtomicBool>,
}

impl Native {
    fn origin(&self, run: &FollowUp) -> Result<Publication, Failure> {
        self.app
            .state::<Host>()
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?
            .load_publications()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|p| p.id == run.publication_id)
            .ok_or_else(|| Failure::permanent("The owned review receipt is unavailable."))
    }
    fn client(&self, run: &FollowUp) -> Result<GithubClient<HttpTransport>, Failure> {
        let host = self.app.state::<Host>();
        connection_gate(&host, &run.review.job.account_id, self.generation)?;
        let (identity, client) = crate::github_session(&host, &run.review.job.account_id)?;
        if identity.id != run.review.job.account_id {
            return Err(Failure::permanent("Acting account changed."));
        }
        connection_gate(&host, &run.review.job.account_id, self.generation)?;
        Ok(client)
    }
    fn read_client(
        &self,
        run: &FollowUp,
    ) -> Result<GithubClient<GuardedTransport<HttpTransport>>, Failure> {
        let native = self.clone();
        let snapshot = run.clone();
        Ok(self.client(run)?.guarded(Arc::new(move || {
            connection_gate(
                &native.app.state::<Host>(),
                &snapshot.review.job.account_id,
                native.generation,
            )
            .map_err(|_| github::ConnectionError::Configuration)?;
            let operation = snapshot
                .publication
                .as_ref()
                .or(snapshot.analysis.as_ref())
                .ok_or(github::ConnectionError::Configuration)?;
            if now_seconds().map_err(|_| github::ConnectionError::Configuration)?
                >= operation.retry_deadline
            {
                return Err(github::ConnectionError::Timeout);
            }
            Ok(())
        })))
    }
    fn local(&self, run: &FollowUp) -> Result<(), Failure> {
        connection_gate(
            &self.app.state::<Host>(),
            &run.review.job.account_id,
            self.generation,
        )?;
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(Failure::permanent("Thread follow-up cancelled."));
        }
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        let current = store
            .load_follow_ups()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|r| r.id == run.id)
            .ok_or_else(|| Failure::permanent("Thread follow-up disappeared."))?;
        if current.cancelled {
            return Err(Failure::permanent("Thread follow-up cancelled."));
        }
        let settings = store.load_settings().map_err(Failure::permanent)?;
        let jobs = store.load_queue().map_err(Failure::permanent)?;
        let job = jobs
            .iter()
            .find(|j| run.review.matches_job(j))
            .ok_or_else(|| Failure::permanent("The reviewed revision is no longer detected."))?;
        let selection = Selection::resolve(&settings, job, &run.review.assignment_id)
            .map_err(Failure::permanent)?;
        let automatic = publication::automatic_policy(&settings, &run.review, job)
            .map_err(Failure::permanent)?;
        run.check_publication_grant(&current, automatic)?;
        if run.publication.is_none()
            && (selection != run.review.selection
                || (!selection.policy.automatic_agent_start && !run.manual_start))
        {
            return Err(Failure::permanent(
                "Follow-up selection or start gate changed; explicit retry required.",
            ));
        }
        Ok(())
    }
}

impl Environment for Native {
    fn now(&self) -> Result<i64, Failure> {
        now_seconds().map_err(Failure::permanent)
    }
    fn save(&mut self, run: &mut FollowUp) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        let accepting_analysis = run.publication.is_none()
            && run.result.is_some()
            && run
                .analysis
                .as_ref()
                .is_some_and(|op| op.state == OperationState::Completed);
        let generations = accepting_analysis
            .then(|| host.github_generations.lock())
            .transpose()
            .map_err(|_| Failure::permanent("GitHub coordination unavailable."))?;
        let auth = accepting_analysis
            .then(|| host.github_auth.lock())
            .transpose()
            .map_err(|_| Failure::permanent("GitHub account unavailable."))?;
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        let current = store
            .load_follow_ups()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|r| r.id == run.id)
            .ok_or_else(|| Failure::permanent("Thread follow-up disappeared."))?;
        let failure = if accepting_analysis {
            let allowed = !host.quitting.load(Ordering::SeqCst)
                && !self.cancelled.load(Ordering::SeqCst)
                && generations.as_ref().is_some_and(|g| {
                    g.get(&run.review.job.account_id).copied().unwrap_or(0) == self.generation
                })
                && auth.as_ref().is_some_and(|a| {
                    a.account_session_allowed(&run.review.job.account_id)
                        .is_ok()
                });
            super::validate_analysis_commit(&store, run, allowed).err()
        } else {
            None
        };
        if current.cancelled {
            run.cancelled = true;
            run.confirmed = false;
            if run.publication.is_none() {
                run.result = None;
                run.phase = Phase::Stopped;
                run.error = Some("Thread follow-up cancelled.".into());
                if let Some(op) = run.analysis.as_mut() {
                    op.fail(&Failure::permanent("Cancelled.").monitoring(), self.now()?);
                }
            }
        }
        if let Some(error) = &failure {
            run.result = None;
            run.phase = Phase::Stopped;
            run.error = Some(error.message.clone());
            run.analysis
                .as_mut()
                .unwrap()
                .fail(&error.monitoring(), self.now()?);
        }
        save_to_store(&store, run).map_err(Failure::permanent)?;
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn thread(&mut self, run: &FollowUp) -> Result<Thread, Failure> {
        self.read_client(run)?
            .owned_thread(&self.origin(run)?, &run.thread.id)?
            .ok_or_else(|| Failure::permanent("The owned review thread is no longer available."))
    }
    fn gate(&mut self, run: &FollowUp, thread: &Thread) -> Result<Option<String>, Failure> {
        let client = self.read_client(run)?;
        let job = &run.review.job;
        let repo = RemoteRepository {
            id: job.repository_id.clone(),
            name: job.repository_name.clone(),
        };
        let connection = client.connect(&repo.name, Some(&job.account_id))?;
        if connection.repository.id != repo.id {
            return Err(Failure::permanent("Repository identity changed."));
        }
        let pull = client.review_pull(&repo, job.number)?;
        let local = self.local(run);
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        let settings = store.load_settings().map_err(Failure::permanent)?;
        let jobs = store.load_queue().map_err(Failure::permanent)?;
        let current_job = jobs.iter().find(|j| run.review.matches_job(j));
        let current_auto = current_job
            .and_then(|j| publication::automatic_policy(&settings, &run.review, j).ok())
            .unwrap_or(false);
        let mut monitor = host
            .monitor
            .lock()
            .map_err(|_| Failure::permanent("Monitoring unavailable."))?;
        let active = monitor
            .activation_status(&settings, &job.configuration_id)
            .active;
        let gate = publication::evaluate_review_gate(
            &settings,
            &run.review,
            current_job,
            &pull,
            GatePermissions {
                active,
                can_comment: connection.capabilities.comment == CommentCapability::Available,
                cancelled: run.cancelled || self.cancelled.load(Ordering::SeqCst),
                automatic: if run.publication.is_some() {
                    run.automatic_publication
                } else {
                    current_auto
                },
                confirmed: run.publication.is_none() || run.confirmed,
            },
        );
        if gate.requeue {
            monitor
                .request_revision_check(&store, job, self.now()?)
                .map_err(Failure::permanent)?;
        }
        if let Err(error) = local {
            return Ok(Some(error.message));
        }
        if !thread.owned_by(&self.origin_without_lock(&store, run)?) || !run.fresh_thread(thread) {
            return Ok(Some(
                "The owned thread was resolved, changed, superseded, or no longer permits replies."
                    .into(),
            ));
        }
        Ok(gate.stop)
    }
    fn reply(&mut self, run: &FollowUp) -> Result<String, WriteFailure> {
        let prepared = (|| {
            let origin = self.origin(run)?;
            let client = self.client(run)?;
            let current = self.thread(run)?;
            if let Some(receipt) = run.reconcile(&current)? {
                return Ok((client, origin, Some(receipt)));
            }
            if let Some(reason) = self.gate(run, &current)? {
                return Err(Failure::permanent(reason));
            }
            self.local(run)?;
            if self.now()? >= run.publication.as_ref().unwrap().retry_deadline {
                return Err(Failure::timeout());
            }
            Ok((client, origin, None))
        })()
        .map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        if let Some(receipt) = prepared.2 {
            return Ok(receipt);
        }
        prepared.0.reply_to_thread(
            &prepared.1,
            &run.thread
                .root()
                .map_err(|error| WriteFailure {
                    failure: error.into(),
                    uncertain: false,
                })?
                .id,
            run.body.as_deref().ok_or_else(|| WriteFailure {
                failure: Failure::permanent("Reply body unavailable."),
                uncertain: false,
            })?,
        )
    }
}

impl Native {
    fn origin_without_lock(&self, store: &Store, run: &FollowUp) -> Result<Publication, Failure> {
        store
            .load_publications()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|p| p.id == run.publication_id)
            .ok_or_else(|| Failure::permanent("Owned review receipt unavailable."))
    }
}

async fn analyze(native: &mut Native, run: &mut FollowUp) -> Result<(), Failure> {
    if let Err(error) = run
        .analysis
        .as_mut()
        .ok_or_else(|| Failure::permanent("Follow-up start was not authorized."))?
        .begin_attempt(native.now()?)
    {
        run.error = Some(error.clone());
        native.save(run)?;
        return Err(Failure::permanent(error));
    }
    run.phase = Phase::Analyzing;
    native.save(run)?;
    let result = async {
        let mut worker = native.clone();
        let snapshot = run.clone();
        let (context, client, thread) = tauri::async_runtime::spawn_blocking(move || {
            let thread = worker.thread(&snapshot)?;
            if thread
                .latest_external(&snapshot.review.job.account_id)
                .map(|c| &c.id)
                != Some(&snapshot.trigger_id)
            {
                return Err(Failure::permanent(
                    "A later external comment superseded this follow-up.",
                ));
            }
            let mut current = snapshot.clone();
            if current.manual_start {
                current.thread = thread.clone();
            }
            if let Some(reason) = worker.gate(&current, &thread)? {
                return Err(Failure::permanent(reason));
            }
            let client = worker.read_client(&current)?;
            let context = client.review_context(
                &RemoteRepository {
                    id: current.review.job.repository_id.clone(),
                    name: current.review.job.repository_name.clone(),
                },
                current.review.job.number,
                &current.review.job.head_sha,
            )?;
            if current
                .review
                .result
                .as_ref()
                .and_then(|r| r.reviewed_base_sha.as_ref())
                != Some(&context.pull.base_sha)
            {
                return Err(Failure::permanent(
                    "The reviewed base changed; wait for a new review.",
                ));
            }
            Ok((context, Arc::new(client), thread))
        })
        .await
        .map_err(|_| Failure::permanent("Follow-up preparation failed."))??;
        run.thread = thread;
        native.save(run)?;
        let local_native = native.clone();
        let local_run = run.clone();
        let before_native = native.clone();
        let before_run = run.clone();
        let request = runtime::Request {
            task: ReplyTask {
                thread: run.thread.clone(),
                trigger_id: run.trigger_id.clone(),
            },
            context,
            client,
            repository_name: run.review.job.repository_name.clone(),
            selection: run.review.selection.clone(),
            local_gate: Arc::new(move || local_native.local(&local_run)),
            before_send: Arc::new(move || {
                let mut native = before_native.clone();
                let thread = native.thread(&before_run)?;
                match native.gate(&before_run, &thread)? {
                    Some(reason) => Err(Failure::permanent(reason)),
                    None => Ok(()),
                }
            }),
        };
        let remaining = run
            .analysis
            .as_ref()
            .unwrap()
            .retry_deadline
            .saturating_sub(native.now()?);
        if remaining <= 0 {
            return Err(Failure::timeout());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(remaining as u64);
        let integration = native.app.state::<Host>().copilot.clone();
        let mut result = integration
            .review(request, native.cancelled.clone(), deadline)
            .await?;
        let mut final_native = native.clone();
        let final_run = run.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let thread = final_native.thread(&final_run)?;
            match final_native.gate(&final_run, &thread)? {
                Some(reason) => Err(Failure::permanent(reason)),
                None => Ok(()),
            }
        })
        .await
        .map_err(|_| Failure::permanent("Final follow-up verification failed."))??;
        result.reviewed_base_sha = run
            .review
            .result
            .as_ref()
            .and_then(|r| r.reviewed_base_sha.clone());
        Ok::<_, Failure>(result)
    }
    .await;
    match result {
        Ok(result) => {
            run.phase = match result.output.decision {
                ReplyDecision::Reply => Phase::WaitingPublication,
                ReplyDecision::Quiet => Phase::Quiet,
                ReplyDecision::HumanInputRequired => Phase::HumanInputRequired,
            };
            run.result = Some(result);
            let operation = run.analysis.as_mut().unwrap();
            operation.state = OperationState::Completed;
            operation.next_attempt_at = None;
            operation.failure = None;
            native.save(run)
        }
        Err(error) => {
            run.analysis
                .as_mut()
                .unwrap()
                .fail(&error.monitoring(), native.now()?);
            run.phase = Phase::Stopped;
            run.error = Some(error.message.clone());
            run.result = None;
            native.save(run)?;
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) async fn start_follow_up(
    app: tauri::AppHandle,
    id: String,
    publish: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || Coordinator::launch(&app, &id, publish, true))
        .await
        .map_err(|_| "Follow-up action could not finish.".to_string())?
}

#[tauri::command]
pub(crate) async fn cancel_follow_up(app: tauri::AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let active = host
            .follow_ups
            .active
            .lock()
            .map_err(|_| "Thread coordination unavailable.")?;
        let store = host
            .store
            .lock()
            .map_err(|_| "Thread storage unavailable.")?;
        let mut runs = store.load_follow_ups()?;
        let run = runs
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or("Thread follow-up unavailable.")?;
        if run.receipt.is_some() {
            return Err("A published reply cannot be withdrawn.".into());
        }
        run.cancelled = true;
        run.confirmed = false;
        run.error = Some("Thread follow-up cancelled.".into());
        if let Some((_, cancelled)) = active.as_ref().filter(|(active, _)| active == &id) {
            cancelled.store(true, Ordering::SeqCst);
        } else {
            let now = now_seconds()?;
            for op in [&mut run.analysis, &mut run.publication]
                .into_iter()
                .flatten()
            {
                if op.state != OperationState::Completed {
                    op.fail(&Failure::permanent("Cancelled.").monitoring(), now);
                }
            }
            run.phase = if run.uncertain {
                Phase::Unresolved
            } else {
                Phase::Stopped
            };
        }
        store.save_follow_ups(&runs)
    })
    .await
    .map_err(|_| "Follow-up cancellation failed.".to_string())?
}
