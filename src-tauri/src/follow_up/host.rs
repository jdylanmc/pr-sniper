use super::*;
use crate::{
    github::{
        http::HttpTransport,
        provider::GithubClient,
        review::GuardedTransport,
        threads::{Ownership, Provenance},
    },
    monitoring::{self, PollTicket},
    now_seconds,
    provider::{
        self, CommentCapability, PullRequest, PullRequestReader, RemoteRepository, RepositoryReader,
    },
    publication::WriteFailure,
    review::Selection,
    Host,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Manager;

pub(crate) struct Observed {
    origin: Publication,
    head: String,
    threads: Vec<Thread>,
}

#[derive(Default)]
pub(crate) struct Scan {
    pub(crate) retained: Vec<crate::retention::Observed>,
    pub(crate) feedback: Vec<Observed>,
    pub(crate) mentions: Vec<(
        crate::feedback::MentionBinding,
        Vec<crate::github::conversation::TopComment>,
    )>,
    pub(crate) threads: Vec<(crate::feedback::MentionBinding, Vec<Thread>)>,
}

pub(crate) fn scan(
    app: &tauri::AppHandle,
    ticket: &PollTicket,
    pulls: &mut [PullRequest],
    identity: &provider::Person,
) -> Result<Scan, provider::ConnectionError> {
    let (origins, tracked, retained, cursors) = {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| provider::ConnectionError::Configuration)?;
        (
            super::observation_origins(&store, ticket, pulls)
                .map_err(|_| provider::ConnectionError::Configuration)?,
            store
                .load_queue_state()
                .map_err(|_| provider::ConnectionError::Configuration)?
                .tracked,
            crate::retention::scan_origins(&store, ticket, pulls)
                .map_err(|_| provider::ConnectionError::Configuration)?,
            store
                .load_feedback()
                .map_err(|_| provider::ConnectionError::Configuration)?
                .conversation_cursors,
        )
    };
    let guard_app = app.clone();
    let guard_ticket = ticket.clone();
    let guard = Arc::new(move || {
        let host = guard_app.state::<Host>();
        connection_gate(
            &host,
            &guard_ticket.provider_account_id,
            guard_ticket.account_generation,
        )
        .map_err(|_| provider::ConnectionError::Configuration)?;
        let store = host
            .store
            .lock()
            .map_err(|_| provider::ConnectionError::Configuration)?;
        let settings = store
            .load_settings()
            .map_err(|_| provider::ConnectionError::Configuration)?;
        let context =
            monitoring::Monitor::activation_context(&settings, &guard_ticket.repository_id)?;
        if context.account_id != guard_ticket.provider_account_id
            || context.provider_repository_id != guard_ticket.provider_repository_id
            || context.trigger_policy != guard_ticket.trigger_policy
        {
            return Err(provider::ConnectionError::Configuration);
        }
        let health = host
            .monitor
            .lock()
            .map_err(|_| provider::ConnectionError::Configuration)?
            .snapshot();
        let operation = health
            .iter()
            .find(|h| h.repository_id == guard_ticket.repository_id)
            .and_then(|h| h.operation.as_ref())
            .ok_or(provider::ConnectionError::Configuration)?;
        if now_seconds().map_err(|_| provider::ConnectionError::Configuration)?
            >= operation.retry_deadline
        {
            return Err(provider::ConnectionError::Timeout);
        }
        Ok(())
    });
    guard()?;
    let (_, client) = crate::github_session(&app.state::<Host>(), &ticket.provider_account_id)?;
    if identity.id != ticket.provider_account_id {
        return Err(provider::ConnectionError::WrongIdentity);
    }
    let client = client.guarded(guard);
    let mut result = Scan::default();
    for origin in retained {
        let pull = pulls
            .iter()
            .find(|p| p.id == origin.proof.pull_request_id)
            .ok_or(provider::ConnectionError::IncompleteRead)?;
        let threads = match client.owned_threads_at(&origin.proof, &pull.head_sha) {
            Ok(threads) => threads,
            Err(provider::ConnectionError::InvalidResponse) => {
                crate::record(app, crate::DiagnosticEvent::ThreadSkipped);
                continue;
            }
            Err(error) => return Err(error),
        };
        result.retained.push(crate::retention::Observed {
            origin,
            head: pull.head_sha.clone(),
            threads,
        });
    }
    for origin in origins {
        let pull = pulls
            .iter()
            .find(|p| p.id == origin.review.job.pull_request_id)
            .ok_or(provider::ConnectionError::IncompleteRead)?;
        let threads = match client.owned_threads_at(&origin, &pull.head_sha) {
            Ok(threads) => threads,
            Err(provider::ConnectionError::InvalidResponse) => {
                crate::record(app, crate::DiagnosticEvent::ThreadSkipped);
                continue;
            }
            Err(error) => return Err(error),
        };
        result.feedback.push(Observed {
            origin,
            head: pull.head_sha.clone(),
            threads,
        });
    }
    for pull in pulls
        .iter_mut()
        .filter(|pull| pull.state == provider::Lifecycle::Open && !pull.draft)
    {
        let binding = crate::feedback::MentionBinding {
            configuration_id: ticket.repository_id.clone(),
            account_id: identity.id.clone(),
            account_login: identity.login.clone(),
            repository_id: ticket.provider_repository_id.clone(),
            repository_name: ticket.name.clone(),
            pull_request_id: pull.id.clone(),
            number: pull.number,
        };
        let comments = client.top_comments(
            &RemoteRepository {
                id: binding.repository_id.clone(),
                name: binding.repository_name.clone(),
            },
            binding.number,
        )?;
        let cursor = cursors
            .iter()
            .find(|cursor| cursor.binding.key("") == binding.key(""));
        pull.mentioned = cursor.is_some_and(|cursor| cursor.comments_initialized)
            && comments.iter().any(|comment| {
                comment.eligible_other_user(&identity.id)
                    && comment.mentions(&identity.login, &identity.id)
                    && cursor.is_none_or(|cursor| !cursor.comments.contains(&comment.id))
            });
        let admitted = tracked.iter().any(|tracked| {
            binding.matches_tracked(tracked) && tracked.lifecycle == provider::Lifecycle::Open
        }) || monitoring::scan_admits(ticket, pull);
        if admitted {
            match client.review_threads_at(
                &binding.repository_name,
                &binding.repository_id,
                &binding.pull_request_id,
                binding.number,
                &pull.head_sha,
            ) {
                Ok(threads) => result.threads.push((binding.clone(), threads)),
                // A malformed discussion on one PR must not fail the repository scan.
                Err(provider::ConnectionError::InvalidResponse) => {
                    crate::record(app, crate::DiagnosticEvent::ThreadSkipped);
                }
                Err(error) => return Err(error),
            }
        }
        result.mentions.push((binding, comments));
    }
    Ok(result)
}

