use pr_sniper_lib::{
    exit_writes,
    follow_up::{self, FollowUp},
    github::{metadata::Lifecycle, ConnectionError},
    monitoring::{
        AccountAvailability, JobOperation, Monitor, OperationFailure, OperationState, QueueJob,
    },
    pr_state::PrStates,
    publication::{self, Batch, Publication, Receipt, RemoteState},
    queue::{self, State},
    retention::Binding,
    review::{self, Decision, ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::json;

const REPO: &str = "00000000-0000-4000-8000-000000000001";
const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "00000000-0000-4000-8000-000000000003";

fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"stored"}]
    })).unwrap()
}

fn review(number: u64) -> ReviewRun {
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"operator",
        "configuration_id":REPO,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":number.to_string(),"number":number,"title":"Change","head_sha":"a".repeat(40),"observed_base_sha":"b".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"human_start","detected_at":100
    })).unwrap();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    ReviewRun {
        failed_duck: None,
        attempt_ducks: Vec::new(),
        mode: pr_sniper_lib::review::RunMode::Packet,
        packet: None,
        feedback_context: None,
        key: review::key(&job, ASSIGNMENT),
        selection: Selection::resolve(&settings(), &job, ASSIGNMENT).unwrap().into(),
        job, assignment_id: ASSIGNMENT.into(), operation,
        manual_start: true, trust_confirmed: true, phase: "Completed".into(), error: None,
        result: Some(serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),"output":{"synopsis":"The change looks good.",
                "files":[{"path":"src/space & <tag>.rs","explanation":"Read this file.","order":1}],
                "findings":[],"decision":"machine_sign_off"},
            "session_id":"session","model":"model","runtime_version":"fixture",
            "input_tokens":1,"output_tokens":1,"tool_calls":1
        })).unwrap()),
    }
}

fn published(review: &ReviewRun) -> Publication {
    let mut publication = Publication::new(review.clone(), false, true, 100).unwrap();
    publication.phase = publication::Phase::Published;
    publication.operation.state = OperationState::Completed;
    publication.batch = Some(Batch {
        commit_id: review.job.head_sha.clone(),
        body: "Review".into(),
        comments: vec![],
        unmappable: vec![],
    });
    publication.receipts = vec![Receipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec![],
    }];
    publication
}

fn seed(store: &Store, reviews: &[ReviewRun], publications: &[Publication]) {
    store.save_settings(&settings()).unwrap();
    store
        .save_queue(&reviews.iter().map(|r| r.job.clone()).collect::<Vec<_>>())
        .unwrap();
    store.save_reviews(reviews).unwrap();
    store.save_publications(publications).unwrap();
}

fn state(store: &Store) -> State {
    queue::snapshot(store, vec![]).unwrap().items[0].state
}

#[test]
fn accumulated_history_snapshot_preserves_all_items() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let reviews: Vec<_> = (1..=64)
        .map(|number| {
            let mut run = review(number);
            run.selection.agent.prompt = "Synthetic review evidence. ".repeat(2500);
            run
        })
        .collect();
    let publications: Vec<_> = reviews.iter().map(published).collect();
    seed(&store, &reviews, &publications);
    let start = std::time::Instant::now();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    println!("64-job accumulated-history snapshot: {:?}", start.elapsed());
    assert_eq!(snapshot.jobs.len(), 64);
    assert_eq!(snapshot.items.len(), 64);
    assert!(snapshot
        .items
        .iter()
        .all(|item| item.state == State::MachineSignedOff));
}

#[test]
fn snapshot_feedback_matches_single_job_views_and_reads_fresh_evidence() {
    use pr_sniper_lib::feedback::{self, Record};

    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let reviews = vec![review(1), review(2)];
    let publications: Vec<_> = reviews
        .iter()
        .map(|run| {
            let mut origin = published(run);
            origin.receipts[0].comment_ids = vec![format!("root-{}", run.job.number)];
            origin
        })
        .collect();
    seed(&store, &reviews, &publications);
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    for run in &reviews {
        let projected = &snapshot.feedback[&queue::item_id(&run.job)];
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].state, "unavailable");
        assert_eq!(
            serde_json::to_value(projected).unwrap(),
            serde_json::to_value(feedback::views(&store, &run.job).unwrap()).unwrap()
        );
    }

    let mut context = snapshot.feedback[&queue::item_id(&reviews[0].job)][0]
        .context
        .clone();
    context.unavailable = None;
    context.closed = true;
    let mut ledger = store.load_feedback().unwrap();
    ledger.records.push(Record {
        context,
        job: reviews[0].job.clone(),
        observed_head: reviews[0].job.head_sha.clone(),
    });
    store.save_feedback(&ledger).unwrap();
    let fresh = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(
        fresh.feedback[&queue::item_id(&reviews[0].job)][0].state,
        "closed"
    );
    assert_eq!(
        fresh.feedback[&queue::item_id(&reviews[1].job)][0].state,
        "unavailable"
    );

    std::fs::write(root.path().join("state/reviews.json"), b"invalid").unwrap();
    assert!(queue::snapshot(&store, vec![]).is_err());
}

fn with_exit(exit: pr_sniper_lib::exits::ExitOutcome) -> ReviewRun {
    let mut run = review(1);
    let output = &mut run.result.as_mut().unwrap().output;
    output.decision = Decision::HumanInputRequired;
    output.exit = Some(exit);
    run
}

