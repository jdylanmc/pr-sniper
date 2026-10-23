//! Live-size cost measurement (ignored; run in release):
//! `cargo test --release --lib storage::cost_bench -- --ignored --nocapture`.
//! A running review saves progress every 1.5 s while the panel polls the
//! queue, notification and automation snapshots every second and the
//! scheduler ticks every 5 s. Reports CPU, bytes written, decodes and memory.

use crate::monitoring::{OperationState, QueueState, WAITING_HUMAN_START, WAITING_SUPERSEDED};
use crate::storage::Store;
use crate::wake::tests::{job, review, settings, Mode, Sim};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PRS: u64 = 460;
const PASSES: u64 = 3;
const SELECTIONS: usize = 31;
const CONTEXTS: usize = 152;
const PUBLICATIONS: usize = 174;
const OPERATIONS: usize = 4082;
const NOW: i64 = 1_800_000_000;

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
    for number in 1..=PRS {
        for pass in 1..=PASSES {
            let waiting = if pass == PASSES {
                WAITING_HUMAN_START
            } else {
                WAITING_SUPERSEDED
            };
            let entry = job(number, pass, waiting);
            let index = reviews.len();
            if index < 1376 {
                let mut run = review(&settings, &job(number, pass, WAITING_HUMAN_START), "p");
                run.selection.doctrine =
                    Some(format!("{}{}", index % SELECTIONS, "d".repeat(9800)));
                run.selection.agent.prompt =
                    format!("{}{}", index % SELECTIONS, "q".repeat(12_000));
                run.feedback_context =
                    Some(serde_json::from_value(contexts[index % CONTEXTS].clone()).unwrap());
                if index % 340 == 0 {
                    run.operation.state = OperationState::Running;
                }
                reviews.push(run);
            }
            queue.jobs.push(entry);
        }
    }
    queue.next_enqueue_order = (PRS + 1) * 1000;
    let publications: Vec<_> = reviews
        .iter()
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
    // Warm start: the app has already loaded once when the panel opens.
    {
        let store = store.lock().unwrap();
        crate::review::restore(&store).unwrap();
        store.take_state_reads();
        store.take_state_writes();
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
                {
                    let store = store.lock().unwrap();
                    let mut reviews = store.load_reviews().unwrap();
                    let running = reviews
                        .iter_mut()
                        .find(|r| r.operation.state == OperationState::Running)
                        .unwrap();
                    running.phase = format!("Reviewing file {step}");
                    store.save_reviews(&reviews).unwrap();
                }
                step += 1;
                load.lock().unwrap().saves += 1;
                std::thread::sleep(Duration::from_millis(1500));
            }
        });
        scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                let view = store.lock().unwrap().freeze().unwrap();
                crate::queue::snapshot(&view, vec![]).unwrap();
                let view = store.lock().unwrap().freeze().unwrap();
                crate::notifications::host::view(&view, Err("bench".into())).unwrap();
                let ai = crate::capacity::Coordinator::default();
                let view = store.lock().unwrap().freeze().unwrap();
                ai.snapshot(&view, NOW).unwrap();
                load.lock().unwrap().snapshots += 1;
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        scope.spawn(|| {
            let mut sim = Sim::new(&store, Mode::Gated);
            let start = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                sim.now = NOW + start.elapsed().as_secs() as i64;
                sim.tick();
                load.lock().unwrap().ticks += 1;
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
    let parses = store.take_parses();
    let heavy = |name: &str| *reads.get(name).unwrap_or(&0) as f64 / minutes;
    eprintln!("load: {:?} over {elapsed:.0} s", load.into_inner().unwrap());
    eprintln!("cpu: average {average:.1}%, peak {peak:.1}% (1 s samples)");
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
