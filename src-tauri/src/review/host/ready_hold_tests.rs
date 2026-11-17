//! A Ready PR stays under the heartbeat. When its latest read of the same head
//! is not mergeable it waits for the author with a typed hold, and returns to
//! Ready once the fact clears: no review, comment or notification of its own.

use super::stale_revision_tests::{
    configured, finished, poll_result, pull, seq, snapshot, teammate_entries, teammate_report,
    GREEN, NOW, PRIMARY, TEAMMATE,
};
use super::*;
use crate::github::state::PrSnapshot;
use crate::monitoring::Monitor;
use crate::notifications::{Category, Ledger, Phase};
use crate::queue::{hold::Hold, State};
use crate::storage::Store;
use serde_json::json;

const RED: [(&str, &str, &str); 2] = [
    ("macos-native", "COMPLETED", "SUCCESS"),
    ("browser (3/4)", "COMPLETED", "FAILURE"),
];

fn scan(monitor: &mut Monitor, store: &Store, state: PrSnapshot, at: i64) {
    let ticket = monitor.prepare_checks(store, at, true).unwrap().remove(0);
    let mut listed = pull(state.head.chars().next().unwrap());
    listed.draft = state.draft;
    let mut result = poll_result(vec![listed]);
    result.states.insert("9".into(), state);
    monitor.finish(store, ticket, Ok(result), at + 1).unwrap();
}

/// The assignment's newest entry, dispatched into a running attempt.
fn dispatch(store: &Store, assignment: &str, now: i64) -> ReviewRun {
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.assignment_id.as_deref() == Some(assignment))
        .max_by_key(|j| j.work.as_ref().map(|w| w.state_seq))
        .unwrap();
    let key = key(&job, assignment);
    request(store, &key, true, now).unwrap();
    prepare_dispatch(store, &key, now).unwrap().unwrap()
}

fn sign_off(store: &Store, at: i64) {
    let teammate = dispatch(store, TEAMMATE, at);
    complete(store, &teammate.operation.id, Ok(teammate_report()), at + 1).unwrap();
    let primary = dispatch(store, PRIMARY, at + 2);
    // The packet the primary read: the PR state the heartbeat last saw.
    let read = store
        .load_pr_states()
        .unwrap()
        .get(&crate::retention::Binding::job(&primary.job))
        .and_then(|r| r.hash.clone());
    let mut reviews = store.load_reviews().unwrap();
    let run = reviews
        .iter_mut()
        .find(|r| r.operation.id == primary.operation.id)
        .unwrap();
    run.mode = crate::review::RunMode::Packet;
    run.packet = Some(crate::packet::PacketRecord {
        state_id: "0:hash".into(),
        hash: "h".into(),
        schema: 1,
        tool_set: vec![],
        exits_offered: vec![],
        exits_denied: vec![],
        state_hash: read,
        state_marks: None,
        wait_snapshot: None,
    });
    store.save_reviews(&reviews).unwrap();
    let result = finished(json!({"exit":{"exit":"ready_for_human","summary":"Clean."}}));
    complete(store, &primary.operation.id, Ok(result), at + 3).unwrap();
}

/// The current Iteration's item.
fn current(store: &Store, monitor: &Monitor) -> crate::queue::Item {
    let snapshot = crate::queue::snapshot(store, monitor.snapshot()).unwrap();
    let tracked = &snapshot.tracked[0].item_id;
    snapshot
        .items
        .into_iter()
        .find(|item| &item.id == tracked)
        .unwrap()
}

/// Observes the queue as the notifier does, delivering every queued notice.
fn notices(store: &Store, monitor: &Monitor, ledger: &mut Ledger, at: i64) -> Vec<Category> {
    let snapshot = crate::queue::snapshot(store, monitor.snapshot()).unwrap();
    ledger
        .observe(&crate::notifications::frames(&snapshot), at)
        .unwrap();
    for notice in &mut ledger.notices {
        if notice.phase == Phase::Queued {
            notice.phase = Phase::AcceptedUnconfirmed;
        }
    }
    ledger.notices.iter().map(|n| n.event.category).collect()
}

fn enabled() -> Ledger {
    Ledger {
        enabled: true,
        ..Ledger::default()
    }
}

fn green() -> PrSnapshot {
    snapshot('a', &GREEN, &[])
}

