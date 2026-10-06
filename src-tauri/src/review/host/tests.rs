use super::*;
use crate::storage::{Settings, Store};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Store, ReviewRun) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let repo_id = uuid::Uuid::new_v4().to_string();
    let agent_id = uuid::Uuid::new_v4().to_string();
    let assignment_id = uuid::Uuid::new_v4().to_string();
    let settings:Settings=serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{
            "id":repo_id,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":assignment_id,"agent_id":agent_id,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":false}]
        }],
        "agents":[{
            "id":agent_id,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"machine"
        }]
    })).unwrap();
    store.save_settings(&settings).unwrap();
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id":assignment_id,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":repo_id,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"all_authors":false,"requested_reviewer":false,
        "waiting":"human_start","detected_at":100
    }))
    .unwrap();
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    let run = ReviewRun {
        feedback_context: None,
        key: key(&job, &assignment_id),
        assignment_id: assignment_id.clone(),
        selection: Selection::resolve(&settings, &job, &assignment_id).unwrap(),
        operation: monitoring::JobOperation::review(&job, 100),
        job,
        manual_start: true,
        trust_confirmed: false,
        phase: "Queued".into(),
        error: None,
        result: None,
    };
    (root, store, run)
}

#[test]
fn configuration_edits_and_eligibility_loss_invalidate_saved_attempts() {
    for change in 0..5 {
        let (_root, store, run) = fixture();
        crate::review::validate_execution_selection(&store, &run).unwrap();
        let mut settings = store.load_settings().unwrap();
        match change {
            0 => settings.repositories[0].enabled = false,
            1 => settings.agents[0].prompt = "Changed prompt.".into(),
            2 => settings.agents[0].model = "another-model".into(),
            3 => settings.defaults.automatic_comment_publication = true,
            _ => {
                let mut jobs = store.load_queue().unwrap();
                jobs[0].waiting = "superseded".into();
                store.save_queue(&jobs).unwrap();
            }
        }
        store.save_settings(&settings).unwrap();
        assert!(crate::review::validate_execution_selection(&store, &run).is_err());
    }
}

#[test]
fn doctrine_reconciliation_captures_real_dispatch_and_preserves_actual_running_evidence() {
    let (root, store, template) = fixture();
    let mut settings = store.load_settings().unwrap();
    settings.doctrines = vec![
        crate::storage::Doctrine {
            title: "code".into(),
            body: "Old code principles.".into(),
        },
        crate::storage::Doctrine {
            title: "domain".into(),
            body: "Obsolete domain principles.".into(),
        },
    ];
    settings.agents[0].doctrines = Some(vec!["domain".into(), "code".into()]);
    store.save_settings(&settings).unwrap();
    request(&store, &template.key, true, 100).unwrap();
    let actual = prepare_dispatch(&store, &template.key, 100).unwrap();
    assert_eq!(actual.operation.state, OperationState::Running);
    let actual_catalog = actual
        .selection
        .configuration
        .as_ref()
        .unwrap()
        .doctrine_catalog
        .clone();
    let evidence_path = root.path().join("state/reviews.json");
    let evidence = std::fs::read(&evidence_path).unwrap();
    let mut legacy = serde_json::to_value(&settings).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("doctrine_catalog_version");
    std::fs::write(
        root.path().join("config/settings.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let resolved = store.load_settings().unwrap();
    assert_eq!(resolved.doctrines.len(), 10);
    assert_eq!(resolved.agents[0].doctrine_titles(), ["code"]);
    assert_eq!(std::fs::read(&evidence_path).unwrap(), evidence);
    let retained = store.load_reviews().unwrap().remove(0);
    assert_eq!(retained.selection, actual.selection);
    assert_eq!(
        retained
            .selection
            .configuration
            .as_ref()
            .unwrap()
            .doctrine_catalog,
        actual_catalog
    );
    assert!(crate::review::validate_execution_selection(&store, &retained).is_err());

    // Another admitted revision dispatches from the reconciled shared resource.
    let mut next_job = template.job.clone();
    next_job.head_sha = "c".repeat(40);
    let next_key = key(&next_job, &template.assignment_id);
    store.save_queue(std::slice::from_ref(&next_job)).unwrap();
    request(&store, &next_key, true, 200).unwrap();
    let next = prepare_dispatch(&store, &next_key, 200).unwrap();
    let captured = next.selection.configuration.as_ref().unwrap();
    assert_eq!(captured.doctrine_catalog, Some(resolved.doctrine_catalog()));
    assert_eq!(
        captured.doctrines,
        vec![resolved
            .doctrines
            .iter()
            .find(|d| d.title == "code")
            .unwrap()
            .clone()]
    );
    assert_eq!(
        next.selection.doctrine.as_deref(),
        Some(captured.doctrines[0].body.as_str())
    );
    assert_eq!(store.load_reviews().unwrap().len(), 2);
    let mut edited = resolved.clone();
    edited
        .doctrines
        .iter_mut()
        .find(|d| d.title == "code")
        .unwrap()
        .body = "Later deliberate principles.".into();
    store.save_preferences(edited, &resolved).unwrap();
    assert_eq!(store.load_reviews().unwrap()[1].selection, next.selection);
    assert!(crate::review::validate_execution_selection(&store, &next).is_err());
}

#[test]
fn history_survives_assignment_removal_and_legacy_detections_stay_blocked() {
    let (_root, store, mut run) = fixture();
    run.operation.state = OperationState::Failed;
    run.error = Some("Visible review failure.".into());
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    assert_eq!(
        candidates(&store).unwrap()[0].run.as_ref().unwrap().error,
        run.error
    );
    let mut jobs = store.load_queue().unwrap();
    jobs[0].assignment_id = None;
    store.save_queue(&jobs).unwrap();
    assert!(candidates(&store).unwrap()[0].blocked.is_some());
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments.clear();
    store.save_settings(&settings).unwrap();
    let historical = candidates(&store).unwrap();
    assert_eq!(historical.len(), 1);
    assert!(historical[0].blocked.is_some());
    assert_eq!(
        historical[0].run.as_ref().unwrap().operation.id,
        run.operation.id
    );
}

#[test]
fn retry_replaces_only_same_operation_and_preserves_prior_operation_history() {
    let (_root, _store, run) = fixture();
    let mut records = vec![run.clone()];
    let mut update = run.clone();
    update.operation.begin_attempt(100).unwrap();
    replace_run(&mut records, &update);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation.attempt_count, 1);
    update.operation = monitoring::JobOperation::review(&run.job, 2000);
    replace_run(&mut records, &update);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].operation.id, run.operation.id);
}
