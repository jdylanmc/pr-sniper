use pr_sniper_lib::{
    admission::{AdmissionConfig, AdmissionMode, AdmissionRuleKind},
    storage::{ResourceEdit, Settings, Store},
};
use serde_json::{json, Value};
use std::fs;

const FIRST: &str = "c53ae3be-2ad5-44aa-b257-9298186e1c6f";
const SECOND: &str = "d53ae3be-2ad5-44aa-b257-9298186e1c6f";
const THIRD: &str = "e53ae3be-2ad5-44aa-b257-9298186e1c6f";

fn repository(id: &str, name: &str, extra: Value) -> Value {
    let mut value = json!({
        "id": id, "provider": "github", "name": name, "enabled": true,
        "provider_account_id": "101", "provider_repository_id": (1000 + name.len()).to_string(),
    });
    for (key, extra) in extra.as_object().unwrap() {
        value[key] = extra.clone();
    }
    value
}

fn legacy_file(root: &std::path::Path, document: Value) -> Store {
    let path = root.join("config/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    Store::new(root.to_path_buf())
}

fn kinds(config: &AdmissionConfig) -> Vec<(AdmissionRuleKind, bool)> {
    config.rules.iter().map(|r| (r.kind, r.enabled)).collect()
}

#[test]
fn migration_materializes_legacy_values_and_disables_monitoring() {
    use AdmissionRuleKind::*;
    let root = tempfile::tempdir().unwrap();
    let store = legacy_file(
        root.path(),
        json!({
            "launch_at_login": false,
            "defaults": {
                "schedule": {"kind":"cron","expression":"*/15 * * * *","timezone":"UTC"},
                "watched_authors": [{"id": "7", "login": "global"}],
                "reviewer_assignment": true, "adapter": "copilot", "selector": {"kind":"default"},
                "prompt": "Review.", "automatic_agent_start": false, "automatic_comment_publication": false
            },
            "repositories": [
                repository(FIRST, "o/watched", json!({"watched_authors": [{"id": "9", "login": "local"}]})),
                repository(SECOND, "o/noreviewer", json!({"overrides": {"reviewer_assignment": false}})),
            ]
        }),
    );
    let settings = store.load_settings().unwrap();
    assert_eq!(settings.admission_migration_version, 1);
    let first = &settings.repositories[0];
    let admission = first.admission.as_ref().unwrap();
    assert!(!first.enabled);
    assert_eq!(admission.mode, AdmissionMode::Filtered);
    assert_eq!(
        kinds(admission),
        [
            (IncludeUsers, true),
            (Assigned, true),
            (Mentioned, true),
            (ExcludeUsers, false),
            (IncludePaths, false),
            (ExcludePaths, false)
        ]
    );
    let ids: Vec<_> = admission.rules[0]
        .users
        .as_ref()
        .unwrap()
        .iter()
        .map(|u| u.id.as_str())
        .collect();
    assert_eq!(ids, ["7", "9"]);
    let second = settings.repositories[1].admission.as_ref().unwrap();
    assert!(!settings.repositories[1].enabled);
    // The global watched author applies, reviewer assignment overridden off.
    assert_eq!(kinds(second)[1], (Assigned, false));
    assert!(settings.capability_notice.is_none());
}

#[test]
fn migration_without_watched_authors_uses_mode_all_defaults() {
    let root = tempfile::tempdir().unwrap();
    let store = legacy_file(
        root.path(),
        json!({"launch_at_login": false, "repositories": [repository(FIRST, "o/everyone", json!({}))]}),
    );
    let settings = store.load_settings().unwrap();
    assert_eq!(
        settings.repositories[0].admission,
        Some(AdmissionConfig::default())
    );
    assert!(!settings.repositories[0].enabled);
}

#[test]
fn migration_is_idempotent_and_never_reenables() {
    let root = tempfile::tempdir().unwrap();
    let store = legacy_file(
        root.path(),
        json!({"launch_at_login": false, "repositories": [repository(FIRST, "o/a", json!({}))]}),
    );
    let migrated = store.load_settings().unwrap();
    let on_disk = fs::read(root.path().join("config/settings.json")).unwrap();
    assert!(String::from_utf8_lossy(&on_disk).contains("\"admission_migration_version\": 1"));
    // A human re-enables after review; a later load must not touch it.
    let mut reviewed = migrated.clone();
    reviewed.repositories[0].enabled = true;
    reviewed.repositories[0].admission.as_mut().unwrap().mode = AdmissionMode::Filtered;
    fs::write(
        root.path().join("config/settings.json"),
        serde_json::to_vec_pretty(&reviewed).unwrap(),
    )
    .unwrap();
    assert_eq!(store.load_settings().unwrap(), reviewed);
    assert_eq!(store.load_settings().unwrap(), reviewed);
    assert_eq!(
        fs::read(root.path().join("config/settings.json")).unwrap(),
        serde_json::to_vec_pretty(&reviewed).unwrap(),
        "an up-to-date file is not rewritten"
    );
}

#[test]
fn migration_leaves_repositories_with_admission_alone() {
    let root = tempfile::tempdir().unwrap();
    let configured = json!({"mode": "filtered", "rules": [
        {"kind": "exclude_paths", "enabled": true, "paths": ["docs/"]}
    ]});
    let store = legacy_file(
        root.path(),
        json!({"launch_at_login": false, "repositories": [
            repository(FIRST, "o/has", json!({"admission": configured})),
            repository(SECOND, "o/lacks", json!({})),
        ]}),
    );
    let settings = store.load_settings().unwrap();
    assert!(settings.repositories[0].enabled);
    assert_eq!(
        serde_json::to_value(settings.repositories[0].admission.as_ref().unwrap()).unwrap(),
        configured
    );
    assert!(!settings.repositories[1].enabled);
}

#[test]
fn programmatic_settings_are_current_and_not_migrated() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    let mut settings: Settings = serde_json::from_value(json!({
        "launch_at_login": false, "repositories": [repository(FIRST, "o/a", json!({}))]
    }))
    .unwrap();
    assert!(settings.repositories[0].admission.is_none());
    store.save_settings(&settings).unwrap();
    settings = store.load_settings().unwrap();
    assert!(settings.repositories[0].enabled);
    assert!(settings.repositories[0].admission.is_none());
}

