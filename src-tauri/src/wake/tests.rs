use super::*;
use crate::{
    monitoring::{
        AccountAvailability, Admission, JobOperation, Monitor, OperationState, QueueJob,
        QueueState, TrackedPullRequest, WAITING_HUMAN_START, WAITING_MERGED, WAITING_SUPERSEDED,
    },
    provider::Lifecycle,
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
fn deadline_heartbeat_and_transitions_gate_each_coordinator() {
    let mut gates = Gates::default();
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.save_settings(&settings()).unwrap();
    store
        .write_state(
            "backpressure.json",
            &json!({"gates": [], "wake_at": NOW + 7}),
        )
        .unwrap();
    let generations = store.generations();
    assert_eq!(gates.due(&generations, NOW), Coordinator::ALL.to_vec());
    gates.settle(
        &Coordinator::ALL,
        &generations,
        &store.freeze().unwrap(),
        NOW,
    );
    assert!(gates.due(&generations, NOW + TICK).is_empty());
    // A saved instant wakes exactly the coordinators that read its file.
    let woken = gates.due(&generations, NOW + 2 * TICK);
    assert!(woken.contains(&Coordinator::Capacity), "{woken:?}");
    assert!(!woken.contains(&Coordinator::ExitWrites), "{woken:?}");
    // A transition wakes only its readers.
    store.write_state("pr-state.json", &json!({})).unwrap();
    let woken = gates.due(&store.generations(), NOW + TICK);
    assert!(woken.contains(&Coordinator::Publication), "{woken:?}");
    assert!(!woken.contains(&Coordinator::ExitWrites), "{woken:?}");
    assert!(!woken.contains(&Coordinator::Maintenance), "{woken:?}");
    // Nothing saved at all: the heartbeat bounds in-memory transitions.
    assert!(gates
        .due(&generations, NOW + HEARTBEAT_SECONDS)
        .contains(&Coordinator::Maintenance));
    // A clock that moves backwards cannot suppress work forever.
    assert_eq!(
        gates.due(&generations, NOW - 1).len(),
        Coordinator::ALL.len()
    );
}

#[test]
fn instants_are_found_in_saved_documents() {
    let bytes = serde_json::to_vec_pretty(&json!({
        "operation": {"next_attempt_at": NOW + 6, "detected_at": NOW - 9, "retry_deadline": NOW + 900},
        "gates": [{"until": NOW + 17}],
        "count": NOW + 3,
        "text": "\"resume_at\": 1"
    }))
    .unwrap();
    assert_eq!(
        future_instants(&bytes, NOW),
        vec![NOW + 6, NOW + 17, NOW + 900]
    );
}

#[test]
fn bookkeeping_and_progress_writes_do_not_wake_coordinators() {
    use crate::connectivity::Observation::{Reachable, Unreachable};
    let transitions =
        |store: &Store| -> u64 { store.generations().values().map(|c| c.transitions).sum() };
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    bloated(&store, 8);
    let start = transitions(&store);
    store.save_queue_selection(Some("item")).unwrap();
    // Control request receipts replay responses; they never create work.
    crate::control::operations::begin_request(&store, "request", NOW, "hash", NOW).unwrap();
    crate::control::operations::finish_request(&store, "request", Ok(serde_json::json!({})))
        .unwrap();
    let mut connectivity = crate::connectivity::Connectivity::default();
    store.save_connectivity(&connectivity).unwrap();
    assert_eq!(transitions(&store), start);
    let scope = crate::backpressure::Scope::github("22");
    connectivity.observe(scope.clone(), Reachable, NOW);
    store.save_connectivity(&connectivity).unwrap();
    let online = transitions(&store);
    assert_ne!(online, start, "a newly observed service is a change");
    connectivity.observe(scope.clone(), Reachable, NOW + 30);
    store.save_connectivity(&connectivity).unwrap();
    assert_eq!(transitions(&store), online, "a repeated success is not");
    connectivity.observe(scope, Unreachable, NOW + 60);
    store.save_connectivity(&connectivity).unwrap();
    let offline = transitions(&store);
    assert_ne!(offline, online, "going offline is");

    let mut reviews = store.load_reviews().unwrap();
    store.save_reviews(&reviews).unwrap();
    let saved = transitions(&store);
    reviews[0].phase = "Reading changed files".into();
    store.save_reviews(&reviews).unwrap();
    assert_eq!(transitions(&store), saved, "progress is not a transition");
    reviews[0].operation.next_attempt_at = Some(NOW + 5);
    store.save_reviews(&reviews).unwrap();
    assert_ne!(transitions(&store), saved, "a new retry time is");
}

#[test]
fn coordinator_inputs_cover_every_file_their_pass_reads() {
    let root = tempfile::tempdir().unwrap();
    let store = std::sync::Mutex::new(Store::new(root.path().into()));
    bloated(&store.lock().unwrap(), 8);
    // A native-control watch hold (#209) the capacity pass must wake for.
    crate::control::work::hold(&store.lock().unwrap(), "held-repository", 1, true).unwrap();
    let mut sim = Sim::new(&store, Mode::Gated);
    // Compaction is time-driven (hourly), not woken by inputs.
    assert!(sim.gates.compaction_due(sim.now));
    for coordinator in Coordinator::ALL {
        store.lock().unwrap().take_state_reads();
        let view = store.lock().unwrap().freeze().unwrap();
        sim.pass(coordinator, &view);
        let mut reads = store.lock().unwrap().take_state_reads();
        reads.extend(view.take_state_reads());
        for name in reads.keys() {
            assert!(
                coordinator.reads(name),
                "{coordinator:?} reads {name} without waking for it"
            );
        }
        if coordinator == Coordinator::Capacity {
            assert!(reads.contains_key("control-watch-holds.json"), "{reads:?}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The current scheduler: gates, frozen scans, locked writes only.
    Gated,
    /// The former loop: every coordinator, every tick, scans under the lock.
    Legacy,
}

#[derive(Debug, Default)]
pub(crate) struct Stats {
    pub(crate) ticks: usize,
    pub(crate) passes: BTreeMap<String, usize>,
    pub(crate) snapshots: usize,
    pub(crate) notices: usize,
    pub(crate) writes: BTreeMap<String, usize>,
    pub(crate) heavy_reads: usize,
    pub(crate) max_hold: std::time::Duration,
    pub(crate) total_hold: std::time::Duration,
    pub(crate) holds: BTreeMap<String, std::time::Duration>,
}

impl Stats {
    pub(crate) fn passes(&self) -> usize {
        self.passes.values().sum()
    }
}

/// Scheduler model: `prepare` every tick, then the due coordinators with the
/// same store-level steps and lock scopes as the native pumps.
pub(crate) struct Sim<'a> {
    store: &'a std::sync::Mutex<Store>,
    mode: Mode,
    gates: Gates,
    monitor: Monitor,
    pub(crate) ai: crate::capacity::Coordinator,
    accounts: BTreeMap<String, AccountAvailability>,
    pub(crate) now: i64,
    pub(crate) stats: Stats,
    pub(crate) dispatched: Vec<crate::capacity::Dispatch>,
    step: &'static str,
}

impl<'a> Sim<'a> {
    pub(crate) fn new(store: &'a std::sync::Mutex<Store>, mode: Mode) -> Self {
        let monitor = Monitor::restore(&store.lock().unwrap()).unwrap();
        let accounts = store
            .lock()
            .unwrap()
            .load_settings()
            .unwrap()
            .repositories
            .iter()
            .filter_map(|r| r.provider_account_id.clone())
            .map(|id| {
                (
                    id,
                    AccountAvailability {
                        login: "operator".into(),
                        connected: true,
                    },
                )
            })
            .collect();
        Self {
            store,
            mode,
            gates: Gates::default(),
            monitor,
            ai: Default::default(),
            accounts,
            now: NOW,
            stats: Stats::default(),
            dispatched: Vec::new(),
            step: "tick",
        }
    }

    fn locked<T>(&mut self, f: impl FnOnce(&Store, &mut Monitor) -> T) -> T {
        let store = self.store.lock().unwrap();
        let held = std::time::Instant::now();
        let value = f(&store, &mut self.monitor);
        let held = held.elapsed();
        self.stats.max_hold = self.stats.max_hold.max(held);
        self.stats.total_hold += held;
        let step = self.stats.holds.entry(self.step.into()).or_default();
        *step = (*step).max(held);
        value
    }

    pub(crate) fn run(&mut self, seconds: i64) {
        let end = self.now + seconds;
        while self.now < end {
            self.now += TICK;
            self.tick();
        }
    }

    pub(crate) fn tick(&mut self) -> Vec<Coordinator> {
        self.stats.ticks += 1;
        let (now, accounts) = (self.now, self.accounts.clone());
        let tickets = self.locked(|store, monitor| {
            monitor
                .prepare_checks_with_accounts(store, &accounts, now, false)
                .unwrap()
        });
        assert!(tickets.is_empty());
        let generations = self.locked(|store, _| store.generations());
        self.step = "prepare";
        let due = match self.mode {
            Mode::Gated => self.gates.due(&generations, now),
            Mode::Legacy => Coordinator::ALL.to_vec(),
        };
        if !due.is_empty() {
            self.step = "freeze";
            let view = match self.mode {
                Mode::Gated => self.locked(|store, _| store.freeze().unwrap()),
                Mode::Legacy => Store::new(std::path::PathBuf::new()),
            };
            for &coordinator in &due {
                self.pass(coordinator, &view);
            }
            if self.mode == Mode::Gated {
                self.gates.settle(&due, &generations, &view, now);
            }
        }
        self.step = "tick";
        let (writes, reads) =
            self.locked(|store, _| (store.take_state_writes(), store.take_state_reads()));
        for (name, count) in writes {
            *self.stats.writes.entry(name).or_default() += count;
        }
        self.stats.heavy_reads += reads
            .into_iter()
            .filter(|(name, _)| {
                matches!(
                    name.as_str(),
                    "reviews.json" | "publications.json" | "actions.json" | "feedback.json"
                )
            })
            .map(|(_, count)| count)
            .sum::<usize>();
        due
    }

    /// One coordinator pass. Gated: scans read `view` outside the lock and
    /// only writes lock, as in the native pumps. Legacy: all under the lock.
    pub(crate) fn pass(&mut self, coordinator: Coordinator, view: &Store) {
        *self
            .stats
            .passes
            .entry(format!("{coordinator:?}"))
            .or_default() += 1;
        let (now, accounts, legacy) = (self.now, self.accounts.clone(), self.mode == Mode::Legacy);
        self.step = match coordinator {
            Coordinator::Maintenance => "maintenance",
            Coordinator::Resume => "resume",
            Coordinator::Capacity => "capacity",
            Coordinator::Publication => "publication",
            Coordinator::ExitWrites => "exit_writes",
            Coordinator::FollowUps => "follow_ups",
            Coordinator::Actions => "actions",
            Coordinator::Notifications => "notifications",
        };
        // Legacy scans run on the locked live store instead of the view.
        macro_rules! scan {
            (|$store:ident| $body:expr) => {
                if legacy {
                    self.locked(|$store, _| $body)
                } else {
                    let $store = view;
                    $body
                }
            };
        }
        match coordinator {
            Coordinator::Maintenance => {
                let compact = legacy || self.gates.compaction_due(now);
                self.locked(|store, _| {
                    crate::retention::recover(store).unwrap();
                    if compact {
                        crate::monitoring::compact_saved_queue(store).unwrap();
                    }
                });
            }
            Coordinator::Resume => {
                if legacy || self.monitor.auto_resume_due(view, &accounts, now).unwrap() {
                    self.locked(|store, monitor| {
                        monitor.auto_resume(store, &accounts, now).unwrap()
                    });
                }
            }
            Coordinator::Capacity => {
                let ai = std::mem::take(&mut self.ai);
                if !legacy && ai.dispatch_idle(view, now).unwrap() {
                    self.ai = ai;
                    return;
                }
                let batch = self.locked(|store, _| ai.dispatch(store, now).unwrap());
                self.ai = ai;
                assert!(batch.errors.is_empty(), "{:?}", batch.errors);
                self.dispatched.extend(batch.dispatched);
            }
            Coordinator::Publication => {
                assert!(
                    scan!(|store| crate::publication::host::next_candidate(store, now))
                        .unwrap()
                        .is_none()
                );
            }
            Coordinator::ExitWrites => {
                if scan!(|store| crate::exit_writes::write_pending(store, now)).unwrap() {
                    assert!(self
                        .locked(|store, _| crate::exit_writes::next_write(store, now))
                        .unwrap()
                        .is_none());
                }
            }
            Coordinator::FollowUps => {
                assert!(scan!(|store| crate::follow_up::host::next_due(store, now))
                    .unwrap()
                    .is_none());
            }
            Coordinator::Actions => {
                if scan!(|store| crate::actions::prepared_expired(store, now)).unwrap() {
                    self.locked(|store, _| crate::actions::expire_prepared(store, now))
                        .unwrap();
                }
                assert!(scan!(|store| crate::actions::host::next_target(store, now))
                    .unwrap()
                    .is_none());
            }
            Coordinator::Notifications => {
                let (snapshot_view, health) = self.locked(|store, monitor| {
                    monitor
                        .synchronize_configuration(store, &accounts, now)
                        .unwrap();
                    let view = if legacy {
                        None
                    } else {
                        Some(store.freeze().unwrap())
                    };
                    (view, monitor.snapshot())
                });
                let snapshot = match &snapshot_view {
                    Some(view) => crate::queue::snapshot(view, health).unwrap(),
                    None => self.locked(|store, _| crate::queue::snapshot(store, health).unwrap()),
                };
                self.stats.snapshots += 1;
                let notices = self.locked(|store, _| {
                    // Deliver as the native loop does; idle windows send nothing.
                    let mut sent = 0;
                    while let Some(next) =
                        crate::notifications::observe_queue(store, &snapshot, now).unwrap()
                    {
                        sent += 1;
                        let notice =
                            crate::notifications::prepare_queue(store, &snapshot, &next.id, now)
                                .unwrap();
                        crate::notifications::complete(
                            store,
                            &notice.id,
                            crate::notifications::Phase::AcceptedUnconfirmed,
                            None,
                        )
                        .unwrap();
                    }
                    sent
                });
                self.stats.notices += notices;
            }
        }
    }
}

#[test]
fn bloated_profile_settles_to_idle_without_rewrites() {
    let root = tempfile::tempdir().unwrap();
    let store = std::sync::Mutex::new(Store::new(root.path().into()));
    {
        let store = store.lock().unwrap();
        bloated(&store, 64);
        let jobs = store.load_queue().unwrap().len();
        assert!(jobs >= 1_400, "{jobs}");
        // Startup compaction drops unreviewed retired passes but keeps every
        // entry evidence would restore and each PR's newest pass.
        let removed = crate::monitoring::compact_saved_queue(&store).unwrap();
        assert_eq!(removed, MERGED_PRS as usize * 2 + OPEN_PRS as usize * 2);
        assert_eq!(crate::monitoring::compact_saved_queue(&store).unwrap(), 0);
    }
    let mut sim = Sim::new(&store, Mode::Gated);
    // Settle whatever the first passes adopt, then observe five idle minutes.
    sim.run(2 * TICK);
    sim.stats = Stats::default();
    sim.run(300);
    let idle = sim.stats;
    assert_eq!(idle.ticks, 60);
    assert_eq!(idle.notices, 0);
    assert!(
        idle.writes.is_empty(),
        "an unchanged store must never be rewritten: {:?}",
        idle.writes
    );
    // Only heartbeats: nothing re-triggers itself.
    let heartbeats = (300 / HEARTBEAT_SECONDS) as usize;
    for (coordinator, passes) in &idle.passes {
        assert!(*passes <= heartbeats, "{coordinator}: {passes}");
    }
    let mut legacy = Sim::new(&store, Mode::Legacy);
    legacy.run(2 * TICK);
    assert!(legacy.stats.writes.is_empty(), "{:?}", legacy.stats.writes);
    assert_eq!(legacy.stats.passes(), 2 * Coordinator::ALL.len());
    // Idle ticks touch none of the large stores; only gated passes do.
    assert!(
        idle.heavy_reads * legacy.stats.ticks * 10 <= legacy.stats.heavy_reads * idle.ticks,
        "{} reads in {} gated ticks vs {} in {} legacy",
        idle.heavy_reads,
        idle.ticks,
        legacy.stats.heavy_reads,
        legacy.stats.ticks
    );
}

/// Ticks until a coordinator dispatches; panics after `limit` seconds.
fn ticks_until_dispatch(sim: &mut Sim, limit: i64) -> i64 {
    let start = sim.now;
    while sim.dispatched.is_empty() {
        assert!(sim.now - start < limit, "nothing dispatched in {limit} s");
        sim.now += TICK;
        sim.tick();
    }
    sim.now
}

fn finish(sim: &mut Sim, error: crate::review::Failure) -> i64 {
    let work = sim.dispatched.remove(0);
    let key = work.key();
    let crate::capacity::Dispatch::Review(run, _) = work;
    let now = sim.now + 1;
    crate::review::host::complete(
        &sim.store.lock().unwrap(),
        &run.operation.id,
        Err(error),
        now,
    )
    .unwrap();
    sim.ai.release(&key, &run.operation.id).unwrap();
    now
}

fn retried(store: &std::sync::Mutex<Store>) -> i64 {
    let reviews = store.lock().unwrap().load_reviews().unwrap();
    reviews[0].operation.next_attempt_at.unwrap()
}

fn assert_next_tick(sim: &mut Sim, due: i64) {
    let dispatched = ticks_until_dispatch(sim, HEARTBEAT_SECONDS);
    assert!(
        (due..due + TICK).contains(&dispatched),
        "due at {due}, dispatched at {dispatched}"
    );
}

#[test]
fn transient_retry_dispatches_on_the_first_tick_after_it_is_due() {
    let (_root, store) = crate::capacity::tests::fixture(1, 1, true);
    let store = std::sync::Mutex::new(store);
    let mut sim = Sim::new(&store, Mode::Gated);
    ticks_until_dispatch(&mut sim, TICK * 2);
    finish(&mut sim, crate::review::Failure::provider("transient"));
    let due = retried(&store);
    // Full jitter may land inside the next tick; it still waits until due.
    assert!(due > sim.now, "retry {due} must be in the future");
    assert_next_tick(&mut sim, due);
}

#[test]
fn provider_retry_after_dispatches_on_the_first_tick_after_it_is_due() {
    let (_root, store) = crate::capacity::tests::fixture(1, 1, true);
    let store = std::sync::Mutex::new(store);
    let mut sim = Sim::new(&store, Mode::Gated);
    ticks_until_dispatch(&mut sim, TICK * 2);
    let mut limited = crate::review::Failure::provider("rate limited");
    limited.kind = crate::monitoring::OperationFailure::RateLimited;
    limited.retry_after_seconds = Some(17);
    let failed_at = finish(&mut sim, limited);
    let due = retried(&store);
    assert_eq!(due, failed_at + 17);
    assert_next_tick(&mut sim, due);
}

#[test]
fn superseded_retry_dispatches_on_the_first_tick_after_it_is_due() {
    let (_root, store) = crate::capacity::tests::fixture(1, 1, true);
    let store = std::sync::Mutex::new(store);
    let mut sim = Sim::new(&store, Mode::Gated);
    ticks_until_dispatch(&mut sim, TICK * 2);
    let work = sim.dispatched.remove(0);
    let key = work.key();
    let crate::capacity::Dispatch::Review(run, _) = work;
    let superseded_at = sim.now + 1;
    {
        let store = store.lock().unwrap();
        let mut reviews = store.load_reviews().unwrap();
        let saved = reviews
            .iter_mut()
            .find(|r| r.operation.id == run.operation.id)
            .unwrap();
        saved.operation.requeue_superseded(superseded_at, None);
        store.save_reviews(&reviews).unwrap();
    }
    sim.ai.release(&key, &run.operation.id).unwrap();
    let due = retried(&store);
    assert_eq!(due, superseded_at + 30);
    assert_next_tick(&mut sim, due);
}

#[test]
fn running_review_progress_neither_wakes_coordinators_nor_waits_on_scans() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let store = std::sync::Mutex::new(Store::new(root.path().into()));
    // Live byte sizes (reviews ~74 MB) for local release measurement: 17000.
    let scale = std::env::var("PR_SNIPER_FIXTURE_SCALE")
        .ok()
        .and_then(|scale| scale.parse().ok())
        .unwrap_or(64);
    bloated(&store.lock().unwrap(), scale);
    let measure = |mode: Mode, seconds: i64| {
        let stop = AtomicBool::new(false);
        let (sim, progress) = std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                let mut saves = 0usize;
                let mut wait = std::time::Duration::ZERO;
                while !stop.load(Ordering::SeqCst) {
                    let asked = std::time::Instant::now();
                    let store = store.lock().unwrap();
                    wait = wait.max(asked.elapsed());
                    let mut reviews = store.load_reviews().unwrap();
                    reviews[saves % 4].phase = format!("Reading file {saves}");
                    store.save_reviews(&reviews).unwrap();
                    drop(store);
                    saves += 1;
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                (saves, wait)
            });
            let mut sim = Sim::new(&store, mode);
            sim.run(2 * TICK);
            sim.stats = Stats::default();
            sim.run(seconds);
            stop.store(true, Ordering::SeqCst);
            (sim.stats, writer.join().unwrap())
        });
        (sim, progress)
    };
    // The former loop also decoded every file on every read.
    store.lock().unwrap().set_caching(false);
    let (legacy, (legacy_saves, legacy_wait)) = measure(Mode::Legacy, 3 * TICK);
    store.lock().unwrap().set_caching(true);
    let (gated, (gated_saves, gated_wait)) = measure(Mode::Gated, 120);
    eprintln!(
        "active load legacy: {} passes, max hold {:?}, total hold {:?}, writer {} saves, max wait {:?}",
        legacy.passes(),
        legacy.max_hold,
        legacy.total_hold,
        legacy_saves,
        legacy_wait
    );
    eprintln!("active load legacy holds: {:?}", legacy.holds);
    eprintln!("active load gated holds: {:?}", gated.holds);
    eprintln!(
        "active load gated: {} passes, max hold {:?}, total hold {:?}, writer {} saves, max wait {:?}",
        gated.passes(),
        gated.max_hold,
        gated.total_hold,
        gated_saves,
        gated_wait
    );
    assert!(gated_saves > 0 && legacy_saves > 0);
    // Progress writes wake nobody: at most one pass per coordinator per
    // heartbeat while reviews keep saving.
    let heartbeats = (120 / HEARTBEAT_SECONDS) as usize + 1;
    for (coordinator, passes) in &gated.passes {
        assert!(*passes <= heartbeats, "{coordinator}: {passes}");
    }
    assert_eq!(legacy.passes(), 3 * Coordinator::ALL.len());
    // Scans parse outside the lock: the longest hold is a fraction of the
    // former locked snapshot, the Store is held far less per tick, and a
    // review saving progress waits far less for it.
    assert!(
        gated.max_hold * 4 <= legacy.max_hold,
        "gated {:?} vs legacy {:?}",
        gated.max_hold,
        legacy.max_hold
    );
    // Absolute ceiling for unoptimized builds on slow CI runners.
    assert!(gated.max_hold < std::time::Duration::from_secs(1));
    assert!(
        gated.total_hold.as_secs_f64() / gated.ticks as f64 * 10.0
            <= legacy.total_hold.as_secs_f64() / legacy.ticks as f64
    );
    assert!(
        gated_wait * 4 <= legacy_wait,
        "writer waited {gated_wait:?} vs {legacy_wait:?}"
    );
}

