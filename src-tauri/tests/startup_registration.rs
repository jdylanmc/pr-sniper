mod support;

use pr_sniper_lib::startup::{LoginRegistration, RegistrationStatus};
use pr_sniper_lib::storage::Settings;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use support::Fixture;

fn executable(fixture: &Fixture) -> std::path::PathBuf {
    let path = fixture.path().join("PR & Sniper");
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn registration(fixture: &Fixture) -> LoginRegistration {
    LoginRegistration::new(fixture.path().join("PR Sniper.plist"), executable(fixture))
}

#[test]
fn missing_registration_is_absent_without_creating_a_file() {
    let fixture = Fixture::new();
    assert_eq!(
        registration(&fixture).status().unwrap(),
        RegistrationStatus::Absent
    );
    assert!(!fixture.path().join("PR Sniper.plist").exists());
}

#[test]
fn explicit_request_writes_a_valid_escaped_registration_and_saved_intent() {
    let fixture = Fixture::new();
    registration(&fixture)
        .set_enabled(&fixture.store(), true)
        .unwrap();
    let path = fixture.path().join("PR Sniper.plist");
    let plist = plist::Value::from_file(&path).unwrap();
    assert_eq!(
        plist.as_dictionary().unwrap()["ProgramArguments"]
            .as_array()
            .unwrap()[0]
            .as_string(),
        Some(fixture.path().join("PR & Sniper").to_str().unwrap())
    );
    assert!(fs::read_to_string(path)
        .unwrap()
        .contains("PR &amp; Sniper"));
    assert!(fixture.store().load_settings().unwrap().launch_at_login);
    assert_eq!(
        registration(&fixture).status().unwrap(),
        RegistrationStatus::Registered
    );
    assert_eq!(
        serde_json::to_string(&RegistrationStatus::Registered).unwrap(),
        "\"registered\"",
        "Registration must not serialize as effective enabled"
    );
}

#[test]
fn malformed_registration_is_invalid_not_registered_or_an_error_echo() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("PR Sniper.plist"),
        "secret=synthetic-token",
    )
    .unwrap();
    assert_eq!(
        registration(&fixture).status().unwrap(),
        RegistrationStatus::Invalid
    );
}

#[test]
fn registration_for_an_old_executable_is_invalid() {
    let fixture = Fixture::new();
    registration(&fixture)
        .set_enabled(&fixture.store(), true)
        .unwrap();
    let current = fixture.path().join("moved-app");
    fs::write(&current, "new app").unwrap();
    fs::set_permissions(&current, fs::Permissions::from_mode(0o700)).unwrap();
    let moved = LoginRegistration::new(fixture.path().join("PR Sniper.plist"), current);
    assert_eq!(moved.status().unwrap(), RegistrationStatus::Invalid);
}

#[test]
fn missing_registered_executable_is_invalid() {
    let fixture = Fixture::new();
    let registration = registration(&fixture);
    registration.set_enabled(&fixture.store(), true).unwrap();
    fs::remove_file(fixture.path().join("PR & Sniper")).unwrap();
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Invalid);
}

#[test]
fn unreadable_registration_is_an_explicit_safe_error() {
    let fixture = Fixture::new();
    let path = fixture.path().join("PR Sniper.plist");
    fs::create_dir(&path).unwrap();
    let error = registration(&fixture).status().unwrap_err();
    assert_eq!(error, "Cannot read the launch-at-login registration.");
    assert!(!error.contains(fixture.path().to_str().unwrap()));
}

#[test]
fn permission_denied_registration_is_not_treated_as_absent() {
    let fixture = Fixture::new();
    let registration = registration(&fixture);
    let path = fixture.path().join("PR Sniper.plist");
    fs::write(&path, "synthetic sensitive contents").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let result = registration.status();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        result.unwrap_err(),
        "Cannot read the launch-at-login registration."
    );
}

#[test]
fn explicit_opt_out_removes_registration_and_persists_false() {
    let fixture = Fixture::new();
    let registration = registration(&fixture);
    registration.set_enabled(&fixture.store(), true).unwrap();
    registration.set_enabled(&fixture.store(), false).unwrap();
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Absent);
    assert!(!fixture.store().load_settings().unwrap().launch_at_login);
}

#[test]
fn failed_settings_save_restores_exact_previous_registration_bytes() {
    let fixture = Fixture::new();
    let registration = registration(&fixture);
    fixture.store().save_settings(&Settings::default()).unwrap();
    fs::create_dir(fixture.path().join("config/settings.json.tmp")).unwrap();
    let previous = b"malformed prior registration preserved verbatim";
    let path = fixture.path().join("PR Sniper.plist");
    fs::write(&path, previous).unwrap();
    assert!(registration.set_enabled(&fixture.store(), true).is_err());
    assert_eq!(fs::read(path).unwrap(), previous);
    assert!(!fixture.store().load_settings().unwrap().launch_at_login);
}

#[test]
fn failed_settings_save_does_not_leave_new_registration_enabled() {
    let fixture = Fixture::new();
    let registration = registration(&fixture);
    fixture.store().save_settings(&Settings::default()).unwrap();
    fs::create_dir(fixture.path().join("config/settings.json.tmp")).unwrap();
    assert!(registration.set_enabled(&fixture.store(), true).is_err());
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Absent);
}

#[test]
fn failed_registration_write_leaves_settings_and_prior_bytes_unchanged() {
    let fixture = Fixture::new();
    let registration = registration(&fixture);
    let previous = b"prior registration";
    fs::write(fixture.path().join("PR Sniper.plist"), previous).unwrap();
    fs::write(
        fixture
            .path()
            .join(format!(".pr-sniper-{}.tmp", std::process::id())),
        "occupied",
    )
    .unwrap();
    assert!(registration.set_enabled(&fixture.store(), true).is_err());
    assert_eq!(
        fs::read(fixture.path().join("PR Sniper.plist")).unwrap(),
        previous
    );
    assert!(!fixture.path().join("config/settings.json").exists());
}
