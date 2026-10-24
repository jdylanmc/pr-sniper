use super::*;
use crate::github::state::{
    PrSnapshot, SnapshotCheck, SnapshotComment, SnapshotReview, SnapshotThread,
};

fn binding() -> Binding {
    Binding {
        provider: "github".into(),
        configuration_id: "c".into(),
        account_id: "1".into(),
        repository_id: "2".into(),
        pull_request_id: "3".into(),
    }
}

fn snapshot() -> PrSnapshot {
    PrSnapshot {
        required_checks: vec![],
        number: 1,
        pull_request_id: "3".into(),
        title: "t".into(),
        body: "b".into(),
        labels: vec![],
        head_ref: "feature".into(),
        head: "h".into(),
        base: "s".into(),
        state: "OPEN".into(),
        draft: false,
        mergeable: "MERGEABLE".into(),
        review_decision: None,
        reviews: vec![SnapshotReview {
            actor: "a".into(),
            state: "APPROVED".into(),
            head: Some("h".into()),
        }],
        checks: vec![SnapshotCheck {
            name: "ci".into(),
            status: "COMPLETED".into(),
            conclusion: "SUCCESS".into(),
        }],
        check_total: 1,
        threads: vec![SnapshotThread {
            id: "t1".into(),
            resolved: false,
            outdated: false,
            comments: vec![SnapshotComment {
                id: "20".into(),
                body: "x".into(),
                commit: Some("h".into()),
            }],
            comment_total: 1,
        }],
        thread_total: 1,
        comments: vec![SnapshotComment {
            id: "10".into(),
            body: "y".into(),
            commit: None,
        }],
        comment_total: 1,
    }
}

fn observe(states: &mut PrStates, snapshot: &PrSnapshot, reviewed: bool) -> Change {
    let normalized = snapshot.normalize();
    states.observe(
        &binding(),
        Observation {
            iteration_id: "i1",
            new_iteration: false,
            snapshot: Some(&normalized),
            already_reviewed: reviewed,
            now: 10,
        },
    )
}

#[test]
fn diff_table_queues_only_real_changes() {
    type Edit = fn(&mut PrSnapshot);
    let table: [(&str, Edit, bool); 11] = [
        ("no change", |_| {}, false),
        ("provider reorder", |s| s.reviews.reverse(), false),
        (
            "ci conclusion",
            |s| s.checks[0].conclusion = "FAILURE".into(),
            false,
        ),
        (
            "check started",
            |s| s.checks[0].status = "IN_PROGRESS".into(),
            false,
        ),
        ("mergeable", |s| s.mergeable = "CONFLICTING".into(), false),
        ("label", |s| s.labels.push("ready".into()), false),
        ("base moved", |s| s.base = "s2".into(), true),
        (
            "comment edit",
            |s| s.comments[0].body = "edited".into(),
            true,
        ),
        ("new comment", |s| s.comment_total += 1, true),
        ("thread resolved", |s| s.threads[0].resolved = true, true),
        ("body edit", |s| s.body = "new".into(), true),
    ];
    for (name, edit, changes) in table {
        let mut states = PrStates::default();
        assert_eq!(observe(&mut states, &snapshot(), true), Change::Adopted);
        let mut next = snapshot();
        edit(&mut next);
        let change = observe(&mut states, &next, true);
        assert_eq!(change != Change::None, changes, "{name}");
        assert_eq!(states.state_seq(&binding()), u64::from(changes), "{name}");
    }
}

#[test]
fn own_write_baseline_does_not_retrigger() {
    let mut states = PrStates::default();
    assert_eq!(observe(&mut states, &snapshot(), false), Change::Pending(1));
    let mut written = snapshot();
    written.comments.push(SnapshotComment {
        id: "99".into(),
        body: "own".into(),
        commit: None,
    });
    written.comment_total += 1;
    let baseline = written.normalize().hash();
    assert_eq!(states.consume(&binding(), 1, &baseline), Consumed::Consumed);
    assert_eq!(observe(&mut states, &written, false), Change::None);
    assert!(states.get(&binding()).unwrap().pending.is_none());
    assert_eq!(states.get(&binding()).unwrap().error_count, 0);
}

