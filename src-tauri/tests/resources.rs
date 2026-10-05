use pr_sniper_lib::{
    monitoring::{JobOperation, OperationState, QueueJob},
    review::{self, ReviewRun, Selection},
    storage::{ResourceEdit, Settings},
};
use serde_json::json;

mod support;
use support::Fixture;

const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const ASSIGNMENT: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login": false,
        "doctrines": [
            {"title":"Correctness","body":"Trace state transitions."},
            {"title":"Boundaries","body":"Keep authority explicit."}
        ],
        "agents": [{
            "id":AGENT,"name":"Reviewer","model":"explicit-model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "doctrine":"Correctness","prompt":"Find defects.","signature":"machine"
        }],
        "repositories": [{
            "id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{
                "id":ASSIGNMENT,"agent_id":AGENT,
                "schedule":{"kind":"interval","minutes":7,"timezone":"UTC"},
                "comment":true,"approve":true
            }]
        }]
    }))
    .unwrap()
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

fn agent_edit(expected: &Settings, name: &str) -> ResourceEdit {
    let mut agent = expected.agents[0].clone();
    agent.name = name.into();
    ResourceEdit::Agent {
        id: agent.id.clone(),
        expected: Some(expected.agents[0].clone()),
        value: Some(agent),
    }
}

#[test]
fn legacy_folder_and_cas_survive_upgrade_without_implicit_repository_authorization() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut original = settings();
    original.root_folder = Some("/retired/local/clones".into());
    original.repositories[0].enabled = false;
    store.save_settings(&original).unwrap();
    let loaded = store.load_settings().unwrap();
    let mut preferences = loaded.global_preferences();
    preferences.capacity = 7;
    let saved = store
        .save_resource(ResourceEdit::Preferences {
            expected: loaded.global_preferences(),
            value: preferences,
        })
        .unwrap();
    assert_eq!(saved.root_folder, original.root_folder);
    assert!(!saved.repositories[0].enabled);
    assert!(saved.repository_authorizations.is_empty());
    let mut repo = saved.repositories[0].clone();
    repo.enabled = true;
    let edit = ResourceEdit::Repository {
        id: REPO.into(),
        expected: Some(Box::new(saved.repositories[0].clone())),
        value: Some(Box::new(repo)),
    };
    let bytes = std::fs::read(fixture.path().join("config/settings.json")).unwrap();
    std::fs::create_dir(fixture.path().join("config/settings.json.tmp")).unwrap();
    assert!(store.save_resource(edit.clone()).is_err());
    assert_eq!(
        std::fs::read(fixture.path().join("config/settings.json")).unwrap(),
        bytes
    );
    std::fs::remove_dir(fixture.path().join("config/settings.json.tmp")).unwrap();
    let authorized = store.save_resource(edit.clone()).unwrap();
    assert_eq!(authorized.repository_authorizations.len(), 1);
    assert!(store
        .save_resource(edit)
        .unwrap_err()
        .contains("Resource changed"));
    assert_eq!(store.load_settings().unwrap(), authorized);
    assert_eq!(authorized.root_folder, original.root_folder);
}

#[test]
fn resource_saves_merge_unrelated_commits_and_reject_same_resource_conflicts() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let original = settings();
    store.save_settings(&original).unwrap();
    let mut repository = original.repositories[0].clone();
    repository.enabled = false;
    store
        .save_resource(ResourceEdit::Repository {
            id: REPO.into(),
            expected: Some(Box::new(original.repositories[0].clone())),
            value: Some(Box::new(repository.clone())),
        })
        .unwrap();
    let saved = store
        .save_resource(agent_edit(&original, "Renamed"))
        .unwrap();
    assert_eq!(saved.repositories[0], repository);
    assert_eq!(saved.agents[0].id, AGENT);
    assert_eq!(saved.agents[0].name, "Renamed");
    assert!(store
        .save_resource(agent_edit(&original, "Stale"))
        .unwrap_err()
        .contains("Resource changed"));
    assert_eq!(store.load_settings().unwrap(), saved);
    assert_eq!(saved.repositories[0].assignments[0].agent_id, AGENT);
    assert!(store
        .save_resource(ResourceEdit::Agent {
            id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into(),
            expected: None,
            value: None,
        })
        .unwrap_err()
        .contains("not saved"));
}

#[test]
fn validation_and_failed_writes_preserve_the_last_valid_bytes() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let original = settings();
    store.save_settings(&original).unwrap();
    let path = fixture.path().join("config/settings.json");
    let bytes = std::fs::read(&path).unwrap();
    let candidate = store
        .validate_resource(agent_edit(&original, "Validated only"))
        .unwrap();
    assert_eq!(candidate.agents[0].name, "Validated only");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(store.save_resource(agent_edit(&original, "")).is_err());
    std::fs::create_dir(fixture.path().join("config/settings.json.tmp")).unwrap();
    assert!(store
        .save_resource(agent_edit(&original, "Write failure"))
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(store.load_settings().unwrap(), original);
}

