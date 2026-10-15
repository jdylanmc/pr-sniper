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

fn voice<'a>(settings: &'a Settings, title: &str) -> &'a Voice {
    settings.voices.iter().find(|v| v.title == title).unwrap()
}

fn delete(title: &str, settings: &Settings) -> ResourceEdit {
    let voice = voice(settings, title);
    ResourceEdit::Voice {
        id: voice.id.clone(),
        expected: Some(voice.clone()),
        value: None,
    }
}

fn with_voice(store: &pr_sniper_lib::storage::Store, title: &str) -> Settings {
    let settings = store.load_settings().unwrap();
    let mut agent = settings.agents[0].clone();
    agent.voice_id = Some(voice(&settings, title).id.clone());
    store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(settings.agents[0].clone()),
            value: Some(agent),
        })
        .unwrap()
}

fn doctrine_titles(settings: &Settings) -> Vec<String> {
    settings.agents[0]
        .doctrine_ids
        .iter()
        .map(|id| {
            settings
                .doctrines
                .iter()
                .find(|d| &d.id == id)
                .map_or_else(|| format!("{id} (unavailable)"), |d| d.title.clone())
        })
        .collect()
}

fn is_uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

#[test]
fn a_fresh_store_seeds_the_three_voices_with_ids_and_guardrails_on() {
    let fixture = Fixture::new();
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(titles(&settings), seed_titles());
    assert!(settings
        .voices
        .iter()
        .all(|v| v.guardrails && is_uuid(&v.id)));
    assert!(settings.doctrines.iter().all(|d| is_uuid(&d.id)));
    assert!(settings
        .voices
        .iter()
        .all(|v| !v.body.trim().is_empty() && !v.body.starts_with('#') && !v.body.contains("---")));
    assert_eq!(settings.voice_catalog_version, 1);
}

