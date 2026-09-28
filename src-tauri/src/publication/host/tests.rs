use super::*;
use crate::review::{ReviewRun, Selection};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Store, ReviewRun) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let repository_id = uuid::Uuid::new_v4().to_string();
    let assignment_id = uuid::Uuid::new_v4().to_string();
    let agent_id = uuid::Uuid::new_v4().to_string();
    let mut settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":repository_id,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":assignment_id,"agent_id":agent_id,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":agent_id,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"machine"}]
    })).unwrap();
    settings.defaults.automatic_comment_publication = true;
    store.save_settings(&settings).unwrap();
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id":assignment_id,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":repository_id,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"human_start","detected_at":100
    }))
    .unwrap();
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    let review = ReviewRun {
        key: crate::review::key(&job,&assignment_id),
        selection: Selection::resolve(&settings,&job,&assignment_id).unwrap(),
        job, assignment_id, operation, manual_start:true,
        trust_confirmed:true, phase:"Complete".into(), error:None,
        result:Some(serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),
            "output":{"synopsis":"No changes need attention.","files":[],"findings":[],"decision":"machine_sign_off"},
            "session_id":"session","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":0
        })).unwrap()),
    };
    store.save_reviews(std::slice::from_ref(&review)).unwrap();
    (root, store, review)
}

#[test]
fn newer_local_attempt_blocks_automatic_publication_of_the_older_result() {
    let (_root, store, review) = fixture();
    assert!(candidates(&store).unwrap()[0].automatic);
    let mut newer = review.clone();
    newer.operation.id = "new-attempt".into();
    newer.operation.state = OperationState::Running;
    newer.result = None;
    store.save_reviews(&[review, newer]).unwrap();
    let candidates = candidates(&store).unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(!candidates[0].automatic);
    assert!(candidates[0]
        .blocked
        .as_ref()
        .unwrap()
        .contains("newer local review"));
}

#[test]
fn uncertain_publication_reserves_revision_even_after_another_result_appears() {
    let (_root, store, review) = fixture();
    let mut publication = Publication::new(review.clone(), true, false, 100).unwrap();
    publication.uncertain = true;
    publication.operation.state = OperationState::ManualRetry;
    store
        .save_publications(std::slice::from_ref(&publication))
        .unwrap();
    let mut newer = review.clone();
    newer.operation.id = "new-attempt".into();
    store.save_reviews(&[review, newer]).unwrap();
    let candidates = candidates(&store).unwrap();
    assert!(candidates[1]
        .blocked
        .as_ref()
        .unwrap()
        .contains("owns this revision"));
    assert!(publication.reserves_revision());
    assert_eq!(
        candidates[0].publication.as_ref().unwrap().id,
        publication.id
    );
}
