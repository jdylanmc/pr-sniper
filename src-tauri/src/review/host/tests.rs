use super::*;
use crate::storage::{Settings, Store};
use serde_json::json;

#[path = "owned_feedback_tests.rs"]
mod owned_feedback_tests;

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
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: key(&job, &assignment_id),
        assignment_id: assignment_id.clone(),
        selection: Selection::resolve(&settings, &job, &assignment_id)
            .unwrap()
            .into(),
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
    let actual = prepare_dispatch(&store, &template.key, 100)
        .unwrap()
        .unwrap();
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
    let next = prepare_dispatch(&store, &next_key, 200).unwrap().unwrap();
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
    prepare_dispatch(store, &template.key, now)
        .unwrap()
        .unwrap();
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
        wait_snapshot: None,
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
fn nothing_new_while_own_findings_still_apply_waits_for_the_author_not_a_handoff() {
    for (disposition, expected) in [
        ("still_applies", crate::queue::State::WaitingForAuthor),
        ("withdrawn", crate::queue::State::MachineSignedOff),
    ] {
        let (_root, store, template) = fixture();
        let run = running_packet(&store, &template, 100);
        let mut result =
            packet_result(json!({"exit":"ready_for_human","summary":"No new defect."}));
        result.finding_dispositions = vec![serde_json::from_value(json!({
            "thread_id":"T","disposition":disposition,"head_sha":"h","base_sha":"b",
            "rationale":"Unchanged.","evidence":[]
        }))
        .unwrap()];
        complete(&store, &run.operation.id, Ok(result), 101).unwrap();
        let mut queue = store.load_queue().unwrap();
        queue[0].observed_base_sha = Some("b".repeat(40));
        store.save_queue(&queue).unwrap();
        let snapshot = crate::queue::snapshot(&store, vec![]).unwrap();
        assert_eq!(snapshot.items[0].state, expected, "{disposition}");
    }
}

#[test]
fn a_legacy_body_only_result_shows_its_confirmed_publication() {
    let (_root, store, template) = fixture();
    let run = running_packet(&store, &template, 100);
    let result = packet_result(json!({"exit":"comments","new_comments":[],
        "body":"Neither finding is fixed.","reply_comments":[],"resolve_own_threads":[]}));
    complete(&store, &run.operation.id, Ok(result), 101).unwrap();
    let mut queue = store.load_queue().unwrap();
    queue[0].observed_base_sha = Some("b".repeat(40));
    store.save_queue(&queue).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    let mut publication = crate::publication::Publication::new(saved, true, false, 102).unwrap();
    publication.phase = crate::publication::Phase::Published;
    publication.operation.state = OperationState::Completed;
    publication.receipts.push(crate::publication::Receipt {
        review_id: "1".into(),
        state: crate::publication::RemoteState::Commented,
        comment_ids: vec![],
    });
    store.save_publications(&[publication]).unwrap();
    let state = crate::queue::snapshot(&store, vec![]).unwrap().items[0].state;
    assert_eq!(state, crate::queue::State::WaitingForAuthor);
}

