//! Rubber duck calls over the synthetic Copilot runtime: one primary process, one duck
//! process, both the fixture. Synthetic transport, not live Copilot acceptance.

use super::tests::{
    context, diagnostic_records, diagnostic_trace, operation, options, receipt, selection, stopped,
    Synthetic,
};
use super::*;
use crate::{
    packet::{AuthorRelation, Directive, PacketInput, PrFacts, Role},
    review::duck::{DuckFailure, DuckOutcome, VerdictKind},
    storage::DuckLimits,
};
use std::path::Path;

fn base() -> PacketInput {
    PacketInput {
        state_id: "7:abcdef".into(),
        revision: 1,
        created_at: "2026-10-14T00:00:00Z".into(),
        admitted_by: None,
        acting_login: "pr-sniper-bot".into(),
        include_self: false,
        role: Role::Primary,
        tools: vec![],
        exits: vec![],
        pr: PrFacts {
            title: "Synthetic review".into(),
            description: None,
            author: "octo".into(),
            author_relation: AuthorRelation::Other,
            state: "OPEN".into(),
            draft: false,
            base_ref: "main".into(),
            head_ref: "topic".into(),
            base_sha: "b".repeat(40),
            head_sha: "a".repeat(40),
            labels: vec![],
            size: None,
            checks: vec![],
            mergeable: "MERGEABLE".into(),
            review_decision: None,
            approvals: vec![],
        },
        conversation: Default::default(),
        files: vec![],
        delta: None,
        directive: Directive::default(),
        teammates: vec![],
    }
}

struct Duck {
    setup: DuckSetup,
    paced: Arc<Mutex<Vec<Option<i64>>>>,
}

fn duck(root: &Path, scenario: &str, limits: DuckLimits, gated: bool) -> Duck {
    let paced = Arc::new(Mutex::new(Vec::new()));
    let sink = paced.clone();
    let mut duck_selection = selection();
    duck_selection.agent.id = "duck".into();
    duck_selection.agent.name = "Duck".into();
    std::fs::create_dir_all(root).unwrap();
    Duck {
        setup: DuckSetup {
            prep: DuckPrep {
                base: base(),
                primary_hash: "primaryhash".into(),
                directive: Directive {
                    agent_prompt: "Check one claim.".into(),
                    ..Directive::default()
                },
                tools: tools::duck_tools(&["read_changes".into(), "read_source".into()]),
                changed: vec!["source.rs".into()],
                threads: vec![],
            },
            selection: duck_selection,
            limits,
            launch: DuckLaunch::Options {
                identity: Identity {
                    id: "44".into(),
                    login: "review-account".into(),
                },
                options: Box::new(options(root, scenario)),
            },
            rate_limited: Arc::new(move |after| sink.lock().unwrap().push(after)),
            gated: Arc::new(move || gated),
            account: None,
            log: Default::default(),
        },
        paced,
    }
}

fn limits(max_calls: u32, call_timeout_seconds: u32) -> DuckLimits {
    DuckLimits {
        max_calls,
        call_timeout_seconds,
        primary_window_minutes: 15,
    }
}

fn primary_request(setup: DuckSetup) -> Request<Synthetic> {
    let granted: Vec<String> = ["read_changes", "read_source", crate::review::duck::TOOL]
        .map(String::from)
        .to_vec();
    let offered = [Exit::ReadyForHuman, Exit::PanicExit];
    Request {
        failure_commit: None,
        inactivity: Duration::from_secs(900),
        exits: ExitSetup {
            tool_set: tools::tool_set(&granted, true, &offered),
            offered: offered.to_vec(),
            facts: Arc::new(ToolFacts::default()),
            gate: Arc::new(|_: &ExitCall| Ok(())),
        },
        diagnostics: None,
        task: ExitReview {
            packet_text: "packet".into(),
        },
        context: context(true),
        client: Arc::new(GithubClient::new(Synthetic)),
        repository_name: "example/repo".into(),
        selection: selection(),
        before_send: Arc::new(|| Ok(())),
        local_gate: Arc::new(|| Ok(())),
        duck: Some(setup),
    }
}

