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
        // Retired work, started or not, leaves Running and ends quietly.
        assert!(running_for_pr(&store, NOW + 60).is_empty(), "{change}");
        assert!(dispatched(&store, NOW + 60).is_empty(), "{change}");
        let ended = stored(&store, &run.operation.id);
        assert!(ended.operation.ended_superseded(), "{change}");
        assert_eq!(ended.operation.attempt_count, 0);
        assert_quiet(&store, &monitor);
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
    assert!(stale.operation.ended_superseded());
    assert!(running_for_pr(&store, NOW + 86_400).is_empty());
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
    for later in [NOW + 161, NOW + 400] {
        assert!(!dispatched(&store, later).contains(&run.operation.id));
    }
    let resumed = stored(&store, &run.operation.id);
    assert_eq!(resumed.operation.state, OperationState::Running);
    assert_eq!(resumed.operation.attempt_count, 1);
    assert_eq!(resumed.job.head_sha, pull('a').head_sha);
    assert_quiet(&store, &monitor);
}

#[test]
fn an_older_intake_read_never_moves_the_observation_back() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    complete(&store, &run.operation.id, Err(base_changed(None)), NOW + 60).unwrap();
    // A scheduled poll prepared after supersession releases the run.
    scan(&mut monitor, &store, vec![pull('a')], NOW + 100);
    assert_eq!(reason(&store, &run.key, NOW + 150), None);
    // A +URL intake whose read began before supersession lands afterwards.
    let expected = store.load_settings().unwrap().repositories.remove(0);
    monitor
        .admit_explicit_pull_request(
            &store,
            &BTreeMap::from([(
                "22".into(),
                crate::monitoring::AccountAvailability {
                    login: "acting-account".into(),
                    connected: true,
                },
            )]),
            &expected,
            crate::github::intake::ResolvedTarget {
                connection: poll_result(vec![]).connection,
                pull_request: Some(pull('a')),
            },
            NOW + 50,
            NOW + 150,
        )
        .unwrap();
    let observed = store
        .load_queue_state()
        .unwrap()
        .tracked
        .into_iter()
        .find(|t| t.pull_request_id == "9")
        .unwrap()
        .observed_at;
    assert_eq!(observed, NOW + 100);
    assert_eq!(reason(&store, &run.key, NOW + 160), None);
    assert!(dispatched(&store, NOW + 160).contains(&run.operation.id));
}

/// Running entries for the fixture PR (normal work only).
fn running_for_pr(store: &Store, now: i64) -> Vec<String> {
    crate::capacity::candidates(store, now)
        .unwrap()
        .into_iter()
        .map(|w| w.key.id)
        .collect()
}

fn merge(monitor: &mut Monitor, store: &Store, at: i64) {
    let mut merged = pull('a');
    merged.state = Lifecycle::Merged;
    scan(monitor, store, vec![merged], at);
}

#[test]
fn a_queued_teammate_run_on_a_merged_pr_ends_quietly_and_leaves_running() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    complete(&store, &run.operation.id, Err(base_changed(None)), NOW + 3).unwrap();
    merge(&mut monitor, &store, NOW + 100);

    assert!(!dispatched(&store, NOW + 160).contains(&run.operation.id));
    let ended = stored(&store, &run.operation.id);
    assert!(ended.operation.ended_superseded());
    assert_eq!(ended.operation.attempt_count, 0);
    assert_eq!(ended.operation.next_attempt_at, None);
    assert!(ended.result.is_none());
    assert!(running_for_pr(&store, NOW + 200).is_empty());
    assert_quiet(&store, &monitor);
    let snapshot = crate::queue::snapshot(&store, monitor.snapshot()).unwrap();
    assert!(snapshot.items.iter().all(|i| i.state == State::Merged));
    assert!(store.load_publications().unwrap().is_empty());
}

