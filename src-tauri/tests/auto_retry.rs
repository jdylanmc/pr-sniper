use pr_sniper_lib::{
    capacity,
    github::ConnectionError,
    monitoring::{FailureClass, JobOperation, OperationFailure, OperationState, QueueJob},
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
        OperationFailure::RateLimited,
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
fn retry_after_is_honored_inside_the_retry_window_and_parks_beyond_it() {
    let mut operation = JobOperation::review(&job(), 1_000);
    operation.begin_attempt(1_000).unwrap();
    operation.fail(
        &failure(OperationFailure::RateLimited, Some(600)).monitoring(),
        1_000,
    );
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.next_attempt_at, Some(1_600));
    assert!(operation.begin_attempt(1_599).is_err());
    operation.begin_attempt(1_600).unwrap();
    operation.fail(
        &failure(OperationFailure::RateLimited, Some(3_600)).monitoring(),
        1_600,
    );
    assert_eq!(operation.state, OperationState::ManualRetry);
    assert_eq!(operation.failure_class(), Some(FailureClass::Network));
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
