use super::*;
use crate::{
    github::metadata::Lifecycle,
    monitoring::{
        AccountAvailability, Admission, JobOperation, Monitor, OperationState, QueueJob,
        QueueState, TrackedPullRequest, WAITING_HUMAN_START, WAITING_MERGED, WAITING_SUPERSEDED,
    },
    publication::{Batch, Publication, Receipt, RemoteState},
    review::{ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::json;
use std::collections::BTreeMap;

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ASSIGNMENT: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const NOW: i64 = 1_800_000_000;
const TICK: i64 = 5;
/// Merged and open PRs, each with this many superseded/merged passes: the live
/// install had 1,362 entries, single PRs up to 182, 97% retired.
const MERGED_PRS: u64 = 55;
const OPEN_PRS: u64 = 5;
const PASSES: u64 = 24;

#[test]
fn heartbeat_bounds_an_unchanged_generation() {
    let mut wake = Wake::default();
    assert!(wake.due(0, NOW), "the first tick always runs");
    assert!(!wake.due(0, NOW + TICK));
    assert!(!wake.due(0, NOW + HEARTBEAT_SECONDS - 1));
    assert!(wake.due(0, NOW + HEARTBEAT_SECONDS));
    assert!(
        wake.due(1, NOW + HEARTBEAT_SECONDS + TICK),
        "a saved change wakes"
    );
    assert!(!wake.due(1, NOW + HEARTBEAT_SECONDS + 2 * TICK));
    // A clock that moves backwards cannot suppress work forever.
    assert!(wake.due(1, NOW));
}

#[test]
fn bookkeeping_writes_do_not_wake_coordinators() {
    use crate::connectivity::Observation::{Reachable, Unreachable};
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let start = store.wake_generation();
    store.save_queue_selection(Some("item")).unwrap();
    let mut connectivity = crate::connectivity::Connectivity::default();
    store.save_connectivity(&connectivity).unwrap();
    assert_eq!(store.wake_generation(), start);
    let scope = crate::backpressure::Scope::github("22");
    connectivity.observe(scope.clone(), Reachable, NOW);
    store.save_connectivity(&connectivity).unwrap();
    let online = store.wake_generation();
    assert_ne!(online, start, "a newly observed service is a change");
    connectivity.observe(scope.clone(), Reachable, NOW + 30);
    store.save_connectivity(&connectivity).unwrap();
    assert_eq!(store.wake_generation(), online, "a repeated success is not");
    connectivity.observe(scope, Unreachable, NOW + 60);
    store.save_connectivity(&connectivity).unwrap();
    assert_ne!(store.wake_generation(), online, "going offline is");
}

fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,"doctrines":[],
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,
                "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"fixture"}]
    }))
    .unwrap()
}

fn job(number: u64, pass: u64, waiting: &str) -> QueueJob {
    serde_json::from_value(json!({
        "work": {
            "id": format!("work-{number}-{pass}"), "item_id": format!("iteration-{number}"),
            "iteration_id": format!("iteration-{number}"), "iteration": 1, "agent_id": AGENT,
            "enqueue_order": number * 1000 + pass, "pass_ordinal": pass, "trigger": "admission",
            "admission": {"watched_author": true, "all_authors": false, "requested_reviewer": false},
            "state_seq": pass
        },
        "assignment_id": ASSIGNMENT, "provider": "github", "account_id": "22",
        "account_login": "operator", "configuration_id": REPO, "repository_id": "100",
        "repository_name": "example/repo", "pull_request_id": number.to_string(),
        "number": number, "title": "Change",
        // Three passes share each head, like re-reviews of one revision.
        "head_sha": format!("{:040x}", number * 1000 + (pass - 1) / 3),
        "observed_base_sha": "b".repeat(40), "trigger_policy": "fixture",
        "author_id": "11", "author_login": "author", "watched_author": true,
        "requested_reviewer": false, "waiting": waiting, "detected_at": NOW
    }))
    .unwrap()
}