#[test]
fn a_legacy_parked_run_on_a_merged_pr_ends_instead_of_requeueing() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let mut reviews = store.load_reviews().unwrap();
    let message = "Pull request revision or eligibility changed; wait for the next poll.";
    reviews[0]
        .operation
        .fail(&Failure::permanent(message).monitoring(), NOW + 3);
    reviews[0].error = Some(message.into());
    store.save_reviews(&reviews).unwrap();
    merge(&mut monitor, &store, NOW + 100);

    crate::review::restore(&store).unwrap();

    let ended = stored(&store, &run.operation.id);
    assert!(ended.operation.ended_superseded());
    assert_ne!(ended.operation.state, OperationState::Queued);
    assert!(ended.error.is_none());
    assert!(running_for_pr(&store, NOW + 200).is_empty());
    assert_quiet(&store, &monitor);
}

#[test]
fn owner_zombie_runs_on_merged_prs_clean_up_at_startup() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    merge(&mut monitor, &store, NOW + 100);
    // The installed build's shape: queued, never started, waiting for current state.
    let mut reviews = store.load_reviews().unwrap();
    let zombie = &mut reviews[0];
    zombie.operation.state = OperationState::Queued;
    zombie.operation.attempt_count = 0;
    zombie.operation.next_attempt_at = Some(0);
    zombie.operation.failure = None;
    zombie.operation.superseded = None;
    zombie.operation.ai_attempt = None;
    zombie.phase = "Waiting for current PR state".into();
    zombie.error = None;
    store.save_reviews(&reviews).unwrap();
    assert!(store
        .load_queue()
        .unwrap()
        .iter()
        .all(|j| j.waiting == crate::monitoring::WAITING_MERGED));

    crate::review::restore(&store).unwrap();

    let ended = stored(&store, &run.operation.id);
    assert!(ended.operation.ended_superseded());
    assert_eq!(ended.operation.attempt_count, 0);
    assert!(running_for_pr(&store, NOW + 200).is_empty());
    assert!(!dispatched(&store, NOW + 200).contains(&run.operation.id));
    assert_quiet(&store, &monitor);
}

#[test]
fn a_primary_never_waits_on_a_retired_teammate() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let primary = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|j| j.assignment_id.as_deref() == Some(PRIMARY))
        .unwrap();
    let primary_key = key(&primary, PRIMARY);
    let blocked = |store: &Store| {
        crate::capacity::candidates(store, NOW + 10)
            .unwrap()
            .into_iter()
            .find(|w| w.key.id == primary_key)
            .unwrap()
            .blocked_by
    };
    assert!(
        blocked(&store).is_some(),
        "barrier holds while the teammate is live"
    );
    let mut jobs = store.load_queue().unwrap();
    for job in &mut jobs {
        if job.assignment_id.as_deref() == Some(TEAMMATE) {
            job.waiting = crate::monitoring::WAITING_SCOPE_EXCLUDED.into();
        }
    }
    store.save_queue(&jobs).unwrap();
    assert!(blocked(&store).is_none());
    assert!(dispatched(&store, NOW + 10)
        .iter()
        .any(|id| stored(&store, id).assignment_id == PRIMARY));
}

#[test]
fn a_teammate_ended_for_a_transient_draft_reviews_again_when_ready() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    complete(&store, &run.operation.id, Err(base_changed(None)), NOW + 3).unwrap();
    let mut draft = pull('a');
    draft.draft = true;
    scan(&mut monitor, &store, vec![draft], NOW + 100);
    assert!(!dispatched(&store, NOW + 150).contains(&run.operation.id));
    assert!(stored(&store, &run.operation.id)
        .operation
        .ended_superseded());
    // Ready again on the same head: a fresh operation runs, the ended one stays history.
    scan(&mut monitor, &store, vec![pull('a')], NOW + 200);
    let started = dispatched(&store, NOW + 260);
    let fresh: Vec<_> = started
        .iter()
        .filter(|id| stored(&store, id).assignment_id == TEAMMATE)
        .collect();
    assert_eq!(fresh.len(), 1);
    assert_ne!(*fresh[0], run.operation.id);
    assert!(stored(&store, &run.operation.id)
        .operation
        .ended_superseded());
    assert_quiet(&store, &monitor);
}

