use super::*;
use crate::{
    github::threads::{Comment, Thread},
    monitoring::{JobOperation, OperationFailure, QueueJob},
    publication::{Publication, Receipt, RemoteState},
    review::{Failure, ReviewResult, Selection},
    storage::Settings,
};
use serde_json::json;

pub(crate) fn fixture(count: usize, capacity: u32, automatic: bool) -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut settings = Settings {
        capacity,
        ..Settings::default()
    };
    settings.defaults.automatic_agent_start = automatic;
    let mut jobs = Vec::new();
    for i in 1..=count {
        let repo = format!("bbbbbbbb-bbbb-4bbb-8bbb-{i:012}");
        let agent = format!("aaaaaaaa-aaaa-4aaa-8aaa-{i:012}");
        let assignment = format!("cccccccc-cccc-4ccc-8ccc-{i:012}");
        let account = (100 + i).to_string();
        settings.agents.push(
            serde_json::from_value(json!({
                "id":agent,"name":format!("Agent {i}"),"model":"fixture-model",
                "ai_account":{"provider":"copilot","account_id":(200+i).to_string()},
                "prompt":"Review correctness.","signature":"fixture"
            }))
            .unwrap(),
        );
        settings.repositories.push(serde_json::from_value(json!({
            "id":repo,"provider":"github","name":format!("fixture/repo-{i}"),"enabled":true,
            "provider_account_id":account,"provider_repository_id":(300+i).to_string(),
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":assignment,"agent_id":agent,"schedule":settings.defaults.schedule,"comment":true}]
        })).unwrap());
        jobs.push(serde_json::from_value(json!({
            "assignment_id":assignment,"provider":"github","account_id":account,"account_login":"actor",
            "configuration_id":repo,"repository_id":(300+i).to_string(),"repository_name":format!("fixture/repo-{i}"),
            "pull_request_id":i.to_string(),"number":i,"title":"Review","head_sha":"a".repeat(40),
            "observed_base_sha":"b".repeat(40),"trigger_policy":"[[\"11\"],true]",
            "author_id":"11","author_login":"author","watched_author":true,"requested_reviewer":false,
            "waiting":"human_start","detected_at":100,
            "work":{"id":format!("normal-{i}"),"item_id":format!("item-{i}"),"iteration_id":format!("iteration-{i}"),
                "iteration":1,"agent_id":agent,"enqueue_order":i,"pass_ordinal":1,"trigger":"admission",
                "admission":{"watched_author":true,"all_authors":false,"requested_reviewer":false}}
        })).unwrap());
    }
    store.save_settings(&settings).unwrap();
    store.save_queue(&jobs).unwrap();
    (root, store)
}

fn output() -> ReviewResult {
    serde_json::from_value(json!({
        "reviewed_base_sha":"b".repeat(40),
        "output":{"synopsis":"Review completed.","files":[],"findings":[],"decision":"machine_sign_off"},
        "session_id":"fixture","model":"fixture-model","runtime_version":"fixture",
        "input_tokens":1,"output_tokens":1,"tool_calls":1
    })).unwrap()
}

