use pr_sniper_lib::{
    exits::ExitCall,
    github::{
        metadata::{Lifecycle, PullRequest},
        Identity,
    },
    monitoring::{JobOperation, OperationFailure, OperationState, QueueJob},
    review::{self, Events, Failure, ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::{json, Value};

fn job() -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":"assignment","provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":"repo","repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"all_authors":false,"requested_reviewer":false,
        "waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

fn settings() -> Settings {
    let mut settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{
            "id":"repo","provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":"assignment","agent_id":"agent","schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":false}]
        }],
        "agents":[{
            "id":"agent","name":"Reviewer","model":"configured-model",
            "ai_account":{"provider":"copilot","account_id":"33"},"doctrine":"Correctness",
            "prompt":"Look for correctness defects.","signature":"machine"
        }],
        "doctrines":[{"title":"Correctness","body":"Trace state transitions."}]
    })).unwrap();
    settings.migrate_library_ids();
    settings
}

fn pull() -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 1,
        title: "Review".into(),
        author: Some(Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: "a".repeat(40),
        base_sha: "b".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-09-27T00:00:00Z".into(),
        files: vec![],
    }
}

#[test]
fn jsonl_normalization_requires_idle_usage_and_an_exit() {
    let lines = [
        json!({"type":"assistant.message","data":{"content":"Considering the changes.","toolRequests":[{"name":"read_changes"}]}}),
        json!({"type":"tool.execution_start","data":{"toolName":"read_changes"}}),
        json!({"type":"assistant.usage","data":{"inputTokens":100.0,"outputTokens":40.0}}),
        json!({"type":"session.idle","data":{}}),
    ];
    let jsonl = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut events = Events::default();
    for line in jsonl.lines() {
        let event: Value = serde_json::from_str(line).unwrap();
        events
            .push(event["type"].as_str().unwrap(), &event["data"])
            .unwrap();
    }
    let exit = || ExitCall::ReadyForHuman {
        summary: "No findings.".into(),
    };
    let result = events
        .finish_exit(
            Some(exit()),
            "session".into(),
            "model".into(),
            "runtime".into(),
            Ok,
        )
        .unwrap();
    assert_eq!(result.input_tokens, 100);
    assert_eq!(result.output_tokens, 40);
    assert_eq!(result.tool_calls, 1);
    // No idle and usage evidence: no result, even with an exit.
    assert!(Events::default()
        .finish_exit(Some(exit()), "s".into(), "m".into(), "v".into(), Ok)
        .is_err());
    // Idle without an exit is a retryable agent failure.
    let mut idle = Events::default();
    idle.push(
        "assistant.usage",
        &json!({"inputTokens":1,"outputTokens":1}),
    )
    .unwrap();
    idle.push("session.idle", &json!({})).unwrap();
    let error = idle
        .finish_exit(None, "s".into(), "m".into(), "v".into(), Ok)
        .unwrap_err();
    assert_eq!(error.kind, OperationFailure::Provider);
    assert!(Events::default()
        .push("session.error", &json!({"message":"secret"}))
        .unwrap_err()
        .message
        .find("secret")
        .is_none());
}

#[test]
fn selection_is_assignment_bound_and_keeps_account_prompt_and_doctrine() {
    let settings = settings();
    let job = job();
    let selected = Selection::resolve(&settings, &job, "assignment").unwrap();
    assert_eq!(selected.agent.ai_account.unwrap().account_id, "33");
    assert_eq!(
        selected.doctrine.as_deref(),
        Some("Trace state transitions.")
    );
    assert_eq!(selected.agent.model, "configured-model");
    assert!(Selection::resolve(&settings, &job, "other-assignment").is_err());
    let mut legacy = job.clone();
    legacy.assignment_id = None;
    assert!(Selection::resolve(&settings, &legacy, "assignment").is_err());
    for change in 0..4 {
        let mut changed = settings.clone();
        match change {
            0 => changed.repositories[0].enabled = false,
            1 => changed.repositories[0].provider_account_id = Some("44".into()),
            2 => changed.doctrines.clear(),
            _ => changed.agents[0].ai_account = None,
        }
        assert!(Selection::resolve(&changed, &job, "assignment").is_err());
    }
}