#[test]
fn doctrine_rename_preserves_all_references_and_deletion_requires_repair() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut original = settings();
    original.agents[0].doctrines = Some(vec!["correctness".into(), "Boundaries".into()]);
    store.save_settings(&original).unwrap();
    let doctrine = original.doctrines[0].clone();
    assert!(store
        .save_resource(ResourceEdit::Doctrine {
            title: doctrine.title.clone(),
            expected: Some(doctrine.clone()),
            value: None,
        })
        .unwrap_err()
        .contains("used by an Agent"));
    let mut renamed = doctrine.clone();
    renamed.title = "State".into();
    renamed.body = "Edited library text.".into();
    let saved = store
        .save_resource(ResourceEdit::Doctrine {
            title: doctrine.title.clone(),
            expected: Some(doctrine),
            value: Some(renamed),
        })
        .unwrap();
    assert_eq!(saved.agents[0].doctrine.as_deref(), Some("State"));
    assert_eq!(
        saved.agents[0].doctrine_titles(),
        vec!["State", "Boundaries"]
    );
    assert_eq!(saved.repositories, original.repositories);
    assert!(store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(saved.agents[0].clone()),
            value: None,
        })
        .unwrap_err()
        .contains("assigned"));
    let saved = store
        .save_resource(ResourceEdit::Repository {
            id: REPO.into(),
            expected: Some(Box::new(saved.repositories[0].clone())),
            value: None,
        })
        .unwrap();
    let saved = store
        .save_resource(ResourceEdit::Agent {
            id: AGENT.into(),
            expected: Some(saved.agents[0].clone()),
            value: None,
        })
        .unwrap();
    for doctrine in saved.doctrines {
        store
            .save_resource(ResourceEdit::Doctrine {
                title: doctrine.title.clone(),
                expected: Some(doctrine),
                value: None,
            })
            .unwrap();
    }
    let reopened = fixture.store().load_settings().unwrap();
    assert!(reopened.doctrines.is_empty());
    assert!(reopened.agents.is_empty());
}

#[test]
fn zero_one_many_doctrines_preserve_selection_order_and_legacy_loading() {
    let mut settings = settings();
    let resolve = |settings: &Settings| Selection::resolve(settings, &job(), ASSIGNMENT).unwrap();
    assert_eq!(
        resolve(&settings).doctrine.as_deref(),
        Some("Trace state transitions.")
    );
    settings.agents[0].doctrines = Some(vec![]);
    assert_eq!(resolve(&settings).doctrine, None);
    settings.agents[0].doctrines = Some(vec!["Boundaries".into(), "Correctness".into()]);
    let first = resolve(&settings);
    settings.doctrines.reverse();
    assert_eq!(resolve(&settings), first);
    assert_eq!(first.doctrine.as_deref(), Some("## Boundaries\n\nKeep authority explicit.\n\n## Correctness\n\nTrace state transitions."));
    assert_eq!(
        first.configuration.unwrap().doctrines[0].title,
        "Boundaries"
    );
    settings.agents[0].doctrines = Some(vec!["Correctness".into(), " correctness ".into()]);
    assert!(settings.validate().is_err());
    settings.agents[0].doctrines = Some(vec!["Missing".into()]);
    assert!(settings.validate().is_err());
}

#[test]
fn primary_and_action_choices_are_scoped_independent_and_safe_on_upgrade() {
    let mut settings = settings();
    assert_eq!(settings.capacity, 4);
    assert_eq!(
        settings.defaults.schedule,
        pr_sniper_lib::policy::Policy::default().schedule
    );
    let repository = &mut settings.repositories[0];
    let original = repository.assignments[0].clone();
    assert!(original.approve);
    let authority = repository.assignment_authority(&original);
    assert!(authority.primary);
    assert!(authority.comment);
    assert!(!authority.approve && !authority.merge);
    let mut second = original.clone();
    second.id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into();
    repository.assignments.push(second.clone());
    assert_eq!(repository.primary_assignment_id(), None);
    repository.primary_assignment_id = Some(original.id.clone());
    assert!(!repository.assignment_authority(&original).approve);
    repository.assignments[0].actions = Some(pr_sniper_lib::storage::ActionPermissions {
        approve: false,
        merge: true,
    });
    let authority = repository.assignment_authority(&repository.assignments[0]);
    assert!(!authority.approve && authority.merge);
    repository.primary_assignment_id = Some(second.id);
    let authority = repository.assignment_authority(&repository.assignments[0]);
    assert!(!authority.primary && !authority.merge);
    assert!(repository.assignments[0].actions.as_ref().unwrap().merge);
    repository.primary_assignment_id = Some("missing".into());
    assert!(settings.validate().is_err());
}