#[test]
fn packet_exits_decide_queue_state_without_a_publication_unless_they_write_comments() {
    use pr_sniper_lib::exits::{ExitOutcome, NewComment};
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let ready = with_exit(ExitOutcome::ReadyForHuman {
        summary: "Looks fine.".into(),
    });
    seed(&store, &[ready], &[]);
    assert_eq!(state(&store), State::MachineSignedOff);
    let snapshot = serde_json::to_value(queue::snapshot(&store, vec![]).unwrap()).unwrap();
    assert_eq!(snapshot["publications"][0]["local_only"], true);

    let comments = with_exit(ExitOutcome::Comments {
        new_comments: vec![NewComment {
            path: "a.rs".into(),
            line: 1,
            side: None,
            body: "Fix.".into(),
        }],
        body: None,
        reply_comments: vec![],
        resolve_own_threads: vec![],
    });
    seed(&store, &[comments], &[]);
    assert_eq!(state(&store), State::AwaitingPublication);

    let replies_only = with_exit(ExitOutcome::Comments {
        new_comments: vec![],
        body: None,
        reply_comments: vec![pr_sniper_lib::exits::ReplyComment {
            thread_id: "T".into(),
            body: "Agreed.".into(),
        }],
        resolve_own_threads: vec![],
    });
    seed(&store, &[replies_only], &[]);
    assert_eq!(state(&store), State::AwaitingPublication);

    for exit in [
        ExitOutcome::Approve {
            comment: "LGTM".into(),
        },
        ExitOutcome::ApproveAndMerge {
            comment: "LGTM".into(),
        },
    ] {
        seed(&store, &[with_exit(exit)], &[]);
        assert_eq!(state(&store), State::MachineSignedOff);
        let item = &queue::snapshot(&store, vec![]).unwrap().items[0];
        assert_eq!(item.progress, Some(queue::Progress::Approving));
    }
}

fn replies_only() -> ReviewRun {
    with_exit(pr_sniper_lib::exits::ExitOutcome::Comments {
        new_comments: vec![],
        body: None,
        reply_comments: vec![pr_sniper_lib::exits::ReplyComment {
            thread_id: "T".into(),
            body: "Agreed.".into(),
        }],
        resolve_own_threads: vec!["R".into()],
    })
}

fn with_write(store: &Store, run: &ReviewRun, change: impl Fn(&mut exit_writes::ExitWrite)) {
    let mut write = exit_writes::plan(run, 100).unwrap();
    change(&mut write);
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes = vec![write];
    store.save_feedback(&ledger).unwrap();
}

fn tosses(store: &Store, run: &ReviewRun, count: u8) {
    let mut states = PrStates::default();
    states.observe(
        &Binding::job(&run.job),
        pr_sniper_lib::pr_state::Observation {
            iteration_id: "i",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 1,
        },
    );
    for _ in 0..count {
        states.toss(&Binding::job(&run.job));
    }
    store.save_pr_states(&states).unwrap();
}

fn item(store: &Store) -> queue::Item {
    queue::snapshot(store, vec![]).unwrap().items.remove(0)
}

#[test]
fn a_reply_only_exit_publishes_then_leaves_the_queue_once_consumed() {
    use exit_writes::Phase;
    use pr_sniper_lib::exits::Exit;
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = replies_only();
    seed(&store, std::slice::from_ref(&run), &[]);
    // Not admitted yet, planned, verified, confirmed but not consumed: publishing.
    let admitted = item(&store);
    assert_eq!(admitted.state, State::AwaitingPublication);
    assert_eq!(admitted.progress, Some(queue::Progress::Publishing));
    assert_eq!(admitted.exit, Some(Exit::LeaveComments));
    for phase in [Phase::Planned, Phase::Verified, Phase::Completed] {
        with_write(&store, &run, |w| w.phase = phase);
        let publishing = item(&store);
        assert_eq!(publishing.state, State::AwaitingPublication, "{phase:?}");
        assert_eq!(publishing.progress, Some(queue::Progress::Publishing));
    }
    with_write(&store, &run, |w| {
        w.phase = Phase::Completed;
        w.consumed = true;
    });
    let done = item(&store);
    assert_eq!(done.state, State::WaitingForAuthor);
    assert_eq!(done.progress, None);
    let snapshot = serde_json::to_value(queue::snapshot(&store, vec![]).unwrap()).unwrap();
    assert_eq!(snapshot["exit_writes"][0]["phase"], "completed");
    assert_eq!(snapshot["items"][0]["exit"], "leave_comments");
}

#[test]
fn a_tossed_exit_write_is_requeued_until_the_budget_parks_it_as_tossed() {
    use exit_writes::Phase;
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = replies_only();
    seed(&store, std::slice::from_ref(&run), &[]);
    tosses(&store, &run, 0);
    with_write(&store, &run, |w| w.phase = Phase::Tossed);
    // The toss counts and queues a new run of the same review.
    let write = store.load_feedback().unwrap().exit_writes.remove(0);
    exit_writes::requeue(&store, &write, 200).unwrap();
    assert_eq!(store.load_reviews().unwrap().len(), 2);
    let requeued = item(&store);
    assert_eq!(requeued.state, State::Queued);
    assert_eq!(requeued.failure_cause, None);
    // Only the newest run of a key re-queues.
    exit_writes::requeue(&store, &write, 201).unwrap();
    assert_eq!(store.load_reviews().unwrap().len(), 2);

    // Budget spent: one Needs attention row, typed `tossed`, no automatic retry.
    seed(&store, std::slice::from_ref(&run), &[]);
    tosses(&store, &run, 3);
    with_write(&store, &run, |w| w.phase = Phase::Tossed);
    let parked = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(parked.items.len(), 1);
    assert_eq!(parked.items[0].state, State::Failed);
    assert_eq!(parked.items[0].failure_cause, Some(queue::CAUSE_TOSSED));
    assert_eq!(parked.items[0].resume_at, None);
    // Under budget but not yet re-queued: shown as queued again.
    tosses(&store, &run, 1);
    assert_eq!(item(&store).state, State::Queued);
}

