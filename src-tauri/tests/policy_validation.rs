mod support;

use pr_sniper_lib::policy::{Policy, PolicyOverrides, Schedule, Selector};
use pr_sniper_lib::storage::{DiagnosticEvent, Settings};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use support::Fixture;

fn reject_field(field: &str, value: Value) {
    for repository_override in [false, true] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let saved = store.add_repository("octo/hello-world").unwrap();
        let id = &saved.repositories[0].id;
        let path = fixture.path().join("config/settings.json");
        let before = fs::read(&path).unwrap();
        let result = if repository_override {
            let mut input = serde_json::Map::new();
            input.insert(field.to_string(), value.clone());
            serde_json::from_value::<PolicyOverrides>(Value::Object(input))
                .map_err(|_| "Unsupported policy override.".to_string())
                .and_then(|policy| store.save_repository_policy(id, policy))
        } else {
            let mut input = serde_json::to_value(Policy::default()).unwrap();
            input[field] = value.clone();
            serde_json::from_value::<Policy>(input)
                .map_err(|_| "Unsupported policy.".to_string())
                .and_then(|policy| store.save_defaults(policy))
        };
        assert!(
            result.is_err(),
            "invalid {field} accepted (repository override: {repository_override})"
        );
        assert!(!result.unwrap_err().is_empty());
        assert_eq!(
            fs::read(path).unwrap(),
            before,
            "rejected input changed config"
        );
    }
}

#[test]
fn invalid_intervals_are_rejected_before_global_or_override_persistence() {
    for minutes in [json!(0), json!(-1), json!(0.5)] {
        reject_field(
            "schedule",
            json!({"kind":"interval","minutes":minutes,"timezone":"UTC"}),
        );
    }
}

#[test]
fn invalid_time_zones_are_rejected_for_both_schedule_kinds() {
    for timezone in ["", "Mars/Olympus_Mons", "Not/A_Timezone"] {
        reject_field(
            "schedule",
            json!({"kind":"interval","minutes":15,"timezone":timezone}),
        );
        reject_field(
            "schedule",
            json!({"kind":"cron","expression":"0 9 * * MON-FRI","timezone":timezone}),
        );
    }
}

#[test]
fn cron_rejects_wrong_field_counts_out_of_range_values_and_empty_ranges() {
    for expression in [
        "* * * *",
        "0 * * * * *",
        "60 * * * *",
        "0 24 * * *",
        "0 0 0 * *",
        "0 0 * 13 *",
        "0 0 * * 8",
        "10-5 * * * *",
        "*/0 * * * *",
    ] {
        reject_field(
            "schedule",
            json!({"kind":"cron","expression":expression,"timezone":"UTC"}),
        );
    }
}

#[test]
fn standard_five_field_cron_and_positive_intervals_survive_restart() {
    for schedule in [
        json!({"kind":"cron","expression":"0 9 * * MON-FRI","timezone":"America/New_York"}),
        json!({"kind":"cron","expression":"*/5 9-17 * JAN-MAR MON-FRI","timezone":"Europe/London"}),
        json!({"kind":"cron","expression":"0 0 1,15 * *","timezone":"UTC"}),
        json!({"kind":"interval","minutes":1,"timezone":"UTC"}),
        json!({"kind":"interval","minutes":90,"timezone":"Asia/Tokyo"}),
    ] {
        let fixture = Fixture::new();
        let policy = Policy {
            schedule: serde_json::from_value(schedule).unwrap(),
            ..Policy::default()
        };
        fixture.store().save_defaults(policy.clone()).unwrap();
        assert_eq!(fixture.store().load_settings().unwrap().defaults, policy);
    }
}

#[test]
fn watched_accounts_require_positive_numeric_unique_ids_and_a_login_label() {
    for id in ["0", "-1", "12x", ""] {
        reject_field("watched_authors", json!([{"id":id,"login":"octo"}]));
    }
    reject_field("watched_authors", json!([{"id":"123","login":""}]));
    reject_field(
        "watched_authors",
        json!([
            {"id":"123","login":"octo"},
            {"id":"123","login":"renamed-octo"}
        ]),
    );
}

#[test]
fn model_and_named_agent_selectors_require_nonempty_values() {
    for kind in ["model", "agent"] {
        for value in ["", " \t\n"] {
            reject_field("selector", json!({"kind":kind,"value":value}));
        }
    }
}

#[test]
fn unsupported_adapters_and_ambiguous_selector_shapes_are_rejected() {
    reject_field("adapter", json!("unsupported-adapter"));
    reject_field(
        "selector",
        json!({"kind":"model","value":"model","agent":"other"}),
    );
    reject_field("selector", json!({"kind":"default","value":"hidden-model"}));
    reject_field(
        "selector",
        json!({"kind":"both","model":"model","agent":"agent"}),
    );
}

#[test]
fn empty_review_instructions_are_rejected() {
    for prompt in ["", " \n\t"] {
        reject_field("prompt", json!(prompt));
    }
}