#[test]
fn legacy_settings_load_with_seeded_voices_ids_and_agents_without_a_voice() {
    let fixture = Fixture::new();
    let path = write(&fixture, legacy());
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(titles(&settings), seed_titles());
    assert_eq!(settings.agents[0].voice_id, None);
    assert_eq!(doctrine_titles(&settings), ["Correctness"]);
    let persisted: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted["voice_catalog_version"], 1);
    let agent = persisted["agents"][0].as_object().unwrap();
    assert!(agent.get("voice_id").is_none() && agent.get("doctrines").is_none());
    assert_eq!(agent["doctrine_ids"], json!([settings.doctrines[0].id]));
    // Migrated once: a second load writes nothing and keeps the ids.
    let bytes = fs::read(&path).unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), settings);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn title_references_migrate_to_ids_in_order_and_unmatched_stay_unavailable() {
    let fixture = Fixture::new();
    let mut value = legacy();
    value["doctrines"] = json!([
        {"title":"Correctness","body":"Trace state transitions."},
        {"title":"Boundaries","body":"Keep authority explicit."},
        {"title":"Tests","body":"Prove behavior."}
    ]);
    value["agents"][0]["doctrines"] = json!(["tests", " CORRECTNESS ", "Gone"]);
    value["agents"][0]["doctrine"] = json!("Boundaries");
    let path = write(&fixture, value);
    let settings = fixture.store().load_settings().unwrap();
    let agent = &settings.agents[0];
    assert!(agent.doctrine.is_none() && agent.doctrines.is_none());
    // The list supersedes the inactive single title, as before ids.
    assert_eq!(
        doctrine_titles(&settings),
        ["Tests", "Correctness", "Gone (unavailable)"]
    );
    assert!(agent.doctrine_ids[..2].iter().all(|id| is_uuid(id)));
    assert_eq!(agent.unavailable_references(&settings), ["Gone"]);
    let bytes = fs::read(&path).unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), settings);
    assert_eq!(fs::read(&path).unwrap(), bytes);

    // Keeping the unavailable reference saves; adding a new one does not.
    let store = fixture.store();
    let mut kept = agent.clone();
    kept.prompt = "Edited prompt.".into();
    let saved = store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(agent.clone()),
            value: Some(kept),
        })
        .unwrap();
    let mut added = saved.agents[0].clone();
    added.doctrine_ids.push("Another".into());
    assert!(store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(saved.agents[0].clone()),
            value: Some(added),
        })
        .unwrap_err()
        .contains("must exist"));
    let mut voiced = saved.agents[0].clone();
    voiced.voice_id = Some("0e0e0e0e-0e0e-4e0e-8e0e-0e0e0e0e0e0e".into());
    assert!(store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(saved.agents[0].clone()),
            value: Some(voiced),
        })
        .is_err());
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
        id: "7c9e6679-7425-40de-944b-e07fc1f90ae7".into(),
        title: "Pirate".into(),
        body: "Talk like a pirate.".into(),
        guardrails: false,
    };
    store
        .save_resource(ResourceEdit::Voice {
            id: custom.id.clone(),
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
fn referenced_voice_cannot_be_deleted_and_rename_leaves_the_agent_untouched() {
    let fixture = Fixture::new();
    write(&fixture, legacy());
    let store = fixture.store();
    let settings = with_voice(&store, "Merlin");
    let agent_json = serde_json::to_value(&settings.agents[0]).unwrap();
    assert!(store
        .save_resource(delete("Merlin", &settings))
        .unwrap_err()
        .contains("used by an Agent"));
    let merlin = voice(&settings, "Merlin").clone();
    for title in ["Wizard", "wizard"] {
        let current = store.load_settings().unwrap();
        let before = current.voices[2].clone();
        let settings = store
            .save_resource(ResourceEdit::Voice {
                id: merlin.id.clone(),
                expected: Some(before.clone()),
                value: Some(Voice {
                    title: title.into(),
                    ..before
                }),
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(&settings.agents[0]).unwrap(),
            agent_json
        );
        assert_eq!(voice(&settings, title).id, merlin.id);
    }
    let reopened = fixture.store().load_settings().unwrap();
    assert_eq!(titles(&reopened), ["Caveman", "Gilfoyle", "wizard"]);
    assert_eq!(
        reopened.agents[0].voice_id.as_deref(),
        Some(merlin.id.as_str())
    );
}

#[test]
fn voices_need_unique_ids_and_titles_and_agents_keep_dangling_ids_visible() {
    let fixture = Fixture::new();
    write(&fixture, legacy());
    let mut settings = fixture.store().load_settings().unwrap();
    settings.agents[0].voice_id = Some("Missing".into());
    settings.validate().unwrap();
    assert_eq!(
        settings.agents[0].unavailable_references(&settings),
        ["Missing"]
    );
    let first = settings.voices[0].clone();
    settings.voices.push(Voice {
        title: " caveman ".into(),
        id: "9a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into(),
        ..first.clone()
    });
    assert_eq!(
        settings.validate().unwrap_err(),
        "Voices need a unique, nonempty title."
    );
    settings.voices.pop();
    settings.voices.push(Voice {
        title: "Copy".into(),
        ..first
    });
    assert_eq!(
        settings.validate().unwrap_err(),
        "Voices need a unique stable identity."
    );
}

#[test]
fn the_job_snapshot_keeps_the_voice_it_started_with() {
    let fixture = Fixture::new();
    write(&fixture, legacy());
    let store = fixture.store();
    let settings = with_voice(&store, "Caveman");
    let selection = Selection::resolve(&settings, &job(), ASSIGNMENT).unwrap();
    let captured = selection
        .configuration
        .as_ref()
        .unwrap()
        .voice
        .clone()
        .unwrap();
    assert_eq!(captured, settings.voices[0]);
    let captured_doctrine = &selection.configuration.as_ref().unwrap().doctrines[0];
    assert_eq!(captured_doctrine.id, settings.doctrines[0].id);
    let mut edited = captured.clone();
    edited.title = "Grunt".into();
    edited.body = "Changed after the job started.".into();
    edited.guardrails = false;
    let later = store
        .save_resource(ResourceEdit::Voice {
            id: captured.id.clone(),
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
    // Jobs that predate ids or Voices load as recorded, never rebuilt.
    let mut legacy = serde_json::to_value(&selection).unwrap();
    let configuration = legacy["configuration"].as_object_mut().unwrap();
    configuration.remove("voice");
    configuration["doctrines"][0]
        .as_object_mut()
        .unwrap()
        .remove("id");
    let agent = legacy["agent"].as_object_mut().unwrap();
    agent.remove("voice_id");
    agent.remove("doctrine_ids");
    agent.insert("doctrine".into(), json!("Correctness"));
    let recorded = legacy.clone();
    let legacy: Selection = serde_json::from_value(legacy).unwrap();
    assert!(legacy.configuration.as_ref().unwrap().voice.is_none());
    assert!(legacy.configuration.as_ref().unwrap().doctrines[0]
        .id
        .is_empty());
    assert_eq!(legacy.agent.doctrine.as_deref(), Some("Correctness"));
    assert_eq!(serde_json::to_value(&legacy).unwrap(), recorded);
}

#[test]
fn teammate_jobs_validate_but_never_capture_the_voice() {
    const PRIMARY: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    const LEAD: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
    let mut value = legacy();
    value["agents"].as_array_mut().unwrap().push(json!({
        "id":LEAD,"name":"Lead","model":"explicit-model",
        "ai_account":{"provider":"copilot","account_id":"33"},
        "prompt":"Decide.","signature":"lead"
    }));
    let repository = &mut value["repositories"][0];
    repository["assignments"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id":PRIMARY,"agent_id":LEAD,
            "schedule":{"kind":"interval","minutes":7,"timezone":"UTC"},
            "comment":true,"approve":false
        }));
    repository["primary_assignment_id"] = json!(PRIMARY);
    let fixture = Fixture::new();
    write(&fixture, value);
    let mut settings = with_voice(&fixture.store(), "Gilfoyle");
    let gilfoyle = voice(&settings, "Gilfoyle").id.clone();
    let teammate = Selection::resolve(&settings, &job(), ASSIGNMENT).unwrap();
    let configuration = teammate.configuration.as_ref().unwrap();
    assert!(!configuration.authority.primary);
    assert_eq!(configuration.voice, None);
    assert_eq!(teammate.agent.voice_id.as_deref(), Some(gilfoyle.as_str()));
    // Editing the Voice does not change the teammate's execution.
    settings.voices[1].body = "Edited after the job started.".into();
    settings.voices[1].guardrails = false;
    let later = Selection::resolve(&settings, &job(), ASSIGNMENT).unwrap();
    assert!(later.same_execution(&teammate));
    // A teammate's referenced Voice must still exist.
    settings.voices.remove(1);
    assert!(Selection::resolve(&settings, &job(), ASSIGNMENT)
        .unwrap_err()
        .contains("Voice"));
}