#[test]
fn an_exit_write_that_failed_after_an_effect_needs_attention_typed() {
    use exit_writes::Phase;
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = replies_only();
    seed(&store, std::slice::from_ref(&run), &[]);
    with_write(&store, &run, |w| {
        w.phase = Phase::Failed;
        w.steps[0].state = exit_writes::StepState::Done;
        w.operation.state = OperationState::ManualRetry;
        w.operation.failure = Some(OperationFailure::Permanent);
    });
    let failed = item(&store);
    assert_eq!(failed.state, State::Failed);
    assert_eq!(failed.failure_cause, Some(queue::CAUSE_EXIT_FAILED));
    assert_eq!(
        failed.failure_class,
        Some(pr_sniper_lib::monitoring::FailureClass::Other)
    );
    assert_eq!(failed.progress, None);
}

#[test]
fn a_review_tossed_through_its_budget_parks_typed_tossed() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut run = review(1);
    run.result = None;
    run.operation.state = OperationState::Running;
    run.operation.fail(
        &review::Failure::tossed("PR state moved.").monitoring(),
        1_000,
    );
    run.operation.state = OperationState::ManualRetry;
    seed(&store, &[run], &[]);
    let parked = item(&store);
    assert_eq!(parked.state, State::Failed);
    assert_eq!(parked.failure_cause, Some(queue::CAUSE_TOSSED));
}

#[test]
fn a_panic_exit_parks_at_once_typed_and_resumes_on_the_cooldown_schedule() {
    let mut run = review(1);
    run.result = None;
    run.operation.state = OperationState::Running;
    run.operation
        .fail(&review::Failure::panic("tool broken").monitoring(), 1_000);
    assert_eq!(run.operation.state, OperationState::Failed);
    assert_eq!(
        run.operation.failure,
        Some(pr_sniper_lib::monitoring::OperationFailure::Panic)
    );
    assert!(run.operation.resume_at.is_some());
    run.error = Some("Agent panic exit: tool broken".into());
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    seed(&store, &[run], &[]);
    assert_eq!(state(&store), State::Failed);
    let parked = item(&store);
    assert_eq!(parked.failure_cause, Some(queue::CAUSE_PANIC));
    assert_eq!(parked.exit, Some(pr_sniper_lib::exits::Exit::PanicExit));
    assert!(parked.resume_at.is_some(), "the cooldown clock");
}

#[test]
fn legacy_run_records_load_as_legacy_with_no_packet_and_round_trip() {
    let run = review(1);
    let mut value = serde_json::to_value(&run).unwrap();
    value.as_object_mut().unwrap().remove("mode");
    value.as_object_mut().unwrap().remove("packet");
    let loaded: ReviewRun = serde_json::from_value(value).unwrap();
    assert_eq!(loaded.mode, review::RunMode::Legacy);
    assert!(loaded.packet.is_none());
    assert!(serde_json::to_value(&loaded)
        .unwrap()
        .get("packet")
        .is_none());
}

#[test]
fn local_sign_off_never_hides_uncertain_rejected_or_unverified_publication() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    let baseline = published(&run);
    for (operation, phase, uncertain, error, expected) in [
        (
            OperationState::ManualRetry,
            publication::Phase::Unresolved,
            true,
            Some("Lost response"),
            State::Failed,
        ),
        (
            OperationState::Failed,
            publication::Phase::Stopped,
            false,
            Some("Rejected"),
            State::Failed,
        ),
        (
            OperationState::Failed,
            publication::Phase::Published,
            false,
            Some("Post-publication verification failed"),
            State::Failed,
        ),
        (
            OperationState::Running,
            publication::Phase::Pending,
            false,
            None,
            State::AwaitingPublication,
        ),
        (
            OperationState::Completed,
            publication::Phase::StaleAfterPublication,
            false,
            Some("Head changed"),
            State::StaleAfterPublication,
        ),
    ] {
        let mut publication = baseline.clone();
        publication.operation.state = operation;
        publication.phase = phase;
        publication.uncertain = uncertain;
        publication.error = error.map(String::from);
        seed(&store, std::slice::from_ref(&run), &[publication]);
        assert_eq!(state(&store), expected);
    }
}

