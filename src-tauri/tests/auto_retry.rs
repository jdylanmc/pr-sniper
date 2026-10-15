use pr_sniper_lib::{
    backpressure::Origin,
    capacity,
    github::ConnectionError,
    monitoring::{
        AccountAvailability, FailureClass, JobOperation, Monitor, MonitoringError,
        OperationFailure, OperationState, QueueJob, RESUME_COOLDOWNS_SECONDS,
    },
    queue::{self, State},
    review::{self, Failure, ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::json;

const REPO: &str = "00000000-0000-4000-8000-000000000001";
const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "00000000-0000-4000-8000-000000000003";

fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":false}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"stored"}]
    })).unwrap()
}

fn job() -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"operator",
        "configuration_id":REPO,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"1","number":1,"title":"Change","head_sha":"a".repeat(40),"observed_base_sha":"b".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"human_start","detected_at":100
    })).unwrap()
}

fn failure(kind: OperationFailure, retry_after_seconds: Option<i64>) -> Failure {
    Failure {
        cancelled: false,
        kind,
        message: "Failed".into(),
        retry_after_seconds,
        origin: Origin::Github,
        duck_setup: false,
        resume: Default::default(),
    }
}

/// Runs one attempt at its due time and fails it.
fn attempt(operation: &mut JobOperation, error: &Failure) -> i64 {
    let now = operation.next_attempt_at.unwrap();
    operation.begin_attempt(now).unwrap();
    operation.fail(&error.monitoring(), now);
    now
}

#[test]
fn transient_failures_back_off_and_park_on_the_third_failure() {
    for kind in [
        OperationFailure::Timeout,
        OperationFailure::Network,
        OperationFailure::Provider,
    ] {
        let mut operation = JobOperation::review(&job(), 1_000);
        let error = failure(kind.clone(), None);
        let first = attempt(&mut operation, &error);
        assert_eq!(operation.state, OperationState::Queued);
        let first_delay = operation.next_attempt_at.unwrap() - first;
        assert!(first_delay > 0);
        assert_eq!(operation.failure_class(), None);
        let second = attempt(&mut operation, &error);
        assert_eq!(operation.state, OperationState::Queued);
        assert!(operation.next_attempt_at.unwrap() - second > first_delay);
        attempt(&mut operation, &error);
        assert_eq!(operation.state, OperationState::ManualRetry);
        assert_eq!(operation.attempt_count, 3);
        assert_eq!(operation.next_attempt_at, None);
        assert_eq!(operation.failure, Some(kind.clone()));
        assert_eq!(operation.failure_class(), Some(kind.class()));
        assert!(operation.begin_attempt(u32::MAX as i64).is_err());
    }
}

#[test]
fn rate_limits_are_backpressure_that_never_spend_attempts_or_park() {
    for (origin, retry_after) in [
        (Origin::Github, Some(600)),
        (Origin::Github, Some(3_600)),
        (Origin::Copilot, None),
        (Origin::Copilot, Some(5)),
    ] {
        let mut operation = JobOperation::review(&job(), 1_000);
        let mut now = 1_000;
        for _ in 0..10 {
            operation.begin_attempt(now).unwrap();
            let mut error = failure(OperationFailure::RateLimited, retry_after);
            error.origin = origin;
            operation.fail(&error.monitoring(), now);
            assert_eq!(operation.state, OperationState::Queued);
            assert_eq!(operation.attempt_count, 0);
            assert_eq!(operation.failure, None);
            assert_eq!(operation.failure_class(), None);
            assert!(!operation.parked());
            let until = operation.next_attempt_at.unwrap();
            assert_eq!(operation.backpressure_until, Some(until));
            if let Some(stated) = retry_after {
                assert_eq!(until, now + stated);
            }
            assert!(operation.begin_attempt(until - 1).is_err());
            now = until;
        }
    }
}

