use super::*;
use crate::{
    github::threads::Comment,
    monitoring::QueueJob,
    publication::{Batch, InlineComment, Receipt, RemoteState},
    review::ReviewRun,
    storage::{ActionPermissions, ResourceEdit, Settings},
};
use serde_json::json;

const REPOSITORY: &str = "00000000-0000-4000-8000-000000000001";
const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "00000000-0000-4000-8000-000000000003";
const SIBLING_ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000004";
const SIBLING_AGENT: &str = "00000000-0000-4000-8000-000000000005";

fn fixture() -> (tempfile::TempDir, Store, FollowUp) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{
            "id":REPOSITORY,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "primary_assignment_id":ASSIGNMENT,
            "assignments":[
                {"id":ASSIGNMENT,"agent_id":AGENT,"comment":true,
                 "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"}},
                {"id":SIBLING_ASSIGNMENT,"agent_id":SIBLING_AGENT,"comment":true,
                 "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"}}
            ]
        }],
        "agents":[
            {"id":AGENT,"name":"A","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
             "doctrines":["Correctness"],"prompt":"Review correctness.","signature":"stored"},
            {"id":SIBLING_AGENT,"name":"B","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
             "prompt":"Review boundaries.","signature":"stored"}
        ],
        "doctrines":[{"title":"Correctness","body":"Trace state transitions."}]
    }))
    .unwrap();
    settings.defaults.automatic_agent_start = true;
    store.save_settings(&settings).unwrap();
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":REPOSITORY,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"human_start","detected_at":100
    }))
    .unwrap();
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    let mut review = ReviewRun {
        feedback_context: None,
        key: crate::review::key(&job, ASSIGNMENT),
        assignment_id: ASSIGNMENT.into(),
        selection: Selection::resolve(&settings, &job, ASSIGNMENT).unwrap(),
        operation: JobOperation::review(&job, 100),
        job,
        manual_start: false,
        trust_confirmed: true,
        phase: "Complete".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({
                "reviewed_base_sha":"b".repeat(40),
                "output":{"synopsis":"Review completed.","files":[],"findings":[],"decision":"machine_sign_off"},
                "session_id":"original-review","model":"model","runtime_version":"fixture",
                "input_tokens":1,"output_tokens":1,"tool_calls":1
            }))
            .unwrap(),
        ),
    };
    review.operation.state = OperationState::Completed;
    store.save_reviews(std::slice::from_ref(&review)).unwrap();
    let mut origin = Publication::new(review, false, true, 100).unwrap();
    origin.batch = Some(Batch {
        commit_id: "a".repeat(40),
        body: "Summary".into(),
        comments: vec![InlineComment {
            path: "source.rs".into(),
            line: 1,
            side: "RIGHT".into(),
            body: "Root finding".into(),
        }],
        unmappable: vec![],
    });
    origin.receipts.push(Receipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["100".into()],
    });
    store
        .save_publications(std::slice::from_ref(&origin))
        .unwrap();
    let root_comment = Comment {
        id: "100".into(),
        body: "Root finding".into(),
        author_id: Some("22".into()),
        author_login: Some("owner".into()),
        reply_to: None,
        review_id: Some("42".into()),
        original_commit: Some("a".repeat(40)),
        created_at: "2026-09-30T00:00:00Z".into(),
        published_at: "2026-09-30T00:00:00Z".into(),
    };
    let reply = Comment {
        id: "101".into(),
        body: "Why is this required?".into(),
        author_id: Some("11".into()),
        author_login: Some("author".into()),
        reply_to: Some("100".into()),
        ..root_comment.clone()
    };
    let run = FollowUp::new(
        &origin,
        Thread {
            id: "owned-thread".into(),
            resolved: false,
            can_reply: true,
            comments: vec![root_comment, reply],
        },
    )
    .unwrap();
    store.save_follow_ups(std::slice::from_ref(&run)).unwrap();
    (root, store, run)
}

fn start(store: &Store, run: &mut FollowUp, manual: bool) {
    start_at(store, run, manual, 100);
}

fn start_at(store: &Store, run: &mut FollowUp, manual: bool, now: i64) {
    let candidate = candidates(store).unwrap().remove(0);
    assert!(candidate.blocked.is_none(), "{:?}", candidate.blocked);
    assert_eq!(candidate.run, *run);
    prepare_analysis(
        store,
        run,
        candidate.blocked,
        candidate.human_gate,
        manual,
        now,
    )
    .unwrap();
    save_to_store(store, run).unwrap();
}

