use pr_sniper_lib::storage::{DiagnosticEvent, Settings};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

mod support;
use pr_sniper_lib::storage::diagnostics::{
    Attempt, CompletionStage, Connectivity, Coverage, Event, FailureKind, Outcome, Paths,
    PublicationStage, Record, RetryDecision, StopReason, ToolName, VerificationField,
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
        .record_attempt(attempt_record("replacement", Event::Started))
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
        records[0]["attempt"]["attempt"]["operation_id"],
        records[2]["attempt"]["attempt"]["operation_id"]
    );
    assert_ne!(
        records[0]["attempt"]["attempt"]["session_id"],
        records[2]["attempt"]["attempt"]["session_id"]
    );
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
        },
        Event::Publication {
            stage: PublicationStage::Ownership,
            mismatch: Some(VerificationField::Author),
            ownership: Outcome::Failed,
            failure: Some(FailureKind::Provider),
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
