//! A PR that moves or leaves review while an attempt is in flight supersedes
//! that attempt: never a parked failure, never a spent attempt, never a write.

use super::*;
use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::{Capabilities, CommentCapability, Connection, RemoteRepository},
    Identity,
};
use crate::monitoring::{
    ActivationMode, Monitor, MonitoringActivation, OperationFailure, PollResult, WAITING_SUPERSEDED,
};
use crate::queue::State;
use crate::storage::{Settings, Store};
use serde_json::json;
use std::collections::BTreeMap;

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const PRIMARY: &str = "cccccccc-cccc-4ccc-8ccc-000000000001";
const TEAMMATE: &str = "cccccccc-cccc-4ccc-8ccc-000000000002";
const NOW: i64 = 1_800_000_000;

fn configured(review_drafts: bool) -> (tempfile::TempDir, Store, Monitor) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let agent = |index: u8, name: &str| {
        json!({
            "id":format!("aaaaaaaa-aaaa-4aaa-8aaa-00000000000{index}"),"name":name,
            "model":"explicit-model","ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Find actionable defects.","signature":"fixture"
        })
    };
    let assignment = |id: &str, index: u8| {
        json!({
            "id":id,"agent_id":format!("aaaaaaaa-aaaa-4aaa-8aaa-00000000000{index}"),
            "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true
        })
    };
    let settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,"doctrines":[],
        "repositories":[{
            "id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "review_drafts":review_drafts,
            "primary_assignment_id":PRIMARY,
            "assignments":[assignment(PRIMARY, 1), assignment(TEAMMATE, 2)]
        }],
        "agents":[agent(1, "Primary"), agent(2, "SOLID Snake")]
    }))
    .unwrap();
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "fixture-activation".into(),
            repository_id: REPO.into(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: 100,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW - 1,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    let monitor = Monitor::restore(&store).unwrap();
    (root, store, monitor)
}

fn pull(head: char) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 297,
        title: "docs: remove stray merge marker".into(),
        author: Some(Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: head.to_string().repeat(40),
        base_sha: "f".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-10-14T22:10:46Z".into(),
        files: vec![],
    }
}

fn scan(monitor: &mut Monitor, store: &Store, pulls: Vec<PullRequest>, at: i64) {
    let ticket = monitor.prepare_checks(store, at, true).unwrap().remove(0);
    let result = PollResult {
        states: Default::default(),
        connection: Connection {
            identity: Identity {
                id: "22".into(),
                login: "acting-account".into(),
            },
            repository: RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Available,
            },
        },
        pull_requests: pulls,
    };
    monitor.finish(store, ticket, Ok(result), at + 1).unwrap();
}

/// The assignment's current entry, dispatched into a running attempt.
fn dispatch(store: &Store, assignment: &str, now: i64) -> ReviewRun {
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.assignment_id.as_deref() == Some(assignment))
        .find(|j| j.waiting != WAITING_SUPERSEDED)
        .unwrap();
    let key = key(&job, assignment);
    request(store, &key, true, now).unwrap();
    let run = prepare_dispatch(store, &key, now).unwrap();
    assert_eq!(run.operation.state, OperationState::Running);
    run
}

fn stored(store: &Store, id: &str) -> ReviewRun {
    store
        .load_reviews()
        .unwrap()
        .into_iter()
        .find(|r| r.operation.id == id)
        .unwrap()
}

fn assert_quiet(store: &Store, monitor: &Monitor) {
    let snapshot = crate::queue::snapshot(store, monitor.snapshot()).unwrap();
    for item in &snapshot.items {
        assert_ne!(item.state, State::Failed, "{:?}", item.warnings);
        assert!(item.failed_teammate.is_none());
        assert!(item.failure_class.is_none());
    }
}

