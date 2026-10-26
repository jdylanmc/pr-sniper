//! Live-size cost measurement (ignored; run in release):
//! `cargo test --release --lib storage::cost_bench -- --ignored --nocapture`.
//! A running review saves progress every 1.5 s while the panel polls the
//! queue, notification and automation snapshots every second and the
//! scheduler ticks every 5 s. Reports CPU, bytes written, decodes and memory.

use crate::monitoring::{OperationState, QueueState, WAITING_HUMAN_START, WAITING_SUPERSEDED};
use crate::storage::Store;
use crate::wake::tests::{job, review, settings};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PRS: u64 = 96;
const OPEN_PRS: u64 = 10;
const SELECTIONS: usize = 31;
const CONTEXTS: usize = 152;
const PUBLICATIONS: usize = 174;
const OPERATIONS: usize = 4082;
const NOW: i64 = 1_800_000_000;
/// The panel refreshes its queue, notification and automation views every 5 s.
const POLL_SECONDS: u64 = 5;

#[cfg(target_os = "macos")]
mod usage {
    #[repr(C)]
    #[derive(Default)]
    struct RusageInfoV2 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
        child_user_time: u64,
        child_system_time: u64,
        child_pkg_idle_wkups: u64,
        child_interrupt_wkups: u64,
        child_pageins: u64,
        child_elapsed_abstime: u64,
        diskio_bytesread: u64,
        diskio_byteswritten: u64,
        padding: [u64; 32],
    }

    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV2) -> i32;
        fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
        fn clock_gettime_nsec_np(clock: u32) -> u64;
    }

    /// CPU time of the calling thread (ns).
    pub(super) fn thread_cpu() -> u64 {
        // SAFETY: plain call; 16 is CLOCK_THREAD_CPUTIME_ID on macOS.
        unsafe { clock_gettime_nsec_np(16) }
    }

    /// (CPU ns, resident bytes, footprint bytes, disk bytes written).
    pub(super) fn sample() -> (u64, u64, u64, u64) {
        let mut info = RusageInfoV2::default();
        let mut base = [0u32; 2];
        // SAFETY: both buffers are plain data at least as large as requested.
        unsafe {
            assert_eq!(proc_pid_rusage(std::process::id() as i32, 2, &mut info), 0);
            mach_timebase_info(&mut base);
        }
        let ns = (info.user_time + info.system_time) as u128 * base[0] as u128 / base[1] as u128;
        (
            ns as u64,
            info.resident_size,
            info.phys_footprint,
            info.diskio_byteswritten,
        )
    }
}

