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
    let table: [(&str, Edit, bool); 7] = [
        ("no change", |_| {}, false),
        ("provider reorder", |s| s.reviews.reverse(), false),
        (
            "ci conclusion",
            |s| s.checks[0].conclusion = "FAILURE".into(),
            true,
        ),
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