#[test]
fn a_teammate_attempt_on_a_moved_head_is_superseded_and_the_new_revision_queues_once() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let settings = store.load_settings().unwrap();

    let error = pull_gate(&settings, &run.job, &pull('b')).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Superseded);
    complete(&store, &run.operation.id, Err(error), NOW + 3).unwrap();

    let stale = stored(&store, &run.operation.id);
    assert_eq!(stale.operation.state, OperationState::Queued);
    assert_eq!(stale.operation.attempt_count, 0);
    assert_eq!(stale.operation.failure, None);
    assert!(!stale.operation.parked());
    assert!(stale.result.is_none());
    assert_quiet(&store, &monitor);

    // The next poll retires the stale entry and queues the new head once.
    scan(&mut monitor, &store, vec![pull('b')], NOW + 10);
    scan(&mut monitor, &store, vec![pull('b')], NOW + 20);
    let jobs = store.load_queue().unwrap();
    let fresh: Vec<_> = jobs
        .iter()
        .filter(|j| {
            j.assignment_id.as_deref() == Some(TEAMMATE) && j.head_sha == pull('b').head_sha
        })
        .collect();
    assert_eq!(fresh.len(), 1);
    assert_ne!(fresh[0].waiting, WAITING_SUPERSEDED);
    assert!(jobs
        .iter()
        .filter(|j| j.head_sha == pull('a').head_sha)
        .all(|j| j.waiting == WAITING_SUPERSEDED));
    assert!(prepare_dispatch(&store, &run.key, NOW + 60).is_err());
    let next = dispatch(&store, TEAMMATE, NOW + 61);
    assert_eq!(next.job.head_sha, pull('b').head_sha);
    assert_eq!(next.operation.attempt_count, 1);
    assert_quiet(&store, &monitor);
    assert!(store.load_publications().unwrap().is_empty());
}

#[test]
fn a_primary_attempt_whose_head_moved_after_the_ai_run_publishes_nothing() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, PRIMARY, NOW + 2);
    let settings = store.load_settings().unwrap();
    // The final eligibility check after the run sees the new head.
    let error = pull_gate(&settings, &run.job, &pull('b')).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Superseded);
    complete(&store, &run.operation.id, Err(error), NOW + 3).unwrap();
    let stale = stored(&store, &run.operation.id);
    assert_eq!(stale.operation.state, OperationState::Queued);
    assert_eq!(stale.operation.attempt_count, 0);
    assert!(stale.result.is_none());
    assert!(store.load_publications().unwrap().is_empty());
    assert!(store.load_feedback().unwrap().exit_writes.is_empty());
    assert_quiet(&store, &monitor);
}

#[test]
fn a_closed_merged_or_unadmitted_draft_pr_ends_the_attempt_quietly() {
    for change in ["closed", "merged", "draft"] {
        let (_root, store, mut monitor) = configured(false);
        scan(&mut monitor, &store, vec![pull('a')], NOW);
        let run = dispatch(&store, TEAMMATE, NOW + 2);
        let mut moved = pull('a');
        match change {
            "closed" => moved.state = Lifecycle::Closed,
            "merged" => moved.state = Lifecycle::Merged,
            _ => moved.draft = true,
        }
        let settings = store.load_settings().unwrap();
        let error = pull_gate(&settings, &run.job, &moved).unwrap_err();
        assert_eq!(error.kind, OperationFailure::Superseded, "{change}");
        complete(&store, &run.operation.id, Err(error), NOW + 3).unwrap();
        assert!(!stored(&store, &run.operation.id).operation.parked());
        assert_quiet(&store, &monitor);

        scan(&mut monitor, &store, vec![moved], NOW + 10);
        assert!(
            prepare_dispatch(&store, &run.key, NOW + 60).is_err(),
            "{change}"
        );
        assert_quiet(&store, &monitor);
        // The stale attempt waits exactly like the never-started primary entry.
        let work = crate::capacity::candidates(&store, NOW + 60).unwrap();
        let state = |key: &str| {
            let entry = work.iter().find(|w| w.key.id == key).unwrap();
            (entry.state, entry.failure)
        };
        let primary = store
            .load_queue()
            .unwrap()
            .into_iter()
            .find(|j| j.assignment_id.as_deref() == Some(PRIMARY))
            .unwrap();
        assert_eq!(state(&run.key), state(&key(&primary, PRIMARY)), "{change}");
        assert_eq!(state(&run.key).1, None);
    }
}

