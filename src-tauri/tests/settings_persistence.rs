use pr_sniper_lib::{
    policy::PolicyOverrides,
    storage::{ProviderId, Repository, Settings, Store, Theme},
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
            "doctrine_catalog_version": 1,
            "voices": [],
            "voice_catalog_version": 1,
            "capacity": 4,
            "review_inactivity_minutes": 15,
            "admission_migration_version": 1,
            "time_zone": "UTC",
            "defaults": {
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
        review_drafts: false,
        id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderId::Github,
        name: "octo/example".into(),
        enabled: true,
        cron: "*/30 * * * *".into(),
        provider_account_id: Some("202".into()),
        legacy_installation_id: None,
        provider_repository_id: Some("42".into()),
        overrides: PolicyOverrides {
            automatic_agent_start: Some(false),
            ..PolicyOverrides::default()
        },
        review_preset: None,
        watched_authors: Vec::new(),
        admission: None,
        assignments: Vec::new(),
        primary_assignment_id: None,
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
        review_drafts: false,
        id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderId::AzureDevops,
        name: "organization/project/repository".into(),
        enabled: true,
        cron: "*/30 * * * *".into(),
        provider_account_id: Some("entra-object-id".into()),
        legacy_installation_id: None,
        provider_repository_id: Some("repository-guid".into()),
        overrides: PolicyOverrides::default(),
        review_preset: None,
        watched_authors: Vec::new(),
        admission: None,
        assignments: Vec::new(),
        primary_assignment_id: None,
    });

    store.save_settings(&settings).unwrap();
    let restored = store.load_settings().unwrap();

    assert_eq!(restored.repositories[0].provider, ProviderId::AzureDevops);
    assert_eq!(
        restored.repositories[0].provider_account_id.as_deref(),
        Some("entra-object-id")
    );
}

const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PRIMARY: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const SECOND: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn packet_settings(
    _fixture: &Fixture,
    primary: serde_json::Value,
    second: serde_json::Value,
) -> Settings {
    let helper = Fixture::new();
    let mut settings = helper.store().add_repository("octo/example").unwrap();
    settings.repositories[0].provider_account_id = Some("101".into());
    settings.repositories[0].provider_repository_id = Some("42".into());
    let mut value = serde_json::to_value(&settings).unwrap();
    value["agents"] = serde_json::json!([{"id": AGENT, "name":"A", "model":"m", "ai_account":{"provider":"copilot","account_id":"101"}, "prompt":"p", "signature":"s"}]);
    let schedule = serde_json::json!({"kind":"interval","minutes":5,"timezone":"UTC"});
    let mut a = serde_json::json!({"id": PRIMARY, "agent_id": AGENT, "schedule": schedule, "comment": true});
    let mut b = serde_json::json!({"id": SECOND, "agent_id": AGENT, "schedule": schedule, "comment": false});
    for (target, extra) in [(&mut a, primary), (&mut b, second)] {
        for (k, v) in extra.as_object().unwrap() {
            target[k] = v.clone();
        }
    }
    value["repositories"][0]["assignments"] = serde_json::json!([a, b]);
    value["repositories"][0]["primary_assignment_id"] = PRIMARY.into();
    settings = serde_json::from_value(value).unwrap();
    settings
}

fn issues(settings: &Settings) -> Vec<String> {
    settings.readiness().repositories[0].issues.clone()
}

#[test]
fn packet_fields_round_trip_and_old_files_load_unchanged() {
    let fixture = Fixture::new();
    let mut settings = packet_settings(
        &fixture,
        serde_json::json!({"actions": {"approve": true, "merge": true, "include_self": true}, "tools": ["read_changes", "leave_comments", "panic_exit"]}),
        serde_json::json!({"tools": ["submit_report"]}),
    );
    settings.repositories[0].review_drafts = true;
    fixture.store().save_settings(&settings).unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), settings);

    let mut old = packet_settings(&fixture, serde_json::json!({}), serde_json::json!({}));
    old.repositories[0].review_drafts = false;
    let value = serde_json::to_value(&old).unwrap();
    let repository = &value["repositories"][0];
    assert!(repository.get("review_drafts").is_none());
    assert!(repository["assignments"][0].get("tools").is_none());
    assert!(repository["assignments"][0]["actions"]
        .get("include_self")
        .is_none());
    let text = value.to_string();
    let reloaded: Settings = serde_json::from_str(&text).unwrap();
    assert!(reloaded.repositories[0].assignments[0].tools.is_none());
    assert!(!reloaded.repositories[0].review_drafts);
}

