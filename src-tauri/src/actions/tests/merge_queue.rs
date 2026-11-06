use super::*;

fn queued() -> Observation {
    let mut o = observed();
    o.merge_queue = true;
    o
}

/// Approve, then enqueue, an approve-and-merge final on a merge-queue base.
fn enqueue<'a>(store: &'a Store, item: &str) -> Env<'a> {
    approve_exits::exit_run(store, approve_exits::approve_exit(true), false);
    approve_exits::adopted(store, item, queued());
    let mut env = env(store);
    env.wire.0.lock().unwrap().observation = queued();
    let approve = host::prepare_next_effect(store, item, NOW + 20)
        .unwrap()
        .unwrap();
    host::execute_action(&mut env, &approve).unwrap();
    sync(store, item, &env, NOW + 22);
    let merge = host::prepare_next_effect(store, item, NOW + 23)
        .unwrap()
        .unwrap();
    assert_eq!(merge.action, Action::Merge);
    env.now = NOW + 23;
    host::execute_action(&mut env, &merge).unwrap();
    env
}

fn sync(store: &Store, item: &str, env: &Env<'_>, now: i64) {
    let current = env.wire.0.lock().unwrap().observation.clone();
    synchronize(store, item, Ok(current), now).unwrap();
}

fn merges(store: &Store) -> Vec<Effect> {
    store
        .load_actions()
        .unwrap()
        .effects
        .into_iter()
        .filter(|e| e.action == Action::Merge)
        .collect()
}

fn dequeue(store: &Store, item: &str, env: &Env<'_>, now: i64) {
    env.wire.0.lock().unwrap().observation.in_merge_queue = false;
    sync(store, item, env, now);
}

#[test]
fn a_merge_queue_base_enqueues_the_pinned_head_and_confirms_only_the_queue_merge() {
    let (_root, store, item) = fixture(1, true, true);
    let env = enqueue(&store, &item);
    let writes = env.wire.0.lock().unwrap().writes.clone();
    assert_eq!(writes.len(), 2);
    assert!(writes[1].1["query"]
        .as_str()
        .unwrap()
        .contains("enqueuePullRequest"));
    let merge = merges(&store).remove(0);
    assert_eq!(merge.state, EffectState::Queued);
    let receipt = merge.receipt.clone().unwrap();
    assert_eq!(
        (receipt.id.as_str(), receipt.merge_commit),
        ("MQE_entry", None)
    );
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_ne!(snapshot.items[0].state, State::Merged);
    assert_ne!(snapshot.items[0].state, State::Failed);
    // Still queued: nothing settles, nothing is resent.
    sync(&store, &item, &env, NOW + 30);
    assert_eq!(merges(&store)[0].state, EffectState::Queued);
    assert!(host::prepare_next_effect(&store, &item, NOW + 31)
        .unwrap()
        .is_none());
    let current = env.wire.0.lock().unwrap().observation.clone();
    assert!(packet::settle(&store, &item, &current, NOW + 31)
        .unwrap()
        .is_empty());
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.in_merge_queue = false;
        server.observation.state = "MERGED".into();
        server.observation.merge_commit = Some("e".repeat(40));
    }
    sync(&store, &item, &env, NOW + 40);
    let merge = merges(&store).remove(0);
    assert_eq!(merge.state, EffectState::Confirmed);
    assert_eq!(merge.receipt.unwrap().merge_commit, Some("e".repeat(40)));
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::Merged
    );
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 2);
}

#[test]
fn a_dequeue_is_typed_reconciled_by_a_later_read_then_re_enqueued_up_to_the_limit() {
    let (_root, store, item) = fixture(1, true, true);
    let mut env = enqueue(&store, &item);
    for round in 1..=MAX_ENQUEUES {
        let at = NOW + 100 * round as i64;
        dequeue(&store, &item, &env, at);
        let last = merges(&store).pop().unwrap();
        assert_eq!(last.state, EffectState::Dequeued);
        assert_eq!(
            last.dequeue,
            Some(Dequeue {
                cause: DequeueCause::Removed,
                at,
                reconciled: false
            })
        );
        assert!(last.error.is_none());
        // Non-parking: never Needs attention, never a toss.
        let snapshot = queue::snapshot(&store, vec![]).unwrap();
        assert_ne!(snapshot.items[0].state, State::Failed);
        let current = env.wire.0.lock().unwrap().observation.clone();
        assert!(packet::settle(&store, &item, &current, at)
            .unwrap()
            .is_empty());
        assert_eq!(store.load_reviews().unwrap().len(), 1, "no review requeued");
        // The dequeuing read never authorizes the retry.
        assert!(host::prepare_next_effect(&store, &item, at + 1)
            .unwrap()
            .is_none());
        sync(&store, &item, &env, at + 2);
        assert!(merges(&store).pop().unwrap().dequeue.unwrap().reconciled);
        let retry = host::prepare_next_effect(&store, &item, at + 3).unwrap();
        if round == MAX_ENQUEUES {
            assert!(retry.is_none(), "the queue stops being fed");
            let status = queue::snapshot(&store, vec![]).unwrap().items[0]
                .action_status
                .clone()
                .unwrap();
            assert!(status
                .blockers
                .iter()
                .any(|b| b.contains("no further enqueue")));
            break;
        }
        let retry = retry.unwrap();
        assert_eq!(retry.action, Action::Merge);
        env.now = at + 3;
        host::execute_action(&mut env, &retry).unwrap();
        assert_eq!(merges(&store).pop().unwrap().state, EffectState::Queued);
    }
    assert_eq!(merges(&store).len(), MAX_ENQUEUES);
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1 + MAX_ENQUEUES);
}