fn review(settings: &Settings, job: &QueueJob, pad: &str) -> ReviewRun {
    let mut operation = JobOperation::review(job, NOW);
    operation.state = OperationState::Completed;
    ReviewRun {
        mode: crate::review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: crate::review::key(job, ASSIGNMENT),
        assignment_id: ASSIGNMENT.into(),
        job: job.clone(),
        selection: Selection::resolve(settings, job, ASSIGNMENT).unwrap(),
        operation,
        manual_start: true,
        trust_confirmed: true,
        phase: "Complete".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({
                "reviewed_base_sha": "b".repeat(40),
                "output": {"synopsis": pad, "files": [{"path": "source.rs", "explanation": pad, "order": 1}],
                    "findings": [{"path": "source.rs", "side": "RIGHT", "line": 1, "severity": "high",
                        "title": "Concern", "explanation": pad, "confidence": 99}],
                    "decision": "human_input_required"},
                "session_id": "session", "model": "model", "runtime_version": "fixture",
                "input_tokens": 1, "output_tokens": 1, "tool_calls": 1
            }))
            .unwrap(),
        ),
    }
}

fn observed(number: u64, pad: &str) -> serde_json::Value {
    json!({
        "failures": 0, "retry_at": null, "item_id": format!("iteration-{number}"), "at": NOW,
        "error": null,
        "observation": {
            "write_capability": true, "node_id": "node", "repository_id": "100",
            "pull_request_id": number.to_string(), "account_id": "22", "author_id": "11",
            "head_repository_id": "100", "head": "a".repeat(40), "base": "b".repeat(40),
            "base_name": "main", "merge_rules": null, "merge_rules_error": null,
            "state": "MERGED", "draft": false, "permission": "WRITE", "mergeable": "MERGEABLE",
            "merge_state": "CLEAN", "review_decision": null, "checks": null,
            "check_contexts": [], "in_merge_queue": false, "method": "SQUASH",
            "protection": pad, "threads": [], "reviews": [], "comments": [],
            "merged_by": "11", "merged_at": "2026-10-02T00:00:00Z", "merge_commit": "c".repeat(40)
        }
    })
}

