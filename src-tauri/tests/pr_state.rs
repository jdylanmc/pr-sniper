use pr_sniper_lib::{
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{Capabilities, CommentCapability, Connection, RemoteRepository},
        state::{PrSnapshot, SnapshotCheck, SnapshotComment},
        Identity,
    },
    monitoring::{
        ActivationMode, JobOperation, Monitor, MonitoringActivation, OperationState, PollResult,
        QueueJob,
    },
    pr_state::{self, Consumed},
    queue::{self, State},
    retention::Binding,
    review::{self, ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::json;
use std::collections::BTreeMap;

mod support;
use support::Fixture;

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const NOW: i64 = 1_800_000_000;

fn configured(review_drafts: bool) -> (Fixture, Store, Monitor) {
    let fixture = Fixture::new();
    let store = fixture.store();
    let agent = "aaaaaaaa-aaaa-4aaa-8aaa-000000000001";
    let settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false, "doctrines":[],
        "agents":[{"id":agent,"name":"Agent","model":"explicit-model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Find actionable defects.","signature":"fixture"}],
        "repositories":[{
            "id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "review_drafts":review_drafts,
            "assignments":[{"id":"cccccccc-cccc-4ccc-8ccc-000000000001","agent_id":agent,
                "schedule":{"kind":"interval","minutes":2,"timezone":"UTC"},"comment":true}]
        }]
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
    (fixture, store, monitor)
}

fn pull(state: Lifecycle, draft: bool) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 101,
        title: "Review".into(),
        author: Some(Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state,
        draft,
        head_sha: "a".repeat(40),
        base_sha: "f".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-09-30T00:00:00Z".into(),
        files: vec![],
    }
}

fn snapshot(body: &str) -> PrSnapshot {
    PrSnapshot {
        required_checks: vec![],
        number: 101,
        pull_request_id: "9".into(),
        title: "Review".into(),
        body: body.into(),
        labels: vec![],
        head_ref: "feature".into(),
        head: "a".repeat(40),
        base: "f".repeat(40),
        state: "OPEN".into(),
        draft: false,
        mergeable: "MERGEABLE".into(),
        review_decision: None,
        reviews: vec![],
        checks: vec![SnapshotCheck {
            name: "ci".into(),
            status: "COMPLETED".into(),
            conclusion: "SUCCESS".into(),
        }],
        check_total: 1,
        threads: vec![],
        thread_total: 0,
        comments: vec![SnapshotComment {
            id: "1".into(),
            body: "hello".into(),
            commit: None,
        }],
        comment_total: 1,
    }
}

fn binding() -> Binding {
    Binding {
        provider: "github".into(),
        configuration_id: REPO.into(),
        account_id: "22".into(),
        repository_id: "100".into(),
        pull_request_id: "9".into(),
    }
}

fn scan(
    monitor: &mut Monitor,
    store: &Store,
    pull: PullRequest,
    state: Option<PrSnapshot>,
    at: i64,
) {
    let tickets = monitor.prepare_checks(store, at, true).unwrap();
    assert_eq!(tickets.len(), 1);
    let result = PollResult {
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
        pull_requests: vec![pull],
        states: state.into_iter().map(|s| ("9".to_string(), s)).collect(),
    };
    monitor
        .finish(store, tickets[0].clone(), Ok(result), at + 1)
        .unwrap();
}

fn open(draft: bool) -> PullRequest {
    pull(Lifecycle::Open, draft)
}

fn completed(settings: &Settings, job: &QueueJob) -> ReviewRun {
    let assignment = job.assignment_id.as_ref().unwrap();
    let mut operation = JobOperation::review(job, NOW + 1);
    operation.begin_attempt(NOW + 1).unwrap();
    operation.state = OperationState::Completed;
    ReviewRun {
        mode: pr_sniper_lib::review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: review::key(job, assignment),
        assignment_id: assignment.clone(),
        job: job.clone(),
        selection: Selection::resolve(settings, job, assignment)
            .unwrap()
            .into(),
        operation,
        manual_start: true,
        trust_confirmed: false,
        phase: "Completed fixture".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({
                "reviewed_base_sha":"f".repeat(40),
                "output":{"synopsis":"No actionable defects were found.","files":[
                    {"path":"source.rs","explanation":"Reviewed source.","order":1}
                ],"findings":[],"decision":"machine_sign_off"},
                "session_id":"fixture-session","model":"explicit-model","runtime_version":"fixture",
                "input_tokens":10,"output_tokens":20,"tool_calls":1
            }))
            .unwrap(),
        ),
    }
}