#[test]
fn permanent_failures_park_immediately() {
    for kind in [OperationFailure::Permanent, OperationFailure::Superseded] {
        let mut operation = JobOperation::review(&job(), 1_000);
        attempt(&mut operation, &failure(kind, None));
        assert_eq!(operation.state, OperationState::Failed);
        assert_eq!(operation.attempt_count, 1);
        assert_eq!(operation.next_attempt_at, None);
        assert_eq!(operation.failure_class(), Some(FailureClass::Other));
    }
}

#[test]
fn credential_and_permission_failures_park_immediately_with_a_class() {
    for (error, expected) in [
        (ConnectionError::SignedOut, FailureClass::Credentials),
        (ConnectionError::WrongIdentity, FailureClass::Credentials),
        (ConnectionError::MissingScope, FailureClass::Credentials),
        (ConnectionError::ScopeUnverified, FailureClass::Credentials),
        (
            ConnectionError::OrganizationPolicyDeniedWithMissingScope,
            FailureClass::Credentials,
        ),
        (
            ConnectionError::ProviderRejectedStatus(401),
            FailureClass::Credentials,
        ),
        (
            ConnectionError::MissingReadPermission,
            FailureClass::Permission,
        ),
        (
            ConnectionError::RepositoryUnavailable,
            FailureClass::Permission,
        ),
        (
            ConnectionError::OrganizationPolicyDenied,
            FailureClass::Permission,
        ),
        (
            ConnectionError::ProviderRejectedStatus(403),
            FailureClass::Permission,
        ),
        (ConnectionError::Network, FailureClass::Network),
        (ConnectionError::ProviderFailure, FailureClass::Other),
    ] {
        let mut operation = JobOperation::review(&job(), 1_000);
        attempt(&mut operation, &Failure::from(error));
        let transient = matches!(
            error,
            ConnectionError::Network | ConnectionError::ProviderFailure
        );
        assert_eq!(
            operation.state,
            if transient {
                OperationState::Queued
            } else {
                OperationState::Failed
            },
            "{error:?}"
        );
        if !transient {
            assert_eq!(operation.failure_class(), Some(expected), "{error:?}");
        }
        assert_eq!(operation.failure.as_ref().unwrap().class(), expected);
    }
}

#[test]
fn success_and_explicit_retry_start_from_a_clean_budget() {
    let mut operation = JobOperation::review(&job(), 1_000);
    attempt(&mut operation, &failure(OperationFailure::Timeout, None));
    attempt(&mut operation, &failure(OperationFailure::Timeout, None));
    let now = operation.next_attempt_at.unwrap();
    operation.begin_attempt(now).unwrap();
    operation.state = OperationState::Completed;
    operation.failure = None;
    assert_eq!(operation.failure_class(), None);
    assert!(!operation.parked());

    let mut parked = JobOperation::review(&job(), 1_000);
    for _ in 0..3 {
        attempt(&mut parked, &failure(OperationFailure::Network, None));
    }
    assert!(parked.parked());
    let retried = JobOperation::review(&job(), 5_000);
    assert_eq!(retried.attempt_count, 0);
    assert_eq!(retried.failure, None);
    assert_eq!(retried.state, OperationState::Queued);
}

#[test]
fn persisted_operations_load_across_the_new_failure_kinds() {
    let mut operation = JobOperation::review(&job(), 1_000);
    attempt(&mut operation, &Failure::from(ConnectionError::SignedOut));
    let stored = serde_json::to_value(&operation).unwrap();
    assert_eq!(stored["failure"], "credentials");
    assert_eq!(stored["state"], "failed");
    let restored: JobOperation = serde_json::from_value(stored).unwrap();
    assert_eq!(restored, operation);
    let mut legacy = serde_json::to_value(JobOperation::review(&job(), 1_000)).unwrap();
    legacy["state"] = json!("manual_retry");
    legacy["failure"] = json!("timeout");
    legacy["attempt_count"] = json!(4);
    let legacy: JobOperation = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.failure_class(), Some(FailureClass::Network));
}

