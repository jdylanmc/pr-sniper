use super::*;
use crate::storage::ActionPermissions;

fn clear_local_review(store: &Store) {
    let mut reviews = store.load_reviews().unwrap();
    for review in &mut reviews {
        let output = &mut review.result.as_mut().unwrap().output;
        output.findings.clear();
        output.decision = crate::review::Decision::MachineSignOff;
    }
    store.save_reviews(&reviews).unwrap();
    store.save_publications(&[]).unwrap();
}

#[test]
fn fixwave_retired_publication_flags_do_not_control_readiness_or_final_basis() {
    for publish in [false, true] {
        for reply in [false, true] {
            let (_root, store, _, _) = fixture(1);
            clear_local_review(&store);
            let mut settings = store.load_settings().unwrap();
            settings.repositories[0].assignments[0].comment = publish;
            settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
                reply,
                approve: true,
                merge: false,
            });
            for global in [false, true] {
                for repository in [None, Some(false), Some(true)] {
                    settings.defaults.automatic_comment_publication = global;
                    settings.repositories[0]
                        .overrides
                        .automatic_comment_publication = repository;
                    store.save_settings(&settings).unwrap();
                    let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
                    let expected = if publish {
                        crate::queue::State::AwaitingPublication
                    } else {
                        crate::queue::State::MachineSignedOff
                    };
                    assert_eq!(
                        snapshot.items[0].state, expected,
                        "publish={publish}, reply={reply}, global={global}, repo={repository:?}"
                    );
                    assert_eq!(
                        crate::actions::basis(&store, &snapshot.items[0].id).is_ok(),
                        !publish
                    );
                }
            }
        }
    }
}

fn unowned_thread(thread: &Thread) -> Thread {
    let mut thread = thread.clone();
    thread.id = "unowned-thread".into();
    thread.comments[0].id = "800".into();
    thread.comments[0].author_id = Some("44".into());
    thread.comments[0].body = "Explain this implementation".into();
    thread.comments.truncate(1);
    thread
}

fn observe_general(store: &Store, thread: &Thread, head: char, now: i64) {
    let ticket = poll(store, head, now);
    let mut binding = primary_binding();
    binding.account_login = "actor".into();
    admit_scan(
        store,
        &ticket,
        Scan {
            threads: vec![(binding, vec![thread.clone()])],
            ..Default::default()
        },
        now + 2,
    )
    .unwrap();
}

fn primary_binding() -> MentionBinding {
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
fn fixwave_settled_new_unowned_work_does_not_keep_superseded_analysis_failed() {
    let (_root, store, _, thread) = fixture(1);
    clear_local_review(&store);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].comment = false;
    store.save_settings(&settings).unwrap();
    let mut thread = unowned_thread(&thread);
    observe_general(&store, &thread, 'a', NOW + 10);
    let capacity = Capacity::default();
    let Dispatch::Reply(mut old, _) = capacity
        .dispatch(&store, NOW + 20)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Old assessment expected");
    };
    let mut comment = thread.comments[0].clone();
    comment.id = "801".into();
    comment.reply_to = Some("800".into());
    comment.body = "A more recent question".into();
    comment.published_at = "2026-10-01T00:00:00Z".into();
    thread.comments.push(comment);
    observe_general(&store, &thread, 'a', NOW + 30);
    let failure = validate_observed_trigger(&old, &Observation::Owned(thread.clone())).unwrap_err();
    complete_analysis(&store, &mut old, Err(failure), true, NOW + 32).unwrap_err();
    let saved_old = store
        .load_follow_ups()
        .unwrap()
        .into_iter()
        .find(|run| run.id == old.id)
        .unwrap();
    let Dispatch::Reply(mut latest, _) = Capacity::default()
        .dispatch(&store, NOW + 40)
        .unwrap()
        .dispatched
        .into_iter()
        .find(|work| work.key().id != old.id)
        .unwrap()
    else {
        panic!("Latest assessment expected");
    };
    let result = output_for(&latest, ReplyDecision::Quiet, vec![]);
    complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
    assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
    assert_eq!(
        store
            .load_follow_ups()
            .unwrap()
            .into_iter()
            .find(|run| run.id == old.id)
            .unwrap(),
        saved_old
    );
    thread.resolved = true;
    observe_general(&store, &thread, 'a', NOW + 50);
    super::super::super::restore(&store).unwrap();
    assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
}

