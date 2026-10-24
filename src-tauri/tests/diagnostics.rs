use pr_sniper_lib::storage::{DiagnosticEvent, Settings};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

mod support;
use pr_sniper_lib::storage::diagnostics::{
    Attempt, CompletionStage, Connectivity, Coverage, Event, ExitRefusal, FailureKind, Outcome,
    Paths, PublicationStage, Record, RetryDecision, StopReason, ToolName, VerificationField,
};
use support::Fixture;

fn attempt_record(id: &str, event: Event) -> Record {
    Record {
        attempt: Attempt {
            id: id.into(),
            operation_id: "same-operation".into(),
            work_id: "same-work".into(),
            number: 153,
            head: "a".repeat(40),
            model: "fixture-model".into(),
            started_at_ms: Some(1000),
            session_id: Some(format!("session-{id}")),
            runtime_version: Some("fixture-runtime".into()),
        },
        sequence: Some(1),
        elapsed_ms: Some(50),
        event,
    }
}

// Records written before coverage became informational must still load.
fn failure_record(id: &str) -> Record {
    attempt_record(
        id,
        Event::Finished {
            stage: CompletionStage::Attempt,
            failure: Some(FailureKind::IncompleteCoverage),
            coverage: Some(Coverage {
                changed: Paths::new(["one.rs".into(), "two.rs".into()]),
                covered: Paths::new(["one.rs".into()]),
                missing: Paths::new(["two.rs".into()]),
                responsible_tool: ToolName::ReadChanges,
            }),
            stop_reason: StopReason::NotExposed,
            session_events: 3,
            summarized_events: 3,
        },
    )
}

#[test]
fn failures_replacements_and_host_retry_decisions_survive_fresh_readback() {
    let fixture = Fixture::new();
    fixture
        .store()
        .record_attempt(failure_record("first"))
        .unwrap();
    fixture
        .store()
        .record_attempt_decision(
            "same-operation",
            Event::Retry {
                decision: RetryDecision::Scheduled,
                failure: Some(FailureKind::Provider),
                next_attempt_at: Some(2000),
                attempt_count: 1,
            },
        )
        .unwrap();
    fixture
        .store()
        .record_attempt_start(attempt_record("replacement", Event::Started))
        .unwrap();
    let records = serde_json::to_value(fixture.store().diagnostics().unwrap()).unwrap();
    assert_eq!(
        records[0]["attempt"]["event"]["coverage"]["missing"]["paths"],
        serde_json::json!(["two.rs"])
    );
    assert_eq!(records[1]["attempt"]["attempt"]["id"], "first");
    assert_eq!(records[1]["attempt"]["event"]["next_attempt_at"], 2000);
    assert_eq!(records[1]["attempt"]["event"]["failure"], "provider");
    assert_eq!(records[2]["attempt"]["attempt"]["id"], "replacement");
    assert_eq!(
        records[3]["attempt"]["event"]["previous_attempt_id"],
        "first"
    );
    assert_eq!(
        records[3]["attempt"]["event"]["previous_operation_id"],
        "same-operation"
    );
    assert_eq!(
        records[0]["attempt"]["attempt"]["operation_id"],
        records[2]["attempt"]["attempt"]["operation_id"]
    );
    assert_ne!(
        records[0]["attempt"]["attempt"]["session_id"],
        records[2]["attempt"]["attempt"]["session_id"]
    );
}

#[test]
fn replacement_links_cross_manual_operation_but_not_unrelated_work() {
    let fixture = Fixture::new();
    fixture
        .store()
        .record_attempt(failure_record("original"))
        .unwrap();
    let mut unrelated = failure_record("unrelated");
    unrelated.attempt.work_id = "different-work".into();
    fixture.store().record_attempt(unrelated).unwrap();
    let mut next = attempt_record("manual", Event::Started);
    next.attempt.operation_id = "new-operation".into();
    fixture.store().record_attempt_start(next).unwrap();
    let records = serde_json::to_value(fixture.store().diagnostics().unwrap()).unwrap();
    assert_eq!(
        records[3]["attempt"]["event"]["previous_attempt_id"],
        "original"
    );
    assert_eq!(
        records[3]["attempt"]["event"]["previous_operation_id"],
        "same-operation"
    );
    assert_eq!(
        records[3]["attempt"]["attempt"]["operation_id"],
        "new-operation"
    );
}