pub(crate) fn admit_scan(
    store: &Store,
    ticket: &PollTicket,
    mut observations: Scan,
    now: i64,
) -> Result<(), String> {
    let queue = store.load_queue_state()?;
    let mut ledger = store.load_feedback()?;
    let publications = store.load_publications()?;
    // Capture history as context, with one durable observation cursor per PR.
    for observation in &observations.feedback {
        let job = &observation.origin.review.job;
        merge_thread_observation(
            &mut observations.threads,
            crate::feedback::MentionBinding {
                configuration_id: if job.configuration_id.is_empty() {
                    ticket.repository_id.clone()
                } else {
                    job.configuration_id.clone()
                },
                account_id: job.account_id.clone(),
                account_login: String::new(),
                repository_id: job.repository_id.clone(),
                repository_name: job.repository_name.clone(),
                pull_request_id: job.pull_request_id.clone(),
                number: job.number,
            },
            observation.threads.clone(),
        )?;
    }
    for observation in &observations.retained {
        let proof = &observation.origin.proof;
        merge_thread_observation(
            &mut observations.threads,
            crate::feedback::MentionBinding {
                configuration_id: proof.configuration_id.clone(),
                account_id: proof.account_id.clone(),
                account_login: String::new(),
                repository_id: proof.repository_id.clone(),
                repository_name: proof.repository_name.clone(),
                pull_request_id: proof.pull_request_id.clone(),
                number: proof.number,
            },
            observation.threads.clone(),
        )?;
    }
    for observation in &observations.feedback {
        let proof = observation
            .origin
            .ownership()
            .map_err(|_| "Invalid owned feedback provenance.")?;
        if !publications
            .iter()
            .any(|p| p.ownership().is_ok_and(|saved| saved == proof))
        {
            return Err("Owned feedback provenance changed during the scan.".into());
        }
        crate::feedback::observed_pr(&queue.tracked, &proof, &observation.head)?;
        ledger.observe(&observation.origin, &observation.head, &observation.threads)?;
    }
    store.save_feedback(&ledger)?;
    crate::retention::admit_observations(store, observations.retained, now)?;
    ledger = store.load_feedback()?;
    observe_threads(&queue.tracked, &mut ledger, observations.threads);
    for (binding, comments) in observations.mentions {
        let tracked = queue
            .tracked
            .iter()
            .any(|tracked| binding.matches_tracked(tracked));
        let index = conversation_cursor(&mut ledger, &binding);
        ledger.conversation_cursors[index].comments_initialized = true;
        if tracked {
            ledger.conversation_cursors[index].discussion = comments.clone();
        }
        // Comments are read in the review packet; no follow-up is admitted.
        for comment in comments {
            ledger.conversation_cursors[index]
                .comments
                .insert(comment.id);
        }
    }
    store.save_feedback(&ledger)?;
    Ok(())
}