#[test]
fn fixwave_same_assignment_agent_replacement_replans_only_unexecuted_intents() {
    let (_root, store, origin, mut thread) = fixture(2);
    explanation(&mut thread, "11");
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![mention("701", "Explain the result")],
        NOW + 10,
    );
    let original = store.load_follow_ups().unwrap();
    assert_eq!(original.len(), 2);
    let mut settings = store.load_settings().unwrap();
    let assignment = settings.repositories[0].assignments[0].id.clone();
    settings.repositories[0].assignments[0].agent_id = settings.agents[1].id.clone();
    store.save_settings(&settings).unwrap();
    poll(&store, 'a', NOW + 20);
    let proposed = candidates(&store).unwrap();
    assert_eq!(proposed.len(), 2);
    for candidate in proposed {
        let old = original
            .iter()
            .find(|run| run.id == candidate.run.id)
            .unwrap();
        assert!(candidate.blocked.is_none(), "{:?}", candidate.blocked);
        assert_eq!(candidate.run.context.assignment_id, assignment);
        assert_eq!(
            candidate.run.context.selection.agent.id,
            settings.agents[1].id
        );
        assert_eq!(candidate.run.key, old.key);
        assert_eq!(candidate.run.enqueue_order, old.enqueue_order);
        assert_eq!(candidate.run.enqueued_at, old.enqueued_at);
        assert_eq!(
            crate::queue::item_id(&candidate.run.context.job),
            crate::queue::item_id(&old.context.job)
        );
        let dispatched = prepare_dispatch(&store, &candidate.run.id, NOW + 30).unwrap();
        assert_eq!(dispatched.context.selection.agent.id, settings.agents[1].id);
    }
}

#[test]
fn fixwave_replacement_keeps_cancelled_executed_and_uncertain_evidence_frozen() {
    for phase in ["cancelled", "executed", "uncertain"] {
        let (_root, store, origin, thread) = fixture(2);
        observe(
            &store,
            &origin,
            &thread,
            'a',
            vec![mention("701", "Explain the result")],
            NOW + 10,
        );
        let mut run = store.load_follow_ups().unwrap().remove(0);
        if phase == "cancelled" {
            run.cancelled = true;
            save_to_store(&store, &run).unwrap();
        } else {
            run = prepare_dispatch(&store, &run.id, NOW + 20).unwrap();
            let result = output_for(
                &run,
                if phase == "uncertain" {
                    ReplyDecision::Reply
                } else {
                    ReplyDecision::Quiet
                },
                vec![],
            );
            complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
            if phase == "uncertain" {
                run.publication = Some(run.operation("mention_reply", NOW + 22));
                run.publication.as_mut().unwrap().attempted_mutation = Some("mention_reply".into());
                run.uncertain = true;
                run.body = Some(run.reply_body().unwrap());
                save_to_store(&store, &run).unwrap();
            }
        }
        let frozen = store.load_follow_ups().unwrap().remove(0);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0].agent_id = settings.agents[1].id.clone();
        settings.repositories[0].assignments[0]
            .actions
            .as_mut()
            .unwrap()
            .reply = false;
        store.save_settings(&settings).unwrap();
        poll(&store, 'a', NOW + 30);
        let candidate = candidates(&store).unwrap().remove(0);
        assert_eq!(candidate.run, frozen, "{phase}");
        assert_eq!(store.load_follow_ups().unwrap().remove(0), frozen);
        assert!(!candidate.automatic_publication);
        assert_eq!(store.load_publications().unwrap(), vec![origin]);
    }
}