#[test]
fn nested_duck_evidence_is_bounded_redacted_and_cannot_nest_again() {
    let fixture = Fixture::new();
    let nested = |event| Event::DuckEvent {
        ordinal: 1,
        session_id: Some("ghp_secret".into()),
        model: "duck-model".into(),
        runtime_version: Some("fixture".into()),
        event: Box::new(event),
    };
    fixture
        .store()
        .record_attempt(attempt_record(
            "primary",
            nested(Event::ToolStarted {
                call: 1,
                runtime_call_id: Some("Bearer secret".into()),
                tool: ToolName::ReadChanges,
                requested: Paths::new((0..1000).map(|i| format!("path-{i}"))),
                unknown_paths: 0,
                side: None,
                query_bytes: None,
            }),
        ))
        .unwrap();
    fixture
        .store()
        .record_attempt(attempt_record("primary", nested(nested(Event::Started))))
        .unwrap();
    let records = serde_json::to_value(fixture.store().diagnostics().unwrap()).unwrap();
    assert_eq!(
        records[0]["attempt"]["event"]["event"]["requested"]["paths"]
            .as_array()
            .unwrap()
            .len(),
        128
    );
    assert_eq!(
        records[0]["attempt"]["event"]["event"]["requested"]["omitted"],
        872
    );
    assert_eq!(
        records[1]["attempt"]["event"]["event"]["kind"],
        "evidence_unavailable"
    );
    let export = records.to_string();
    assert!(!export.contains("ghp_secret") && !export.contains("Bearer secret"));
}

#[test]
fn lifecycle_and_publication_seams_store_only_typed_decisions() {
    let fixture = Fixture::new();
    for event in [
        Event::Watchdog {
            failure: FailureKind::InactivityTimeout,
            idle_ms: Some(900_000),
            total_ms: 1_000_000,
        },
        Event::Watchdog {
            failure: FailureKind::TotalDurationTimeout,
            idle_ms: Some(100),
            total_ms: 7_200_001,
        },
        Event::Cancelled,
        Event::Connectivity {
            state: Connectivity::Suspended,
            next_check_at: Some(100),
        },
        Event::Connectivity {
            state: Connectivity::Resumed,
            next_check_at: None,
        },
        Event::Teardown {
            abort: Outcome::Failed,
            shutdown: Outcome::ForcedStopUnverified,
        },
        Event::Publication {
            stage: PublicationStage::VerifyPending,
            mismatch: Some(VerificationField::Body),
            ownership: Outcome::Success,
            failure: None,
            duration_ms: None,
            uncertain: None,
        },
        Event::Publication {
            stage: PublicationStage::Ownership,
            mismatch: Some(VerificationField::Author),
            ownership: Outcome::Failed,
            failure: Some(FailureKind::Provider),
            duration_ms: None,
            uncertain: None,
        },
    ] {
        fixture
            .store()
            .record_attempt(attempt_record("seam", event))
            .unwrap();
    }
    let records = serde_json::to_value(fixture.store().diagnostics().unwrap()).unwrap();
    assert_eq!(
        records[0]["attempt"]["event"]["failure"],
        "inactivity_timeout"
    );
    assert_eq!(records[1]["attempt"]["event"]["total_ms"], 7_200_001_u64);
    assert_eq!(records[3]["attempt"]["event"]["state"], "suspended");
    assert_eq!(records[4]["attempt"]["event"]["state"], "resumed");
    assert_eq!(
        records[5]["attempt"]["event"]["shutdown"],
        "forced_stop_unverified"
    );
    assert_eq!(records[6]["attempt"]["event"]["mismatch"], "body");
    assert_eq!(records[6]["attempt"]["event"]["stage"], "verify_pending");
    assert_eq!(records[7]["attempt"]["event"]["ownership"], "failed");
}