fn activate(store: &Store, run: &mut FollowUp) {
    start(store, run, false);
    run.analysis.as_mut().unwrap().begin_attempt(100).unwrap();
    run.phase = Phase::Analyzing;
    save_to_store(store, run).unwrap();
}

fn change_sibling_comment(store: &Store, run: &FollowUp) {
    let settings = store.load_settings().unwrap();
    let mut repository = settings.repositories[0].clone();
    repository.assignments[1].comment = false;
    let saved = store
        .save_resource(ResourceEdit::Repository {
            id: REPOSITORY.into(),
            expected: Some(Box::new(settings.repositories[0].clone())),
            value: Some(Box::new(repository)),
        })
        .unwrap();
    let current = Selection::resolve(&saved, &run.context.job, ASSIGNMENT).unwrap();
    assert_ne!(current, run.context.selection);
    assert!(current.same_execution(&run.context.selection));
    assert!(current.configuration.as_ref().unwrap().authority.primary);
    assert!(
        run.context
            .selection
            .configuration
            .as_ref()
            .unwrap()
            .repository
            .assignments[1]
            .comment
    );
}

fn complete_analysis(run: &mut FollowUp) {
    run.result = Some(
        serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),
            "output":{"decision":"quiet","body":"","new_information":"","reason":"No new evidence.","evidence":[]},
            "session_id":"reply-analysis","model":"model","runtime_version":"fixture",
            "input_tokens":1,"output_tokens":1,"tool_calls":1
        }))
        .unwrap(),
    );
    run.phase = Phase::Quiet;
    run.analysis.as_mut().unwrap().state = OperationState::Completed;
}

#[test]
fn native_local_gate_accepts_sibling_comment_save_during_active_analysis() {
    let (_root, store, mut run) = fixture();
    activate(&store, &mut run);
    local_gate(&store, &run).unwrap();
    let original = run.clone();
    change_sibling_comment(&store, &run);
    assert_eq!(candidates(&store).unwrap()[0].run, original);
    local_gate(&store, &run).unwrap();
    assert_eq!(store.load_follow_ups().unwrap()[0], original);
}

#[test]
fn analysis_commit_accepts_sibling_comment_save_with_original_snapshot() {
    let (root, store, mut run) = fixture();
    activate(&store, &mut run);
    let selection = run.context.selection.clone();
    let review_bytes = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    let publication_bytes = std::fs::read(root.path().join("state/publications.json")).unwrap();
    complete_analysis(&mut run);
    super::super::validate_analysis_commit(&store, &run, true).unwrap();
    change_sibling_comment(&store, &run);
    super::super::validate_analysis_commit(&store, &run, true).unwrap();
    save_to_store(&store, &run).unwrap();
    let reopened = Store::new(root.path().into());
    assert_eq!(reopened.load_follow_ups().unwrap()[0], run);
    assert_eq!(run.context.selection, selection);
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        review_bytes
    );
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        publication_bytes
    );
}

#[test]
fn first_start_and_manual_retry_preserve_equivalent_captured_selection() {
    for retry in [false, true] {
        let (root, store, mut run) = fixture();
        if retry {
            activate(&store, &mut run);
            run.analysis
                .as_mut()
                .unwrap()
                .fail(&Failure::timeout().monitoring(), 1000);
            save_to_store(&store, &run).unwrap();
        }
        let selection = run.context.selection.clone();
        let original_review = run.owned().unwrap().review.clone();
        let previous = run.analysis.clone();
        change_sibling_comment(&store, &run);
        start_at(&store, &mut run, retry, if retry { 1001 } else { 100 });
        assert_eq!(run.context.selection, selection, "manual retry: {retry}");
        assert_eq!(store.load_reviews().unwrap()[0], original_review);
        assert_eq!(
            store.load_publications().unwrap()[0].review,
            original_review
        );
        assert_eq!(
            Store::new(root.path().into()).load_follow_ups().unwrap()[0],
            run
        );
        assert_eq!(run.history, previous.into_iter().collect::<Vec<_>>());
        local_gate(&store, &run).unwrap();
    }
}