#[test]
fn fixwave_supersession_never_hides_uncertain_publication_or_human_judgment() {
    for human in [false, true] {
        let (_root, store, _, thread) = fixture(1);
        clear_local_review(&store);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0].comment = false;
        store.save_settings(&settings).unwrap();
        let mut thread = unowned_thread(&thread);
        observe_general(&store, &thread, 'a', NOW + 10);
        let mut run =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
        let result = output_for(
            &run,
            if human {
                ReplyDecision::HumanInputRequired
            } else {
                ReplyDecision::Reply
            },
            vec![],
        );
        complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
        if !human {
            run.publication = Some(run.operation("thread_reply", NOW + 22));
            run.uncertain = true;
            run.body = Some(run.reply_body().unwrap());
            save_to_store(&store, &run).unwrap();
        }
        let mut comment = thread.comments[0].clone();
        comment.id = "801".into();
        comment.reply_to = Some("800".into());
        thread.comments.push(comment);
        observe_general(&store, &thread, 'a', NOW + 30);
        let old = candidates(&store)
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.run.id == run.id)
            .unwrap();
        assert!(!old.superseded);
        assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
        if human {
            assert!(old.human_gate);
        } else {
            assert!(old.run.uncertain);
        }
    }
}

#[test]
fn fixwave_unowned_human_boundary_survives_physical_cleanup_and_reopen() {
    for (head, interrupted_write) in [
        ('a', None),
        ('c', None),
        ('a', Some("feedback.json")),
        ('c', Some("publications.json")),
    ] {
        let (root, store, _, thread) = fixture(1);
        clear_local_review(&store);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0].comment = false;
        store.save_settings(&settings).unwrap();
        let mut thread = unowned_thread(&thread);
        observe_general(&store, &thread, 'a', NOW + 10);
        let Dispatch::Reply(mut run, _) = Capacity::default()
            .dispatch(&store, NOW + 20)
            .unwrap()
            .dispatched
            .remove(0)
        else {
            panic!("Primary assessment expected");
        };
        let result = output_for(&run, ReplyDecision::HumanInputRequired, vec![]);
        complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
        let mut queue = store.load_queue_state().unwrap();
        queue.tracked[0].lifecycle = Lifecycle::Closed;
        queue.tracked[0].terminal_observed = true;
        for job in &mut queue.jobs {
            job.waiting = monitoring::WAITING_CLOSED.into();
        }
        store.save_queue_state(&queue).unwrap();
        if let Some(file) = interrupted_write {
            store.fail_state_write(file, 1);
            assert!(crate::retention::maintain(&store, true).is_err());
            crate::retention::recover(&Store::new(root.path().into())).unwrap();
        } else {
            assert_eq!(crate::retention::maintain(&store, true).unwrap(), 1);
        }
        assert!(store.load_follow_ups().unwrap().is_empty());
        assert!(
            !std::fs::read_to_string(root.path().join("state/feedback.json"))
                .unwrap()
                .contains("Explain this implementation")
        );
        drop(store);
        let store = Store::new(root.path().into());
        let mut comment = thread.comments[0].clone();
        comment.id = "801".into();
        comment.reply_to = Some("800".into());
        comment.body = "New evidence after reopening".into();
        comment.published_at = "2026-10-01T00:00:00Z".into();
        thread.comments.push(comment);
        observe_general(&store, &thread, head, NOW + 30);
        let pending = candidates(&store).unwrap().remove(0);
        assert!(pending.human_gate, "head={head}");
        assert!(!pending.automatic_start);
        assert!(request_analysis(&store, &pending.run.id, false, NOW + 40).is_err());
        let mut explicit = request_analysis(&store, &pending.run.id, true, NOW + 40).unwrap();
        explicit
            .analysis
            .as_mut()
            .unwrap()
            .begin_ai_attempt(NOW + 40)
            .unwrap();
        save_to_store(&store, &explicit).unwrap();
        let result = output_for(&explicit, ReplyDecision::Quiet, vec![]);
        complete_analysis(&store, &mut explicit, Ok(result), true, NOW + 41).unwrap();
        let mut next = thread.comments.last().unwrap().clone();
        next.id = "802".into();
        thread.comments.push(next);
        observe_general(&store, &thread, head, NOW + 50);
        assert!(
            !candidates(&store)
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.run.trigger_id == "802")
                .unwrap()
                .human_gate
        );
    }
}