fn identity() -> Identity {
    Identity {
        id: "33".into(),
        login: "review-account".into(),
    }
}

/// The primary's tool results for its `rubber_duck` calls, in call order.
fn duck_results(root: &Path) -> Vec<String> {
    receipt(root)
        .into_iter()
        .filter(|r| r["method"] == "fixture.duck")
        .map(|r| r["result"].to_string())
        .collect()
}

fn methods(root: &Path, method: &str) -> usize {
    receipt(root)
        .iter()
        .filter(|r| r["method"] == method)
        .count()
}

async fn review(
    scenario: &str,
    duck_scenario: &str,
    limits: DuckLimits,
    seconds: u64,
) -> (
    tempfile::TempDir,
    Result<ReviewResult<crate::review::ReviewOutput>, Failure>,
    Arc<Mutex<Vec<Option<i64>>>>,
) {
    let root = tempfile::tempdir().unwrap();
    let duck = duck(&root.path().join("duck"), duck_scenario, limits, false);
    let mut request = primary_request(duck.setup);
    let attempt = crate::review::host::AttemptGuard::new(request.local_gate.clone());
    request.local_gate = attempt.gate();
    request.failure_commit = Some(attempt.committer());
    let result = execute(
        options(root.path(), scenario),
        &identity(),
        &operation(seconds),
        request,
    )
    .await;
    (root, attempt.finish(result), duck.paced)
}

#[tokio::test]
async fn only_meaningful_nested_progress_keeps_the_primary_attempt_alive() {
    for scenario in ["verdict-progress", "verdict-heartbeat"] {
        let root = tempfile::tempdir().unwrap();
        let duck = duck(&root.path().join("duck"), scenario, limits(8, 180), false);
        let mut request = primary_request(duck.setup);
        request.inactivity = Duration::from_secs(2);
        let result = execute(
            options(root.path(), "duck-call"),
            &identity(),
            &operation(600),
            request,
        )
        .await;
        if scenario == "verdict-progress" {
            assert!(matches!(
                result.unwrap().ducks[0].outcome,
                DuckOutcome::Verdict { .. }
            ));
        } else {
            assert_eq!(
                result.unwrap_err().kind,
                crate::monitoring::OperationFailure::Timeout
            );
        }
        stopped(root.path());
        stopped(&root.path().join("duck"));
    }
}