fn conversation_cursor(
    ledger: &mut crate::feedback::Ledger,
    binding: &crate::feedback::MentionBinding,
) -> usize {
    ledger.conversation_cursor(binding)
}

fn merge_thread_observation(
    observations: &mut Vec<(crate::feedback::MentionBinding, Vec<Thread>)>,
    binding: crate::feedback::MentionBinding,
    threads: Vec<Thread>,
) -> Result<(), String> {
    if let Some((_, current)) = observations
        .iter_mut()
        .find(|(existing, _)| existing.key("") == binding.key(""))
    {
        for thread in threads {
            if let Some(existing) = current.iter().find(|existing| existing.id == thread.id) {
                if existing != &thread {
                    return Err(
                        "Discussion changed between complete observations; retry the scan.".into(),
                    );
                }
            } else {
                current.push(thread);
            }
        }
    } else {
        observations.push((binding, threads));
    }
    Ok(())
}

/// Cursor bookkeeping for owned-feedback and closure reads. Comments are read in
/// the review packet; no Reply follow-up is admitted.
fn observe_threads(
    tracked: &[monitoring::TrackedPullRequest],
    ledger: &mut crate::feedback::Ledger,
    observations: Vec<(crate::feedback::MentionBinding, Vec<Thread>)>,
) {
    for (binding, threads) in observations {
        let Ok(pr) = binding.tracked(tracked) else {
            continue;
        };
        if pr.lifecycle != provider::Lifecycle::Open {
            continue;
        }
        let index = conversation_cursor(ledger, &binding);
        ledger.conversation_cursors[index].threads_initialized = true;
        for thread in threads {
            if thread.resolved {
                ledger.conversation_cursors[index]
                    .closed_threads
                    .insert(thread.id.clone());
            }
            let external = thread.latest_other_user(&binding.account_id);
            ledger.conversation_cursors[index].thread_triggers.insert(
                thread.id.clone(),
                external.map(|comment| comment.id.clone()),
            );
            for comment in &thread.comments {
                ledger.conversation_cursors[index]
                    .threads
                    .insert(serde_json::json!([thread.id, comment.id]).to_string());
            }
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Candidate {
    #[serde(serialize_with = "serialize_run")]
    pub(crate) run: FollowUp,
    pub(crate) planned_selection: Option<Selection>,
    pub(crate) blocked: Option<String>,
    pub(crate) automatic_start: bool,
    pub(crate) automatic_publication: bool,
    pub(crate) captured_local_response: bool,
    pub(crate) human_gate: bool,
    pub(crate) superseded: bool,
}

fn serialize_run<S: serde::Serializer>(run: &FollowUp, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::{ser::Error, Serialize};
    let mut value = serde_json::to_value(run).map_err(S::Error::custom)?;
    if let ConversationTarget::Retained(origin) = &run.target {
        // Keep the existing inspector's thread fallback without fabricating a
        // historical ReviewRun. Execution context remains the actual new job.
        value["thread"] = serde_json::to_value(&origin.thread).map_err(S::Error::custom)?;
    }
    value.serialize(serializer)
}

pub(crate) fn candidates(store: &Store) -> Result<Vec<Candidate>, String> {
    let runs = store.load_follow_ups()?;
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    let mut feedback_ledger = store.load_feedback()?;
    feedback_ledger.observe_human_boundaries(&runs);
    let retained = crate::retention::load(store)?;
    Ok(runs
        .iter()
        .map(|saved_run| {
            let pending = saved_run.clone();
            let run = &pending;
            let policy = jobs
                .iter()
                .find(|j| run.matches_job(j))
                .ok_or("This revision is no longer detected.".to_string())
                .and_then(|job| {
                    let selection = Selection::resolve(&settings, job, &run.context.assignment_id)?;
                    if (run.result.is_some() || run.publication.is_some())
                        && !run.same_result_lens(&selection)
                    {
                        return Err(
                            "Captured conversation inputs changed; no stale publication allowed."
                                .into(),
                        );
                    }
                    run.authority(&settings, job)?;
                    if run.thread().is_ok_and(|thread| feedback_ledger.conversation_closed(job, &thread.id)) {
                        return Err("This discussion was human-closed; it cannot be resurrected by a new comment or role change.".into());
                    }
                    if run.context.feedback_checked {
                        let feedback = crate::feedback::conversation_contexts(store,job,&selection.agent.id).map_err(|e|e.message)?;
                        if feedback.iter().any(|c| run.owns_feedback(c) && c.closed) {
                            return Err("This owned discussion was closed externally; no new analysis or reply is authorized.".into());
                        }
                    }
                    if run.context.feedback_checked && (run.result.is_some() || run.publication.is_some()) {
                        crate::feedback::validate_conversation_context(
                            store,
                            job,
                            &run.context.selection.agent.id,
                            &run.context.feedback,
                        )
                        .map_err(|e| e.message)?;
                    }
                    Ok(selection)
                });
            let human_gate = run.thread().is_ok_and(|thread| feedback_ledger.human_gated(&run.context.job, &thread.id)
                || (!feedback_ledger.human_answered(&run.context.job, &thread.id)
                    && retained.receipts.iter().any(|receipt| crate::retention::Binding::tracked(&receipt.scope).matches(&run.context.job)
                        && receipt.owned.iter().any(|owned| owned.human_input_threads.contains(&thread.id)))));
            Candidate {
                planned_selection: policy.as_ref().ok().cloned(),
                run: run.clone(),
                blocked: policy.as_ref().err().cloned(),
                automatic_start: policy.is_ok() && !human_gate,
                automatic_publication: policy.is_ok() && jobs.iter().find(|job| run.matches_job(job))
                    .is_some_and(|job| run.authority(&settings, job).is_ok_and(|grant| grant))
                    && !run.captured_local_response(),
                human_gate,
                superseded: feedback_ledger.thread_superseded(run),
                captured_local_response: run.captured_local_response(),
            }
        })
        .collect())
}

#[derive(Default)]
pub(crate) struct Coordinator {
    pub(crate) active: Mutex<Option<(String, Arc<AtomicBool>)>>,
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
    /// Scans `view`, a frozen copy of saved state; launch revalidates.
    pub(crate) fn pump(app: &tauri::AppHandle, view: &Store) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) || !host.follow_ups.finished() {
            return Ok(());
        }
        let next = next_due(view, now_seconds()?)?;
        if let Some(id) = next {
            Self::launch(app, &id, false)?;
        }
        Ok(())
    }
}

/// The follow-up whose analysis or publication should run next, if any.
pub(crate) fn next_due(store: &Store, now: i64) -> Result<Option<String>, String> {
    if store.load_automation()?.paused {
        return Ok(None);
    }
    Ok(candidates(store)?.into_iter().find_map(|candidate| {
        let run = &candidate.run;
        let due = |op: &JobOperation| {
            matches!(
                op.state,
                OperationState::Queued | OperationState::Interrupted
            ) && op.next_attempt_at.is_some_and(|n| n <= now)
        };
        if run.publication.as_ref().is_some_and(due) {
            return Some(run.id.clone());
        }
        if candidate.blocked.is_some() || run.cancelled {
            return None;
        }
        if run.phase == Phase::WaitingPublication
            && run.publication.is_none()
            && candidate.automatic_publication
        {
            return Some(run.id.clone());
        }
        None
    }))
}

impl Coordinator {
    /// Publish an already-analyzed legacy reply. Analysis itself is retired.
    fn launch(app: &tauri::AppHandle, id: &str, manual: bool) -> Result<(), String> {
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
            if store.load_automation()?.paused {
                return Err("Automation is paused. Resume before starting publication; existing receipts are retained.".into());
            }
            let candidate = candidates(&store)?
                .into_iter()
                .find(|c| c.run.id == id)
                .ok_or("Thread follow-up disappeared.")?;
            let mut run = candidate.run;
            let now = now_seconds()?;
            if run.captured_local_response() {
                return Err("This response completed locally without a Reply Comment grant. Historical local responses are not replayed; a new comment creates new work.".into());
            }
            if !candidate.automatic_publication && !run.uncertain && run.receipt.is_none() {
                return Err("Reply Comment permission is off. This response remains local; manual confirmation cannot grant a capability.".into());
            }
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
                let mut operation = run.operation(
                    if run.kind() == crate::capacity::Kind::Mention {
                        "mention_reply"
                    } else {
                        "thread_reply"
                    },
                    now,
                );
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
            if manual {
                run.cancelled = false;
            }
            run.error = None;
            save_to_store(&store, &run)?;
            run
        };
        let generation = generations
            .get(&run.context.job.account_id)
            .copied()
            .unwrap_or(0);
        drop(generations);
        let cancelled = Arc::new(AtomicBool::new(false));
        let mutation_owner = format!("conversation:{}", run.id);
        if !host.mutations.acquire(&mutation_owner)? {
            return Err("Another provider mutation is running; this reply remains queued.".into());
        }
        *active = Some((run.id.clone(), cancelled.clone()));
        drop(active);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let worker = app.clone();
            let outcome = tauri::async_runtime::spawn_blocking(move || {
                let mut native = Native {
                    app: worker,
                    generation,
                    cancelled,
                    cancelled_read: Arc::new(AtomicBool::new(false)),
                };
                super::publish(&mut native, &mut run)
            })
            .await;
            match outcome {
                Ok(Err(error)) => crate::report(&app, error.message),
                Err(_) => crate::report(&app, "Thread publication worker could not finish.".into()),
                _ => {}
            }
            if let Err(error) = app.state::<Host>().mutations.release(&mutation_owner) {
                crate::report(&app, error);
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
        return Err(Failure::cancelled());
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
    cancelled_read: Arc<AtomicBool>,
}

impl Native {
    fn read_failure(&self, error: provider::ConnectionError) -> Failure {
        if error == provider::ConnectionError::Configuration
            && self.cancelled_read.load(Ordering::SeqCst)
        {
            Failure::cancelled()
        } else {
            error.into()
        }
    }
    fn origin(&self, run: &FollowUp) -> Result<Ownership, Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        self.origin_without_lock(&store, run)
    }
    fn client(&self, run: &FollowUp) -> Result<GithubClient<HttpTransport>, Failure> {
        let host = self.app.state::<Host>();
        connection_gate(&host, &run.context.job.account_id, self.generation)?;
        let (identity, client) = crate::github_session(&host, &run.context.job.account_id)?;
        if identity.id != run.context.job.account_id {
            return Err(Failure::permanent("Acting account changed."));
        }
        connection_gate(&host, &run.context.job.account_id, self.generation)?;
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
                &snapshot.context.job.account_id,
                native.generation,
            )
            .map_err(|_| provider::ConnectionError::Configuration)?;
            if snapshot.publication.is_none() && native.cancelled.load(Ordering::SeqCst) {
                native.cancelled_read.store(true, Ordering::SeqCst);
                return Err(provider::ConnectionError::Configuration);
            }
            let operation = snapshot
                .publication
                .as_ref()
                .or(snapshot.analysis.as_ref())
                .ok_or(provider::ConnectionError::Configuration)?;
            if now_seconds().map_err(|_| provider::ConnectionError::Configuration)?
                >= operation.retry_deadline
            {
                return Err(provider::ConnectionError::Timeout);
            }
            Ok(())
        })))
    }
    fn local(&self, run: &FollowUp) -> Result<(), Failure> {
        connection_gate(
            &self.app.state::<Host>(),
            &run.context.job.account_id,
            self.generation,
        )?;
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(Failure::cancelled());
        }
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        local_gate(&store, run)
    }
}