/// Saved state shaped like the live install: 1,376 runs whose selection
/// and feedback snapshots repeat (31 and 152 distinct), 174 publications
/// embedding their review, 4,082 monitoring operations.
pub(crate) fn live_profile(store: &Store) {
    let settings = settings();
    store.save_settings(&settings).unwrap();
    let mut queue = QueueState::default();
    let mut reviews = Vec::new();
    let contexts: Vec<serde_json::Value> = (0..CONTEXTS)
        .map(|variant| {
            json!((0..6).map(|n| json!({
                "id": format!("context-{variant}-{n}"), "publication_id": format!("publication-{variant}"),
                "owner_agent_id": "agent", "owner_assignment_id": "assignment",
                "original_head": "a".repeat(40), "root_id": format!("root-{n}"), "path": "src/lib.rs",
                "title": "Concern", "body": format!("{variant}{}", "b".repeat(2900)),
                "thread": null, "closed": false, "unavailable": null
            })).collect::<Vec<_>>())
        })
        .collect();
    // Live shape: 10 open PRs hold about half the runs; the rest belong to
    // PRs observed merged or closed.
    for number in 1..=PRS {
        let open = number <= OPEN_PRS;
        let passes = if open { 66 } else { 8 };
        for pass in 1..=passes {
            let waiting = match (open, pass == passes) {
                (false, _) => crate::monitoring::WAITING_MERGED,
                (true, true) => WAITING_HUMAN_START,
                (true, false) => WAITING_SUPERSEDED,
            };
            let entry = job(number, pass, waiting);
            let index = reviews.len();
            let mut run = review(&settings, &job(number, pass, WAITING_HUMAN_START), "p");
            if open && number <= 3 && pass == passes {
                // Running with today's configuration: its attempt gate passes.
                run.operation.state = OperationState::Running;
            } else {
                run.selection.doctrine =
                    Some(format!("{}{}", index % SELECTIONS, "d".repeat(9800)));
                run.selection.agent.prompt =
                    format!("{}{}", index % SELECTIONS, "q".repeat(12_000));
                run.feedback_context =
                    Some(serde_json::from_value(contexts[index % CONTEXTS].clone()).unwrap());
            }
            reviews.push(run);
            queue.jobs.push(entry);
        }
        let last = queue.jobs.last().unwrap().clone();
        queue.tracked.push(crate::monitoring::TrackedPullRequest {
            terminal_observed: !open,
            provider: "github".into(),
            configuration_id: last.configuration_id.clone(),
            account_id: "22".into(),
            repository_id: "100".into(),
            pull_request_id: number.to_string(),
            number,
            head_sha: last.head_sha,
            lifecycle: if open {
                crate::provider::Lifecycle::Open
            } else {
                crate::provider::Lifecycle::Merged
            },
            iteration_id: format!("iteration-{number}"),
            item_id: format!("iteration-{number}"),
            iteration: 1,
            admission: crate::monitoring::Admission {
                watched_author: true,
                all_authors: false,
                requested_reviewer: false,
            },
            admitted_at: NOW,
            observed_at: NOW,
            observed_sequence: 0,
        });
    }
    queue.next_enqueue_order = (PRS + 1) * 1000;
    let publications: Vec<_> = reviews
        .iter()
        .filter(|run| run.operation.state == OperationState::Completed)
        .step_by(7)
        .take(PUBLICATIONS)
        .map(|run| {
            let mut publication =
                crate::publication::Publication::new(run.clone(), false, true, NOW).unwrap();
            publication.operation.state = OperationState::Completed;
            publication.phase = crate::publication::Phase::Published;
            publication
        })
        .collect();
    let mut monitoring = crate::monitoring::MonitoringState::default();
    for n in 0..OPERATIONS {
        let mut operation = reviews[n % reviews.len()].operation.clone();
        operation.id = format!("operation-{n}");
        monitoring
            .operations
            .insert(operation.id.clone(), operation);
    }
    store.write_state("queue.json", &queue).unwrap();
    store.write_state("reviews.json", &reviews).unwrap();
    store
        .write_state("publications.json", &publications)
        .unwrap();
    store.write_state("monitoring.json", &monitoring).unwrap();
    store
        .write_state(
            "actions.json",
            &json!({"finals": [], "effects": [], "observations": []}),
        )
        .unwrap();
}

#[derive(Default, Debug)]
struct Load {
    /// Wall time spent per event kind (ms).
    spent: std::collections::BTreeMap<&'static str, u128>,
    payload: usize,
    gates: usize,
    saves: usize,
    snapshots: usize,
    ticks: usize,
}