/// A bloated profile shaped like the live install: mostly retired queue
/// history, one completed review per pass, a published batch per review and
/// a provider observation per merged PR. `scale` sizes the free text so the
/// same fixture can be rebuilt at live byte sizes for local measurement.
pub(crate) fn bloated(store: &Store, scale: usize) {
    let settings = settings();
    store.save_settings(&settings).unwrap();
    let pad = "x".repeat(scale);
    let mut queue = QueueState::default();
    let mut reviews = Vec::new();
    let mut publications = Vec::new();
    let mut observations = Vec::new();
    for number in 1..=MERGED_PRS + OPEN_PRS {
        let merged = number <= MERGED_PRS;
        for pass in 1..=PASSES {
            let waiting = match (merged, pass == PASSES) {
                (true, _) => WAITING_MERGED,
                (false, true) => WAITING_HUMAN_START,
                (false, false) => WAITING_SUPERSEDED,
            };
            let entry = job(number, pass, waiting);
            // Two passes per PR were never reviewed: compaction candidates.
            if pass > 2 {
                // Saved runs keep the entry as it was when the review started.
                let run = review(&settings, &job(number, pass, WAITING_HUMAN_START), &pad);
                let mut publication = Publication::new(run.clone(), false, true, NOW).unwrap();
                publication.batch = Some(Batch {
                    commit_id: entry.head_sha.clone(),
                    body: pad.clone(),
                    comments: Vec::new(),
                    unmappable: Vec::new(),
                });
                publication.operation.state = OperationState::Completed;
                publication.phase = crate::publication::Phase::Published;
                publication.receipts.push(Receipt {
                    review_id: format!("{number}{pass}"),
                    state: RemoteState::Commented,
                    comment_ids: Vec::new(),
                });
                reviews.push(run);
                publications.push(publication);
            }
            queue.jobs.push(entry);
        }
        let last = queue.jobs.last().unwrap().clone();
        queue.tracked.push(TrackedPullRequest {
            terminal_observed: merged,
            provider: "github".into(),
            configuration_id: REPO.into(),
            account_id: "22".into(),
            repository_id: "100".into(),
            pull_request_id: number.to_string(),
            number,
            head_sha: last.head_sha,
            lifecycle: if merged {
                Lifecycle::Merged
            } else {
                Lifecycle::Open
            },
            iteration_id: format!("iteration-{number}"),
            item_id: format!("iteration-{number}"),
            iteration: 1,
            admission: Admission {
                watched_author: true,
                all_authors: false,
                requested_reviewer: false,
            },
            admitted_at: NOW,
            observed_at: NOW,
        });
        if merged {
            observations.push(observed(number, &pad));
        }
    }
    queue.next_enqueue_order = (MERGED_PRS + OPEN_PRS + 1) * 1000;
    store.write_state("queue.json", &queue).unwrap();
    store.write_state("reviews.json", &reviews).unwrap();
    store
        .write_state("publications.json", &publications)
        .unwrap();
    store
        .write_state(
            "actions.json",
            &json!({"finals": [], "effects": [], "observations": observations}),
        )
        .unwrap();
    // Opted in against the current queue, with the confirmation delivered.
    let frames = crate::notifications::frames(&crate::queue::snapshot(store, vec![]).unwrap());
    crate::notifications::save_preference(store, true, &frames, NOW, || true).unwrap();
    while let Some(next) = crate::notifications::observe_queue(
        store,
        &crate::queue::snapshot(store, vec![]).unwrap(),
        NOW,
    )
    .unwrap()
    {
        let notice = crate::notifications::prepare(store, &frames, &next.id, NOW).unwrap();
        crate::notifications::complete(
            store,
            &notice.id,
            crate::notifications::Phase::AcceptedUnconfirmed,
            None,
        )
        .unwrap();
    }
}

#[derive(Debug, Default)]
pub(crate) struct Idle {
    pub(crate) ticks: usize,
    pub(crate) passes: usize,
    pub(crate) snapshots: usize,
    pub(crate) notices: usize,
    pub(crate) writes: BTreeMap<String, usize>,
    pub(crate) heavy_reads: usize,
}

/// One scheduler window: `prepare` runs every tick exactly as in the native
/// loop; the coordinator passes run when `Wake` allows (or always, ungated).
/// Coordinator steps are the same store-level functions the native pumps call.
pub(crate) fn simulate(store: &Store, seconds: i64, gated: bool) -> Idle {
    let accounts: BTreeMap<_, _> = [(
        "22".to_string(),
        AccountAvailability {
            login: "operator".into(),
            connected: true,
        },
    )]
    .into();
    let mut monitor = Monitor::restore(store).unwrap();
    let ai = crate::capacity::Coordinator::default();
    let mut wake = Wake::default();
    let mut idle = Idle::default();
    store.take_state_writes();
    store.take_state_reads();
    let mut now = NOW;
    while now < NOW + seconds {
        now += TICK;
        idle.ticks += 1;
        assert!(monitor
            .prepare_checks_with_accounts(store, &accounts, now, false)
            .unwrap()
            .is_empty());
        if !gated || wake.due(store.wake_generation(), now) {
            idle.passes += 1;
            crate::retention::recover(store).unwrap();
            crate::monitoring::compact_saved_queue(store).unwrap();
            monitor.auto_resume(store, &accounts, now).unwrap();
            assert!(ai.dispatch(store, now).unwrap().dispatched.is_empty());
            assert!(crate::publication::host::next_candidate(store, now)
                .unwrap()
                .is_none());
            assert!(crate::exit_writes::next_write(store, now)
                .unwrap()
                .is_none());
            assert!(crate::follow_up::host::next_due(store, now)
                .unwrap()
                .is_none());
            assert!(crate::actions::host::next_target(store, now)
                .unwrap()
                .is_none());
            let snapshot = crate::queue::snapshot(store, monitor.snapshot()).unwrap();
            idle.snapshots += 2;
            // Deliver as the native loop does; idle windows must send nothing.
            let frames = crate::notifications::frames(&snapshot);
            while let Some(next) =
                crate::notifications::observe_queue(store, &snapshot, now).unwrap()
            {
                idle.notices += 1;
                let notice = crate::notifications::prepare(store, &frames, &next.id, now).unwrap();
                crate::notifications::complete(
                    store,
                    &notice.id,
                    crate::notifications::Phase::AcceptedUnconfirmed,
                    None,
                )
                .unwrap();
            }
        }
        for (name, count) in store.take_state_writes() {
            *idle.writes.entry(name).or_default() += count;
        }
        idle.heavy_reads += store
            .take_state_reads()
            .into_iter()
            .filter(|(name, _)| {
                matches!(
                    name.as_str(),
                    "reviews.json" | "publications.json" | "actions.json" | "feedback.json"
                )
            })
            .map(|(_, count)| count)
            .sum::<usize>();
    }
    idle
}

