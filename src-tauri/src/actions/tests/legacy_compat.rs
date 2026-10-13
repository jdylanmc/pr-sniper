//! Old records written by the removed legacy path still load. Unfinished legacy
//! work is retired on restore and never runs again; finished records stay readable.

use super::*;
use crate::{
    follow_up::{ConversationContext, FollowUp, Phase, ReplyOutput},
    github::conversation::TopComment,
    review::RunMode,
};

/// Serialize, then drop the keys the legacy path never wrote.
fn old_shape(value: impl serde::Serialize, drop: &[&str]) -> Value {
    let mut value = serde_json::to_value(value).unwrap();
    for key in drop {
        value.as_object_mut().unwrap().remove(*key);
    }
    value
}

fn write(root: &std::path::Path, file: &str, value: Value) {
    std::fs::write(
        root.join("state").join(file),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

#[test]
fn old_legacy_records_load_and_unfinished_legacy_work_is_retired_not_run() {
    let (root, store, item) = fixture(1, true, false);
    let final_review = packet_final(&store, &item, &observed());
    let completed = store.load_reviews().unwrap().remove(0);

    // reviews.json: a finished legacy review (JSON decision) and a queued one.
    let mut finished = completed.clone();
    finished.mode = RunMode::Legacy;
    finished.packet = None;
    finished.result = Some(output());
    finished.feedback_context = Some(vec![]);
    let mut queued = finished.clone();
    queued.operation = JobOperation::review(&queued.job, NOW + 1);
    queued.result = None;
    queued.phase = "Waiting for shared AI capacity".into();
    let legacy_runs = [&finished, &queued].map(|r| old_shape(r, &["mode", "packet"]));
    assert!(legacy_runs[0]["result"]["output"]["synopsis"].is_string());
    write(root.path(), "reviews.json", json!(legacy_runs));

    // actions.json: a queued primary final review, no packet approval.
    let mut legacy_final = final_review.clone();
    legacy_final.id = "final-legacy".into();
    legacy_final.packet = None;
    legacy_final.execution = queued.clone();
    legacy_final.execution.operation.operation_type = "primary_final_review".into();
    let mut ledger = store.load_actions().unwrap();
    ledger.finals = vec![legacy_final];
    let mut actions = serde_json::to_value(&ledger).unwrap();
    for f in actions["finals"].as_array_mut().unwrap() {
        f.as_object_mut().unwrap().remove("packet");
        f["execution"].as_object_mut().unwrap().remove("mode");
        f["execution"].as_object_mut().unwrap().remove("packet");
    }
    write(root.path(), "actions.json", actions);

    // follow-ups.json: an unanalyzed mention and an analyzed reply awaiting publication.
    let context = ConversationContext {
        assignment_id: finished.assignment_id.clone(),
        job: finished.job.clone(),
        selection: finished.selection.clone(),
        trust_confirmed: false,
        feedback: vec![],
        feedback_checked: true,
    };
    let comment = |id: &str| TopComment {
        id: id.into(),
        body: "@actor why?".into(),
        author_id: Some("11".into()),
        author_login: Some("author".into()),
        created_at: "2026-09-30T00:00:00Z".into(),
        updated_at: "2026-09-30T00:00:00Z".into(),
    };
    let mut waiting = FollowUp::mention(comment("501"), context.clone());
    waiting.analysis = Some(waiting.operation("mention_analysis", NOW));
    let mut analyzed = FollowUp::mention(comment("502"), context);
    let mut analysis = analyzed.operation("mention_analysis", NOW);
    analysis.state = OperationState::Completed;
    analyzed.analysis = Some(analysis);
    analyzed.phase = Phase::WaitingPublication;
    analyzed.result = Some(ReviewResult::<ReplyOutput> {
        reviewed_base_sha: Some("b".repeat(40)),
        output: serde_json::from_value(json!({"decision":"reply","body":"Because.",
            "new_information":"n","reason":"r","evidence":[]}))
        .unwrap(),
        session_id: "s".into(),
        model: "model".into(),
        runtime_version: "fixture".into(),
        intelligence: None,
        input_tokens: 1,
        output_tokens: 1,
        tool_calls: 0,
        byte_coverage: false,
        coverage: None,
    });
    write(
        root.path(),
        "follow-ups.json",
        json!([old_shape(&waiting, &[]), old_shape(&analyzed, &[])]),
    );

    // Every old file loads unchanged.
    let reviews = store.load_reviews().unwrap();
    assert!(reviews.iter().all(|r| r.mode == RunMode::Legacy));
    assert_eq!(reviews[0].result, finished.result);
    assert_eq!(store.load_actions().unwrap().finals[0].packet, None);
    assert_eq!(store.load_follow_ups().unwrap().len(), 2);
    queue::snapshot(&store, vec![]).unwrap();

    crate::review::restore(&store).unwrap();
    crate::actions::restore(&store).unwrap();
    crate::follow_up::restore(&store).unwrap();

    let reviews = store.load_reviews().unwrap();
    assert_eq!(reviews[0], finished, "a finished legacy result is history");
    assert_eq!(reviews[1].operation.state, OperationState::Failed);
    assert_eq!(
        reviews[1].error.as_deref(),
        Some(crate::review::LEGACY_RETIRED)
    );
    let legacy_final = store.load_actions().unwrap().finals.remove(0);
    assert!(legacy_final.cancelled);
    assert_eq!(
        legacy_final.execution.operation.state,
        OperationState::Failed
    );
    assert!(validate_local(&store, &legacy_final).is_err());
    assert!(ready(&store, &legacy_final, &observed(), Action::Approve).is_err());
    let follow_ups = store.load_follow_ups().unwrap();
    assert!(follow_ups[0].cancelled && follow_ups[0].phase == Phase::Stopped);
    assert!(!follow_ups[1].cancelled && follow_ups[1].phase == Phase::WaitingPublication);

    // Nothing legacy runs: no AI work is dispatched for the retired runs.
    let batch = Capacity::default().dispatch(&store, NOW + 100).unwrap();
    assert!(batch.dispatched.is_empty(), "{:?}", batch.errors);
    // A restart is idempotent.
    let before = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    crate::review::restore(&store).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        before
    );
}

#[test]
fn a_new_run_is_packet_mode_even_without_explicit_tools() {
    let (_root, store, _item) = fixture(1, true, true);
    store.save_reviews(&[]).unwrap();
    let settings = store.load_settings().unwrap();
    let repository = &settings.repositories[0];
    assert!(repository.assignments[0].tools.is_none());
    assert_eq!(
        repository.assignment_tools(&repository.assignments[0]),
        [
            "read_changes",
            "read_source",
            "get_thread",
            "get_checks",
            "list_tree",
            "search_code",
            "ready_for_human",
            "approve_pull_request",
            "approve_and_merge",
            "panic_exit"
        ]
    );
    let key = crate::review::host::candidates(&store).unwrap()[0]
        .key
        .clone();
    let run = crate::review::host::request(&store, &key, true, NOW + 5).unwrap();
    assert_eq!(run.mode, RunMode::Packet);
}

#[test]
fn role_defaults_follow_grants_and_teammates_report() {
    use crate::storage::{default_tools, AssignmentAuthority};
    let authority = |primary, comment, approve, merge| AssignmentAuthority {
        primary,
        comment,
        reply: false,
        approve,
        merge,
    };
    let exits = |tools: Vec<String>| tools[6..].to_vec();
    assert_eq!(
        exits(default_tools(&authority(true, false, false, false))),
        ["ready_for_human", "panic_exit"]
    );
    assert_eq!(
        exits(default_tools(&authority(true, true, true, false))),
        [
            "leave_comments",
            "ready_for_human",
            "approve_pull_request",
            "panic_exit"
        ]
    );
    // A teammate's grants are ignored: it only reports.
    assert_eq!(
        exits(default_tools(&authority(false, true, false, false))),
        ["submit_report", "panic_exit"]
    );
}
