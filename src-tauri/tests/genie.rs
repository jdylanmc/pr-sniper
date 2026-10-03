use pr_sniper_lib::{
    capacity::Automation,
    github::{
        provider::{Capabilities, CommentCapability, Connection, RemoteRepository},
        Identity,
    },
    monitoring::{
        AccountAvailability, ActivationMode, ActivationPreviewEvidence, Monitor, SetupActivation,
        SetupReview,
    },
    storage::{Settings, Store},
};
use serde_json::json;
use std::{collections::BTreeMap, fs};

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

fn fixture() -> (tempfile::TempDir, Store, Monitor) {
    fs::create_dir_all("target/genie-41-delivery").unwrap();
    let root = tempfile::tempdir_in("target/genie-41-delivery").unwrap();
    let store = Store::new(root.path().to_path_buf());
    let mut settings = store.load_settings().unwrap();
    settings.agents = serde_json::from_value(json!([{
        "id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","name":"Explicit reviewer",
        "model":"fixture-model","ai_account":{"provider":"copilot","account_id":"33"},
        "doctrines":[],"prompt":"Review correctness.","signature":"machine"
    }]))
    .unwrap();
    settings.repositories = serde_json::from_value(json!([{
        "id":REPO,"provider":"github","name":"fixture/genie","enabled":true,
        "provider_account_id":"22","provider_repository_id":"100",
        "assignments":[{
            "id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","agent_id":settings.agents[0].id,
            "schedule":{"kind":"cron","expression":"*/15 * * * *","timezone":"UTC"},
            "comment":false,"actions":{"approve":false,"merge":false}
        }]
    }]))
    .unwrap();
    store.save_settings(&settings).unwrap();
    let monitor = Monitor::restore(&store).unwrap();
    (root, store, monitor)
}

fn accounts(id: &str) -> BTreeMap<String, AccountAvailability> {
    BTreeMap::from([(
        id.into(),
        AccountAvailability {
            login: format!("fixture-{id}"),
            connected: true,
        },
    )])
}

fn review(store: &Store, monitor: &Monitor) -> SetupReview {
    monitor
        .setup_review(store, accounts("22"), accounts("33"), &BTreeMap::new())
        .unwrap()
}

fn preview(store: &Store, monitor: &mut Monitor, id: &str) -> SetupActivation {
    let settings = store.load_settings().unwrap();
    let context = Monitor::activation_context(&settings, id).unwrap();
    let connection = Connection {
        identity: Identity {
            id: context.account_id.clone(),
            login: "fixture-22".into(),
        },
        repository: RemoteRepository {
            id: context.provider_repository_id.clone(),
            name: context.name.clone(),
        },
        capabilities: Capabilities {
            read: true,
            comment: CommentCapability::Available,
        },
    };
    let view = monitor
        .stage_activation_preview(
            &settings,
            ActivationPreviewEvidence {
                context,
                connection,
                pull_requests: Vec::new(),
                creation_watermark: 12,
                account_generation: 0,
            },
            0,
        )
        .unwrap();
    SetupActivation {
        repository_id: id.into(),
        preview_id: view.preview_id,
        mode: ActivationMode::NewOnly,
        selected_pull_request_ids: Vec::new(),
    }
}

#[test]
fn fresh_and_saved_partial_setup_never_manufacture_work_or_activate() {
    let (root, store, mut monitor) = fixture();
    let saved = store.load_settings().unwrap();
    assert!(saved.readiness().configuration_ready);
    assert!(!saved.defaults.automatic_agent_start);
    assert!(!saved.defaults.automatic_comment_publication);
    assert!(monitor
        .prepare_checks(&store, 1_800_000_000, true)
        .unwrap()
        .is_empty());
    let _unconfirmed = preview(&store, &mut monitor, REPO);
    assert!(monitor
        .prepare_checks(&store, 1_800_000_001, true)
        .unwrap()
        .is_empty());
    let restored = Monitor::restore(&store).unwrap();
    assert!(!restored.activation_status(&saved, REPO).active);
    assert_eq!(store.load_settings().unwrap(), saved);
    assert!(store.load_queue().unwrap().is_empty());
    let fresh = Store::new(root.path().join("fresh"));
    let resources = fresh.saved_resources().unwrap();
    assert!(resources.settings.agents.is_empty());
    assert!(resources.settings.repositories.is_empty());
    assert!(!resources.settings.doctrines.is_empty());
    assert!(!resources.readiness.configuration_ready);
    assert!(!resources.settings.launch_at_login);
}