#[test]
fn state_moving_while_queued_overwrites_pending_without_another_entry() {
    let mut states = PrStates::default();
    assert_eq!(observe(&mut states, &snapshot(), false), Change::Pending(1));
    let mut moved = snapshot();
    moved.body = "new".into();
    assert_eq!(observe(&mut states, &moved, false), Change::Pending(2));
    assert_eq!(states.records.len(), 1);
    assert_eq!(
        states
            .get(&binding())
            .unwrap()
            .pending
            .as_ref()
            .unwrap()
            .seq,
        2
    );
}

#[test]
fn consume_of_a_moved_state_is_refused_and_toss_counts() {
    let mut states = PrStates::default();
    observe(&mut states, &snapshot(), false);
    let mut moved = snapshot();
    moved.body = "new".into();
    observe(&mut states, &moved, false);
    assert_eq!(states.consume(&binding(), 1, "x"), Consumed::Moved);
    assert!(states.get(&binding()).unwrap().baseline_hash.is_none());
    assert_eq!(states.toss(&binding()), 1);
    assert_eq!(states.toss(&binding()), 2);
    assert_eq!(states.consume(&binding(), 2, "x"), Consumed::Consumed);
    assert_eq!(states.get(&binding()).unwrap().error_count, 0);
}

#[test]
fn a_new_iteration_always_needs_a_pass_and_learns_its_hash_later() {
    let mut states = PrStates::default();
    assert_eq!(observe(&mut states, &snapshot(), true), Change::Adopted);
    let change = states.observe(
        &binding(),
        Observation {
            iteration_id: "i2",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 11,
        },
    );
    assert_eq!(change, Change::Pending(1));
    // The first real read fills the unknown hash instead of bumping again.
    assert_eq!(observe(&mut states, &snapshot(), true), Change::None);
    assert_eq!(states.state_seq(&binding()), 1);
}

#[test]
fn pr_state_survives_a_restart() {
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/pr-state/fixtures");
    std::fs::create_dir_all(&parent).unwrap();
    let dir = tempfile::tempdir_in(parent).unwrap();
    let store = Store::new(dir.path().into());
    let mut states = PrStates::default();
    observe(&mut states, &snapshot(), false);
    states.set_local_summary(&binding(), Some("ok".into()));
    store.save_pr_states(&states).unwrap();
    let restored = Store::new(dir.path().into()).load_pr_states().unwrap();
    assert_eq!(restored, states);
}

#[test]
fn consuming_records_when_and_a_new_head_keeps_it() {
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/pr-state/fixtures");
    std::fs::create_dir_all(&parent).unwrap();
    let dir = tempfile::tempdir_in(parent).unwrap();
    let store = Store::new(dir.path().into());
    let mut states = PrStates::default();
    observe(&mut states, &snapshot(), false);
    store.save_pr_states(&states).unwrap();
    let before = chrono::Utc::now().timestamp();
    assert_eq!(
        consume(&store, &binding(), 1, "x").unwrap(),
        Consumed::Consumed
    );
    let mut states = store.load_pr_states().unwrap();
    let at = states.get(&binding()).unwrap().consumed_at.unwrap();
    assert!(at >= before);
    assert_eq!(
        consume(&store, &binding(), 9, "y").unwrap(),
        Consumed::Moved
    );
    states.observe(
        &binding(),
        Observation {
            iteration_id: "i2",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 11,
        },
    );
    assert_eq!(states.get(&binding()).unwrap().consumed_at, Some(at));
}