fn parked_review(failure: OperationFailure, state: OperationState) -> ReviewRun {
    let job = job();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = state;
    operation.attempt_count = 3;
    operation.failure = Some(failure);
    operation.next_attempt_at = None;
    ReviewRun {
        mode: pr_sniper_lib::review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: review::key(&job, ASSIGNMENT),
        selection: Selection::resolve(&settings(), &job, ASSIGNMENT).unwrap(),
        job,
        assignment_id: ASSIGNMENT.into(),
        operation,
        manual_start: true,
        trust_confirmed: true,
        phase: "Review stopped".into(),
        error: Some("Failed".into()),
        result: None,
    }
}

#[test]
fn parked_work_carries_its_class_and_never_consumes_capacity() {
    for (failure, state, class) in [
        (
            OperationFailure::Credentials,
            OperationState::Failed,
            FailureClass::Credentials,
        ),
        (
            OperationFailure::Network,
            OperationState::ManualRetry,
            FailureClass::Network,
        ),
        (
            OperationFailure::Permission,
            OperationState::Failed,
            FailureClass::Permission,
        ),
        (
            OperationFailure::Permanent,
            OperationState::Failed,
            FailureClass::Other,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().into());
        let run = parked_review(failure, state);
        store.save_settings(&settings()).unwrap();
        store.save_queue(std::slice::from_ref(&run.job)).unwrap();
        store.save_reviews(&[run]).unwrap();

        let item = queue::snapshot(&store, vec![]).unwrap().items.remove(0);
        assert_eq!(item.state, State::Failed);
        assert_eq!(item.failure_class, Some(class));

        let work = capacity::candidates(&store, 1_000_000).unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].state, "blocked");
        assert_eq!(work[0].failure, Some(class));
        let value = serde_json::to_value(&work[0]).unwrap();
        assert_eq!(value["failure"], serde_json::to_value(class).unwrap());
    }
}

#[test]
fn retrying_work_is_not_a_failure() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut run = parked_review(OperationFailure::Network, OperationState::Queued);
    run.operation.attempt_count = 1;
    run.operation.next_attempt_at = Some(2_000_000);
    store.save_settings(&settings()).unwrap();
    store.save_queue(std::slice::from_ref(&run.job)).unwrap();
    store.save_reviews(&[run]).unwrap();
    let item = queue::snapshot(&store, vec![]).unwrap().items.remove(0);
    assert_eq!(item.failure_class, None);
    let work = capacity::candidates(&store, 1_000).unwrap();
    assert_eq!(work[0].failure, None);
}

fn park(operation: &mut JobOperation, kind: OperationFailure) -> i64 {
    let mut now = 0;
    for _ in 0..3 {
        if operation.parked() {
            break;
        }
        now = attempt(operation, &failure(kind.clone(), None));
    }
    now
}

#[test]
fn cooldown_cycles_escalate_30m_1h_2h_4h_8h_then_wait_for_a_human() {
    assert_eq!(
        RESUME_COOLDOWNS_SECONDS,
        [1_800, 3_600, 7_200, 14_400, 28_800]
    );
    let mut operation = JobOperation::review(&job(), 1_000);
    for (cycle, cooldown) in RESUME_COOLDOWNS_SECONDS.into_iter().enumerate() {
        let parked_at = park(&mut operation, OperationFailure::Network);
        assert!(operation.parked());
        assert_eq!(operation.resume_cycle as usize, cycle);
        assert_eq!(operation.resume_at, Some(parked_at + cooldown));
        assert!(!operation.auto_resume(false, parked_at + cooldown - 1));
        assert!(operation.auto_resume(false, parked_at + cooldown));
        assert_eq!(operation.state, OperationState::Queued);
        assert_eq!(operation.attempt_count, 0);
        assert_eq!(operation.failure, None);
        assert_eq!(operation.resume_at, None);
        assert_eq!(operation.resume_cycle as usize, cycle + 1);
    }
    // The fifth resumed budget also fails: now only a human can continue.
    park(&mut operation, OperationFailure::Network);
    assert!(operation.parked());
    assert_eq!(operation.resume_at, None);
    assert!(!operation.auto_resume(true, i64::MAX));
}