#[test]
fn interrupted_resume_keeps_original_selection_through_tools_and_commit() {
    let (root, store, mut run) = fixture();
    activate(&store, &mut run);
    let original = run.clone();
    change_sibling_comment(&store, &run);
    let reopened = Store::new(root.path().into());
    super::super::restore(&reopened).unwrap();
    let mut resumed = reopened.load_follow_ups().unwrap().remove(0);
    assert_eq!(
        resumed.analysis.as_ref().unwrap().state,
        OperationState::Interrupted
    );
    assert_eq!(
        resumed.owned().unwrap().review,
        original.owned().unwrap().review
    );
    start(&reopened, &mut resumed, false);
    assert_eq!(
        resumed.analysis.as_ref().unwrap().id,
        original.analysis.as_ref().unwrap().id
    );
    assert_eq!(resumed.history, original.history);
    resumed
        .analysis
        .as_mut()
        .unwrap()
        .begin_attempt(102)
        .unwrap();
    save_to_store(&reopened, &resumed).unwrap();
    local_gate(&reopened, &resumed).unwrap();
    complete_analysis(&mut resumed);
    super::super::validate_analysis_commit(&reopened, &resumed, true).unwrap();
    save_to_store(&reopened, &resumed).unwrap();
    assert_eq!(
        reopened.load_follow_ups().unwrap()[0]
            .owned()
            .unwrap()
            .review,
        original.owned().unwrap().review
    );
}

#[test]
fn active_and_interrupted_analysis_reject_relevant_input_and_authority_changes() {
    type Change = (&'static str, fn(&mut Settings));
    let changes: &[Change] = &[
        ("prompt", |s| s.agents[0].prompt = "Different lens.".into()),
        ("model", |s| s.agents[0].model = "other-model".into()),
        ("ai_account", |s| {
            s.agents[0].ai_account.as_mut().unwrap().account_id = "44".into()
        }),
        ("doctrine", |s| {
            s.doctrines[0].body = "Different doctrine.".into()
        }),
        ("doctrine_title", |s| {
            s.doctrines[0].title = "correctness".into()
        }),
        ("preset", |s| {
            let id = "00000000-0000-4000-8000-000000000006".to_string();
            s.presets.push(crate::storage::ReviewPreset {
                id: id.clone(),
                name: "Another lens".into(),
                body: "Different preset.".into(),
            });
            s.default_review_preset = Some(id);
        }),
        ("policy_prompt", |s| {
            s.defaults.prompt = "Different policy.".into()
        }),
        ("primary", |s| {
            s.repositories[0].primary_assignment_id = Some(SIBLING_ASSIGNMENT.into())
        }),
        ("repository_account", |s| {
            s.repositories[0].provider_account_id = Some("44".into())
        }),
        ("repository_identity", |s| {
            s.repositories[0].provider_repository_id = Some("101".into())
        }),
        ("repository_enabled", |s| s.repositories[0].enabled = false),
        ("assignment_agent", |s| {
            s.repositories[0].assignments[0].agent_id = SIBLING_AGENT.into()
        }),
    ];
    for (label, change) in changes {
        let (_root, store, mut run) = fixture();
        activate(&store, &mut run);
        local_gate(&store, &run).unwrap();
        super::super::validate_analysis_commit(&store, &run, true).unwrap();
        let original = run.owned().unwrap().review.clone();
        let mut settings = store.load_settings().unwrap();
        change(&mut settings);
        store.save_settings(&settings).unwrap();
        assert!(local_gate(&store, &run).is_err(), "active local: {label}");
        complete_analysis(&mut run);
        assert!(
            super::super::validate_analysis_commit(&store, &run, true).is_err(),
            "commit: {label}"
        );
        super::super::restore(&store).unwrap();
        let mut resumed = store.load_follow_ups().unwrap().remove(0);
        assert_eq!(
            resumed.analysis.as_ref().unwrap().state,
            OperationState::Interrupted
        );
        let candidate = candidates(&store).unwrap().remove(0);
        if candidate.blocked.is_none() {
            prepare_analysis(
                &store,
                &mut resumed,
                candidate.blocked,
                candidate.human_gate,
                false,
                102,
            )
            .unwrap();
        }

        assert_eq!(
            resumed.owned().unwrap().review,
            original,
            "resume recapture: {label}"
        );
        assert!(
            local_gate(&store, &resumed).is_err(),
            "resume local: {label}"
        );
        assert!(
            super::super::validate_analysis_commit(&store, &resumed, true).is_err(),
            "resume commit: {label}"
        );
    }
}

#[test]
fn capability_edits_do_not_gate_analysis_and_retry_keeps_actual_prior_configuration() {
    let (root, store, mut run) = fixture();
    activate(&store, &mut run);
    let captured = run.context.clone();
    let history_bytes = std::fs::read(root.path().join("state/follow-ups.json")).unwrap();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].comment = false;
    settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
        reply: true,
        approve: true,
        merge: true,
    });
    store.save_settings(&settings).unwrap();
    local_gate(&store, &run).unwrap();
    super::super::validate_analysis_commit(&store, &run, true).unwrap();
    assert_eq!(run.context, captured);
    assert_eq!(
        std::fs::read(root.path().join("state/follow-ups.json")).unwrap(),
        history_bytes
    );
    run.analysis
        .as_mut()
        .unwrap()
        .fail(&Failure::timeout().monitoring(), 101);
    run.phase = Phase::Stopped;
    save_to_store(&store, &run).unwrap();
    settings.agents[0].prompt = "A deliberately changed lens.".into();
    store.save_settings(&settings).unwrap();
    let retried = request_analysis(&store, &run.id, true, 102).unwrap();
    assert_eq!(retried.analysis_history.len(), 1);
    assert_eq!(retried.analysis_history[0].context, captured);
    assert_eq!(retried.analysis_history[0].operation, run.analysis.unwrap());
    assert_eq!(
        retried.context.selection.agent.prompt,
        "A deliberately changed lens."
    );
    assert_eq!(retried.analysis.as_ref().unwrap().attempt_count, 0);
}