pub(crate) fn add_reply(store: &Store, order: u64) -> FollowUp {
    let settings = store.load_settings().unwrap();
    let mut job: QueueJob = store.load_queue().unwrap().remove(0);
    job.pull_request_id = "99".into();
    job.number = 99;
    job.work.as_mut().unwrap().id = "reply-origin".into();
    let mut jobs = store.load_queue().unwrap();
    jobs.push(job.clone());
    store.save_queue(&jobs).unwrap();
    let mut op = JobOperation::review(&job, 100);
    op.state = OperationState::Completed;
    let review = ReviewRun {
        feedback_context: None,
        key: review::key(&job, job.assignment_id.as_ref().unwrap()),
        assignment_id: job.assignment_id.clone().unwrap(),
        selection: Selection::resolve(&settings, &job, job.assignment_id.as_ref().unwrap())
            .unwrap(),
        job,
        operation: op,
        manual_start: false,
        trust_confirmed: true,
        phase: "Complete".into(),
        error: None,
        result: Some(output()),
    };
    let mut origin = Publication::new(review.clone(), false, true, 100).unwrap();
    origin.batch = Some(crate::publication::Batch {
        commit_id: "a".repeat(40),
        body: "Summary".into(),
        unmappable: vec![],
        comments: vec![crate::publication::InlineComment {
            path: "source.rs".into(),
            line: 1,
            side: "RIGHT".into(),
            body: "Root".into(),
        }],
    });
    origin.receipts.push(Receipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["100".into()],
    });
    origin.operation.state = OperationState::Completed;
    origin.phase = crate::publication::Phase::Published;
    store.save_reviews(&[review]).unwrap();
    store
        .save_publications(std::slice::from_ref(&origin))
        .unwrap();
    let root = Comment {
        id: "100".into(),
        body: "Root".into(),
        author_id: Some("101".into()),
        author_login: Some("actor".into()),
        reply_to: None,
        review_id: Some("42".into()),
        original_commit: Some("a".repeat(40)),
        created_at: "2026-09-30T00:00:00Z".into(),
        published_at: "2026-09-30T00:00:00Z".into(),
    };
    let reply = Comment {
        id: "102".into(),
        author_id: Some("11".into()),
        reply_to: Some("100".into()),
        body: "Why?".into(),
        ..root.clone()
    };
    let mut run = FollowUp::new(
        &origin,
        Thread {
            id: "thread".into(),
            resolved: false,
            can_reply: true,
            comments: vec![root, reply],
        },
    )
    .unwrap();
    run.enqueue_order = Some(order);
    run.enqueued_at = Some(100);
    run.reply_ordinal = Some(1);
    store.save_follow_ups(std::slice::from_ref(&run)).unwrap();
    run
}

fn dispatch(coordinator: &Coordinator, store: &Store, now: i64) -> Vec<Dispatch> {
    let batch = coordinator.dispatch(store, now).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    batch.dispatched
}

fn finish(
    coordinator: &Coordinator,
    store: &Store,
    work: Dispatch,
    error: Option<Failure>,
    now: i64,
) -> Vec<Dispatch> {
    let key = work.key();
    let id = match work {
        Dispatch::Review(run, _) => {
            review::host::complete(
                store,
                &run.operation.id,
                error.map_or_else(|| Ok(output()), Err),
                now,
            )
            .unwrap();
            run.operation.id
        }
        Dispatch::Reply(mut run, _) => {
            let result = error.map_or_else(|| Ok(serde_json::from_value(json!({
                "reviewed_base_sha":"b".repeat(40),
                "output":{"decision":"quiet","body":"","new_information":"","reason":"No reply needed.","evidence":[]},
                "session_id":"reply","model":"fixture-model","runtime_version":"fixture",
                "input_tokens":1,"output_tokens":1,"tool_calls":1
            })).unwrap()), Err);
            let expected_failure = result.as_ref().err().is_some_and(|e| !e.cancelled);
            let completed = follow_up::host::complete_analysis(store, &mut run, result, true, now);
            assert_eq!(completed.is_err(), expected_failure, "{completed:?}");
            run.analysis.as_ref().unwrap().id.clone()
        }
    };
    coordinator.release(&key, &id).unwrap();
    dispatch(coordinator, store, now)
}

#[test]
fn production_dispatch_enforces_four_of_seven_and_positive_limits_independent_of_resources() {
    assert_eq!(Settings::default().capacity, 4);
    for limit in [1, 4, 20, 31] {
        let (_root, store) = fixture(7, limit, true);
        let coordinator = Coordinator::default();
        let jobs = store.load_queue().unwrap();
        let workers = dispatch(&coordinator, &store, 100);
        let active = 7.min(limit as usize);
        assert_eq!(workers.len(), active);
        let snapshot = coordinator.snapshot(&store, 100).unwrap();
        assert_eq!(
            (snapshot.active, snapshot.waiting, snapshot.blocked),
            (active, 7 - active, 0)
        );
        assert!(dispatch(&coordinator, &store, 101).is_empty());
        assert_eq!(store.load_queue().unwrap(), jobs);
        assert!(store
            .load_reviews()
            .unwrap()
            .iter()
            .all(|r| r.operation.state == OperationState::Running));
    }
}