fn no_failure_notice(store: &Store, monitor: &Monitor) {
    let snapshot = crate::queue::snapshot(store, monitor.snapshot()).unwrap();
    assert!(crate::notifications::frames(&snapshot)
        .iter()
        .filter_map(|f| f.event.as_ref())
        .all(|e| e.category != crate::notifications::Category::Failure));
}

#[test]
fn a_recovered_legacy_run_gets_a_full_retry_window() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    let mut reviews = store.load_reviews().unwrap();
    let message = "Pull request revision or eligibility changed; wait for the next poll.";
    reviews[0]
        .operation
        .fail(&Failure::permanent(message).monitoring(), NOW + 3);
    reviews[0].error = Some(message.into());
    store.save_reviews(&reviews).unwrap();

    let before = crate::now_seconds().unwrap();
    crate::review::restore(&store).unwrap();
    let recovered = stored(&store, &run.operation.id);
    // A fresh window from the real recovery time, not one that expired at epoch + 15 minutes.
    assert!(recovered.operation.initial_attempt_at >= before);
    assert!(recovered.operation.retry_deadline > before);

    let attempt = prepare_dispatch(&store, &run.key, NOW + 10).unwrap();
    complete(
        &store,
        &attempt.operation.id,
        Err(Failure::provider("Transient provider failure.")),
        NOW + 20,
    )
    .unwrap();
    let retrying = stored(&store, &run.operation.id);
    assert_eq!(retrying.operation.state, OperationState::Queued);
    assert!(!retrying.operation.parked());
    assert_quiet(&store, &monitor);
}

#[test]
fn a_removed_repository_ends_its_waiting_and_running_runs_quietly() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let waiting = dispatch(&store, TEAMMATE, NOW + 2);
    complete(
        &store,
        &waiting.operation.id,
        Err(base_changed(None)),
        NOW + 3,
    )
    .unwrap();
    let running = dispatch(&store, PRIMARY, NOW + 4);
    let mut settings = store.load_settings().unwrap();
    settings.repositories.clear();
    store.save_settings(&settings).unwrap();
    // The next monitoring tick marks the entries removed.
    assert!(monitor
        .prepare_checks(&store, NOW + 5, false)
        .unwrap()
        .is_empty());
    assert!(store
        .load_queue()
        .unwrap()
        .iter()
        .all(|j| j.waiting == crate::monitoring::WAITING_REPOSITORY_REMOVED));

    // The run still in flight is superseded, not failed.
    let error = crate::review::validate_execution_selection(&store, &running).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Superseded);
    complete(&store, &running.operation.id, Err(error), NOW + 5).unwrap();

    assert!(dispatched(&store, NOW + 60).is_empty());
    for id in [&waiting.operation.id, &running.operation.id] {
        let ended = stored(&store, id);
        assert!(ended.operation.ended_superseded());
        assert_eq!(ended.operation.attempt_count, 0);
    }
    assert!(running_for_pr(&store, NOW + 60).is_empty());
    assert_quiet(&store, &monitor);
    no_failure_notice(&store, &monitor);
}

#[test]
fn a_reopened_pr_starts_a_fresh_run_after_the_closed_one_ended() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    complete(&store, &run.operation.id, Err(base_changed(None)), NOW + 3).unwrap();
    let mut closed = pull('a');
    closed.state = Lifecycle::Closed;
    scan(&mut monitor, &store, vec![closed], NOW + 100);
    assert!(dispatched(&store, NOW + 150).is_empty());
    assert!(stored(&store, &run.operation.id)
        .operation
        .ended_superseded());

    scan(&mut monitor, &store, vec![pull('a')], NOW + 200);
    let started = dispatched(&store, NOW + 260);
    let teammate: Vec<_> = started
        .iter()
        .filter(|id| stored(&store, id).assignment_id == TEAMMATE)
        .collect();
    assert_eq!(teammate.len(), 1);
    assert_ne!(*teammate[0], run.operation.id);
    assert!(stored(&store, &run.operation.id)
        .operation
        .ended_superseded());
    assert_quiet(&store, &monitor);
}