#[tokio::test]
async fn nested_diagnostics_export_outcome_errors_and_cleanup_without_duck_content() {
    for (scenario, expected) in [
        ("verdict-ok", "confirmed"),
        ("verdict-429", "rate_limited"),
        ("verdict-slow", "timed_out"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let duck = duck(
            &root.path().join("duck"),
            scenario,
            limits(8, if scenario == "verdict-slow" { 3 } else { 180 }),
            false,
        );
        let mut request = primary_request(duck.setup);
        request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
        execute(
            options(root.path(), "duck-call"),
            &identity(),
            &operation(600),
            request,
        )
        .await
        .unwrap();
        let records = diagnostic_records(root.path());
        let summary = records
            .iter()
            .find(|record| record["event"]["kind"] == "duck")
            .unwrap();
        assert_eq!(summary["event"]["ordinal"], 1);
        assert!(
            summary["event"]["outcome"] == expected || summary["event"]["failure"] == expected,
            "{summary}"
        );
        let nested: Vec<_> = records
            .iter()
            .filter(|record| record["event"]["kind"] == "duck_event")
            .collect();
        assert!(nested
            .iter()
            .all(|record| record["attempt"]["id"] == summary["attempt"]["id"]));
        assert!(nested
            .iter()
            .any(|record| record["event"]["event"]["kind"] == "session_created"));
        assert!(nested
            .iter()
            .any(|record| record["event"]["event"]["kind"] == "duck_teardown"
                // The fixture returns {}, not the SDK's required {success:true}.
                && record["event"]["event"]["disconnect"] == "failed"));
        if scenario == "verdict-429" {
            assert!(nested
                .iter()
                .any(|record| record["event"]["event"]["kind"] == "session"
                    && record["event"]["event"]["status_code"] == 429));
        }
        if scenario == "verdict-ok" {
            assert!(nested
                .iter()
                .any(|record| record["event"]["event"]["kind"] == "tool_finished"));
        }
        let export = serde_json::to_string(&records).unwrap();
        for forbidden in [
            "proposed_body",
            "packet_text",
            "revised_body",
            "rationale",
            "inputTokens",
            "outputTokens",
        ] {
            assert!(!export.contains(forbidden), "{forbidden}");
        }
        stopped(root.path());
        stopped(&root.path().join("duck"));
    }
}

fn fail_duck_shutdown(setup: &mut DuckSetup) {
    let DuckLaunch::Options { options, .. } = &mut setup.launch else {
        panic!("synthetic Duck options expected");
    };
    options.env.push(("TEST_FAIL_SHUTDOWN".into(), "1".into()));
}

#[tokio::test]
async fn duck_client_cleanup_failure_is_exported_apart_from_primary_and_session_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let mut duck = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        false,
    );
    fail_duck_shutdown(&mut duck.setup);
    let mut request = primary_request(duck.setup);
    request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
    let result = execute(
        options(root.path(), "duck-call"),
        &identity(),
        &operation(600),
        request,
    )
    .await
    .unwrap();
    assert!(matches!(
        result.ducks[0].outcome,
        DuckOutcome::Verdict { .. }
    ));
    let records = diagnostic_records(root.path());
    let primary = records
        .iter()
        .find(|r| r["event"]["kind"] == "teardown")
        .unwrap();
    let client = records
        .iter()
        .find(|r| r["event"]["kind"] == "duck_client_teardown")
        .unwrap();
    assert_eq!(primary["event"]["shutdown"], "success");
    assert_eq!(client["event"]["drain"], "success");
    assert_eq!(client["event"]["shutdown"], "forced_stop_unverified");
    assert_eq!(client["attempt"]["id"], primary["attempt"]["id"]);
    assert!(records
        .iter()
        .any(|r| r["event"]["kind"] == "duck_event"
            && r["event"]["event"]["kind"] == "duck_teardown"));
    let export = serde_json::to_string(&records).unwrap();
    assert!(!export.contains("PRIVATE_DUCK_SHUTDOWN") && !export.contains("ghp_cleanup_secret"));
    stopped(root.path());
    stopped(&root.path().join("duck"));
}

#[tokio::test]
async fn duck_client_cleanup_records_an_expired_drain_without_claiming_the_call_settled() {
    let root = tempfile::tempdir().unwrap();
    let mut duck = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        false,
    );
    fail_duck_shutdown(&mut duck.setup);
    let engine = duck_tool::DuckEngine::start(duck.setup, &operation(600))
        .await
        .unwrap();
    let trace = diagnostic_trace(root.path().join("profile"));
    let held = engine.serial.lock().await;
    shutdown_duck(&engine, Some(trace.as_ref())).await;
    drop(held);
    let records = diagnostic_records(root.path());
    let client = records
        .iter()
        .find(|r| r["event"]["kind"] == "duck_client_teardown")
        .unwrap();
    assert_eq!(client["event"]["drain"], "failed");
    assert_eq!(client["event"]["shutdown"], "forced_stop_unverified");
    assert_eq!(methods(&root.path().join("duck"), "runtime.shutdown"), 1);
    stopped(&root.path().join("duck"));
}