#[test]
fn external_wait_survives_missing_observations_and_legacy_records_have_none() {
    let mut states = PrStates::default();
    let mut state = snapshot();
    state.checks[0].status = "QUEUED".into();
    state.checks[0].conclusion.clear();
    observe(&mut states, &state, false);
    assert_eq!(
        states.wait(
            &binding(),
            &state.normalize().hash(),
            ExternalWait {
                state_seq: 1,
                reason: crate::exits::WaitReason::Ci,
                assignment_id: "primary".into(),
                agent_id: "agent".into(),
                snapshot: state.normalize()
            }
        ),
        Consumed::Consumed
    );
    let before = states.clone();
    assert_eq!(
        states.observe(
            &binding(),
            Observation {
                iteration_id: "i1",
                new_iteration: false,
                snapshot: None,
                already_reviewed: true,
                now: 99,
            }
        ),
        Change::None
    );
    assert_eq!(
        states, before,
        "a failed/interrupted read cannot wake or clear waiting"
    );
    let mut old = serde_json::to_value(&states).unwrap();
    old["records"][0]
        .as_object_mut()
        .unwrap()
        .remove("external_wait");
    let loaded: PrStates = serde_json::from_value(old).unwrap();
    assert!(loaded.get(&binding()).unwrap().external_wait.is_none());
    assert!(loaded.get(&binding()).unwrap().exit.is_none());
}

#[test]
fn external_wait_draft_and_required_reviews_wake_on_observed_changes() {
    for reason in [
        crate::exits::WaitReason::Draft,
        crate::exits::WaitReason::RequiredReviews,
    ] {
        let mut states = PrStates::default();
        let mut state = snapshot();
        state.draft = reason == crate::exits::WaitReason::Draft;
        state.review_decision = Some("REVIEW_REQUIRED".into());
        observe(&mut states, &state, false);
        assert_eq!(
            states.wait(
                &binding(),
                &state.normalize().hash(),
                ExternalWait {
                    state_seq: 1,
                    reason,
                    assignment_id: "primary".into(),
                    agent_id: "agent".into(),
                    snapshot: state.normalize()
                }
            ),
            Consumed::Consumed
        );
        assert_eq!(observe(&mut states, &state, true), Change::None);
        state.draft = false;
        state.review_decision = Some("APPROVED".into());
        assert_eq!(observe(&mut states, &state, true), Change::Pending(2));
        assert_eq!(observe(&mut states, &state, true), Change::None);
        assert!(states.get(&binding()).unwrap().exit.is_none());
    }
}

#[test]
fn legacy_full_snapshot_hashes_translate_without_a_new_state() {
    let mut states = PrStates::default();
    assert_eq!(observe(&mut states, &snapshot(), true), Change::Adopted);
    let record = states.records.first_mut().unwrap();
    let old = snapshot().normalize().legacy_hash();
    record.baseline_hash = Some(old.clone());
    record.hash = Some(old);
    let mut next = snapshot();
    next.checks[0].status = "IN_PROGRESS".into();
    assert_eq!(observe(&mut states, &next, true), Change::None);
    let record = states.get(&binding()).unwrap();
    assert_eq!(record.baseline_hash, Some(next.normalize().hash()));
    assert_eq!(states.state_seq(&binding()), 0);
}

#[test]
fn a_ci_wait_woken_by_green_keeps_the_review_revision() {
    let wait = |states: &mut PrStates, state: &PrSnapshot| {
        let seq = states.state_seq(&binding());
        states.wait(
            &binding(),
            &state.normalize().hash(),
            ExternalWait {
                state_seq: seq,
                reason: crate::exits::WaitReason::Ci,
                assignment_id: "primary".into(),
                agent_id: "agent".into(),
                snapshot: state.normalize(),
            },
        )
    };
    let mut states = PrStates::default();
    let mut state = snapshot();
    state.checks[0].status = "QUEUED".into();
    state.checks[0].conclusion.clear();
    assert_eq!(observe(&mut states, &state, false), Change::Pending(1));
    assert_eq!(wait(&mut states, &state), Consumed::Consumed);
    state.labels.push("noise".into());
    state.mergeable = "UNKNOWN".into();
    assert_eq!(observe(&mut states, &state, true), Change::None);
    state.checks[0].status = "COMPLETED".into();
    state.checks[0].conclusion = "SUCCESS".into();
    assert_eq!(observe(&mut states, &state, true), Change::Pending(2));
    assert_eq!(
        states.revision_seq(&binding()),
        1,
        "teammate reports from seq 1 hold"
    );

    // A wake that also carries a review change is a new revision.
    let mut states = PrStates::default();
    state.checks[0].status = "QUEUED".into();
    state.checks[0].conclusion.clear();
    observe(&mut states, &state, false);
    assert_eq!(wait(&mut states, &state), Consumed::Consumed);
    state.checks[0].status = "COMPLETED".into();
    state.checks[0].conclusion = "SUCCESS".into();
    state.comment_total += 1;
    assert_eq!(observe(&mut states, &state, true), Change::Pending(2));
    assert_eq!(states.revision_seq(&binding()), 2);
}

