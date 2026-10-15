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
    monitor
        .finish(store, ticket, Ok(poll_result(pulls)), at + 1)
        .unwrap();
}

fn poll_result(pulls: Vec<PullRequest>) -> PollResult {
    PollResult {
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
    }
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

/// Operation ids the coordinator actually starts (each start runs full setup).
fn dispatched(store: &Store, now: i64) -> Vec<String> {
    crate::capacity::Coordinator::default()
        .dispatch(store, now)
        .unwrap()
        .dispatched
        .into_iter()
        .map(|crate::capacity::Dispatch::Review(run, _)| run.operation.id)
        .collect()
}

fn failed_poll(monitor: &mut Monitor, store: &Store, at: i64) {
    let ticket = monitor.prepare_checks(store, at, true).unwrap().remove(0);
    assert!(monitor
        .finish(
            store,
            ticket,
            Err(crate::github::ConnectionError::Network),
            at + 1,
        )
        .is_err());
}

fn reason(store: &Store, key: &str, now: i64) -> Option<String> {
    crate::capacity::candidates(store, now)
        .unwrap()
        .into_iter()
        .find(|w| w.key.id == key)
        .unwrap()
        .reason
}

#[test]
fn the_snapshot_gate_rejects_an_unadmitted_draft_before_any_setup() {
    for review_drafts in [false, true] {
        let (_root, store, mut monitor) = configured(review_drafts);
        scan(&mut monitor, &store, vec![pull('a')], NOW);
        let job = store.load_queue().unwrap().remove(0);
        let settings = store.load_settings().unwrap();
        let snapshot = crate::github::state::PrSnapshot {
            state: "OPEN".into(),
            head: job.head_sha.clone(),
            draft: true,
            ..Default::default()
        };
        let gate = snapshot_gate(&settings, &job, &snapshot);
        if review_drafts {
            gate.unwrap();
        } else {
            let error = gate.unwrap_err();
            assert_eq!(error.kind, OperationFailure::Superseded);
            assert_eq!(
                error.resume.as_deref(),
                Some(&crate::review::Resume::NextPoll)
            );
        }
    }
}

#[test]
fn a_superseded_draft_run_waits_for_a_poll_never_a_timer_and_retires_on_ineligible() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let mut draft = pull('a');
    draft.draft = true;
    let settings = store.load_settings().unwrap();
    let error = pull_gate(&settings, &run.job, &draft).unwrap_err();
    complete(&store, &run.operation.id, Err(error), NOW + 3).unwrap();

    // No timer re-dispatch: every start would rebuild context, packet and session.
    for later in [NOW + 33, NOW + 300, NOW + 86_400] {
        assert!(!dispatched(&store, later).contains(&run.operation.id));
        assert_eq!(
            reason(&store, &run.key, later).as_deref(),
            Some(crate::capacity::WAITING_FOR_POLL)
        );
    }
    // A failing poll observes nothing: still no re-dispatch, no loop.
    failed_poll(&mut monitor, &store, NOW + 100);
    for later in [NOW + 140, NOW + 300, NOW + 86_400] {
        assert!(!dispatched(&store, later).contains(&run.operation.id));
    }
    // The poll that observes the draft retires the entry; the run never restarts.
    scan(&mut monitor, &store, vec![draft], NOW + 400);
    assert_eq!(
        store
            .load_queue()
            .unwrap()
            .iter()
            .find(|j| j.assignment_id.as_deref() == Some(TEAMMATE))
            .unwrap()
            .waiting,
        crate::monitoring::WAITING_INELIGIBLE
    );
    for later in [NOW + 440, NOW + 86_400] {
        assert!(!dispatched(&store, later).contains(&run.operation.id));
    }
    let stale = stored(&store, &run.operation.id);
    assert_eq!(stale.operation.attempt_count, 0);
    assert!(!stale.operation.parked());
    assert_quiet(&store, &monitor);
}

