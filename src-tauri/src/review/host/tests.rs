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
        mode: Default::default(),
        packet: None,
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
            3 => {
                settings.repositories[0].overrides.prompt = Some("Changed repository lens.".into())
            }
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
            id: String::new(),
            title: "code".into(),
            body: "Old code principles.".into(),
        },
        crate::storage::Doctrine {
            id: String::new(),
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
    // Pre-catalog files referenced doctrines by title.
    let agent = legacy["agents"][0].as_object_mut().unwrap();
    agent.remove("doctrine_ids");
    agent.insert("doctrines".into(), serde_json::json!(["domain", "code"]));
    std::fs::write(
        root.path().join("config/settings.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let resolved = store.load_settings().unwrap();
    assert_eq!(resolved.doctrines.len(), 10);
    let titles: Vec<_> = resolved.agents[0]
        .doctrine_ids
        .iter()
        .map(|id| {
            &resolved
                .doctrines
                .iter()
                .find(|d| &d.id == id)
                .unwrap()
                .title
        })
        .collect();
    assert_eq!(titles, ["code"]);
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

fn packet_result(exit: serde_json::Value) -> crate::review::ReviewResult {
    serde_json::from_value(json!({"reviewed_base_sha":"b".repeat(40),"output":{"exit":exit},
        "session_id":"s","model":"m","runtime_version":"v","input_tokens":1,"output_tokens":1,"tool_calls":1}))
    .unwrap()
}

/// A dispatched Packet-mode run whose packet record carries the state hash it read.
fn running_packet(store: &Store, template: &ReviewRun, now: i64) -> ReviewRun {
    request(store, &template.key, true, now).unwrap();
    prepare_dispatch(store, &template.key, now).unwrap();
    let mut reviews = store.load_reviews().unwrap();
    let run = &mut reviews[0];
    run.mode = crate::review::RunMode::Packet;
    run.packet = Some(crate::packet::PacketRecord {
        state_id: "0:hash".into(),
        hash: "h".into(),
        schema: 1,
        tool_set: vec![],
        exits_offered: vec![],
        exits_denied: vec![],
        state_hash: Some("hash-read".into()),
        state_marks: None,
    });
    let saved = run.clone();
    store.save_reviews(&reviews).unwrap();
    saved
}

#[test]
fn superseded_never_spends_the_retry_budget_or_parks() {
    let (_root, store, template) = fixture();
    let mut now = 100;
    for _ in 0..6 {
        let run = running_packet(&store, &template, now);
        complete(
            &store,
            &run.operation.id,
            Err(Failure::superseded("The PR head moved.")),
            now + 1,
        )
        .unwrap();
        let op = store.load_reviews().unwrap().remove(0).operation;
        assert_eq!(op.state, OperationState::Queued);
        assert_eq!(op.attempt_count, 0);
        assert!(op.failure.is_none());
        assert!(op.next_attempt_at.unwrap() > now + 1);
        now = op.next_attempt_at.unwrap();
    }
}

#[test]
fn ready_for_human_consumes_the_state_it_read_and_keeps_the_summary_local() {
    let (_root, store, template) = fixture();
    let run = running_packet(&store, &template, 100);
    complete(
        &store,
        &run.operation.id,
        Ok(packet_result(
            json!({"exit":"ready_for_human","summary":"Nothing to post."}),
        )),
        101,
    )
    .unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert_eq!(saved.operation.state, OperationState::Completed);
    let record = store
        .load_pr_states()
        .unwrap()
        .get(&crate::retention::Binding::job(&run.job))
        .cloned()
        .unwrap();
    assert_eq!(record.baseline_hash.as_deref(), Some("hash-read"));
    assert_eq!(record.local_summary.as_deref(), Some("Nothing to post."));
    assert_eq!(record.exit.unwrap().baseline_hash, "hash-read");
}

#[test]
fn a_state_that_moved_during_the_run_tosses_and_requeues_until_it_parks_at_three() {
    let (_root, store, template) = fixture();
    let binding = crate::retention::Binding::job(&template.job);
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "i",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 1,
        },
    );
    store.save_pr_states(&states).unwrap();
    let mut now = 100;
    for toss in 1..=3_u8 {
        let run = running_packet(&store, &template, now);
        complete(
            &store,
            &run.operation.id,
            Ok(packet_result(
                json!({"exit":"ready_for_human","summary":"Stale."}),
            )),
            now + 1,
        )
        .unwrap();
        let saved = store.load_reviews().unwrap().remove(0);
        assert!(saved.result.is_none(), "a tossed result is never stored");
        let record = store
            .load_pr_states()
            .unwrap()
            .get(&binding)
            .cloned()
            .unwrap();
        assert_eq!(record.error_count, toss);
        assert!(record.exit.is_none() && record.local_summary.is_none());
        if toss < 3 {
            assert_eq!(saved.operation.state, OperationState::Queued);
            now = saved.operation.next_attempt_at.unwrap();
        } else {
            assert_eq!(saved.operation.state, OperationState::ManualRetry);
        }
    }
}

#[test]
fn an_exit_time_toss_counts_and_a_failed_run_never_consumes() {
    let (_root, store, template) = fixture();
    let binding = crate::retention::Binding::job(&template.job);
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "i",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 1,
        },
    );
    store.save_pr_states(&states).unwrap();
    let run = running_packet(&store, &template, 100);
    complete(
        &store,
        &run.operation.id,
        Err(Failure::tossed("PR state moved.")),
        101,
    )
    .unwrap();
    let binding = crate::retention::Binding::job(&run.job);
    let states = store.load_pr_states().unwrap();
    assert_eq!(states.get(&binding).unwrap().error_count, 1);
    assert!(states.get(&binding).unwrap().baseline_hash.is_none());
}