#[test]
fn newer_migration_version_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let store = legacy_file(
        root.path(),
        json!({"launch_at_login": false, "admission_migration_version": 2}),
    );
    assert!(store.load_settings().is_err());
}

#[test]
fn new_repositories_are_seeded_with_the_default_rules() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    let settings = store.add_repository("octo/new").unwrap();
    let repository = &settings.repositories[0];
    assert!(repository.enabled);
    assert_eq!(repository.admission, Some(AdmissionConfig::default()));
    assert_eq!(store.load_settings().unwrap(), settings);
}

#[test]
fn new_repositories_with_default_watched_authors_are_seeded_filtered() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    let mut settings = store.load_settings().unwrap();
    settings.defaults.watched_authors =
        serde_json::from_value(json!([{"id": "7", "login": "g"}])).unwrap();
    store.save_settings(&settings).unwrap();
    let settings = store.add_repository("octo/new").unwrap();
    let admission = settings.repositories[0].admission.as_ref().unwrap();
    assert_eq!(admission.mode, AdmissionMode::Filtered);
    assert_eq!(admission.watched_authors().len(), 1);
}

#[test]
fn saved_new_repository_resources_are_seeded_unless_admission_is_given() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    store.load_settings().unwrap();
    let save = |id: &str, name: &str, extra: Value| {
        let value = serde_json::from_value(repository(id, name, extra)).unwrap();
        store
            .save_resource(ResourceEdit::Repository {
                id: id.into(),
                expected: None,
                value: Some(Box::new(value)),
            })
            .unwrap()
    };
    let settings = save(THIRD, "o/seeded", json!({"enabled": false}));
    assert_eq!(
        settings.repositories[0].admission,
        Some(AdmissionConfig::default())
    );
    let given = json!({"mode": "filtered", "rules": [{"kind": "mentioned", "enabled": true}]});
    let settings = save(
        FIRST,
        "o/given",
        json!({"enabled": false, "admission": given}),
    );
    let saved = settings
        .repositories
        .iter()
        .find(|r| r.id == FIRST)
        .unwrap();
    assert_eq!(
        serde_json::to_value(saved.admission.as_ref().unwrap()).unwrap(),
        given
    );
}

#[test]
fn invalid_admission_is_rejected_on_save_and_unknown_fields_fail_to_parse() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    store.load_settings().unwrap();
    for bad in [
        json!({"mode": "filtered", "rules": [{"kind": "include_paths", "enabled": true, "paths": ["/abs"]}]}),
        json!({"mode": "filtered", "rules": [{"kind": "include_paths", "enabled": true, "paths": ["a/../b"]}]}),
        json!({"mode": "filtered", "rules": [{"kind": "assigned", "enabled": true}, {"kind": "assigned", "enabled": true}]}),
        json!({"mode": "filtered", "rules": [{"kind": "include_users", "enabled": true, "users": [{"id": "0", "login": "x"}]}]}),
    ] {
        let value =
            serde_json::from_value(repository(FIRST, "o/bad", json!({"admission": bad}))).unwrap();
        let result = store.save_resource(ResourceEdit::Repository {
            id: FIRST.into(),
            expected: None,
            value: Some(Box::new(value)),
        });
        assert!(result.is_err(), "{bad}");
    }
    assert!(
        serde_json::from_value::<pr_sniper_lib::storage::Repository>(repository(
            FIRST,
            "o/bad",
            json!({"admission": {"mode": "all", "rules": [], "surprise": true}})
        ))
        .is_err()
    );
    assert!(store.load_settings().unwrap().repositories.is_empty());
}