#[test]
fn only_resumable_failures_get_a_cooldown_and_explicit_retry_resets_the_cycle() {
    for (kind, resumes) in [
        (OperationFailure::Timeout, true),
        (OperationFailure::Provider, true),
        (OperationFailure::Permission, true),
        (OperationFailure::Permanent, false),
        (OperationFailure::Credentials, false),
    ] {
        let mut operation = JobOperation::review(&job(), 1_000);
        park(&mut operation, kind.clone());
        assert!(operation.parked(), "{kind:?}");
        assert_eq!(operation.resume_at.is_some(), resumes, "{kind:?}");
    }
    let mut operation = JobOperation::review(&job(), 1_000);
    let at = park(&mut operation, OperationFailure::Network);
    assert!(operation.auto_resume(false, at + 1_800));
    assert_eq!(operation.resume_cycle, 1);
    // An explicit Retry creates a new operation with a clean cycle count.
    let retried = JobOperation::review(&job(), at + 2_000);
    assert_eq!((retried.resume_cycle, retried.resume_at), (0, None));
    let stored = serde_json::to_value(&retried).unwrap();
    assert!(stored.get("resume_cycle").is_none() && stored.get("resume_at").is_none());
}

#[test]
fn legacy_parked_rate_limits_still_load_and_resume_through_the_cooldown() {
    let mut legacy = serde_json::to_value(JobOperation::review(&job(), 1_000)).unwrap();
    legacy["state"] = json!("manual_retry");
    legacy["failure"] = json!("rate_limited");
    legacy["attempt_count"] = json!(3);
    legacy["next_attempt_at"] = json!(null);
    legacy["resume_at"] = json!(2_800);
    let mut operation: JobOperation = serde_json::from_value(legacy).unwrap();
    assert!(operation.parked());
    assert_eq!(operation.backpressure_until, None);
    assert!(operation.auto_resume(false, 2_800));
    assert_eq!(operation.attempt_count, 0);
}

fn accounts(connected: bool) -> std::collections::BTreeMap<String, AccountAvailability> {
    ["22", "33"]
        .into_iter()
        .map(|id| {
            (
                id.to_string(),
                AccountAvailability {
                    login: id.into(),
                    connected,
                },
            )
        })
        .collect()
}

fn store_with(run: &ReviewRun) -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.save_settings(&settings()).unwrap();
    store.save_queue(std::slice::from_ref(&run.job)).unwrap();
    store.save_reviews(std::slice::from_ref(run)).unwrap();
    (root, store)
}

#[test]
fn credential_failures_resume_when_the_account_connects_again_without_a_human_retry() {
    let run = parked_review(OperationFailure::Credentials, OperationState::Failed);
    let (_root, store) = store_with(&run);
    let mut monitor = Monitor::default();
    // Disconnected: nothing resumes, and a steady state never resumes either.
    assert_eq!(
        monitor.auto_resume(&store, &accounts(false), 10).unwrap(),
        0
    );
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].failure_class,
        Some(FailureClass::Credentials)
    );
    // The account flips to connected: fresh budget, no human action.
    assert_eq!(monitor.auto_resume(&store, &accounts(true), 20).unwrap(), 1);
    let resumed = store.load_reviews().unwrap().remove(0);
    assert_eq!(resumed.operation.state, OperationState::Queued);
    assert_eq!(resumed.operation.attempt_count, 0);
    assert_eq!(resumed.operation.resume_cycle, 0);
    assert_eq!(resumed.error, None);
    let item = queue::snapshot(&store, vec![]).unwrap().items.remove(0);
    assert_eq!(item.failure_class, None);
    assert_ne!(item.state, State::Failed);
    // Failing again with the account still connected does not loop.
    let mut again = resumed;
    again.operation.state = OperationState::Failed;
    again.operation.failure = Some(OperationFailure::Credentials);
    store.save_reviews(&[again]).unwrap();
    assert_eq!(monitor.auto_resume(&store, &accounts(true), 30).unwrap(), 0);
}