#[test]
fn ready_requires_all_current_assignments_and_survives_configuration_changes_but_not_a_new_head() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    seed(&store, std::slice::from_ref(&run), &[published(&run)]);
    let mut configuration = settings();
    let mut assignment = configuration.repositories[0].assignments[0].clone();
    assignment.id = "00000000-0000-4000-8000-000000000004".into();
    configuration.repositories[0]
        .assignments
        .push(assignment.clone());
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::Queued);
    let mut second = run.job.clone();
    second.assignment_id = Some(assignment.id.clone());
    store
        .save_queue(&[run.job.clone(), second.clone()])
        .unwrap();
    assert_eq!(state(&store), State::Queued);
    let mut second_run = run.clone();
    second_run.job = second;
    second_run.assignment_id = assignment.id;
    second_run.key = review::key(&second_run.job, &second_run.assignment_id);
    second_run.operation = JobOperation::review(&second_run.job, 100);
    second_run.operation.state = OperationState::Completed;
    store
        .save_reviews(&[run.clone(), second_run.clone()])
        .unwrap();
    store
        .save_publications(&[published(&run), published(&second_run)])
        .unwrap();
    assert_eq!(
        state(&store),
        State::MachineSignedOff,
        "{}",
        serde_json::to_string_pretty(&queue::snapshot(&store, vec![]).unwrap()).unwrap()
    );
    // Finished work stays finished through configuration drift.
    configuration.agents[0].prompt = "A different lens.".into();
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::MachineSignedOff);
    configuration.repositories[0].provider_account_id = Some("44".into());
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::MachineSignedOff);
    // Only a new head or base revision makes it stale.
    let mut old = run.job.clone();
    old.waiting = "superseded".into();
    let mut old_second = second_run.job.clone();
    old_second.waiting = "superseded".into();
    let mut next = run.job.clone();
    next.head_sha = "d".repeat(40);
    store.save_queue(&[old, old_second, next]).unwrap();
    let after = queue::snapshot(&store, vec![]).unwrap();
    assert!(after
        .items
        .iter()
        .any(|i| i.state == State::StaleAfterPublication || i.state == State::Stale));
}

#[test]
fn polling_and_restart_keep_exact_destinations_without_retargeting_old_heads() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    seed(&store, std::slice::from_ref(&run), &[published(&run)]);
    let before = queue::snapshot(&store, vec![]).unwrap();
    let id = before.items[0].id.clone();
    queue::select(&store, Some(&id)).unwrap();
    let restored = Store::new(root.path().into());
    assert_eq!(
        restored.load_queue_selection().unwrap().as_deref(),
        Some(id.as_str())
    );
    assert_eq!(queue::snapshot(&restored, vec![]).unwrap().items[0].id, id);
    assert_eq!(
        queue::destination(&restored, &id, None).unwrap().as_str(),
        "https://github.com/example/repo/pull/1"
    );
    let file = queue::destination(&restored, &id, Some("src/space & <tag>.rs")).unwrap();
    assert_eq!(file.path(), "/example/repo/pull/1/files");
    assert_eq!(file.fragment().unwrap().len(), 69);
    assert!(queue::destination(&restored, &id, Some("https://evil.invalid")).is_err());
    assert!(queue::destination(&restored, "missing", None).is_err());
    assert!(queue::select(&restored, Some("missing")).is_err());
    assert_eq!(
        restored.load_queue_selection().unwrap().as_deref(),
        Some(id.as_str())
    );
    let mut old = run.job.clone();
    old.waiting = "superseded".into();
    let mut next = run.job.clone();
    next.head_sha = "c".repeat(40);
    store.save_queue(&[old, next]).unwrap();
    let after = queue::snapshot(&restored, vec![]).unwrap();
    assert_eq!(after.items.len(), 2);
    assert_eq!(
        after.items.iter().find(|i| i.id == id).unwrap().state,
        State::StaleAfterPublication
    );
    assert!(after
        .items
        .iter()
        .all(|i| i.state != State::MachineSignedOff));
    assert_ne!(after.items.iter().find(|i| i.id != id).unwrap().id, id);
    let mut other_account = run.job.clone();
    other_account.account_id = "44".into();
    assert_ne!(queue::item_id(&other_account), id);
    other_account = run.job.clone();
    other_account.configuration_id = "other-configuration".into();
    assert_ne!(queue::item_id(&other_account), id);
}

#[test]
fn sorted_attention_precedes_routine_work_and_monitoring_failures_prevent_ready() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let ready = review(9);
    let mut author = review(1);
    author.result.as_mut().unwrap().output.decision = Decision::HumanInputRequired;
    seed(
        &store,
        &[author.clone(), ready.clone()],
        &[published(&author), published(&ready)],
    );
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].job.number, 9);
    assert_eq!(snapshot.items[1].state, State::WaitingForAuthor);
    let health = serde_json::from_value(json!({
        "repository_id":REPO,"name":"example/repo","schedule_key":"interval:5:UTC",
        "provider_account_id":"22","enabled":true,"last_attempt":100,"last_success":90,
        "next_run":120,"schedule_available":true,"last_failure":"network","in_flight":false
    }))
    .unwrap();
    assert!(queue::snapshot(&store, vec![health])
        .unwrap()
        .items
        .iter()
        .all(|i| i.state == State::Failed));
}