fn seqs(store: &Store) -> Vec<u64> {
    store
        .load_queue()
        .unwrap()
        .iter()
        .map(|j| j.work.as_ref().unwrap().state_seq)
        .collect()
}

#[test]
fn a_state_that_moves_while_queued_keeps_one_entry() {
    let (_fixture, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, open(false), None, NOW);
    assert_eq!(seqs(&store), [0]);
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 10,
    );
    assert_eq!(seqs(&store), [1]);
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("b")),
        NOW + 20,
    );
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("c")),
        NOW + 30,
    );
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].work.as_ref().unwrap().state_seq, 3);
    assert_eq!(jobs[0].work.as_ref().unwrap().iteration, 1);
    let tracked = store.load_queue_state().unwrap().tracked;
    assert_eq!(tracked[0].iteration, 1);
    let states = store.load_pr_states().unwrap();
    assert_eq!(
        states
            .get(&binding())
            .unwrap()
            .pending
            .as_ref()
            .unwrap()
            .seq,
        3
    );
    assert_eq!(queue::snapshot(&store, vec![]).unwrap().items.len(), 1);
}

#[test]
fn a_changed_state_after_a_completed_pass_adds_one_entry_that_masks_the_old_result() {
    let (_fixture, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, open(false), None, NOW);
    let settings = store.load_settings().unwrap();
    let job = store.load_queue().unwrap().remove(0);
    store.save_reviews(&[completed(&settings, &job)]).unwrap();
    // First snapshot of an already reviewed head is adopted, not re-run.
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 10,
    );
    assert_eq!(seqs(&store), [0]);
    assert!(store
        .load_pr_states()
        .unwrap()
        .get(&binding())
        .unwrap()
        .baseline_hash
        .is_some());
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 20,
    );
    assert_eq!(seqs(&store), [0]);
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("b")),
        NOW + 30,
    );
    assert_eq!(seqs(&store), [0, 1]);
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs[1].work.as_ref().unwrap().pass_ordinal, 2);
    assert_eq!(jobs[1].work.as_ref().unwrap().iteration, 1);
    let items = queue::snapshot(&store, vec![]).unwrap().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Queued);
}

#[test]
fn a_state_that_moves_during_a_run_is_tossed_and_counted() {
    let (_fixture, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, open(false), None, NOW);
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 10,
    );
    let started = store.load_pr_states().unwrap().state_seq(&binding());
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("b")),
        NOW + 20,
    );
    let moved = snapshot("b").normalize().hash();
    assert_eq!(
        pr_state::consume(&store, &binding(), started, &moved).unwrap(),
        Consumed::Moved
    );
    assert_eq!(pr_state::toss(&store, &binding()).unwrap(), 1);
    assert_eq!(store.load_queue().unwrap().len(), 1);
    let current = store.load_pr_states().unwrap().state_seq(&binding());
    assert_eq!(
        pr_state::consume(&store, &binding(), current, &moved).unwrap(),
        Consumed::Consumed
    );
    // The consumed state is the baseline: the same read queues nothing.
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("b")),
        NOW + 30,
    );
    let record = store.load_pr_states().unwrap();
    assert!(record.get(&binding()).unwrap().pending.is_none());
    assert_eq!(record.get(&binding()).unwrap().error_count, 0);
}

#[test]
fn baseline_and_pending_survive_a_restart_without_a_new_review() {
    let (fixture, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, open(false), None, NOW);
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 10,
    );
    assert!(fixture.path().join("state/pr-state.json").exists());
    let before = store.load_pr_states().unwrap();
    let jobs = store.load_queue().unwrap();
    drop(monitor);
    let mut monitor = Monitor::restore(&store).unwrap();
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 20,
    );
    assert_eq!(store.load_pr_states().unwrap(), before);
    assert_eq!(store.load_queue().unwrap(), jobs);
}

#[test]
fn closed_or_merged_before_its_turn_leaves_the_queue_and_needs_no_run() {
    for (lifecycle, waiting) in [(Lifecycle::Merged, "merged"), (Lifecycle::Closed, "closed")] {
        let (_fixture, store, mut monitor) = configured(false);
        scan(&mut monitor, &store, open(false), None, NOW);
        scan(
            &mut monitor,
            &store,
            open(false),
            Some(snapshot("a")),
            NOW + 10,
        );
        scan(&mut monitor, &store, pull(lifecycle, false), None, NOW + 20);
        let jobs = store.load_queue().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].waiting, waiting);
        let items = queue::snapshot(&store, vec![]).unwrap().items;
        assert!(matches!(items[0].state, State::Merged | State::Closed));
    }
}