#[tokio::test]
async fn a_duck_call_returns_a_fenced_verdict_and_is_recorded_with_the_primary() {
    let (root, result, _) = review("duck-call", "verdict-ok", limits(8, 180), 600).await;
    let result = result.unwrap();
    assert!(matches!(
        result.output.exit,
        Some(crate::exits::ExitOutcome::ReadyForHuman { .. })
    ));
    assert_eq!(result.ducks.len(), 1);
    let call = &result.ducks[0];
    let DuckOutcome::Verdict { verdict } = &call.outcome else {
        panic!("{call:?}");
    };
    assert_eq!(verdict.verdict, VerdictKind::Confirmed);
    assert_eq!((call.ordinal, call.agent.as_str()), (1, "Duck"));
    assert_eq!(call.state_id, "7:abcdef");
    assert_eq!(call.limits, limits(8, 180));
    assert!(call.session_id.is_some() && call.packet.is_some());
    assert_eq!((call.input_tokens, call.output_tokens), (20, 10));
    let results = duck_results(root.path());
    assert_eq!(results.len(), 1);
    assert!(
        results[0].contains(r#"<untrusted source=\"rubber_duck\">"#),
        "{}",
        results[0]
    );
    assert!(results[0].contains("confirmed"));
    // The primary catalog offers rubber_duck; the duck's has reads and submit_verdict only.
    let created = |root: &Path| {
        receipt(root)
            .into_iter()
            .filter(|r| r["method"] == "session.create")
            .map(|r| {
                let mut names: Vec<String> = r["config"]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["name"].as_str().unwrap().to_string())
                    .collect();
                names.sort();
                names
            })
            .collect::<Vec<_>>()
    };
    assert!(created(root.path())[0].contains(&"rubber_duck".to_string()));
    assert_eq!(
        created(&root.path().join("duck")),
        vec![vec!["read_changes", "read_source", "submit_verdict"]]
    );
    let prompt = receipt(&root.path().join("duck"))
        .into_iter()
        .find(|r| r["method"] == "fixture.prompt")
        .unwrap()
        .to_string();
    assert!(prompt.contains("rubber duck packet") && prompt.contains("answer may overflow"));
    stopped(root.path());
    stopped(&root.path().join("duck"));
}