#[test]
#[ignore = "live-size measurement; run in release with --ignored --nocapture"]
fn live_size_progress_and_polling_cost() {
    let seconds: u64 = std::env::var("PR_SNIPER_BENCH_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let root = tempfile::tempdir().unwrap();
    let store = Mutex::new(Store::new(root.path().into()));
    live_profile(&store.lock().unwrap());
    let size = |name: &str| {
        std::fs::metadata(root.path().join("state").join(name))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    eprintln!(
        "fixture: reviews {} B, publications {} B, monitoring {} B, queue {} B",
        size("reviews.json"),
        size("publications.json"),
        size("monitoring.json"),
        size("queue.json")
    );
    let footprint = || -> u64 {
        Store::saved_files(&root.path().join("state"))
            .iter()
            .map(|file| std::fs::metadata(file).unwrap().len())
            .sum()
    };
    eprintln!("footprint before: {} B", footprint());
    // Warm start: the app has already loaded once when the panel opens.
    {
        let store = store.lock().unwrap();
        let started = Instant::now();
        let mut pruned = 0;
        loop {
            // PRUNE: startup maintenance passes until the backlog is gone.
            let count = crate::retention::prune_terminal(&store, true, 2).unwrap();
            pruned += count;
            if count == 0 {
                break;
            }
        }
        // Progress first written by this process starts its coalescing window.
        let mut reviews = store.load_reviews().unwrap();
        if let Some(run) = reviews
            .iter_mut()
            .find(|r| r.operation.state == OperationState::Running)
        {
            run.phase = "Starting".into();
        }
        store.save_reviews(&reviews).unwrap();
        eprintln!(
            "pruned {pruned} closed or merged PRs in {:?}; footprint after: {} B",
            started.elapsed(),
            footprint()
        );
        store.load_reviews().unwrap();
        crate::queue::snapshot(&store, vec![]).unwrap();
        store.take_state_reads();
        store.take_state_writes();
        store.take_bytes_written();
        store.take_parses();
    }
    let stop = AtomicBool::new(false);
    let load = Mutex::new(Load::default());
    let samples = Mutex::new(Vec::new());
    let started = Instant::now();
    let base = usage::sample();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut last = base;
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_secs(1));
                let now = usage::sample();
                samples
                    .lock()
                    .unwrap()
                    .push(((now.0 - last.0) as f64 / 1e9 * 100.0, now.1, now.2));
                last = now;
            }
        });
        scope.spawn(|| {
            let mut step = 0usize;
            while !stop.load(Ordering::SeqCst) {
                let cpu = usage::thread_cpu();
                {
                    let store = store.lock().unwrap();
                    // Each of the 3 running reviews reports progress.
                    let mut reviews = store.load_reviews().unwrap();
                    let running = reviews
                        .iter_mut()
                        .filter(|r| r.operation.state == OperationState::Running)
                        .nth(step % 3)
                        .unwrap();
                    running.phase = format!("Reviewing file {step}");
                    store.save_reviews(&reviews).unwrap();
                }
                step += 1;
                let mut load = load.lock().unwrap();
                load.saves += 1;
                *load.spent.entry("progress save").or_default() +=
                    (usage::thread_cpu() - cpu) as u128;
                drop(load);
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        // Attempt gates: every 100 ms per running review (`monitor_attempt`).
        scope.spawn(|| {
            let runs: Vec<_> = store
                .lock()
                .unwrap()
                .load_reviews()
                .unwrap()
                .into_iter()
                .filter(|r| r.operation.state == OperationState::Running)
                .collect();
            let validated: Vec<_> = runs
                .iter()
                .map(|_| crate::review::ValidatedSelection::default())
                .collect();
            while !stop.load(Ordering::SeqCst) {
                let cpu = usage::thread_cpu();
                for (run, validated) in runs.iter().zip(&validated) {
                    // As `review::host::local_gate`: no Store lock while
                    // nothing it validated has changed.
                    if !validated.unchanged() {
                        let store = store.lock().unwrap();
                        crate::review::validate_execution_selection_cached(&store, run, validated)
                            .unwrap();
                    }
                }
                let mut load = load.lock().unwrap();
                load.gates += runs.len();
                *load.spent.entry("tool-callback gate").or_default() +=
                    (usage::thread_cpu() - cpu) as u128;
                drop(load);
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                let cpu = usage::thread_cpu();
                let view = store.lock().unwrap().freeze().unwrap();
                let snapshot = crate::queue::snapshot(&view, vec![]).unwrap();
                // What the panel receives over IPC.
                let payload = serde_json::to_vec(&crate::review::interned(&snapshot).unwrap())
                    .unwrap()
                    .len();
                load.lock().unwrap().payload = payload;
                drop(snapshot);
                let view = store.lock().unwrap().freeze().unwrap();
                crate::notifications::host::view(&view, Err("bench".into())).unwrap();
                let ai = crate::capacity::Coordinator::default();
                let view = store.lock().unwrap().freeze().unwrap();
                ai.snapshot(&view, NOW).unwrap();
                let mut load = load.lock().unwrap();
                load.snapshots += 1;
                *load.spent.entry("UI snapshot").or_default() +=
                    (usage::thread_cpu() - cpu) as u128;
                drop(load);
                std::thread::sleep(Duration::from_secs(POLL_SECONDS));
            }
        });
        // Scheduler: the notifications pass projects the queue each tick;
        // progress saves wake no other coordinator (see `wake`).
        scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                let cpu = usage::thread_cpu();
                {
                    let store = store.lock().unwrap();
                    // Scheduler tick: deferred progress becomes due.
                    let _ = store.flush_progress(false);
                }
                let view = store.lock().unwrap().freeze().unwrap();
                let snapshot = crate::queue::snapshot(&view, vec![]).unwrap();
                crate::notifications::observe_queue(&view, &snapshot, NOW).ok();
                let mut load = load.lock().unwrap();
                load.ticks += 1;
                *load.spent.entry("scheduler projection").or_default() +=
                    (usage::thread_cpu() - cpu) as u128;
                drop(load);
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        std::thread::sleep(Duration::from_secs(seconds));
        stop.store(true, Ordering::SeqCst);
    });
    let elapsed = started.elapsed().as_secs_f64();
    let end = usage::sample();
    let minutes = elapsed / 60.0;
    let samples = samples.into_inner().unwrap();
    let cpu: Vec<f64> = samples.iter().map(|s| s.0).collect();
    let average = (end.0 - base.0) as f64 / 1e9 / elapsed * 100.0;
    let peak = cpu.iter().copied().fold(0.0, f64::max);
    let rss = |pick: fn(&(f64, u64, u64)) -> u64| {
        let values: Vec<u64> = samples.iter().map(pick).collect();
        (
            values.iter().sum::<u64>() / values.len().max(1) as u64 / (1 << 20),
            values.iter().copied().max().unwrap_or(0) / (1 << 20),
        )
    };
    let store = store.into_inner().unwrap();
    let reads = store.take_state_reads();
    let writes = store.take_state_writes();
    eprintln!(
        "state writes per minute: {:?}",
        writes
            .iter()
            .map(|(k, v)| (k.as_str(), (*v as f64 / minutes).round()))
            .collect::<Vec<_>>()
    );
    let parses = store.take_parses();
    let heavy = |name: &str| *reads.get(name).unwrap_or(&0) as f64 / minutes;
    let load = load.into_inner().unwrap();
    eprintln!(
        "cpu ms per minute by event: {:?}",
        load.spent
            .iter()
            .map(|(k, v)| (*k, (*v as f64 / 1e6 / minutes).round()))
            .collect::<Vec<_>>()
    );
    eprintln!("load: {load:?} over {elapsed:.0} s");
    let mut windows: Vec<f64> = cpu
        .chunks(5)
        .map(|w| w.iter().sum::<f64>() / w.len() as f64)
        .collect();
    windows.sort_by(f64::total_cmp);
    let pick = |q: f64| windows[((windows.len() - 1) as f64 * q).round() as usize];
    eprintln!("cpu: average {average:.1}%, peak {peak:.1}% (1 s samples)");
    eprintln!(
        "cpu per 5 s window: p50 {:.1}%, p95 {:.1}%, max {:.1}%",
        pick(0.5),
        pick(0.95),
        pick(1.0)
    );
    let series: Vec<u64> = samples
        .iter()
        .step_by(60)
        .map(|s| s.1 / (1 << 20))
        .collect();
    eprintln!("resident MB per minute: {series:?}");
    eprintln!(
        "disk: {:.1} MB written per minute (OS counter), {:.1} MB per minute through Store",
        (end.3 - base.3) as f64 / minutes / 1e6,
        store.take_bytes_written() as f64 / minutes / 1e6
    );
    eprintln!(
        "reads per minute: reviews {:.0}, publications {:.0}, monitoring {:.0}, retention {:.0}",
        heavy("reviews.json"),
        heavy("publications.json"),
        heavy("monitoring.json"),
        heavy("retention.json")
    );
    eprintln!(
        "decodes per minute: {:?}",
        parses
            .iter()
            .map(|(k, v)| (k.as_str(), (*v as f64 / minutes).round()))
            .collect::<Vec<_>>()
    );
    eprintln!(
        "memory MB (avg, peak): resident {:?}, footprint {:?}",
        rss(|s| s.1),
        rss(|s| s.2)
    );
}