#[test]
fn renewed_credentials_resume_credential_failures_while_the_account_stays_connected() {
    let run = parked_review(OperationFailure::Credentials, OperationState::Failed);
    let (_root, store) = store_with(&run);
    let mut monitor = Monitor::default();
    assert_eq!(monitor.auto_resume(&store, &accounts(true), 10).unwrap(), 1);
    let mut again = store.load_reviews().unwrap().remove(0);
    again.operation.state = OperationState::Failed;
    again.operation.failure = Some(OperationFailure::Credentials);
    store.save_reviews(&[again]).unwrap();
    // The record never left the connected state, so no transition resumes it.
    assert_eq!(monitor.auto_resume(&store, &accounts(true), 20).unwrap(), 0);
    // Saving fresh credentials for the bound account resumes it without a human retry.
    monitor.credentials_renewed("22");
    assert_eq!(monitor.auto_resume(&store, &accounts(true), 30).unwrap(), 1);
    let resumed = store.load_reviews().unwrap().remove(0);
    assert_eq!(resumed.operation.state, OperationState::Queued);
    assert_eq!(resumed.operation.failure, None);
    // An unrelated account's renewal does not resume it.
    let mut again = resumed;
    again.operation.state = OperationState::Failed;
    again.operation.failure = Some(OperationFailure::Credentials);
    store.save_reviews(&[again]).unwrap();
    monitor.credentials_renewed("44");
    assert_eq!(monitor.auto_resume(&store, &accounts(true), 40).unwrap(), 0);
}

#[test]
fn cooldown_resume_requeues_a_parked_review_and_exposes_the_hint() {
    let mut run = parked_review(OperationFailure::Network, OperationState::ManualRetry);
    run.operation.resume_at = Some(5_000);
    let (_root, store) = store_with(&run);
    let item = queue::snapshot(&store, vec![]).unwrap().items.remove(0);
    assert_eq!(item.state, State::Failed);
    assert_eq!(item.resume_at, Some(5_000));
    assert_eq!(
        serde_json::to_value(&item).unwrap()["resume_at"],
        json!(5_000)
    );
    let mut monitor = Monitor::default();
    assert_eq!(
        monitor.auto_resume(&store, &accounts(true), 4_999).unwrap(),
        0
    );
    assert_eq!(
        monitor.auto_resume(&store, &accounts(true), 5_000).unwrap(),
        1
    );
    let resumed = store.load_reviews().unwrap().remove(0);
    assert_eq!(resumed.operation.resume_cycle, 1);
    assert_eq!(resumed.operation.state, OperationState::Queued);
    let item = queue::snapshot(&store, vec![]).unwrap().items.remove(0);
    assert_eq!((item.failure_class, item.resume_at), (None, None));
}

#[test]
fn a_new_head_supersedes_a_parked_failure_on_the_old_head() {
    let mut run = parked_review(OperationFailure::Network, OperationState::ManualRetry);
    run.operation.resume_at = Some(5_000);
    let (_root, store) = store_with(&run);
    let mut old = run.job.clone();
    old.waiting = "superseded".into();
    let mut next = run.job.clone();
    next.head_sha = "c".repeat(40);
    next.waiting = "human_start".into();
    store.save_queue(&[old, next]).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert!(snapshot.items.iter().all(|i| i.state != State::Failed));
    // A superseded run is never resumed.
    let mut monitor = Monitor::default();
    assert_eq!(
        monitor.auto_resume(&store, &accounts(true), 9_999).unwrap(),
        0
    );
}

#[test]
fn legacy_operations_without_resume_fields_still_load() {
    let mut legacy = serde_json::to_value(JobOperation::review(&job(), 1_000)).unwrap();
    legacy.as_object_mut().unwrap().remove("resume_cycle");
    legacy.as_object_mut().unwrap().remove("resume_at");
    let restored: JobOperation = serde_json::from_value(legacy).unwrap();
    assert_eq!((restored.resume_cycle, restored.resume_at), (0, None));
}