#[test]
fn packet_tool_readiness_follows_primary_and_grants() {
    let fixture = Fixture::new();
    let ok = packet_settings(
        &fixture,
        serde_json::json!({"actions": {"approve": true, "merge": true}, "tools": ["read_source", "leave_comments", "ready_for_human", "approve_pull_request", "approve_and_merge", "panic_exit"]}),
        serde_json::json!({"tools": ["get_checks", "submit_report", "panic_exit"]}),
    );
    assert!(issues(&ok).is_empty(), "{:?}", issues(&ok));

    let bad = packet_settings(
        &fixture,
        serde_json::json!({"tools": ["leave_comments", "approve_pull_request", "approve_and_merge", "submit_report"], "comment": false}),
        serde_json::json!({"tools": ["leave_comments", "approve_pull_request", "ready_for_human", "panic_exit"]}),
    );
    let found = issues(&bad).join("\n");
    assert!(found.contains("tool leave_comments needs Publish Comment"));
    assert!(found.contains("tool approve_pull_request needs Approve"));
    assert!(found.contains("tool approve_and_merge needs Approve & Merge"));
    assert!(found.contains("tool submit_report is non-primary only"));
    assert_eq!(found.matches("is primary-only").count(), 3);

    let approve_only = packet_settings(
        &fixture,
        serde_json::json!({"actions": {"approve": true, "merge": false}, "tools": ["approve_pull_request", "approve_and_merge"]}),
        serde_json::json!({}),
    );
    assert_eq!(issues(&approve_only).len(), 1);
}

#[test]
fn unknown_or_duplicate_packet_tools_are_invalid() {
    let fixture = Fixture::new();
    let unknown = packet_settings(
        &fixture,
        serde_json::json!({"tools": ["nope"]}),
        serde_json::json!({}),
    );
    assert!(unknown.validate().is_err());
    assert!(fixture.store().save_settings(&unknown).is_err());
    let dup = packet_settings(
        &fixture,
        serde_json::json!({"tools": ["read_changes", "read_changes"]}),
        serde_json::json!({}),
    );
    assert!(issues(&dup).iter().any(|i| i.contains("is duplicated")));
}

#[test]
fn typescript_tool_registry_mirrors_the_rust_registry() {
    let source =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/policy.ts")).unwrap();
    let block = source
        .split("export const PACKET_TOOLS = [")
        .nth(1)
        .and_then(|rest| rest.split("] as const").next())
        .unwrap();
    let ts: Vec<&str> = block
        .split(',')
        .map(|item| item.trim().trim_matches('"'))
        .filter(|item| !item.is_empty())
        .collect();
    assert_eq!(ts, pr_sniper_lib::storage::PACKET_TOOLS);
}

#[test]
fn only_a_brand_new_profile_takes_the_initial_time_zone() {
    let zone = |store: &Store| store.load_settings().unwrap().time_zone;
    let fresh = tempfile::tempdir().unwrap();
    let created = Store::new(fresh.path().into()).with_initial_time_zone("Asia/Tokyo".into());
    assert_eq!(zone(&created), "Asia/Tokyo");
    let reopened = Store::new(fresh.path().into()).with_initial_time_zone("Europe/Paris".into());
    assert_eq!(zone(&reopened), "Asia/Tokyo");

    let existing = tempfile::tempdir().unwrap();
    assert_eq!(zone(&Store::new(existing.path().into())), "UTC");
    let later = Store::new(existing.path().into()).with_initial_time_zone("Asia/Tokyo".into());
    assert_eq!(zone(&later), "UTC");
}

#[test]
fn theme_round_trips_and_light_keeps_the_previous_file_format() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = fixture.path().join("config/settings.json");
    store
        .save_settings(&Settings {
            launch_at_login: true,
            ..Settings::default()
        })
        .unwrap();

    for (theme, persisted) in [
        (Theme::Dark, Some("dark")),
        (Theme::Os, Some("os")),
        (Theme::Light, None),
    ] {
        store.save_theme(theme).unwrap();
        let bytes = fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.get("theme").and_then(|v| v.as_str()), persisted);
        let reopened = Store::new(fixture.path().to_path_buf())
            .load_settings()
            .unwrap();
        assert_eq!(reopened.theme, theme);
        assert!(reopened.launch_at_login, "theme saves keep other settings");
    }
}

