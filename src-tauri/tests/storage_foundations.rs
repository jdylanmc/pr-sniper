use pr_sniper_lib::storage::{Settings, Store};
use std::fs;

mod support;
use support::Fixture;

#[cfg(windows)]
#[path = "support/windows_permissions.rs"]
mod windows_permissions;

#[test]
fn settings_and_state_roundtrip_in_independent_profiles_with_spaces() {
    let fixture = Fixture::new();
    let first = fixture.path().join("first profile with spaces");
    let second = fixture.path().join("second profile with spaces");
    let settings = Settings {
        root_folder: Some(first.to_str().unwrap().into()),
        ..Settings::default()
    };
    Store::new(first.clone()).save_settings(&settings).unwrap();
    Store::new(first.clone())
        .save_queue_selection(Some("account-bound-job"))
        .unwrap();
    assert_eq!(Store::new(first.clone()).load_settings().unwrap(), settings);
    assert_eq!(
        Store::new(first.clone())
            .load_queue_selection()
            .unwrap()
            .as_deref(),
        Some("account-bound-job")
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(first.join("state/queue-selection.json")).unwrap()
        )
        .unwrap(),
        "account-bound-job"
    );
    assert_eq!(Store::new(second).load_queue_selection().unwrap(), None);
    Store::new(first.clone())
        .save_queue_selection(None)
        .unwrap();
    assert_eq!(Store::new(first).load_queue_selection().unwrap(), None);
}

#[cfg(windows)]
#[test]
fn windows_locked_settings_replacement_preserves_original_and_cleans_stage() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let original = fixture.store().load_settings().unwrap();
    let path = fixture.path().join("config/settings.json");
    let before = fs::read(&path).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let mut changed = original.clone();
    changed.launch_at_login = true;

    assert_eq!(
        fixture.store().save_settings(&changed).unwrap_err(),
        "Cannot replace settings."
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!fixture.path().join("config/settings.json.tmp").exists());
    drop(lock);
    fixture.store().save_settings(&changed).unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), changed);
}

#[cfg(windows)]
#[test]
fn windows_locked_state_replacement_preserves_original_and_cleans_stage() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    fixture
        .store()
        .save_queue_selection(Some("original"))
        .unwrap();
    let path = fixture.path().join("state/queue-selection.json");
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();

    assert_eq!(
        fixture
            .store()
            .save_queue_selection(Some("changed"))
            .unwrap_err(),
        "Cannot replace monitoring state."
    );
    assert_eq!(fs::read(&path).unwrap(), br#""original""#);
    assert!(!fixture
        .path()
        .join("state/queue-selection.json.tmp")
        .exists());
    drop(lock);
    fixture
        .store()
        .save_queue_selection(Some("changed"))
        .unwrap();
    assert_eq!(
        fixture.store().load_queue_selection().unwrap().as_deref(),
        Some("changed")
    );
}

#[cfg(windows)]
#[test]
fn windows_private_acl_survives_replacement_and_diagnostics_creation() {
    let fixture = Fixture::new();
    fixture.store().save_settings(&Settings::default()).unwrap();
    fixture.store().save_queue_selection(Some("old")).unwrap();
    fixture.store().save_queue_selection(Some("new")).unwrap();
    fixture
        .store()
        .record(pr_sniper_lib::storage::DiagnosticEvent::SettingsSaved)
        .unwrap();

    windows_permissions::assert_private(fixture.path(), true);
    windows_permissions::assert_private(&fixture.path().join("config"), true);
    windows_permissions::assert_private(&fixture.path().join("state"), true);
    for path in [
        "config/settings.json",
        "state/queue-selection.json",
        "state/diagnostics.jsonl",
    ] {
        windows_permissions::assert_private(&fixture.path().join(path), false);
    }
}