#[test]
fn global_policy_saves_preserve_libraries_legacy_choices_and_readiness() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = settings();
    store.save_settings(&settings).unwrap();
    let mut value = settings.global_preferences();
    value.capacity = 6;
    value.presets.push(pr_sniper_lib::storage::ReviewPreset {
        id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into(),
        name: "Saved preset".into(),
        body: "Preserve effective preset instructions.".into(),
    });
    value.default_review_preset = Some(value.presets[0].id.clone());
    value.defaults.schedule = serde_json::from_value(json!({
        "kind":"cron","expression":"0 9 * * MON-FRI","timezone":"America/New_York"
    }))
    .unwrap();
    let saved = store
        .save_resource(ResourceEdit::Preferences {
            expected: settings.global_preferences(),
            value,
        })
        .unwrap();
    assert_eq!(saved.agents, settings.agents);
    assert_eq!(saved.repositories, settings.repositories);
    assert!(!saved.defaults.automatic_agent_start);
    assert_eq!(
        saved.defaults.prompt,
        "Preserve effective preset instructions."
    );
    assert_eq!(store.saved_resources().unwrap().settings, saved);
    assert!(
        store
            .saved_resources()
            .unwrap()
            .readiness
            .configuration_ready
    );
    for (expression, timezone, capacity) in [
        ("not cron", "UTC", 4),
        ("0 0 0 * * *", "UTC", 4),
        ("*/15 * * * *", "Mars/Olympus_Mons", 4),
        ("*/15 * * * *", "UTC", 0),
    ] {
        let mut value = saved.global_preferences();
        value.capacity = capacity;
        value.defaults.schedule = serde_json::from_value(json!({
            "kind":"cron","expression":expression,"timezone":timezone
        }))
        .unwrap();
        assert!(store
            .save_resource(ResourceEdit::Preferences {
                expected: saved.global_preferences(),
                value,
            })
            .is_err());
        assert_eq!(store.load_settings().unwrap(), saved);
    }
}

#[test]
fn snapshots_and_completed_evidence_do_not_change_after_edits_or_restart() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let original = settings();
    store.save_settings(&original).unwrap();
    let job = job();
    let selection = Selection::resolve(&original, &job, ASSIGNMENT).unwrap();
    assert_eq!(
        selection.agent.ai_account.as_ref().unwrap().account_id,
        "33"
    );
    assert_eq!(
        selection
            .configuration
            .as_ref()
            .unwrap()
            .repository
            .provider_account_id
            .as_deref(),
        Some("22")
    );
    let mut run = ReviewRun {
        feedback_context: None,
        key: review::key(&job, ASSIGNMENT),
        assignment_id: ASSIGNMENT.into(),
        operation: JobOperation::review(&job, 100),
        job,
        selection: selection.clone(),
        manual_start: true,
        trust_confirmed: true,
        phase: "Completed fixture".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({
                "reviewed_base_sha":"b".repeat(40),
                "output":{
                    "synopsis":"No actionable defects were found.",
                    "files":[{"path":"source.rs","explanation":"Reviewed source.","order":1}],
                    "findings":[],"decision":"machine_sign_off"
                },
                "session_id":"fixture-session","model":"explicit-model","runtime_version":"fixture",
                "input_tokens":10,"output_tokens":20,"tool_calls":1
            }))
            .unwrap(),
        ),
    };
    run.operation.state = OperationState::Completed;
    store.save_reviews(&[run.clone()]).unwrap();
    let bytes = std::fs::read(fixture.path().join("state/reviews.json")).unwrap();
    let saved = store
        .save_resource(agent_edit(&original, "Changed after execution"))
        .unwrap();
    assert_ne!(
        Selection::resolve(&saved, &run.job, ASSIGNMENT).unwrap(),
        selection
    );
    let mut doctrine = saved.doctrines[0].clone();
    doctrine.body = "Changed after execution.".into();
    store
        .save_resource(ResourceEdit::Doctrine {
            title: doctrine.title.clone(),
            expected: Some(saved.doctrines[0].clone()),
            value: Some(doctrine),
        })
        .unwrap();
    store
        .save_resource(ResourceEdit::Repository {
            id: REPO.into(),
            expected: Some(Box::new(saved.repositories[0].clone())),
            value: None,
        })
        .unwrap();
    review::restore(&store).unwrap();
    assert_eq!(
        std::fs::read(fixture.path().join("state/reviews.json")).unwrap(),
        bytes
    );
    assert_eq!(store.load_reviews().unwrap()[0], run);
    run.operation.state = OperationState::Running;
    store.save_reviews(&[run]).unwrap();
    review::restore(&store).unwrap();
    let interrupted = store.load_reviews().unwrap().remove(0);
    assert_eq!(interrupted.operation.state, OperationState::Interrupted);
    assert_eq!(interrupted.selection, selection);
    let mut legacy = serde_json::to_value(selection).unwrap();
    legacy.as_object_mut().unwrap().remove("configuration");
    let legacy: Selection = serde_json::from_value(legacy).unwrap();
    assert!(legacy.configuration.is_none());
}