#[test]
fn settings_saved_before_themes_load_as_light() {
    let fixture = Fixture::new();
    let config = fixture.path().join("config");
    fs::create_dir(&config).unwrap();
    fs::write(
        config.join("settings.json"),
        br#"{"launch_at_login":false,"doctrines":[],"doctrine_catalog_version":1,"capacity":4,"admission_migration_version":1}"#,
    )
    .unwrap();

    let settings = fixture.store().load_settings().unwrap();

    assert_eq!(settings.theme, Theme::Light);
}

#[test]
fn invalid_theme_values_are_rejected() {
    let fixture = Fixture::new();
    let config = fixture.path().join("config");
    fs::create_dir(&config).unwrap();
    for value in [r#""Dark""#, r#""system""#, r#""""#, "1", "null", "{}"] {
        let path = config.join("settings.json");
        let original = format!(r#"{{"launch_at_login":false,"doctrines":[],"theme":{value}}}"#);
        fs::write(&path, &original).unwrap();

        assert!(
            fixture.store().load_settings().is_err(),
            "theme {value} must be rejected"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(serde_json::from_str::<Theme>(value).is_err());
    }
}

fn legacy_settings_file(fixture: &Fixture, defaults: &str) -> std::path::PathBuf {
    let path = fixture.path().join("config/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            r#"{{
          "launch_at_login": false, "admission_migration_version": 1,{defaults}
          "repositories": [
            {{"id": "c53ae3be-2ad5-44aa-b257-9298186e1c6f", "provider": "github",
              "name": "octo/one", "enabled": true}},
            {{"id": "d53ae3be-2ad5-44aa-b257-9298186e1c6f", "provider": "github",
              "name": "octo/two", "enabled": true}}
          ]
        }}"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn the_global_cron_moves_onto_every_repository_once() {
    let fixture = Fixture::new();
    let path = legacy_settings_file(
        &fixture,
        r#""defaults": {
            "schedule": {"kind":"cron","expression":"0 9 * * MON-FRI","timezone":"Europe/Paris"},
            "watched_authors": [], "reviewer_assignment": true, "adapter": "copilot",
            "selector": {"kind":"default"}, "prompt": "Review.",
            "automatic_agent_start": false, "automatic_comment_publication": false
          },"#,
    );
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(settings.time_zone, "Europe/Paris");
    assert_eq!(settings.defaults.schedule, None);
    assert!(settings
        .repositories
        .iter()
        .all(|repository| repository.cron == "0 9 * * MON-FRI" && repository.enabled));
    let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(written["defaults"].get("schedule").is_none());
    assert_eq!(written["time_zone"], "Europe/Paris");
    assert_eq!(written["repositories"][1]["cron"], "0 9 * * MON-FRI");

    let bytes = fs::read(&path).unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), settings);
    assert_eq!(fs::read(&path).unwrap(), bytes, "migration runs once");
}

#[test]
fn files_without_a_saved_schedule_keep_the_former_default_cadence() {
    let fixture = Fixture::new();
    legacy_settings_file(&fixture, "");
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(settings.time_zone, "UTC");
    assert!(settings
        .repositories
        .iter()
        .all(|repository| repository.cron == "*/15 * * * *"));
}

#[test]
fn new_repositories_default_to_every_thirty_minutes() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let added = store.add_repository("octo/example").unwrap();
    assert_eq!(added.repositories[0].cron, "*/30 * * * *");
    let mut value = serde_json::to_value(&added.repositories[0]).unwrap();
    value.as_object_mut().unwrap().remove("cron");
    value["id"] = "e53ae3be-2ad5-44aa-b257-9298186e1c6f".into();
    value["name"] = "octo/other".into();
    value["enabled"] = false.into();
    let created = store
        .save_resource(pr_sniper_lib::storage::ResourceEdit::Repository {
            id: "e53ae3be-2ad5-44aa-b257-9298186e1c6f".into(),
            expected: None,
            value: Some(Box::new(serde_json::from_value(value).unwrap())),
        })
        .unwrap();
    assert_eq!(created.repositories[1].cron, "*/30 * * * *");
}

