use pr_sniper_lib::{
    monitoring::QueueJob,
    review::Selection,
    storage::{DuckLimits, ResourceEdit, Settings},
};
use serde_json::json;
use std::fs;

mod support;
use support::Fixture;

const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DUCK: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const ASSIGNMENT: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const TEAMMATE: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

fn agent(id: &str, name: &str) -> serde_json::Value {
    json!({"id":id,"name":name,"model":"explicit-model",
        "ai_account":{"provider":"copilot","account_id":"33"},
        "prompt":"Find defects.","signature":"machine"})
}

/// Settings written before Rubber Duck existed: no limits, no assignment duck.
fn legacy() -> serde_json::Value {
    json!({
        "launch_at_login": false,
        "doctrine_catalog_version": 1,
        "voice_catalog_version": 1,
        "admission_migration_version": 1,
        "agents": [agent(AGENT, "Reviewer"), agent(DUCK, "Duck")],
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

fn write(fixture: &Fixture, value: &serde_json::Value) {
    let path = fixture.path().join("config/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn job(assignment: &str) -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":assignment,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":REPO,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"all_authors":false,"requested_reviewer":false,
        "waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

fn with_duck(fixture: &Fixture) -> Settings {
    let mut value = legacy();
    value["repositories"][0]["assignments"][0]["rubber_duck"] = json!(DUCK);
    write(fixture, &value);
    fixture.store().load_settings().unwrap()
}

#[test]
fn settings_without_duck_fields_load_with_defaults_and_save_without_them() {
    let fixture = Fixture::new();
    write(&fixture, &legacy());
    let store = fixture.store();
    let settings = store.load_settings().unwrap();
    assert_eq!(settings.rubber_duck, DuckLimits::default());
    assert_eq!(
        (
            settings.rubber_duck.max_calls,
            settings.rubber_duck.call_timeout_seconds,
            settings.rubber_duck.primary_window_minutes
        ),
        (8, 180, 15)
    );
    assert_eq!(settings.repositories[0].assignments[0].rubber_duck, None);
    store.save_settings(&settings).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path().join("config/settings.json")).unwrap())
            .unwrap();
    assert!(saved.get("rubber_duck").is_none());
    assert!(saved["repositories"][0]["assignments"][0]
        .get("rubber_duck")
        .is_none());
}

#[test]
fn unknown_duck_fields_are_still_rejected() {
    let fixture = Fixture::new();
    let mut value = legacy();
    value["rubber_duck"] =
        json!({"max_calls":8,"call_timeout_seconds":180,"primary_window_minutes":15,"extra":1});
    write(&fixture, &value);
    assert!(fixture.store().load_settings().is_err());
}

#[test]
fn preferences_save_tuned_limits_and_refuse_out_of_range_values() {
    let fixture = Fixture::new();
    write(&fixture, &legacy());
    let store = fixture.store();
    let settings = store.load_settings().unwrap();
    let expected = settings.global_preferences();
    let mut value = expected.clone();
    value.rubber_duck = DuckLimits {
        max_calls: 32,
        call_timeout_seconds: 30,
        primary_window_minutes: 120,
    };
    let saved = store
        .save_resource(ResourceEdit::Preferences {
            expected: expected.clone(),
            value: value.clone(),
        })
        .unwrap();
    assert_eq!(saved.rubber_duck, value.rubber_duck);
    assert_eq!(
        store.load_settings().unwrap().rubber_duck,
        value.rubber_duck
    );
    for bad in [
        DuckLimits {
            max_calls: 0,
            ..value.rubber_duck
        },
        DuckLimits {
            max_calls: 33,
            ..value.rubber_duck
        },
        DuckLimits {
            call_timeout_seconds: 29,
            ..value.rubber_duck
        },
        DuckLimits {
            call_timeout_seconds: 601,
            ..value.rubber_duck
        },
        DuckLimits {
            primary_window_minutes: 4,
            ..value.rubber_duck
        },
        DuckLimits {
            primary_window_minutes: 121,
            ..value.rubber_duck
        },
    ] {
        let mut next = value.clone();
        next.rubber_duck = bad;
        assert!(store
            .save_resource(ResourceEdit::Preferences {
                expected: value.clone(),
                value: next,
            })
            .is_err());
    }
    // Preferences sent without the field (older UI) keep defaults.
    let without: pr_sniper_lib::storage::GlobalPreferences = serde_json::from_value({
        let mut v = serde_json::to_value(&expected).unwrap();
        v.as_object_mut().unwrap().remove("rubber_duck");
        v
    })
    .unwrap();
    assert_eq!(without.rubber_duck, DuckLimits::default());
}

#[test]
fn a_primary_duck_adds_rubber_duck_to_the_default_tools_and_a_teammate_duck_is_inert() {
    let fixture = Fixture::new();
    let settings = with_duck(&fixture);
    let repository = &settings.repositories[0];
    let assignment = &repository.assignments[0];
    let tools = repository.assignment_tools(assignment);
    assert_eq!(tools[7], "rubber_duck", "after the investigation tools");
    assert_eq!(repository.assignment_duck(assignment), Some(DUCK));
    // Same duck on a teammate: kept, not honored.
    let mut value = legacy();
    let mut teammate = value["repositories"][0]["assignments"][0].clone();
    teammate["id"] = json!(TEAMMATE);
    teammate["rubber_duck"] = json!(DUCK);
    value["repositories"][0]["assignments"]
        .as_array_mut()
        .unwrap()
        .push(teammate);
    value["repositories"][0]["primary_assignment_id"] = json!(ASSIGNMENT);
    write(&fixture, &value);
    let settings = fixture.store().load_settings().unwrap();
    let repository = &settings.repositories[0];
    let teammate = &repository.assignments[1];
    assert_eq!(teammate.rubber_duck.as_deref(), Some(DUCK));
    assert_eq!(repository.assignment_duck(teammate), None);
    assert!(!repository
        .assignment_tools(teammate)
        .contains(&"rubber_duck".to_string()));
    let selection = Selection::resolve(&settings, &job(TEAMMATE), TEAMMATE).unwrap();
    assert!(selection.configuration.unwrap().rubber_duck.is_none());
}

#[test]
fn readiness_flags_duck_tool_without_a_duck_teammate_duck_tools_and_a_duck_without_a_model() {
    let fixture = Fixture::new();
    let mut value = legacy();
    value["repositories"][0]["assignments"][0]["tools"] =
        json!(["read_changes", "rubber_duck", "ready_for_human"]);
    write(&fixture, &value);
    let settings = fixture.store().load_settings().unwrap();
    let issues = settings.readiness().repositories[0].issues.join(" ");
    assert!(
        issues.contains("tool rubber_duck needs a Rubber duck"),
        "{issues}"
    );

    value["repositories"][0]["assignments"][0]["rubber_duck"] = json!(DUCK);
    value["agents"][1]
        .as_object_mut()
        .unwrap()
        .remove("ai_account");
    write(&fixture, &value);
    let settings = fixture.store().load_settings().unwrap();
    let issues = settings.readiness().repositories[0].issues.join(" ");
    assert!(!issues.contains("needs a Rubber duck"), "{issues}");
    assert!(
        issues.contains("Rubber duck needs an explicit AI account and model"),
        "{issues}"
    );
}

#[test]
fn a_duck_must_exist_and_a_referenced_duck_cannot_be_deleted() {
    let fixture = Fixture::new();
    let mut value = legacy();
    value["repositories"][0]["assignments"][0]["rubber_duck"] =
        json!("ffffffff-ffff-4fff-8fff-ffffffffffff");
    let settings: Settings = serde_json::from_value(value).unwrap();
    assert!(settings.validate().is_err());

    let settings = with_duck(&fixture);
    let duck = settings.agents.iter().find(|a| a.id == DUCK).cloned();
    let error = fixture
        .store()
        .save_resource(ResourceEdit::Agent {
            id: DUCK.into(),
            expected: duck,
            value: None,
        })
        .unwrap_err();
    assert!(error.contains("assigned"), "{error}");
}

#[test]
fn the_duck_may_be_the_primarys_own_agent() {
    let fixture = Fixture::new();
    let mut value = legacy();
    value["repositories"][0]["assignments"][0]["rubber_duck"] = json!(AGENT);
    write(&fixture, &value);
    let settings = fixture.store().load_settings().unwrap();
    let selection = Selection::resolve(&settings, &job(ASSIGNMENT), ASSIGNMENT).unwrap();
    assert_eq!(
        selection
            .configuration
            .unwrap()
            .rubber_duck
            .unwrap()
            .agent
            .id,
        AGENT
    );
}

#[test]
fn the_duck_snapshot_resolves_doctrines_by_id_and_compares_by_lens_without_a_voice() {
    let fixture = Fixture::new();
    let mut value = legacy();
    value["doctrines"] = json!([
        {"title":"Correctness","body":"Trace state."},
        {"title":"Boundaries","body":"Own meaning."}
    ]);
    value["voices"] = json!([{"title":"Merlin","body":"Speak in riddles."}]);
    value["agents"][1]["doctrines"] = json!(["Boundaries", "Correctness"]);
    value["repositories"][0]["assignments"][0]["rubber_duck"] = json!(DUCK);
    write(&fixture, &value);
    let store = fixture.store();
    let settings = store.load_settings().unwrap();
    let duck = settings.agents.iter().find(|a| a.id == DUCK).unwrap();
    assert_eq!(duck.doctrine_ids.len(), 2);
    let selection = Selection::resolve(&settings, &job(ASSIGNMENT), ASSIGNMENT).unwrap();
    let snapshot = selection
        .configuration
        .clone()
        .unwrap()
        .rubber_duck
        .unwrap();
    let ids: Vec<_> = snapshot.doctrines.iter().map(|d| d.id.clone()).collect();
    assert_eq!(ids, duck.doctrine_ids, "in the duck Agent's order, by id");
    assert_eq!(snapshot.doctrines[0].title, "Boundaries");

    let again =
        |settings: &Settings| Selection::resolve(settings, &job(ASSIGNMENT), ASSIGNMENT).unwrap();
    // A Voice on the duck is ignored: duck runs are caveman.
    let mut voiced = settings.clone();
    let voice = voiced.voices[0].id.clone();
    voiced
        .agents
        .iter_mut()
        .find(|a| a.id == DUCK)
        .unwrap()
        .voice_id = Some(voice);
    assert!(selection.same_execution(&again(&voiced)));
    // A duck Doctrine body or order change is a different execution.
    let mut edited = settings.clone();
    let id = &duck.doctrine_ids[0];
    edited
        .doctrines
        .iter_mut()
        .find(|d| &d.id == id)
        .unwrap()
        .body = "Changed.".into();
    assert!(!selection.same_execution(&again(&edited)));
    let mut reordered = settings.clone();
    reordered
        .agents
        .iter_mut()
        .find(|a| a.id == DUCK)
        .unwrap()
        .doctrine_ids
        .reverse();
    assert!(!selection.same_execution(&again(&reordered)));
    // Unchanged settings: same execution; the captured snapshot is never rewritten.
    assert!(selection.same_execution(&again(&settings)));
    assert_eq!(
        selection.configuration.unwrap().rubber_duck.unwrap(),
        snapshot
    );
}