#[test]
fn analysis_gates_keep_cancellation_and_detection_without_a_manual_start_preference() {
    for change in [
        "cancelled",
        "removed",
        "head",
        "account",
        "superseded",
        "legacy_start",
    ] {
        let (_root, store, mut run) = fixture();
        if change == "legacy_start" {
            let mut settings = store.load_settings().unwrap();
            settings.defaults.automatic_agent_start = false;
            settings.repositories[0].overrides.automatic_agent_start = Some(false);
            store.save_settings(&settings).unwrap();
            run.context.selection =
                Selection::resolve(&settings, &run.context.job, ASSIGNMENT).unwrap();
            run.context.selection.policy.automatic_agent_start = false;
            save_to_store(&store, &run).unwrap();
            let candidate = candidates(&store).unwrap().remove(0);
            assert!(candidate.automatic_start);
            prepare_analysis(
                &store,
                &mut run,
                candidate.blocked,
                candidate.human_gate,
                false,
                100,
            )
            .unwrap();
            save_to_store(&store, &run).unwrap();
            assert!(!run.manual_start);
            assert!(!run.context.selection.policy.automatic_agent_start);
            local_gate(&store, &run).unwrap();
            super::super::validate_analysis_commit(&store, &run, true).unwrap();
            continue;
        }
        activate(&store, &mut run);
        let mut jobs = store.load_queue().unwrap();
        match change {
            "cancelled" => {
                let mut current = run.clone();
                current.cancelled = true;
                save_to_store(&store, &current).unwrap();
            }
            "removed" => jobs.clear(),
            "head" => jobs[0].head_sha = "c".repeat(40),
            "account" => jobs[0].account_id = "44".into(),
            "superseded" => jobs[0].waiting = "superseded".into(),
            _ => unreachable!(),
        }
        store.save_queue(&jobs).unwrap();
        assert!(local_gate(&store, &run).is_err(), "{change}");
        assert!(
            super::super::validate_analysis_commit(&store, &run, true).is_err(),
            "{change}"
        );
    }
    let (_root, store, mut run) = fixture();
    activate(&store, &mut run);
    complete_analysis(&mut run);
    assert!(super::super::validate_analysis_commit(&store, &run, false).is_err());
}

#[test]
fn completed_reply_candidates_and_history_never_recapture_current_settings() {
    let (root, store, mut run) = fixture();
    activate(&store, &mut run);
    complete_analysis(&mut run);
    save_to_store(&store, &run).unwrap();
    let original = run.clone();
    let bytes = std::fs::read(root.path().join("state/follow-ups.json")).unwrap();
    change_sibling_comment(&store, &run);
    let candidate = candidates(&store).unwrap().remove(0);
    assert!(candidate.blocked.is_none());
    assert_eq!(candidate.run, original);
    assert!(prepare_analysis(
        &store,
        &mut run,
        candidate.blocked,
        candidate.human_gate,
        true,
        102,
    )
    .is_err());
    let mut settings = store.load_settings().unwrap();
    settings.agents[0].prompt = "Today's lens.".into();
    store.save_settings(&settings).unwrap();
    let candidate = candidates(&store).unwrap().remove(0);
    assert!(candidate.blocked.is_some());
    assert_eq!(candidate.run, original);
    super::super::restore(&store).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("state/follow-ups.json")).unwrap(),
        bytes
    );
}