#[test]
fn final_confirmation_rejects_every_changed_effective_resource() {
    let mutations: Vec<fn(&mut Settings)> = vec![
        |s| s.capacity += 1,
        |s| {
            s.defaults.schedule = serde_json::from_value(
                json!({"kind":"cron","expression":"*/5 * * * *","timezone":"America/New_York"}),
            )
            .unwrap()
        },
        |s| s.defaults.automatic_agent_start = true,
        |s| s.defaults.automatic_comment_publication = true,
        |s| s.repositories[0].assignments[0].comment = true,
        |s| {
            s.repositories[0].assignments[0].actions =
                Some(serde_json::from_value(json!({"approve":true,"merge":true})).unwrap())
        },
        |s| s.repositories[0].provider_account_id = Some("44".into()),
        |s| s.repositories[0].provider_repository_id = Some("101".into()),
        |s| s.repositories[0].enabled = false,
        |s| {
            s.repositories[0]
                .watched_authors
                .push(serde_json::from_value(json!({"id":"55","login":"new-author"})).unwrap())
        },
        |s| s.agents[0].model = "changed-model".into(),
        |s| s.agents[0].ai_account.as_mut().unwrap().account_id = "44".into(),
        |s| s.agents[0].prompt = "Changed principles.".into(),
        |s| s.doctrines[0].body.push_str("\nChanged shared doctrine."),
    ];
    for mutate in mutations {
        let (_root, store, mut monitor) = fixture();
        let request = preview(&store, &mut monitor, REPO);
        let shown = review(&store, &monitor);
        let mut changed = store.load_settings().unwrap();
        mutate(&mut changed);
        store.save_settings(&changed).unwrap();
        let current = review(&store, &monitor);
        assert!(monitor
            .apply_setup(
                &store,
                &current,
                &shown.confirmation,
                &[request],
                &BTreeMap::new(),
                100
            )
            .unwrap_err()
            .contains("Setup changed"));
        assert!(store
            .load_monitoring_state()
            .unwrap()
            .activations
            .is_empty());
    }
}

#[test]
fn disconnected_or_changed_accounts_and_pause_invalidate_confirmation() {
    for role in ["repository", "ai", "login", "generation", "pause"] {
        let (_root, store, mut monitor) = fixture();
        let request = preview(&store, &mut monitor, REPO);
        let shown = review(&store, &monitor);
        let mut repository = accounts("22");
        let mut ai = accounts("33");
        let mut generations = BTreeMap::new();
        match role {
            "repository" => repository.get_mut("22").unwrap().connected = false,
            "ai" => ai.get_mut("33").unwrap().connected = false,
            "login" => ai.get_mut("33").unwrap().login = "renamed".into(),
            "generation" => {
                generations.insert("22".into(), 1);
            }
            "pause" => store.save_automation(&Automation { paused: true }).unwrap(),
            _ => unreachable!(),
        }
        let current = monitor
            .setup_review(&store, repository, ai, &generations)
            .unwrap();
        assert!(monitor
            .apply_setup(
                &store,
                &current,
                &shown.confirmation,
                std::slice::from_ref(&request),
                &generations,
                100
            )
            .unwrap_err()
            .contains("Setup changed"));
        if role == "repository" || role == "ai" {
            assert!(monitor
                .apply_setup(
                    &store,
                    &current,
                    &current.confirmation,
                    &[request],
                    &generations,
                    100
                )
                .unwrap_err()
                .contains("Reconnect"));
        }
        assert!(store
            .load_monitoring_state()
            .unwrap()
            .activations
            .is_empty());
    }
}