#[test]
fn recognized_synthetic_github_token_never_reaches_config_logs_or_error_text() {
    let token = format!("ghp_{}", "A".repeat(36));
    for repository_override in [false, true] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let saved = store.add_repository("octo/hello-world").unwrap();
        store.record(DiagnosticEvent::SessionStarted).unwrap();
        let before = fs::read(fixture.path().join("config/settings.json")).unwrap();
        let prompt = format!("Review carefully. Credential: {token}");
        let result = if repository_override {
            store.save_repository_policy(
                &saved.repositories[0].id,
                PolicyOverrides {
                    prompt: Some(prompt),
                    ..PolicyOverrides::default()
                },
            )
        } else {
            store.save_defaults(Policy {
                prompt,
                ..Policy::default()
            })
        };
        assert!(result.is_err(), "credential-bearing prompt was accepted");
        assert!(!result.unwrap_err().contains(&token));
        assert_eq!(
            fs::read(fixture.path().join("config/settings.json")).unwrap(),
            before
        );
        for path in ["config/settings.json", "state/diagnostics.jsonl"] {
            assert!(!fs::read_to_string(fixture.path().join(path))
                .unwrap()
                .contains(&token));
        }
    }
}

#[test]
fn sparse_overrides_preserve_false_empty_and_default_selector_as_explicit_values() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let saved = store.add_repository("octo/hello-world").unwrap();
    let id = &saved.repositories[0].id;
    let defaults: Policy = serde_json::from_value(json!({
        "schedule":{"kind":"interval","minutes":90,"timezone":"UTC"},
        "watched_authors":[{"id":"123","login":"octo"}],
        "reviewer_assignment":true,
        "adapter":"copilot",
        "selector":{"kind":"model","value":"configured-model"},
        "prompt":"Review carefully.",
        "automatic_agent_start":true,
        "automatic_comment_publication":false
    }))
    .unwrap();
    store.save_defaults(defaults.clone()).unwrap();
    store
        .save_repository_policy(
            id,
            PolicyOverrides {
                watched_authors: Some(vec![]),
                reviewer_assignment: Some(false),
                selector: Some(Selector::Default),
                automatic_agent_start: Some(false),
                automatic_comment_publication: Some(true),
                ..PolicyOverrides::default()
            },
        )
        .unwrap();
    let expected = Policy {
        watched_authors: vec![],
        reviewer_assignment: false,
        selector: Selector::Default,
        automatic_agent_start: false,
        automatic_comment_publication: true,
        ..defaults.clone()
    };
    assert_eq!(
        fixture
            .store()
            .load_settings()
            .unwrap()
            .effective_policy(id),
        Some(expected)
    );
    store
        .save_repository_policy(
            id,
            serde_json::from_value(json!({
                "watched_authors":null,"reviewer_assignment":null,"selector":null,
                "automatic_agent_start":null,"automatic_comment_publication":null
            }))
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        fixture
            .store()
            .load_settings()
            .unwrap()
            .effective_policy(id),
        Some(defaults)
    );
}

#[test]
fn forty_repositories_keep_current_policy_and_identity_across_change_disable_and_remove() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store
        .save_settings(&Settings {
            launch_at_login: true,
            ..Settings::default()
        })
        .unwrap();
    for index in 0..40 {
        store
            .add_repository(&format!("octo/repository-{index}"))
            .unwrap();
    }
    let saved = fixture.store().load_settings().unwrap();
    assert_eq!(saved.repositories.len(), 40);
    assert_eq!(
        saved
            .repositories
            .iter()
            .map(|repo| &repo.id)
            .collect::<HashSet<_>>()
            .len(),
        40
    );
    let first = &saved.repositories[0];
    let neighbor = &saved.repositories[1];
    let defaults = Policy {
        schedule: Schedule::Interval {
            minutes: 27,
            timezone: "UTC".into(),
        },
        automatic_agent_start: true,
        ..Policy::default()
    };
    store.save_defaults(defaults.clone()).unwrap();
    store
        .save_repository_policy(
            &first.id,
            PolicyOverrides {
                prompt: Some("Current repository policy.".into()),
                ..PolicyOverrides::default()
            },
        )
        .unwrap();
    store
        .update_repository(&first.id, &first.name, false)
        .unwrap();
    let disabled = fixture.store().load_settings().unwrap();
    assert!(!disabled.repositories[0].enabled);
    assert_eq!(disabled.repositories[0].id, first.id);
    assert_eq!(
        disabled.effective_policy(&first.id).unwrap().prompt,
        "Current repository policy."
    );
    assert_eq!(disabled.effective_policy(&neighbor.id), Some(defaults));
    assert_eq!(&disabled.repositories[1], neighbor);
    store.remove_repository(&first.id).unwrap();
    let reopened = fixture.store().load_settings().unwrap();
    assert_eq!(reopened.repositories.len(), 39);
    assert!(reopened.effective_policy(&first.id).is_none());
    assert_eq!(&reopened.repositories[0], neighbor);
    assert!(reopened.launch_at_login);
}