#[tokio::test]
async fn fixed_verification_requires_independent_original_thread_and_head_code_reads() {
    for (scenario, confirmed) in [
        ("verdict-fixed-read", true),
        ("verdict-fixed-no-thread", false),
        ("verdict-fixed-partial-thread", false),
        ("verdict-ok", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut duck = duck(&root.path().join("duck"), scenario, limits(8, 180), false);
        duck.setup.prep.threads = vec!["existing".into()];
        duck.setup.prep.tools.insert(0, "get_thread".into());
        let mut request = primary_request(duck.setup);
        request.exits.facts = Arc::new(ToolFacts {
            conversation: crate::packet::Conversation {
                threads: vec![crate::packet::ThreadFact {
                    id: "existing".into(),
                    path: Some("source.rs".into()),
                    line: Some(1),
                    resolved: false,
                    outdated: true,
                    mine: true,
                    new_since_last_review: false,
                    targeted_revision: Some("1".into()),
                    comments: vec![crate::packet::CommentFact {
                        id: "root".into(),
                        author: "reviewer".into(),
                        body: "Original defect.".into(),
                    }],
                }],
                comments: vec![],
            },
            ..Default::default()
        });
        let result = execute(
            options(root.path(), "duck-fixed"),
            &identity(),
            &operation(600),
            request,
        )
        .await
        .unwrap();
        assert_eq!(result.ducks.len(), 1);
        assert_eq!(
            matches!(&result.ducks[0].outcome, DuckOutcome::Verdict {verdict}
            if verdict.verdict == VerdictKind::Confirmed),
            confirmed,
            "{scenario}: {:?}",
            result.ducks
        );
        stopped(root.path());
        stopped(&root.path().join("duck"));
    }
}

#[tokio::test]
async fn parallel_calls_run_one_at_a_time_and_stop_at_the_preference_limit() {
    let (root, result, _) = review("duck-parallel", "verdict-ok", limits(1, 180), 600).await;
    let result = result.unwrap();
    let outcomes: Vec<_> = result.ducks.iter().map(|c| c.outcome.clone()).collect();
    assert_eq!(outcomes.len(), 2);
    assert!(matches!(outcomes[0], DuckOutcome::Verdict { .. }));
    assert_eq!(
        outcomes[1],
        DuckOutcome::Failure {
            failure: DuckFailure::LimitReached
        }
    );
    assert_eq!(methods(&root.path().join("duck"), "session.create"), 1);
    assert!(duck_results(root.path())
        .iter()
        .any(|r| r.contains("duck_failure: limit_reached")));
}

#[tokio::test]
async fn invalid_input_is_a_tool_error_and_spends_no_call() {
    let (root, result, _) = review("duck-invalid", "verdict-ok", limits(8, 180), 600).await;
    assert!(result.unwrap().ducks.is_empty());
    assert!(duck_results(root.path())[0].contains("duck_failure: invalid_input"));
    assert_eq!(methods(&root.path().join("duck"), "session.create"), 0);
}

fn records(root: &Path, method: &str) -> Vec<Value> {
    receipt(root)
        .into_iter()
        .filter(|r| r["method"] == method)
        .collect()
}

#[tokio::test]
async fn a_slow_duck_times_out_is_aborted_and_closed_and_the_primary_still_exits() {
    let (root, result, _) = review("duck-call", "verdict-slow", limits(8, 1), 600).await;
    let result = result.unwrap();
    assert!(result.output.exit.is_some());
    assert_eq!(
        result.ducks[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::TimedOut
        }
    );
    let duck = root.path().join("duck");
    assert_eq!(methods(&duck, "session.abort"), 1);
    assert_eq!(
        methods(&duck, "session.create"),
        methods(&duck, "session.detach")
    );
}

#[tokio::test]
async fn timed_out_turns_are_aborted_before_the_primary_resumes_and_never_overlap() {
    let (root, result, _) = review("duck-sequential", "verdict-slow", limits(8, 1), 600).await;
    let ducks = result.unwrap().ducks;
    assert_eq!(ducks.len(), 2);
    let duck = root.path().join("duck");
    // Each call's turn is aborted; the second starts with no live duck turn.
    let aborted = records(&duck, "fixture.aborted");
    assert_eq!(aborted.len(), 2);
    let turns: Vec<_> = records(&duck, "fixture.turns")
        .iter()
        .map(|r| r["active"].as_u64().unwrap())
        .collect();
    assert_eq!(turns, vec![0, 0]);
    assert_eq!(
        methods(&duck, "session.create"),
        methods(&duck, "session.detach")
    );
    // The primary gets each result only after that duck turn was aborted.
    let results = records(root.path(), "fixture.duck");
    for (abort, result) in aborted.iter().zip(&results) {
        assert!(abort["at"].as_u64().unwrap() <= result["at"].as_u64().unwrap());
    }
}

#[tokio::test]
async fn a_verdict_session_is_closed_without_an_abort() {
    let (root, result, _) = review("duck-sequential", "verdict-ok", limits(8, 180), 600).await;
    assert_eq!(result.unwrap().ducks.len(), 2);
    let duck = root.path().join("duck");
    assert_eq!(methods(&duck, "session.abort"), 0);
    assert_eq!(methods(&duck, "session.create"), 2);
    assert_eq!(methods(&duck, "session.detach"), 2);
}

#[tokio::test]
async fn a_changed_duck_account_makes_the_next_call_unavailable_without_a_session() {
    let root = tempfile::tempdir().unwrap();
    let mut duck = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        false,
    );
    let account = Arc::new(crate::copilot::operation::AccountWork::default());
    duck.setup.account = Some(
        Operation::new(
            account.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(600),
        )
        .unwrap(),
    );
    // The rate-limit check runs right before each call: the account changes there
    // after the first call finished.
    let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
    duck.setup.gated = Arc::new(move || {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            account.invalidate(|| Ok(())).unwrap();
        }
        false
    });
    let result = execute(
        options(root.path(), "duck-sequential"),
        &identity(),
        &operation(600),
        primary_request(duck.setup),
    )
    .await
    .unwrap();
    assert!(matches!(
        result.ducks[0].outcome,
        DuckOutcome::Verdict { .. }
    ));
    assert_eq!(
        result.ducks[1].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::Unavailable
        }
    );
    assert_eq!(methods(&root.path().join("duck"), "session.create"), 1);
    assert!(duck_results(root.path())[1].contains("duck_failure: unavailable"));
}

#[tokio::test]
async fn a_failed_attempt_keeps_its_duck_calls_in_the_shared_log() {
    let root = tempfile::tempdir().unwrap();
    let duck = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        false,
    );
    let log = duck.setup.log.clone();
    let error = execute(
        options(root.path(), "duck-then-error"),
        &identity(),
        &operation(600),
        primary_request(duck.setup),
    )
    .await
    .unwrap_err();
    assert!(!error.duck_setup);
    let calls = log.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(matches!(calls[0].outcome, DuckOutcome::Verdict { .. }));
}

