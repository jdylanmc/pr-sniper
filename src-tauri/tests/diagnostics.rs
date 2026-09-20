use pr_sniper_lib::storage::{DiagnosticEvent, Settings};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

mod support;
use support::Fixture;

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