#[test]
fn batch_validation_and_failed_write_do_not_partially_activate() {
    let (root, store, mut monitor) = fixture();
    let mut settings = store.load_settings().unwrap();
    let mut second = settings.repositories[0].clone();
    second.id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into();
    second.name = "fixture/second".into();
    second.provider_repository_id = Some("101".into());
    settings.repositories.push(second.clone());
    store.save_settings(&settings).unwrap();
    let first = preview(&store, &mut monitor, REPO);
    let mut next = preview(&store, &mut monitor, &second.id);
    let shown = review(&store, &monitor);
    let valid = next.clone();
    next.mode = ActivationMode::SelectedExisting;
    next.selected_pull_request_ids.push("unpreviewed".into());
    assert!(monitor
        .apply_setup(
            &store,
            &shown,
            &shown.confirmation,
            &[first.clone(), next],
            &BTreeMap::new(),
            100
        )
        .is_err());
    assert!(store
        .load_monitoring_state()
        .unwrap()
        .activations
        .is_empty());
    fs::create_dir_all(root.path().join("state/monitoring.json")).unwrap();
    assert!(monitor
        .apply_setup(
            &store,
            &shown,
            &shown.confirmation,
            &[first.clone(), valid.clone()],
            &BTreeMap::new(),
            100
        )
        .is_err());
    assert!(!monitor.activation_status(&settings, REPO).active);
    assert!(!monitor.activation_status(&settings, &second.id).active);
    fs::remove_dir(root.path().join("state/monitoring.json")).unwrap();
    monitor
        .apply_setup(
            &store,
            &shown,
            &shown.confirmation,
            &[first, valid],
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    assert_eq!(store.load_monitoring_state().unwrap().activations.len(), 2);
    assert_eq!(store.load_settings().unwrap(), settings);
}

#[test]
fn reentry_preserves_authorized_scope_pause_and_resources_without_replay() {
    let (_root, store, mut monitor) = fixture();
    let request = preview(&store, &mut monitor, REPO);
    let shown = review(&store, &monitor);
    store.save_automation(&Automation { paused: true }).unwrap();
    let current = review(&store, &monitor);
    assert!(monitor
        .apply_setup(
            &store,
            &current,
            &shown.confirmation,
            std::slice::from_ref(&request),
            &BTreeMap::new(),
            100
        )
        .is_err());
    monitor
        .apply_setup(
            &store,
            &current,
            &current.confirmation,
            &[request],
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    let saved_settings = store.load_settings().unwrap();
    let saved_scope = store.load_monitoring_state().unwrap();
    let mut restored = Monitor::restore(&store).unwrap();
    let reentry = review(&store, &restored);
    restored
        .apply_setup(
            &store,
            &reentry,
            &reentry.confirmation,
            &[],
            &BTreeMap::new(),
            101,
        )
        .unwrap();
    assert_eq!(store.load_monitoring_state().unwrap(), saved_scope);
    assert_eq!(store.load_settings().unwrap(), saved_settings);
    assert!(store.load_automation().unwrap().paused);
    let replay = preview(&store, &mut restored, REPO);
    assert!(restored
        .apply_setup(
            &store,
            &reentry,
            &reentry.confirmation,
            &[replay],
            &BTreeMap::new(),
            102
        )
        .unwrap_err()
        .contains("Already-authorized"));
    assert_eq!(store.load_monitoring_state().unwrap(), saved_scope);
}

#[test]
fn scope_version_and_primary_changes_invalidate_a_shown_final_check() {
    let (_root, store, mut monitor) = fixture();
    let request = preview(&store, &mut monitor, REPO);
    let shown = review(&store, &monitor);
    let mut settings = store.load_settings().unwrap();
    let mut second = settings.repositories[0].assignments[0].clone();
    second.id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into();
    let mut agent = settings.agents[0].clone();
    agent.id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into();
    agent.name = "Second reviewer".into();
    second.agent_id = agent.id.clone();
    settings.agents.push(agent);
    settings.repositories[0].assignments.push(second.clone());
    settings.repositories[0].primary_assignment_id = Some(second.id);
    store.save_settings(&settings).unwrap();
    let current = review(&store, &monitor);
    assert!(monitor
        .apply_setup(
            &store,
            &current,
            &shown.confirmation,
            &[request],
            &BTreeMap::new(),
            100
        )
        .unwrap_err()
        .contains("Setup changed"));
    let request = preview(&store, &mut monitor, REPO);
    let shown = review(&store, &monitor);
    monitor
        .apply_activation(
            &store,
            &settings,
            pr_sniper_lib::monitoring::ActivationApplication {
                repository_id: REPO,
                preview_id: &request.preview_id,
                mode: ActivationMode::NewOnly,
                selected_pull_request_ids: &[],
                account_generation: 0,
                now: 101,
            },
        )
        .unwrap();
    let saved_scope = store.load_monitoring_state().unwrap();
    let current = review(&store, &monitor);
    assert!(monitor
        .apply_setup(
            &store,
            &current,
            &shown.confirmation,
            &[],
            &BTreeMap::new(),
            102
        )
        .unwrap_err()
        .contains("Setup changed"));
    assert_eq!(store.load_monitoring_state().unwrap(), saved_scope);
}

#[test]
fn rejected_stale_preview_cannot_be_reused_after_configuration_roundtrip() {
    let (_root, store, mut monitor) = fixture();
    let request = preview(&store, &mut monitor, REPO);
    let original = store.load_settings().unwrap();
    let mut changed = original.clone();
    changed.defaults.reviewer_assignment = !changed.defaults.reviewer_assignment;
    store.save_settings(&changed).unwrap();
    let current = review(&store, &monitor);
    assert!(monitor
        .apply_setup(
            &store,
            &current,
            &current.confirmation,
            std::slice::from_ref(&request),
            &BTreeMap::new(),
            100
        )
        .unwrap_err()
        .contains("configuration changed"));
    store.save_settings(&original).unwrap();
    let current = review(&store, &monitor);
    assert!(monitor
        .apply_setup(
            &store,
            &current,
            &current.confirmation,
            &[request],
            &BTreeMap::new(),
            101
        )
        .unwrap_err()
        .contains("preview expired"));
    assert!(store
        .load_monitoring_state()
        .unwrap()
        .activations
        .is_empty());
}