#[test]
fn repository_cadence_edits_preserve_running_snapshots_completed_dedup_and_immediate_capacity_drain(
) {
    let (_root, store) = fixture(3, 1, false);
    let coordinator = Coordinator::default();
    let mut workers = dispatch(&coordinator, &store, 100);
    assert_eq!(workers.len(), 1);
    let running = store.load_reviews().unwrap()[0].clone();
    let mut settings = store.load_settings().unwrap();
    settings.defaults.schedule = crate::policy::Schedule::Cron {
        expression: "0 9 * * MON-FRI".into(),
        timezone: "America/New_York".into(),
    };
    settings.repositories[0].overrides.schedule = Some(crate::policy::Schedule::Cron {
        expression: "0 0 1 * *".into(),
        timezone: "Asia/Tokyo".into(),
    });
    store.save_settings(&settings).unwrap();
    assert!(dispatch(&coordinator, &store, 101).is_empty());
    assert_eq!(store.load_reviews().unwrap()[0], running);
    let token = match &workers[0] {
        Dispatch::Review(_, token) => token,
        _ => unreachable!(),
    };
    assert!(
        !token.load(Ordering::SeqCst),
        "Cadence must not cancel running analysis."
    );
    workers = finish(&coordinator, &store, workers.remove(0), None, 102);
    assert_eq!(
        workers.len(),
        1,
        "Queued work drains without another cron poll."
    );
    let completed = store.load_reviews().unwrap()[0].clone();
    assert_eq!(completed.selection, running.selection);
    workers = finish(&coordinator, &store, workers.remove(0), None, 103);
    assert_eq!(workers.len(), 1);
    assert!(finish(&coordinator, &store, workers.remove(0), None, 104).is_empty());
    assert!(dispatch(&coordinator, &store, 10_000).is_empty());
    assert_eq!(store.load_reviews().unwrap().len(), 3);
    assert_eq!(store.load_reviews().unwrap()[0], completed);
}

#[test]
fn false_legacy_defaults_and_overrides_start_automatically_without_granting_provider_actions() {
    let (root, store) = fixture(2, 1, false);
    let mut settings = store.load_settings().unwrap();
    for repository in &mut settings.repositories {
        repository.overrides.automatic_agent_start = Some(false);
        repository.assignments[0].comment = false;
    }
    settings.agents[0].intelligence = serde_json::from_value(json!({
        "reasoning_effort":"high","context_tier":"long_context"
    }))
    .unwrap();
    store.save_settings(&settings).unwrap();
    let reopened = Store::new(root.path().into());
    let coordinator = Coordinator::default();
    let mut workers = dispatch(&coordinator, &reopened, 100);
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].key().id, "normal-1");
    let Dispatch::Review(run, _) = &workers[0] else {
        panic!("Normal review expected")
    };
    assert!(!run.manual_start);
    assert_eq!(
        run.selection.agent.intelligence,
        settings.agents[0].intelligence
    );
    let authority = &run.selection.configuration.as_ref().unwrap().authority;
    assert!(!authority.comment && !authority.approve && !authority.merge);
    assert_eq!(coordinator.snapshot(&reopened, 100).unwrap().waiting, 1);
    let next = finish(&coordinator, &reopened, workers.remove(0), None, 101);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].key().id, "normal-2");
    assert!(reopened.load_publications().unwrap().is_empty());
    assert!(reopened.load_actions().unwrap().effects.is_empty());
    assert!(reopened.load_monitoring_state().unwrap().health.is_empty());
    let saved = reopened.load_settings().unwrap();
    assert!(!saved.defaults.automatic_agent_start);
    assert!(saved
        .repositories
        .iter()
        .all(|r| r.overrides.automatic_agent_start == Some(false)));
}

