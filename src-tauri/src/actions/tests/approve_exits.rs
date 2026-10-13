use super::*;
use crate::{
    exits::{consume, ExitOutcome},
    packet::PacketRecord,
    pr_state::Consumed,
    retention::Binding,
    review::{ReviewOutput, RunMode},
};

fn exit_run(store: &Store, exit: ExitOutcome, include_self: bool) {
    let mut settings = store.load_settings().unwrap();
    let assignment = &mut settings.repositories[0].assignments[0];
    assignment.tools = Some(
        [
            "read_changes",
            "approve_pull_request",
            "approve_and_merge",
            "ready_for_human",
            "panic_exit",
        ]
        .map(String::from)
        .to_vec(),
    );
    assignment.actions.as_mut().unwrap().include_self = include_self;
    store.save_settings(&settings).unwrap();
    let mut reviews = store.load_reviews().unwrap();
    for run in &mut reviews {
        run.mode = RunMode::Packet;
        run.packet = Some(PacketRecord {
            state_id: "1:hash".into(),
            hash: "h".into(),
            schema: 1,
            tool_set: vec![],
            exits_offered: vec![],
            exits_denied: vec![],
            state_hash: Some("hash-read".into()),
        });
        run.result.as_mut().unwrap().output = ReviewOutput::from_exit(exit.clone(), None);
    }
    store.save_reviews(&reviews).unwrap();
    // The heartbeat record the exit's job was queued for.
    let job = &reviews[0].job;
    let mut states = store.load_pr_states().unwrap();
    let binding = Binding::job(job);
    if states.get(&binding).is_none() {
        states.observe(
            &binding,
            crate::pr_state::Observation {
                iteration_id: "i",
                new_iteration: true,
                snapshot: None,
                already_reviewed: false,
                now: 1,
            },
        );
    }
    assert_eq!(
        states.state_seq(&binding),
        consume::state_seq(job),
        "the job and the heartbeat record agree on the state"
    );
    store.save_pr_states(&states).unwrap();
}

fn setup(
    approve: bool,
    merge: bool,
    exit: ExitOutcome,
    include_self: bool,
) -> (tempfile::TempDir, Store, String) {
    let (root, store, item) = fixture(1, approve, merge);
    exit_run(&store, exit, include_self);
    (root, store, item)
}

fn approve_exit(merge: bool) -> ExitOutcome {
    if merge {
        ExitOutcome::ApproveAndMerge {
            comment: "Reviewed the diff. Ship it.".into(),
        }
    } else {
        ExitOutcome::Approve {
            comment: "Reviewed the diff. Looks right.".into(),
        }
    }
}

fn adopted(store: &Store, item: &str, observation: Observation) -> FinalReview {
    synchronize(store, item, Ok(observation), NOW + 10).unwrap();
    packet::adopt(store, item).unwrap();
    let mut finals = store.load_actions().unwrap().finals;
    assert_eq!(finals.len(), 1);
    finals.remove(0)
}

#[test]
fn an_approve_exit_creates_no_legacy_final_and_one_packet_record() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    assert!(
        store.load_actions().unwrap().finals.is_empty(),
        "no PrimaryFinal work for Packet-mode assignments"
    );
    packet::adopt(&store, &item).unwrap();
    packet::adopt(&store, &item).unwrap();
    let ledger = store.load_actions().unwrap();
    assert_eq!(ledger.finals.len(), 1);
    let run = &ledger.finals[0];
    let approval = run.packet.as_ref().unwrap();
    assert_eq!(approval.comment, "Reviewed the diff. Looks right.");
    assert!(!approval.merge);
    assert!(run.basis.permissions.approve && !run.basis.permissions.merge);
    assert_eq!(run.execution.operation.state, OperationState::Completed);
    assert!(run
        .execution
        .result
        .as_ref()
        .unwrap()
        .output
        .findings
        .is_empty());
    assert_eq!(
        run.execution.result.as_ref().unwrap().reviewed_base_sha,
        Some("b".repeat(40))
    );
}

