use super::*;
use crate::{
    exits::{consume, ExitOutcome},
    packet::PacketRecord,
    pr_state::Consumed,
    retention::Binding,
    review::{ReviewOutput, RunMode},
};

pub(super) fn exit_run(store: &Store, exit: ExitOutcome, include_self: bool) {
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
            state_marks: None,
            wait_snapshot: None,
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

pub(super) fn approve_exit(merge: bool) -> ExitOutcome {
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

pub(super) fn adopted(store: &Store, item: &str, observation: Observation) -> FinalReview {
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
fn finding_clearance_survives_restart_and_denies_previously_accepted_native_actions() {
    for disposition in ["still_applies", "unverifiable"] {
        for action in [Action::Approve, Action::Merge] {
            let (root, store, item) = setup(true, true, approve_exit(true), false);
            let mut current = observed();
            current.threads.push(HumanThread {
                id: "corrected-finding".into(),
                resolved: true,
                outdated: false,
                resolved_by: Some("actor".into()),
                comments: vec![],
            });
            current.reviews.push(ProviderReview {
                id: "501".into(),
                actor_id: "33".into(),
                head: current.head.clone(),
                state: "APPROVED".into(),
                body: "Reviewed.".into(),
                submitted_at: Some("2026-09-30T00:02:00Z".into()),
            });
            let run = adopted(&store, &item, current.clone());
            let effect = prepare_effect(&store, &run.id, action, &current, NOW + 20).unwrap();
            // Data accepted by an older host must not authorize a new provider effect.
            let mut ledger = store.load_actions().unwrap();
            ledger.finals[0]
                .execution
                .result
                .as_mut()
                .unwrap()
                .finding_dispositions = vec![serde_json::from_value(json!({
                "thread_id":"corrected-finding","disposition":disposition,
                "head_sha":current.head,"base_sha":current.base,
                "rationale":"The resolved finding is not cleared.","evidence":[]
            }))
            .unwrap()];
            store.save_actions(&ledger).unwrap();
            let restarted = Store::new(root.path().into());
            restore(&restarted).unwrap();
            let mut env = env(&restarted);
            env.wire.0.lock().unwrap().observation = current.clone();
            let result = host::execute_action(&mut env, &effect);
            assert!(
                env.wire.0.lock().unwrap().writes.is_empty(),
                "{disposition} must deny {action:?} before transmission"
            );
            assert!(result.is_err());
            let saved = restarted.load_actions().unwrap().finals.remove(0);
            assert!(packet::ready(&restarted, &saved, &current, action).is_err());
            assert!(prepare_effect(&restarted, &saved.id, action, &current, NOW + 30).is_err());
        }
    }
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

fn item(store: &Store, id: &str) -> crate::queue::Item {
    crate::queue::snapshot(store, vec![])
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.id == id)
        .unwrap()
}

#[test]
fn approve_exits_show_approving_then_merging_then_leave_the_queue_as_merged() {
    use crate::{exits::Exit, queue::Progress};
    let (_root, store, id) = setup(true, true, approve_exit(true), false);
    let before = item(&store, &id);
    assert_eq!(before.state, State::MachineSignedOff);
    assert_eq!(before.exit, Some(Exit::ApproveAndMerge));
    assert_eq!(before.progress, Some(Progress::Approving));
    adopted(&store, &id, observed());
    assert_eq!(item(&store, &id).progress, Some(Progress::Approving));
    let approve = host::prepare_next_effect(&store, &id, NOW + 20)
        .unwrap()
        .unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &approve).unwrap();
    let current = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &id, Ok(current), NOW + 22).unwrap();
    let approving = item(&store, &id);
    // A confirmed approval leaves the Queue; the merge still shows as progress.
    assert_eq!(approving.state, State::Approved);
    assert_eq!(approving.progress, Some(Progress::Merging));
    let merge = host::prepare_next_effect(&store, &id, NOW + 23)
        .unwrap()
        .unwrap();
    env.now = NOW + 23;
    host::execute_action(&mut env, &merge).unwrap();
    let merged = item(&store, &id);
    assert_eq!(merged.state, State::Merged);
    assert_eq!(merged.progress, None);
}

#[test]
fn a_confirmed_packet_approval_leaves_the_queue_as_approved() {
    let (_root, store, id) = setup(true, false, approve_exit(false), false);
    adopted(&store, &id, observed());
    let effect = host::prepare_next_effect(&store, &id, NOW + 20)
        .unwrap()
        .unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &effect).unwrap();
    let current = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &id, Ok(current), NOW + 22).unwrap();
    let approved = item(&store, &id);
    assert_eq!(approved.state, State::Approved);
    assert_eq!(approved.progress, None);
}