#[test]
fn a_draft_stays_reviewable_when_the_repository_reviews_drafts() {
    let (_root, store, mut monitor) = configured(true);
    let mut draft = pull('a');
    draft.draft = true;
    scan(&mut monitor, &store, vec![draft.clone()], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let settings = store.load_settings().unwrap();
    pull_gate(&settings, &run.job, &draft).unwrap();
}

#[test]
fn a_removed_or_replaced_assignment_supersedes_instead_of_parking() {
    for change in ["removed", "replaced"] {
        let (_root, store, mut monitor) = configured(false);
        scan(&mut monitor, &store, vec![pull('a')], NOW);
        let run = dispatch(&store, TEAMMATE, NOW + 2);
        let mut settings = store.load_settings().unwrap();
        let repository = &mut settings.repositories[0];
        match change {
            "removed" => {
                repository.assignments.retain(|a| a.id != TEAMMATE);
            }
            _ => {
                let primary_agent = repository.assignments[0].agent_id.clone();
                repository.assignments[1].agent_id = primary_agent;
            }
        }
        store.save_settings(&settings).unwrap();
        let error = crate::review::validate_execution_selection(&store, &run).unwrap_err();
        assert_eq!(error.kind, OperationFailure::Superseded, "{change}");
        complete(&store, &run.operation.id, Err(error), NOW + 3).unwrap();
        let stale = stored(&store, &run.operation.id);
        assert!(!stale.operation.parked(), "{change}");
        assert_eq!(stale.operation.attempt_count, 0);
        assert!(prepare_dispatch(&store, &run.key, NOW + 60).is_err());
        assert_quiet(&store, &monitor);
    }
}

#[test]
fn a_different_pull_request_identity_still_fails() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let settings = store.load_settings().unwrap();
    let mut other = pull('a');
    other.base_repository_id = "200".into();
    let error = pull_gate(&settings, &run.job, &other).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Permanent);
}

#[test]
fn a_base_move_before_the_run_supersedes_and_after_it_tosses() {
    assert_eq!(base_changed(false).kind, OperationFailure::Superseded);
    assert_eq!(base_changed(true).kind, OperationFailure::Tossed);
}

#[test]
fn a_legacy_parked_revision_change_recovers_at_startup() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let primary = dispatch(&store, PRIMARY, NOW + 2);
    let mut reviews = store.load_reviews().unwrap();
    for (review, message) in reviews.iter_mut().zip([
        "Pull request revision or eligibility changed; wait for the next poll.",
        "Copilot returned invalid or incomplete review output.",
    ]) {
        review
            .operation
            .fail(&Failure::permanent(message).monitoring(), NOW + 3);
        review.phase = "Review stopped".into();
        review.error = Some(message.into());
    }
    store.save_reviews(&reviews).unwrap();
    let snapshot = crate::queue::snapshot(&store, monitor.snapshot()).unwrap();
    assert!(snapshot.items.iter().any(|i| i.state == State::Failed));

    crate::review::restore(&store).unwrap();

    let recovered = stored(&store, &run.operation.id);
    assert_eq!(recovered.operation.state, OperationState::Queued);
    assert_eq!(recovered.operation.attempt_count, 0);
    assert_eq!(recovered.operation.failure, None);
    assert_eq!(recovered.error, None);
    let untouched = stored(&store, &primary.operation.id);
    assert_eq!(untouched.operation.state, OperationState::Failed);
    assert_eq!(
        untouched.operation.failure,
        Some(OperationFailure::Permanent)
    );
    let next = prepare_dispatch(&store, &run.key, NOW + 10).unwrap();
    assert_eq!(next.operation.id, run.operation.id);
    assert_eq!(next.operation.attempt_count, 1);
}
