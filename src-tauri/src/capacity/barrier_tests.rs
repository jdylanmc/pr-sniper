//! Teammate barrier: the primary of a PR state waits for every teammate's report.

use super::{tests::fixture, *};
use crate::{
    exits::{ExitOutcome, ReportFinding, TeammateReport},
    monitoring::{OperationFailure, QueueJob},
    review::{teammates::BlockedBy, Failure, ReviewOutput, ReviewResult},
    storage::{Settings, Store},
};
use serde_json::json;

/// One repository with a packet-mode primary (FIFO first) and `count` teammates.
fn team(count: usize, designated: bool) -> (tempfile::TempDir, Store) {
    let (root, store) = fixture(1, 4, true);
    let mut settings = store.load_settings().unwrap();
    let mut jobs = store.load_queue().unwrap();
    let primary_id = settings.repositories[0].assignments[0].id.clone();
    settings.repositories[0].assignments[0].tools =
        Some(vec!["read_changes".into(), "ready_for_human".into()]);
    if designated {
        settings.repositories[0].primary_assignment_id = Some(primary_id);
    }
    for n in 0..count {
        let agent = format!("aaaaaaaa-aaaa-4aaa-8aaa-9{n:011}");
        let assignment = format!("cccccccc-cccc-4ccc-8ccc-9{n:011}");
        let mut agent_value = serde_json::to_value(&settings.agents[0]).unwrap();
        agent_value["id"] = json!(agent);
        agent_value["name"] = json!(format!("Teammate {n}"));
        settings
            .agents
            .push(serde_json::from_value(agent_value).unwrap());
        let mut assignment_value =
            serde_json::to_value(&settings.repositories[0].assignments[0]).unwrap();
        assignment_value["id"] = json!(assignment);
        assignment_value["agent_id"] = json!(agent);
        assignment_value["tools"] = json!(["read_changes", "submit_report"]);
        settings.repositories[0]
            .assignments
            .push(serde_json::from_value(assignment_value).unwrap());
        let mut job = jobs[0].clone();
        job.assignment_id = Some(assignment);
        let work = job.work.as_mut().unwrap();
        work.id = format!("teammate-{n}");
        work.agent_id = agent;
        work.enqueue_order = 2 + n as u64;
        jobs.push(job);
    }
    store.save_settings(&settings).unwrap();
    store.save_queue(&jobs).unwrap();
    (root, store)
}

fn report_result() -> ReviewResult {
    let mut result: ReviewResult = serde_json::from_value(json!({
        "reviewed_base_sha":"b".repeat(40),
        "output":{},
        "session_id":"fixture","model":"fixture-model","runtime_version":"fixture",
        "input_tokens":1,"output_tokens":1,"tool_calls":1
    }))
    .unwrap();
    result.output = ReviewOutput::from_exit(
        ExitOutcome::SubmitReport,
        Some(TeammateReport {
            findings: vec![ReportFinding {
                file: "src/lib.rs".into(),
                line: 3,
                nano: "bad".into(),
                full: "bad. very".into(),
                fix: "fix".into(),
            }],
        }),
    );
    result
}

fn finish(
    coordinator: &Coordinator,
    store: &Store,
    work: Dispatch,
    outcome: Result<ReviewResult, Failure>,
    now: i64,
) {
    let key = work.key();
    let Dispatch::Review(run, _) = work;
    crate::review::host::complete(store, &run.operation.id, outcome, now).unwrap();
    coordinator.release(&key, &run.operation.id).unwrap();
}

fn dispatched(coordinator: &Coordinator, store: &Store, now: i64) -> Vec<Dispatch> {
    let batch = coordinator.dispatch(store, now).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    batch.dispatched
}

fn primary(store: &Store) -> Work {
    candidates(store, 100)
        .unwrap()
        .into_iter()
        .find(|w| w.key.id == "normal-1")
        .unwrap()
}

fn waiting_on(work: &Work) -> Option<String> {
    work.blocked_by
        .as_ref()
        .map(|BlockedBy::Teammate(t)| t.agent_name.clone())
}

#[test]
fn primary_waits_for_every_teammate_report_while_teammates_run_fifo() {
    let (_root, store) = team(2, true);
    let coordinator = Coordinator::default();
    let started = dispatched(&coordinator, &store, 100);
    let ids: Vec<_> = started.iter().map(|d| d.key().id).collect();
    assert_eq!(ids, ["teammate-0", "teammate-1"]);
    let blocked = primary(&store);
    assert_eq!(blocked.state, "blocked");
    assert_eq!(
        blocked.reason.as_deref(),
        Some(crate::review::teammates::WAITING_FOR_TEAMMATES)
    );
    assert_eq!(waiting_on(&blocked).as_deref(), Some("Teammate 0"));
    let mut started = started.into_iter();
    finish(
        &coordinator,
        &store,
        started.next().unwrap(),
        Ok(report_result()),
        101,
    );
    assert!(dispatched(&coordinator, &store, 102).is_empty());
    assert_eq!(waiting_on(&primary(&store)).as_deref(), Some("Teammate 1"));
    finish(
        &coordinator,
        &store,
        started.next().unwrap(),
        Ok(report_result()),
        103,
    );
    let next = dispatched(&coordinator, &store, 104);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].key().id, "normal-1");
}