/// Fixture whose queue job carries normal work at `state_seq` 1 and a PR state at seq 1.
fn packet_work_fixture() -> (tempfile::TempDir, Store, ReviewRun) {
    let (root, store, mut template) = fixture();
    let mut job = template.job.clone();
    job.work = Some(
        serde_json::from_value(
            json!({"id":"normal-1","item_id":"item-1","iteration_id":"i",
            "iteration":1,"agent_id":template.selection.agent.id,"enqueue_order":1,"pass_ordinal":1,
            "trigger":"admission","state_seq":1,
            "admission":{"watched_author":true,"all_authors":false,"requested_reviewer":false}}),
        )
        .unwrap(),
    );
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    template.key = key(&job, &template.assignment_id);
    template.job = job;
    let mut states = store.load_pr_states().unwrap();
    bump_state(&mut states, &template.job);
    store.save_pr_states(&states).unwrap();
    (root, store, template)
}

fn bump_state(states: &mut crate::pr_state::PrStates, job: &QueueJob) {
    states.observe(
        &crate::retention::Binding::job(job),
        crate::pr_state::Observation {
            iteration_id: "i",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 1,
        },
    );
}

#[test]
fn a_reused_run_reads_the_heartbeat_state_seq_so_its_exit_consumes() {
    let (_root, store, template) = packet_work_fixture();
    let first = running_packet(&store, &template, 100);
    assert_eq!(crate::exits::consume::state_seq(&first.job), 1);
    // Heartbeat: the PR moved to seq 2 while the run was in flight.
    let mut states = store.load_pr_states().unwrap();
    bump_state(&mut states, &template.job);
    store.save_pr_states(&states).unwrap();
    let mut queue = store.load_queue().unwrap();
    queue[0].work.as_mut().unwrap().state_seq = 2;
    store.save_queue(&queue).unwrap();
    complete(
        &store,
        &first.operation.id,
        Err(Failure::tossed("PR state moved.")),
        101,
    )
    .unwrap();
    let requeued = store.load_reviews().unwrap().remove(0);
    assert_eq!(requeued.operation.state, OperationState::Queued);
    let now = requeued.operation.next_attempt_at.unwrap();
    // The retry reads the fresh state (seq 2) and its exit consumes it.
    let retry = running_packet(&store, &template, now);
    assert_eq!(crate::exits::consume::state_seq(&retry.job), 2);
    complete(
        &store,
        &retry.operation.id,
        Ok(packet_result(
            json!({"exit":"ready_for_human","summary":"Fresh."}),
        )),
        now + 1,
    )
    .unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert_eq!(saved.operation.state, OperationState::Completed);
    let record = store
        .load_pr_states()
        .unwrap()
        .get(&crate::retention::Binding::job(&template.job))
        .cloned()
        .unwrap();
    assert_eq!(record.exit.unwrap().state_seq, 2);
    assert!(record.pending.is_none(), "no state left unreviewed");
}