#[test]
fn automatic_start_preserves_pause_disable_assignment_and_account_gates() {
    for gate in [
        "pause",
        "disabled",
        "unassigned",
        "missing_ai_account",
        "account_access",
    ] {
        let (_root, store) = fixture(1, 1, false);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].overrides.automatic_agent_start = Some(false);
        let mut jobs = store.load_queue().unwrap();
        match gate {
            "pause" => store.save_automation(&Automation { paused: true }).unwrap(),
            "disabled" => settings.repositories[0].enabled = false,
            "unassigned" => settings.repositories[0].assignments.clear(),
            "missing_ai_account" => settings.agents[0].ai_account = None,
            "account_access" => {
                jobs[0].waiting = crate::monitoring::WAITING_ACCOUNT_DISCONNECTED.into()
            }
            _ => unreachable!(),
        }
        store.save_settings(&settings).unwrap();
        store.save_queue(&jobs).unwrap();
        let coordinator = Coordinator::default();
        assert!(dispatch(&coordinator, &store, 100).is_empty(), "{gate}");
        let snapshot = coordinator.snapshot(&store, 100).unwrap();
        assert_eq!(snapshot.active, 0, "{gate}");
        if gate == "pause" {
            assert!(snapshot.paused);
            assert_eq!(snapshot.waiting, 1);
        } else if gate == "unassigned" {
            assert!(snapshot.work.is_empty());
        } else {
            assert_eq!(snapshot.blocked, 1, "{gate}");
            assert!(snapshot.work[0].reason.is_some(), "{gate}");
        }
        assert_eq!(store.load_queue().unwrap(), jobs);
        assert!(store.load_reviews().unwrap().is_empty());
    }
    let (_root, store) = fixture(1, 1, false);
    let saved = store.load_settings().unwrap();
    let mut invalid = saved.clone();
    invalid.agents.clear();
    assert_eq!(
        store.save_settings(&invalid).unwrap_err(),
        "The selected agent no longer exists. Choose a local agent."
    );
    assert_eq!(store.load_settings().unwrap(), saved);
}

#[test]
fn mixed_fifo_skips_blocked_work_and_completion_refills_without_a_poll() {
    let (_root, store) = fixture(7, 1, true);
    let reply = add_reply(&store, 1);
    let mut jobs = store.load_queue().unwrap();
    jobs[0].waiting = crate::monitoring::WAITING_ACCOUNT_DISCONNECTED.into();
    for job in &mut jobs {
        job.work.as_mut().unwrap().enqueue_order += 1;
    }
    store.save_queue(&jobs).unwrap();
    let coordinator = Coordinator::default();
    let mut workers = dispatch(&coordinator, &store, 100);
    assert_eq!(
        workers[0].key(),
        WorkId {
            kind: Kind::Reply,
            id: reply.id
        }
    );
    let blocked = coordinator
        .snapshot(&store, 100)
        .unwrap()
        .work
        .into_iter()
        .find(|w| w.key.id == "normal-1")
        .unwrap();
    assert_eq!(blocked.enqueue_order, 2);
    assert_eq!(
        blocked.reason.unwrap(),
        "Review eligibility or repository configuration changed."
    );
    let refilled = finish(&coordinator, &store, workers.remove(0), None, 101);
    assert_eq!(refilled.len(), 1);
    assert_eq!(refilled[0].key().id, "normal-2");
    assert!(!store
        .load_monitoring_state()
        .unwrap()
        .health
        .values()
        .any(|h| h.last_attempt.is_some()));
}

#[test]
fn four_shared_slots_run_mixed_jobs_and_leave_three_waiters() {
    let (_root, store) = fixture(6, 4, true);
    let reply = add_reply(&store, 0);
    let coordinator = Coordinator::default();
    let workers = dispatch(&coordinator, &store, 100);
    assert_eq!(workers.len(), 4);
    assert_eq!(
        workers[0].key(),
        WorkId {
            kind: Kind::Reply,
            id: reply.id
        }
    );
    assert_eq!(
        workers
            .iter()
            .filter(|w| matches!(w, Dispatch::Review(_, _)))
            .count(),
        3
    );
    let snapshot = coordinator.snapshot(&store, 100).unwrap();
    assert_eq!(
        (snapshot.active, snapshot.waiting, snapshot.blocked),
        (4, 3, 0)
    );
}