fn local_gate(store: &Store, run: &FollowUp) -> Result<(), Failure> {
    if run.publication.is_some() {
        crate::capacity::publication_gate(store).map_err(Failure::permanent)?;
    }
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
        .find(|j| run.matches_job(j))
        .ok_or_else(|| Failure::permanent("The reviewed revision is no longer detected."))?;
    let selection = Selection::resolve(&settings, job, &run.context.assignment_id)
        .map_err(Failure::permanent)?;
    let automatic = run.authority(&settings, job).map_err(Failure::permanent)?;
    let ledger = store.load_feedback().map_err(Failure::permanent)?;
    if run
        .thread()
        .is_ok_and(|thread| ledger.conversation_closed(job, &thread.id))
    {
        return Err(Failure::permanent(
            "This human-closed discussion cannot receive a new analysis or reply.",
        ));
    }
    run.check_publication_grant(&current, automatic)?;
    if run.publication.is_none() && !run.same_analysis_execution(&selection) {
        return Err(Failure::permanent(
            "Follow-up selection changed; explicit retry required.",
        ));
    }
    if run.publication.is_none() {
        run.validate_analysis_base(job)?;
    }
    if run.context.feedback_checked || matches!(run.target, ConversationTarget::Mention { .. }) {
        if run.publication.is_none() {
            run.validate_analysis_feedback(store, job)?;
        } else {
            crate::feedback::validate_conversation_context(
                store,
                job,
                &run.context.selection.agent.id,
                &run.context.feedback,
            )?;
        }
    }
    Ok(())
}