#[test]
fn drafts_wait_until_the_repository_opts_in() {
    let (_fixture, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, open(true), None, NOW);
    assert!(store.load_queue().unwrap().is_empty());
    scan(&mut monitor, &store, open(false), None, NOW + 10);
    assert_eq!(store.load_queue().unwrap().len(), 1);

    let (_fixture, store, mut monitor) = configured(true);
    scan(&mut monitor, &store, open(true), None, NOW);
    assert_eq!(store.load_queue().unwrap().len(), 1);
}

/// A completed leave_comments exit for the job's current state.
fn comments_exit(settings: &Settings, job: &QueueJob) -> ReviewRun {
    let mut run = completed(settings, job);
    run.result = Some(
        serde_json::from_value(json!({"reviewed_base_sha":"f".repeat(40),
            "output":{"exit":{"exit":"comments",
                "new_comments":[{"path":"source.rs","line":1,"body":"Defect."}],
                "reply_comments":[{"thread_id":"T1","body":"Fixed."}],
                "resolve_own_threads":[]}},
            "session_id":"s","model":"m","runtime_version":"v",
            "input_tokens":1,"output_tokens":1,"tool_calls":1}))
        .unwrap(),
    );
    run
}

#[test]
fn own_writes_in_flight_never_bump_the_state_and_a_later_external_change_does() {
    let (_fixture, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, open(false), None, NOW);
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("a")),
        NOW + 10,
    );
    assert_eq!(seqs(&store), [1]);
    let settings = store.load_settings().unwrap();
    let job = store.load_queue().unwrap().remove(0);
    let run = comments_exit(&settings, &job);
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    // Batch posting and the reply write are in flight.
    let publication =
        pr_sniper_lib::publication::Publication::new(run.clone(), true, true, NOW).unwrap();
    store
        .save_publications(std::slice::from_ref(&publication))
        .unwrap();
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes = vec![pr_sniper_lib::exit_writes::plan(&run, NOW).unwrap()];
    store.save_feedback(&ledger).unwrap();
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("own post")),
        NOW + 20,
    );
    assert_eq!(seqs(&store), [1], "own posts never retrigger");
    assert_eq!(store.load_pr_states().unwrap().state_seq(&binding()), 1);
    // The writes settle; the exit consumes the post-write state.
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes[0].phase = pr_sniper_lib::exit_writes::Phase::Completed;
    ledger.exit_writes[0].consumed = true;
    store.save_feedback(&ledger).unwrap();
    let mut publications = store.load_publications().unwrap();
    publications[0].phase = pr_sniper_lib::publication::Phase::Published;
    publications[0].operation.state = OperationState::Completed;
    store.save_publications(&publications).unwrap();
    let after = snapshot("own post").normalize().hash();
    assert_eq!(
        pr_state::consume(&store, &binding(), 1, &after).unwrap(),
        Consumed::Consumed
    );
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("own post")),
        NOW + 30,
    );
    assert!(store
        .load_pr_states()
        .unwrap()
        .get(&binding())
        .unwrap()
        .pending
        .is_none());
    // A human change after the writes settled is still a new state.
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(snapshot("human")),
        NOW + 40,
    );
    assert_eq!(store.load_pr_states().unwrap().state_seq(&binding()), 2);
}

/// Snapshot `a` plus one thread rooted in `comment_id`.
fn with_thread(mut s: PrSnapshot, thread: &str, comment_id: &str) -> PrSnapshot {
    s.threads
        .push(pr_sniper_lib::github::state::SnapshotThread {
            id: thread.into(),
            resolved: false,
            outdated: false,
            comments: vec![SnapshotComment {
                id: comment_id.into(),
                body: format!("comment {comment_id}"),
                commit: None,
            }],
            comment_total: 1,
        });
    s.thread_total += 1;
    s
}

/// A leave_comments exit that read snapshot `a` and whose batch posted comment 500.
fn posted_exit(store: &Store, monitor: &mut Monitor) -> (QueueJob, ReviewRun) {
    scan(monitor, store, open(false), None, NOW);
    scan(monitor, store, open(false), Some(snapshot("a")), NOW + 10);
    let settings = store.load_settings().unwrap();
    let job = store.load_queue().unwrap().remove(0);
    let mut run = comments_exit(&settings, &job);
    run.packet = Some(pr_sniper_lib::packet::PacketRecord {
        state_id: "1:a".into(),
        hash: "h".into(),
        schema: 2,
        tool_set: vec![],
        exits_offered: vec![],
        exits_denied: vec![],
        state_hash: Some(snapshot("a").normalize().hash()),
        wait_snapshot: None,
        state_marks: Some(pr_sniper_lib::github::state::StateMarks::of(
            &snapshot("a"),
            "acting-account",
        )),
    });
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    let mut publication =
        pr_sniper_lib::publication::Publication::new(run.clone(), true, true, NOW).unwrap();
    publication.phase = pr_sniper_lib::publication::Phase::Published;
    publication.operation.state = OperationState::Completed;
    publication.receipts = vec![pr_sniper_lib::publication::Receipt {
        review_id: "42".into(),
        state: pr_sniper_lib::publication::RemoteState::Commented,
        comment_ids: vec!["500".into()],
    }];
    store.save_publications(&[publication]).unwrap();
    (job, run)
}