#[test]
fn approve_sends_the_agents_comment_then_the_confirmed_write_consumes_the_state() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    adopted(&store, &item, observed());
    let effect = host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .unwrap();
    assert_eq!(effect.action, Action::Approve);
    assert!(effect.body.starts_with("Reviewed the diff. Looks right."));
    let mut env = env(&store);
    host::execute_action(&mut env, &effect).unwrap();
    let writes = env.wire.0.lock().unwrap().writes.clone();
    assert_eq!(writes.len(), 1);
    assert!(writes[0].1["body"]
        .as_str()
        .unwrap()
        .starts_with("Reviewed the diff. Looks right."));
    let after = env.wire.0.lock().unwrap().observation.clone();
    let job = store.load_reviews().unwrap().remove(0).job;
    // Not consumed until the write is confirmed and settled.
    let binding = Binding::job(&job);
    assert!(store
        .load_pr_states()
        .unwrap()
        .get(&binding)
        .unwrap()
        .exit
        .is_none());
    let settles = packet::settle(&store, &item, &after, NOW + 30).unwrap();
    assert_eq!(settles.len(), 1);
    assert_eq!(
        consume::settle(&store, &settles[0].job, "post-write-hash").unwrap(),
        Consumed::Consumed
    );
    packet::mark_settled(&store, &settles[0].final_id).unwrap();
    let record = store
        .load_pr_states()
        .unwrap()
        .get(&binding)
        .cloned()
        .unwrap();
    assert_eq!(record.baseline_hash.as_deref(), Some("post-write-hash"));
    assert!(record.pending.is_none());
    assert!(packet::settle(&store, &item, &after, NOW + 31)
        .unwrap()
        .is_empty());
    assert!(!packet::open(&store.load_actions().unwrap(), &item));
}

#[test]
fn approve_and_merge_approves_first_then_merges_and_settles_on_the_merged_pr() {
    let (_root, store, item) = setup(true, true, approve_exit(true), false);
    adopted(&store, &item, observed());
    let approve = host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .unwrap();
    assert_eq!(approve.action, Action::Approve);
    let mut env = env(&store);
    host::execute_action(&mut env, &approve).unwrap();
    let current = env.wire.0.lock().unwrap().observation.clone();
    assert!(
        packet::settle(&store, &item, &current, NOW + 21)
            .unwrap()
            .is_empty(),
        "merge still pending: nothing consumed yet"
    );
    synchronize(&store, &item, Ok(current.clone()), NOW + 22).unwrap();
    let merge = host::prepare_next_effect(&store, &item, NOW + 23)
        .unwrap()
        .unwrap();
    assert_eq!(merge.action, Action::Merge);
    env.now = NOW + 23;
    host::execute_action(&mut env, &merge).unwrap();
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 2);
    let merged = env.wire.0.lock().unwrap().observation.clone();
    assert_eq!(merged.state, "MERGED");
    assert!(packet::settle(&store, &item, &merged, NOW + 24)
        .unwrap()
        .is_empty());
    assert!(!packet::open(&store.load_actions().unwrap(), &item));
}

#[test]
fn an_existing_approval_of_ours_is_a_successful_no_op_that_still_consumes() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    let mut current = observed();
    current.reviews.push(ProviderReview {
        id: "101".into(),
        actor_id: "22".into(),
        head: "a".repeat(40),
        state: "APPROVED".into(),
        body: "Approved earlier.".into(),
        submitted_at: Some("2026-09-30T00:00:00Z".into()),
    });
    let run = adopted(&store, &item, current.clone());
    assert_eq!(
        packet::ready(&store, &run, &current, Action::Approve),
        Ok(Ready::AlreadyApproved)
    );
    assert!(host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .is_none());
    let settles = packet::settle(&store, &item, &current, NOW + 21).unwrap();
    assert_eq!(settles.len(), 1);
    assert!(store.load_actions().unwrap().effects.is_empty());
}

fn ready_for(
    exit_merge: bool,
    include_self: bool,
    change: impl Fn(&mut Observation),
) -> (Result<Ready, String>, Result<Ready, String>) {
    let (_root, store, item) = setup(true, true, approve_exit(exit_merge), include_self);
    let mut current = observed();
    current.reviews.push(super::fixwave::peer_approval());
    change(&mut current);
    let run = adopted(&store, &item, current.clone());
    (
        packet::ready(&store, &run, &current, Action::Approve),
        packet::ready(&store, &run, &current, Action::Merge),
    )
}

#[test]
fn gates_deny_what_the_exit_table_denies_and_allow_the_rest() {
    let go = Ok(Ready::Go);
    let (approve, merge) = ready_for(true, false, |_| {});
    assert_eq!((approve, merge), (go.clone(), go.clone()));
    // CI red and conflicts deny both.
    let (a, m) = ready_for(true, false, |o| o.checks = Some("FAILURE".into()));
    assert!(a.unwrap_err().contains("CI is red") && m.is_err());
    let (a, m) = ready_for(true, false, |o| {
        o.mergeable = "CONFLICTING".into();
        o.merge_state = "DIRTY".into();
    });
    assert!(a.unwrap_err().contains("conflicts") && m.is_err());
    // CI pending: approve ok, merge denied.
    let (a, m) = ready_for(true, false, |o| o.checks = Some("PENDING".into()));
    assert_eq!(a, go);
    assert!(m.is_err());
    // Required reviews unmet: approve ok, merge denied.
    let (a, m) = ready_for(true, false, |o| {
        o.review_decision = Some("REVIEW_REQUIRED".into());
        o.reviews.clear();
    });
    assert_eq!(a, go);
    assert!(m.is_err());
    // Others' threads: approve ok, merge denied.
    let (a, m) = ready_for(true, false, |o| {
        o.threads.push(crate::github::actions::HumanThread {
            id: "t".into(),
            resolved: false,
            outdated: false,
            resolved_by: None,
            comments: vec![],
        });
    });
    assert_eq!(a, go);
    assert!(m.is_err());
}