fn with_ci(ci: &str) -> PrSnapshot {
    let mut state = snapshot();
    match ci {
        "none" => {
            state.checks.clear();
            state.check_total = 0;
        }
        "pending" => {
            state.checks[0].status = "IN_PROGRESS".into();
            state.checks[0].conclusion.clear();
        }
        "red" => state.checks[0].conclusion = "FAILURE".into(),
        _ => {}
    }
    state
}

fn consume_exit(states: &mut PrStates, sign_off: bool, fresh: &PrSnapshot) -> Consumed {
    let seq = states.state_seq(&binding());
    let fresh = fresh.normalize();
    states.consume_exit(
        &binding(),
        seq,
        &fresh.hash(),
        Some(ExitCi {
            sign_off,
            fresh: Some(&fresh),
        }),
    )
}

#[test]
fn green_ci_after_an_exit_without_it_wakes_the_primary_once() {
    for start in ["none", "red"] {
        let mut states = PrStates::default();
        assert_eq!(
            observe(&mut states, &with_ci(start), false),
            Change::Pending(1)
        );
        assert_eq!(
            consume_exit(&mut states, false, &with_ci(start)),
            Consumed::Consumed
        );
        assert_eq!(
            observe(&mut states, &with_ci("pending"), true),
            Change::None
        );
        assert_eq!(
            observe(&mut states, &with_ci("green"), true),
            Change::Pending(2),
            "{start}"
        );
        assert_eq!(states.revision_seq(&binding()), 1, "teammate reports hold");
        // The primary's next exit saw green: nothing re-arms, so no loop.
        assert_eq!(
            consume_exit(&mut states, false, &with_ci("green")),
            Consumed::Consumed
        );
        for ci in ["red", "green", "pending", "green"] {
            assert_eq!(
                observe(&mut states, &with_ci(ci), true),
                Change::None,
                "{start}"
            );
        }
    }
}

#[test]
fn a_sign_off_or_a_repository_without_ci_never_wakes_on_ci() {
    // Sign-off: it acted already.
    let mut states = PrStates::default();
    observe(&mut states, &with_ci("pending"), false);
    consume_exit(&mut states, true, &with_ci("pending"));
    assert_eq!(observe(&mut states, &with_ci("green"), true), Change::None);

    // No CI at all: one ready_for_human, then nothing however often it polls.
    let mut states = PrStates::default();
    observe(&mut states, &with_ci("none"), false);
    consume_exit(&mut states, false, &with_ci("none"));
    for _ in 0..3 {
        assert_eq!(observe(&mut states, &with_ci("none"), true), Change::None);
    }
}

#[test]
fn a_ci_wait_holds_while_no_check_is_reported() {
    let pending = with_ci("pending").normalize();
    let wait = ExternalWait {
        reason: crate::exits::WaitReason::Ci,
        state_seq: 1,
        assignment_id: "primary".into(),
        agent_id: "agent".into(),
        snapshot: pending.clone(),
    };
    assert!(wait.pending(&with_ci("none").normalize()));
    assert!(!wait.pending(&with_ci("green").normalize()));
    assert!(!wait.pending(&with_ci("red").normalize()));
}
