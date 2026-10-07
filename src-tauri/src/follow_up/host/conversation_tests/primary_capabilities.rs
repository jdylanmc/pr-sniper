use super::*;
use crate::storage::ActionPermissions;

fn binding() -> MentionBinding {
    MentionBinding {
        configuration_id: REPO.into(),
        account_id: "22".into(),
        account_login: "actor".into(),
        repository_id: "100".into(),
        repository_name: "example/repo".into(),
        pull_request_id: "9".into(),
        number: 1,
    }
}

#[test]
fn primary_general_reply_ignores_cadence_changes_without_rewriting_context_or_provider_receipts() {
    use crate::{policy::Schedule, storage::ResourceEdit};
    let (root, store, origin, thread) = fixture(1);
    let original = store.load_settings().unwrap();
    let mut repository = original.repositories[0].clone();
    repository.assignments[0].comment = false;
    repository.assignments[0].actions = Some(ActionPermissions {
        reply: true,
        approve: false,
        merge: false,
    });
    repository.overrides.automatic_comment_publication = Some(true);
    repository.overrides.schedule = Some(Schedule::Cron {
        expression: "0 9 1 * *".into(),
        timezone: "America/New_York".into(),
    });
    let saved = store
        .save_resource(ResourceEdit::Repository {
            id: REPO.into(),
            expected: Some(Box::new(original.repositories[0].clone())),
            value: Some(Box::new(repository)),
        })
        .unwrap();
    let history = std::fs::read(root.path().join("state/publications.json")).unwrap();
    let reviews = store.load_reviews().unwrap();
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![mention("701", "How does the return value work?")],
        NOW + 10,
    );
    let capacity = Capacity::default();
    let Dispatch::Reply(mut run, token) = capacity
        .dispatch(&store, NOW + 20)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Current primary general reply expected without another cron poll")
    };
    let captured = run.context.clone();
    let mut retimed = saved.repositories[0].clone();
    retimed.overrides.schedule = Some(Schedule::Cron {
        expression: "0 * * * *".into(),
        timezone: "UTC".into(),
    });
    retimed.overrides.automatic_comment_publication = Some(false);
    let current = store
        .save_resource(ResourceEdit::Repository {
            id: REPO.into(),
            expected: Some(Box::new(saved.repositories[0].clone())),
            value: Some(Box::new(retimed)),
        })
        .unwrap();
    assert_eq!(
        current.repositories[0].assignments,
        saved.repositories[0].assignments
    );
    assert!(capacity
        .dispatch(&store, NOW + 21)
        .unwrap()
        .dispatched
        .is_empty());
    assert!(!token.load(std::sync::atomic::Ordering::SeqCst));
    assert!(run
        .authority(&current, &run.context.job)
        .is_ok_and(|grant| grant));
    let result = output_for(&run, ReplyDecision::Reply, vec![]);
    complete_analysis(&store, &mut run, Ok(result), true, NOW + 22).unwrap();
    let candidate = candidates(&store).unwrap().remove(0);
    assert_eq!(candidate.run.context, captured);
    assert!(candidate.automatic_publication);
    assert!(candidate.blocked.is_none());
    assert_eq!(store.load_reviews().unwrap(), reviews);
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        history
    );
    assert!(store.load_actions().unwrap().effects.is_empty());
}