#[cfg(windows)]
#[test]
fn windows_permission_failure_is_visible_and_never_changes_saved_bytes() {
    let fixture = Fixture::new();
    let original = fixture.store().load_settings().unwrap();
    let before = fs::read(fixture.path().join("config/settings.json")).unwrap();
    let denial = windows_permissions::DenyPermissionChanges::new(&fixture.path().join("config"));
    let mut changed = original.clone();
    changed.launch_at_login = true;

    assert_eq!(
        fixture.store().save_settings(&changed).unwrap_err(),
        "Cannot create configuration directory."
    );
    assert_eq!(
        fs::read(fixture.path().join("config/settings.json")).unwrap(),
        before
    );
    assert!(!fixture.path().join("config/settings.json.tmp").exists());
    drop(denial);
    assert_eq!(fixture.store().load_settings().unwrap(), original);
}

#[test]
fn occupied_stage_is_not_truncated_or_removed_and_saved_state_is_unchanged() {
    let fixture = Fixture::new();
    fixture.store().save_queue_selection(Some("old")).unwrap();
    let stage = fixture.path().join("state/queue-selection.json.tmp");
    fs::write(&stage, b"unowned stage").unwrap();

    assert!(fixture.store().save_queue_selection(Some("new")).is_err());
    assert_eq!(fs::read(stage).unwrap(), b"unowned stage");
    assert_eq!(
        fixture.store().load_queue_selection().unwrap().as_deref(),
        Some("old")
    );
}

#[test]
fn corrupt_state_is_visible_and_not_replaced_on_read() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.path().join("state")).unwrap();
    let path = fixture.path().join("state/queue-selection.json");
    fs::write(&path, b"not json").unwrap();
    assert!(fixture.store().load_queue_selection().is_err());
    assert_eq!(fs::read(path).unwrap(), b"not json");
}

#[cfg(windows)]
#[test]
fn windows_junctions_cannot_redirect_storage_to_another_profile() {
    let inside = Fixture::new();
    let outside = Fixture::new();
    let settings = outside.store().load_settings().unwrap();
    let before = fs::read(outside.path().join("config/settings.json")).unwrap();
    let linked_root = inside.path().join("linked root");
    windows_permissions::junction(outside.path(), &linked_root);
    assert!(Store::new(linked_root).load_settings().is_err());
    windows_permissions::junction(
        &outside.path().join("config"),
        &inside.path().join("config"),
    );
    assert!(inside.store().save_settings(&settings).is_err());
    assert_eq!(
        fs::read(outside.path().join("config/settings.json")).unwrap(),
        before
    );
}

#[cfg(unix)]
#[test]
fn unix_settings_state_and_diagnostics_remain_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.store().save_settings(&Settings::default()).unwrap();
    fixture.store().save_queue_selection(Some("job")).unwrap();
    fixture
        .store()
        .record(pr_sniper_lib::storage::DiagnosticEvent::SessionStarted)
        .unwrap();
    for path in ["", "config", "state"] {
        assert_eq!(
            fs::metadata(fixture.path().join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    for path in [
        "config/settings.json",
        "state/queue-selection.json",
        "state/diagnostics.jsonl",
    ] {
        assert_eq!(
            fs::metadata(fixture.path().join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_discovery_does_not_follow_nested_junctions() {
    let inside = Fixture::new();
    let outside = Fixture::new();
    fs::create_dir(outside.path().join(".git")).unwrap();
    fs::write(
        outside.path().join(".git/config"),
        "[remote \"origin\"]\nurl = https://github.com/private/outside\n",
    )
    .unwrap();
    windows_permissions::junction(outside.path(), &inside.path().join("nested junction"));

    let discovered = pr_sniper_lib::discovery::discover(inside.path()).unwrap();

    assert!(discovered.repositories.is_empty());
    assert!(discovered.warnings.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_discovery_does_not_follow_junction_git_metadata() {
    let inside = Fixture::new();
    let outside = Fixture::new();
    fs::create_dir(outside.path().join(".git")).unwrap();
    fs::write(
        outside.path().join(".git/config"),
        "[remote \"origin\"]\nurl = https://github.com/private/outside\n",
    )
    .unwrap();
    windows_permissions::junction(&outside.path().join(".git"), &inside.path().join(".git"));

    let discovered = pr_sniper_lib::discovery::discover(inside.path()).unwrap();

    assert_eq!(discovered.repositories.len(), 1);
    assert!(discovered.repositories[0].name.is_none());
    assert!(discovered.repositories[0]
        .unavailable
        .as_deref()
        .unwrap()
        .contains("not followed"));
}
