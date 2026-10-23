use super::*;
use crate::monitoring::{OperationState, WAITING_HUMAN_START};
use crate::wake::tests::{job, review, settings};

fn profile(store: &Store, runs: u64) -> Vec<ReviewRun> {
    let settings = settings();
    store.save_settings(&settings).unwrap();
    let reviews: Vec<ReviewRun> = (1..=runs)
        .map(|number| {
            let mut run = review(&settings, &job(number, 1, WAITING_HUMAN_START), "p");
            run.selection.doctrine = Some(format!("{}{}", number % 3, "d".repeat(20_000)));
            run
        })
        .collect();
    // Legacy layout: one array.
    store.write_state(MANIFEST, &reviews).unwrap();
    reviews
}

fn manifest(store: &Store) -> Vec<String> {
    let bytes = store.read_state(MANIFEST).unwrap();
    serde_json::from_slice::<Manifest>(&bytes).unwrap().runs
}

#[test]
fn legacy_array_migrates_on_save_with_the_same_logical_view() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let reviews = profile(&store, 12);
    assert_eq!(store.load_reviews().unwrap(), reviews);
    store.save_reviews(&reviews).unwrap();
    assert_eq!(manifest(&store).len(), 12);
    // 12 runs share 3 distinct selections; no feedback context.
    let parts = segment_files(&store)
        .into_iter()
        .filter(|name| name.starts_with("part-"))
        .count();
    assert_eq!(parts, 3);
    assert_eq!(store.load_reviews().unwrap(), reviews);
    // After a restart the same view decodes from disk.
    assert_eq!(
        Store::new(root.path().into()).load_reviews().unwrap(),
        reviews
    );
    let bytes: u64 = Store::saved_files(&root.path().join("state/reviews"))
        .iter()
        .map(|file| std::fs::metadata(file).unwrap().len())
        .sum();
    assert!(bytes * 2 < 12 * 20_000, "{bytes}");
}

#[test]
fn progress_saves_are_coalesced_and_write_only_the_changed_run() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut reviews = profile(&store, 40);
    reviews[7].operation.state = OperationState::Running;
    store.save_reviews(&reviews).unwrap();
    let full: usize = serde_json::to_vec(&reviews).unwrap().len();
    store.take_bytes_written();
    store.take_parses();
    store.take_state_writes();
    let before = segment_files(&store);
    for step in 0..5 {
        let mut reviews = store.load_reviews().unwrap();
        reviews[7].phase = format!("Reviewing file {step}");
        store.save_reviews(&reviews).unwrap();
    }
    // Within `PROGRESS_FLUSH`: nothing written, nothing decoded, and every
    // reader, including a frozen copy, sees the latest progress.
    assert_eq!(store.take_state_writes(), BTreeMap::new());
    assert_eq!(store.take_bytes_written(), 0);
    assert_eq!(store.load_reviews().unwrap()[7].phase, "Reviewing file 4");
    let view = store.freeze().unwrap();
    assert_eq!(view.load_reviews().unwrap()[7].phase, "Reviewing file 4");
    assert!(store.take_parses().keys().all(|key| key == MANIFEST));
    // The scheduler flush writes the run and the manifest once.
    store.flush_progress(true).unwrap();
    store.flush_progress(true).unwrap();
    assert_eq!(store.take_state_writes().get(MANIFEST), Some(&1));
    let written = store.take_bytes_written();
    assert!(written * 20 < full, "{written} vs {full}");
    assert_eq!(segment_files(&store).len(), before.len() + 1);
    drop(view);
    assert_eq!(
        Store::new(root.path().into()).load_reviews().unwrap()[7].phase,
        "Reviewing file 4"
    );
    // A transition is written at once, with the pending progress.
    let mut reviews = store.load_reviews().unwrap();
    reviews[7].phase = "Reviewing file 5".into();
    store.save_reviews(&reviews).unwrap();
    reviews[7].operation.state = OperationState::Completed;
    store.save_reviews(&reviews).unwrap();
    let saved = Store::new(root.path().into()).load_reviews().unwrap();
    assert_eq!(saved[7].operation.state, OperationState::Completed);
    assert_eq!(saved[7].phase, "Reviewing file 5");
}

#[test]
fn snapshots_decode_each_change_once_from_frozen_copies() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut reviews = profile(&store, 20);
    store.save_reviews(&reviews).unwrap();
    crate::storage::file_identity::settle();
    crate::queue::snapshot(&store.freeze().unwrap(), vec![]).unwrap();
    store.take_parses();
    // Unchanged state: snapshots of fresh frozen copies decode nothing.
    for _ in 0..3 {
        let view = store.freeze().unwrap();
        crate::queue::snapshot(&view, vec![]).unwrap();
        crate::capacity::Coordinator::default()
            .snapshot(&view, 0)
            .unwrap();
    }
    assert_eq!(store.take_parses(), BTreeMap::new());
    // One run changed: at most the small manifest decodes again, once; the
    // changed run came from its write, no other run is decoded.
    reviews[3].phase = "Changed".into();
    store.save_reviews(&reviews).unwrap();
    crate::storage::file_identity::settle();
    for _ in 0..3 {
        crate::queue::snapshot(&store.freeze().unwrap(), vec![]).unwrap();
    }
    let parses = store.take_parses();
    assert!(!parses.contains_key("reviews/run"), "{parses:?}");
    assert!(parses.values().all(|&count| count <= 1), "{parses:?}");
}

