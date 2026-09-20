use pr_sniper_lib::storage::{Settings, Store};
use std::fs;

mod support;
use support::Fixture;

#[test]
fn explicit_login_preference_survives_a_fresh_store() {
    let fixture = Fixture::new();
    let store = fixture.store();

    store
        .save_settings(&Settings {
            launch_at_login: true,
            ..Settings::default()
        })
        .expect("persist explicit opt-in in fixture, not the OS login items");
    drop(store);

    let reopened = Store::new(fixture.path().to_path_buf());
    assert!(
        reopened
            .load_settings()
            .expect("read persisted settings through a fresh Store")
            .launch_at_login,
        "an explicitly saved login preference must survive reopening storage"
    );
}

#[test]
fn fresh_profile_is_opted_out_without_creating_settings() {
    let fixture = Fixture::new();

    let settings = fixture.store().load_settings().expect("load new profile");

    assert!(
        !settings.launch_at_login,
        "login must require explicit opt-in"
    );
    assert!(
        !fixture.path().join("config/settings.json").exists(),
        "reading defaults must not create a persisted opt-in"
    );
}

#[test]
fn explicit_opt_out_replaces_the_previous_opt_in() {
    let fixture = Fixture::new();
    fixture
        .store()
        .save_settings(&Settings {
            launch_at_login: true,
            ..Settings::default()
        })
        .expect("arrange previous opt-in");

    fixture
        .store()
        .save_settings(&Settings {
            launch_at_login: false,
            ..Settings::default()
        })
        .expect("persist opt-out");

    assert!(
        !fixture
            .store()
            .load_settings()
            .expect("reopen after opt-out")
            .launch_at_login,
        "a previous opt-in must not reappear after restart"
    );
}

#[test]
fn malformed_settings_report_an_error_without_destroying_the_original() {
    let fixture = Fixture::new();
    let config = fixture.path().join("config");
    fs::create_dir(&config).unwrap();
    let path = config.join("settings.json");
    let original = b"{broken settings";
    fs::write(&path, original).unwrap();

    let error = fixture.store().load_settings().unwrap_err();

    assert!(!error.is_empty(), "invalid settings need a visible error");
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn unknown_secret_fields_are_rejected_without_echoing_their_contents() {
    let fixture = Fixture::new();
    let config = fixture.path().join("config");
    fs::create_dir(&config).unwrap();
    let secret = "synthetic-token-do-not-log-12345";
    fs::write(
        config.join("settings.json"),
        format!(r#"{{"launch_at_login":false,"token":"{secret}"}}"#),
    )
    .unwrap();

    let error = fixture.store().load_settings().unwrap_err();

    assert!(!error.is_empty());
    assert!(
        !error.contains(secret),
        "parse errors must not leak input secrets"
    );
}

#[test]
fn invalid_login_values_are_not_coerced_into_opt_in() {
    let fixture = Fixture::new();
    let config = fixture.path().join("config");
    fs::create_dir(&config).unwrap();
    for value in [r#""true""#, "1", "null", "[]", "{}"] {
        fs::write(
            config.join("settings.json"),
            format!(r#"{{"launch_at_login":{value}}}"#),
        )
        .unwrap();

        assert!(
            fixture.store().load_settings().is_err(),
            "non-Boolean login value {value} must be rejected"
        );
    }
}

#[test]
fn unreadable_settings_are_not_reported_as_a_fresh_profile() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path().join("config/settings.json")).unwrap();

    let result = fixture.store().load_settings();

    assert!(result.is_err(), "an I/O failure must not become defaults");
}

#[test]
fn failed_save_reports_an_error_and_preserves_the_conflicting_file() {
    let fixture = Fixture::new();
    let path = fixture.path().join("config");
    let original = b"existing file blocks config directory";
    fs::write(&path, original).unwrap();

    let result = fixture.store().save_settings(&Settings {
        launch_at_login: true,
        ..Settings::default()
    });

    assert!(
        result.is_err(),
        "an unwritable target must not report success"
    );
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn ordinary_configuration_contains_only_the_host_preference() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.path().join("state")).unwrap();
    let state_path = fixture.path().join("state/fixture-state.json");
    fs::write(&state_path, b"{\"cursor\":\"untouched\"}").unwrap();

    fixture
        .store()
        .save_settings(&Settings {
            launch_at_login: true,
            ..Settings::default()
        })
        .unwrap();

    let bytes = fs::read(fixture.path().join("config/settings.json")).unwrap();
    let persisted: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(persisted, serde_json::json!({ "launch_at_login": true }));
    assert_eq!(
        fs::read(state_path).unwrap(),
        b"{\"cursor\":\"untouched\"}",
        "saving configuration must not overwrite independent state"
    );
}