#[test]
fn fixwave_new_trigger_does_not_hide_a_genuine_failed_unowned_assessment() {
    let (_root, store, _, thread) = fixture(1);
    clear_local_review(&store);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].comment = false;
    store.save_settings(&settings).unwrap();
    let mut thread = unowned_thread(&thread);
    observe_general(&store, &thread, 'a', NOW + 10);
    let mut run =
        prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
    complete_analysis(
        &store,
        &mut run,
        Err(Failure::permanent(
            "The provider returned incomplete evidence.",
        )),
        true,
        NOW + 21,
    )
    .unwrap_err();
    let mut comment = thread.comments[0].clone();
    comment.id = "801".into();
    comment.reply_to = Some("800".into());
    thread.comments.push(comment);
    observe_general(&store, &thread, 'a', NOW + 30);
    let mut latest =
        prepare_dispatch(&store, &store.load_follow_ups().unwrap()[1].id, NOW + 40).unwrap();
    let result = output_for(&latest, ReplyDecision::Quiet, vec![]);
    complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
    assert!(
        !candidates(&store)
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.run.id == run.id)
            .unwrap()
            .superseded
    );
    assert_eq!(current_state(&store), crate::queue::State::Failed);
}

#[test]
fn fixwave_queued_retry_keeps_its_executed_context_after_agent_replacement() {
    let (_root, store, origin, thread) = fixture(2);
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![mention("701", "Explain the result")],
        NOW + 10,
    );
    let id = store.load_follow_ups().unwrap()[0].id.clone();
    let mut run = prepare_dispatch(&store, &id, NOW + 20).unwrap();
    complete_analysis(
        &store,
        &mut run,
        Err(Failure::permanent("Incomplete source evidence.")),
        true,
        NOW + 21,
    )
    .unwrap_err();
    let retry = request_analysis(&store, &id, true, NOW + 22).unwrap();
    assert_eq!(retry.analysis_history.len(), 1);
    assert_eq!(retry.analysis.as_ref().unwrap().attempt_count, 0);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].agent_id = settings.agents[1].id.clone();
    store.save_settings(&settings).unwrap();
    poll(&store, 'a', NOW + 30);
    let candidate = candidates(&store).unwrap().remove(0);
    assert_eq!(candidate.run, retry);
    assert!(candidate.blocked.is_some());
    assert_eq!(store.load_follow_ups().unwrap()[0], retry);
}

#[test]
fn adjacent_owned_failure_survives_newer_primary_clearance() {
    for secondary in [false, true] {
        let (_root, store, origin, mut thread) = fixture(if secondary { 2 } else { 1 });
        if secondary {
            let mut settings = store.load_settings().unwrap();
            settings.repositories[0].assignments[0].actions = None;
            settings.repositories[0].primary_assignment_id =
                Some(settings.repositories[0].assignments[1].id.clone());
            settings.repositories[0].assignments[1].comment = false;
            settings.repositories[0].assignments[1].actions = Some(ActionPermissions {
                reply: true,
                approve: false,
                merge: false,
            });
            store.save_settings(&settings).unwrap();
        }
        explanation(&mut thread, "11");
        observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
        let mut old =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
        complete_analysis(
            &store,
            &mut old,
            Err(Failure::permanent(
                "The provider returned incomplete evidence.",
            )),
            true,
            NOW + 21,
        )
        .unwrap_err();
        let saved = store.load_follow_ups().unwrap()[0].clone();
        explanation(&mut thread, "11");
        thread.comments.last_mut().unwrap().id = "102".into();
        observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
        let mut latest =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[1].id, NOW + 40).unwrap();
        let feedback = latest.context.feedback[0].id.clone();
        let result = output_for(
            &latest,
            ReplyDecision::Quiet,
            vec![assessment(&feedback, Disposition::Cleared)],
        );
        complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
        assert_eq!(current_state(&store), crate::queue::State::Failed);
        let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
        assert!(snapshot.items[0]
            .warnings
            .iter()
            .any(|warning| warning == "The provider returned incomplete evidence."));
        assert_eq!(store.load_follow_ups().unwrap()[0], saved);
    }
}