/// Prunes a copy of a real profile (`PR_SNIPER_BENCH_ROOT`, never the live
/// one) as startup maintenance would, and reports its footprint.
#[test]
#[ignore = "measurement on a copied profile; set PR_SNIPER_BENCH_ROOT"]
fn copied_profile_prune_footprint() {
    let Some(root) = std::env::var_os("PR_SNIPER_BENCH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let footprint = || -> u64 {
        Store::saved_files(&root.join("state"))
            .iter()
            .map(|file| std::fs::metadata(file).unwrap().len())
            .sum()
    };
    let store = Store::new(root.clone());
    eprintln!("footprint before: {} B", footprint());
    let started = Instant::now();
    let mut pruned = 0;
    let mut slowest = Duration::ZERO;
    let before = store.load_reviews().unwrap().len();
    eprintln!("runs before: {before}");
    while std::env::var_os("PR_SNIPER_BENCH_NO_PRUNE").is_none() {
        let pass = Instant::now();
        let count = crate::retention::prune_terminal(&store, true, 1).unwrap();
        eprintln!(
            "pass: {count} pruned, {} runs",
            store.load_reviews().unwrap().len()
        );
        slowest = slowest.max(pass.elapsed());
        pruned += count;
        if count == 0 {
            break;
        }
    }
    // The first save migrates reviews to segments.
    let reviews = store.load_reviews().unwrap();
    store.save_reviews(&reviews).unwrap();
    eprintln!(
        "pruned {pruned} PRs in {:?} (slowest pass {slowest:?}); {} runs kept; footprint after: {} B",
        started.elapsed(),
        reviews.len(),
        footprint()
    );
    let snapshot = crate::queue::snapshot(&store.freeze().unwrap(), vec![]).unwrap();
    eprintln!(
        "panel payload: {} B (was {} B)",
        serde_json::to_vec(&crate::review::interned(&snapshot).unwrap())
            .unwrap()
            .len(),
        serde_json::to_vec(&snapshot).unwrap().len()
    );
}
