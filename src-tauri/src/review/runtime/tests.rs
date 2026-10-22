use super::*;
use crate::{
    copilot::operation::AccountWork,
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::Response,
        review::{ReviewFile, TreeEntry},
        ConnectionError,
    },
    policy::Policy,
    storage::{Agent, AiAccount},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{collections::BTreeMap, path::Path, sync::atomic::AtomicBool, time::Instant};

const OFFERED: [crate::exits::Exit; 2] = [
    crate::exits::Exit::ReadyForHuman,
    crate::exits::Exit::PanicExit,
];

#[path = "outcome_commit_tests.rs"]
mod outcome_commit_tests;

#[test]
fn escaped_client_keeps_runtime_feedback_refusal_for_source_and_log_reads() {
    use crate::review::host::AttemptGuard;
    use std::sync::atomic::AtomicUsize;

    struct Wire {
        calls: Arc<AtomicUsize>,
        changed: Arc<AtomicBool>,
        provider_error: bool,
    }
    impl Transport for Wire {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            let prior_calls = self.calls.fetch_add(1, Ordering::SeqCst);
            if path.ends_with("/logs") {
                return Ok(Response {
                    status: 302,
                    headers: BTreeMap::from([(
                        "location".into(),
                        "https://blob.example/log".into(),
                    )]),
                    body: vec![],
                });
            }
            if prior_calls > 0 {
                self.changed.store(true, Ordering::SeqCst);
                if self.provider_error {
                    return Err(ConnectionError::Network);
                }
            }
            if path.contains("/git/blobs/") {
                Synthetic.get(path)
            } else {
                Ok(Response {
                    status: 200,
                    headers: Default::default(),
                    body: b"{}".to_vec(),
                })
            }
        }

        fn get_unauthenticated_tail(
            &self,
            url: &str,
            _: usize,
        ) -> Result<(Response, bool), ConnectionError> {
            assert_eq!(url, "https://blob.example/log");
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.changed.store(true, Ordering::SeqCst);
            Ok((
                Response {
                    status: 200,
                    headers: Default::default(),
                    body: b"log".to_vec(),
                },
                false,
            ))
        }
    }
    impl crate::github::threads::QueryTransport for Wire {
        fn query(&self, _: &str, _: Value) -> Result<Response, ConnectionError> {
            unreachable!()
        }
    }
    for (log, provider_error) in [(false, false), (true, false), (false, true)] {
        let changed = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let gate_changed = changed.clone();
        let gate: Gate = Arc::new(move || {
            if gate_changed.load(Ordering::SeqCst) {
                Err(Failure::tossed("Owned feedback changed."))
            } else {
                Ok(())
            }
        });
        let context = context(true);
        let attempt = AttemptGuard::new(gate);
        let client = attempt
            .prepare(
                GithubClient::new(Wire {
                    calls: calls.clone(),
                    changed: changed.clone(),
                    provider_error,
                }),
                |client| {
                    client
                        .review_source("example/repo", &context.head["source.rs"])
                        .map_err(Failure::from)?;
                    Ok(client)
                },
            )
            .unwrap();
        let tools = ReadTools::new(
            None,
            context,
            Arc::new(client),
            "example/repo".into(),
            attempt.gate(),
        )
        .with_facts(Arc::new(crate::packet::ToolFacts {
            checks: vec![crate::packet::CheckFact {
                id: Some("1".into()),
                name: "ci".into(),
                status: "COMPLETED".into(),
                conclusion: "FAILURE".into(),
                log: true,
            }],
            ..Default::default()
        }));
        let outcome = if log {
            tools.execute("read_check_log", json!({"id":"1"}))
        } else {
            tools.execute("read_source", json!({"side":"head","path":"source.rs"}))
        };
        let failure = attempt.finish(outcome).unwrap_err();
        let expected_calls = if log { 3 } else { 2 };
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        if provider_error {
            assert_eq!(failure.kind, crate::monitoring::OperationFailure::Network);
            assert_eq!(failure.origin, crate::backpressure::Origin::Github);
        } else {
            assert_eq!(failure.kind, crate::monitoring::OperationFailure::Tossed);
            changed.store(false, Ordering::SeqCst);
            assert_eq!(
                tools.execute("get_checks", json!({})).unwrap_err().kind,
                crate::monitoring::OperationFailure::Tossed
            );
            assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_feedback_refusal_survives_sdk_tool_error_and_teardown() {
    use crate::review::host::AttemptGuard;
    use std::sync::atomic::AtomicUsize;

    struct Wire {
        calls: Arc<AtomicUsize>,
        changed: Arc<AtomicBool>,
    }
    impl Transport for Wire {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
                self.changed.store(true, Ordering::SeqCst);
            }
            Synthetic.get(path)
        }
    }
    impl crate::github::threads::QueryTransport for Wire {
        fn query(&self, _: &str, _: Value) -> Result<Response, ConnectionError> {
            unreachable!()
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(AtomicBool::new(false));
    let gate_changed = changed.clone();
    let attempt = AttemptGuard::new(Arc::new(move || {
        if gate_changed.load(Ordering::SeqCst) {
            Err(Failure::tossed("Owned feedback changed."))
        } else {
            Ok(())
        }
    }));
    let base = request(true);
    let client = attempt
        .prepare(
            GithubClient::new(Wire {
                calls: calls.clone(),
                changed,
            }),
            |client| {
                client
                    .review_source("example/repo", &base.context.head["source.rs"])
                    .map_err(Failure::from)?;
                Ok(Arc::new(client))
            },
        )
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let outcome = execute(
        options(root.path(), "read-source-only"),
        &Identity {
            id: "33".into(),
            login: "review-account".into(),
        },
        &operation(15),
        Request {
            client,
            local_gate: attempt.gate(),
            failure_commit: Some(attempt.committer()),
            inactivity: base.inactivity,
            diagnostics: base.diagnostics,
            context: base.context,
            repository_name: base.repository_name,
            selection: base.selection,
            before_send: base.before_send,
            task: base.task,
            exits: base.exits,
            duck: base.duck,
        },
    )
    .await;
    let error = attempt.finish(outcome).unwrap_err();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(error.kind, crate::monitoring::OperationFailure::Tossed);
    stopped(root.path());
}

/// The run's tool ids: two investigation tools, then the offered exits.
fn tool_names() -> Vec<String> {
    tools::tool_set(
        &["read_changes".into(), "read_source".into()],
        false,
        &OFFERED,
    )
}

pub(super) struct Synthetic;
impl Transport for Synthetic {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        assert!(path.starts_with("/repos/example/repo/git/blobs/"));
        let text = "pub fn answer() -> u32 { 42 }\n";
        Ok(Response{status:200,headers:BTreeMap::new(),body:serde_json::to_vec(&json!({
            "sha":path.rsplit('/').next().unwrap(),"size":text.len(),"encoding":"base64","content":STANDARD.encode(text)
        })).unwrap()})
    }
}

pub(super) fn selection() -> Selection {
    Selection {
        agent: Agent {
            id: "agent".into(),
            name: "Reviewer".into(),
            model: "review-model".into(),
            intelligence: None,
            ai_account: Some(AiAccount {
                provider: "copilot".into(),
                account_id: "33".into(),
            }),
            doctrine: Some("Correctness".into()),
            doctrines: None,
            doctrine_ids: vec![],
            voice_id: None,
            prompt: "Look for defects.".into(),
            signature: "machine".into(),
        },
        policy: Policy::default(),
        doctrine: Some("Trace behavior precisely.".into()),
        preset: None,
        configuration: None,
    }
}
pub(super) fn context(changed: bool) -> ReviewContext {
    let entry = TreeEntry {
        path: "source.rs".into(),
        mode: "100644".into(),
        sha: "c".repeat(40),
        kind: "blob".into(),
    };
    ReviewContext {
        base_revision: "b".repeat(40),
        pull: PullRequest {
            mentioned: false,
            id: "1".into(),
            number: 1,
            title: "Synthetic review".into(),
            author: None,
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
        },
        files: if changed {
            vec![ReviewFile {
                path: "source.rs".into(),
                previous_path: None,
                status: "modified".into(),
                patch: None,
            }]
        } else {
            vec![]
        },
        head: BTreeMap::from([("source.rs".into(), entry.clone())]),
        base: BTreeMap::from([("source.rs".into(), entry)]),
    }
}
fn exit_setup() -> ExitSetup {
    ExitSetup {
        tool_set: tool_names(),
        offered: OFFERED.to_vec(),
        facts: Arc::new(crate::packet::ToolFacts::default()),
        gate: Arc::new(|_: &ExitCall| Ok(())),
    }
}
fn session_config(selection: &Selection, tools: Arc<ReadTools<Synthetic>>) -> SessionConfig {
    let setup = exit_setup();
    let handler = Arc::new(exit_tools::ExitTools::new(setup.offered, setup.gate, None));
    config(selection, tools, &setup.tool_set, handler)
}
fn request(changed: bool) -> Request<Synthetic> {
    Request {
        failure_commit: None,
        inactivity: Duration::from_secs(900),
        exits: exit_setup(),
        diagnostics: None,
        task: ExitReview {
            packet_text: "packet".into(),
        },
        context: context(changed),
        client: Arc::new(GithubClient::new(Synthetic)),
        repository_name: "example/repo".into(),
        selection: selection(),
        before_send: Arc::new(|| Ok(())),
        local_gate: Arc::new(|| Ok(())),
        duck: None,
    }
}
pub(super) fn operation(seconds: u64) -> Operation {
    Operation::new(
        Arc::new(AccountWork::default()),
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(seconds),
    )
    .unwrap()
}
pub(super) fn options(root: &Path, scenario: &str) -> ClientOptions {
    let (node, script) = crate::copilot::runtime::fixture_program("review-runtime.mjs");
    let mut options = crate::copilot::runtime::options(
        node,
        root,
        "fixture",
        std::env::vars_os().map(|(k, _)| k),
    )
    .unwrap()
    .with_prefix_args([script.into_os_string()]);
    for (k, v) in [
        ("REVIEW_SCENARIO", scenario.into()),
        ("TEST_RECEIPT", root.join("receipt.jsonl").into_os_string()),
        (
            "TEST_ABORT_RELEASE",
            root.join("release-abort").into_os_string(),
        ),
    ] {
        options.env_remove.retain(|name| name != k);
        options.env.push((k.into(), v));
    }
    if scenario == "waiting" {
        options
            .env
            .push(("TEST_STARTUP_DELAY_MS".into(), "750".into()));
    }
    options
}
pub(super) fn receipt(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("receipt.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
pub(super) fn stopped(root: &Path) {
    let receipt = receipt(root);
    assert_eq!(receipt[0]["ambient"], json!([]));
    let pid = receipt[0]["pid"].as_u64().unwrap();
    crate::copilot::runtime::assert_process_stopped(pid as u32);
}

#[tokio::test]
async fn unread_files_are_recorded_as_coverage_and_never_fail_the_attempt() {
    for (scenario, read) in [("read-none", 0), ("read-full", 1)] {
        let root = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::new(root.path().join("profile"));
        let mut request = request(true);
        request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
        let result = execute(
            options(root.path(), scenario),
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation(8),
            request,
        )
        .await
        .unwrap();
        assert_eq!(
            result.coverage,
            Some(crate::review::ReadCoverage { read, changed: 1 }),
            "{scenario}"
        );
        assert_eq!(result.byte_coverage, read == 1, "{scenario}");
        stopped(root.path());
        let records = serde_json::to_string(&store.diagnostics().unwrap()).unwrap();
        assert!(!records.contains("incomplete_coverage"), "{records}");
        assert!(records.contains("source.rs"), "{records}");
        let finished = diagnostic_records(root.path())
            .into_iter()
            .find(|r| r["event"]["kind"] == "finished")
            .unwrap();
        assert!(finished["event"]["failure"].is_null(), "{scenario}");
        assert_eq!(finished["event"]["coverage"]["covered"]["count"], read);
        assert_eq!(finished["event"]["coverage"]["changed"]["count"], 1);
    }
}

pub(super) fn diagnostic_trace(path: std::path::PathBuf) -> Arc<crate::review::diagnostics::Trace> {
    crate::review::diagnostics::Trace::new(
        crate::storage::diagnostics::Attempt {
            id: uuid::Uuid::new_v4().to_string(),
            operation_id: "operation".into(),
            work_id: "work".into(),
            number: 1,
            head: "a".repeat(40),
            model: "review-model".into(),
            started_at_ms: Some(1),
            session_id: None,
            runtime_version: None,
        },
        Arc::new(move |record| crate::storage::Store::new(path.clone()).record_attempt(record)),
    )
}
fn explicit_intelligence() -> crate::storage::AgentIntelligence {
    crate::storage::AgentIntelligence {
        reasoning_effort: Some("high".into()),
        context_tier: Some("long_context".into()),
    }
}

pub(super) fn diagnostic_records(root: &Path) -> Vec<Value> {
    crate::storage::Store::new(root.join("profile"))
        .diagnostics()
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.attempt)
        .map(|record| serde_json::to_value(record).unwrap())
        .collect()
}

#[tokio::test]
async fn diagnostic_fake_sessions_distinguish_failure_causes_without_content() {
    for (scenario, changed, expected) in [
        ("read-none", true, None),
        ("malformed", false, Some("invalid_output")),
        ("runtime-error", false, Some("runtime_execution")),
        ("read-source-only", true, None),
        ("tool-invalid-path", true, None),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(changed);
        request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
        let result = execute(
            options(root.path(), scenario),
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation(8),
            request,
        )
        .await;
        assert_eq!(result.is_err(), expected.is_some(), "{scenario}");
        stopped(root.path());
        let records = diagnostic_records(root.path());
        let finished = records
            .iter()
            .find(|r| r["event"]["kind"] == "finished")
            .unwrap();
        assert_eq!(finished["event"]["failure"], json!(expected), "{scenario}");
        assert_eq!(finished["attempt"]["runtime_version"], "synthetic");
        assert!(finished["attempt"]["session_id"].is_string());
        assert!(finished["elapsed_ms"].as_u64().is_some());
        assert!(records
            .iter()
            .any(|r| r["event"]["kind"] == "teardown" && r["event"]["shutdown"] == "success"));
        if scenario == "read-source-only" {
            let tool = records
                .iter()
                .find(|r| r["event"]["kind"] == "tool_finished")
                .unwrap();
            assert_eq!(tool["event"]["returned"]["paths"], json!(["source.rs"]));
            assert_eq!(tool["event"]["covered"]["count"], 0);
            assert!(tool["event"]["response_bytes"].as_u64().unwrap() > 0);
        }
        if scenario == "tool-invalid-path" {
            let tool = records
                .iter()
                .find(|r| r["event"]["kind"] == "tool_finished")
                .unwrap();
            assert_eq!(tool["event"]["failure"], "tool_error");
            assert_eq!(tool["event"]["returned"]["count"], 0);
        }
        if scenario == "runtime-error" {
            assert!(records
                .iter()
                .any(|r| r["event"]["runtime_error"] == "network"
                    && r["event"]["status_code"] == 503));
        }
        let text = serde_json::to_string(&records).unwrap();
        for forbidden in [
            "ghp_secret-runtime-token",
            "SOURCE PRIVATE PROMPT",
            "private source",
            "pub fn answer",
            "Look for defects.",
            "not JSON",
        ] {
            assert!(!text.contains(forbidden), "{scenario}: {forbidden}");
        }
    }
}

#[test]
fn closed_sdk_event_stream_records_loss_without_inventing_a_drop_count() {
    let root = tempfile::tempdir().unwrap();
    let trace = diagnostic_trace(root.path().join("profile"));
    let error = event_stream_failure(
        github_copilot_sdk::subscription::RecvErrorKind::Closed.into(),
        Some(&trace),
    );
    trace.finish(
        crate::storage::diagnostics::CompletionStage::Runtime,
        Some(&error),
    );
    let records = diagnostic_records(root.path());
    assert!(records
        .iter()
        .any(|r| r["event"]["kind"] == "event_stream_lost" && r["event"]["lost_events"].is_null()));
    assert_eq!(
        records.last().unwrap()["event"]["failure"],
        "event_stream_lost"
    );
}

#[test]
fn timeout_failure_alone_does_not_claim_a_watchdog_decision() {
    assert_eq!(
        crate::review::diagnostics::failure_kind(&Failure::timeout()),
        FailureKind::Timeout
    );
}

#[tokio::test]
async fn runtime_truncation_is_not_app_response_limit_rejection() {
    let root = tempfile::tempdir().unwrap();
    let mut request = request(false);
    request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
    execute(
        options(root.path(), "runtime-truncation"),
        &Identity {
            id: "33".into(),
            login: "review-account".into(),
        },
        &operation(8),
        request,
    )
    .await
    .unwrap();
    stopped(root.path());
    let records = diagnostic_records(root.path());
    assert!(
        records
            .iter()
            .any(|r| r["event"]["kind"] == "runtime_truncation"
                && r["event"]["messages_removed"] == 3)
    );
    assert!(records.iter().any(|r| r["event"]["tool_success"] == false));
    assert!(!serde_json::to_string(&records)
        .unwrap()
        .contains("DO NOT RECORD"));
    assert!(!records
        .iter()
        .any(|r| r["event"]["rejected_by_limit"] == true));
}

struct Text(String);
impl Transport for Text {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&json!({
                "sha": path.rsplit('/').next().unwrap(), "size": self.0.len(),
                "encoding": "base64", "content": STANDARD.encode(&self.0),
            }))
            .unwrap(),
        })
    }
}