#[test]
fn thread_judgment_and_reply_failures_remain_operator_work_not_author_waits() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    let origin = published(&run);
    seed(
        &store,
        std::slice::from_ref(&run),
        std::slice::from_ref(&origin),
    );
    let mut follow: FollowUp = follow_up::decode_runs(&serde_json::to_vec(&json!([{
        "id":"follow-1","key":"key","publication_id":origin.id,"review":run,
        "thread":{"id":"thread-1","resolved":false,"can_reply":true,"comments":[]},
        "trigger_id":"99","phase":"human_input_required","analysis":null,"publication":null,"history":[],
        "manual_start":true,"confirmed":false,"automatic_publication":false,"cancelled":false,
        "error":null,"result":null,"body":null,"uncertain":false,"receipt":null
    }])).unwrap()).unwrap().remove(0);
    store.save_follow_ups(&[follow.clone()]).unwrap();
    assert_eq!(state(&store), State::WaitingForHuman);
    follow.phase = follow_up::Phase::Unresolved;
    follow.uncertain = true;
    store.save_follow_ups(&[follow.clone()]).unwrap();
    assert_eq!(state(&store), State::Failed);
    follow.phase = follow_up::Phase::StaleAfterPublication;
    follow.uncertain = false;
    follow.receipt = Some("reply-1".into());
    store.save_follow_ups(&[follow]).unwrap();
    assert_eq!(state(&store), State::StaleAfterPublication);
}

#[test]
fn complete_guide_order_is_preserved_and_missing_storage_is_not_success() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut run = review(1);
    let paths: Vec<_> = (0..301).map(|n| format!("source-{n}.rs")).collect();
    let files: Vec<_> = paths
        .iter()
        .rev()
        .enumerate()
        .map(|(index, path)| json!({"path":path,"order":index + 1,"explanation":"Read it."}))
        .collect();
    let output: review::ReviewOutput = serde_json::from_value(json!({
        "synopsis":"Read every file.","decision":"machine_sign_off","findings":[],"files":files
    }))
    .unwrap();
    run.result.as_mut().unwrap().output = output.clone();
    seed(&store, &[run.clone()], &[published(&run)]);
    let value = serde_json::to_value(queue::snapshot(&store, vec![]).unwrap()).unwrap();
    assert_eq!(
        value["reviews"][0]["run"]["result"]["output"]["files"],
        serde_json::to_value(output.files).unwrap()
    );
    std::fs::write(root.path().join("state/publications.json"), "invalid").unwrap();
    assert!(queue::snapshot(&store, vec![]).is_err());
}

#[test]
fn review_matching_requires_the_actual_assignment_without_changing_persisted_keys() {
    let run = review(1);
    assert!(run.matches_job(&run.job));
    let mut other = run.job.clone();
    other.assignment_id = None;
    assert!(!run.matches_job(&other));
    other.assignment_id = Some("other-assignment".into());
    assert_eq!(review::key(&other, &run.assignment_id), run.key);
    assert!(!run.matches_job(&other));
}

#[test]
fn automatic_queue_and_retry_states_are_distinct_from_handoff_and_survive_restoration() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut run = review(1);
    seed(&store, &[run.clone()], &[]);
    store.save_reviews(&[]).unwrap();
    assert_eq!(state(&store), State::Queued);
    let mut configuration = settings();
    configuration.defaults.automatic_agent_start = true;
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::Queued);
    let mut untrusted = run.job.clone();
    untrusted.waiting = "trust_confirmation".into();
    store.save_queue(&[untrusted]).unwrap();
    assert_eq!(state(&store), State::Queued);
    store.save_queue(&[run.job.clone()]).unwrap();
    run.result = None;
    run.operation.state = OperationState::Running;
    store.save_reviews(&[run]).unwrap();
    assert_eq!(state(&store), State::Reviewing);
    review::restore(&store).unwrap();
    assert_eq!(state(&store), State::Queued);
    let mut runs = store.load_reviews().unwrap();
    runs[0].operation.state = OperationState::ManualRetry;
    store.save_reviews(&runs).unwrap();
    assert_eq!(state(&store), State::Failed);
}

#[test]
fn base_only_movement_and_legacy_detections_cannot_claim_current_readiness() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    seed(&store, std::slice::from_ref(&run), &[published(&run)]);
    let id = queue::snapshot(&store, vec![]).unwrap().items[0].id.clone();
    let mut job = run.job.clone();
    job.observed_base_sha = None;
    store.save_queue(&[job.clone()]).unwrap();
    assert_eq!(state(&store), State::Queued);
    job.observed_base_sha = Some("c".repeat(40));
    store.save_queue(&[job]).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].id, id);
    assert_eq!(snapshot.items[0].state, State::StaleAfterPublication);
    assert!(snapshot.items[0]
        .warnings
        .iter()
        .any(|w| w.contains("target base changed")));
}

#[test]
fn queue_selection_is_profile_scoped_and_read_write_failures_are_visible() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let store = Store::new(first.path().into());
    let run = review(1);
    seed(&store, std::slice::from_ref(&run), &[published(&run)]);
    let id = queue::item_id(&run.job);
    queue::select(&store, Some(&id)).unwrap();
    assert_eq!(
        Store::new(second.path().into())
            .load_queue_selection()
            .unwrap(),
        None
    );
    let path = first.path().join("state/queue-selection.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    windows_permissions::assert_private(&path, false);
    std::fs::write(&path, "invalid").unwrap();
    assert!(store.load_queue_selection().is_err());
    queue::select(&store, None).unwrap();
    assert_eq!(store.load_queue_selection().unwrap(), None);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(queue::select(&store, Some(&id)).is_err());
}
#[cfg(windows)]
#[allow(dead_code)]
#[path = "support/windows_permissions.rs"]
mod windows_permissions;