fn native_gate_result(check: Result<(), Failure>) -> Result<Option<String>, Failure> {
    check.map(|_| None)
}

impl Environment for Native {
    fn now(&self) -> Result<i64, Failure> {
        now_seconds().map_err(Failure::permanent)
    }
    fn save(&mut self, run: &mut FollowUp) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        save_progress(&store, run, self.now()?)
    }
    fn observe(&mut self, run: &FollowUp) -> Result<Observation, Failure> {
        let client = self.read_client(run)?;
        match &run.target {
            ConversationTarget::Owned(_) | ConversationTarget::Retained(_) => {
                Ok(Observation::Owned(
                    client
                        .owned_thread(
                            &self.origin(run)?,
                            &run.thread().map_err(Failure::permanent)?.id,
                        )
                        .map_err(|e| self.read_failure(e))?
                        .ok_or_else(|| {
                            Failure::permanent("The owned review thread is no longer available.")
                        })?,
                ))
            }
            ConversationTarget::Mention { comment } => {
                let replies = client
                    .top_comments(
                        &RemoteRepository {
                            id: run.context.job.repository_id.clone(),
                            name: run.context.job.repository_name.clone(),
                        },
                        run.context.job.number,
                    )
                    .map_err(|e| self.read_failure(e))?;
                let current = replies.iter().find(|c| c.id == comment.id).cloned();
                Ok(Observation::Mention {
                    comment: current,
                    replies,
                })
            }
            ConversationTarget::Thread { thread } => Ok(Observation::Owned(
                client
                    .review_thread(
                        &run.context.job.repository_id,
                        &run.context.job.pull_request_id,
                        &thread.id,
                    )
                    .map_err(|e| self.read_failure(e))?
                    .ok_or_else(|| {
                        Failure::permanent("The review discussion is no longer available.")
                    })?,
            )),
        }
    }
    fn gate(&mut self, run: &FollowUp, observed: &Observation) -> Result<Option<String>, Failure> {
        let client = self.read_client(run)?;
        let job = &run.context.job;
        let repo = RemoteRepository {
            id: job.repository_id.clone(),
            name: job.repository_name.clone(),
        };
        let connection = client
            .connect(&repo.name, Some(&job.account_id))
            .map_err(|e| self.read_failure(e))?;
        if connection.repository.id != repo.id {
            return Err(Failure::permanent("Repository identity changed."));
        }
        let pull = client
            .pull_request(&repo, job.number)
            .map_err(|e| self.read_failure(e))?;
        let local = self.local(run);
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
        let settings = store.load_settings().map_err(Failure::permanent)?;
        let jobs = store.load_queue().map_err(Failure::permanent)?;
        let current_job = jobs.iter().find(|j| run.matches_job(j));
        let mut monitor = host
            .monitor
            .lock()
            .map_err(|_| Failure::permanent("Monitoring unavailable."))?;
        let active = monitor
            .activation_status(&settings, &job.configuration_id)
            .active;
        let gate = current_job
            .ok_or_else(|| Failure::permanent("Conversation iteration is unavailable."))
            .and_then(|job| {
                run.validate_current(
                    &settings,
                    job,
                    &pull,
                    active,
                    connection.capabilities.comment == CommentCapability::Available,
                )
            });
        if pull.head_sha != job.head_sha && monitoring::new_revision_eligible(&settings, job, &pull)
        {
            monitor
                .request_revision_check(&store, job, self.now()?)
                .map_err(Failure::permanent)?;
        }
        native_gate_result(gate)?;
        if local.is_err() {
            return native_gate_result(local);
        }
        if let Observation::Owned(thread) = observed {
            if !matches!(run.target, ConversationTarget::Thread { .. })
                && !thread.owned_by(&self.origin_without_lock(&store, run)?)
            {
                return Ok(Some("Original thread provenance changed.".into()));
            }
            if run.publication.is_none() {
                run.validate_observed_trigger(observed)?;
            }
        }
        if !run.fresh_observation(observed) {
            return Ok(Some(
                "The owned thread was resolved, changed, superseded, or no longer permits replies."
                    .into(),
            ));
        }
        Ok(None)
    }
    fn reply(&mut self, run: &FollowUp) -> Result<String, WriteFailure> {
        let prepared = (|| {
            let client = self.client(run)?;
            let current = self.observe(run)?;
            if let Some(receipt) = run.reconcile_observation(&current)? {
                return Ok((client, Some(receipt)));
            }
            if let Some(reason) = self.gate(run, &current)? {
                return Err(Failure::permanent(reason));
            }
            self.local(run)?;
            if self.now()? >= run.publication.as_ref().unwrap().retry_deadline {
                return Err(Failure::timeout());
            }
            Ok((client, None))
        })()
        .map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        if let Some(receipt) = prepared.1 {
            return Ok(receipt);
        }
        let body = run.body.as_deref().ok_or_else(|| WriteFailure {
            failure: Failure::permanent("Reply body unavailable."),
            uncertain: false,
        })?;
        let mut result = match &run.target {
            ConversationTarget::Owned(_) | ConversationTarget::Retained(_) => {
                prepared.0.reply_to_thread(
                    &self.origin(run).map_err(|failure| WriteFailure {
                        failure,
                        uncertain: false,
                    })?,
                    &run.thread()
                        .map_err(|message| WriteFailure {
                            failure: Failure::permanent(message),
                            uncertain: false,
                        })?
                        .root()
                        .map_err(|e| WriteFailure {
                            failure: e.into(),
                            uncertain: false,
                        })?
                        .id,
                    body,
                )
            }
            ConversationTarget::Mention { .. } => prepared.0.reply_to_mention(
                &RemoteRepository {
                    id: run.context.job.repository_id.clone(),
                    name: run.context.job.repository_name.clone(),
                },
                run.context.job.number,
                &run.context.job.account_id,
                body,
            ),
            ConversationTarget::Thread { thread } => prepared.0.reply_to_review_thread(
                &run.context.job.repository_name,
                run.context.job.number,
                &run.context.job.account_id,
                &thread
                    .root()
                    .map_err(|error| WriteFailure {
                        failure: error.into(),
                        uncertain: false,
                    })?
                    .id,
                body,
            ),
        };
        if let Err(error) = &mut result {
            if let (Ok(now), Ok(store)) = (
                self.now(),
                self.app.state::<Host>().store.lock().map_err(|_| ()),
            ) {
                crate::backpressure::pace_write(
                    &store,
                    &mut error.failure,
                    error.uncertain,
                    &run.context.job.account_id,
                    run.publication
                        .as_ref()
                        .map_or(run.id.as_str(), |op| op.id.as_str()),
                    now,
                );
            }
        }
        result
    }
}

