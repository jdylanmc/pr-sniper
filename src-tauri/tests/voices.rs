use pr_sniper_lib::{
    monitoring::QueueJob,
    review::Selection,
    storage::{ResourceEdit, Settings, Voice},
};
use serde_json::json;
use std::fs;

mod support;
use support::Fixture;

const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const ASSIGNMENT: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn seed_titles() -> Vec<String> {
    ["Caveman", "Gilfoyle", "Merlin"].map(String::from).to_vec()
}

fn titles(settings: &Settings) -> Vec<String> {
    settings.voices.iter().map(|v| v.title.clone()).collect()
}

fn write(fixture: &Fixture, value: serde_json::Value) -> std::path::PathBuf {
    let path = fixture.path().join("config/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    path
}

/// Settings written before Voices existed: no `voices`, no version, an Agent.
fn legacy() -> serde_json::Value {
    json!({
        "launch_at_login": false,
        "doctrine_catalog_version": 1,
        "admission_migration_version": 1,
        "doctrines": [{"title":"Correctness","body":"Trace state transitions."}],
        "agents": [{
            "id":AGENT,"name":"Reviewer","model":"explicit-model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "doctrines":["Correctness"],"prompt":"Find defects.","signature":"machine"
        }],
        "repositories": [{
            "id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{
                "id":ASSIGNMENT,"agent_id":AGENT,
                "schedule":{"kind":"interval","minutes":7,"timezone":"UTC"},
                "comment":true,"approve":false
            }]
        }]
    })
}

fn job() -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":REPO,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"all_authors":false,"requested_reviewer":false,
        "waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

fn delete(title: &str, settings: &Settings) -> ResourceEdit {
    ResourceEdit::Voice {
        title: title.into(),
        expected: settings.voices.iter().find(|v| v.title == title).cloned(),
        value: None,
    }
}

#[test]
fn a_fresh_store_seeds_the_three_voices_with_guardrails_on() {
    let fixture = Fixture::new();
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(titles(&settings), seed_titles());
    assert!(settings.voices.iter().all(|v| v.guardrails));
    assert!(settings
        .voices
        .iter()
        .all(|v| !v.body.trim().is_empty() && !v.body.starts_with('#') && !v.body.contains("---")));
    assert_eq!(settings.voice_catalog_version, 1);
}

#[test]
fn legacy_settings_load_with_seeded_voices_and_agents_without_a_voice() {
    let fixture = Fixture::new();
    let path = write(&fixture, legacy());
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(titles(&settings), seed_titles());
    assert_eq!(settings.agents[0].voice, None);
    assert_eq!(settings.agents[0].doctrine_titles(), ["Correctness"]);
    let persisted: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted["voice_catalog_version"], 1);
    assert!(persisted["agents"][0].get("voice").is_none());
    // Seeded once: a second load writes nothing.
    let bytes = fs::read(&path).unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), settings);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn deleted_seeds_and_an_empty_library_are_never_topped_up() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.load_settings().unwrap();
    settings = store.save_resource(delete("Gilfoyle", &settings)).unwrap();
    assert_eq!(
        titles(&fixture.store().load_settings().unwrap()),
        ["Caveman", "Merlin"]
    );
    for title in ["Caveman", "Merlin"] {
        settings = store.save_resource(delete(title, &settings)).unwrap();
    }
    let reopened = fixture.store().load_settings().unwrap();
    assert!(reopened.voices.is_empty());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path().join("config/settings.json")).unwrap())
            .unwrap();
    assert_eq!(value["voices"], json!([]));
}

#[test]
fn custom_voice_round_trips_with_its_guard_flag_and_legacy_voice_defaults_on() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = store.load_settings().unwrap();
    let custom = Voice {
        title: "Pirate".into(),
        body: "Talk like a pirate.".into(),
        guardrails: false,
    };
    store
        .save_resource(ResourceEdit::Voice {
            title: custom.title.clone(),
            expected: None,
            value: Some(custom.clone()),
        })
        .unwrap();
    let reopened = fixture.store().load_settings().unwrap();
    assert_eq!(reopened.voices.len(), settings.voices.len() + 1);
    assert_eq!(reopened.voices.last(), Some(&custom));
    let parsed: Voice = serde_json::from_value(json!({"title":"Old","body":"Text."})).unwrap();
    assert!(parsed.guardrails);
}