#[test]
fn adjacent_owned_running_cancelled_and_uncertain_work_survives_newer_clearance() {
    for phase in ["running", "cancelled", "uncertain"] {
        let (_root, store, origin, mut thread) = fixture(1);
        explanation(&mut thread, "11");
        observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
        let mut old =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
        if phase == "cancelled" {
            cancel_in_store(&store, &Capacity::default(), None, &old.id, NOW + 21).unwrap();
        } else if phase == "uncertain" {
            let result = output_for(&old, ReplyDecision::Reply, vec![]);
            complete_analysis(&store, &mut old, Ok(result), true, NOW + 21).unwrap();
            old.publication = Some(old.operation("thread_reply", NOW + 22));
            old.publication.as_mut().unwrap().attempted_mutation = Some("thread_reply".into());
            old.uncertain = true;
            old.body = Some(old.reply_body().unwrap());
            save_to_store(&store, &old).unwrap();
        }
        let saved = store.load_follow_ups().unwrap()[0].clone();
        explanation(&mut thread, "11");
        thread.comments.last_mut().unwrap().id = "102".into();
        observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
        let mut latest =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[1].id, NOW + 40).unwrap();
        let result = output_for(
            &latest,
            ReplyDecision::Quiet,
            vec![assessment(
                &latest.context.feedback[0].id,
                Disposition::Cleared,
            )],
        );
        complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
        assert!(!candidates(&store).unwrap()[0].superseded, "{phase}");
        assert_ne!(
            current_state(&store),
            crate::queue::State::MachineSignedOff,
            "{phase}"
        );
        assert_eq!(store.load_follow_ups().unwrap()[0], saved);
    }
}

#[test]
fn adjacent_explicit_answer_clears_only_the_current_unowned_human_boundary() {
    for outcome in ["quiet", "failed", "cancelled", "human"] {
        let (root, store, _, thread) = fixture(1);
        clear_local_review(&store);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0].comment = false;
        store.save_settings(&settings).unwrap();
        let mut thread = unowned_thread(&thread);
        observe_general(&store, &thread, 'a', NOW + 10);
        let mut old =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
        let result = output_for(&old, ReplyDecision::HumanInputRequired, vec![]);
        complete_analysis(&store, &mut old, Ok(result), true, NOW + 21).unwrap();
        let saved = store.load_follow_ups().unwrap()[0].clone();
        let mut comment = thread.comments[0].clone();
        comment.id = "801".into();
        comment.reply_to = Some("800".into());
        thread.comments.push(comment);
        observe_general(&store, &thread, 'a', NOW + 30);
        let id = store.load_follow_ups().unwrap()[1].id.clone();
        assert!(request_analysis(&store, &id, false, NOW + 40).is_err());
        assert_eq!(current_state(&store), crate::queue::State::WaitingForHuman);
        let mut answer = request_analysis(&store, &id, true, NOW + 40).unwrap();
        answer
            .analysis
            .as_mut()
            .unwrap()
            .begin_ai_attempt(NOW + 40)
            .unwrap();
        save_to_store(&store, &answer).unwrap();
        if outcome == "cancelled" {
            cancel_in_store(&store, &Capacity::default(), None, &id, NOW + 41).unwrap();
        } else {
            let result = if outcome == "failed" {
                Err(Failure::permanent("Incomplete answer evidence."))
            } else {
                Ok(output_for(
                    &answer,
                    if outcome == "human" {
                        ReplyDecision::HumanInputRequired
                    } else {
                        ReplyDecision::Quiet
                    },
                    vec![],
                ))
            };
            let completed = complete_analysis(&store, &mut answer, result, true, NOW + 41);
            assert_eq!(completed.is_ok(), outcome != "failed");
        }
        drop(store);
        let store = Store::new(root.path().into());
        assert_eq!(store.load_follow_ups().unwrap()[0], saved);
        let old = candidates(&store).unwrap().remove(0);
        assert_eq!(old.human_gate, outcome != "quiet");
        assert_eq!(old.superseded, outcome == "quiet");
        if outcome == "quiet" {
            assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
            assert!(
                crate::actions::basis(&store, &crate::queue::item_id(&saved.context.job)).is_ok()
            );
        } else {
            assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
        }
    }
}