#[test]
fn publish_and_reply_matrix_uses_real_store_dispatch_and_validated_task_output() {
    for publish in [false, true] {
        for reply in [false, true] {
            let (_root, store, origin, thread) = fixture(1);
            let mut settings = store.load_settings().unwrap();
            settings.repositories[0].assignments[0].comment = publish;
            settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
                reply,
                approve: false,
                merge: false,
            });
            // The retired global default is deliberately contradictory.
            settings.defaults.automatic_comment_publication = !publish;
            store.save_settings(&settings).unwrap();
            let initial = publication::host::candidates(&store).unwrap().remove(0);
            assert_eq!(initial.automatic, publish);
            assert!(
                !initial.local_only,
                "A historical published receipt remains published even after revocation."
            );
            assert_eq!(
                settings.repositories[0]
                    .assignment_authority(&settings.repositories[0].assignments[0])
                    .comment,
                publish
            );
            observe(
                &store,
                &origin,
                &thread,
                'a',
                vec![mention("701", "How does the return value work?")],
                NOW + 10,
            );
            let before = store.load_reviews().unwrap();
            let capacity = Capacity::default();
            let Dispatch::Reply(mut run, _) = capacity
                .dispatch(&store, NOW + 20)
                .unwrap()
                .dispatched
                .remove(0)
            else {
                panic!("Primary analysis expected");
            };
            assert!(!run.manual_start);
            assert!(run
                .authority(&settings, &run.context.job)
                .is_ok_and(|grant| grant == reply));
            let result = output_for(&run, ReplyDecision::Reply, vec![]);
            complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
            let candidate = candidates(&store).unwrap().remove(0);
            assert_eq!(candidate.automatic_publication, reply);
            assert!(candidate.blocked.is_none());
            assert_eq!(candidate.run.phase, Phase::WaitingPublication);
            assert_eq!(store.load_reviews().unwrap(), before);
            assert!(candidate.run.publication.is_none());
            let body = candidate.run.reply_body().unwrap();
            assert!(body.contains("Agent Agent 1 / model model"));
            assert!(body.contains("issuecomment-701"));
            assert!(store.load_actions().unwrap().effects.is_empty());
        }
    }
}

#[test]
fn primary_assesses_secondary_and_unowned_threads_without_rewriting_original_ownership() {
    let (root, store, origin, mut thread) = fixture(2);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].actions = None;
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[1].id.clone());
    store.save_settings(&settings).unwrap();
    let history = std::fs::read(root.path().join("state/publications.json")).unwrap();
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let run = store.load_follow_ups().unwrap().remove(0);
    assert_eq!(
        run.context.assignment_id,
        settings.repositories[0].assignments[1].id
    );
    assert_eq!(run.context.selection.agent.id, settings.agents[1].id);
    let original = &store.load_feedback().unwrap().records[0].context;
    assert_eq!(original.owner_agent_id, origin.review.selection.agent.id);
    assert_eq!(original.owner_assignment_id, origin.review.assignment_id);
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        history
    );
    let result = output_for(&run, ReplyDecision::Quiet, vec![]);
    assert_eq!(result.output.decision, ReplyDecision::Quiet);
    let original_context = original.clone();
    let Dispatch::Reply(mut analyzed, _) = Capacity::default()
        .dispatch(&store, NOW + 15)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Primary reply analysis expected");
    };
    let reassessment = output_for(
        &analyzed,
        ReplyDecision::Quiet,
        vec![assessment(&original_context.id, Disposition::Cleared)],
    );
    complete_analysis(&store, &mut analyzed, Ok(reassessment), true, NOW + 16).unwrap();
    assert_eq!(
        crate::feedback::views(&store, &analyzed.context.job).unwrap()[0].state,
        "cleared"
    );
    assert_eq!(
        store.load_feedback().unwrap().records[0]
            .context
            .owner_agent_id,
        original_context.owner_agent_id
    );
    assert!(
        !store.load_feedback().unwrap().records[0].context.closed,
        "Local assessment is not fabricated human closure."
    );
    let mut unowned = thread.clone();
    unowned.id = "external-thread".into();
    unowned.comments[0].id = "801".into();
    unowned.comments[0].author_id = Some("44".into());
    unowned.comments[0].body = "Another user's review question".into();
    unowned.comments.truncate(1);
    let ticket = poll(&store, 'a', NOW + 20);
    admit_scan(
        &store,
        &ticket,
        Scan {
            threads: vec![(binding(), vec![unowned])],
            ..Scan::default()
        },
        NOW + 22,
    )
    .unwrap();
    assert_eq!(store.load_follow_ups().unwrap().len(), 2);
    assert!(store
        .load_follow_ups()
        .unwrap()
        .iter()
        .all(|run| run.context.assignment_id == settings.repositories[0].assignments[1].id));
}