#[test]
fn a_legacy_body_and_reply_result_waits_for_the_author_once_its_replies_land() {
    let (_root, store, template) = fixture();
    let run = running_packet(&store, &template, 100);
    let result = packet_result(json!({"exit":"comments","new_comments":[],
        "body":"Neither finding is fixed.","resolve_own_threads":[],
        "reply_comments":[{"thread_id":"T","body":"Yes, cleanup has the same gap."}]}));
    complete(&store, &run.operation.id, Ok(result), 101).unwrap();
    let mut queue = store.load_queue().unwrap();
    queue[0].observed_base_sha = Some("b".repeat(40));
    store.save_queue(&queue).unwrap();
    assert_eq!(crate::exit_writes::admit(&store, 102).unwrap(), 1);
    let mut ledger = store.load_feedback().unwrap();
    let write = &mut ledger.exit_writes[0];
    write.phase = crate::exit_writes::Phase::Completed;
    write.consumed = true;
    store.save_feedback(&ledger).unwrap();
    let state = crate::queue::snapshot(&store, vec![]).unwrap().items[0].state;
    assert_eq!(state, crate::queue::State::WaitingForAuthor);
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

/// The fixture with a Rubber Duck on its (sole, so primary) assignment.
fn duck_fixture(window: u32) -> (tempfile::TempDir, Store, ReviewRun) {
    let (root, store, mut run) = fixture();
    let mut settings = store.load_settings().unwrap();
    let duck_id = uuid::Uuid::new_v4().to_string();
    settings.agents.push(
        serde_json::from_value(json!({
            "id":duck_id,"name":"Duck","model":"duck-model",
            "ai_account":{"provider":"copilot","account_id":"44"},
            "prompt":"Check one claim.","signature":"duck"
        }))
        .unwrap(),
    );
    settings.repositories[0].assignments[0].rubber_duck = Some(duck_id);
    settings.rubber_duck.primary_window_minutes = window;
    store.save_settings(&settings).unwrap();
    run.selection = Selection::resolve(&settings, &run.job, &run.assignment_id)
        .unwrap()
        .into();
    (root, store, run)
}

fn duck_call() -> crate::review::duck::DuckCall {
    serde_json::from_value(json!({
        "ordinal":1,"input":{"kind":"pr","claim":"c","proposed_body":"b"},
        "outcome":{"outcome":"verdict","verdict":{"verdict":"confirmed","nano":"real"}},
        "limits":{"max_calls":8,"call_timeout_seconds":180,"primary_window_minutes":15},
        "agent":"Duck","model":"duck-model","state_id":"0:hash","duration_ms":5
    }))
    .unwrap()
}

#[test]
fn the_job_snapshot_captures_the_duck_and_a_duck_edit_changes_the_execution() {
    let (_root, store, run) = duck_fixture(15);
    let snapshot = run
        .selection
        .configuration
        .as_ref()
        .unwrap()
        .rubber_duck
        .clone()
        .unwrap();
    assert_eq!(snapshot.agent.name, "Duck");
    crate::review::validate_execution_selection(&store, &run).unwrap();
    let mut settings = store.load_settings().unwrap();
    let duck = settings
        .agents
        .iter_mut()
        .find(|a| a.name == "Duck")
        .unwrap();
    duck.prompt = "Changed duck prompt.".into();
    store.save_settings(&settings).unwrap();
    assert!(crate::review::validate_execution_selection(&store, &run).is_err());
    // The captured evidence is never rewritten by the edit.
    assert_eq!(
        run.selection
            .configuration
            .clone()
            .unwrap()
            .rubber_duck
            .unwrap(),
        snapshot
    );
}

#[test]
fn a_primary_with_a_duck_gets_the_preference_window_and_others_keep_the_default() {
    let (_root, store, template) = duck_fixture(60);
    request(&store, &template.key, true, 100).unwrap();
    let run = prepare_dispatch(&store, &template.key, 100)
        .unwrap()
        .unwrap();
    assert_eq!(run.operation.retry_deadline, 100 + 60 * 60);
    let (_root, store, template) = fixture();
    request(&store, &template.key, true, 100).unwrap();
    let run = prepare_dispatch(&store, &template.key, 100)
        .unwrap()
        .unwrap();
    assert_eq!(run.operation.retry_deadline, 100 + 15 * 60);
}

#[test]
fn a_duck_setup_failure_parks_through_the_normal_path_with_typed_provenance() {
    let (_root, store, template) = duck_fixture(15);
    let run = running_packet(&store, &template, 100);
    complete(
        &store,
        &run.operation.id,
        Err(Failure::permanent("The Rubber duck model is not available.").duck_setup()),
        101,
    )
    .unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert_eq!(saved.operation.state, OperationState::Failed);
    assert_eq!(saved.failed_duck.as_deref(), Some("Duck"));
    // Same message without the typed marker: no duck provenance is inferred.
    let (_root, store, template) = duck_fixture(15);
    let run = running_packet(&store, &template, 100);
    complete(
        &store,
        &run.operation.id,
        Err(Failure::permanent(
            "The Rubber duck model is not available.",
        )),
        101,
    )
    .unwrap();
    assert_eq!(store.load_reviews().unwrap().remove(0).failed_duck, None);
}

#[test]
fn a_disconnected_duck_account_parks_as_credentials() {
    let (_root, store, template) = duck_fixture(15);
    let run = running_packet(&store, &template, 100);
    let mut failure = Failure::permanent("Reconnect the Copilot account.").duck_setup();
    failure.kind = monitoring::OperationFailure::Credentials;
    complete(&store, &run.operation.id, Err(failure), 101).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert_eq!(
        saved.operation.failure,
        Some(monitoring::OperationFailure::Credentials)
    );
    assert_eq!(saved.failed_duck.as_deref(), Some("Duck"));
}

#[test]
fn duck_evidence_is_kept_with_a_consumed_result_and_never_outlives_a_moved_state() {
    let (_root, store, template) = duck_fixture(15);
    let run = running_packet(&store, &template, 100);
    let mut result = packet_result(json!({"exit":"ready_for_human","summary":"Checked."}));
    result.ducks = vec![duck_call()];
    complete(&store, &run.operation.id, Ok(result), 101).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert_eq!(saved.operation.state, OperationState::Completed);
    assert_eq!(saved.result.unwrap().ducks, vec![duck_call()]);
    assert_eq!(saved.failed_duck, None);

    // An exit-gate or post-run toss, and a state superseded before the run.
    for error in [
        Failure::tossed("PR state moved. Review discarded. Stop."),
        Failure::superseded("The PR head moved since the work was queued."),
    ] {
        let (_root, store, template) = duck_fixture(15);
        let run = running_packet(&store, &template, 100);
        record_attempt_ducks(&store, &run.operation.id, vec![duck_call()]).unwrap();
        complete(&store, &run.operation.id, Err(error), 101).unwrap();
        let saved = store.load_reviews().unwrap().remove(0);
        assert!(saved.result.is_none(), "verdicts of a moved state are gone");
        assert!(
            saved.attempt_ducks.is_empty(),
            "verdicts of a moved state are gone"
        );
    }

    // A result whose state moved by the time it is consumed.
    let (_root, store, template) = duck_fixture(15);
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
    record_attempt_ducks(&store, &run.operation.id, vec![duck_call()]).unwrap();
    let mut result = packet_result(json!({"exit":"ready_for_human","summary":"Stale."}));
    result.ducks = vec![duck_call()];
    complete(&store, &run.operation.id, Ok(result), 101).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert!(saved.result.is_none());
    assert!(saved.attempt_ducks.is_empty());
}

#[test]
fn a_failed_attempt_keeps_its_duck_calls_until_a_result_replaces_them() {
    let (_root, store, template) = duck_fixture(15);
    let run = running_packet(&store, &template, 100);
    record_attempt_ducks(&store, &run.operation.id, vec![duck_call()]).unwrap();
    complete(&store, &run.operation.id, Err(Failure::timeout()), 101).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert!(saved.result.is_none());
    assert_eq!(saved.attempt_ducks, vec![duck_call()]);
    // A later completed attempt carries its own calls and clears the failed ones.
    let mut reviews = store.load_reviews().unwrap();
    reviews[0].operation.state = OperationState::Running;
    store.save_reviews(&reviews).unwrap();
    let mut result = packet_result(json!({"exit":"ready_for_human","summary":"Checked."}));
    result.ducks = vec![duck_call()];
    complete(&store, &run.operation.id, Ok(result), 102).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert!(saved.attempt_ducks.is_empty());
    assert_eq!(saved.result.unwrap().ducks.len(), 1);
}