#[test]
fn conversation_admission_errors_retry_park_with_a_cooldown_and_resume() {
    let error = MonitoringError::Recoverable {
        code: "conversation_admission".into(),
        message: "Could not write conversation state.".into(),
        failure: OperationFailure::Provider,
        retry_after_seconds: None,
    };
    let mut operation = JobOperation::review(&job(), 1_000);
    let mut now = 0;
    for attempt in 1..=3 {
        now = operation.next_attempt_at.unwrap();
        operation.begin_attempt(now).unwrap();
        operation.fail(&error, now);
        assert_eq!(
            operation.state,
            if attempt < 3 {
                OperationState::Queued
            } else {
                OperationState::ManualRetry
            }
        );
    }
    assert_eq!(operation.failure_class(), Some(FailureClass::Other));
    assert_eq!(operation.resume_at, Some(now + RESUME_COOLDOWNS_SECONDS[0]));
    assert!(operation.auto_resume(false, now + RESUME_COOLDOWNS_SECONDS[0]));
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.resume_cycle, 1);
}

fn stale_admission_state() -> serde_json::Value {
    let mut operation = serde_json::to_value(JobOperation::review(&job(), 100)).unwrap();
    operation["operation_type"] = json!("repository_poll");
    operation["state"] = json!("failed");
    operation["failure"] = json!("permanent");
    operation["attempt_count"] = json!(1);
    operation["next_attempt_at"] = json!(null);
    json!({"health": {REPO: {
        "repository_id": REPO, "name": "example/repo", "schedule_key": "fixture",
        "provider_account_id": "22", "enabled": true, "last_attempt": 100,
        "last_success": null, "next_run": 0, "schedule_available": false,
        "last_failure": "conversation_admission", "in_flight": false,
        "operation": operation
    }}})
}

#[test]
fn parked_permanent_admission_failures_from_the_old_build_are_requeued_once() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.save_settings(&settings()).unwrap();
    store
        .save_monitoring_state(&serde_json::from_value(stale_admission_state()).unwrap())
        .unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    assert_eq!(
        monitor.auto_resume(&store, &accounts(true), 1_000).unwrap(),
        1
    );
    let health = Monitor::restore(&store).unwrap().snapshot().remove(0);
    let operation = health.operation.unwrap();
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.failure, None);
    assert_eq!(operation.attempt_count, 0);
    assert_eq!(health.last_failure, None);
    assert!(health.schedule_available);
    // The marker is persisted: a later identical failure is not requeued again.
    let mut state: pr_sniper_lib::monitoring::MonitoringState =
        serde_json::from_value(stale_admission_state()).unwrap();
    state.admission_upgrade_done = true;
    store.save_monitoring_state(&state).unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    assert_eq!(
        monitor.auto_resume(&store, &accounts(true), 2_000).unwrap(),
        0
    );
    let kept = Monitor::restore(&store).unwrap().snapshot().remove(0);
    assert_eq!(kept.operation.unwrap().state, OperationState::Failed);
}

#[test]
fn a_duck_credentials_park_resumes_when_only_the_duck_account_reconnects() {
    let mut run = parked_review(OperationFailure::Credentials, OperationState::Failed);
    run.failed_duck = Some("Duck".into());
    let mut duck = run.selection.agent.clone();
    duck.name = "Duck".into();
    duck.ai_account.as_mut().unwrap().account_id = "77".into();
    run.selection.configuration.as_mut().unwrap().rubber_duck = Some(review::duck::DuckSnapshot {
        agent: duck,
        doctrines: vec![],
    });
    let (_root, store) = store_with(&run);
    let mut monitor = Monitor::default();
    // GitHub and the primary's Copilot account stay disconnected throughout.
    let mut connected = accounts(false);
    let duck_account = |connected: bool| AccountAvailability {
        login: "duck".into(),
        connected,
    };
    connected.insert("77".into(), duck_account(false));
    assert_eq!(monitor.auto_resume(&store, &connected, 10).unwrap(), 0);
    connected.insert("77".into(), duck_account(true));
    assert_eq!(monitor.auto_resume(&store, &connected, 20).unwrap(), 1);
    let resumed = store.load_reviews().unwrap().remove(0);
    assert_eq!(resumed.operation.state, OperationState::Queued);
}