pub(crate) fn settings() -> Settings {
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

pub(crate) fn job(number: u64, pass: u64, waiting: &str) -> QueueJob {
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

pub(crate) fn review(settings: &Settings, job: &QueueJob, pad: &str) -> ReviewRun {
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
        selection: Selection::resolve(settings, job, ASSIGNMENT).unwrap().into(),
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
            observed_sequence: 0,
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
    let baseline = crate::queue::snapshot(store, vec![]).unwrap();
    let frames = crate::notifications::frames(&baseline);
    crate::notifications::save_preference(store, true, &frames, NOW, || true).unwrap();
    while let Some(next) = crate::notifications::observe_queue(
        store,
        &crate::queue::snapshot(store, vec![]).unwrap(),
        NOW,
    )
    .unwrap()
    {
        let notice = crate::notifications::prepare_queue(store, &baseline, &next.id, NOW).unwrap();
        crate::notifications::complete(
            store,
            &notice.id,
            crate::notifications::Phase::AcceptedUnconfirmed,
            None,
        )
        .unwrap();
    }
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

#[test]
fn concurrent_frozen_copies_share_unchanged_files_and_release_them() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.save_settings(&settings()).unwrap();
    store
        .write_state("reviews.json", &vec!["x".repeat(1 << 20)])
        .unwrap();
    store
        .write_state("queue-dismissals.json", &json!([]))
        .unwrap();
    crate::storage::file_identity::settle();
    let first = store.freeze().unwrap();
    let second = store.freeze().unwrap();
    let held = |view: &Store, name| view.frozen_bytes(name).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &held(&first, "reviews.json"),
        &held(&second, "reviews.json")
    ));
    // A write is never served from an older copy.
    store.write_state("reviews.json", &vec!["y"]).unwrap();
    crate::storage::file_identity::settle();
    let third = store.freeze().unwrap();
    assert_eq!(
        &*held(&third, "reviews.json"),
        &store.read_state("reviews.json").unwrap()
    );
    assert!(std::sync::Arc::ptr_eq(
        &held(&first, "queue-dismissals.json"),
        &held(&third, "queue-dismissals.json")
    ));
    // Equal-length foreign rewrites that restore the modification time, by
    // atomic replace and in place, are never served from a live copy.
    let path = root.path().join("state/reviews.json");
    for (atomic, from, to) in [(true, b'y', b'z'), (false, b'z', b'w')] {
        crate::storage::file_identity::rewrite_foreign(&path, atomic, |bytes| {
            for byte in bytes.iter_mut().filter(|b| **b == from) {
                *byte = to;
            }
        });
        let fresh = store.freeze().unwrap();
        assert_eq!(
            &*held(&fresh, "reviews.json"),
            &std::fs::read(&path).unwrap()
        );
        assert!(held(&fresh, "reviews.json").contains(&to));
        assert!(held(&third, "reviews.json").contains(&b'y'));
    }
    // Dropped copies release their bytes; nothing retains them.
    let weak = std::sync::Arc::downgrade(&held(&first, "reviews.json"));
    drop((first, second));
    assert!(weak.upgrade().is_none());
}