#[test]
fn a_parked_approve_toss_is_one_failed_row_typed_tossed_and_retry_requeues() {
    let (_root, store, id) = setup(true, false, approve_exit(false), false);
    adopted(&store, &id, observed());
    let effect = host::prepare_next_effect(&store, &id, NOW + 20)
        .unwrap()
        .unwrap();
    let mut env = env(&store);
    env.wire.0.lock().unwrap().reject = true;
    let _ = host::execute_action(&mut env, &effect);
    let job = store.load_reviews().unwrap().remove(0).job;
    let binding = Binding::job(&job);
    // Two earlier tosses: this one spends the budget and parks.
    let mut states = store.load_pr_states().unwrap();
    states.toss(&binding);
    states.toss(&binding);
    store.save_pr_states(&states).unwrap();
    packet::settle(&store, &id, &observed(), NOW + 30).unwrap();
    assert_eq!(store.load_reviews().unwrap().len(), 1, "parked: no new run");
    let snapshot = crate::queue::snapshot(&store, vec![]).unwrap();
    let rows: Vec<_> = snapshot.items.iter().filter(|i| i.id == id).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, State::Failed);
    assert_eq!(rows[0].failure_cause, Some(crate::queue::CAUSE_TOSSED));
    assert_eq!(rows[0].progress, None);

    assert_eq!(consume::retry_parked(&store, &id, NOW + 40).unwrap(), 1);
    let record = store
        .load_pr_states()
        .unwrap()
        .get(&binding)
        .cloned()
        .unwrap();
    assert_eq!(
        record.error_count, 0,
        "an explicit retry restarts the budget"
    );
    let reviews = store.load_reviews().unwrap();
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[1].operation.state, OperationState::Queued);
    assert_eq!(item(&store, &id).state, State::Queued);
    assert!(consume::retry_parked(&store, &id, NOW + 41).is_err());
}

#[test]
fn a_second_approve_exit_on_an_approved_head_settles_without_another_vote() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    let mut current = observed();
    current.checks = Some("PENDING".into());
    adopted(&store, &item, current);
    let effect = host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .unwrap();
    let mut env = env(&store);
    env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
    host::execute_action(&mut env, &effect).unwrap();
    let approved = env.wire.0.lock().unwrap().observation.clone();
    let settles = packet::settle(&store, &item, &approved, NOW + 21).unwrap();
    assert_eq!(settles.len(), 1);
    assert_eq!(
        consume::settle(&store, &settles[0].job, "after-first").unwrap(),
        Consumed::Consumed
    );
    packet::mark_settled(&store, &settles[0].final_id).unwrap();
    // CI turns green: a new state on the same head; the agent approves again.
    let mut green = approved.clone();
    green.checks = Some("SUCCESS".into());
    let mut reviews = store.load_reviews().unwrap();
    let mut second = reviews[0].clone();
    second.operation.id = "second-approve".into();
    second.job.work.as_mut().unwrap().state_seq = 2;
    reviews.push(second.clone());
    store.save_reviews(&reviews).unwrap();
    let binding = Binding::job(&second.job);
    let mut states = store.load_pr_states().unwrap();
    let record = states
        .records
        .iter_mut()
        .find(|r| r.binding == binding)
        .unwrap();
    record.state_seq = 2;
    record.pending = Some(crate::pr_state::Pending {
        seq: 2,
        hash: Some("green".into()),
        observed_at: NOW + 22,
    });
    store.save_pr_states(&states).unwrap();
    synchronize(&store, &item, Ok(green.clone()), NOW + 23).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert_eq!(store.load_actions().unwrap().finals.len(), 2);
    assert!(
        host::prepare_next_effect(&store, &item, NOW + 24)
            .unwrap()
            .is_none(),
        "no second vote"
    );
    let settles = packet::settle(&store, &item, &green, NOW + 25).unwrap();
    assert_eq!(settles.len(), 1, "our standing approval settles the exit");
    assert_eq!(settles[0].final_id, "final-packet-second-approve");
    assert_eq!(
        consume::settle(&store, &settles[0].job, "after-second").unwrap(),
        Consumed::Consumed
    );
    packet::mark_settled(&store, &settles[0].final_id).unwrap();
    assert!(!packet::open(&store.load_actions().unwrap(), &item));
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1);
}