#[tokio::test]
async fn a_duck_without_an_exit_is_no_verdict_and_a_bad_verdict_is_retried_in_the_loop() {
    let (_root, result, _) = review("duck-call", "verdict-none", limits(8, 180), 600).await;
    assert_eq!(
        result.unwrap().ducks[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::NoVerdict
        }
    );
    let (_root, result, _) = review(
        "duck-call",
        "verdict-revise-bad-then-ok",
        limits(8, 180),
        600,
    )
    .await;
    let ducks = result.unwrap().ducks;
    let DuckOutcome::Verdict { verdict } = &ducks[0].outcome else {
        panic!("{ducks:?}");
    };
    assert_eq!(verdict.verdict, VerdictKind::Revise);
    assert!(verdict.revised_body.is_some());
}

#[tokio::test]
async fn a_duck_rate_limit_is_typed_and_gates_the_duck_account() {
    let (_root, result, paced) = review("duck-call", "verdict-429", limits(8, 180), 600).await;
    assert_eq!(
        result.unwrap().ducks[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::RateLimited
        }
    );
    assert_eq!(*paced.lock().unwrap(), vec![Some(30)]);
    // Already gated: refused at once, no session.
    let root = tempfile::tempdir().unwrap();
    let gated = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        true,
    );
    let result = execute(
        options(root.path(), "duck-call"),
        &identity(),
        &operation(600),
        primary_request(gated.setup),
    )
    .await
    .unwrap();
    assert_eq!(
        result.ducks[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::RateLimited
        }
    );
    assert_eq!(methods(&root.path().join("duck"), "session.create"), 0);
}

#[tokio::test]
async fn too_little_attempt_time_is_budget_exhausted_without_a_session() {
    // 100 s left is inside the 120 s exit reserve.
    let (root, result, _) = review("duck-call", "verdict-ok", limits(8, 180), 100).await;
    assert_eq!(
        result.unwrap().ducks[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::BudgetExhausted
        }
    );
    assert_eq!(methods(&root.path().join("duck"), "session.create"), 0);
}

#[tokio::test]
async fn duck_setup_failure_fails_the_attempt_before_any_inference() {
    for scenario in ["wrong-account", "catalog-error"] {
        let (root, result, _) = review("duck-call", scenario, limits(8, 180), 600).await;
        let error = result.unwrap_err();
        assert!(error.duck_setup, "{scenario}");
        assert_eq!(methods(root.path(), "session.send"), 0, "{scenario}");
        assert_eq!(methods(root.path(), "session.create"), 0, "{scenario}");
        stopped(root.path());
    }
    let root = tempfile::tempdir().unwrap();
    let mut missing = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        false,
    );
    missing.setup.selection.agent.ai_account = None;
    let error = execute(
        options(root.path(), "duck-call"),
        &identity(),
        &operation(600),
        primary_request(missing.setup),
    )
    .await
    .unwrap_err();
    assert!(error.duck_setup);
    assert_eq!(error.kind, crate::monitoring::OperationFailure::Permanent);
}

#[tokio::test]
async fn at_ai_capacity_one_the_duck_runs_inside_the_primarys_slot() {
    let root = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(root.path().join("profile"));
    store
        .save_settings(&crate::storage::Settings {
            capacity: 1,
            ..Default::default()
        })
        .unwrap();
    let coordinator = crate::capacity::Coordinator::default();
    let work = |id: &str| crate::capacity::Work {
        offline: None,
        key: crate::capacity::WorkId {
            kind: crate::capacity::Kind::Normal,
            id: id.into(),
        },
        enqueue_order: 1,
        state: "waiting",
        reason: None,
        failure: None,
        delayed_until: None,
        blocked_by: None,
    };
    let slot = coordinator
        .reserve(&store, &work("primary"), |_| Ok(("op".into(), ())))
        .unwrap();
    assert!(slot.is_some(), "the primary holds the only slot");
    let duck = duck(
        &root.path().join("duck"),
        "verdict-ok",
        limits(8, 180),
        false,
    );
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        execute(
            options(root.path(), "duck-call"),
            &identity(),
            &operation(600),
            primary_request(duck.setup),
        ),
    )
    .await
    .expect("no deadlock")
    .unwrap();
    assert!(matches!(
        result.ducks[0].outcome,
        DuckOutcome::Verdict { .. }
    ));
    assert!(
        coordinator
            .reserve(&store, &work("other"), |_| Ok(("op2".into(), ())))
            .unwrap()
            .is_none(),
        "still one slot, held by the primary alone"
    );
}