#[test]
fn a_base_move_after_the_ai_run_requeues_once_per_new_base_and_never_parks() {
    let (_root, store, mut monitor) = configured(false);
    let mut current = pull('a');
    scan(&mut monitor, &store, vec![current.clone()], NOW);
    let first = dispatch(&store, TEAMMATE, NOW + 2);
    let id = first.operation.id.clone();
    let mut ai_runs = 1;
    let mut at = NOW + 10;
    for next_base in ['e', 'd', 'c', 'b'] {
        let reviewed = current.base_sha.clone();
        complete(&store, &id, Err(base_changed(Some(&reviewed))), at).unwrap();
        let waiting = stored(&store, &id);
        assert_eq!(waiting.operation.state, OperationState::Queued);
        assert_eq!(waiting.operation.attempt_count, 0);
        assert!(!waiting.operation.parked());
        assert_quiet(&store, &monitor);
        // No poll yet, or a poll that still sees the reviewed base: no AI attempt.
        assert!(!dispatched(&store, at + 60).contains(&id));
        scan(&mut monitor, &store, vec![current.clone()], at + 100);
        assert!(!dispatched(&store, at + 160).contains(&id));
        failed_poll(&mut monitor, &store, at + 200);
        assert!(!dispatched(&store, at + 260).contains(&id));
        // A poll that observes a new base earns exactly one attempt.
        current.base_sha = next_base.to_string().repeat(40);
        scan(&mut monitor, &store, vec![current.clone()], at + 300);
        let started = dispatched(&store, at + 360);
        assert_eq!(started.iter().filter(|s| **s == id).count(), 1);
        ai_runs += 1;
        assert_eq!(stored(&store, &id).operation.attempt_count, 1);
        assert!(!dispatched(&store, at + 361).contains(&id));
        at += 1_000;
    }
    assert_eq!(ai_runs, 5, "one AI attempt per distinct base");
    assert!(store.load_publications().unwrap().is_empty());
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

#[test]
fn a_poll_fetched_before_supersession_never_releases_the_run() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    // Prepared (and fetched) at t-1, finished at t+3; the run is superseded at t+1.
    let t = NOW + 10;
    let in_flight = monitor
        .prepare_checks(&store, t - 1, true)
        .unwrap()
        .remove(0);
    complete(&store, &run.operation.id, Err(base_changed(None)), t + 1).unwrap();
    monitor
        .finish(&store, in_flight, Ok(poll_result(vec![pull('a')])), t + 3)
        .unwrap();
    assert!(!dispatched(&store, t + 60).contains(&run.operation.id));
    assert_eq!(
        reason(&store, &run.key, t + 60).as_deref(),
        Some(crate::capacity::WAITING_FOR_POLL)
    );
    // The next poll, prepared after supersession, releases it.
    scan(&mut monitor, &store, vec![pull('a')], t + 100);
    assert!(dispatched(&store, t + 160).contains(&run.operation.id));
}

#[test]
fn a_superseded_run_whose_pr_is_eligible_again_runs_after_the_next_poll() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    // The gate saw a transient draft; the same head is ready again by the next poll.
    let mut draft = pull('a');
    draft.draft = true;
    let settings = store.load_settings().unwrap();
    let error = pull_gate(&settings, &run.job, &draft).unwrap_err();
    assert_eq!(
        error.resume.as_deref(),
        Some(&crate::review::Resume::NextPoll)
    );
    complete(&store, &run.operation.id, Err(error), NOW + 3).unwrap();
    assert!(!dispatched(&store, NOW + 60).contains(&run.operation.id));

    scan(&mut monitor, &store, vec![pull('a')], NOW + 100);
    let started = dispatched(&store, NOW + 160);
    assert_eq!(
        started.iter().filter(|id| **id == run.operation.id).count(),
        1
    );
    let resumed = stored(&store, &run.operation.id);
    assert_eq!(resumed.operation.state, OperationState::Running);
    assert_eq!(resumed.operation.attempt_count, 1);
    assert_eq!(resumed.job.head_sha, pull('a').head_sha);
    assert_quiet(&store, &monitor);
}