#[test]
fn adjacent_completed_local_reply_is_not_an_impossible_confirmation_or_replayed_grant() {
    for publish in [false, true] {
        for reply in [false, true] {
            let (_root, store, _, thread) = fixture(1);
            clear_local_review(&store);
            let mut settings = store.load_settings().unwrap();
            settings.repositories[0].assignments[0].comment = publish;
            settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
                reply,
                approve: true,
                merge: false,
            });
            store.save_settings(&settings).unwrap();
            let thread = unowned_thread(&thread);
            observe_general(&store, &thread, 'a', NOW + 10);
            let mut run =
                prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20)
                    .unwrap();
            let result = output_for(&run, ReplyDecision::Reply, vec![]);
            complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
            let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
            assert_eq!(
                snapshot.items[0].state,
                if publish || reply {
                    crate::queue::State::AwaitingPublication
                } else {
                    crate::queue::State::MachineSignedOff
                }
            );
            assert!(run.publication.is_none());
            assert_eq!(
                run.result.as_ref().unwrap().output.decision,
                ReplyDecision::Reply
            );
            if !publish && !reply {
                assert!(crate::actions::basis(&store, &snapshot.items[0].id).is_ok());
                settings.repositories[0].assignments[0]
                    .actions
                    .as_mut()
                    .unwrap()
                    .reply = true;
                store.save_settings(&settings).unwrap();
                let candidate = candidates(&store).unwrap().remove(0);
                assert!(
                    !candidate.automatic_publication,
                    "Local history is not a pending grant."
                );
                assert_eq!(store.load_follow_ups().unwrap()[0], run);
                assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
            }
        }
    }
}

#[test]
fn interleaved_owned_scan_preserves_native_gate_cause_until_new_work_settles() {
    for checkpoint in ["before_send", "final_verification"] {
        let (_root, store, origin, mut thread) = fixture(1);
        explanation(&mut thread, "11");
        observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
        let mut old =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
        let captured = old.clone();
        let mut environment = AnalysisObservation {
            store: &store,
            observation: Ok(Observation::Owned(thread.clone())),
        };
        analysis_checkpoint(&mut environment, &old).unwrap();
        if checkpoint == "final_verification" {
            output_for(&old, ReplyDecision::Quiet, vec![]);
        }
        // Keep the completed provider read of A while the real scan commits B.
        explanation(&mut thread, "11");
        thread.comments.last_mut().unwrap().id = "102".into();
        observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
        let Observation::Owned(held) = environment.observation.as_ref().unwrap() else {
            panic!("Owned observation expected");
        };
        assert_eq!(held.comments.last().unwrap().id, "101");
        assert_eq!(
            store.load_feedback().unwrap().records[0]
                .context
                .thread
                .as_ref()
                .unwrap()
                .comments
                .last()
                .unwrap()
                .id,
            "102"
        );
        let error = analysis_checkpoint(&mut environment, &old).unwrap_err();
        assert_eq!(error.kind, monitoring::OperationFailure::Superseded);
        complete_analysis(&store, &mut old, Err(error), true, NOW + 31).unwrap_err();
        let saved = store.load_follow_ups().unwrap()[0].clone();
        assert_eq!(saved.context, captured.context);
        assert_eq!(saved.target, captured.target);
        assert_eq!(saved.history, captured.history);
        assert_eq!(
            saved.analysis.as_ref().unwrap().failure,
            Some(monitoring::OperationFailure::Superseded)
        );
        assert!(saved.analysis.as_ref().unwrap().next_attempt_at.is_none());
        assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
        let mut latest =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[1].id, NOW + 40).unwrap();
        let result = output_for(
            &latest,
            ReplyDecision::Quiet,
            vec![assessment(
                &latest.context.feedback[0].id,
                Disposition::Cleared,
            )],
        );
        complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
        assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
        assert_eq!(store.load_follow_ups().unwrap()[0], saved);
    }
}