#[test]
fn repository_saves_reject_crons_that_cannot_run() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let saved = store.add_repository("octo/example").unwrap();
    let path = fixture.path().join("config/settings.json");
    let before = fs::read(&path).unwrap();
    for cron in [
        "",
        "* * * *",
        "0 0 * * * *",
        "61 * * * *",
        "0 0 30 2 *",
        "@hourly",
    ] {
        let mut value = saved.repositories[0].clone();
        value.cron = cron.into();
        assert!(
            store
                .save_resource(pr_sniper_lib::storage::ResourceEdit::Repository {
                    id: value.id.clone(),
                    expected: Some(Box::new(saved.repositories[0].clone())),
                    value: Some(Box::new(value)),
                })
                .is_err(),
            "{cron:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

fn legacy_defaults(schedule: &str) -> String {
    format!(
        r#""defaults": {{
            "schedule": {schedule},
            "watched_authors": [], "reviewer_assignment": true, "adapter": "copilot",
            "selector": {{"kind":"default"}}, "prompt": "Review.",
            "automatic_agent_start": false, "automatic_comment_publication": false
          }},"#
    )
}

#[test]
fn legacy_intervals_and_crons_that_never_run_pause_every_repository_once() {
    for (schedule, zone) in [
        (
            r#"{"kind":"interval","minutes":7,"timezone":"Europe/London"}"#,
            "Europe/London",
        ),
        (
            r#"{"kind":"cron","expression":"0 0 30 2 *","timezone":"Asia/Tokyo"}"#,
            "Asia/Tokyo",
        ),
    ] {
        let fixture = Fixture::new();
        let path = legacy_settings_file(&fixture, &legacy_defaults(schedule));
        let settings = fixture.store().load_settings().unwrap();
        assert_eq!(settings.time_zone, zone, "{schedule}");
        assert_eq!(settings.defaults.schedule, None);
        assert!(settings
            .repositories
            .iter()
            .all(|repository| !repository.enabled && repository.cron == "*/30 * * * *"));
        let bytes = fs::read(&path).unwrap();
        assert_eq!(fixture.store().load_settings().unwrap(), settings);
        assert_eq!(fs::read(&path).unwrap(), bytes, "migration runs once");
        // Saves keep working; re-enabling is an explicit user choice.
        let mut enabled = settings.clone();
        enabled.repositories[0].enabled = true;
        fixture.store().save_settings(&enabled).unwrap();
        assert!(fixture.store().load_settings().unwrap().repositories[0].enabled);
    }
}

#[test]
fn azure_devops_approve_and_merge_is_refused_and_narrowed_to_approve_on_load() {
    let fixture = Fixture::new();
    let assignment = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    let document = |actions: serde_json::Value, tools: serde_json::Value| {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["agents"] = serde_json::json!([
            {"id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "name":"Reviewer", "model":"m",
             "ai_account":{"provider":"copilot","account_id":"101"},
             "prompt":"Review carefully.", "signature":"Fixture"}
        ]);
        value["repositories"] = serde_json::json!([{
            "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd", "provider": "azure_devops",
            "name": "contoso/Web/app", "enabled": false,
            "provider_account_id": "00000000-0000-0000-0000-000000000001",
            "provider_repository_id": "contoso/00000000-0000-0000-0000-0000000000b1/00000000-0000-0000-0000-0000000000c1",
            "primary_assignment_id": assignment,
            "assignments": [{"id": assignment, "agent_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "comment": false, "actions": actions, "tools": tools}]
        }]);
        value
    };
    let merge =
        serde_json::json!({"reply": false, "approve": true, "merge": true, "include_self": true});
    let tools = serde_json::json!(["read_changes", "approve_pull_request", "approve_and_merge"]);
    for (actions, tools) in [
        (merge.clone(), serde_json::Value::Null),
        (
            serde_json::json!({"reply": false, "approve": true, "merge": false}),
            tools.clone(),
        ),
    ] {
        let settings: Settings = serde_json::from_value(document(actions, tools)).unwrap();
        assert_eq!(
            fixture.store().save_settings(&settings).unwrap_err(),
            pr_sniper_lib::storage::APPROVE_AND_MERGE_UNAVAILABLE
        );
    }

    let config = fixture.path().join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("settings.json"),
        serde_json::to_vec(&document(merge, tools)).unwrap(),
    )
    .unwrap();
    let loaded = fixture.store().load_settings().unwrap();
    let assignment = &loaded.repositories[0].assignments[0];
    let actions = assignment.actions.as_ref().unwrap();
    assert!(actions.approve && !actions.merge && !actions.include_self);
    assert_eq!(
        assignment.tools.as_deref().unwrap(),
        ["read_changes", "approve_pull_request"]
    );
    let authority = loaded.repositories[0].assignment_authority(assignment);
    assert!(authority.approve && !authority.merge);
}