#[test]
fn legacy_manual_waits_automatically_resume_after_restart_and_long_capacity_wait() {
    let (_root, store) = fixture(7, 1, false);
    let coordinator = Coordinator::default();
    for index in 1..=7 {
        let mut run =
            review::host::request(&store, &format!("normal-{index}"), false, 100).unwrap();
        run.selection.policy.automatic_agent_start = false;
        run.operation.state = OperationState::Interrupted;
        let mut runs = store.load_reviews().unwrap();
        let saved = runs
            .iter_mut()
            .find(|r| r.operation.id == run.operation.id)
            .unwrap();
        *saved = run;
        store.save_reviews(&runs).unwrap();
    }
    let ids: Vec<_> = store
        .load_reviews()
        .unwrap()
        .iter()
        .map(|r| r.operation.id.clone())
        .collect();
    review::restore(&store).unwrap();
    let workers = dispatch(&coordinator, &store, 10_000);
    assert_eq!(workers.len(), 1);
    let runs = store.load_reviews().unwrap();
    assert_eq!(
        runs.iter()
            .map(|r| r.operation.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert!(runs.iter().all(|r| !r.manual_start));
    assert_eq!(runs[0].operation.initial_attempt_at, 10_000);
    assert_eq!(runs[0].operation.retry_deadline, 10_900);
    assert_eq!(runs[1].operation.attempt_count, 0);
    let reply = add_reply(&store, 8);
    let requested = follow_up::host::request_analysis(&store, &reply.id, true, 100).unwrap();
    let id = requested.analysis.unwrap().id;
    follow_up::restore(&store).unwrap();
    let prepared = follow_up::host::prepare_dispatch(&store, &reply.id, 50_000).unwrap();
    assert_eq!(prepared.analysis.as_ref().unwrap().id, id);
    assert_eq!(prepared.analysis.as_ref().unwrap().retry_deadline, 50_900);
    assert!(prepared.manual_start);
}

#[test]
fn rapid_concurrent_requests_share_one_reservation_owner() {
    let (_root, store) = fixture(7, 4, false);
    let store = Arc::new(Mutex::new(store));
    let coordinator = Arc::new(Coordinator::default());
    let workers = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for _ in 1..=7 {
            let store = store.clone();
            let coordinator = coordinator.clone();
            let workers = workers.clone();
            scope.spawn(move || {
                let store = store.lock().unwrap();
                workers
                    .lock()
                    .unwrap()
                    .extend(dispatch(&coordinator, &store, 100));
            });
        }
    });
    let store = store.lock().unwrap();
    assert_eq!(workers.lock().unwrap().len(), 4);
    assert_eq!(coordinator.snapshot(&store, 100).unwrap().active, 4);
    assert_eq!(store.load_reviews().unwrap().len(), 4);
}

#[test]
fn pause_is_durable_and_rapid_resume_cannot_reuse_stopping_slots() {
    let (_root, store) = fixture(7, 4, true);
    let coordinator = Coordinator::default();
    let mut workers = dispatch(&coordinator, &store, 100);
    let jobs = store.load_queue().unwrap();
    store.save_automation(&Automation { paused: true }).unwrap();
    assert!(dispatch(&coordinator, &store, 101).is_empty());
    let snapshot = coordinator.snapshot(&store, 101).unwrap();
    assert_eq!((snapshot.active, snapshot.stopping), (4, 4));
    for work in &workers {
        let token = match work {
            Dispatch::Review(_, token) | Dispatch::Reply(_, token) => token,
        };
        assert!(token.load(Ordering::SeqCst));
    }
    store
        .save_automation(&Automation { paused: false })
        .unwrap();
    assert!(dispatch(&coordinator, &store, 102).is_empty());
    let replacement = finish(
        &coordinator,
        &store,
        workers.remove(0),
        Some(Failure::cancelled()),
        103,
    );
    assert_eq!(replacement.len(), 1);
    assert_eq!(replacement[0].key().id, "normal-1");
    let runs = store.load_reviews().unwrap();
    assert_eq!(
        runs[0].operation.attempt_count, 1,
        "Intentional cancellation consumed no retry."
    );
    assert!(runs[0].result.is_none());
    assert_eq!(store.load_queue().unwrap(), jobs);
}