#[test]
fn an_unadoptable_approve_exit_stops_being_observed() {
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    let unadopted =
        |store: &Store| packet::unadopted(store, &store.load_actions().unwrap()).unwrap();
    assert!(unadopted(&store).contains(&item));
    let mut closed = observed();
    closed.state = "CLOSED".into();
    synchronize(&store, &item, Ok(closed), NOW + 10).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(
        unadopted(&store).is_empty(),
        "a closed PR is not observed again"
    );

    // A deactivated scope cannot be adopted either.
    let (_root, store, item) = setup(true, false, approve_exit(false), false);
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let mut monitor = store.load_monitoring_state().unwrap();
    monitor.activations.clear();
    store.save_monitoring_state(&monitor).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(unadopted(&store).is_empty(), "nothing can adopt it");
}

#[test]
fn a_settled_thread_changed_after_the_merge_intent_is_saved_blocks_the_merge() {
    use crate::review::findings::body_hash;
    let reply = "Won't fix: intended product behavior.";
    let comment = |id: u64, author: u64, body: &str| json!({"fullDatabaseId":id,"body":body,"author":{"login":"x","databaseId":author}});
    for changed in [false, true] {
        let (_root, store, item) = setup(true, true, approve_exit(true), false);
        let mut current = observed();
        current.threads.push(HumanThread {
            id: "T1".into(),
            resolved: true,
            outdated: false,
            resolved_by: Some("actor".into()),
            comments: vec![comment(100, 22, "Root"), comment(101, 11, reply)],
        });
        current.reviews.push(ProviderReview {
            id: "501".into(),
            actor_id: "33".into(),
            head: current.head.clone(),
            state: "APPROVED".into(),
            body: "Reviewed.".into(),
            submitted_at: Some("2026-09-30T00:02:00Z".into()),
        });
        let run = adopted(&store, &item, current.clone());
        let mut ledger = store.load_actions().unwrap();
        ledger.finals[0]
            .execution
            .result
            .as_mut()
            .unwrap()
            .finding_dispositions = vec![serde_json::from_value(json!({
            "thread_id":"T1","disposition":"author_wont_fix",
            "head_sha":current.head,"base_sha":current.base,
            "rationale":"Accepted.","evidence":[],"author_comment_id":"101",
            "anchor":{"base_sha":current.base,"blobs":{},"manifests":"m",
                "comments":{"100":body_hash("Root"),"101":body_hash(reply)},"author_id":"11"}
        }))
        .unwrap()];
        store.save_actions(&ledger).unwrap();
        let effect = prepare_effect(&store, &run.id, Action::Merge, &current, NOW + 20).unwrap();
        let mut env = env(&store);
        env.wire.0.lock().unwrap().observation = current.clone();
        // Lands after the intent is persisted, before the final fresh read.
        env.comment_after_intent = changed.then(|| comment(102, 33, "This still loses data."));
        let _ = host::execute_action(&mut env, &effect);
        assert_eq!(
            env.wire.0.lock().unwrap().writes.len(),
            usize::from(!changed),
            "changed={changed}"
        );
        assert_eq!(
            env.wire.0.lock().unwrap().observation.state == "MERGED",
            !changed
        );
    }
}
