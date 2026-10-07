use super::*;
use crate::{
    github::{
        self,
        http::HttpTransport,
        metadata::PullRequest,
        provider::{CommentCapability, GithubClient, RemoteRepository},
        review::GuardedTransport,
        threads::{Ownership, Provenance},
    },
    monitoring::{self, PollTicket},
    now_seconds,
    publication::WriteFailure,
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
    head: String,
    threads: Vec<Thread>,
}

#[derive(Default)]
pub(crate) struct Scan {
    pub(crate) retained: Vec<crate::retention::Observed>,
    pub(crate) feedback: Vec<Observed>,
    pub(crate) mentions: Vec<(
        crate::feedback::MentionBinding,
        Vec<github::conversation::TopComment>,
    )>,
    pub(crate) threads: Vec<(crate::feedback::MentionBinding, Vec<Thread>)>,
}

pub(crate) fn scan(
    app: &tauri::AppHandle,
    ticket: &PollTicket,
    pulls: &mut [PullRequest],
    identity: &github::Identity,
) -> Result<Scan, github::ConnectionError> {
    let (origins, tracked, retained, cursors) = {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| github::ConnectionError::Configuration)?;
        (
            super::observation_origins(&store, ticket, pulls)
                .map_err(|_| github::ConnectionError::Configuration)?,
            store
                .load_queue_state()
                .map_err(|_| github::ConnectionError::Configuration)?
                .tracked,
            crate::retention::scan_origins(&store, ticket, pulls)
                .map_err(|_| github::ConnectionError::Configuration)?,
            store
                .load_feedback()
                .map_err(|_| github::ConnectionError::Configuration)?
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
    if identity.id != ticket.provider_account_id {
        return Err(github::ConnectionError::WrongIdentity);
    }
    let client = client.guarded(guard);
    let mut result = Scan::default();
    for origin in retained {
        let pull = pulls
            .iter()
            .find(|p| p.id == origin.proof.pull_request_id)
            .ok_or(github::ConnectionError::IncompleteRead)?;
        let threads = client.owned_threads_at(&origin.proof, &pull.head_sha)?;
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
            .ok_or(github::ConnectionError::IncompleteRead)?;
        let threads = client.owned_threads_at(&origin, &pull.head_sha)?;
        result.feedback.push(Observed {
            origin,
            head: pull.head_sha.clone(),
            threads,
        });
    }
    for pull in pulls
        .iter_mut()
        .filter(|pull| pull.state == github::metadata::Lifecycle::Open && !pull.draft)
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
            binding.matches_tracked(tracked)
                && tracked.lifecycle == github::metadata::Lifecycle::Open
        }) || monitoring::scan_admits(ticket, pull);
        if admitted {
            let threads = client.review_threads_at(
                &binding.repository_name,
                &binding.repository_id,
                &binding.pull_request_id,
                binding.number,
                &pull.head_sha,
            )?;
            result.threads.push((binding.clone(), threads));
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
    let settings = store.load_settings()?;
    let queue = store.load_queue_state()?;
    let jobs = &queue.jobs;
    let mut ledger = store.load_feedback()?;
    let publications = store.load_publications()?;
    // Capture history as context, with one durable observation cursor per PR.
    // Pending intents are saved before execution; permission activation never
    // replays this baseline or changes the original feedback provenance.
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
    let mut runs = store.load_follow_ups()?;
    let before = runs.len();
    admit_thread_intents(
        store,
        &queue.tracked,
        &mut ledger,
        observations.threads,
        now,
    )?;
    for (binding, comments) in observations.mentions {
        let tracked = if queue
            .tracked
            .iter()
            .any(|tracked| binding.matches_tracked(tracked))
        {
            Some(binding.tracked(&queue.tracked)?)
        } else {
            None
        };
        let index = conversation_cursor(&mut ledger, &binding);
        let initialized = ledger.conversation_cursors[index].comments_initialized;
        ledger.conversation_cursors[index].comments_initialized = true;
        if tracked.is_some() {
            ledger.conversation_cursors[index].discussion = comments.clone();
        }
        for comment in comments {
            let newly_observed = ledger.conversation_cursors[index]
                .comments
                .insert(comment.id.clone());
            if tracked.is_none()
                || !initialized
                || !newly_observed
                || !comment.eligible_other_user(&binding.account_id)
            {
                continue;
            }
            let key = binding.key(&comment.id);
            if ledger.mentions.iter().any(|m| m.key == key)
                || crate::retention::known_key(store, &key)?
            {
                continue;
            }
            let existing = runs.iter().find(|run| run.key == key);
            let (work_id, enqueue_order, enqueued_at) = if let Some(run) = existing {
                (
                    run.id.clone(),
                    run.enqueue_order
                        .ok_or("Retained mention order is unavailable.")?,
                    run.enqueued_at
                        .ok_or("Retained mention enqueue time is unavailable.")?,
                )
            } else {
                (
                    uuid::Uuid::new_v4().to_string(),
                    store.allocate_enqueue_order()?,
                    now,
                )
            };
            let item_id = if let Some(run) = existing {
                crate::queue::item_id(&run.context.job)
            } else {
                let tracked = binding.tracked(&queue.tracked)?;
                if tracked.lifecycle != github::metadata::Lifecycle::Open {
                    return Err("The observed mention's tracked PR is no longer open.".into());
                }
                tracked.item_id.clone()
            };
            ledger.mentions.push(crate::feedback::Mention {
                item_id: Some(item_id),
                key,
                work_id,
                enqueue_order,
                enqueued_at,
                binding: binding.clone(),
                comment,
                follow_up_id: None,
                blocked: Some("Comment observed; primary assessment admission is pending.".into()),
            });
        }
    }
    // Commit observation identity/order before materializing any mention execution or linkage.
    store.save_feedback(&ledger)?;
    materialize_thread_intents(store, ticket, &settings, jobs, &mut ledger, &mut runs)?;
    // Recover saved intent even when this poll does not rediscover the remote comment.
    for mention in ledger.mentions.iter_mut().filter(|m| {
        m.binding.configuration_id == ticket.repository_id
            && m.binding.account_id == ticket.provider_account_id
            && m.binding.repository_id == ticket.provider_repository_id
    }) {
        let item_id = match mention.association(&queue.tracked, jobs, &runs) {
            Ok(id) => id,
            Err(reason) => {
                mention.blocked = Some(reason.into());
                continue;
            }
        };
        mention.item_id = Some(item_id.clone());
        let binding = &mention.binding;
        if let Some(run) = runs
            .iter()
            .find(|r| r.key == mention.key || r.id == mention.work_id)
        {
            mention.follow_up_id = Some(run.id.clone());
            mention.blocked = None;
            continue;
        }
        let route = (|| -> Result<FollowUp, String> {
            let tracked = binding.tracked(&queue.tracked)?;
            if tracked.item_id != item_id || tracked.lifecycle != github::metadata::Lifecycle::Open
            {
                return Err("Mention belongs to a superseded or terminal iteration; retained without replay.".into());
            }
            let repository = settings
                .repositories
                .iter()
                .find(|r| r.id == binding.configuration_id)
                .ok_or("Repository removed.")?;
            let primary = repository
                .primary_assignment_id()
                .ok_or("No primary assigned; mention retained without fallback.")?;
            let assigned = repository
                .assignments
                .iter()
                .find(|a| a.id == primary)
                .ok_or("Primary assignment unavailable.")?;
            if !ticket
                .assignments
                .iter()
                .any(|a| a.assignment_id == primary && a.agent_id == assigned.agent_id)
            {
                return Err(
                    "Primary was not captured by this scan; waiting for the next global scan."
                        .into(),
                );
            }
            let job = jobs
                .iter()
                .rev()
                .find(|j| {
                    binding.matches(j)
                        && item_id == crate::queue::item_id(j)
                        && j.assignment_id.as_deref() == Some(primary)
                        && monitoring::review_policy(&settings, j, None).is_ok()
                })
                .ok_or("The primary's current iteration is unavailable.")?;
            let selection = Selection::resolve(&settings, job, primary)?;
            let feedback = crate::feedback::conversation_contexts(store, job, &selection.agent.id)
                .map_err(|e| e.message)?;
            let mut run = FollowUp::mention(
                mention.comment.clone(),
                ConversationContext {
                    assignment_id: primary.into(),
                    job: job.clone(),
                    selection,
                    trust_confirmed: false,
                    feedback,
                    feedback_checked: true,
                },
            );
            run.id = mention.work_id.clone();
            run.discussion = ledger
                .conversation_cursors
                .iter()
                .find(|cursor| cursor.binding.key("") == binding.key(""))
                .map(|cursor| cursor.discussion.clone())
                .unwrap_or_default();
            Ok(run)
        })();
        match route {
            Ok(run) => {
                mention.item_id = Some(crate::queue::item_id(&run.context.job));
                let id = run.id.clone();
                if super::admit_run(&mut runs, run)? {
                    let work = runs.last_mut().ok_or("Mention admission disappeared.")?;
                    work.enqueue_order = Some(mention.enqueue_order);
                    work.enqueued_at = Some(mention.enqueued_at);
                }
                mention.follow_up_id = Some(id);
                mention.blocked = None;
            }
            Err(reason) => mention.blocked = Some(reason),
        }
    }
    if runs.len() != before {
        store.save_follow_ups(&runs)?;
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

fn primary_context(
    store: &Store,
    settings: &crate::storage::Settings,
    jobs: &[monitoring::QueueJob],
    binding: &crate::feedback::MentionBinding,
    item_id: &str,
) -> Result<ConversationContext, String> {
    let repository = settings
        .repositories
        .iter()
        .find(|r| r.id == binding.configuration_id)
        .ok_or("Repository removed.")?;
    let primary = repository
        .primary_assignment_id()
        .ok_or("No primary assigned; conversation retained locally without fallback.")?;
    let job = jobs
        .iter()
        .rev()
        .find(|job| {
            binding.matches(job)
                && crate::queue::item_id(job) == item_id
                && job.assignment_id.as_deref() == Some(primary)
                && monitoring::review_policy(settings, job, None).is_ok()
        })
        .ok_or("The current primary's admitted iteration is unavailable.")?;
    let selection = Selection::resolve(settings, job, primary)?;
    Ok(ConversationContext {
        assignment_id: primary.into(),
        job: job.clone(),
        feedback: crate::feedback::conversation_contexts(store, job, &selection.agent.id)
            .map_err(|e| e.message)?,
        selection,
        trust_confirmed: false,
        feedback_checked: true,
    })
}

fn admit_thread_intents(
    store: &Store,
    tracked: &[monitoring::TrackedPullRequest],
    ledger: &mut crate::feedback::Ledger,
    observations: Vec<(crate::feedback::MentionBinding, Vec<Thread>)>,
    now: i64,
) -> Result<(), String> {
    for (binding, threads) in observations {
        let pr = binding.tracked(tracked)?;
        if pr.lifecycle != github::metadata::Lifecycle::Open {
            continue;
        }
        let index = conversation_cursor(ledger, &binding);
        let initialized = ledger.conversation_cursors[index].threads_initialized;
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
            let key = external.map(|comment| {
                serde_json::json!([
                    "thread",
                    "github",
                    binding.account_id,
                    binding.configuration_id,
                    binding.repository_id,
                    binding.pull_request_id,
                    thread.id,
                    comment.id
                ])
                .to_string()
            });
            // Record every historical external identity, not just the current
            // last comment, so deletion cannot replay an older conversation.
            let mut fresh = false;
            for comment in &thread.comments {
                let observed = serde_json::json!([thread.id, comment.id]).to_string();
                let added = ledger.conversation_cursors[index].threads.insert(observed);
                if external.is_some_and(|external| external.id == comment.id) {
                    fresh = added;
                }
            }
            let root = thread.root().map_err(|_| "Invalid discussion root.")?;
            let closed = ledger.records.iter().any(|record| {
                record.context.root_id == root.id
                    && binding.matches(&record.job)
                    && record.context.closed
            });
            let Some(key) = key else {
                continue;
            };
            if !initialized
                || !fresh
                || ledger.conversation_cursors[index]
                    .closed_threads
                    .contains(&thread.id)
                || closed
                || ledger
                    .pending_threads
                    .iter()
                    .any(|intent| intent.key == key)
                || crate::retention::known_key(store, &key)?
            {
                continue;
            }
            ledger.pending_threads.push(crate::feedback::PendingThread {
                binding: binding.clone(),
                item_id: pr.item_id.clone(),
                thread,
                key,
                work_id: uuid::Uuid::new_v4().to_string(),
                enqueue_order: store.allocate_enqueue_order()?,
                enqueued_at: now,
                follow_up_id: None,
                blocked: Some("Primary assessment pending.".into()),
            });
        }
    }
    Ok(())
}

fn materialize_thread_intents(
    store: &Store,
    ticket: &PollTicket,
    settings: &crate::storage::Settings,
    jobs: &[monitoring::QueueJob],
    ledger: &mut crate::feedback::Ledger,
    runs: &mut Vec<FollowUp>,
) -> Result<(), String> {
    for intent in ledger.pending_threads.iter_mut().filter(|intent| {
        intent.binding.configuration_id == ticket.repository_id
            && intent.binding.account_id == ticket.provider_account_id
            && intent.binding.repository_id == ticket.provider_repository_id
    }) {
        if let Some(run) = runs.iter().find(|run| run.key == intent.key) {
            if run.id != intent.work_id
                || crate::queue::item_id(&run.context.job) != intent.item_id
                || !intent.binding.matches(&run.context.job)
                || run.enqueue_order != Some(intent.enqueue_order)
                || run.enqueued_at != Some(intent.enqueued_at)
            {
                return Err("Discussion execution conflicts with its durable intent; no replacement was created.".into());
            }
            intent.follow_up_id = Some(run.id.clone());
            intent.blocked = None;
            continue;
        }
        if intent.follow_up_id.is_some() {
            intent.blocked = Some("Discussion execution history is unavailable; reconcile the original intent instead of replaying it.".into());
            continue;
        }
        let route = primary_context(store, settings, jobs, &intent.binding, &intent.item_id)
            .and_then(|context| FollowUp::general_thread(intent.thread.clone(), context));
        match route {
            Ok(mut run) => {
                run.id = intent.work_id.clone();
                run.enqueue_order = Some(intent.enqueue_order);
                run.enqueued_at = Some(intent.enqueued_at);
                intent.follow_up_id = Some(run.id.clone());
                super::admit_stored(store, runs, run)?;
                intent.blocked = None;
            }
            Err(reason) => intent.blocked = Some(reason),
        }
    }
    Ok(())
}

#[derive(Serialize)]
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
            let mut pending = saved_run.clone();
            if !pending.cancelled && pending.result.is_none() && pending.publication.is_none()
                && pending.analysis_history.is_empty()
                && pending.history.iter().all(|operation| operation.attempt_count == 0)
                && pending.analysis.as_ref().is_none_or(|operation| operation.attempt_count == 0 && operation.state != OperationState::Running)
            {
                let job = &pending.context.job;
                let binding = crate::feedback::MentionBinding {
                    configuration_id: job.configuration_id.clone(), account_id: job.account_id.clone(),
                    account_login: String::new(), repository_id: job.repository_id.clone(),
                    repository_name: job.repository_name.clone(), pull_request_id: job.pull_request_id.clone(), number: job.number,
                };
                if let Ok(context) = primary_context(store, &settings, &jobs, &binding, &crate::queue::item_id(job)) {
                    if context.assignment_id != pending.context.assignment_id
                        || context.selection.agent != pending.context.selection.agent
                        || context.job != pending.context.job {
                        if let Some(operation) = pending.analysis.take() { pending.retain_analysis_attempt(operation); }
                        pending.context = context;
                    }
                }
            }
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
            if store.load_automation()?.paused {
                return Ok(());
            }
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
                if candidate.blocked.is_some() || run.cancelled {
                    return None;
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
        if !publish {
            {
                let store = host
                    .store
                    .lock()
                    .map_err(|_| "Thread storage unavailable.")?;
                request_analysis(&store, id, manual, now_seconds()?)?;
            }
            return crate::capacity::Coordinator::pump(app);
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
            if publish {
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
            if publish {
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
                    Err(_) => {
                        crate::report(&app, "Thread publication worker could not finish.".into())
                    }
                    _ => {}
                }
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

pub(crate) fn prepare_analysis(
    store: &Store,
    run: &mut FollowUp,
    blocked: Option<String>,
    human_gate: bool,
    manual: bool,
    now: i64,
) -> Result<(), String> {
    if run.result.is_some() || run.publication.is_some() {
        return Err(
            "This follow-up was already analyzed; a later external comment creates new work."
                .into(),
        );
    }
    if let Some(error) = blocked {
        return Err(error);
    }
    if human_gate && !manual && !run.manual_start {
        return Err("A prior comment needs human judgment before retrying this follow-up.".into());
    }
    if run
        .analysis
        .as_ref()
        .is_some_and(|op| op.state == OperationState::Running)
    {
        return Err("This analysis is running or stopping; wait for teardown.".into());
    }
    if run.analysis.is_none() || manual {
        let settings = store.load_settings()?;
        let jobs = store.load_queue()?;
        let job = jobs
            .iter()
            .find(|j| run.matches_job(j))
            .ok_or("The reviewed revision is no longer available.")?;
        let selection = Selection::resolve(&settings, job, &run.context.assignment_id)?;
        if let Some(previous) = run.analysis.take() {
            run.retain_analysis_attempt(previous);
        }
        if !run.same_analysis_execution(&selection) {
            run.context.selection = selection;
        }
        if run.context.feedback_checked {
            run.context.feedback =
                crate::feedback::conversation_contexts(store, job, &run.context.selection.agent.id)
                    .map_err(|e| e.message)?;
        }
        run.analysis = Some(run.operation(
            if run.kind() == crate::capacity::Kind::Mention {
                "mention_analysis"
            } else {
                "thread_analysis"
            },
            now,
        ));
        run.manual_start = manual;
    }
    run.manual_start |= manual;
    Ok(())
}

pub(crate) fn request_analysis(
    store: &Store,
    id: &str,
    manual: bool,
    now: i64,
) -> Result<FollowUp, String> {
    let candidate = candidates(store)?
        .into_iter()
        .find(|c| c.run.id == id)
        .ok_or("Thread follow-up disappeared.")?;
    let mut run = candidate.run;
    prepare_analysis(
        store,
        &mut run,
        candidate.blocked,
        candidate.human_gate,
        manual,
        now,
    )?;
    if manual {
        run.cancelled = false;
    }
    run.error = None;
    save_to_store(store, &run)?;
    Ok(run)
}

pub(crate) fn prepare_dispatch(store: &Store, id: &str, now: i64) -> Result<FollowUp, String> {
    let mut run = request_analysis(store, id, false, now)?;
    let result = run
        .analysis
        .as_mut()
        .ok_or("Reply analysis missing.")?
        .begin_ai_attempt(now);
    if let Err(error) = result {
        run.error = Some(error.clone());
        save_to_store(store, &run)?;
        return Err(error);
    }
    run.phase = Phase::Analyzing;
    save_to_store(store, &run)?;
    Ok(run)
}

pub(crate) fn launch_analysis_worker(
    app: &tauri::AppHandle,
    mut run: FollowUp,
    cancelled: Arc<AtomicBool>,
) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let operation_id = run
            .analysis
            .as_ref()
            .expect("dispatched analysis")
            .id
            .clone();
        let key = crate::capacity::WorkId {
            kind: run.kind(),
            id: run.id.clone(),
        };
        let outcome = async {
            let generation = app
                .state::<Host>()
                .github_generations
                .lock()
                .map_err(|_| Failure::permanent("GitHub coordination unavailable."))?
                .get(&run.context.job.account_id)
                .copied()
                .unwrap_or(0);
            let mut native = Native {
                app: app.clone(),
                generation,
                cancelled,
                cancelled_read: Arc::new(AtomicBool::new(false)),
            };
            analyze(&mut native, &mut run).await
        }
        .await;
        if let Err(error) = outcome {
            crate::report(&app, error.message);
        }
        let host = app.state::<Host>();
        let saved = host
            .store
            .lock()
            .map_err(|_| "Thread storage unavailable.".to_string())
            .and_then(|store| finish_analysis_worker(&store, &host.ai, &key, &operation_id));
        if let Err(error) = saved {
            crate::report(&app, error);
            if let Err(error) = host.ai.persistence_failed(&operation_id) {
                crate::report(&app, error);
            }
        } else if let Err(error) = crate::capacity::Coordinator::pump(&app) {
            crate::report(&app, error);
        }
    });
}

fn finish_analysis_worker(
    store: &Store,
    capacity: &crate::capacity::Coordinator,
    key: &crate::capacity::WorkId,
    operation_id: &str,
) -> Result<(), String> {
    let saved = store
        .load_follow_ups()?
        .into_iter()
        .find(|r| r.id == key.id)
        .and_then(|r| r.analysis)
        .is_some_and(|op| op.id != operation_id || op.state != OperationState::Running);
    if !saved {
        return Err("Analysis outcome could not be persisted; capacity remains reserved.".into());
    }
    capacity.release(key, operation_id)
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
    fn read_failure(&self, error: github::ConnectionError) -> Failure {
        if error == github::ConnectionError::Configuration
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
            .map_err(|_| github::ConnectionError::Configuration)?;
            if snapshot.publication.is_none() && native.cancelled.load(Ordering::SeqCst) {
                native.cancelled_read.store(true, Ordering::SeqCst);
                return Err(github::ConnectionError::Configuration);
            }
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
        if accepting_analysis {
            let allowed = !host.quitting.load(Ordering::SeqCst)
                && !self.cancelled.load(Ordering::SeqCst)
                && generations.as_ref().is_some_and(|g| {
                    g.get(&run.context.job.account_id).copied().unwrap_or(0) == self.generation
                })
                && auth.as_ref().is_some_and(|a| {
                    a.account_session_allowed(&run.context.job.account_id)
                        .is_ok()
                });
            let result = run
                .result
                .take()
                .ok_or_else(|| Failure::permanent("Analysis result unavailable."))?;
            return complete_analysis(&store, run, Ok(result), allowed, self.now()?);
        }
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
            .review_pull(&repo, job.number)
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
        match &run.target {
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
        }
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

fn analysis_checkpoint(
    environment: &mut impl Environment,
    run: &FollowUp,
) -> Result<Observation, Failure> {
    let observation = environment.observe(run)?;
    if let Some(reason) = environment.gate(run, &observation)? {
        return Err(analysis_gate_failure(reason));
    }
    Ok(observation)
}

async fn analyze(native: &mut Native, run: &mut FollowUp) -> Result<(), Failure> {
    run.phase = Phase::Analyzing;
    native.save(run)?;
    let result = async {
        let mut worker = native.clone();
        let snapshot = run.clone();
        let (context, client, observation) = tauri::async_runtime::spawn_blocking(move || {
            let observation = worker.observe(&snapshot)?;
            worker.local(&snapshot)?;
            snapshot.validate_observed_trigger(&observation)?;
            let mut current = snapshot.clone();
            if let Observation::Mention { replies, .. } = &observation {
                current.discussion = replies.clone();
            }
            if current.manual_start {
                if let Observation::Owned(thread) = &observation {
                    *current.thread_mut().map_err(Failure::permanent)? = thread.clone();
                }
            }
            if let Some(reason) = worker.gate(&current, &observation)? {
                return Err(analysis_gate_failure(reason));
            }
            let client = worker.read_client(&current)?;
            let context = client
                .review_context(
                    &RemoteRepository {
                        id: current.context.job.repository_id.clone(),
                        name: current.context.job.repository_name.clone(),
                    },
                    current.context.job.number,
                    &current.context.job.head_sha,
                )
                .map_err(|e| worker.read_failure(e))?;
            if current
                .context
                .job
                .observed_base_sha
                .as_ref()
                .is_some_and(|base| base != &context.pull.base_sha)
            {
                return Err(Failure::permanent(
                    "The reviewed base changed; wait for a new review.",
                ));
            }
            Ok((context, Arc::new(client), observation))
        })
        .await
        .map_err(|_| Failure::permanent("Follow-up preparation failed."))??;
        match observation {
            Observation::Owned(thread) => *run.thread_mut().map_err(Failure::permanent)? = thread,
            Observation::Mention { replies, .. } => run.discussion = replies,
        }
        let expected_base = context.pull.base_sha.clone();
        run.context.job.observed_base_sha = Some(expected_base.clone());
        native.save(run)?;
        let local_native = native.clone();
        let local_run = run.clone();
        let before_native = native.clone();
        let before_run = run.clone();
        let request = runtime::Request {
            task: ReplyTask {
                conversation: run.input(),
                trigger_id: run.trigger_id.clone(),
                feedback: run.context.feedback.clone(),
                owner_agent_id: run.assessment_owner().into(),
            },
            context,
            client,
            repository_name: run.context.job.repository_name.clone(),
            selection: run.context.selection.clone(),
            local_gate: Arc::new(move || local_native.local(&local_run)),
            before_send: Arc::new(move || {
                let mut native = before_native.clone();
                analysis_checkpoint(&mut native, &before_run).map(|_| ())
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
        let mut final_run = run.clone();
        final_run.result = Some(result.clone());
        tauri::async_runtime::spawn_blocking(move || {
            analysis_checkpoint(&mut final_native, &final_run).map(|_| ())
        })
        .await
        .map_err(|_| Failure::permanent("Final follow-up verification failed."))??;
        result.reviewed_base_sha = Some(expected_base);
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
            let host = native.app.state::<Host>();
            let store = host
                .store
                .lock()
                .map_err(|_| Failure::permanent("Thread storage unavailable."))?;
            complete_analysis(&store, run, Err(error), false, native.now()?)
        }
    }
}

pub(crate) fn complete_analysis(
    store: &Store,
    run: &mut FollowUp,
    outcome: Result<crate::review::ReviewResult<ReplyOutput>, Failure>,
    account_allowed: bool,
    now: i64,
) -> Result<(), Failure> {
    let current = store
        .load_follow_ups()
        .map_err(Failure::permanent)?
        .into_iter()
        .find(|r| r.id == run.id)
        .ok_or_else(|| Failure::permanent("Thread follow-up disappeared."))?;
    if current.analysis.as_ref().map(|o| &o.id) != run.analysis.as_ref().map(|o| &o.id) {
        return Err(Failure::permanent(
            "A newer analysis attempt owns this follow-up.",
        ));
    }
    if current.has_provider_work() || run.has_provider_work() {
        return match outcome {
            Err(error) => Err(error),
            Ok(_) => Err(Failure::permanent(
                "Provider reply work cannot be overwritten by an analysis result.",
            )),
        };
    }
    if outcome.is_ok()
        && (current.result.is_some()
            || current.error.is_some()
            || current.analysis.as_ref().is_some_and(|operation| {
                matches!(
                    operation.state,
                    OperationState::Completed
                        | OperationState::Failed
                        | OperationState::ManualRetry
                )
            }))
    {
        return Err(Failure::permanent(
            "An existing analysis outcome cannot be replaced by another successful result.",
        ));
    }
    run.analysis = current.analysis;
    if !current.cancelled
        && run
            .analysis
            .as_mut()
            .is_some_and(|op| crate::capacity::interrupted(op, outcome.as_ref().err(), now))
    {
        run.result = None;
        run.error = None;
        run.phase = Phase::WaitingStart;
        return save_to_store(store, run).map_err(Failure::permanent);
    }
    let outcome = outcome.and_then(|result| {
        let mut checked = run.clone();
        checked.result = Some(result.clone());
        crate::feedback::owned_reply_assessment(&checked)?;
        super::validate_analysis_commit(store, &checked, account_allowed)?;
        Ok(result)
    });
    match outcome {
        Ok(result) => {
            run.phase = match result.output.decision {
                ReplyDecision::Reply => Phase::WaitingPublication,
                ReplyDecision::Quiet => Phase::Quiet,
                ReplyDecision::HumanInputRequired => Phase::HumanInputRequired,
            };
            run.result = Some(result);
            let op = run
                .analysis
                .as_mut()
                .ok_or_else(|| Failure::permanent("Analysis operation missing."))?;
            op.state = OperationState::Completed;
            op.next_attempt_at = None;
            op.failure = None;
            op.ai_attempt = None;
            op.interruption = None;
            run.error = None;
            save_to_store(store, run).map_err(Failure::permanent)
        }
        Err(error) => {
            run.cancelled |= current.cancelled;
            run.result = None;
            run.phase = Phase::Stopped;
            run.error = Some(error.message.clone());
            run.analysis
                .as_mut()
                .ok_or_else(|| Failure::permanent("Analysis operation missing."))?
                .fail(&error.monitoring(), now);
            save_to_store(store, run).map_err(Failure::permanent)?;
            Err(error)
        }
    }
}

fn analysis_gate_failure(reason: String) -> Failure {
    if reason == Failure::cancelled().message {
        Failure::cancelled()
    } else {
        Failure::permanent(reason)
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

#[cfg(test)]
mod conversation_tests;
#[cfg(test)]
mod tests;
