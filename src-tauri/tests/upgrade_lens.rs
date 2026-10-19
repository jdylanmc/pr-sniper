//! Upgrading to id-keyed library references must never be read as a lens change:
//! work captured before ids (title references, id-less Doctrines, no Voice)
//! keeps publishing and running under the migrated settings.
use pr_sniper_lib::{
    follow_up::{FollowUp, ReplyDecision, ReplyOutput},
    github::{
        metadata::{Lifecycle, PullRequest},
        threads::{Comment, Thread},
    },
    monitoring::{JobOperation, OperationState, QueueJob},
    publication::{self, Batch, InlineComment, Publication, Receipt, RemoteState},
    review::{self, ReviewResult, ReviewRun, Selection},
    storage::Settings,
};
use serde_json::{json, Value};
use std::fs;

mod support;
use support::Fixture;

const REPOSITORY: &str = "00000000-0000-4000-8000-000000000001";
const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "00000000-0000-4000-8000-000000000003";

/// Settings as written before library ids and Voices.
fn pre_id_settings() -> Value {
    json!({
        "launch_at_login": false,
        "doctrine_catalog_version": 1,
        "admission_migration_version": 1,
        "doctrines": [
            {"title":"Correctness","body":"Trace state transitions."},
            {"title":"Boundaries","body":"Keep authority explicit."}
        ],
        "repositories": [{"id":REPOSITORY,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,
                "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true,
                "actions":{"reply":true,"approve":false,"merge":false}}]}],
        "agents": [{"id":AGENT,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "doctrines":["Correctness","Boundaries"],
            "prompt":"Review correctness.","signature":"stored"}]
    })
}

fn job() -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":REPOSITORY,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

fn pull() -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 1,
        title: "Review".into(),
        author: Some(pr_sniper_lib::github::Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: "a".repeat(40),
        base_sha: "b".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-09-27T00:00:00Z".into(),
        files: vec![],
    }
}

/// The selection the pre-id code captured: the same inputs, recorded as title
/// references on the Agent and id-less Doctrines.
fn pre_id_selection() -> Selection {
    let mut settings: Settings = serde_json::from_value(pre_id_settings()).unwrap();
    settings.migrate_library_ids();
    let current = Selection::resolve(&settings, &job(), ASSIGNMENT).unwrap();
    let mut value = serde_json::to_value(current).unwrap();
    let agent = value["agent"].as_object_mut().unwrap();
    agent.remove("doctrine_ids");
    agent.insert("doctrines".into(), json!(["Correctness", "Boundaries"]));
    for doctrine in value["configuration"]["doctrines"].as_array_mut().unwrap() {
        doctrine.as_object_mut().unwrap().remove("id");
    }
    assert!(value["configuration"].get("voice").is_none());
    serde_json::from_value(value).unwrap()
}

fn run(state: OperationState, result: bool) -> ReviewRun {
    let job = job();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = state;
    ReviewRun {
        mode: review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: review::key(&job, ASSIGNMENT),
        selection: pre_id_selection(),
        job,
        assignment_id: ASSIGNMENT.into(),
        operation,
        manual_start: true,
        trust_confirmed: true,
        phase: "Fixture".into(),
        error: None,
        result: result.then(|| {
            serde_json::from_value(json!({
                "reviewed_base_sha":"b".repeat(40),
                "output":{"exit":{"exit":"comments",
                    "new_comments":[{"path":"source.rs","line":1,"body":"A defect. Evidence."}],
                    "reply_comments":[],"resolve_own_threads":[]}},
                "session_id":"session","model":"model","runtime_version":"fixture",
                "input_tokens":10,"output_tokens":10,"tool_calls":1
            }))
            .unwrap()
        }),
    }
}

fn comment(id: &str, body: &str, reply_to: Option<&str>) -> Comment {
    Comment {
        id: id.into(),
        body: body.into(),
        author_id: Some(if reply_to.is_none() { "22" } else { "11" }.into()),
        author_login: Some("person".into()),
        reply_to: reply_to.map(String::from),
        review_id: Some("42".into()),
        original_commit: Some("a".repeat(40)),
        created_at: "2026-09-27T00:00:00Z".into(),
        published_at: "2026-09-27T00:00:00Z".into(),
    }
}

fn follow_up(review: ReviewRun) -> FollowUp {
    let mut origin = Publication::new(review, false, true, 100).unwrap();
    origin.batch = Some(Batch {
        commit_id: "a".repeat(40),
        body: "Summary".into(),
        comments: vec![InlineComment {
            diff_position: None,
            path: "source.rs".into(),
            line: 1,
            side: "RIGHT".into(),
            body: "Root finding".into(),
        }],
        unmappable: vec![],
    });
    origin.receipts = vec![Receipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["100".into()],
    }];
    let thread = Thread {
        id: "thread-node".into(),
        resolved: false,
        can_reply: true,
        comments: vec![
            comment("100", "Root finding", None),
            comment("101", "Why is this required?", Some("100")),
        ],
    };
    let mut run = FollowUp::new(&origin, thread).unwrap();
    run.result = Some(ReviewResult {
        reviewed_base_sha: Some("b".repeat(40)),
        output: ReplyOutput {
            feedback_assessments: Vec::new(),
            decision: ReplyDecision::Reply,
            body: "The function returns 42.".into(),
            new_information: "The function returns 42.".into(),
            reason: "Source evidence.".into(),
            evidence: vec![],
        },
        session_id: "session".into(),
        model: "model".into(),
        runtime_version: "fixture".into(),
        intelligence: None,
        input_tokens: 1,
        output_tokens: 1,
        tool_calls: 1,
        byte_coverage: false,
        coverage: None,
        ducks: vec![],
        finding_dispositions: vec![],
    });
    run
}