#[test]
fn invocation_revalidates_head_lifecycle_and_trigger_without_revision_consent() {
    let settings = settings();
    let job = job();
    let pull = pull();
    assert!(pr_sniper_lib::monitoring::review_policy(&settings, &job, Some(&pull)).is_ok());
    for change in 0..5 {
        let mut changed = pull.clone();
        match change {
            0 => changed.head_sha = "b".repeat(40),
            1 => changed.draft = true,
            2 => changed.state = Lifecycle::Closed,
            3 => changed.author.as_mut().unwrap().id = "55".into(),
            _ => changed.base_repository_id = "99".into(),
        }
        assert!(pr_sniper_lib::monitoring::review_policy(&settings, &job, Some(&changed)).is_err());
    }
    assert!(pr_sniper_lib::monitoring::review_policy(&settings, &job, Some(&pull)).is_ok());
    let mut fork = pull.clone();
    fork.head_repository_id = Some("200".into());
    assert!(pr_sniper_lib::monitoring::review_policy(&settings, &job, Some(&fork)).is_ok());
    let mut all_authors = job;
    all_authors.watched_author = false;
    assert!(pr_sniper_lib::monitoring::review_policy(&settings, &all_authors, Some(&pull)).is_ok());
}

#[test]
fn durable_review_uses_shared_retry_budget_and_restores_without_new_identity() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let job = job();
    let mut run = ReviewRun {
        mode: pr_sniper_lib::review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: review::key(&job, "assignment"),
        assignment_id: "assignment".into(),
        selection: Selection::resolve(&settings(), &job, "assignment").unwrap(),
        operation: JobOperation::review(&job, 100),
        job,
        manual_start: true,
        trust_confirmed: false,
        phase: "running".into(),
        error: None,
        result: None,
    };
    run.operation.begin_attempt(100).unwrap();
    let id = run.operation.id.clone();
    store.save_reviews(&[run]).unwrap();
    review::restore(&store).unwrap();
    let mut restored = store.load_reviews().unwrap().remove(0);
    assert_eq!(restored.operation.id, id);
    assert_eq!(restored.operation.retry_deadline, 1000);
    assert_eq!(restored.operation.attempt_count, 1);
    assert_eq!(restored.operation.state, OperationState::Interrupted);
    for attempt in 2..=3 {
        let now = restored.operation.next_attempt_at.unwrap().max(101);
        restored.operation.begin_attempt(now).unwrap();
        assert_eq!(restored.operation.attempt_count, attempt);
        restored
            .operation
            .fail(&Failure::timeout().monitoring(), now);
    }
    assert_eq!(restored.operation.state, OperationState::ManualRetry);
    assert!(restored.operation.attempted_mutation.is_none());
    assert!(restored.operation.confirmed_receipt.is_none());
    let mut operation = JobOperation::review(&restored.job, 100);
    operation.begin_attempt(100).unwrap();
    let failure = Failure {
        cancelled: false,
        kind: OperationFailure::RateLimited,
        message: "Rate limited".into(),
        retry_after_seconds: Some(900),
        origin: pr_sniper_lib::backpressure::Origin::Copilot,
        duck_setup: false,
    };
    // Beyond the retry window a rate limit still only waits: no park, no budget.
    operation.fail(&failure.monitoring(), 100);
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.attempt_count, 0);
    assert_eq!(operation.failure, None);
    assert_eq!(operation.next_attempt_at, Some(1000));
    let mut boundary = JobOperation::review(&restored.job, 100);
    boundary.begin_attempt(1000).unwrap();
    assert_eq!(boundary.initial_attempt_at, 1000);
    assert_eq!(boundary.retry_deadline, 1900);
    boundary.fail(&Failure::timeout().monitoring(), 1001);
    assert!(boundary.begin_attempt(1900).is_err());
    assert_eq!(boundary.state, OperationState::ManualRetry);
}

#[test]
fn runtime_errors_preserve_retry_classification_without_provider_messages() {
    for (status, expected) in [
        (408, OperationFailure::Timeout),
        (429, OperationFailure::RateLimited),
        (503, OperationFailure::Provider),
        (401, OperationFailure::Credentials),
        (403, OperationFailure::Permission),
        // Unclassified provider statuses retry: an execution failure is not proof of a permanent fault.
        (400, OperationFailure::Provider),
    ] {
        let error = Events::default()
            .push(
                "session.error",
                &json!({
                    "statusCode":status,"message":"secret-provider-data","retryAfterSeconds":60
                }),
            )
            .unwrap_err();
        assert_eq!(error.kind, expected);
        assert_eq!(error.rate_limited(), status == 429);
        assert_eq!(error.retry_after_seconds, Some(60));
        assert!(!error.message.contains("secret-provider"));
    }
    assert_eq!(
        Failure::operation("Copilot operation timed out. Retry.".into()).kind,
        OperationFailure::Timeout
    );
}
