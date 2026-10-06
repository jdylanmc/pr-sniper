use pr_sniper_lib::{
    follow_up::{self, FollowUp},
    monitoring::{JobOperation, OperationState, QueueJob},
    publication::{self, Batch, Publication, Receipt, RemoteState},
    queue::{self, State},
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
        feedback_context: None,
        key: review::key(&job, ASSIGNMENT),
        selection: Selection::resolve(&settings(), &job, ASSIGNMENT).unwrap(),
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
fn publication_is_a_separate_fact_and_the_author_is_not_the_operator() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut run = review(1);
    seed(&store, &[run.clone()], &[]);
    assert_eq!(state(&store), State::MachineSignedOff);
    let snapshot = serde_json::to_value(queue::snapshot(&store, vec![]).unwrap()).unwrap();
    assert_eq!(snapshot["publications"][0]["local_only"], false);
    assert_eq!(
        snapshot["publications"][0]["publication"],
        serde_json::Value::Null
    );
    let mut receipt = published(&run);
    store.save_publications(&[receipt.clone()]).unwrap();
    assert_eq!(state(&store), State::MachineSignedOff);
    run.result.as_mut().unwrap().output.decision = Decision::HumanInputRequired;
    receipt.review = run.clone();
    store.save_reviews(&[run.clone()]).unwrap();
    store.save_publications(&[receipt.clone()]).unwrap();
    assert_eq!(state(&store), State::WaitingForAuthor);
    receipt.batch.as_mut().unwrap().unmappable.push(0);
    store.save_publications(&[receipt]).unwrap();
    assert_eq!(state(&store), State::WaitingForHuman);
    store.save_publications(&[]).unwrap();
    let mut configuration = settings();
    configuration.repositories[0].assignments[0].comment = false;
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::WaitingForHuman);
    run.result.as_mut().unwrap().output.decision = Decision::MachineSignOff;
    store.save_reviews(&[run]).unwrap();
    assert_eq!(state(&store), State::MachineSignedOff);
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
fn ready_requires_all_current_assignments_and_does_not_survive_configuration_changes() {
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
    configuration.agents[0].prompt = "A different lens.".into();
    store.save_settings(&configuration).unwrap();
    assert_eq!(state(&store), State::Stale);
    configuration.repositories[0].provider_account_id = Some("44".into());
    store.save_settings(&configuration).unwrap();
    assert_ne!(state(&store), State::MachineSignedOff);
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
        .enumerate()
        .map(|(index, path)| json!({"path":path,"order":301-index,"explanation":"Read it."}))
        .collect();
    let output = review::validate_output(
        &json!({
            "synopsis":"Read every file.","decision":"machine_sign_off","findings":[],"files":files
        })
        .to_string(),
        &paths,
    )
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
