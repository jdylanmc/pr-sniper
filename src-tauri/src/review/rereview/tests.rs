use super::*;
use crate::monitoring::{OperationState, QueueJob};
use crate::publication::{Publication, Receipt};
use crate::queue::State;
use serde_json::json;

const NOW: i64 = 1_000;

fn fixture() -> (tempfile::TempDir, Store, ReviewRun) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let repository_id = uuid::Uuid::new_v4().to_string();
    let assignment_id = uuid::Uuid::new_v4().to_string();
    let agent_id = uuid::Uuid::new_v4().to_string();
    let settings: crate::storage::Settings = serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":repository_id,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":assignment_id,"agent_id":agent_id,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":agent_id,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"machine"}]
    }))
    .unwrap();
    store.save_settings(&settings).unwrap();
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id":assignment_id,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":repository_id,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"ai_capacity","detected_at":100,
        "observed_base_sha":"b".repeat(40)
    }))
    .unwrap();
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    let review = ReviewRun {
        mode: Default::default(),
        packet: None,
        feedback_context: None,
        key: super::super::key(&job, &assignment_id),
        selection: Selection::resolve(&settings, &job, &assignment_id).unwrap(),
        job,
        assignment_id,
        operation,
        manual_start: false,
        trust_confirmed: false,
        phase: "Complete".into(),
        error: None,
        result: Some(serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),
            "output":{"synopsis":"Needs a look.","files":[],"findings":[],"decision":"human_input_required"},
            "session_id":"session","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":0
        })).unwrap()),
    };
    store.save_reviews(std::slice::from_ref(&review)).unwrap();
    (root, store, review)
}

fn publish(store: &Store, review: &ReviewRun, phase: Phase, receipts: &[RemoteState]) {
    let mut publication = Publication::new(review.clone(), true, false, 100).unwrap();
    publication.phase = phase;
    publication.operation.state = OperationState::Completed;
    publication.receipts = receipts
        .iter()
        .map(|state| Receipt {
            review_id: "1".into(),
            state: state.clone(),
            comment_ids: vec![],
        })
        .collect();
    let mut all = store.load_publications().unwrap();
    all.push(publication);
    store.save_publications(&all).unwrap();
}

fn discard(store: &Store, review: &ReviewRun) {
    publish(
        store,
        review,
        Phase::Discarded,
        &[RemoteState::Pending, RemoteState::Deleted],
    );
}

fn edit_lens(store: &Store, prompt: &str) {
    let mut settings = store.load_settings().unwrap();
    settings.agents[0].prompt = prompt.into();
    store.save_settings(&settings).unwrap();
}

fn latest(store: &Store) -> ReviewRun {
    store.load_reviews().unwrap().pop().unwrap()
}

fn complete_latest(store: &Store, template: &ReviewRun) -> ReviewRun {
    let mut reviews = store.load_reviews().unwrap();
    let run = reviews.last_mut().unwrap();
    run.operation.state = OperationState::Completed;
    run.result = template.result.clone();
    let done = run.clone();
    store.save_reviews(&reviews).unwrap();
    done
}

fn item_state(store: &Store) -> State {
    crate::queue::normal_snapshot(store, vec![]).unwrap().items[0].state
}

#[test]
fn lens_change_discard_queues_exactly_one_run_under_the_current_lens() {
    let (_root, store, review) = fixture();
    edit_lens(&store, "Review security too.");
    discard(&store, &review);
    assert_eq!(item_state(&store), State::WaitingForHuman);
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 1);
    let run = latest(&store);
    assert_eq!(store.load_reviews().unwrap().len(), 2);
    assert_ne!(run.operation.id, review.operation.id);
    assert_eq!(run.operation.state, OperationState::Queued);
    assert_eq!(run.key, review.key);
    assert!(run.result.is_none() && !run.manual_start);
    assert_eq!(run.selection.agent.prompt, "Review security too.");
    assert_eq!(run.operation.rereview_lens, Some(run.selection.lens_hash()));
    assert_eq!(item_state(&store), State::Queued);
    let mut reviews = store.load_reviews().unwrap();
    reviews[1].operation.state = OperationState::Running;
    store.save_reviews(&reviews).unwrap();
    assert_eq!(item_state(&store), State::Reviewing);
    // Queued, running, restarted or polled again: never a second run.
    assert_eq!(enqueue_discarded(&store, NOW + 1).unwrap(), 0);
    assert_eq!(store.load_reviews().unwrap().len(), 2);
}

#[test]
fn queued_rereview_is_dispatched_by_the_normal_capacity_queue() {
    let (_root, store, review) = fixture();
    edit_lens(&store, "Review security too.");
    discard(&store, &review);
    enqueue_discarded(&store, NOW).unwrap();
    let work = crate::capacity::candidates(&store, NOW).unwrap();
    assert!(work
        .iter()
        .any(|w| w.key.id == review.key && w.state == "waiting"));
}