fn tracked_open(run: &ReviewRun) -> serde_json::Value {
    json!({
        "terminal_observed": false, "provider": "github", "configuration_id": REPO,
        "account_id": "22", "repository_id": "100", "pull_request_id": run.job.pull_request_id,
        "number": run.job.number, "head_sha": run.job.head_sha, "lifecycle": "open",
        "iteration_id": "iteration", "item_id": queue::item_id(&run.job), "iteration": 1,
        "admission": {"watched_author": true, "all_authors": false, "requested_reviewer": false},
        "admitted_at": 100, "observed_at": 100
    })
}

fn disabled_with_tracked_pr() -> (tempfile::TempDir, Store, ReviewRun) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(49);
    seed(&store, std::slice::from_ref(&run), &[published(&run)]);
    let mut disabled = settings();
    disabled.repositories[0].enabled = false;
    store.save_settings(&disabled).unwrap();
    let mut queue = store.load_queue_state().unwrap();
    queue.tracked = vec![serde_json::from_value(tracked_open(&run)).unwrap()];
    store.save_queue_state(&queue).unwrap();
    (root, store, run)
}

fn connected() -> std::collections::BTreeMap<String, AccountAvailability> {
    [(
        "22".to_string(),
        AccountAvailability {
            login: "operator".into(),
            connected: true,
        },
    )]
    .into()
}

#[test]
fn offline_disabled_lifecycle_recovers_without_runnable_reviews_or_new_admission() {
    use pr_sniper_lib::{
        backpressure::Scope,
        connectivity::{due_scopes, record, Observation},
    };
    let (_root, store, run) = disabled_with_tracked_pr();
    store.save_settings(&settings()).unwrap();
    let account = Scope::github("22");
    record(&store, account.clone(), Observation::Unreachable, 1_000).unwrap();
    let mut disabled = settings();
    disabled.repositories[0].enabled = false;
    store.save_settings(&disabled).unwrap();
    let reviews = store.load_reviews().unwrap();
    assert!(reviews
        .iter()
        .all(|r| r.operation.state == OperationState::Completed));
    let before = store.load_queue_state().unwrap();
    let receipts = store.load_publications().unwrap();
    let mut monitor = Monitor::default();
    let blocked = monitor
        .lifecycle_tickets(&store, &connected(), 1_001)
        .unwrap();
    assert_eq!(blocked.len(), 1);
    assert!(store
        .load_connectivity()
        .unwrap()
        .offline(std::slice::from_ref(&account))
        .is_some());
    assert!(due_scopes(&store, &connected(), 1_029).unwrap().is_empty());
    assert_eq!(
        due_scopes(&store, &connected(), 1_030).unwrap(),
        vec![account.clone()]
    );
    record(&store, account.clone(), Observation::Reachable, 1_030).unwrap();
    assert!(store
        .load_connectivity()
        .unwrap()
        .offline(&[account])
        .is_none());
    assert_eq!(store.load_queue_state().unwrap(), before);
    // Scope discovery must not consume the lifecycle sweep or admit review work.
    let tickets = monitor
        .lifecycle_tickets(&store, &connected(), 1_601)
        .unwrap();
    assert_eq!(tickets.len(), 1);
    monitor
        .apply_lifecycle(
            &store,
            &tickets[0],
            Ok(vec![(run.job.pull_request_id.clone(), Lifecycle::Merged)]),
            1_602,
        )
        .unwrap();
    assert_eq!(state(&store), State::Merged);
    assert!(due_scopes(&store, &connected(), 1_603).unwrap().is_empty());
    assert_eq!(store.load_reviews().unwrap(), reviews);
    assert_eq!(store.load_publications().unwrap(), receipts);
    assert_eq!(
        store.load_queue_state().unwrap().jobs.len(),
        before.jobs.len()
    );
    assert!(!store.load_settings().unwrap().repositories[0].enabled);
}

#[test]
fn offline_lifecycle_probe_scopes_exclude_removed_unbound_and_wrong_accounts() {
    use pr_sniper_lib::{
        backpressure::Scope,
        connectivity::{due_scopes, record, Observation},
    };
    for case in [
        "removed",
        "unbound",
        "account_changed",
        "repository_changed",
        "account_removed",
        "disconnected",
    ] {
        let (_root, store, _) = disabled_with_tracked_pr();
        record(&store, Scope::github("22"), Observation::Unreachable, 1_000).unwrap();
        let mut settings = store.load_settings().unwrap();
        let mut accounts = connected();
        match case {
            "removed" => settings.repositories.clear(),
            "unbound" => {
                settings.repositories[0].provider_account_id = None;
                settings.repositories[0].provider_repository_id = None;
            }
            "account_changed" => {
                settings.repositories[0].provider_account_id = Some("44".into());
                accounts.insert(
                    "44".into(),
                    AccountAvailability {
                        login: "other".into(),
                        connected: true,
                    },
                );
            }
            "repository_changed" => {
                settings.repositories[0].provider_repository_id = Some("200".into())
            }
            "account_removed" => accounts.clear(),
            "disconnected" => accounts.get_mut("22").unwrap().connected = false,
            _ => unreachable!(),
        }
        store.save_settings(&settings).unwrap();
        assert!(
            due_scopes(&store, &accounts, 2_000).unwrap().is_empty(),
            "{case}"
        );
        assert!(
            Monitor::default()
                .lifecycle_tickets(&store, &accounts, 2_000)
                .unwrap()
                .is_empty(),
            "{case}"
        );
    }
}