#[test]
fn out_of_band_rewrites_are_never_served_from_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    profile(&store, 3);
    crate::storage::file_identity::settle();
    assert_eq!(store.load_reviews().unwrap()[0].phase, "Complete");
    let path = root.path().join("state").join(MANIFEST);
    crate::storage::file_identity::rewrite_foreign(&path, false, |bytes| {
        let at = bytes.windows(8).position(|w| w == b"Complete").unwrap();
        bytes[at..at + 8].copy_from_slice(b"Finished");
    });
    assert_eq!(store.load_reviews().unwrap()[0].phase, "Finished");
}

#[test]
fn collection_waits_for_frozen_copies_that_may_read_old_runs() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut reviews = profile(&store, 4);
    store.save_reviews(&reviews).unwrap();
    let old = manifest(&store)[1].clone();
    let view = store.freeze().unwrap();
    reviews[1].error = Some("Changed".into());
    store.save_reviews(&reviews).unwrap();
    assert!(segment_files(&store).contains(&old));
    // The copy reads its own point in time from disk, not the cache.
    view.cache.reviews.lock().unwrap().runs.clear();
    assert_eq!(view.load_reviews().unwrap()[1].error, None);
    drop(view);
    reviews[2].error = Some("Changed".into());
    store.save_reviews(&reviews).unwrap();
    let files = segment_files(&store);
    assert!(!files.contains(&old));
    let referenced: HashSet<String> = manifest(&store).into_iter().collect();
    assert!(files
        .iter()
        .filter(|name| name.starts_with("run-"))
        .all(|name| referenced.contains(name)));
}

#[test]
fn interrupted_saves_keep_the_committed_view_and_are_swept() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut reviews = profile(&store, 3);
    store.save_reviews(&reviews).unwrap();
    // A crash after writing run files but before the manifest commit.
    let directory = root.path().join("state/reviews");
    let orphan = format!("run-{}.json", "0".repeat(32));
    std::fs::write(directory.join(&orphan), b"{}").unwrap();
    std::fs::write(directory.join("run-x.json.pr-sniper-write-y.partial"), b"{").unwrap();
    let fresh = Store::new(root.path().into());
    assert_eq!(fresh.load_reviews().unwrap(), reviews);
    reviews[0].error = Some("Changed".into());
    fresh.save_reviews(&reviews).unwrap();
    let files = segment_files(&fresh);
    assert!(!files.contains(&orphan) && files.iter().all(|n| !n.ends_with(".partial")));
    // A failed manifest write commits nothing.
    fresh.fail_state_write(MANIFEST, 1);
    reviews[0].error = Some("Lost".into());
    assert!(fresh.save_reviews(&reviews).is_err());
    assert_eq!(
        Store::new(root.path().into()).load_reviews().unwrap()[0].error,
        Some("Changed".into())
    );
}

#[test]
fn panel_snapshots_send_each_repeated_selection_once() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let reviews = profile(&store, 9);
    store.save_reviews(&reviews).unwrap();
    let runs = Store::new(root.path().into()).load_reviews().unwrap();
    let plain = serde_json::to_value(&runs).unwrap();
    let sent = crate::review::interned(&runs).unwrap();
    assert_eq!(sent["shared"].as_object().unwrap().len(), 3);
    assert!(
        serde_json::to_vec(&sent).unwrap().len() * 2 < serde_json::to_vec(&plain).unwrap().len()
    );
    fn restore(node: &mut Value, shared: &Map<String, Value>) {
        match node {
            Value::Array(items) => items.iter_mut().for_each(|n| restore(n, shared)),
            Value::Object(map) if map.len() == 1 && map.contains_key("$shared") => {
                *node = shared[map["$shared"].as_str().unwrap()].clone();
            }
            Value::Object(map) => map.values_mut().for_each(|n| restore(n, shared)),
            _ => {}
        }
    }
    let shared = sent["shared"].as_object().unwrap().clone();
    let mut restored = sent["interned"].clone();
    restore(&mut restored, &shared);
    assert_eq!(restored, plain);
    // Outside `interned`, serialization is unchanged.
    assert_eq!(serde_json::to_value(&runs).unwrap(), plain);
}

#[test]
fn caches_and_files_stay_bounded_over_a_long_running_review() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut reviews = profile(&store, 30);
    reviews[4].operation.state = OperationState::Running;
    store.save_reviews(&reviews).unwrap();
    let files = segment_files(&store).len();
    for step in 0..200 {
        let view = store.freeze().unwrap();
        crate::queue::snapshot(&view, vec![]).unwrap();
        let mut reviews = store.load_reviews().unwrap();
        reviews[4].phase = format!("Reviewing file {step}");
        if step % 20 == 0 {
            // A transition: written at once with the pending progress.
            reviews[4].operation.attempt_count += 1;
        }
        store.save_reviews(&reviews).unwrap();
        if step % 50 == 0 {
            store.flush_progress(true).unwrap();
        }
    }
    store.flush_progress(true).unwrap();
    // One run file per run, no part growth, no retained readers or stale
    // decoded runs: memory and disk return to the size of the live view.
    assert_eq!(segment_files(&store).len(), files);
    let segments = store.cache.reviews.lock().unwrap();
    assert_eq!(segments.runs.len(), 30);
    assert!(segments.parts.len() <= 3);
    assert!(segments.garbage.is_empty());
    drop(segments);
    assert_eq!(store.cache.oldest_reader(), None);
    assert!(store.progress.lock().unwrap().pending.is_none());
}