#[test]
fn work_captured_before_library_ids_survives_the_upgrade() {
    let fixture = Fixture::new();
    let path = fixture.path().join("config/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&pre_id_settings()).unwrap()).unwrap();
    let store = fixture.store();
    let completed = run(OperationState::Completed, true);
    let queued = run(OperationState::Queued, false);
    store.save_queue(&[job()]).unwrap();
    store
        .save_reviews(&[completed.clone(), queued.clone()])
        .unwrap();
    store
        .save_follow_ups(&[follow_up(completed.clone())])
        .unwrap();

    // The upgrade: load migrates settings; stored evidence keeps its shape.
    let settings = store.load_settings().unwrap();
    assert!(settings.agents[0].doctrines.is_none());
    assert_eq!(settings.agents[0].doctrine_ids.len(), 2);
    let reviews = store.load_reviews().unwrap();
    assert_eq!(reviews, vec![completed.clone(), queued.clone()]);
    assert_eq!(
        reviews[0].selection.agent.doctrines.as_deref(),
        Some(&["Correctness".to_string(), "Boundaries".to_string()][..])
    );

    let current = Selection::resolve(&settings, &job(), ASSIGNMENT).unwrap();
    let gate = publication::evaluate_gate(
        &settings,
        &Publication::new(reviews[0].clone(), false, true, 100).unwrap(),
        Some(&job()),
        &pull(),
        true,
        true,
        false,
    );
    assert!(!gate.discard, "{:?}", gate.stop);
    assert!(publication::automatic_policy(&settings, &reviews[0], &job()).is_ok());
    for run in &reviews {
        assert!(review::validate_execution_selection(&store, run).is_ok());
    }
    let follow_up = store.load_follow_ups().unwrap().remove(0);
    assert!(follow_up.result.is_some());
    assert!(follow_up.same_result_lens(&current));
    assert!(follow_up.same_analysis_execution(&current));
}

#[test]
fn real_changes_after_the_upgrade_still_change_the_lens() {
    let mut settings: Settings = serde_json::from_value(pre_id_settings()).unwrap();
    settings.migrate_library_ids();
    let captured = pre_id_selection();
    let current = |settings: &Settings| Selection::resolve(settings, &job(), ASSIGNMENT).unwrap();
    assert!(current(&settings).same_lens(&captured));
    assert!(current(&settings).same_execution(&captured));
    // Titles render verbatim in the packet: case or spacing edits are changes.
    let edits: [fn(&mut Settings); 9] = [
        |s| s.doctrines[0].body = "Edited.".into(),
        |s| s.doctrines[0].title = "State".into(),
        |s| s.doctrines[0].title = "CORRECTNESS".into(),
        |s| s.doctrines[0].title = "Correctness ".into(),
        |s| s.agents[0].doctrine_ids.reverse(),
        |s| s.agents[0].doctrine_ids.truncate(1),
        |s| s.agents[0].prompt = "Another lens.".into(),
        |s| s.agents[0].model = "another-model".into(),
        |s| s.agents[0].ai_account.as_mut().unwrap().account_id = "44".into(),
    ];
    for edit in edits {
        let mut changed = settings.clone();
        edit(&mut changed);
        assert!(!current(&changed).same_lens(&captured));
        assert!(!current(&changed).same_execution(&captured));
    }
    // A primary's Voice is part of its lens.
    let mut voiced = settings.clone();
    voiced.voices = Fixture::new().store().load_settings().unwrap().voices;
    voiced.agents[0].voice_id = Some(voiced.voices[0].id.clone());
    assert!(!current(&voiced).same_lens(&captured));
    let before = current(&voiced);
    assert!(current(&voiced).same_lens(&before));
    let voice_edits: [fn(&mut Settings); 5] = [
        |s| s.voices[0].guardrails = false,
        |s| s.voices[0].body = "Edited style.".into(),
        |s| s.voices[0].title = "Grunt".into(),
        |s| s.voices[0].title = "caveman".into(),
        |s| s.agents[0].voice_id = Some(s.voices[1].id.clone()),
    ];
    for edit in voice_edits {
        let mut changed = voiced.clone();
        edit(&mut changed);
        assert!(!current(&changed).same_lens(&before));
        assert!(!current(&changed).same_execution(&before));
    }
}