fn save_progress(store: &Store, run: &mut FollowUp, now: i64) -> Result<(), Failure> {
    let current = store
        .load_follow_ups()
        .map_err(Failure::permanent)?
        .into_iter()
        .find(|r| r.id == run.id)
        .ok_or_else(|| Failure::permanent("Thread follow-up disappeared."))?;
    if run.publication.is_none()
        && current.analysis.as_ref().map(|op| &op.id) != run.analysis.as_ref().map(|op| &op.id)
    {
        return Err(Failure::permanent(
            "A newer analysis attempt owns this follow-up.",
        ));
    }
    if run.publication.is_none()
        && run
            .analysis
            .as_ref()
            .is_some_and(|op| op.state == OperationState::Running)
    {
        if let (Some(operation), Some(saved)) = (&mut run.analysis, &current.analysis) {
            operation.interruption = saved.interruption;
        }
    }
    if current.cancelled {
        run.cancelled = true;
        run.confirmed = false;
        if run.publication.is_none() {
            run.result = None;
            run.phase = Phase::Stopped;
            run.error = Some("Thread follow-up cancelled.".into());
            if let Some(op) = run.analysis.as_mut() {
                op.fail(&Failure::permanent("Cancelled.").monitoring(), now);
            }
        }
    }
    save_to_store(store, run).map_err(Failure::permanent)
}