#[test]
fn path_caps_and_secret_redaction_keep_counts_and_distinct_opaque_paths() {
    let fixture = Fixture::new();
    let mut paths = vec![
        "ghp_secret-one.rs".to_string(),
        "github_pat_secret-two.rs".into(),
    ];
    paths.extend((0..200).map(|i| format!("path-{i}.rs")));
    let mut record = attempt_record(
        "safe",
        Event::Coverage {
            coverage: Coverage {
                changed: Paths::new(paths.clone()),
                covered: Paths::new(Vec::new()),
                missing: Paths::new(paths),
                responsible_tool: ToolName::ReadChanges,
            },
        },
    );
    record.attempt.model = "Bearer synthetic-secret".into();
    record.attempt.runtime_version = Some("-----BEGIN PRIVATE KEY-----".into());
    fixture.store().record_attempt(record).unwrap();
    let records = serde_json::to_value(fixture.store().diagnostics().unwrap()).unwrap();
    let changed = &records[0]["attempt"]["event"]["coverage"]["changed"];
    assert_eq!(changed["count"], 202);
    assert_eq!(changed["paths"].as_array().unwrap().len(), 128);
    assert_eq!(changed["omitted"], 74);
    assert_ne!(changed["paths"][0], changed["paths"][1]);
    let text = fs::read_to_string(fixture.path().join("state/attempts.jsonl")).unwrap();
    for secret in [
        "ghp_secret",
        "github_pat_secret",
        "synthetic-secret",
        "PRIVATE KEY",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert!(text.len() <= 128 * 1024);
}

#[test]
fn attempt_rotation_is_capped_and_diagnostics_includes_retained_failure_context() {
    let fixture = Fixture::new();
    fixture
        .store()
        .record_attempt(failure_record("oldest"))
        .unwrap();
    let path = fixture.path().join("state/attempts.jsonl");
    let line = fs::read_to_string(&path).unwrap();
    const LIMIT: usize = 1024 * 1024;
    for round in 1..=5 {
        let current = fs::read_to_string(&path).unwrap();
        let first = current.lines().next().unwrap().to_string() + "\n";
        fs::write(&path, first.repeat(LIMIT / first.len())).unwrap();
        fixture
            .store()
            .record_attempt(failure_record(&format!("attempt-{round}")))
            .unwrap();
    }
    assert!(line.contains("incomplete_coverage"));
    let files: Vec<_> = fs::read_dir(fixture.path().join("state"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    assert_eq!(files.len(), 4);
    assert!(files
        .iter()
        .all(|file| file.metadata().unwrap().len() <= LIMIT as u64));
    let records = fixture.store().diagnostics().unwrap();
    let ids: Vec<_> = records
        .iter()
        .filter_map(|r| r.attempt.as_ref())
        .map(|r| r.attempt.id.as_str())
        .collect();
    assert!(ids.contains(&"attempt-5"));
    assert!(ids.contains(&"attempt-4"));
    assert!(ids.contains(&"attempt-2"));
    assert!(!ids.contains(&"oldest"));
    assert!(records.iter().filter_map(|r| r.attempt.as_ref()).all(|r|
        matches!(&r.event, Event::Finished { coverage: Some(coverage), .. } if coverage.missing.paths == ["two.rs"])
    ));
}

#[test]
fn corrupt_or_oversized_attempt_logs_fail_visibly_without_echoing_content() {
    let fixture = Fixture::new();
    fixture
        .store()
        .record_attempt(failure_record("one"))
        .unwrap();
    let path = fixture.path().join("state/attempts.jsonl");
    let mut record: serde_json::Value =
        serde_json::from_str(fs::read_to_string(&path).unwrap().trim()).unwrap();
    record["attempt"]["event"]["body"] = "PRIVATE_COMMENT".into();
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let error = fixture.store().diagnostics().unwrap_err();
    assert!(!error.contains("PRIVATE_COMMENT"));
    fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(fixture
        .store()
        .diagnostics()
        .unwrap_err()
        .contains("size limit"));
}

#[test]
fn host_panics_and_exit_refusals_keep_typed_cause_in_both_logs() {
    let fixture = Fixture::new();
    let panic = pr_sniper_lib::host_panic::HostPanic {
        message: "index out of bounds: the len is 0 but the index is 3".into(),
        file: "src/review/findings.rs".into(),
        line: 42,
        column: 9,
    };
    let store = fixture.store();
    store
        .record_attempt(attempt_record(
            "refused",
            Event::ExitRefused {
                call: 1,
                refusal: ExitRefusal::ProviderRead,
                provider_error: Some(pr_sniper_lib::github::ConnectionError::RateLimitedAfter(30)),
            },
        ))
        .unwrap();
    store
        .record_attempt(attempt_record(
            "refused",
            Event::HostPanic {
                panic: panic.clone(),
            },
        ))
        .unwrap();
    store.record_host_panic("refused", &panic).unwrap();
    let secret = pr_sniper_lib::host_panic::HostPanic {
        message: "token ghp_abcdef leaked".into(),
        ..panic.clone()
    };
    store.record_host_panic("refused", &secret).unwrap();

    let attempts = attempt_values(&fixture);
    assert_eq!(
        attempts[0]["event"],
        serde_json::json!({"kind":"exit_refused","call":1,"refusal":"provider_read",
            "provider_error":{"rate_limited_after":30}})
    );
    assert_eq!(
        attempts[1]["event"]["panic"]["file"],
        "src/review/findings.rs"
    );
    assert_eq!(attempts[1]["event"]["panic"]["line"], 42);
    let records = serde_json::to_value(store.diagnostics().unwrap()).unwrap();
    let host: Vec<_> = records
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["event"] == "host_panic")
        .collect();
    assert_eq!(host.len(), 2);
    assert_eq!(
        host[0]["panic"],
        serde_json::json!({"attempt":"refused","message":panic.message,
            "file":"src/review/findings.rs","line":42,"column":9})
    );
    let redacted = host[1]["panic"]["message"].as_str().unwrap();
    assert!(redacted.starts_with("[redacted:") && !redacted.contains("ghp_"));
    // A host panic marker cannot be written without its typed record.
    assert!(store.record(DiagnosticEvent::HostPanic).is_err());
}

#[test]
fn diagnostics_append_and_survive_a_fresh_reader() {
    let fixture = Fixture::new();
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    fixture
        .store()
        .record(DiagnosticEvent::SessionStarted)
        .unwrap();

    fixture
        .store()
        .record(DiagnosticEvent::WindowHidden)
        .unwrap();

    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let records = fixture.store().diagnostics().expect("read durable events");
    let actual = serde_json::to_value(&records).unwrap();
    assert_eq!(records.len(), 2, "recording must append, not truncate");
    assert_eq!(actual[0]["event"], "session_started");
    assert_eq!(actual[1]["event"], "window_hidden");
    for record in records {
        assert!(
            (before..=after).contains(&record.timestamp_secs),
            "diagnostics need the actual event time, not a default timestamp"
        );
    }
}

#[test]
fn diagnostic_writes_preserve_separate_configuration() {
    let fixture = Fixture::new();
    fixture
        .store()
        .save_settings(&Settings {
            launch_at_login: true,
            ..Settings::default()
        })
        .unwrap();
    let settings = fixture.path().join("config/settings.json");
    let before = fs::read(&settings).unwrap();

    fixture
        .store()
        .record(DiagnosticEvent::SettingsSaved)
        .unwrap();

    assert_eq!(fs::read(settings).unwrap(), before);
    let lines = fs::read_to_string(fixture.path().join("state/diagnostics.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
    let fields = record.as_object().unwrap();
    assert_eq!(fields.len(), 2, "host diagnostics must use a closed schema");
    assert!(fields.contains_key("timestamp_secs"));
    assert_eq!(fields["event"], "settings_saved");
}

#[test]
fn native_close_request_is_distinct_from_hide_only_dismissal() {
    let fixture = Fixture::new();
    for event in [
        DiagnosticEvent::WindowHidden,
        DiagnosticEvent::WindowCloseRequested,
        DiagnosticEvent::WindowHidden,
    ] {
        fixture.store().record(event).unwrap();
    }
    let records = fixture.store().diagnostics().unwrap();
    let values = serde_json::to_value(records).unwrap();
    assert_eq!(values[0]["event"], "window_hidden");
    assert_eq!(values[1]["event"], "window_close_requested");
    assert_eq!(values[2]["event"], "window_hidden");
    assert!(values
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record.as_object().unwrap().len() == 2));
}

#[test]
fn a_fresh_profile_has_no_diagnostic_events() {
    let fixture = Fixture::new();

    let records = fixture.store().diagnostics().unwrap();

    assert!(records.is_empty());
}

#[test]
fn arbitrary_secret_payloads_are_not_returned_or_echoed_in_errors() {
    let fixture = Fixture::new();
    let directory = fixture.path().join("state");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("diagnostics.jsonl");
    let secret = "synthetic-bearer-secret-12345";
    let original =
        format!("{{\"timestamp_secs\":0,\"event\":\"host_failure\",\"detail\":\"{secret}\"}}\n");
    fs::write(&path, &original).unwrap();

    let error = match fixture.store().diagnostics() {
        Ok(_) => panic!("unknown payload fields must not pass the redacted schema"),
        Err(error) => error,
    };

    assert!(!error.is_empty(), "corrupt diagnostics must remain visible");
    assert!(
        !error.contains(secret),
        "diagnostic errors must not echo secrets"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn unknown_event_text_is_not_echoed_as_a_diagnostic() {
    let fixture = Fixture::new();
    let directory = fixture.path().join("state");
    fs::create_dir(&directory).unwrap();
    let secret = "synthetic-token-in-event";
    fs::write(
        directory.join("diagnostics.jsonl"),
        format!("{{\"timestamp_secs\":0,\"event\":\"{secret}\"}}\n"),
    )
    .unwrap();

    let error = match fixture.store().diagnostics() {
        Ok(_) => panic!("arbitrary event strings must be rejected"),
        Err(error) => error,
    };

    assert!(!error.contains(secret));
    assert!(!error.is_empty());
}

#[test]
fn unreadable_diagnostics_do_not_become_an_empty_healthy_history() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path().join("state/diagnostics.jsonl")).unwrap();

    assert!(fixture.store().diagnostics().is_err());
}

#[test]
fn a_failed_diagnostic_write_does_not_report_success() {
    let fixture = Fixture::new();
    let path = fixture.path().join("state");
    fs::write(&path, b"existing non-directory").unwrap();

    let result = fixture.store().record(DiagnosticEvent::HostFailure);

    assert!(result.is_err());
    assert_eq!(fs::read(path).unwrap(), b"existing non-directory");
}

#[test]
fn crossing_the_log_limit_rotates_before_the_new_record_exceeds_it() {
    let fixture = Fixture::new();
    let directory = fixture.path().join("state");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("diagnostics.jsonl");
    const LIMIT: usize = 256 * 1024;
    let line = "{\"timestamp_secs\":0,\"event\":\"session_started\"}\n";
    let original = line.repeat(LIMIT / line.len());
    fs::write(&path, &original).unwrap();

    fixture
        .store()
        .record(DiagnosticEvent::QuitRequested)
        .unwrap();

    assert!(
        fs::metadata(&path).unwrap().len() <= LIMIT as u64,
        "the current diagnostic log must stay within its declared size bound"
    );
    assert_eq!(
        fs::read_to_string(directory.join("diagnostics.previous.jsonl")).unwrap(),
        original,
        "rotation must preserve the previous history"
    );
    let records = fixture.store().diagnostics().unwrap();
    let actual = serde_json::to_value(records).unwrap();
    assert_eq!(actual.as_array().unwrap().len(), 1);
    assert_eq!(actual[0]["event"], "quit_requested");
}

fn duck(session: Option<&str>, event: Event) -> Event {
    Event::DuckEvent {
        ordinal: 1,
        session_id: session.map(Into::into),
        runtime_version: session.map(|_| "duck-runtime".into()),
        model: "duck-model".into(),
        event: Box::new(event),
    }
}

// One attempt whose header and Duck identity change mid-stream.
fn lifecycle() -> Vec<Record> {
    let mut started = attempt_record("lifecycle", Event::Started);
    started.attempt.session_id = None;
    let session = |event| Event::Session {
        event,
        status_code: None,
        failure: None,
        runtime_error: None,
        tool_success: None,
        aborted: None,
        runtime_call_id: None,
        tool: None,
    };
    let mut records = vec![started.clone()];
    for event in [
        duck(None, Event::Started),
        duck(Some("duck-session"), Event::SessionCreated),
        duck(
            Some("duck-session"),
            session(pr_sniper_lib::storage::diagnostics::SessionEvent::Message),
        ),
        duck(None, Event::RuntimeReady),
    ] {
        records.push(Record {
            event,
            ..started.clone()
        });
    }
    let mut created = attempt_record("lifecycle", Event::SessionCreated);
    created.sequence = Some(6);
    records.push(created.clone());
    records.push(Record {
        event: session(pr_sniper_lib::storage::diagnostics::SessionEvent::Error),
        ..created.clone()
    });
    let paths = || Paths::new(["one.rs".into(), "two.rs".into()]);
    records.push(Record {
        event: Event::ToolStarted {
            call: 1,
            runtime_call_id: Some("call-1".into()),
            tool: ToolName::ReadChanges,
            requested: paths(),
            unknown_paths: 0,
            side: None,
            query_bytes: None,
        },
        ..created.clone()
    });
    records.push(Record {
        event: Event::ToolFinished {
            call: 1,
            runtime_call_id: Some("call-1".into()),
            tool: ToolName::ReadChanges,
            duration_ms: 5,
            response_bytes: Some(10),
            response_limit_bytes: 32_000,
            rejected_by_limit: false,
            returned: paths(),
            covered: Paths::new(["one.rs".into()]),
            failure: None,
        },
        ..created.clone()
    });
    records.push(failure_record("lifecycle"));
    records
}

fn attempt_values(fixture: &Fixture) -> Vec<serde_json::Value> {
    fixture
        .store()
        .diagnostics()
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.attempt)
        .map(|record| serde_json::to_value(record).unwrap())
        .collect()
}

#[test]
fn compact_and_legacy_attempt_lines_read_back_identically() {
    let expected: Vec<_> = lifecycle()
        .into_iter()
        .map(|record| serde_json::to_value(record).unwrap())
        .collect();
    let compact = Fixture::new();
    for record in lifecycle() {
        compact.store().record_attempt(record).unwrap();
    }
    assert_eq!(attempt_values(&compact), expected);
    let text = fs::read_to_string(compact.path().join("state/attempts.jsonl")).unwrap();
    assert_eq!(text.matches("\"work_id\"").count(), 2, "header per change");
    assert_eq!(text.matches("duck-model").count(), 3, "identity per change");
    assert_eq!(
        text.matches("call-1").count(),
        1,
        "tool identity once per call"
    );
    assert!(!text.contains("\"returned\"") && !text.contains("\"missing\""));
    // The Duck declares a changed identity without session/runtime: no nulls.
    let first_duck = text.lines().nth(1).unwrap();
    assert!(first_duck.contains("duck-model") && !first_duck.contains("session_id"));
    assert!(!text.contains("null"), "{text}");

    // Older full-header lines, in a rotated file and before compact lines in
    // the current file, still read back and correlate.
    let legacy = Fixture::new();
    let line = |record: Record| {
        let mut line = serde_json::to_string(&pr_sniper_lib::storage::Diagnostic {
            timestamp_secs: 1,
            event: DiagnosticEvent::Attempt,
            attempt: Some(record),
            panic: None,
        })
        .unwrap();
        line.push('\n');
        line
    };
    let mut older = lifecycle();
    let newer = older.split_off(4);
    let legacy_bytes: usize = lifecycle().into_iter().map(line).map(|l| l.len()).sum();
    assert!(
        text.len() * 3 < legacy_bytes * 2,
        "{} vs {legacy_bytes}",
        text.len()
    );
    fs::create_dir_all(legacy.path().join("state")).unwrap();
    fs::write(
        legacy.path().join("state/attempts.1.jsonl"),
        older.into_iter().map(line).collect::<String>(),
    )
    .unwrap();
    fs::write(
        legacy.path().join("state/attempts.jsonl"),
        line(newer[0].clone()),
    )
    .unwrap();
    for record in newer.into_iter().skip(1) {
        legacy.store().record_attempt(record).unwrap();
    }
    assert_eq!(attempt_values(&legacy), expected);
    legacy
        .store()
        .record_attempt_decision(
            "same-operation",
            Event::Retry {
                decision: RetryDecision::Scheduled,
                failure: Some(FailureKind::Provider),
                next_attempt_at: None,
                attempt_count: 2,
            },
        )
        .unwrap();
    assert_eq!(
        attempt_values(&legacy).last().unwrap()["attempt"],
        expected[9]["attempt"]
    );
}

#[test]
fn every_rotated_attempt_generation_reads_on_its_own() {
    let fixture = Fixture::new();
    let state = fixture.path().join("state");
    let coverage = |round: usize| {
        let paths: Vec<String> = (0..128)
            .map(|i| format!("src/{round}/{i:0>64}.rs"))
            .collect();
        attempt_record(
            "long",
            Event::Coverage {
                coverage: Coverage {
                    changed: Paths::new(paths.clone()),
                    covered: Paths::new(Vec::new()),
                    missing: Paths::new(paths),
                    responsible_tool: ToolName::ReadChanges,
                },
            },
        )
    };
    let mut round = 0;
    while !state.join("attempts.2.jsonl").exists() {
        fixture.store().record_attempt(coverage(round)).unwrap();
        round += 1;
    }
    fixture
        .store()
        .record_attempt(failure_record("long"))
        .unwrap();
    for name in ["attempts.jsonl", "attempts.1.jsonl", "attempts.2.jsonl"] {
        let text = fs::read_to_string(state.join(name)).unwrap();
        let mut lines = text.lines();
        assert!(lines.next().unwrap().contains("\"work_id\""), "{name}");
        assert!(!lines.next().unwrap().contains("\"work_id\""), "{name}");
        // Each generation alone, as the only retained file.
        let alone = Fixture::new();
        fs::create_dir_all(alone.path().join("state")).unwrap();
        fs::write(alone.path().join("state/attempts.jsonl"), &text).unwrap();
        let records = attempt_values(&alone);
        assert_eq!(records.len(), text.lines().count(), "{name}");
        assert!(
            records
                .iter()
                .all(|record| record["attempt"]["work_id"] == "same-work"),
            "{name}"
        );
    }

    // A restart forgets what the file holds: the next line declares in full.
    let before = fs::read_to_string(state.join("attempts.jsonl"))
        .unwrap()
        .lines()
        .count();
    fixture.store().forget_attempt_journal();
    fixture
        .store()
        .record_attempt(failure_record("long"))
        .unwrap();
    let current = fs::read_to_string(state.join("attempts.jsonl")).unwrap();
    assert_eq!(current.lines().count(), before + 1);
    assert!(current.lines().last().unwrap().contains("\"work_id\""));
    let alone = Fixture::new();
    fs::create_dir_all(alone.path().join("state")).unwrap();
    fs::write(alone.path().join("state/attempts.jsonl"), &current).unwrap();
    let records = attempt_values(&alone);
    assert_eq!(records.len(), before + 1);
    assert_eq!(records[before]["event"]["failure"], "incomplete_coverage");
}