#[test]
fn capacity_reduction_keeps_oldest_active_and_counts_teardown_until_finished() {
    let (_root, store) = fixture(7, 4, true);
    let coordinator = Coordinator::default();
    let mut workers = dispatch(&coordinator, &store, 100);
    let mut settings = store.load_settings().unwrap();
    settings.capacity = 1;
    store.save_settings(&settings).unwrap();
    assert!(dispatch(&coordinator, &store, 101).is_empty());
    assert_eq!(coordinator.snapshot(&store, 101).unwrap().stopping, 3);
    assert!(!match &workers[0] {
        Dispatch::Review(_, token) => token.load(Ordering::SeqCst),
        _ => unreachable!(),
    });
    while workers.len() > 1 {
        assert!(finish(
            &coordinator,
            &store,
            workers.pop().unwrap(),
            Some(Failure::cancelled()),
            102
        )
        .is_empty());
    }
    let refilled = finish(&coordinator, &store, workers.remove(0), None, 103);
    assert_eq!(refilled[0].key().id, "normal-2");
    assert_eq!(coordinator.snapshot(&store, 103).unwrap().active, 1);
    assert_eq!(
        store.load_reviews().unwrap()[0].operation.state,
        OperationState::Completed
    );
}

#[test]
fn real_failure_concurrent_with_pause_is_not_refunded_and_deadline_is_not_reset() {
    let (_root, store) = fixture(1, 4, true);
    let coordinator = Coordinator::default();
    let work = dispatch(&coordinator, &store, 100).remove(0);
    store.save_automation(&Automation { paused: true }).unwrap();
    dispatch(&coordinator, &store, 101);
    finish(&coordinator, &store, work, Some(Failure::timeout()), 102);
    let failed = store.load_reviews().unwrap().remove(0);
    assert_eq!(failed.operation.attempt_count, 1);
    assert_eq!(failed.operation.failure, Some(OperationFailure::Timeout));
    assert_eq!(failed.operation.retry_deadline, 1000);
    store
        .save_automation(&Automation { paused: false })
        .unwrap();
    let work = dispatch(
        &coordinator,
        &store,
        failed.operation.next_attempt_at.unwrap(),
    )
    .remove(0);
    store.save_automation(&Automation { paused: true }).unwrap();
    dispatch(&coordinator, &store, 120);
    finish(&coordinator, &store, work, Some(Failure::cancelled()), 121);
    let stopped = store.load_reviews().unwrap().remove(0);
    assert_eq!(stopped.operation.attempt_count, 1);
    assert_eq!(stopped.operation.retry_deadline, 1000);
    assert_eq!(stopped.operation.failure, Some(OperationFailure::Timeout));
    store
        .save_automation(&Automation { paused: false })
        .unwrap();
    let batch = coordinator.dispatch(&store, 1001).unwrap();
    assert!(batch.dispatched.is_empty());
    assert_eq!(
        store.load_reviews().unwrap()[0].operation.state,
        OperationState::ManualRetry
    );
}