#[test]
fn disabled_repository_still_observes_a_merge_and_leaves_ready_for_merged() {
    let (_root, store, run) = disabled_with_tracked_pr();
    assert_eq!(state(&store), State::MachineSignedOff);
    let mut monitor = Monitor::default();
    let tickets = monitor
        .lifecycle_tickets(&store, &connected(), 1_000)
        .unwrap();
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].tracked, vec![("49".to_string(), 49)]);
    // The sweep is throttled.
    assert!(monitor
        .lifecycle_tickets(&store, &connected(), 1_001)
        .unwrap()
        .is_empty());
    monitor
        .apply_lifecycle(
            &store,
            &tickets[0],
            Ok(vec![(run.job.pull_request_id.clone(), Lifecycle::Merged)]),
            1_000,
        )
        .unwrap();
    let queue_state = store.load_queue_state().unwrap();
    assert!(queue_state.tracked[0].terminal_observed);
    assert_eq!(queue_state.tracked[0].lifecycle, Lifecycle::Merged);
    assert_eq!(state(&store), State::Merged);
    // Terminal PRs are not swept again.
    let mut later = Monitor::default();
    assert!(later
        .lifecycle_tickets(&store, &connected(), 5_000)
        .unwrap()
        .is_empty());
}

#[test]
fn disabled_repository_provider_failure_stays_invisible_and_open_stays_open() {
    let (_root, store, run) = disabled_with_tracked_pr();
    let before = store.load_queue_state().unwrap();
    let mut monitor = Monitor::default();
    let ticket = monitor
        .lifecycle_tickets(&store, &connected(), 1_000)
        .unwrap()
        .remove(0);
    monitor
        .apply_lifecycle(&store, &ticket, Err(ConnectionError::Network), 1_000)
        .unwrap();
    monitor
        .apply_lifecycle(
            &store,
            &ticket,
            Ok(vec![(run.job.pull_request_id.clone(), Lifecycle::Open)]),
            1_000,
        )
        .unwrap();
    assert_eq!(store.load_queue_state().unwrap(), before);
    assert_eq!(state(&store), State::MachineSignedOff);
}

#[test]
fn lifecycle_sweep_skips_monitored_repositories_and_disconnected_accounts() {
    let (_root, store, _run) = disabled_with_tracked_pr();
    let mut monitor = Monitor::default();
    assert!(monitor
        .lifecycle_tickets(&store, &Default::default(), 1_000)
        .unwrap()
        .is_empty());
    store.save_settings(&settings()).unwrap();
    let mut monitor = Monitor::default();
    assert!(monitor
        .lifecycle_tickets(&store, &connected(), 1_000)
        .unwrap()
        .is_empty());
}

#[test]
fn discarded_batch_leaves_attention_and_the_review_decision_stands_even_after_a_lens_change() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut run = review(1);
    let mut batch = Publication::new(run.clone(), true, false, 100).unwrap();
    batch.phase = publication::Phase::Pending;
    batch.operation.state = OperationState::Failed;
    batch.error = Some("Rejected".into());
    seed(&store, std::slice::from_ref(&run), &[batch.clone()]);
    assert_eq!(state(&store), State::Failed);
    batch.phase = publication::Phase::Discarded;
    batch.operation.state = OperationState::Completed;
    batch.error = None;
    batch.receipts = vec![Receipt {
        review_id: "42".into(),
        state: RemoteState::Deleted,
        comment_ids: vec![],
    }];
    seed(&store, std::slice::from_ref(&run), &[batch.clone()]);
    assert_eq!(state(&store), State::MachineSignedOff);
    let mut configuration = settings();
    configuration.agents[0].prompt = "A different lens.".into();
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::MachineSignedOff);
    run.result.as_mut().unwrap().output.decision = Decision::HumanInputRequired;
    batch.review = run.clone();
    seed(&store, std::slice::from_ref(&run), &[batch]);
    assert_eq!(state(&store), State::WaitingForHuman);
}

#[test]
fn a_resumed_publication_that_completes_clears_attention() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    let mut batch = published(&run);
    batch.operation.state = OperationState::ManualRetry;
    batch.operation.failure = Some(pr_sniper_lib::monitoring::OperationFailure::Network);
    batch.phase = publication::Phase::Unresolved;
    batch.uncertain = true;
    batch.error = Some("Repository check failed: Timeout".into());
    seed(&store, std::slice::from_ref(&run), &[batch.clone()]);
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].state, State::Failed);
    assert_eq!(
        snapshot.items[0].failure_class,
        Some(pr_sniper_lib::monitoring::FailureClass::Network)
    );
    assert_eq!(
        snapshot.items[0].failure_cause,
        Some("publication_unresolved")
    );
    batch.uncertain = false;
    seed(&store, std::slice::from_ref(&run), &[batch.clone()]);
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].failure_cause,
        Some("publication_failed")
    );
    batch.uncertain = true;
    batch.operation.state = OperationState::Completed;
    batch.operation.failure = None;
    batch.phase = publication::Phase::Published;
    batch.uncertain = false;
    batch.error = None;
    seed(&store, std::slice::from_ref(&run), &[batch]);
    assert_eq!(state(&store), State::MachineSignedOff);
}