#[test]
fn referenced_voice_cannot_be_deleted_and_rename_updates_the_agent() {
    let fixture = Fixture::new();
    write(&fixture, legacy());
    let store = fixture.store();
    let settings = store.load_settings().unwrap();
    let mut agent = settings.agents[0].clone();
    agent.voice = Some("merlin".into());
    let settings = store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(settings.agents[0].clone()),
            value: Some(agent),
        })
        .unwrap();
    assert!(store
        .save_resource(delete("Merlin", &settings))
        .unwrap_err()
        .contains("used by an Agent"));
    let merlin = settings.voices[2].clone();
    let renamed = Voice {
        title: "Wizard".into(),
        ..merlin.clone()
    };
    let settings = store
        .save_resource(ResourceEdit::Voice {
            title: merlin.title.clone(),
            expected: Some(merlin),
            value: Some(renamed),
        })
        .unwrap();
    assert_eq!(settings.agents[0].voice.as_deref(), Some("Wizard"));
    assert_eq!(settings.agents[0].doctrine_titles(), ["Correctness"]);
    let reopened = fixture.store().load_settings().unwrap();
    assert_eq!(reopened, settings);
    assert_eq!(titles(&reopened), ["Caveman", "Gilfoyle", "Wizard"]);
}

#[test]
fn agents_need_an_existing_voice_and_voices_need_unique_titles() {
    let mut settings: Settings = serde_json::from_value(legacy()).unwrap();
    settings.voices = pr_sniper_lib::storage::Settings::default().voices;
    settings.agents[0].voice = Some("Missing".into());
    assert!(settings.validate().unwrap_err().contains("Voice"));
    settings.voices = vec![
        Voice {
            title: "Missing".into(),
            body: "a".into(),
            guardrails: true,
        },
        Voice {
            title: " missing ".into(),
            body: "b".into(),
            guardrails: true,
        },
    ];
    assert_eq!(
        settings.validate().unwrap_err(),
        "Voices need a unique, nonempty title."
    );
    settings.voices.pop();
    settings.validate().unwrap();
}

#[test]
fn the_job_snapshot_keeps_the_voice_it_started_with() {
    let fixture = Fixture::new();
    write(&fixture, legacy());
    let store = fixture.store();
    let mut settings = store.load_settings().unwrap();
    settings.agents[0].voice = Some("Caveman".into());
    let settings = store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(store.load_settings().unwrap().agents[0].clone()),
            value: Some(settings.agents[0].clone()),
        })
        .unwrap();
    let selection = Selection::resolve(&settings, &job(), ASSIGNMENT).unwrap();
    let captured = selection
        .configuration
        .as_ref()
        .unwrap()
        .voice
        .clone()
        .unwrap();
    assert_eq!(captured, settings.voices[0]);
    let mut edited = captured.clone();
    edited.body = "Changed after the job started.".into();
    edited.guardrails = false;
    let later = store
        .save_resource(ResourceEdit::Voice {
            title: captured.title.clone(),
            expected: Some(captured.clone()),
            value: Some(edited.clone()),
        })
        .unwrap();
    let restored: Selection =
        serde_json::from_value(serde_json::to_value(&selection).unwrap()).unwrap();
    assert_eq!(restored.configuration.unwrap().voice, Some(captured));
    let current = Selection::resolve(&later, &job(), ASSIGNMENT).unwrap();
    assert_eq!(current.configuration.as_ref().unwrap().voice, Some(edited));
    assert!(!current.same_execution(&selection));
    // Jobs that predate Voices load as none, never rebuilt from the library.
    let mut legacy = serde_json::to_value(&selection).unwrap();
    legacy["configuration"]
        .as_object_mut()
        .unwrap()
        .remove("voice");
    legacy["agent"].as_object_mut().unwrap().remove("voice");
    let legacy: Selection = serde_json::from_value(legacy).unwrap();
    assert!(legacy.configuration.unwrap().voice.is_none());
    assert!(legacy.agent.voice.is_none());
}