#[test]
fn restart_after_pause_refunds_only_signalled_attempts_and_never_restores_in_process_workers() {
    let (_root, store) = fixture(2, 4, true);
    let coordinator = Coordinator::default();
    dispatch(&coordinator, &store, 100);
    let ids: Vec<_> = store
        .load_reviews()
        .unwrap()
        .iter()
        .map(|r| r.operation.id.clone())
        .collect();
    assert!(dispatch(&coordinator, &store, 101).is_empty());
    assert!(store
        .load_reviews()
        .unwrap()
        .iter()
        .all(|r| r.operation.state == OperationState::Running));
    store.save_automation(&Automation { paused: true }).unwrap();
    dispatch(&coordinator, &store, 102);
    review::restore(&store).unwrap();
    let new_process = Coordinator::default();
    assert!(dispatch(&new_process, &store, 5000).is_empty());
    assert!(store
        .load_reviews()
        .unwrap()
        .iter()
        .all(|r| r.operation.attempt_count == 0));
    store
        .save_automation(&Automation { paused: false })
        .unwrap();
    assert_eq!(dispatch(&new_process, &store, 5000).len(), 2);
    assert_eq!(
        store
            .load_reviews()
            .unwrap()
            .iter()
            .map(|r| r.operation.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn pause_discards_successful_but_uncommitted_reply_output_and_preserves_original_receipts() {
    let (_root, store) = fixture(1, 1, true);
    let reply = add_reply(&store, 0);
    let before = store.load_publications().unwrap();
    let coordinator = Coordinator::default();
    let worker = dispatch(&coordinator, &store, 100).remove(0);
    assert_eq!(worker.key().id, reply.id);
    store.save_automation(&Automation { paused: true }).unwrap();
    dispatch(&coordinator, &store, 101);
    finish(&coordinator, &store, worker, None, 102);
    let restored = store.load_follow_ups().unwrap().remove(0);
    assert_eq!(restored.analysis.as_ref().unwrap().attempt_count, 0);
    assert!(restored.result.is_none());
    assert_eq!(restored.reply_ordinal, Some(1));
    assert_eq!(restored.enqueue_order, Some(0));
    assert_eq!(store.load_publications().unwrap(), before);
}

#[test]
fn failed_dispatch_does_not_lose_previously_reserved_workers() {
    let (_root, store) = fixture(2, 4, true);
    review::host::request(&store, "normal-2", true, 100).unwrap();
    let mut settings = store.load_settings().unwrap();
    settings.agents[1].prompt = "Changed input.".into();
    store.save_settings(&settings).unwrap();
    let coordinator = Coordinator::default();
    let batch = coordinator.dispatch(&store, 101).unwrap();
    assert_eq!(batch.dispatched.len(), 1);
    assert_eq!(batch.errors.len(), 1);
    assert_eq!(coordinator.snapshot(&store, 101).unwrap().active, 1);
    assert_eq!(
        store.load_reviews().unwrap()[0].operation.state,
        OperationState::Failed
    );
}

#[test]
fn future_final_and_mention_adapters_reserve_the_same_pool_as_normal_and_reply_workers() {
    let (_root, store) = fixture(7, 4, true);
    let coordinator = Coordinator::default();
    let mut reserved = Vec::new();
    for (index, kind) in [(1, Kind::PrimaryFinal), (2, Kind::Mention)] {
        let work = Work {
            key: WorkId {
                kind,
                id: format!("future-{index}"),
            },
            enqueue_order: index,
            state: "waiting",
            reason: None,
        };
        let run = coordinator
            .reserve(&store, &work, |_| {
                let mut run = review::host::request(&store, &format!("normal-{index}"), true, 100)?;
                run.operation.operation_type = if kind == Kind::PrimaryFinal {
                    "primary_final_review"
                } else {
                    "mention_analysis"
                }
                .into();
                run.operation.begin_ai_attempt(100)?;
                let mut reviews = store.load_reviews()?;
                *reviews
                    .iter_mut()
                    .find(|r| r.operation.id == run.operation.id)
                    .unwrap() = run.clone();
                store.save_reviews(&reviews)?;
                Ok((run.operation.id.clone(), run))
            })
            .unwrap()
            .unwrap();
        reserved.push((work, run));
    }
    // The adapter owns these work IDs; normal selection cannot run their already-running attempts.
    let batch = coordinator.dispatch(&store, 100).unwrap();
    assert_eq!(batch.dispatched.len(), 2);
    assert_eq!(coordinator.snapshot(&store, 100).unwrap().active, 4);
    let more = Work {
        key: WorkId {
            kind: Kind::PrimaryFinal,
            id: "another-final".into(),
        },
        enqueue_order: 8,
        state: "waiting",
        reason: None,
    };
    assert!(coordinator
        .reserve::<()>(&store, &more, |_| panic!("No fifth slot"))
        .unwrap()
        .is_none());
    for (work, run) in reserved {
        review::host::complete(&store, &run.operation.id, Ok(output()), 101).unwrap();
        coordinator.release(&work.key, &run.operation.id).unwrap();
    }
    assert_eq!(coordinator.snapshot(&store, 101).unwrap().active, 2);
}

#[test]
fn disabled_first_request_retains_order_and_new_work_joins_the_tail() {
    let (_root, store) = fixture(3, 1, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].enabled = false;
    store.save_settings(&settings).unwrap();
    let coordinator = Coordinator::default();
    let first = dispatch(&coordinator, &store, 100).remove(0);
    assert_eq!(first.key().id, "normal-2");
    settings.repositories[0].enabled = true;
    store.save_settings(&settings).unwrap();
    let mut jobs = store.load_queue().unwrap();
    let mut new = jobs[2].clone();
    new.pull_request_id = "4".into();
    new.number = 4;
    new.work.as_mut().unwrap().id = "new-tail".into();
    new.work.as_mut().unwrap().enqueue_order = store.allocate_enqueue_order().unwrap();
    assert_eq!(new.work.as_ref().unwrap().enqueue_order, 4);
    jobs.push(new);
    store.save_queue(&jobs).unwrap();
    let oldest = finish(&coordinator, &store, first, None, 102).remove(0);
    assert_eq!(oldest.key().id, "normal-1");
    let next = finish(&coordinator, &store, oldest, None, 103).remove(0);
    assert_eq!(next.key().id, "normal-3");
    assert_eq!(
        finish(&coordinator, &store, next, None, 104)[0].key().id,
        "new-tail"
    );
}

#[test]
fn legacy_trust_waits_resume_under_saved_assignment_without_a_confirmation() {
    let (root, store) = fixture(2, 4, true);
    let mut jobs = store.load_queue().unwrap();
    for job in &mut jobs {
        job.waiting = crate::monitoring::WAITING_TRUST_CONFIRMATION.into();
        job.watched_author = false;
        job.author_id = Some("unwatched-author".into());
    }
    store.save_queue(&jobs).unwrap();
    let restored = Store::new(root.path().into());
    let coordinator = Coordinator::default();
    let workers = dispatch(&coordinator, &restored, 100);
    assert_eq!(workers.len(), 2);
    assert!(restored
        .load_reviews()
        .unwrap()
        .iter()
        .all(|run| !run.trust_confirmed
            && !run.manual_start
            && run.operation.state == OperationState::Running));
}

#[test]
fn reply_failure_racing_pause_preserves_its_real_budget_and_original_archive() {
    let (_root, store) = fixture(1, 1, true);
    add_reply(&store, 0);
    let origin = store.load_reviews().unwrap();
    let coordinator = Coordinator::default();
    let worker = dispatch(&coordinator, &store, 100).remove(0);
    store.save_automation(&Automation { paused: true }).unwrap();
    dispatch(&coordinator, &store, 101);
    finish(&coordinator, &store, worker, Some(Failure::timeout()), 102);
    let failed = store.load_follow_ups().unwrap().remove(0);
    assert_eq!(failed.analysis.as_ref().unwrap().attempt_count, 1);
    assert_eq!(
        failed.analysis.as_ref().unwrap().failure,
        Some(OperationFailure::Timeout)
    );
    assert_eq!(store.load_reviews().unwrap(), origin);
    let next = failed.analysis.as_ref().unwrap().next_attempt_at.unwrap();
    store
        .save_automation(&Automation { paused: false })
        .unwrap();
    let worker = dispatch(&coordinator, &store, next).remove(0);
    store.save_automation(&Automation { paused: true }).unwrap();
    dispatch(&coordinator, &store, next + 1);
    finish(
        &coordinator,
        &store,
        worker,
        Some(Failure::cancelled()),
        next + 2,
    );
    let stopped = store.load_follow_ups().unwrap().remove(0);
    assert_eq!(stopped.analysis.as_ref().unwrap().attempt_count, 1);
    assert_eq!(stopped.analysis.as_ref().unwrap().retry_deadline, 1000);
    assert!(stopped.result.is_none());
    assert_eq!(store.load_reviews().unwrap(), origin);
}

#[test]
fn pause_gates_real_poll_admission_and_corrupt_pause_storage_never_enables_work() {
    let (root, store) = fixture(1, 4, true);
    let coordinator = Coordinator::default();
    let mut monitor = crate::monitoring::Monitor::restore(&store).unwrap();
    store.save_automation(&Automation { paused: true }).unwrap();
    assert!(monitor
        .prepare_checks(&store, 100, true)
        .unwrap()
        .is_empty());
    assert!(dispatch(&coordinator, &store, 100).is_empty());
    assert!(publication_gate(&store).is_err());
    std::fs::write(root.path().join("state/automation.json"), b"{broken").unwrap();
    assert!(coordinator.dispatch(&store, 101).is_err());
    assert!(monitor.prepare_checks(&store, 101, true).is_err());
    assert!(publication_gate(&store).is_err());
    assert!(store.load_reviews().unwrap().is_empty());
}