#[test]
fn remote_difference_discard_under_an_unchanged_lens_is_re_reviewed_once() {
    let (_root, store, review) = fixture();
    discard(&store, &review);
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 1);
    let second = complete_latest(&store, &review);
    discard(&store, &second);
    assert_eq!(enqueue_discarded(&store, NOW + 1).unwrap(), 0);
    assert_eq!(store.load_reviews().unwrap().len(), 2);
}

#[test]
fn a_later_lens_change_earns_one_more_run() {
    let (_root, store, review) = fixture();
    edit_lens(&store, "Lens two.");
    discard(&store, &review);
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 1);
    let second = complete_latest(&store, &review);
    edit_lens(&store, "Lens three.");
    discard(&store, &second);
    assert_eq!(enqueue_discarded(&store, NOW + 1).unwrap(), 1);
    assert_eq!(latest(&store).selection.agent.prompt, "Lens three.");
    let third = complete_latest(&store, &review);
    discard(&store, &third);
    assert_eq!(enqueue_discarded(&store, NOW + 2).unwrap(), 0);
    assert_eq!(store.load_reviews().unwrap().len(), 3);
}

#[test]
fn published_head_is_never_re_reviewed() {
    let (_root, store, review) = fixture();
    edit_lens(&store, "Review security too.");
    discard(&store, &review);
    let mut other = review.clone();
    other.operation.id = "earlier-attempt".into();
    publish(&store, &other, Phase::Published, &[RemoteState::Commented]);
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 0);
    assert_eq!(store.load_reviews().unwrap().len(), 1);
}

#[test]
fn only_discarded_batches_trigger_a_run() {
    let (_root, store, review) = fixture();
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 0);
    publish(&store, &review, Phase::Published, &[RemoteState::Commented]);
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 0);
    assert_eq!(store.load_reviews().unwrap().len(), 1);
}

#[test]
fn moved_head_closed_or_merged_pull_requests_are_skipped() {
    for case in ["head", "closed", "merged", "superseded"] {
        let (_root, store, review) = fixture();
        edit_lens(&store, "Review security too.");
        discard(&store, &review);
        let mut jobs = store.load_queue().unwrap();
        match case {
            "head" => {
                let mut next = jobs[0].clone();
                next.head_sha = "c".repeat(40);
                jobs[0].waiting = "superseded".into();
                jobs.push(next);
            }
            other => jobs[0].waiting = other.into(),
        }
        store.save_queue(&jobs).unwrap();
        assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 0, "{case}");
        assert_eq!(store.load_reviews().unwrap().len(), 1, "{case}");
    }
}

#[test]
fn legacy_state_without_the_marker_loads_and_is_picked_up_once() {
    let (root, store, review) = fixture();
    discard(&store, &review);
    let on_disk = std::fs::read_to_string(root.path().join("state/reviews.json")).unwrap();
    assert!(!on_disk.contains("rereview_lens"));
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 1);
    let reopened = Store::new(root.path().into());
    assert_eq!(enqueue_discarded(&reopened, NOW + 60).unwrap(), 0);
}

#[test]
fn completed_review_with_a_changed_lens_and_no_batch_is_re_reviewed_once() {
    let (_root, store, review) = fixture();
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 0);
    edit_lens(&store, "Review security too.");
    assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 1);
    let run = latest(&store);
    assert_eq!(run.operation.state, OperationState::Queued);
    assert_eq!(run.selection.agent.prompt, "Review security too.");
    assert_eq!(item_state(&store), State::Queued);
    assert_eq!(enqueue_discarded(&store, NOW + 1).unwrap(), 0);
    // The new result is under the current lens: nothing more to do.
    complete_latest(&store, &review);
    assert_eq!(enqueue_discarded(&store, NOW + 2).unwrap(), 0);
    edit_lens(&store, "Lens three.");
    assert_eq!(enqueue_discarded(&store, NOW + 3).unwrap(), 1);
    assert_eq!(store.load_reviews().unwrap().len(), 3);
}

#[test]
fn unbatched_lens_change_skips_published_in_flight_closed_and_local_only() {
    for case in ["published", "pending", "closed", "local_only"] {
        let (_root, store, review) = fixture();
        edit_lens(&store, "Review security too.");
        match case {
            "published" => publish(&store, &review, Phase::Published, &[RemoteState::Commented]),
            "pending" => publish(&store, &review, Phase::Pending, &[RemoteState::Pending]),
            "closed" => {
                let mut jobs = store.load_queue().unwrap();
                jobs[0].waiting = "closed".into();
                store.save_queue(&jobs).unwrap();
            }
            _ => {
                let mut settings = store.load_settings().unwrap();
                settings.repositories[0].assignments[0].comment = false;
                store.save_settings(&settings).unwrap();
            }
        }
        assert_eq!(enqueue_discarded(&store, NOW).unwrap(), 0, "{case}");
        assert_eq!(store.load_reviews().unwrap().len(), 1, "{case}");
    }
}