fn text_tools(text: String, patch: Option<String>) -> ReadTools<Text> {
    let mut context = context(true);
    context.files[0].patch = patch;
    ReadTools::new(
        None,
        context,
        Arc::new(GithubClient::new(Text(text))),
        "example/repo".into(),
        Arc::new(|| Ok(())),
    )
}

fn page(tools: &ReadTools<Text>, tool: &str, args: Value, max_bytes: Option<usize>) -> String {
    let mut out = String::new();
    let mut offset = 0;
    loop {
        let mut call = args.clone();
        call["offset"] = json!(offset);
        if let Some(max) = max_bytes {
            call["max_bytes"] = json!(max);
        }
        let value = tools.execute(tool, call).unwrap();
        assert!(value.to_string().len() <= MAX_TOOL_BYTES);
        assert_eq!(value["offset"], offset);
        out.push_str(value["content"].as_str().unwrap());
        match value["next_offset"].as_u64() {
            Some(next) => {
                assert_eq!(value["complete"], false);
                assert!(next as usize > offset);
                offset = next as usize;
            }
            None => {
                assert_eq!(value["complete"], true);
                assert_eq!(value["total_bytes"], out.len());
                return out;
            }
        }
    }
}

#[test]
fn chunked_reads_reassemble_big_and_multibyte_content_byte_identically() {
    // Quotes, control characters and 2/3/4-byte characters stress escaping and boundaries.
    let unit = "{\"é\":\"日本語𝄞\\n\"}\u{1}\n";
    let text = unit.repeat(40_000);
    assert!(text.len() > 1_000_000);
    let tools = text_tools(text.clone(), Some(text.clone()));
    for max in [
        None,
        Some(1),
        Some(3),
        Some(5),
        Some(7_777),
        Some(usize::MAX / 2),
    ] {
        // Tiny windows would need millions of calls; the small sample covers boundaries.
        let sample = if matches!(max, Some(1 | 3 | 5)) {
            let small = unit.repeat(20);
            let tools = text_tools(small.clone(), None);
            assert_eq!(
                page(
                    &tools,
                    "read_changes",
                    json!({"path":"source.rs","part":"after"}),
                    max
                ),
                small
            );
            continue;
        } else {
            &text
        };
        for part in ["after", "before", "patch"] {
            assert_eq!(
                page(
                    &tools,
                    "read_changes",
                    json!({"path":"source.rs","part":part}),
                    max
                ),
                *sample
            );
        }
        assert_eq!(
            page(
                &tools,
                "read_source",
                json!({"side":"head","path":"source.rs"}),
                max
            ),
            *sample
        );
    }
}