fn record(store: &Store) -> pr_state::PrStateRecord {
    store
        .load_pr_states()
        .unwrap()
        .get(&binding())
        .cloned()
        .unwrap()
}

#[test]
fn own_writes_only_baseline_the_fresh_state_and_queue_nothing() {
    let (_fixture, store, mut monitor) = configured(false);
    let (job, run) = posted_exit(&store, &mut monitor);
    let fresh = with_thread(snapshot("a"), "T9", "500");
    assert_eq!(
        pr_sniper_lib::exits::consume::settle_fresh(&store, &job, &run.operation.id, &fresh)
            .unwrap(),
        Consumed::Consumed
    );
    assert_eq!(record(&store).baseline_hash, Some(fresh.normalize().hash()));
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(fresh.clone()),
        NOW + 30,
    );
    assert_eq!(seqs(&store), [1], "own writes never retrigger");
}

#[test]
fn an_own_review_body_never_retriggers() {
    let (_fixture, store, mut monitor) = configured(false);
    let (job, run) = posted_exit(&store, &mut monitor);
    // A body-only or body + inline review is a COMMENTED review of the acting account.
    let mut fresh = with_thread(snapshot("a"), "T9", "500");
    fresh
        .reviews
        .push(pr_sniper_lib::github::state::SnapshotReview {
            actor: "acting-account".into(),
            state: "COMMENTED".into(),
            head: Some(fresh.head.clone()),
        });
    assert_eq!(
        pr_sniper_lib::exits::consume::settle_fresh(&store, &job, &run.operation.id, &fresh)
            .unwrap(),
        Consumed::Consumed
    );
    assert_eq!(record(&store).baseline_hash, Some(fresh.normalize().hash()));
    scan(&mut monitor, &store, open(false), Some(fresh), NOW + 30);
    assert_eq!(seqs(&store), [1], "own review body never retriggers");
    assert_eq!(record(&store).error_count, 0);
}

#[test]
fn a_heartbeat_that_saw_only_own_writes_before_consume_is_absorbed() {
    let (_fixture, store, mut monitor) = configured(false);
    let (job, run) = posted_exit(&store, &mut monitor);
    let fresh = with_thread(snapshot("a"), "T9", "500");
    // Published, consume not yet run: the heartbeat records the own post.
    scan(
        &mut monitor,
        &store,
        open(false),
        Some(fresh.clone()),
        NOW + 20,
    );
    assert_eq!(record(&store).state_seq, 2);
    assert_eq!(
        pr_sniper_lib::exits::consume::settle_fresh(&store, &job, &run.operation.id, &fresh)
            .unwrap(),
        Consumed::Consumed
    );
    let after = record(&store);
    assert!(after.pending.is_none());
    assert_eq!(after.error_count, 0, "no toss");
    assert_eq!(after.exit.unwrap().state_seq, 2);
}

#[test]
fn an_external_change_in_the_write_window_requeues_once_after_consume() {
    let human = with_thread(with_thread(snapshot("a"), "T9", "500"), "T10", "600");
    let mut ci = with_thread(snapshot("a"), "T9", "500");
    ci.checks[0].conclusion = "FAILURE".into();
    for fresh in [human, ci] {
        let (_fixture, store, mut monitor) = configured(false);
        let (job, run) = posted_exit(&store, &mut monitor);
        assert_eq!(
            pr_sniper_lib::exits::consume::settle_fresh(&store, &job, &run.operation.id, &fresh)
                .unwrap(),
            Consumed::Consumed
        );
        assert_eq!(
            record(&store).baseline_hash,
            Some(snapshot("a").normalize().hash()),
            "the packet state is the baseline"
        );
        scan(
            &mut monitor,
            &store,
            open(false),
            Some(fresh.clone()),
            NOW + 30,
        );
        scan(
            &mut monitor,
            &store,
            open(false),
            Some(fresh.clone()),
            NOW + 40,
        );
        assert_eq!(seqs(&store), [1, 2], "one review of the external change");
    }
}