#[test]
fn a_run_in_flight_at_merge_finishes_superseded_and_ends_next_cycle() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    merge(&mut monitor, &store, NOW + 100);
    // The worker's completion gate sees the retired entry.
    let error = crate::review::validate_execution_selection(&store, &run).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Superseded);
    complete(&store, &run.operation.id, Err(error), NOW + 150).unwrap();
    assert!(dispatched(&store, NOW + 155).is_empty());
    let ended = stored(&store, &run.operation.id);
    assert!(ended.operation.ended_superseded());
    assert_eq!(ended.operation.attempt_count, 0);
    assert!(ended.result.is_none());
    assert!(running_for_pr(&store, NOW + 160).is_empty());
    assert_quiet(&store, &monitor);
    no_failure_notice(&store, &monitor);
}

#[test]
fn superseded_and_merged_entries_leave_running_but_keep_their_history() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let run = dispatch(&store, TEAMMATE, NOW + 2);
    complete(&store, &run.operation.id, Err(base_changed(None)), NOW + 3).unwrap();
    scan(&mut monitor, &store, vec![pull('b')], NOW + 100);
    assert!(!dispatched(&store, NOW + 150).contains(&run.operation.id));
    assert!(stored(&store, &run.operation.id)
        .operation
        .ended_superseded());
    // A new revision preserves the old entries as history, not live work.
    let work = crate::capacity::candidates(&store, NOW + 160).unwrap();
    let superseded: Vec<_> = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.waiting == WAITING_SUPERSEDED)
        .collect();
    assert_eq!(superseded.len(), 2);
    for job in &superseded {
        let id = key(job, job.assignment_id.as_deref().unwrap());
        assert!(
            !work.iter().any(|w| w.key.id == id),
            "superseded entry stays in saved history, not Running"
        );
    }
    assert_quiet(&store, &monitor);
    // Once merged, every entry of the PR, superseded or not, leaves Running.
    let mut merged = pull('b');
    merged.state = Lifecycle::Merged;
    scan(&mut monitor, &store, vec![merged], NOW + 200);
    let work = crate::capacity::candidates(&store, NOW + 260).unwrap();
    let merged: Vec<_> = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.waiting == crate::monitoring::WAITING_MERGED)
        .collect();
    assert_eq!(merged.len(), 4);
    // Saved in-flight metadata is not a live coordinator reservation.
    let in_flight: Vec<_> = store
        .load_reviews()
        .unwrap()
        .into_iter()
        .filter(|r| r.operation.state == OperationState::Running)
        .map(|r| r.key)
        .collect();
    assert_eq!(in_flight.len(), 1);
    assert!(work.is_empty());
}

fn snapshot(
    head: char,
    checks: &[(&str, &str, &str)],
    comments: &[&str],
) -> crate::github::state::PrSnapshot {
    use crate::github::state::{SnapshotCheck, SnapshotComment};
    crate::github::state::PrSnapshot {
        number: 297,
        pull_request_id: "9".into(),
        title: "docs: remove stray merge marker".into(),
        head_ref: "feature".into(),
        head: head.to_string().repeat(40),
        base: "f".repeat(40),
        state: "OPEN".into(),
        mergeable: "MERGEABLE".into(),
        checks: checks
            .iter()
            .map(|(name, status, conclusion)| SnapshotCheck {
                name: (*name).into(),
                status: (*status).into(),
                conclusion: (*conclusion).into(),
            })
            .collect(),
        check_total: checks.len() as u64,
        comments: comments
            .iter()
            .map(|id| SnapshotComment {
                id: (*id).into(),
                body: "human".into(),
                commit: None,
            })
            .collect(),
        comment_total: comments.len() as u64,
        ..Default::default()
    }
}