#[tokio::test]
async fn a_primary_flooding_rubber_duck_past_the_limit_keeps_a_bounded_log() {
    let (_root, result, _) = review("duck-flood", "verdict-ok", limits(1, 180), 600).await;
    let ducks = result.unwrap().ducks;
    assert_eq!(ducks.len(), 2, "one started call, one folded refusal");
    assert!(matches!(ducks[0].outcome, DuckOutcome::Verdict { .. }));
    assert_eq!(
        ducks[1].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::LimitReached
        }
    );
    assert_eq!(ducks[1].repeats, 98);
    let persisted = serde_json::to_vec(&ducks).unwrap().len();
    assert!(persisted < 8 * 1024, "{persisted} bytes");
}

#[tokio::test]
async fn cancelling_the_primary_mid_call_stops_the_duck_and_leaves_no_turn() {
    let root = tempfile::tempdir().unwrap();
    let duck = duck(
        &root.path().join("duck"),
        "verdict-slow",
        limits(8, 180),
        false,
    );
    let log = duck.setup.log.clone();
    let operation = operation(600);
    let cancel = operation.cancelled.clone();
    let duck_root = root.path().join("duck");
    let watcher = tokio::spawn(async move {
        // Cancel (pause) once the duck turn is in flight.
        loop {
            if std::fs::read_to_string(duck_root.join("receipt.jsonl"))
                .is_ok_and(|r| r.contains("fixture.turns"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        execute(
            options(root.path(), "duck-call"),
            &identity(),
            &operation,
            primary_request(duck.setup),
        ),
    )
    .await
    .expect("cancellation ends the attempt");
    watcher.await.unwrap();
    // The primary either stops on its own watchdog or exits after the duck's typed
    // error; in both cases the operation is cancelled, so the host discards it
    // (`copilot::Integration::review` checks the operation after the run).
    match &outcome {
        Err(error) => assert!(error.cancelled, "{}", error.message),
        Ok(_) => assert!(operation.check().is_err()),
    }
    // The duck client is torn down with the primary: no duck process, no live turn.
    stopped(&root.path().join("duck"));
    stopped(root.path());
    // The in-flight call is recorded as stopped, with its session, never as a verdict.
    let calls = log.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].ordinal, 1);
    assert!(calls[0].packet.is_some() && calls[0].session_id.is_some());
    assert_eq!(
        calls[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::Unavailable
        }
    );
    assert_eq!(methods(&root.path().join("duck"), "session.abort"), 1);
}

#[tokio::test]
async fn a_primary_failing_mid_call_stops_the_duck_at_once_and_records_it() {
    let root = tempfile::tempdir().unwrap();
    let duck = duck(
        &root.path().join("duck"),
        "verdict-slow",
        limits(8, 180),
        false,
    );
    let log = duck.setup.log.clone();
    let started = Instant::now();
    let error = execute(
        options(root.path(), "duck-primary-error"),
        &identity(),
        &operation(600),
        primary_request(duck.setup),
    )
    .await
    .unwrap_err();
    let elapsed = started.elapsed();
    assert!(!error.cancelled && !error.duck_setup, "{}", error.message);
    assert!(
        elapsed < Duration::from_secs(4),
        "teardown took {elapsed:?}"
    );
    stopped(&root.path().join("duck"));
    stopped(root.path());
    let calls = log.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].packet.is_some() && calls[0].session_id.is_some());
    assert_eq!(
        calls[0].outcome,
        DuckOutcome::Failure {
            failure: DuckFailure::Unavailable
        }
    );
    assert_eq!(methods(&root.path().join("duck"), "session.abort"), 1);
}