#[test]
fn activation_history_pending_work_and_role_removal_remain_durable_without_fanout() {
    let (root, store) = tracking_fixture(2);
    let ticket = poll(&store, 'a', NOW + 10);
    let mut ledger = store.load_feedback().unwrap();
    ledger.conversation_cursors.clear();
    store.save_feedback(&ledger).unwrap();
    admit_scan(
        &store,
        &ticket,
        mention_scan(vec![mention("700", "Historical context")]),
        NOW + 12,
    )
    .unwrap();
    assert!(store.load_follow_ups().unwrap().is_empty());
    admit_scan(
        &store,
        &ticket,
        mention_scan(vec![
            mention("700", "Historical context"),
            mention("701", "New question"),
        ]),
        NOW + 13,
    )
    .unwrap();
    assert_eq!(store.load_feedback().unwrap().mentions.len(), 1);
    assert!(store.load_follow_ups().unwrap().is_empty());
    drop(store);
    let store = Store::new(root.path().into());
    let mut settings = store.load_settings().unwrap();
    let primary = settings.repositories[0].assignments[1].id.clone();
    settings.repositories[0].primary_assignment_id = Some(primary.clone());
    store.save_settings(&settings).unwrap();
    let ticket = poll(&store, 'a', NOW + 20);
    admit_scan(&store, &ticket, Scan::default(), NOW + 22).unwrap();
    let run = store.load_follow_ups().unwrap().remove(0);
    assert_eq!(run.trigger_id, "701");
    assert_eq!(run.context.assignment_id, primary);
    assert!(!candidates(&store).unwrap()[0].automatic_publication);
    settings.repositories[0].primary_assignment_id = None;
    store.save_settings(&settings).unwrap();
    assert!(candidates(&store).unwrap()[0]
        .blocked
        .as_ref()
        .unwrap()
        .contains("primary"));
    assert_eq!(store.load_follow_ups().unwrap(), vec![run]);
}

#[test]
fn self_duplicate_and_human_closed_threads_do_not_replay_or_resurrect() {
    let (_root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "22");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    assert!(store.load_follow_ups().unwrap().is_empty());
    thread.resolved = true;
    observe(&store, &origin, &thread, 'a', vec![], NOW + 20);
    thread.resolved = false;
    explanation(&mut thread, "11");
    thread.comments.last_mut().unwrap().id = "102".into();
    observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
    assert!(store.load_follow_ups().unwrap().is_empty());
    assert!(store.load_feedback().unwrap().records[0].context.closed);
    let self_output = TopComment {
        author_id: Some("22".into()),
        ..mention("801", "Self comment")
    };
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![
            self_output,
            mention("802", "Machine output <!-- pr-sniper:reply:loop -->"),
        ],
        NOW + 40,
    );
    assert!(store.load_follow_ups().unwrap().is_empty());
}

#[test]
fn obsolete_secondary_grants_are_disabled_without_promotion_or_history_and_receipt_reset() {
    let (root, store, _, _) = fixture(2);
    let before_reviews = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    let before_receipts = std::fs::read(root.path().join("state/publications.json")).unwrap();
    let mut legacy = store.load_settings().unwrap();
    legacy.repositories[0].assignments[1].actions = Some(ActionPermissions {
        reply: true,
        approve: true,
        merge: true,
    });
    assert!(store
        .save_settings(&legacy)
        .unwrap_err()
        .contains("Secondary"));
    // Simulate an already-saved pre-amendment configuration in this isolated fixture.
    std::fs::write(
        root.path().join("config/settings.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let mut current = store.load_settings().unwrap();
    assert!(current
        .capability_notice
        .as_ref()
        .unwrap()
        .contains("disabled"));
    assert_eq!(
        current.repositories[0].assignments[1].actions,
        Some(ActionPermissions::default())
    );
    current.repositories[0].primary_assignment_id =
        Some(current.repositories[0].assignments[1].id.clone());
    current.repositories[0].assignments[0].actions = None;
    store.save_settings(&current).unwrap();
    let primary =
        current.repositories[0].assignment_authority(&current.repositories[0].assignments[1]);
    assert!(primary.primary);
    assert!(!primary.reply && !primary.approve && !primary.merge);
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        before_reviews
    );
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        before_receipts
    );
}