fn scan_state(
    monitor: &mut Monitor,
    store: &Store,
    state: crate::github::state::PrSnapshot,
    at: i64,
) {
    let ticket = monitor.prepare_checks(store, at, true).unwrap().remove(0);
    let mut result = poll_result(vec![pull(state.head.chars().next().unwrap())]);
    result.states.insert("9".into(), state);
    monitor.finish(store, ticket, Ok(result), at + 1).unwrap();
}

fn finished(output: serde_json::Value) -> crate::review::ReviewResult {
    serde_json::from_value(json!({
        "reviewed_base_sha":"f".repeat(40),"output":output,
        "session_id":"fixture","model":"fixture-model","runtime_version":"fixture",
        "input_tokens":1,"output_tokens":1,"tool_calls":1
    }))
    .unwrap()
}

fn teammate_report() -> crate::review::ReviewResult {
    let mut result = finished(json!({}));
    result.output = crate::review::ReviewOutput::from_exit(
        crate::exits::ExitOutcome::SubmitReport,
        Some(crate::exits::TeammateReport { findings: vec![] }),
    );
    result
}

fn teammate_entries(store: &Store) -> usize {
    store
        .load_queue()
        .unwrap()
        .iter()
        .filter(|j| j.assignment_id.as_deref() == Some(TEAMMATE))
        .count()
}

fn seq(store: &Store, run: &ReviewRun) -> u64 {
    store
        .load_pr_states()
        .unwrap()
        .state_seq(&crate::retention::Binding::job(&run.job))
}

const PENDING: [(&str, &str, &str); 2] = [
    ("macos-native", "IN_PROGRESS", ""),
    ("browser (3/4)", "QUEUED", ""),
];
const ONE_DONE: [(&str, &str, &str); 2] = [
    ("macos-native", "COMPLETED", "SUCCESS"),
    ("browser (3/4)", "IN_PROGRESS", ""),
];
const GREEN: [(&str, &str, &str); 2] = [
    ("macos-native", "COMPLETED", "SUCCESS"),
    ("browser (3/4)", "COMPLETED", "SUCCESS"),
];

#[test]
fn check_runs_completing_on_an_unchanged_head_never_supersede_or_rerun_teammates() {
    let (_root, store, mut monitor) = configured(false);
    scan_state(&mut monitor, &store, snapshot('a', &PENDING, &[]), NOW);
    let teammate = dispatch(&store, TEAMMATE, NOW + 2);
    complete(
        &store,
        &teammate.operation.id,
        Ok(teammate_report()),
        NOW + 3,
    )
    .unwrap();
    let primary = dispatch(&store, PRIMARY, NOW + 4);
    let read = seq(&store, &primary);
    assert_eq!(primary.job.work.as_ref().unwrap().state_seq, read);

    for (at, checks) in [(NOW + 10, &ONE_DONE), (NOW + 20, &GREEN)] {
        let mut state = snapshot('a', checks, &[]);
        state.mergeable = "UNKNOWN".into();
        state.labels = vec!["ci-done".into()];
        scan_state(&mut monitor, &store, state, at);
        assert_eq!(seq(&store, &primary), read, "CI is not a new review state");
        assert_eq!(teammate_entries(&store), 1, "no teammate re-run");
        let running = stored(&store, &primary.operation.id);
        assert_eq!(running.operation.state, OperationState::Running);
        assert_eq!(running.operation.failure, None);
        assert_quiet(&store, &monitor);
    }
    // The primary's exit consumes the state it read: nothing moved under it.
    assert_eq!(
        crate::exits::consume::consume(&store, &primary.job, "after").unwrap(),
        crate::pr_state::Consumed::Consumed
    );

    // A head push still supersedes.
    let settings = store.load_settings().unwrap();
    let error = pull_gate(&settings, &primary.job, &pull('b')).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Superseded);
}