#[test]
fn a_teammate_without_a_report_blocks_even_when_it_completed_without_one() {
    let (_root, store) = team(1, true);
    let coordinator = Coordinator::default();
    let mut started = dispatched(&coordinator, &store, 100);
    let work = started.remove(0);
    let mut result = report_result();
    result.output.report = None;
    finish(&coordinator, &store, work, Ok(result), 101);
    assert!(dispatched(&coordinator, &store, 102).is_empty());
    assert_eq!(waiting_on(&primary(&store)).as_deref(), Some("Teammate 0"));
}

#[test]
fn teammate_failure_keeps_the_primary_blocked_and_successful_reports_are_reused_on_retry() {
    let (_root, store) = team(2, true);
    let coordinator = Coordinator::default();
    let mut started = dispatched(&coordinator, &store, 100).into_iter();
    let ok = started.next().unwrap();
    let ok_operation = match &ok {
        Dispatch::Review(run, _) => run.operation.id.clone(),
    };
    finish(&coordinator, &store, ok, Ok(report_result()), 101);
    finish(
        &coordinator,
        &store,
        started.next().unwrap(),
        Err(Failure::permanent("Agent configuration is invalid.")),
        102,
    );
    // Parked for the human: nothing runs, the primary stays blocked on the failed teammate.
    assert!(dispatched(&coordinator, &store, 100_000).is_empty());
    assert_eq!(waiting_on(&primary(&store)).as_deref(), Some("Teammate 1"));
    let runs = store.load_reviews().unwrap();
    let failed = runs
        .iter()
        .find(|r| r.operation.state == OperationState::Failed)
        .unwrap();
    assert_eq!(failed.operation.failure, Some(OperationFailure::Permanent));
    // Retry only the failed teammate.
    let mut again = runs.clone();
    let retry = again
        .iter_mut()
        .find(|r| r.operation.state == OperationState::Failed)
        .unwrap();
    retry.operation.state = OperationState::Queued;
    retry.operation.next_attempt_at = Some(100);
    retry.operation.failure = None;
    retry.error = None;
    store.save_reviews(&again).unwrap();
    let retried = dispatched(&coordinator, &store, 200);
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].key().id, "teammate-1");
    let kept = store.load_reviews().unwrap();
    assert!(kept
        .iter()
        .any(|r| r.operation.id == ok_operation && r.operation.state == OperationState::Completed));
    finish(
        &coordinator,
        &store,
        retried.into_iter().next().unwrap(),
        Ok(report_result()),
        201,
    );
    let next = dispatched(&coordinator, &store, 202);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].key().id, "normal-1");
}

#[test]
fn a_moved_state_reruns_every_teammate_and_old_reports_do_not_count() {
    let (_root, store) = team(1, true);
    let coordinator = Coordinator::default();
    let work = dispatched(&coordinator, &store, 100).remove(0);
    finish(&coordinator, &store, work, Ok(report_result()), 101);
    // State moves: the primary entry is retargeted, teammates get new entries.
    let mut jobs: Vec<QueueJob> = store.load_queue().unwrap();
    let mut next = jobs[1].clone();
    next.work.as_mut().unwrap().id = "teammate-0-seq1".into();
    next.work.as_mut().unwrap().state_seq = 1;
    jobs[0].work.as_mut().unwrap().state_seq = 1;
    jobs.push(next);
    store.save_queue(&jobs).unwrap();
    assert_eq!(waiting_on(&primary(&store)).as_deref(), Some("Teammate 0"));
    let started = dispatched(&coordinator, &store, 103);
    assert_eq!(
        started.iter().map(|d| d.key().id).collect::<Vec<_>>(),
        ["teammate-0-seq1"]
    );
}

#[test]
fn sole_assignment_and_unassigned_primary_have_no_barrier() {
    let (_root, store) = fixture(1, 4, true);
    let coordinator = Coordinator::default();
    assert!(primary(&store).blocked_by.is_none());
    assert_eq!(dispatched(&coordinator, &store, 100).len(), 1);

    let (_root, store) = team(2, false);
    let coordinator = Coordinator::default();
    assert_eq!(dispatched(&coordinator, &store, 100).len(), 3);
}

#[test]
fn a_primary_on_default_tools_is_held_by_the_barrier() {
    let (_root, store) = team(1, true);
    let mut settings: Settings = store.load_settings().unwrap();
    for assignment in &mut settings.repositories[0].assignments {
        assignment.tools = None;
    }
    store.save_settings(&settings).unwrap();
    assert!(primary(&store).blocked_by.is_some());
}

fn fail_teammate(store: &Store, coordinator: &Coordinator, failure: Failure) {
    let started = dispatched(coordinator, store, 100);
    let mut started = started.into_iter();
    let work = started.next().unwrap();
    finish(coordinator, store, work, Err(failure), 101);
}