#[test]
fn bad_offsets_are_rejected_and_search_is_paged_losslessly() {
    let tools = text_tools("é".repeat(10), None);
    let args = |offset: u64| json!({"path":"source.rs","part":"after","offset":offset});
    assert!(tools.execute("read_changes", args(1)).is_err());
    assert!(tools.execute("read_changes", args(21)).is_err());
    assert_eq!(
        tools.execute("read_changes", args(20)).unwrap()["complete"],
        true
    );
    assert!(tools
        .execute("read_changes", json!({"path":"source.rs","part":"patch"}))
        .is_err());
    let mut context = context(true);
    for i in 0..3_000 {
        context.head.insert(
            format!("dir/file-{i:05}-{}", "x".repeat(60)),
            context.head["source.rs"].clone(),
        );
    }
    let tools = ReadTools::new(
        None,
        context,
        Arc::new(GithubClient::new(Synthetic)),
        "example/repo".into(),
        Arc::new(|| Ok(())),
    );
    let mut seen = vec![];
    let mut offset = 0;
    loop {
        let value = tools
            .execute("list_tree", json!({"prefix":"dir/","offset":offset}))
            .unwrap();
        assert!(value.to_string().len() <= MAX_TOOL_BYTES);
        seen.extend(
            value["paths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap().to_string()),
        );
        match value["next_offset"].as_u64() {
            Some(next) => offset = next as usize,
            None => break,
        }
    }
    assert_eq!(seen.len(), 3_000);
    assert!(seen.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn the_read_ledger_records_a_file_only_when_every_part_was_fully_served() {
    let text = "line of source\n".repeat(5_000);
    let tools = text_tools(text.clone(), Some("@@ patch @@\n".repeat(100)));
    let read = |part: &str, offset: usize, max: usize| {
        tools
            .execute(
                "read_changes",
                json!({"path":"source.rs","part":part,"offset":offset,"max_bytes":max}),
            )
            .unwrap()
    };
    let first = read("after", 0, 1_000);
    assert_eq!(first["other_parts_unread"], json!(["patch", "before"]));
    // Skipping a gap, repeating a range and reading read_source never complete a file.
    read("after", 2_000, 1_000);
    read("after", 2_000, 1_000);
    tools
        .execute("read_source", json!({"side":"head","path":"source.rs"}))
        .unwrap();
    assert!(tools.read.lock().unwrap().is_empty());
    page(
        &tools,
        "read_changes",
        json!({"path":"source.rs","part":"after"}),
        None,
    );
    page(
        &tools,
        "read_changes",
        json!({"path":"source.rs","part":"before"}),
        None,
    );
    assert!(tools.read.lock().unwrap().is_empty());
    page(
        &tools,
        "read_changes",
        json!({"path":"source.rs","part":"patch"}),
        None,
    );
    assert_eq!(tools.read.lock().unwrap().len(), 1);
}

#[test]
fn partial_reads_are_not_recorded_as_read_even_when_every_part_was_started() {
    let tools = text_tools("x".repeat(100_000), Some("p".repeat(100)));
    for part in ["patch", "before", "after"] {
        let value = tools
            .execute("read_changes", json!({"path":"source.rs","part":part}))
            .unwrap();
        assert!(value["next_offset"].is_null() == (part == "patch"));
    }
    assert!(tools.read.lock().unwrap().is_empty());
}

#[test]
fn limit_hits_are_retryable_failures_never_review_decisions() {
    let root = tempfile::tempdir().unwrap();
    let mut context = context(true);
    let long = format!("dir/{}", "x".repeat(MAX_TOOL_BYTES));
    context.head.insert(long, context.head["source.rs"].clone());
    let tools = ReadTools::new(
        Some(diagnostic_trace(root.path().join("profile"))),
        context,
        Arc::new(GithubClient::new(Synthetic)),
        "example/repo".into(),
        Arc::new(|| Ok(())),
    );
    assert!(tools
        .execute("list_tree", json!({"prefix":"dir/"}))
        .is_err());
    let failure = tools.limit_failure.lock().unwrap().take().unwrap();
    assert!(matches!(
        failure.kind,
        crate::monitoring::OperationFailure::Provider
    ));
    let records = diagnostic_records(root.path());
    let tool = records
        .iter()
        .find(|r| r["event"]["kind"] == "tool_finished")
        .unwrap();
    assert_eq!(tool["event"]["failure"], "response_limit");
    assert_eq!(tool["event"]["rejected_by_limit"], true);
    assert_eq!(tool["event"]["response_limit_bytes"], MAX_TOOL_BYTES);
    assert_eq!(tool["event"]["returned"]["count"], 0);
    assert_eq!(tool["event"]["covered"]["count"], 0);
}

#[tokio::test]
async fn intelligence_is_advertised_transported_read_back_or_rejected_before_inference() {
    for (scenario, explicit, success) in [
        ("success", false, true),
        ("success", true, true),
        ("unsupported-intelligence", true, false),
        ("reject-intelligence", true, false),
        ("ignore-intelligence", true, false),
        ("catalog-error", true, false),
        ("intelligence-readback-error", true, false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(false);
        request.selection.agent.intelligence = Some(if explicit {
            explicit_intelligence()
        } else {
            Default::default()
        });
        let result = execute(
            options(root.path(), scenario),
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation(8),
            request,
        )
        .await;
        assert_eq!(result.is_ok(), success, "{scenario}: {result:?}");
        let receipt = receipt(root.path());
        if success {
            let actual = result.unwrap().intelligence.unwrap();
            assert_eq!(
                actual,
                if explicit {
                    explicit_intelligence()
                } else {
                    Default::default()
                }
            );
            let create = receipt
                .iter()
                .find(|r| r["method"] == "session.create")
                .unwrap();
            if explicit {
                assert_eq!(create["config"]["reasoningEffort"], "high");
                assert_eq!(create["config"]["contextTier"], "long_context");
            } else {
                assert!(create["config"].get("reasoningEffort").is_none());
                assert!(create["config"].get("contextTier").is_none());
            }
        } else {
            assert!(!receipt.iter().any(|r| r["method"] == "session.send"));
            if scenario == "unsupported-intelligence" {
                assert!(!receipt.iter().any(|r| r["method"] == "session.create"));
            }
        }
        stopped(root.path());
    }
}

#[tokio::test]
#[ignore = "explicit offline bundled-runtime acceptance; no credentials or inference"]
async fn bundled_runtime_intelligence_offline() {
    use crate::copilot::runtime::{private_directory, runtime_program, shutdown, with_directory};
    use github_copilot_sdk::ProviderConfig;
    let root = private_directory("pr-sniper-intelligence-offline-").unwrap();
    let path = root.path().to_path_buf();
    let reject_provider = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    reject_provider.set_nonblocking(true).unwrap();
    let mut options = crate::copilot::runtime::options(
        runtime_program().unwrap(),
        root.path(),
        "",
        std::env::vars_os().map(|(key, _)| key),
    )
    .unwrap();
    options.github_token = None;
    options.env_remove.push("COPILOT_SDK_AUTH_TOKEN".into());
    let mut provider = ProviderConfig::default();
    provider.provider_type = Some("openai".into());
    provider.base_url = format!("http://{}/v1", reject_provider.local_addr().unwrap());
    for (key, value) in [
        ("COPILOT_OFFLINE", "true".to_string()),
        ("COPILOT_PROVIDER_BASE_URL", provider.base_url.clone()),
        ("COPILOT_PROVIDER_TYPE", "openai".to_string()),
        ("COPILOT_MODEL", "gpt-5".to_string()),
    ] {
        options.env_remove.retain(|name| {
            !crate::copilot::runtime::environment_key_eq(name, std::ffi::OsStr::new(key))
        });
        options.env.push((key.into(), value.into()));
    }
    let (version, pid, evidence) = with_directory(root, async {
        let client = tokio::time::timeout(Duration::from_secs(30), Client::start(options))
            .await
            .unwrap()
            .unwrap();
        let pid = client.pid().unwrap();
        let version = client.get_status().await.unwrap().version;
        let evidence = tokio::time::timeout(Duration::from_secs(10), async {
            let mut evidence = Vec::new();
            for (effort, tier) in [
                (None, None),
                (Some("high"), Some("default")),
                (Some("low"), Some("long_context")),
            ] {
                let request = request(false);
                let tools = Arc::new(ReadTools::new(
                    None,
                    request.context,
                    request.client,
                    request.repository_name,
                    request.local_gate,
                ));
                let mut selection = selection();
                selection.agent.model = "gpt-5".into();
                selection.agent.intelligence = Some(crate::storage::AgentIntelligence {
                    reasoning_effort: effort.map(String::from),
                    context_tier: tier.map(String::from),
                });
                // Same production session configuration and readback gate; only the provider
                // is an offline rejecting loopback endpoint. Never send a prompt.
                let session = client
                    .create_session(
                        session_config(&selection, tools).with_provider(provider.clone()),
                    )
                    .await
                    .map_err(|_| "Pinned runtime rejected production configuration.".to_string())?;
                let actual = verified_intelligence(&session, &selection)
                    .await
                    .map_err(|error| error.message)?;
                evidence.push(actual);
            }
            Ok::<_, String>(evidence)
        })
        .await;
        shutdown(&client).await;
        (version, pid, evidence)
    })
    .await
    .unwrap();
    let evidence = evidence.unwrap().unwrap();
    assert_eq!(evidence[1].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(evidence[1].context_tier.as_deref(), Some("default"));
    assert_eq!(evidence[2].reasoning_effort.as_deref(), Some("low"));
    assert_eq!(evidence[2].context_tier.as_deref(), Some("long_context"));
    assert!(
        matches!(reject_provider.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
    );
    crate::copilot::runtime::assert_process_stopped(pid);
    assert!(!path.exists());
    eprintln!("offline pinned runtime {version}: production session/readback={evidence:?}; no provider requests, credentials or inference; child stopped and private state removed");
}

#[test]
fn tools_read_immutable_content_and_reject_arbitrary_paths_or_execution() {
    let request = request(true);
    let tools = Arc::new(ReadTools::new(
        None,
        request.context,
        request.client,
        request.repository_name,
        Arc::new(|| Ok(())),
    ));
    let result = tools
        .execute("read_changes", json!({"path":"source.rs","part":"after"}))
        .unwrap();
    assert_eq!(result["content"], "pub fn answer() -> u32 { 42 }\n");
    assert_eq!(result["complete"], true);
    assert!(tools.read.lock().unwrap().is_empty());
    tools
        .execute("read_changes", json!({"path":"source.rs","part":"before"}))
        .unwrap();
    assert_eq!(tools.read.lock().unwrap().len(), 1);
    assert!(tools
        .execute(
            "read_source",
            json!({"side":"head","path":"../../.ssh/id_rsa"})
        )
        .is_err());
    assert!(tools
        .execute("bash", json!({"command":"touch /tmp/forbidden"}))
        .is_err());
    let config = session_config(&selection(), tools);
    assert_eq!(
        config.available_tools.unwrap(),
        tool_names()
            .iter()
            .map(|s| format!("custom:{s}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(config.allowed_models.unwrap(), vec!["review-model"]);
    assert_eq!(config.request_extensions, Some(false));
    assert_eq!(config.enable_file_hooks, Some(false));
    assert_eq!(config.enable_host_git_operations, Some(false));
}

#[test]
fn fixed_evidence_must_reference_head_lines_actually_served_not_merely_cached() {
    let request = request(true);
    let tools = ReadTools::new(
        None,
        request.context,
        request.client,
        request.repository_name,
        Arc::new(|| Ok(())),
    );
    let evidence = crate::review::duck::EvidenceRef {
        path: "source.rs".into(),
        side: Some("head".into()),
        line: Some(1),
        end_line: None,
    };
    assert!(!tools.served_evidence(&evidence));
    tools
        .execute("read_source", json!({"side":"base","path":"source.rs"}))
        .unwrap();
    assert!(!tools.served_evidence(&evidence));
    tools
        .execute(
            "read_source",
            json!({"side":"head","path":"source.rs","max_bytes":1}),
        )
        .unwrap();
    assert!(!tools.served_evidence(&evidence));
    tools
        .execute("read_source", json!({"side":"head","path":"source.rs"}))
        .unwrap();
    assert!(tools.served_evidence(&evidence));
    let mut invalid = evidence.clone();
    invalid.end_line = Some(2);
    assert!(!tools.served_evidence(&invalid));
    invalid = evidence;
    invalid.side = Some("base".into());
    assert!(!tools.served_evidence(&invalid));
}

#[tokio::test]
async fn synthetic_runtime_validates_identity_tools_output_and_reaps_process() {
    for (scenario, success) in [
        ("success", true),
        ("wrong-account", false),
        ("extra-tool", false),
        ("malformed", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let result = execute(
            options(root.path(), scenario),
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation(8),
            request(false),
        )
        .await;
        assert_eq!(
            result.is_ok(),
            success,
            "{scenario}: {result:?}; receipt={:?}",
            receipt(root.path())
        );
        if matches!(scenario, "extra-tool" | "wrong-account") {
            assert!(!receipt(root.path())
                .iter()
                .any(|r| r["method"] == "session.send"));
        }
        stopped(root.path());
    }
}

#[test]
fn intelligence_validation_uses_actual_model_metadata_and_pricing_not_invented_lists() {
    use crate::storage::AgentIntelligence;
    let model = |extra: Value| {
        let mut value = json!({"id":"fixture","name":"Fixture","capabilities":{}});
        for (key, value_extra) in extra.as_object().unwrap() {
            value[key] = value_extra.clone();
        }
        serde_json::from_value::<github_copilot_sdk::Model>(value).unwrap()
    };
    for extra in [
        json!({"supportedReasoningEfforts":["high"],"supportedContextTiers":["default","long_context"]}),
        json!({"supportedReasoningEfforts":["high"],"billing":{"tokenPrices":{"maxPromptTokens":100,"longContext":{"maxPromptTokens":200}}}}),
        json!({"supportedReasoningEfforts":["high"],"billing":{"tokenPrices":{"contextMax":100,"longContext":{"contextMax":200}}}}),
    ] {
        assert!(crate::copilot::runtime::validate_intelligence(
            &model(extra),
            Some(&explicit_intelligence())
        )
        .is_ok());
    }
    for extra in [
        json!({}),
        json!({"supportedReasoningEfforts":["low"],"supportedContextTiers":["default","long_context"]}),
        json!({"capabilities":{"supports":{"reasoningEffort":false}},"supportedReasoningEfforts":["high"],"supportedContextTiers":["default","long_context"]}),
        json!({"supportedReasoningEfforts":["high"],"supportedContextTiers":["default"]}),
        json!({"supportedReasoningEfforts":["high"],"supportedContextTiers":["default","long_context"],"policy":{"state":"disabled"}}),
    ] {
        assert!(crate::copilot::runtime::validate_intelligence(
            &model(extra),
            Some(&explicit_intelligence())
        )
        .is_err());
    }
    assert!(crate::copilot::runtime::validate_intelligence(
        &model(json!({})),
        Some(&AgentIntelligence::default())
    )
    .is_ok());
    let future = AgentIntelligence {
        reasoning_effort: None,
        context_tier: Some("future".into()),
    };
    assert!(crate::copilot::runtime::validate_intelligence(
        &model(json!({"supportedContextTiers":["future"]})),
        Some(&future)
    )
    .is_err());
}

#[tokio::test]
async fn inactivity_retires_silent_and_heartbeat_attempts_before_fresh_replacement() {
    let identity = Identity {
        id: "33".into(),
        login: "review-account".into(),
    };
    for scenario in ["waiting", "heartbeat-only"] {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(false);
        request.inactivity = Duration::from_secs(2);
        request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
        let operation = operation(30);
        let result = execute(
            options(root.path(), scenario),
            &identity,
            &operation,
            request,
        )
        .await;
        assert_eq!(
            result.unwrap_err().kind,
            crate::monitoring::OperationFailure::Timeout
        );
        assert!(Instant::now() < operation.deadline);
        let records = diagnostic_records(root.path());
        let watchdog = records
            .iter()
            .find(|r| r["event"]["kind"] == "watchdog")
            .unwrap();
        assert_eq!(watchdog["event"]["failure"], "inactivity_timeout");
        assert!(watchdog["event"]["idle_ms"].as_u64().unwrap() >= 2000);
        assert!(records.iter().any(|r| r["event"]["kind"] == "teardown"
            && r["event"]["abort"] == "success"
            && r["event"]["shutdown"] == "success"));
        stopped(root.path());
    }
    let root = tempfile::tempdir().unwrap();
    let replacement = execute(
        options(root.path(), "exit-ready"),
        &identity,
        &operation(30),
        request(false),
    )
    .await;
    assert!(replacement.is_ok());
    stopped(root.path());
}

#[test]
fn inactivity_timeout_backoff_obeys_pause_and_the_existing_three_attempt_budget() {
    use crate::capacity::{tests as fixtures, Automation, Coordinator, Dispatch};
    use crate::monitoring::OperationState;
    let (_root, store) = fixtures::fixture(1, 1, true);
    let coordinator = Coordinator::default();
    let mut now = 100;
    for attempt in 1..=3 {
        let batch = coordinator.dispatch(&store, now).unwrap();
        assert!(batch.errors.is_empty());
        assert_eq!(batch.dispatched.len(), 1);
        let worker = &batch.dispatched[0];
        let Dispatch::Review(run, _) = worker;
        assert_eq!(run.operation.attempt_count, attempt);
        crate::review::host::complete(&store, &run.operation.id, Err(Failure::timeout()), now + 1)
            .unwrap();
        coordinator
            .release(&worker.key(), &run.operation.id)
            .unwrap();
        let saved = store.load_reviews().unwrap().remove(0);
        assert!(saved.result.is_none());
        assert_eq!(saved.operation.retry_deadline, 1000);
        if attempt < 3 {
            now = saved.operation.next_attempt_at.unwrap();
            store.save_automation(&Automation { paused: true }).unwrap();
            assert!(coordinator
                .dispatch(&store, now)
                .unwrap()
                .dispatched
                .is_empty());
            assert!(coordinator
                .dispatch(&store, now + 1)
                .unwrap()
                .dispatched
                .is_empty());
            assert_eq!(
                store.load_reviews().unwrap()[0].operation.attempt_count,
                attempt
            );
            store
                .save_automation(&Automation { paused: false })
                .unwrap();
        } else {
            assert_eq!(saved.operation.state, OperationState::ManualRetry);
        }
        assert!(store.load_publications().unwrap().is_empty());
        assert!(store.load_actions().unwrap().effects.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_host_read_expires_and_cannot_return_a_late_result() {
    struct SlowRead(Arc<AtomicBool>);
    impl Transport for SlowRead {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            self.0.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_secs(4));
            Synthetic.get(path)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let base = request(true);
    let request = Request {
        inactivity: Duration::from_secs(2),
        failure_commit: None,
        client: Arc::new(GithubClient::new(SlowRead(called.clone()))),
        diagnostics: Some(diagnostic_trace(root.path().join("profile"))),
        context: base.context,
        repository_name: base.repository_name,
        selection: base.selection,
        before_send: base.before_send,
        local_gate: base.local_gate,
        task: base.task,
        exits: base.exits,
        duck: None,
    };
    let result = execute(
        options(root.path(), "read-full"),
        &Identity {
            id: "33".into(),
            login: "review-account".into(),
        },
        &operation(30),
        request,
    )
    .await;
    assert!(called.load(Ordering::SeqCst));
    assert_eq!(
        result.unwrap_err().kind,
        crate::monitoring::OperationFailure::Timeout
    );
    assert!(
        diagnostic_records(root.path())
            .iter()
            .any(|r| r["event"]["kind"] == "watchdog"
                && r["event"]["failure"] == "inactivity_timeout")
    );
    stopped(root.path());
}

#[test]
fn late_tool_completion_is_rejected_before_success_diagnostics() {
    struct RetiringRead(Arc<Watchdog>);
    impl Transport for RetiringRead {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            self.0.retire();
            Synthetic.get(path)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let progress = Arc::new(Watchdog::new(
        (Instant::now() + Duration::from_secs(60)).into(),
        Duration::from_secs(10),
    ));
    let tools = ReadTools::new(
        Some(diagnostic_trace(root.path().join("profile"))),
        context(true),
        Arc::new(GithubClient::new(RetiringRead(progress.clone()))),
        "example/repo".into(),
        Arc::new(|| Ok(())),
    )
    .with_progress(Some(progress));
    let error = tools
        .execute("read_source", json!({"side":"head","path":"source.rs"}))
        .unwrap_err();
    assert!(error.cancelled);
    let records = diagnostic_records(root.path());
    let finished = records
        .iter()
        .find(|r| r["event"]["kind"] == "tool_finished")
        .unwrap();
    assert_eq!(finished["event"]["failure"], "cancelled");
    assert!(finished["event"]["response_bytes"].is_null());
}

#[tokio::test]
async fn cancellation_timeout_and_last_moment_gate_change_never_complete() {
    for scenario in ["cancel", "timeout", "gate"] {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(false);
        request.diagnostics = Some(diagnostic_trace(root.path().join("profile")));
        if scenario == "gate" {
            request.before_send = Arc::new(|| Err(Failure::permanent("Start gate changed.")));
        }
        let operation = operation(8);
        let cancelled = operation.cancelled.clone();
        let trigger = async {
            let method = if scenario == "gate" {
                "session.tools.getCurrentMetadata"
            } else {
                "session.send"
            };
            crate::copilot::runtime::wait_for_fixture_method(root.path(), method).await;
            assert!(Instant::now() < operation.deadline);
            if scenario == "cancel" {
                cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        };
        let identity = Identity {
            id: "33".into(),
            login: "review-account".into(),
        };
        let (result, ()) = tokio::time::timeout_at(
            (operation.deadline + Duration::from_secs(2)).into(),
            async {
                tokio::join!(
                    execute(
                        options(root.path(), "waiting"),
                        &identity,
                        &operation,
                        request
                    ),
                    trigger
                )
            },
        )
        .await
        .expect("operation deadline must bound the review and cleanup");
        let error = result.unwrap_err();
        if scenario == "cancel" {
            assert!(error.message.contains("cancelled"), "{error:?}");
            let records = diagnostic_records(root.path());
            assert!(records.iter().any(|r| r["event"]["failure"] == "cancelled"));
            assert!(records.iter().any(|r| r["event"]["kind"] == "cancelled"));
        }
        if scenario == "timeout" {
            assert_eq!(error.kind, crate::monitoring::OperationFailure::Timeout);
            assert!(Instant::now() >= operation.deadline);
            let records = diagnostic_records(root.path());
            assert!(records
                .iter()
                .any(|r| r["event"]["failure"] == "total_duration_timeout"));
            let watchdog = records
                .iter()
                .find(|r| r["event"]["kind"] == "watchdog")
                .unwrap();
            assert!(watchdog["event"]["total_ms"].as_u64().unwrap() >= 7000);
            assert!(watchdog["event"]["idle_ms"].is_null());
        }
        if scenario == "gate" {
            assert_eq!(error.message, "Start gate changed.");
            assert!(!receipt(root.path())
                .iter()
                .any(|r| r["method"] == "session.send"));
        }
        stopped(root.path());
    }
}

async fn failure_during_abort(signal: &str, scenario: &str) {
    use crate::{
        capacity::{tests as fixtures, Automation, Coordinator, Dispatch},
        monitoring::{OperationFailure, OperationState},
    };
    use std::sync::atomic::Ordering;

    let (_state, store) = fixtures::fixture(2, 2, true);
    let mut settings = store.load_settings().unwrap();
    for agent in &mut settings.agents {
        agent.model = "review-model".into();
    }
    store.save_settings(&settings).unwrap();
    crate::review::host::request(&store, "normal-2", false, 100).unwrap();
    let archived = store.load_publications().unwrap();
    let coordinator = Coordinator::default();
    let mut batch = coordinator.dispatch(&store, 200).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 2);
    let mut worker = batch.dispatched.pop().unwrap();
    let key = worker.key();
    let Dispatch::Review(run, token) = &worker;
    let (budget, cancelled) = (run.operation.clone(), token.clone());
    assert_eq!(budget.initial_attempt_at, 200);
    assert_eq!(budget.retry_deadline, 1100);
    let mut operation = operation(8);
    operation.cancelled = cancelled;
    let root = tempfile::tempdir().unwrap();
    let finished = AtomicBool::new(false);
    let identity = Identity {
        id: "33".into(),
        login: "review-account".into(),
    };
    let inference = async {
        let base = request(false);
        let Dispatch::Review(run, _) = &mut worker;
        let error = execute(
            options(root.path(), scenario),
            &identity,
            &operation,
            Request {
                selection: run.selection.clone(),
                ..base
            },
        )
        .await
        .unwrap_err();
        stopped(root.path());
        crate::review::host::complete(&store, &budget.id, Err(error.clone()), 203).unwrap();
        let saved = store
            .load_reviews()
            .unwrap()
            .into_iter()
            .find(|r| r.operation.id == run.operation.id)
            .unwrap()
            .operation;
        coordinator.release(&key, &budget.id).unwrap();
        finished.store(true, Ordering::SeqCst);
        (error, saved)
    };
    let interrupt = async {
        crate::copilot::runtime::wait_for_fixture_method(root.path(), "session.abort").await;
        match signal {
            "cancel" => coordinator.cancel(&budget.id).unwrap(),
            "pause" => store.save_automation(&Automation { paused: true }).unwrap(),
            "reduction" => {
                settings.capacity = 1;
                store.save_settings(&settings).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(coordinator
            .dispatch(&store, 201)
            .unwrap()
            .dispatched
            .is_empty());
        assert!(operation.cancelled.load(Ordering::SeqCst));
        // Cross the production monitor's 100 ms poll while abort is still held.
        tokio::time::sleep(Duration::from_millis(250)).await;
        let pending = !finished.load(Ordering::SeqCst);
        let snapshot = coordinator.snapshot(&store, 202).unwrap();
        let blocked = coordinator
            .dispatch(&store, 202)
            .unwrap()
            .dispatched
            .is_empty();
        std::fs::write(root.path().join("release-abort"), b"release").unwrap();
        (pending, snapshot, blocked)
    };
    let ((error, saved), (pending, snapshot, blocked)) = tokio::join!(inference, interrupt);
    assert_eq!(coordinator.snapshot(&store, 203).unwrap().active, 1);
    assert_eq!(store.load_publications().unwrap(), archived);
    assert!(
        !error.cancelled,
        "{signal}/{scenario}: observed failure became {error:?}"
    );
    // Both are retryable: a runtime error, or a run that ended without an exit.
    assert_eq!(error.kind, OperationFailure::Provider);
    assert_eq!(saved.failure, Some(OperationFailure::Provider));
    assert_eq!(saved.attempt_count, budget.attempt_count);
    assert_eq!(saved.initial_attempt_at, budget.initial_attempt_at);
    assert_eq!(saved.retry_deadline, budget.retry_deadline);
    if scenario == "failure-held-abort" {
        assert_eq!(saved.next_attempt_at, Some(210));
    } else {
        assert!(saved.next_attempt_at.is_some());
    }
    assert_eq!(saved.state, OperationState::Queued);
    assert!(pending, "Runtime returned before the held abort response");
    assert_eq!(snapshot.active, 2);
    assert!(snapshot.stopping >= 1);
    assert!(blocked);
    assert!(receipt(root.path())
        .iter()
        .any(|r| r["method"] == "fixture.abortReleased"));
}

#[tokio::test]
async fn r72_observed_review_failure_survives_cancellation_during_abort_and_keeps_budget() {
    for signal in ["pause", "reduction", "cancel"] {
        for scenario in ["failure-held-abort", "malformed-held-abort"] {
            failure_during_abort(signal, scenario).await;
        }
    }
}

#[test]
#[ignore = "explicit live Copilot inference against synthetic source only"]
fn live_restricted_review() {
    let token = zeroize::Zeroizing::new(
        std::env::var("PR_SNIPER_SMOKE_TOKEN").expect("explicit smoke credential"),
    );
    let login = std::env::var("PR_SNIPER_SMOKE_LOGIN").expect("explicit smoke identity");
    let mut request = request(true);
    let pair = TokenPair::new(
        &*token,
        "",
        Duration::from_secs(900),
        Duration::from_secs(900),
    );
    let identity = GithubClient::new(HttpTransport::from_token_pair(&pair).unwrap())
        .current_identity()
        .expect("verify smoke account");
    assert_eq!(identity.login, login);
    let models = crate::copilot::runtime::models(
        &identity,
        &pair,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(90),
    )
    .expect("live account model catalog");
    let model = std::env::var("PR_SNIPER_SMOKE_MODEL").unwrap_or_else(|_| {
        models
            .iter()
            .find(|m| m.id != "auto")
            .expect("concrete account model")
            .id
            .clone()
    });
    assert!(models.iter().any(|m| m.id == model));
    request.selection.agent.model = model.clone();
    let result = run(&identity, &pair, &operation(180), request).expect("restricted live review");
    assert!(result.output.exit.is_some());
    assert_eq!(result.model, model);
    assert!(result.tool_calls > 0);
    println!(
        "Restricted live review complete: runtime={}, tools={}, input_tokens={}, output_tokens={}",
        result.runtime_version, result.tool_calls, result.input_tokens, result.output_tokens
    );
}

mod exits {
    use super::*;
    use crate::exits::{Exit, ExitCall, ExitOutcome};
    use crate::monitoring::OperationFailure;
    use crate::packet::ToolFacts;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    const ALL_EXITS: [Exit; 7] = [
        Exit::Wait,
        Exit::LeaveComments,
        Exit::ReadyForHuman,
        Exit::ApprovePullRequest,
        Exit::ApproveAndMerge,
        Exit::PanicExit,
        Exit::SubmitReport,
    ];

    fn exit_request(offered: &[Exit], gate: ExitGate) -> Request<Synthetic, ExitReview> {
        let granted = ["read_changes".to_string(), "get_thread".to_string()];
        Request {
            failure_commit: None,
            inactivity: Duration::from_secs(900),
            exits: ExitSetup {
                tool_set: tools::tool_set(&granted, false, offered),
                offered: offered.to_vec(),
                facts: Arc::new(ToolFacts::default()),
                gate,
            },
            task: ExitReview {
                packet_text: "packet".into(),
            },
            diagnostics: None,
            context: context(true),
            client: Arc::new(GithubClient::new(Synthetic)),
            repository_name: "example/repo".into(),
            selection: selection(),
            before_send: Arc::new(|| Ok(())),
            local_gate: Arc::new(|| Ok(())),
            duck: None,
        }
    }

    fn open_gate() -> ExitGate {
        Arc::new(|_: &ExitCall| Ok(()))
    }

    async fn run_exit(
        scenario: &str,
        gate: ExitGate,
    ) -> (
        Result<ReviewResult<crate::review::ReviewOutput>, Failure>,
        tempfile::TempDir,
    ) {
        let root = tempfile::tempdir().unwrap();
        let result = execute(
            options(root.path(), scenario),
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation(8),
            exit_request(&ALL_EXITS, gate),
        )
        .await;
        stopped(root.path());
        (result, root)
    }

    fn failures(root: &Path) -> Vec<bool> {
        receipt(root)
            .iter()
            .filter(|r| r["method"] == "fixture.exit")
            .map(|r| r["failed"].as_bool().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn ready_for_human_ends_the_turn_with_no_final_message() {
        let (result, root) = run_exit("exit-ready", open_gate()).await;
        let result = result.unwrap();
        assert_eq!(
            result.output.exit,
            Some(ExitOutcome::ReadyForHuman {
                summary: "No findings.".into()
            })
        );
        assert!(result.output.files.is_empty() && result.output.findings.is_empty());
        assert_eq!(result.input_tokens, 20);
        assert_eq!(failures(root.path()), vec![false]);
    }

    #[tokio::test]
    async fn wait_ends_the_turn_and_stops_the_runtime_without_human_handoff() {
        let (result, root) = run_exit("exit-wait", open_gate()).await;
        let result = result.unwrap();
        assert_eq!(
            result.output.exit,
            Some(ExitOutcome::Wait {
                reason: crate::exits::WaitReason::Ci
            })
        );
        assert_eq!(
            result.output.decision,
            crate::review::Decision::HumanInputRequired
        );
        assert_eq!(failures(root.path()), vec![false]);
        stopped(root.path());
    }

    #[tokio::test]
    async fn leave_comments_stores_the_validated_payload() {
        let (result, _root) = run_exit("exit-comments", open_gate()).await;
        match result.unwrap().output.exit {
            Some(ExitOutcome::Comments { new_comments, .. }) => {
                assert_eq!(new_comments.len(), 1);
                assert_eq!(new_comments[0].path, "source.rs");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_denied_exit_keeps_the_loop_running_and_the_agent_picks_another() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let gate: ExitGate = Arc::new(move |call| {
            seen.fetch_add(1, AtomicOrdering::SeqCst);
            match call {
                ExitCall::LeaveComments { .. } => Err(Refusal::Denied("CI is red.".into())),
                _ => Ok(()),
            }
        });
        let (result, root) = run_exit("exit-denied-then-ok", gate).await;
        assert!(matches!(
            result.unwrap().output.exit,
            Some(ExitOutcome::ReadyForHuman { .. })
        ));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(failures(root.path()), vec![true, false]);
    }

    #[tokio::test]
    async fn idle_without_an_exit_is_a_retryable_agent_quality_failure() {
        let (result, _root) = run_exit("exit-none", open_gate()).await;
        let error = result.unwrap_err();
        assert_eq!(error.kind, OperationFailure::Provider);
        assert!(error.kind.transient());
    }

    #[tokio::test]
    async fn parallel_exit_calls_store_exactly_one_exit() {
        let (result, _root) = run_exit("exit-double", open_gate()).await;
        assert!(result.unwrap().output.exit.is_some());
    }

    #[tokio::test]
    async fn panic_exit_is_a_typed_parking_failure() {
        let (result, _root) = run_exit("panic", open_gate()).await;
        let error = result.unwrap_err();
        assert_eq!(error.kind, OperationFailure::Panic);
        assert!(!error.kind.transient() && error.kind.resumable());
    }

    #[tokio::test]
    async fn a_moved_state_at_exit_time_tosses_the_run_through_the_budget() {
        let gate: ExitGate = Arc::new(|_| Err(Refusal::Moved("PR state moved.".into())));
        let (result, _root) = run_exit("exit-ready", gate).await;
        let error = result.unwrap_err();
        assert_eq!(error.kind, OperationFailure::Tossed);
        assert!(error.kind.transient());
    }

    #[tokio::test]
    async fn approve_exits_store_the_agents_comment_for_the_host_pipeline() {
        let (result, root) = run_exit("exit-approve", open_gate()).await;
        assert_eq!(
            result.unwrap().output.exit,
            Some(ExitOutcome::Approve {
                comment: "Reviewed. Approving.".into()
            })
        );
        assert_eq!(failures(root.path()), vec![false]);
        let (result, _root) = run_exit("exit-merge", open_gate()).await;
        assert_eq!(
            result.unwrap().output.exit,
            Some(ExitOutcome::ApproveAndMerge {
                comment: "Reviewed. Approving and merging.".into()
            })
        );
    }

    #[tokio::test]
    async fn an_approve_denied_on_the_own_pr_lets_the_agent_hand_over_instead() {
        let gate: ExitGate = Arc::new(|call| match call {
            ExitCall::ApprovePullRequest { .. } => Err(Refusal::Denied(
                "Acting account authored this PR. GitHub refuses self-approval.".into(),
            )),
            _ => Ok(()),
        });
        let (result, root) = run_exit("exit-approve-denied-self", gate).await;
        assert!(matches!(
            result.unwrap().output.exit,
            Some(ExitOutcome::ReadyForHuman { .. })
        ));
        assert_eq!(failures(root.path()), vec![true, false]);
    }

    #[tokio::test]
    async fn teammate_report_is_stored_and_an_over_limit_one_is_retried_not_truncated() {
        let (result, root) = run_exit("report-ok", open_gate()).await;
        let output = result.unwrap().output;
        assert_eq!(output.exit, Some(ExitOutcome::SubmitReport));
        assert_eq!(output.report.unwrap().findings[0].nano, "unwrap may panic");
        assert_eq!(failures(root.path()), vec![false]);
        let (result, root) = run_exit("report-too-long", open_gate()).await;
        assert_eq!(
            result.unwrap().output.report.unwrap().findings[0].nano,
            "unwrap may panic"
        );
        assert_eq!(failures(root.path()), vec![true, false]);
    }

    #[tokio::test]
    async fn repeated_malformed_exit_calls_are_a_permanent_contract_violation() {
        let (result, _root) = run_exit("exit-malformed-forever", open_gate()).await;
        let error = result.unwrap_err();
        assert_eq!(error.kind, OperationFailure::Permanent);
        assert!(error.message.contains("repeatedly"));
    }

    #[tokio::test]
    async fn the_catalog_must_equal_the_per_run_tool_set() {
        let (result, _root) = run_exit("exit-catalog-mismatch", open_gate()).await;
        assert!(result.unwrap_err().message.contains("unexpected tools"));
    }

    #[tokio::test]
    async fn the_session_exposes_only_the_per_run_tool_set_with_terminal_exits() {
        let (result, root) = run_exit("exit-ready", open_gate()).await;
        result.unwrap();
        let created = receipt(root.path())
            .into_iter()
            .find(|r| r["method"] == "session.create")
            .unwrap();
        let names: Vec<_> = created["config"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "read_changes",
                "get_thread",
                "wait",
                "leave_comments",
                "ready_for_human",
                "approve_pull_request",
                "approve_and_merge",
                "panic_exit",
                "submit_report"
            ]
        );
        let terminal: Vec<_> = created["config"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["isTerminal"] == true)
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(terminal.len(), 7, "{terminal:?}");
        assert!(!terminal.contains(&"read_changes".to_string()));
    }

    #[test]
    fn pull_tools_page_and_never_cut() {
        use crate::packet::{CheckFact, CommentFact, Conversation, ThreadFact};
        let facts = Arc::new(ToolFacts {
            findings: Default::default(),
            corrections: Vec::new(),
            description: Some("d".repeat(70_000)),
            conversation: Conversation {
                threads: vec![ThreadFact {
                    carried: false,
                    id: "T1".into(),
                    path: None,
                    line: None,
                    resolved: false,
                    outdated: false,
                    mine: false,
                    new_since_last_review: false,
                    targeted_revision: None,
                    comments: vec![CommentFact {
                        author_id: None,
                        id: "7".into(),
                        author: "a".into(),
                        body: "thread body".into(),
                    }],
                }],
                comments: vec![],
            },
            checks: (0..2_000)
                .map(|n| CheckFact {
                    name: format!("check-{n}"),
                    status: "COMPLETED".into(),
                    conclusion: "SUCCESS".into(),
                    id: Some(n.to_string()),
                    log: false,
                })
                .collect(),
        });
        let tools = text_tools("line one\nneedle here\n".into(), None).with_facts(facts);
        let listed = tools.execute("get_thread", json!({})).unwrap();
        assert_eq!(listed["threads"][0]["id"], "T1");
        let thread = tools
            .execute("get_thread", json!({"thread_id":"T1"}))
            .unwrap();
        assert!(thread["content"].as_str().unwrap().contains("thread body"));
        assert!(tools
            .execute("get_thread", json!({"thread_id":"nope"}))
            .is_err());
        let description = tools
            .execute("get_thread", json!({"thread_id":"description"}))
            .unwrap();
        assert_eq!(description["complete"], false);
        assert!(description["next_offset"].as_u64().is_some());
        let mut offset = 0;
        let mut seen = 0;
        loop {
            let page = tools
                .execute("get_checks", json!({"offset":offset}))
                .unwrap();
            assert!(page.to_string().len() <= MAX_TOOL_BYTES);
            seen += page["checks"].as_array().unwrap().len();
            match page["next_offset"].as_u64() {
                Some(next) => offset = next as usize,
                None => break,
            }
        }
        assert_eq!(seen, 2_000);
        let tree = tools
            .execute("list_tree", json!({"prefix":"source"}))
            .unwrap();
        assert_eq!(tree["paths"], json!(["source.rs"]));
        assert!(tools
            .execute("list_tree", json!({"side":"nowhere"}))
            .is_err());
        assert!(!tools.touched_paths().contains("source.rs"));
        let found = tools
            .execute("search_code", json!({"query":"needle"}))
            .unwrap();
        assert_eq!(found["matches"][0]["line"], 2);
        // A returned match anchors its whole file, even when the snippet is cut.
        assert_eq!(found["matches"][0]["path"], "source.rs");
        assert!(tools.touched_paths().contains("source.rs"));
        assert!(tools
            .execute("search_code", json!({"query":"absent"}))
            .unwrap()["matches"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(found["matches"][0]["truncated"], false);
        assert!(tools.execute("search_code", json!({"query":""})).is_err());
    }
}

/// Blocking HTTP as reqwest does it in debug builds: build and drop a runtime.
struct BlockingTransport;
impl Transport for BlockingTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        drop(
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap(),
        );
        Text("fn main() {}\n".into()).get(path)
    }
}

#[test]
fn tool_handlers_run_blocking_http_off_the_async_worker() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let tools = ReadTools::new(
        None,
        context(true),
        Arc::new(GithubClient::new(BlockingTransport)),
        "example/repo".into(),
        Arc::new(|| Ok(())),
    );
    let path = tools.context.files[0].path.clone();
    let read = runtime.block_on(async {
        tokio::spawn(async move {
            off_runtime(|| {
                tools.execute(
                    "read_source",
                    json!({"path": path, "side": "head", "offset": 0}),
                )
            })
        })
        .await
    });
    assert!(read.expect("no panic on the worker").is_ok());
    // Without leaving the worker, the same blocking call panics there.
    let direct = runtime.block_on(async {
        tokio::spawn(async {
            drop(
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap(),
            )
        })
        .await
    });
    assert!(direct.is_err());
}

mod check_logs {
    use super::*;
    use crate::packet::CheckFact;

    /// API answers per job id; the redirect target is only ever fetched credential-free.
    struct LogWire {
        log: Vec<u8>,
        calls: Mutex<Vec<String>>,
    }
    impl Transport for &LogWire {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            self.calls.lock().unwrap().push(format!("api {path}"));
            assert!(
                path.starts_with("/repos/example/repo/actions/jobs/"),
                "{path}"
            );
            let job = path.split('/').nth(6).unwrap();
            let (status, headers): (u16, &[(&str, &str)]) = match job {
                "1" => (302, &[("location", "https://blob.example/job-1?sig=s")]),
                "2" => (404, &[]),
                "3" => (
                    403,
                    &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "99")],
                ),
                "4" => (410, &[]),
                _ => (500, &[]),
            };
            Ok(Response {
                status,
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body: b"{}".to_vec(),
            })
        }
        fn get_unauthenticated_tail(
            &self,
            url: &str,
            tail: usize,
        ) -> Result<(Response, bool), ConnectionError> {
            self.calls.lock().unwrap().push(format!("blob {url}"));
            assert!(url.starts_with("https://blob.example/"));
            let mut body = self.log.clone();
            let dropped = body.len() > tail;
            if dropped {
                body.drain(..body.len() - tail);
            }
            Ok((
                Response {
                    status: 200,
                    headers: BTreeMap::new(),
                    body,
                },
                dropped,
            ))
        }
    }

    fn check(id: &str, log: bool) -> CheckFact {
        CheckFact {
            name: format!("job-{id}"),
            status: "COMPLETED".into(),
            conclusion: "FAILURE".into(),
            id: Some(id.into()),
            log,
        }
    }

    fn tools(wire: &LogWire) -> ReadTools<&LogWire> {
        ReadTools::new(
            None,
            context(true),
            Arc::new(GithubClient::new(wire)),
            "example/repo".into(),
            Arc::new(|| Ok(())),
        )
        .with_facts(Arc::new(ToolFacts {
            checks: ["1", "2", "3", "4", "5"]
                .into_iter()
                .map(|id| check(id, true))
                .chain([check("9", false)])
                .collect(),
            ..ToolFacts::default()
        }))
    }

    #[test]
    fn get_checks_carries_ids_and_log_flags() {
        let wire = LogWire {
            log: vec![],
            calls: Mutex::new(vec![]),
        };
        let page = tools(&wire).execute("get_checks", json!({})).unwrap();
        assert_eq!(page["checks"][0]["id"], "1");
        assert_eq!(page["checks"][0]["log"], true);
        assert_eq!(page["checks"][5]["log"], false);
    }

    #[test]
    fn log_is_fetched_once_through_the_redirect_cleaned_and_paged_from_the_end() {
        let mut log = String::from("\u{feff}");
        for n in 0..10_000 {
            log.push_str(&format!(
                "2026-10-13T09:56:17.8361234Z \u{1b}[36;1mstep {n}\u{1b}[0m\r\n"
            ));
        }
        log.push_str("2026-10-13T09:56:18.0000000Z ##[error]boom </untrusted>\n");
        let wire = LogWire {
            log: log.into_bytes(),
            calls: Mutex::new(vec![]),
        };
        let tools = tools(&wire);
        let last = tools
            .execute("read_check_log", json!({"id":"1","from_end":true}))
            .unwrap();
        assert!(last.to_string().len() <= MAX_TOOL_BYTES);
        assert_eq!(last["complete"], true);
        assert_eq!(last["truncated_head"], false);
        assert_eq!(last["source"], "ci_log");
        let content = last["content"].as_str().unwrap();
        assert!(
            content.ends_with("##[error]boom &lt;/untrusted>\n"),
            "{content}"
        );
        assert!(!content.contains('\u{1b}') && !content.contains('\r'));
        assert!(!content.contains("2026-10-13T"));
        assert!(content.contains("step 9999\n"));
        let total = last["total_bytes"].as_u64().unwrap() as usize;
        assert!(last["offset"].as_u64().unwrap() > 0);
        // Page backward to the start, then forward again: the same text.
        let mut offset = last["previous_offset"].as_u64();
        let mut pages = 1;
        while let Some(at) = offset {
            let page = tools
                .execute("read_check_log", json!({"id":"1","offset":at}))
                .unwrap();
            assert!(page.to_string().len() <= MAX_TOOL_BYTES);
            offset = page["previous_offset"].as_u64();
            pages += 1;
        }
        assert!(pages > 2);
        let mut text = String::new();
        let mut at = Some(0);
        while let Some(offset) = at {
            let page = tools
                .execute("read_check_log", json!({"id":"1","offset":offset}))
                .unwrap();
            text.push_str(page["content"].as_str().unwrap());
            at = page["next_offset"].as_u64();
        }
        assert_eq!(text.len(), total);
        assert!(text.starts_with("step 0\nstep 1\n"));
        let calls = wire.calls.lock().unwrap();
        assert_eq!(
            *calls,
            [
                "api /repos/example/repo/actions/jobs/1/logs",
                "blob https://blob.example/job-1?sig=s"
            ]
        );
        assert!(tools
            .execute(
                "read_check_log",
                json!({"id":"1","from_end":true,"offset":0})
            )
            .is_err());
    }

    #[test]
    fn an_oversized_log_keeps_its_tail_and_says_so() {
        let line = "2026-10-13T09:56:17.8361234Z padding padding padding padding\n";
        let mut log = line.repeat(tools::MAX_LOG_BYTES / line.len() + 1000);
        log.push_str("2026-10-13T09:56:18Z final failure\n");
        let wire = LogWire {
            log: log.into_bytes(),
            calls: Mutex::new(vec![]),
        };
        let tools = tools(&wire);
        let last = tools
            .execute("read_check_log", json!({"id":"1","from_end":true}))
            .unwrap();
        assert_eq!(last["truncated_head"], true);
        assert!(last["content"]
            .as_str()
            .unwrap()
            .ends_with("final failure\n"));
        let first = tools.execute("read_check_log", json!({"id":"1"})).unwrap();
        assert_eq!(first["truncated_head"], true);
        assert!(first["content"].as_str().unwrap().starts_with("padding"));
        assert!(first["total_bytes"].as_u64().unwrap() <= tools::MAX_LOG_BYTES as u64);
    }

    #[test]
    fn a_missing_log_is_a_tool_error_and_a_rate_limit_is_backpressure() {
        let wire = LogWire {
            log: vec![],
            calls: Mutex::new(vec![]),
        };
        let tools = tools(&wire);
        for id in ["2", "4", "9", "nope"] {
            let error = tools
                .execute("read_check_log", json!({"id":id}))
                .unwrap_err();
            assert_eq!(error.message, tools::NO_LOG, "{id}");
            assert!(!error.rate_limited());
        }
        let failed = tools
            .execute("read_check_log", json!({"id":"5"}))
            .unwrap_err();
        assert!(matches!(
            failed.kind,
            crate::monitoring::OperationFailure::Permanent
        ));
        assert!(tools.source_failure.lock().unwrap().is_none());
        // A missing log is cached: no second request.
        tools
            .execute("read_check_log", json!({"id":"2"}))
            .unwrap_err();
        assert_eq!(
            wire.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.ends_with("/jobs/2/logs"))
                .count(),
            1
        );
        let limited = tools
            .execute("read_check_log", json!({"id":"3"}))
            .unwrap_err();
        assert!(limited.rate_limited());
        assert!(tools
            .source_failure
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(Failure::rate_limited));
    }
}