#[test]
fn a_new_human_comment_is_a_new_state_that_reruns_teammates() {
    let (_root, store, mut monitor) = configured(false);
    scan_state(&mut monitor, &store, snapshot('a', &PENDING, &[]), NOW);
    let teammate = dispatch(&store, TEAMMATE, NOW + 2);
    complete(
        &store,
        &teammate.operation.id,
        Ok(teammate_report()),
        NOW + 3,
    )
    .unwrap();
    let primary = dispatch(&store, PRIMARY, NOW + 4);
    let read = seq(&store, &primary);
    scan_state(
        &mut monitor,
        &store,
        snapshot('a', &PENDING, &["501"]),
        NOW + 10,
    );
    assert_eq!(seq(&store, &primary), read + 1);
    assert_eq!(
        teammate_entries(&store),
        2,
        "the teammate reviews the comment"
    );
    assert_eq!(
        crate::exits::consume::consume(&store, &primary.job, "after").unwrap(),
        crate::pr_state::Consumed::Moved
    );
}

#[test]
fn a_ci_wait_that_wakes_on_green_reuses_teammate_reports() {
    let (_root, store, _) = configured(false);
    let mut settings = store.load_settings().unwrap();
    let primary_assignment = &mut settings.repositories[0].assignments[0];
    primary_assignment.tools = Some(vec![
        "ready_for_human".into(),
        "approve_and_merge".into(),
        "wait".into(),
    ]);
    primary_assignment.actions =
        serde_json::from_value(json!({"reply":false,"approve":true,"merge":true})).unwrap();
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    let pending = snapshot('a', &PENDING, &[]);
    scan_state(&mut monitor, &store, pending.clone(), NOW);
    let teammate = dispatch(&store, TEAMMATE, NOW + 2);
    complete(
        &store,
        &teammate.operation.id,
        Ok(teammate_report()),
        NOW + 3,
    )
    .unwrap();
    let primary = dispatch(&store, PRIMARY, NOW + 4);
    complete(
        &store,
        &primary.operation.id,
        Ok(finished(json!({}))),
        NOW + 5,
    )
    .unwrap();
    let binding = crate::retention::Binding::job(&primary.job);
    let mut states = store.load_pr_states().unwrap();
    let normalized = pending.normalize();
    assert_eq!(
        states.wait(
            &binding,
            &normalized.hash(),
            crate::pr_state::ExternalWait {
                reason: crate::exits::WaitReason::Ci,
                state_seq: seq(&store, &primary),
                assignment_id: PRIMARY.into(),
                agent_id: primary.selection.agent.id.clone(),
                snapshot: normalized,
            },
        ),
        crate::pr_state::Consumed::Consumed
    );
    store.save_pr_states(&states).unwrap();
    let waited = seq(&store, &primary);

    scan_state(
        &mut monitor,
        &store,
        snapshot('a', &ONE_DONE, &[]),
        NOW + 10,
    );
    assert_eq!(seq(&store, &primary), waited, "progress keeps the wait");
    scan_state(&mut monitor, &store, snapshot('a', &GREEN, &[]), NOW + 20);
    assert_eq!(seq(&store, &primary), waited + 1, "green wakes the primary");
    assert_eq!(teammate_entries(&store), 1, "teammate report still holds");
    let settings = store.load_settings().unwrap();
    let candidates = candidates(&store).unwrap();
    let woken = candidates
        .iter()
        .find(|c| {
            c.assignment_id == PRIMARY
                && c.job.work.as_ref().unwrap().state_seq == waited + 1
                && c.run.is_none()
        })
        .expect("the primary gets one fresh pass");
    assert!(
        crate::review::teammates::blocker(&settings.repositories[0], &candidates, woken).is_none()
    );
}