fn accounts() -> std::collections::BTreeMap<String, AccountAvailability> {
    ["22", "33"]
        .into_iter()
        .map(|id| {
            (
                id.to_string(),
                AccountAvailability {
                    login: id.into(),
                    connected: true,
                },
            )
        })
        .collect()
}

#[test]
fn transient_publication_failures_resume_on_the_cooldown_table_and_credentials_on_reconnect() {
    use pr_sniper_lib::monitoring::{OperationFailure, RESUME_COOLDOWNS_SECONDS};
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = review(1);
    let mut batch = published(&run);
    batch.operation.state = OperationState::ManualRetry;
    batch.operation.failure = Some(OperationFailure::Timeout);
    batch.operation.attempt_count = 3;
    batch.phase = publication::Phase::Unresolved;
    batch.uncertain = true;
    batch.error = Some("Repository check failed: Timeout".into());
    batch.receipts.clear();
    let at = 1_000;
    batch.operation.resume_at = Some(at + RESUME_COOLDOWNS_SECONDS[0]);
    seed(&store, std::slice::from_ref(&run), &[batch.clone()]);
    let item = queue::snapshot(&store, vec![]).unwrap().items.remove(0);
    assert_eq!(item.state, State::Failed);
    assert_eq!(item.resume_at, Some(at + RESUME_COOLDOWNS_SECONDS[0]));
    let mut monitor = Monitor::default();
    let due = at + RESUME_COOLDOWNS_SECONDS[0];
    assert_eq!(
        monitor.auto_resume(&store, &accounts(), due - 1).unwrap(),
        0
    );
    assert_eq!(monitor.auto_resume(&store, &accounts(), due).unwrap(), 1);
    let resumed = store.load_publications().unwrap().remove(0);
    assert_eq!(resumed.operation.state, OperationState::Queued);
    assert_eq!(resumed.operation.attempt_count, 0);
    assert_eq!(resumed.operation.resume_cycle, 1);
    assert_eq!(resumed.operation.next_attempt_at, Some(due));
    assert!(
        resumed.uncertain,
        "the original intent is kept for reconcile"
    );
    assert_eq!(resumed.id, batch.id);
    assert_eq!(resumed.error, None);
    // Fifth failure ends the cycles: no sixth automatic attempt.
    let mut spent = resumed;
    spent.operation.resume_cycle = RESUME_COOLDOWNS_SECONDS.len() as u8;
    spent.operation.state = OperationState::ManualRetry;
    spent.operation.resume_at = None;
    store.save_publications(&[spent]).unwrap();
    assert_eq!(
        monitor.auto_resume(&store, &accounts(), i64::MAX).unwrap(),
        0
    );
    // Permanent publication failures never resume.
    let mut permanent = batch;
    permanent.operation.state = OperationState::Failed;
    permanent.operation.failure = Some(OperationFailure::Permanent);
    permanent.operation.resume_at = None;
    store.save_publications(&[permanent]).unwrap();
    assert_eq!(
        monitor.auto_resume(&store, &accounts(), i64::MAX).unwrap(),
        0
    );
}

#[test]
fn a_content_tossed_batch_requeues_then_parks_as_tossed() {
    use pr_sniper_lib::exits::{ExitOutcome, NewComment};
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = with_exit(ExitOutcome::Comments {
        new_comments: vec![NewComment {
            path: "CHANGELOG.md".into(),
            line: 151,
            side: Some("head".into()),
            body: "Conflict marker.".into(),
        }],
        body: None,
        reply_comments: vec![],
        resolve_own_threads: vec![],
    });
    let mut tossed = Publication::new(run.clone(), true, false, 100).unwrap();
    tossed.phase = publication::Phase::Discarded;
    tossed.operation.state = OperationState::Completed;
    tossed.operation.failure = Some(OperationFailure::Tossed);
    assert!(tossed.tossed());
    seed(&store, std::slice::from_ref(&run), &[tossed.clone()]);
    tosses(&store, &run, 1);
    let queued = item(&store);
    assert_eq!(queued.state, State::Queued);
    assert_eq!(queued.failure_cause, None);
    tosses(&store, &run, 3);
    let parked = item(&store);
    assert_eq!(parked.state, State::Failed);
    assert_eq!(parked.failure_cause, Some(queue::CAUSE_TOSSED));
    // Retry restarts the toss budget and reviews the current state, never the batch.
    assert_eq!(
        pr_sniper_lib::exits::consume::retry_parked(&store, &parked.id, 300).unwrap(),
        1
    );
    let record = store.load_pr_states().unwrap();
    assert_eq!(record.get(&Binding::job(&run.job)).unwrap().error_count, 0);
    assert_eq!(store.load_publications().unwrap(), vec![tossed]);
}

#[test]
fn a_body_only_exit_awaits_its_publication() {
    use pr_sniper_lib::exits::ExitOutcome;
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let run = with_exit(ExitOutcome::Comments {
        new_comments: vec![],
        body: Some("Merge conflicts: rebase on main.".into()),
        reply_comments: vec![],
        resolve_own_threads: vec![],
    });
    seed(&store, std::slice::from_ref(&run), &[]);
    let waiting = item(&store);
    assert_eq!(waiting.state, State::AwaitingPublication);
    assert_eq!(waiting.progress, Some(queue::Progress::Publishing));
    assert!(
        exit_writes::plan(&run, 100).is_none(),
        "no replies: batch only"
    );
}
