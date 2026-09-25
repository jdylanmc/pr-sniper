use pr_sniper_lib::{
    policy::PolicyOverrides,
    storage::{ProviderId, Repository, Settings, Store},
};
use std::fs;

mod support;
use support::Fixture;

#[test]
fn legacy_and_disconnected_ai_agents_load_without_remapping() {
    let fixture = Fixture::new();
    let mut value = serde_json::to_value(Settings::default()).unwrap();
    value["agents"] = serde_json::json!([
        {"id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "name":"Legacy", "model":"copilot", "prompt":"Review carefully.", "signature":"Fixture"},
        {"id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "name":"Pinned", "model":"returned-model", "ai_account":{"provider":"copilot","account_id":"101"}, "prompt":"Review carefully.", "signature":"Fixture"}
    ]);
    let settings: Settings = serde_json::from_value(value).unwrap();
    fixture.store().save_settings(&settings).unwrap();
    let reloaded = fixture.store().load_settings().unwrap();
    assert_eq!(settings, reloaded);
    assert!(reloaded.agents[0].ai_account.is_none());
    assert_eq!(reloaded.agents[0].model, "copilot");
    assert_eq!(
        reloaded.agents[1].ai_account.as_ref().unwrap().account_id,
        "101"
    );
    let mut invalid = settings.clone();
    invalid.agents[1].ai_account.as_mut().unwrap().account_id = "mutable-login".into();
    assert!(fixture.store().save_settings(&invalid).is_err());
    assert_eq!(fixture.store().load_settings().unwrap(), settings);
}
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
fn fresh_profile_persists_starters_without_opting_in_to_startup() {
    let fixture = Fixture::new();

    let settings = fixture.store().load_settings().expect("load new profile");

    assert!(
        !settings.launch_at_login,
        "login must require explicit opt-in"
    );
    assert!(
        fixture.path().join("config/settings.json").exists(),
        "starter doctrines must be persisted without enabling startup"
    );
    assert!(!fixture.store().load_settings().unwrap().launch_at_login);
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
fn ordinary_configuration_contains_only_nonsecret_settings() {
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
    assert_eq!(
        persisted,
        serde_json::json!({
            "launch_at_login": true,
            "doctrines": [],
            "defaults": {
                "schedule": {"kind":"interval","minutes":15,"timezone":"UTC"},
                "watched_authors": [],
                "reviewer_assignment": true,
                "adapter": "copilot",
                "selector": {"kind":"default"},
                "prompt": "Review this pull request for actionable defects.",
                "automatic_agent_start": false,
                "automatic_comment_publication": false
            }
        })
    );
    assert_eq!(
        fs::read(state_path).unwrap(),
        b"{\"cursor\":\"untouched\"}",
        "saving configuration must not overwrite independent state"
    );
}

#[test]
fn legacy_installation_binding_is_migrated_to_an_explicitly_unbound_repository() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings_path = fixture.path().join("config/settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        br#"{
          "launch_at_login": false,
          "repositories": [{
            "id": "c53ae3be-2ad5-44aa-b257-9298186e1c6f",
            "provider": "github",
            "name": "octo/example",
            "enabled": true,
            "provider_account_id": "101",
            "installation_id": "9001",
            "provider_repository_id": "42"
          }]
        }"#,
    )
    .unwrap();
    let restored = store.load_settings().unwrap();

    assert!(restored.repositories[0].account_binding().is_none());
    assert!(restored.repositories[0].provider_account_id.is_none());
    assert!(restored.repositories[0].provider_repository_id.is_none());
    assert!(!String::from_utf8(fs::read(settings_path).unwrap())
        .unwrap()
        .contains("installation_id"));
}

#[test]
fn repository_binding_preserves_provider_account_and_repository_identity() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.add_repository("octo/example").unwrap();
    let repository = &mut settings.repositories[0];
    repository.provider_account_id = Some("6954990".into());
    repository.provider_repository_id = Some("42".into());

    store.save_settings(&settings).unwrap();
    let binding = store.load_settings().unwrap().repositories[0]
        .account_binding()
        .unwrap();

    assert_eq!(binding.account.account_id, "6954990");
    assert_eq!(binding.repository.repository_id, "42");
}

#[test]
fn the_same_provider_repository_persists_as_distinct_account_bindings() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.add_repository("octo/example").unwrap();
    let first = &mut settings.repositories[0];
    first.provider_account_id = Some("101".into());
    first.provider_repository_id = Some("42".into());
    first.overrides.automatic_agent_start = Some(true);
    settings.repositories.push(Repository {
        id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderId::Github,
        name: "octo/example".into(),
        enabled: true,
        provider_account_id: Some("202".into()),
        legacy_installation_id: None,
        provider_repository_id: Some("42".into()),
        overrides: PolicyOverrides {
            automatic_agent_start: Some(false),
            ..PolicyOverrides::default()
        },
        review_preset: None,
        watched_authors: Vec::new(),
        assignments: Vec::new(),
    });

    store.save_settings(&settings).unwrap();
    let restored = store.load_settings().unwrap();

    assert_eq!(restored.repositories.len(), 2);
    assert_eq!(
        restored.repositories[0].provider_account_id.as_deref(),
        Some("101")
    );
    assert_eq!(
        restored.repositories[1].provider_account_id.as_deref(),
        Some("202")
    );
    assert_eq!(
        restored.repositories[0].overrides.automatic_agent_start,
        Some(true)
    );
    assert_eq!(
        restored.repositories[1].overrides.automatic_agent_start,
        Some(false)
    );
}

#[test]
fn azure_devops_contract_persists_without_a_live_provider_implementation() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = Settings::default();
    settings.repositories.push(Repository {
        id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderId::AzureDevops,
        name: "organization/project/repository".into(),
        enabled: true,
        provider_account_id: Some("entra-object-id".into()),
        legacy_installation_id: None,
        provider_repository_id: Some("repository-guid".into()),
        overrides: PolicyOverrides::default(),
        review_preset: None,
        watched_authors: Vec::new(),
        assignments: Vec::new(),
    });

    store.save_settings(&settings).unwrap();
    let restored = store.load_settings().unwrap();

    assert_eq!(restored.repositories[0].provider, ProviderId::AzureDevops);
    assert_eq!(
        restored.repositories[0].provider_account_id.as_deref(),
        Some("entra-object-id")
    );
}