#[test]
fn queue_shows_one_row_naming_the_failed_teammate_with_a_typed_cause() {
    for (failure, cause) in [
        (
            Failure::permanent("Invalid configuration."),
            OperationFailure::Permanent,
        ),
        (Failure::panic("stuck"), OperationFailure::Panic),
    ] {
        let (_root, store) = team(1, true);
        fail_teammate(&store, &Coordinator::default(), failure);
        let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
        assert_eq!(snapshot.items.len(), 1);
        let item = &snapshot.items[0];
        assert_eq!(item.state, crate::queue::State::Failed);
        let failed = item.failed_teammate.as_ref().unwrap();
        assert_eq!(failed.agent_name, "Teammate 0");
        assert_eq!(failed.cause, cause);
        assert!(matches!(item.blocked_by, Some(BlockedBy::Teammate(_))));
    }
}

#[test]
fn queue_primary_shows_blocked_while_a_teammate_is_pending_without_failure() {
    let (_root, store) = team(1, true);
    let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items.len(), 1);
    let item = &snapshot.items[0];
    assert!(item.failed_teammate.is_none());
    assert!(matches!(
        &item.blocked_by,
        Some(BlockedBy::Teammate(t)) if t.agent_name == "Teammate 0"
    ));
    assert_eq!(item.state, crate::queue::State::Queued);
}

#[test]
fn a_finished_report_clears_the_blocked_chip_and_feeds_the_primary_packet() {
    let (_root, store) = team(2, true);
    let coordinator = Coordinator::default();
    for work in dispatched(&coordinator, &store, 100) {
        finish(&coordinator, &store, work, Ok(report_result()), 101);
    }
    let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
    assert!(snapshot.items[0].blocked_by.is_none());
    let run = crate::review::host::request(&store, "normal-1", false, 102).unwrap();
    let entries = crate::review::teammates::entries(&store, &run).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.agent.as_str()).collect::<Vec<_>>(),
        ["Teammate 0", "Teammate 1"]
    );
    assert_eq!(entries[0].report.findings[0].nano, "bad");

    // A teammate whose report is not for this state is never papered over.
    let mut jobs = store.load_queue().unwrap();
    jobs[0].work.as_mut().unwrap().state_seq = 1;
    store.save_queue(&jobs).unwrap();
    assert!(crate::review::teammates::entries(&store, &run).is_err());
}

#[test]
fn retry_all_rearms_every_parked_teammate_on_a_tossed_item_and_counts_reviews() {
    for second in [OperationFailure::Provider, OperationFailure::Tossed] {
        let (_root, store) = team(2, true);
        let coordinator = Coordinator::default();
        let started = dispatched(&coordinator, &store, 100);
        assert_eq!(started.len(), 2, "both teammates run before the primary");
        for work in started {
            finish(
                &coordinator,
                &store,
                work,
                Err(Failure::provider("refused")),
                101,
            );
        }
        let mut reviews = store.load_reviews().unwrap();
        for (run, failure) in reviews
            .iter_mut()
            .zip([OperationFailure::Tossed, second.clone()])
        {
            run.operation.state = crate::monitoring::OperationState::ManualRetry;
            run.operation.failure = Some(failure);
            run.operation.next_attempt_at = None;
        }
        let parked: Vec<_> = reviews.iter().map(|r| r.operation.id.clone()).collect();
        store.save_reviews(&reviews).unwrap();
        let item = crate::queue::snapshot(&store, vec![])
            .unwrap()
            .items
            .remove(0);
        assert_eq!(item.failure_cause, Some(crate::queue::CAUSE_TOSSED));

        assert_eq!(
            crate::review::host::retry_all_parked(&store, 102).unwrap(),
            (2, 0),
            "{second:?}"
        );
        let live: Vec<_> = store
            .load_reviews()
            .unwrap()
            .into_iter()
            .filter(|r| r.operation.state == crate::monitoring::OperationState::Queued)
            .collect();
        assert_eq!(live.len(), 2, "both teammates get new operations");
        assert!(live.iter().all(|r| !parked.contains(&r.operation.id)));
    }
}

#[test]
fn retry_all_counts_a_partly_applied_item_retry_per_review() {
    let (_root, store) = team(2, true);
    let coordinator = Coordinator::default();
    for work in dispatched(&coordinator, &store, 100) {
        finish(
            &coordinator,
            &store,
            work,
            Err(Failure::provider("refused")),
            101,
        );
    }
    let mut reviews = store.load_reviews().unwrap();
    for run in &mut reviews {
        run.operation.state = crate::monitoring::OperationState::ManualRetry;
        run.operation.failure = Some(OperationFailure::Tossed);
        run.operation.next_attempt_at = None;
    }
    store.save_reviews(&reviews).unwrap();
    // The second teammate's Agent lost its AI account: its retry is refused.
    let mut settings = store.load_settings().unwrap();
    settings
        .agents
        .iter_mut()
        .find(|agent| agent.name == "Teammate 1")
        .unwrap()
        .ai_account = None;
    store.save_settings(&settings).unwrap();

    assert_eq!(
        crate::review::host::retry_all_parked(&store, 102).unwrap(),
        (1, 1)
    );
}