#[test]
fn bloated_profile_settles_to_idle_without_rewrites() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    bloated(&store, 64);
    let jobs = store.load_queue().unwrap().len();
    assert!(jobs >= 1_400, "{jobs}");
    // Startup compaction drops unreviewed retired passes but keeps every
    // entry evidence would restore and each PR's newest pass.
    let removed = crate::monitoring::compact_saved_queue(&store).unwrap();
    assert_eq!(removed, MERGED_PRS as usize * 2 + OPEN_PRS as usize * 2);
    assert_eq!(crate::monitoring::compact_saved_queue(&store).unwrap(), 0);

    // Settle whatever the first pass adopts, then observe five idle minutes.
    simulate(&store, TICK, true);
    let idle = simulate(&store, 300, true);
    assert_eq!(idle.ticks, 60);
    assert_eq!(idle.notices, 0);
    assert!(
        idle.writes.is_empty(),
        "an unchanged store must never be rewritten: {:?}",
        idle.writes
    );
    // One pass at start plus one per heartbeat; nothing re-triggers itself.
    assert_eq!(idle.passes, (300 / HEARTBEAT_SECONDS) as usize);
    assert!(idle.snapshots <= 2 * idle.passes);
    // The former loop ran every coordinator on every 5 s tick.
    let ungated = simulate(&store, 20, false);
    assert!(ungated.writes.is_empty(), "{:?}", ungated.writes);
    assert_eq!(ungated.passes, ungated.ticks);
    // Idle ticks touch none of the large stores; only gated passes do.
    assert!(
        idle.heavy_reads * ungated.ticks * 10 <= ungated.heavy_reads * idle.ticks,
        "{} reads in {} gated ticks vs {} in {} ungated",
        idle.heavy_reads,
        idle.ticks,
        ungated.heavy_reads,
        ungated.ticks
    );
}

#[test]
fn snapshot_reads_do_not_scale_with_retired_history() {
    let reads = |merged_passes: u64| {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().into());
        store.save_settings(&settings()).unwrap();
        let mut queue = QueueState::default();
        for pass in 1..=merged_passes {
            queue.jobs.push(job(1, pass, WAITING_MERGED));
        }
        queue.jobs.push(job(2, 1, WAITING_HUMAN_START));
        store.write_state("queue.json", &queue).unwrap();
        store.take_state_reads();
        crate::queue::snapshot(&store, vec![]).unwrap();
        store.take_state_reads()
    };
    assert_eq!(reads(1), reads(400));
}