#[test]
fn linked_thread_execution_loss_never_replays_a_provider_intent() {
    let (_root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let intent = store.load_feedback().unwrap().pending_threads.remove(0);
    assert!(intent.follow_up_id.is_some());
    store.save_follow_ups(&[]).unwrap();
    let ticket = poll(&store, 'a', NOW + 20);
    admit_scan(&store, &ticket, Scan::default(), NOW + 22).unwrap();
    assert!(store.load_follow_ups().unwrap().is_empty());
    let retained = store.load_feedback().unwrap().pending_threads.remove(0);
    assert_eq!(retained.work_id, intent.work_id);
    assert_eq!(retained.follow_up_id, intent.follow_up_id);
    assert!(retained.blocked.unwrap().contains("history is unavailable"));
    assert_eq!(store.load_publications().unwrap(), vec![origin]);
}

#[test]
fn provider_reply_denial_does_not_disable_read_only_primary_analysis() {
    let (_root, store, origin, mut thread) = fixture(1);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].comment = false;
    settings.repositories[0].assignments[0]
        .actions
        .as_mut()
        .unwrap()
        .reply = false;
    store.save_settings(&settings).unwrap();
    thread.can_reply = false;
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let Dispatch::Reply(mut run, _) = Capacity::default()
        .dispatch(&store, NOW + 20)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Read-only primary analysis expected");
    };
    assert!(run
        .validate_current(&settings, &run.context.job, &pull('a'), true, false)
        .is_ok());
    let result = output_for(&run, ReplyDecision::Reply, vec![]);
    complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
    assert!(run.result.is_some());
    assert!(!candidates(&store).unwrap()[0].automatic_publication);
    run.publication = Some(run.operation("thread_reply", NOW + 22));
    assert!(!run.fresh_thread(&thread));
    assert!(run
        .validate_current(&settings, &run.context.job, &pull('a'), true, false)
        .is_err());
}

#[test]
fn mention_admission_overrides_discovery_only_and_general_reply_grants_do_not_admit_prs() {
    let (_root, store) = tracking_fixture(1);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors = vec![crate::policy::WatchedIdentity {
        id: "44".into(),
        login: "watched".into(),
    }];
    settings.repositories[0].overrides.reviewer_assignment = Some(false);
    settings.repositories[0].assignments[0].comment = false;
    settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
        reply: false,
        approve: false,
        merge: false,
    });
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.get_mut(REPO).unwrap().trigger_policy = context.trigger_policy;
    store.save_monitoring_state(&state).unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut candidate = pull('c');
    candidate.id = "10".into();
    candidate.number = 2;
    let comment = mention("701", "@actor can you inspect this?");
    for mentioned in [false, true] {
        settings.repositories[0].assignments[0]
            .actions
            .as_mut()
            .unwrap()
            .reply = !mentioned;
        store.save_settings(&settings).unwrap();
        candidate.mentioned =
            mentioned && comment.eligible_other_user("22") && comment.mentions("actor", "22");
        let ticket = monitor
            .prepare_checks(&store, NOW + 10, true)
            .unwrap()
            .remove(0);
        monitor
            .finish(
                &store,
                ticket,
                Ok(PollResult {
                    connection: Connection {
                        identity: Identity {
                            id: "22".into(),
                            login: "actor".into(),
                        },
                        repository: RemoteRepository {
                            id: "100".into(),
                            name: "example/repo".into(),
                        },
                        capabilities: Capabilities {
                            read: true,
                            comment: CommentCapability::Available,
                        },
                    },
                    pull_requests: vec![pull('a'), candidate.clone()],
                }),
                NOW + 11,
            )
            .unwrap();
        assert_eq!(
            store
                .load_queue_state()
                .unwrap()
                .tracked
                .iter()
                .any(|tracked| tracked.pull_request_id == "10"),
            mentioned
        );
        assert!(store.load_actions().unwrap().effects.is_empty());
    }
    let authority =
        settings.repositories[0].assignment_authority(&settings.repositories[0].assignments[0]);
    assert!(!authority.comment && !authority.reply && !authority.approve && !authority.merge);
}