#[test]
fn a_self_authored_pr_denies_approve_and_merges_only_with_include_self() {
    let own = |o: &mut Observation| o.author_id = "22".into();
    let (a, m) = ready_for(false, false, own);
    assert!(a.unwrap_err().contains("self-approval"));
    assert!(m.is_err());
    // Merge exit on an own PR: no approval vote, merge only with Include self.
    let (_a, m) = ready_for(true, false, own);
    assert!(m.is_err());
    let (a, m) = ready_for(true, true, own);
    assert!(a.is_err(), "the vote is never attempted on an own PR");
    assert_eq!(m, Ok(Ready::Go));
}

#[test]
fn a_moved_state_is_never_adopted_or_executed_and_a_head_change_denies_writes() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    let job = store.load_reviews().unwrap().remove(0).job;
    let binding = Binding::job(&job);
    // The heartbeat saw the state change: the exit is stale.
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "i",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 2,
        },
    );
    store.save_pr_states(&states).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(packet::unadopted(&store, &store.load_actions().unwrap())
        .unwrap()
        .is_empty());
    // A final whose head moved after adoption is refused and abandoned without a consume.
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    let run = adopted(&store, &item, observed());
    let mut moved = observed();
    moved.head = "d".repeat(40);
    assert!(packet::ready(&store, &run, &moved, Action::Approve).is_err());
    assert!(packet::settle(&store, &item, &moved, NOW + 20)
        .unwrap()
        .is_empty());
    assert!(!packet::open(&store.load_actions().unwrap(), &item));
    let job = store.load_reviews().unwrap().remove(0).job;
    assert!(store
        .load_pr_states()
        .unwrap()
        .get(&Binding::job(&job))
        .unwrap()
        .exit
        .is_none());
}

#[test]
fn a_rejected_write_tosses_and_requeues_the_review_until_it_parks() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    adopted(&store, &item, observed());
    let effect = host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .unwrap();
    let mut env = env(&store);
    env.wire.0.lock().unwrap().reject = true;
    let _ = host::execute_action(&mut env, &effect);
    let ledger = store.load_actions().unwrap();
    assert_eq!(ledger.effects[0].state, EffectState::Rejected);
    let job = store.load_reviews().unwrap().remove(0).job;
    let binding = Binding::job(&job);
    let current = observed();
    assert!(packet::settle(&store, &item, &current, NOW + 30)
        .unwrap()
        .is_empty());
    let record = store
        .load_pr_states()
        .unwrap()
        .get(&binding)
        .cloned()
        .unwrap();
    assert_eq!(record.error_count, 1);
    assert!(record.exit.is_none(), "a failed write never consumes");
    let reviews = store.load_reviews().unwrap();
    assert_eq!(reviews.len(), 2, "a new operation was queued");
    assert_eq!(reviews[1].operation.state, OperationState::Queued);
    assert!(reviews[1].result.is_none());
    // The third toss parks: no further operation.
    let last = reviews[1].clone();
    assert!(consume::toss_and_requeue(&store, &job, &last, NOW + 40).unwrap());
    assert!(!consume::toss_and_requeue(&store, &job, &last, NOW + 41).unwrap());
    assert_eq!(store.load_reviews().unwrap().len(), 3);
}

#[test]
fn consume_after_a_moved_state_tosses_instead_of_baselining() {
    let (_root, store, _item) = setup(true, false, approve_exit(false), false);
    let mut job = store.load_reviews().unwrap().remove(0).job;
    let binding = Binding::job(&job);
    assert_eq!(
        consume::settle(&store, &job, "fresh").unwrap(),
        Consumed::Consumed
    );
    job.work.as_mut().unwrap().state_seq += 5;
    assert_eq!(
        consume::settle(&store, &job, "other").unwrap(),
        Consumed::Moved
    );
    let record = store
        .load_pr_states()
        .unwrap()
        .get(&binding)
        .cloned()
        .unwrap();
    assert_eq!(record.baseline_hash.as_deref(), Some("fresh"));
    assert_eq!(record.error_count, 1);
}