#[test]
fn a_ready_pr_that_stops_being_mergeable_waits_quietly_and_returns_on_the_same_head() {
    let cases: [(fn(&mut PrSnapshot), Hold); 3] = [
        (|s| s.mergeable = "CONFLICTING".into(), Hold::Conflicts),
        (|s| *s = snapshot('a', &RED, &[]), Hold::CiFailed),
        (|s| s.behind = true, Hold::BehindBase),
    ];
    for (turn, expected) in cases {
        let (_root, store, mut monitor) = configured(false);
        let mut ledger = enabled();
        scan(&mut monitor, &store, green(), NOW);
        sign_off(&store, NOW + 2);
        let ready = current(&store, &monitor);
        assert_eq!(ready.state, State::MachineSignedOff);
        assert_eq!(ready.hold, None);
        assert_eq!(
            notices(&store, &monitor, &mut ledger, NOW + 10),
            [Category::Ready]
        );
        let read = seq(&store, &dispatch_job(&store));
        let reviews = store.load_reviews().unwrap().len();

        let mut moved = green();
        turn(&mut moved);
        scan(&mut monitor, &store, moved, NOW + 100);
        let held = current(&store, &monitor);
        assert_eq!(held.state, State::WaitingForAuthor, "{expected:?}");
        assert_eq!(held.hold, Some(expected));
        assert_eq!(held.rest, None);
        // Not a review state: no new pass, teammate re-run or comment.
        assert_eq!(seq(&store, &dispatch_job(&store)), read, "{expected:?}");
        assert_eq!(teammate_entries(&store), 1);
        assert_eq!(store.load_reviews().unwrap().len(), reviews);
        assert!(store.load_publications().unwrap().is_empty());
        assert_eq!(
            notices(&store, &monitor, &mut ledger, NOW + 110),
            [Category::Ready]
        );

        scan(&mut monitor, &store, green(), NOW + 200);
        let back = current(&store, &monitor);
        assert_eq!(back.state, State::MachineSignedOff, "{expected:?}");
        assert_eq!(back.hold, None);
        assert_eq!(seq(&store, &dispatch_job(&store)), read);
        // Ready announces an Iteration once, whatever it went through.
        assert_eq!(
            notices(&store, &monitor, &mut ledger, NOW + 210),
            [Category::Ready]
        );
    }
}

#[test]
fn a_new_head_on_a_held_pr_takes_the_new_iteration_path() {
    let (_root, store, mut monitor) = configured(false);
    scan(&mut monitor, &store, green(), NOW);
    sign_off(&store, NOW + 2);
    let signed = current(&store, &monitor).id;
    let mut conflicting = green();
    conflicting.mergeable = "CONFLICTING".into();
    scan(&mut monitor, &store, conflicting, NOW + 100);
    assert_eq!(current(&store, &monitor).hold, Some(Hold::Conflicts));

    scan(&mut monitor, &store, snapshot('b', &GREEN, &[]), NOW + 200);
    let snapshot = crate::queue::snapshot(&store, monitor.snapshot()).unwrap();
    let old = snapshot.items.iter().find(|i| i.id == signed).unwrap();
    assert_eq!(old.state, State::Stale);
    assert_eq!(old.hold, None);
    let new = current(&store, &monitor);
    assert_ne!(new.id, signed);
    assert_eq!(new.state, State::Queued);
    assert_eq!(teammate_entries(&store), 2, "the new head is reviewed");
}

#[test]
fn a_draft_never_enters_ready_and_announces_once_when_marked_ready() {
    let (_root, store, mut monitor) = configured(true);
    let mut ledger = enabled();
    let mut draft = green();
    draft.draft = true;
    scan(&mut monitor, &store, draft, NOW);
    sign_off(&store, NOW + 2);
    let held = current(&store, &monitor);
    assert_eq!(held.state, State::WaitingForAuthor);
    assert_eq!(held.hold, Some(Hold::Draft));
    assert!(notices(&store, &monitor, &mut ledger, NOW + 10).is_empty());

    // Marked ready on the same head: the normal pass for the new PR state.
    scan(&mut monitor, &store, green(), NOW + 100);
    let pass = current(&store, &monitor);
    assert_eq!(pass.hold, None);
    assert_eq!(pass.state, State::Queued);
    assert!(notices(&store, &monitor, &mut ledger, NOW + 101).is_empty());
    sign_off(&store, NOW + 102);
    let ready = current(&store, &monitor);
    assert_eq!(ready.state, State::MachineSignedOff);
    assert_eq!(ready.job.head_sha, "a".repeat(40));
    assert_eq!(
        notices(&store, &monitor, &mut ledger, NOW + 110),
        [Category::Ready]
    );
    scan(&mut monitor, &store, green(), NOW + 200);
    assert_eq!(
        notices(&store, &monitor, &mut ledger, NOW + 210),
        [Category::Ready]
    );
}

fn dispatch_job(store: &Store) -> ReviewRun {
    store
        .load_reviews()
        .unwrap()
        .into_iter()
        .find(|r| r.assignment_id == PRIMARY)
        .unwrap()
}