impl Native {
    fn origin_without_lock(&self, store: &Store, run: &FollowUp) -> Result<Ownership, Failure> {
        if let ConversationTarget::Retained(origin) = &run.target {
            return crate::retention::load(store)
                .map_err(Failure::permanent)?
                .receipts
                .into_iter()
                .flat_map(|r| r.owned)
                .find(|r| r.proof == origin.proof)
                .map(|r| r.proof)
                .ok_or_else(|| {
                    Failure::permanent("Retained owned-review provenance unavailable.")
                });
        }
        store
            .load_publications()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|p| run.owned().is_ok_and(|o| p.id == o.publication_id))
            .ok_or_else(|| Failure::permanent("Owned review receipt unavailable."))?
            .ownership()
            .map_err(Failure::from)
    }
}

#[tauri::command]
pub(crate) async fn start_follow_up(app: tauri::AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || Coordinator::launch(&app, &id, true))
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
        cancel_in_store(&store, &host.ai, active.as_ref(), &id, now_seconds()?)
    })
    .await
    .map_err(|_| "Follow-up cancellation failed.".to_string())?
}

fn cancel_in_store(
    store: &Store,
    capacity: &crate::capacity::Coordinator,
    active_publication: Option<&(String, Arc<AtomicBool>)>,
    id: &str,
    now: i64,
) -> Result<(), String> {
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
    if let Some(operation) = &run.analysis {
        capacity.cancel(&operation.id)?;
    }
    if let Some((_, cancelled)) = active_publication.filter(|(active, _)| active == id) {
        cancelled.store(true, Ordering::SeqCst);
    } else {
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
}