#[test]
fn native_gate_preserves_real_failure_fields_and_aggregate_attention() {
    for failure in [
        Failure::permanent(super::super::super::SUPERSEDED_TRIGGER),
        Failure {
            kind: monitoring::OperationFailure::Network,
            retry_after_seconds: Some(17),
            ..Failure::permanent(super::super::super::SUPERSEDED_TRIGGER)
        },
        Failure::cancelled(),
    ] {
        let (_root, store, origin, mut thread) = fixture(1);
        explanation(&mut thread, "11");
        observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
        let mut old =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
        let actual = native_gate_result(Err(failure.clone())).unwrap_err();
        assert_eq!(actual.kind, failure.kind);
        assert_eq!(actual.message, failure.message);
        assert_eq!(actual.cancelled, failure.cancelled);
        assert_eq!(actual.retry_after_seconds, failure.retry_after_seconds);
        complete_analysis(&store, &mut old, Err(actual), true, NOW + 21).unwrap_err();
        let saved = store.load_follow_ups().unwrap()[0].clone();
        explanation(&mut thread, "11");
        thread.comments.last_mut().unwrap().id = "102".into();
        observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
        let mut latest =
            prepare_dispatch(&store, &store.load_follow_ups().unwrap()[1].id, NOW + 40).unwrap();
        let result = output_for(
            &latest,
            ReplyDecision::Quiet,
            vec![assessment(
                &latest.context.feedback[0].id,
                Disposition::Cleared,
            )],
        );
        complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
        assert_eq!(current_state(&store), crate::queue::State::Failed);
        assert!(!candidates(&store).unwrap()[0].superseded);
        assert_eq!(store.load_follow_ups().unwrap()[0], saved);
    }
}

struct AnalysisObservation<'a> {
    store: &'a Store,
    observation: Result<Observation, Failure>,
}

impl Environment for AnalysisObservation<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(NOW + 31)
    }
    fn save(&mut self, _: &mut FollowUp) -> Result<(), Failure> {
        panic!("Checkpoint must not rewrite captured work");
    }
    fn observe(&mut self, _: &FollowUp) -> Result<Observation, Failure> {
        self.observation.clone()
    }
    fn gate(
        &mut self,
        run: &FollowUp,
        observation: &Observation,
    ) -> Result<Option<String>, Failure> {
        if let Some(reason) = native_gate_result(local_gate(self.store, run))? {
            return Ok(Some(reason));
        }
        Ok((!run.fresh_observation(observation)).then(|| "Discussion changed.".into()))
    }
    fn reply(&mut self, _: &FollowUp) -> Result<String, WriteFailure> {
        panic!("Read-only analysis checkpoint cannot publish");
    }
}