#[test]
fn a_merge_seen_after_the_dequeuing_read_confirms_without_another_enqueue() {
    let (_root, store, item) = fixture(1, true, true);
    let env = enqueue(&store, &item);
    dequeue(&store, &item, &env, NOW + 50);
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.state = "MERGED".into();
        server.observation.merge_commit = Some("e".repeat(40));
    }
    sync(&store, &item, &env, NOW + 60);
    let merge = merges(&store).remove(0);
    assert_eq!(merge.state, EffectState::Confirmed);
    assert!(merge.dequeue.is_none());
    assert!(host::prepare_next_effect(&store, &item, NOW + 61)
        .unwrap()
        .is_none());
}

#[test]
fn a_new_head_or_close_while_queued_is_a_dequeue_that_never_retries() {
    for cause in [DequeueCause::HeadChanged, DequeueCause::Closed] {
        let (_root, store, item) = fixture(1, true, true);
        let env = enqueue(&store, &item);
        {
            let mut server = env.wire.0.lock().unwrap();
            server.observation.in_merge_queue = false;
            match cause {
                DequeueCause::HeadChanged => server.observation.head = "d".repeat(40),
                _ => server.observation.state = "CLOSED".into(),
            }
        }
        sync(&store, &item, &env, NOW + 50);
        assert_eq!(merges(&store)[0].dequeue.as_ref().unwrap().cause, cause);
        sync(&store, &item, &env, NOW + 60);
        assert!(!action_taken(
            &store.load_actions().unwrap().effects,
            "other",
            Action::Merge
        ));
        assert!(action_taken(
            &store.load_actions().unwrap().effects,
            &item,
            Action::Merge
        ));
    }
}

#[test]
fn a_lost_enqueue_response_reconciles_to_queued_without_a_receipt_or_resend() {
    let (_root, store, item) = fixture(1, true, true);
    approve_exits::exit_run(&store, approve_exits::approve_exit(true), false);
    let mut current = queued();
    current.reviews.push(fixwave::peer_approval());
    let run = approve_exits::adopted(&store, &item, current.clone());
    let mut env = env(&store);
    env.wire.0.lock().unwrap().observation = current.clone();
    env.wire.0.lock().unwrap().lost = true;
    let effect = prepare_effect(&store, &run.id, Action::Merge, &current, NOW + 20).unwrap();
    host::execute_action(&mut env, &effect).unwrap();
    assert_eq!(merges(&store)[0].state, EffectState::Uncertain);
    sync(&store, &item, &env, NOW + 30);
    let merge = merges(&store).remove(0);
    assert_eq!(merge.state, EffectState::Queued);
    assert!(merge.receipt.is_none());
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.in_merge_queue = false;
        server.observation.state = "MERGED".into();
    }
    sync(&store, &item, &env, NOW + 40);
    assert_eq!(merges(&store)[0].state, EffectState::ExternalMerge);
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1);
}

#[test]
fn merge_queue_readiness_skips_method_and_behind_but_keeps_every_other_gate() {
    let mut o = queued();
    o.method = None;
    o.merge_state = "BEHIND".into();
    o.write_capability = true;
    assert!(o.blocker(Action::Merge).is_none());
    assert_eq!(
        crate::exits::gates::Facts::from_observation(&o).merge_pending,
        None
    );
    for change in [
        |o: &mut Observation| o.in_merge_queue = true,
        |o: &mut Observation| o.checks = Some("PENDING".into()),
        |o: &mut Observation| o.mergeable = "CONFLICTING".into(),
        |o: &mut Observation| o.merge_state = "HAS_HOOKS".into(),
        |o: &mut Observation| o.merge_rules = None,
    ] {
        let mut blocked = o.clone();
        change(&mut blocked);
        assert!(blocked.blocker(Action::Merge).is_some());
    }
    // Without a queue: direct merge still needs a method and an up-to-date branch.
    let mut direct = observed();
    direct.write_capability = true;
    direct.merge_state = "BEHIND".into();
    assert_eq!(
        direct.blocker(Action::Merge).as_deref(),
        Some(crate::provider::BEHIND_BASE)
    );
    direct.merge_state = "CLEAN".into();
    direct.method = None;
    assert!(direct.blocker(Action::Merge).is_some());
}