#[test]
fn adjacent_mid_analysis_checkpoints_classify_only_verified_new_comment_supersession() {
    for owned in [false, true] {
        for checkpoint in ["before_send", "final_verification"] {
            let (_root, store, origin, thread) = fixture(1);
            let mut thread = if owned {
                thread
            } else {
                clear_local_review(&store);
                let mut settings = store.load_settings().unwrap();
                settings.repositories[0].assignments[0].comment = false;
                store.save_settings(&settings).unwrap();
                unowned_thread(&thread)
            };
            if owned {
                explanation(&mut thread, "11");
                observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
            } else {
                observe_general(&store, &thread, 'a', NOW + 10);
            }
            let mut old =
                prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20)
                    .unwrap();
            let captured = old.context.clone();
            let mut environment = AnalysisObservation {
                store: &store,
                observation: Ok(Observation::Owned(thread.clone())),
            };
            analysis_checkpoint(&mut environment, &old).unwrap();
            if checkpoint == "final_verification" {
                output_for(&old, ReplyDecision::Quiet, vec![]);
            }
            let mut comment = thread.comments.last().unwrap().clone();
            comment.id = if owned { "102" } else { "801" }.into();
            comment.reply_to = Some(thread.comments[0].id.clone());
            comment.body = "A new question before the next analysis checkpoint".into();
            thread.comments.push(comment);
            environment.observation = Ok(Observation::Owned(thread.clone()));
            if owned {
                observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
                assert_eq!(
                    local_gate(&store, &old).unwrap_err().kind,
                    monitoring::OperationFailure::Superseded
                );
            }
            let error = analysis_checkpoint(&mut environment, &old).unwrap_err();
            assert_eq!(error.kind, monitoring::OperationFailure::Superseded);
            complete_analysis(&store, &mut old, Err(error), true, NOW + 31).unwrap_err();
            if owned {
                observe(&store, &origin, &thread, 'a', vec![], NOW + 32);
            } else {
                observe_general(&store, &thread, 'a', NOW + 32);
            }
            let saved = store.load_follow_ups().unwrap()[0].clone();
            assert_eq!(saved.context, captured);
            assert_eq!(
                saved.analysis.as_ref().unwrap().failure,
                Some(monitoring::OperationFailure::Superseded)
            );
            assert_eq!(
                saved.analysis.as_ref().unwrap().state,
                OperationState::Failed
            );
            assert!(saved.analysis.as_ref().unwrap().next_attempt_at.is_none());
            let mut latest =
                prepare_dispatch(&store, &store.load_follow_ups().unwrap()[1].id, NOW + 40)
                    .unwrap();
            let assessments = if owned {
                vec![assessment(
                    &latest.context.feedback[0].id,
                    Disposition::Cleared,
                )]
            } else {
                vec![]
            };
            let result = output_for(&latest, ReplyDecision::Quiet, assessments);
            complete_analysis(&store, &mut latest, Ok(result), true, NOW + 41).unwrap();
            assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
            assert_eq!(store.load_follow_ups().unwrap()[0], saved);
        }
    }
}

#[test]
fn adjacent_analysis_checkpoint_keeps_provider_failures_and_edits_distinct_from_supersession() {
    let (_root, store, _, thread) = fixture(1);
    clear_local_review(&store);
    let thread = unowned_thread(&thread);
    observe_general(&store, &thread, 'a', NOW + 10);
    let run = prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
    let mut changed = thread.clone();
    changed.comments[0].body = "Edited original evidence".into();
    let mut environment = AnalysisObservation {
        store: &store,
        observation: Ok(Observation::Owned(changed.clone())),
    };
    assert_eq!(
        analysis_checkpoint(&mut environment, &run)
            .unwrap_err()
            .kind,
        monitoring::OperationFailure::Permanent
    );
    let mut comment = changed.comments[0].clone();
    comment.id = "801".into();
    changed.comments.push(comment);
    environment.observation = Ok(Observation::Owned(changed));
    assert_eq!(
        analysis_checkpoint(&mut environment, &run)
            .unwrap_err()
            .kind,
        monitoring::OperationFailure::Permanent
    );
    environment.observation = Err(Failure::permanent(super::super::super::SUPERSEDED_TRIGGER));
    assert_eq!(
        analysis_checkpoint(&mut environment, &run)
            .unwrap_err()
            .kind,
        monitoring::OperationFailure::Permanent,
        "A diagnostic alone is not a supersession cause"
    );
}
