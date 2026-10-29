use super::*;
use crate::{
    exits::{Exit, ExitCall},
    github::{
        review::{ReviewContext, ReviewFile},
        state::{PrSnapshot, SnapshotCheck},
    },
    retention::Binding,
    review::{host as review_host, packet_run, runtime::Refusal, ReviewOutput},
};

fn snapshot(draft: bool, conclusion: &str) -> PrSnapshot {
    PrSnapshot {
        number: 1,
        pull_request_id: "9".into(),
        title: "Review".into(),
        head: "a".repeat(40),
        base: "b".repeat(40),
        state: "OPEN".into(),
        draft,
        mergeable: "MERGEABLE".into(),
        checks: vec![SnapshotCheck {
            name: "CI".into(),
            status: if conclusion.is_empty() {
                "IN_PROGRESS"
            } else {
                "COMPLETED"
            }
            .into(),
            conclusion: conclusion.into(),
        }],
        check_total: 1,
        ..Default::default()
    }
}

fn context(state: &PrSnapshot) -> ReviewContext {
    ReviewContext {
        pull: PullRequest {
            mentioned: false,
            id: state.pull_request_id.clone(),
            number: state.number,
            title: state.title.clone(),
            author: Some(Identity {
                id: "11".into(),
                login: "author".into(),
            }),
            requested_reviewers: vec![],
            requested_teams: vec![],
            state: match state.state.as_str() {
                "CLOSED" => Lifecycle::Closed,
                "MERGED" => Lifecycle::Merged,
                _ => Lifecycle::Open,
            },
            draft: state.draft,
            head_sha: state.head.clone(),
            base_sha: state.base.clone(),
            head_repository_id: Some("100".into()),
            base_repository_id: "100".into(),
            updated_at: "2026-09-30T00:00:00Z".into(),
            files: vec![],
        },
        base_revision: state.base.clone(),
        files: vec![ReviewFile {
            path: "source.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: Some("@@ -1 +1 @@\n-old\n+new".into()),
        }],
        head: BTreeMap::new(),
        base: BTreeMap::new(),
    }
}

fn poll(store: &Store, state: &PrSnapshot, now: i64) {
    let mut monitor = Monitor::restore(store).unwrap();
    let ticket = monitor.prepare_checks(store, now, true).unwrap().remove(0);
    finish_poll(&mut monitor, store, ticket, state, now);
}

fn finish_poll(
    monitor: &mut Monitor,
    store: &Store,
    ticket: crate::monitoring::PollTicket,
    state: &PrSnapshot,
    now: i64,
) {
    monitor
        .finish(
            store,
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
                pull_requests: vec![context(state).pull],
                states: BTreeMap::from([("9".into(), state.clone())]),
            }),
            now + 1,
        )
        .unwrap();
}

fn waiting_ci() -> (tempfile::TempDir, Store, String, ReviewRun, PrSnapshot) {
    let (root, store, item) = fixture(1, true, true);
    store.save_reviews(&[]).unwrap();
    let state = snapshot(false, "");
    poll(&store, &state, NOW + 10);
    let env = env(&store);
    env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
    let (run, prepared) = prepare(&store, &env, &state, true, NOW + 12);
    assert!(prepared.setup.offered.contains(&Exit::Wait));
    // Pending CI alone parks a sign-off: lgtm is the clean exit, wait stays callable.
    assert!(prepared.packet.text.contains("clean_review_exit: lgtm"));
    complete(
        &store,
        &run,
        &prepared,
        ExitCall::Wait {
            reason: crate::exits::WaitReason::Ci,
        },
        NOW + 13,
    );
    (root, store, item, run, state)
}

#[test]
fn wait_yields_persists_and_scheduled_ci_completion_wakes_exactly_once() {
    for conclusion in ["SUCCESS", "FAILURE"] {
        let (root, store, item, run, mut state) = waiting_ci();
        let binding = Binding::job(&run.job);
        let record = store
            .load_pr_states()
            .unwrap()
            .get(&binding)
            .unwrap()
            .clone();
        assert!(record.external_wait.is_some());
        assert!(record.exit.is_none() && record.local_summary.is_none());
        let coordinator = crate::capacity::Coordinator::default();
        let capacity = coordinator.snapshot(&store, NOW + 14).unwrap();
        assert_eq!(capacity.active, 0);
        assert!(capacity.work.is_empty());
        let projected = crate::queue::snapshot(&store, vec![]).unwrap();
        assert_eq!(projected.items[0].state, crate::queue::State::WaitingOnCi);
        assert!(crate::notifications::frames(&projected)
            .iter()
            .all(|f| f.event.is_none()));
        assert!(host::prepare_next_effect(&store, &item, NOW + 14)
            .unwrap()
            .is_none());

        // Real Store reload; no new schedule, no held runtime, no repeated reviews.
        let store = Store::new(root.path().into());
        state.checks[0].status = "QUEUED".into();
        poll(&store, &state, NOW + 15);
        state.checks[0].status = "IN_PROGRESS".into();
        state.checks.push(SnapshotCheck {
            name: "lint".into(),
            status: "COMPLETED".into(),
            conclusion: "SUCCESS".into(),
        });
        state.check_total = 2;
        poll(&store, &state, NOW + 20);
        assert_eq!(
            store.load_pr_states().unwrap().state_seq(&binding),
            record.state_seq
        );
        assert!(crate::capacity::candidates(&store, NOW + 22)
            .unwrap()
            .is_empty());
        assert_eq!(store.load_reviews().unwrap().len(), 1);

        let due = store
            .load_monitoring_state()
            .unwrap()
            .health
            .values()
            .next()
            .unwrap()
            .scheduled_run;
        assert!(due > NOW + 22);
        let mut monitor = Monitor::restore(&store).unwrap();
        assert!(monitor
            .prepare_checks(&store, due - 1, false)
            .unwrap()
            .is_empty());
        let ticket = monitor
            .prepare_checks(&store, due, false)
            .unwrap()
            .remove(0);
        assert!(ticket.tracked.iter().any(|(_, n)| *n == state.number));
        state.checks[0].status = "COMPLETED".into();
        state.checks[0].conclusion = conclusion.into();
        finish_poll(&mut monitor, &store, ticket, &state, due);
        poll(&store, &state, due + 2);
        let states = store.load_pr_states().unwrap();
        assert_eq!(states.state_seq(&binding), record.state_seq + 1);
        assert!(states.get(&binding).unwrap().external_wait.is_none());
        assert!(states.get(&binding).unwrap().exit.is_none());
        assert_eq!(
            crate::capacity::candidates(&store, due + 4).unwrap().len(),
            1
        );
        let env = env(&store);
        env.wire.0.lock().unwrap().observation.checks = Some(conclusion.into());
        let (_, prepared) = prepare(&store, &env, &state, true, due + 4);
        assert_eq!(
            prepared.setup.offered.contains(&Exit::ApproveAndMerge),
            conclusion == "SUCCESS"
        );
        assert_eq!(
            prepared.setup.offered.contains(&Exit::ApprovePullRequest),
            conclusion == "SUCCESS"
        );
        assert!(!prepared.setup.offered.contains(&Exit::Wait));
        assert!(env.wire.0.lock().unwrap().writes.is_empty());
    }
}

#[test]
fn wait_invalidates_on_new_head_base_or_discussion_without_clearance() {
    for change in ["head", "base", "body"] {
        let (_root, store, _, run, mut state) = waiting_ci();
        match change {
            "head" => state.head = "c".repeat(40),
            "base" => state.base = "c".repeat(40),
            _ => state.body = "New review context".into(),
        }
        poll(&store, &state, NOW + 20);
        let states = store.load_pr_states().unwrap();
        let record = states.get(&Binding::job(&run.job)).unwrap();
        assert!(
            record.external_wait.is_none() && record.exit.is_none(),
            "{change}"
        );
        let work = crate::capacity::candidates(&store, NOW + 22).unwrap();
        assert_eq!(work.len(), 1, "{change}");
        assert!(work[0].reason.is_none(), "{change}: {:?}", work[0].reason);
    }
}

#[test]
fn wait_pause_disable_and_removed_assignment_never_admit_work() {
    for gate in ["paused", "disabled", "unassigned"] {
        let (_root, store, _, _, mut state) = waiting_ci();
        let mut settings = store.load_settings().unwrap();
        match gate {
            "paused" => {
                let mut automation = store.load_automation().unwrap();
                automation.paused = true;
                store.save_automation(&automation).unwrap();
            }
            "disabled" => {
                settings.repositories[0].enabled = false;
                store.save_settings(&settings).unwrap();
            }
            _ => {
                settings.repositories[0].assignments.clear();
                settings.repositories[0].primary_assignment_id = None;
                store.save_settings(&settings).unwrap();
            }
        }
        state.checks[0].conclusion = "SUCCESS".into();
        let mut monitor = Monitor::restore(&store).unwrap();
        let tickets = monitor.prepare_checks(&store, NOW + 30, false).unwrap();
        assert!(tickets.is_empty(), "{gate}");
        let batch = crate::capacity::Coordinator::default()
            .dispatch(&store, NOW + 30)
            .unwrap();
        assert!(batch.dispatched.is_empty(), "{gate}");
    }
}

#[test]
fn wait_exit_rechecks_ci_and_any_primary_may_wait_for_ci() {
    for custom in [false, true] {
        let (_root, store, _) = fixture(1, true, true);
        store.save_reviews(&[]).unwrap();
        if custom {
            let mut settings = store.load_settings().unwrap();
            settings.repositories[0].assignments[0].tools = Some(vec![
                "read_changes".into(),
                "approve_and_merge".into(),
                "ready_for_human".into(),
                "panic_exit".into(),
            ]);
            store.save_settings(&settings).unwrap();
        }

        let state = snapshot(false, "");
        poll(&store, &state, NOW + 10);
        let env = env(&store);
        env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
        let (_, prepared) = prepare(&store, &env, &state, true, NOW + 12);
        // Pending CI alone blocks the sign-off: Wait is offered without the tool.
        assert!(prepared.setup.offered.contains(&Exit::Wait), "{custom}");
        let call = ExitCall::Wait {
            reason: crate::exits::WaitReason::Ci,
        };
        for rollup in ["SUCCESS", "FAILURE"] {
            env.wire.0.lock().unwrap().observation.checks = Some(rollup.into());
            assert!(matches!(
                (prepared.setup.gate)(&call),
                Err(Refusal::Denied(_))
            ));
        }
        assert!(env.wire.0.lock().unwrap().writes.is_empty());
    }
}

#[test]
fn wait_persistence_before_result_save_does_not_restart_ai_or_fabricate_clearance() {
    let (root, store, _, run, mut state) = waiting_ci();
    // Model a restart after the wait write committed but before the result write.
    let mut reviews = store.load_reviews().unwrap();
    reviews[0].operation.state = OperationState::Interrupted;
    reviews[0].result = None;
    store.save_reviews(&reviews).unwrap();
    let store = Store::new(root.path().into());
    assert!(crate::capacity::candidates(&store, NOW + 20)
        .unwrap()
        .is_empty());
    assert_eq!(
        crate::queue::snapshot(&store, vec![]).unwrap().items[0].state,
        crate::queue::State::WaitingOnCi
    );
    assert!(store.load_reviews().unwrap()[0].result.is_none());
    state.checks[0].status = "COMPLETED".into();
    state.checks[0].conclusion = "SUCCESS".into();
    poll(&store, &state, NOW + 30);
    poll(&store, &state, NOW + 35);
    assert_eq!(
        crate::capacity::candidates(&store, NOW + 37).unwrap().len(),
        1
    );
    assert!(store
        .load_pr_states()
        .unwrap()
        .get(&Binding::job(&run.job))
        .unwrap()
        .exit
        .is_none());
}

#[test]
fn wait_inflight_completion_while_paused_only_dispatches_after_resume() {
    let (_root, store, _, run, mut state) = waiting_ci();
    let mut monitor = Monitor::restore(&store).unwrap();
    let ticket = monitor
        .prepare_checks(&store, NOW + 20, true)
        .unwrap()
        .remove(0);
    let mut automation = store.load_automation().unwrap();
    automation.paused = true;
    store.save_automation(&automation).unwrap();
    state.checks[0].status = "COMPLETED".into();
    state.checks[0].conclusion = "SUCCESS".into();
    let result = monitor.finish(
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
            pull_requests: vec![context(&state).pull],
            states: BTreeMap::from([("9".into(), state.clone())]),
        }),
        NOW + 21,
    );
    result.unwrap();
    let states = store.load_pr_states().unwrap();
    let record = states.get(&Binding::job(&run.job)).unwrap();
    assert!(record.external_wait.is_none() && record.exit.is_none());
    assert_eq!(record.state_seq, 2);
    assert!(crate::capacity::Coordinator::default()
        .dispatch(&store, NOW + 22)
        .unwrap()
        .dispatched
        .is_empty());
    automation.paused = false;
    store.save_automation(&automation).unwrap();
    poll(&store, &state, NOW + 30);
    assert_eq!(
        store
            .load_pr_states()
            .unwrap()
            .state_seq(&Binding::job(&run.job)),
        2
    );
    assert_eq!(
        crate::capacity::candidates(&store, NOW + 32).unwrap().len(),
        1
    );
}

#[test]
fn wait_uses_the_fresh_packet_state_not_the_older_poll_snapshot() {
    let (_root, store, _) = fixture(1, true, true);
    store.save_reviews(&[]).unwrap();
    let mut state = snapshot(false, "");
    poll(&store, &state, NOW + 10);
    state.body = "Changed before dispatch; reviewed in the fresh packet.".into();
    let env = env(&store);
    env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
    let (run, prepared) = prepare(&store, &env, &state, true, NOW + 12);
    complete(
        &store,
        &run,
        &prepared,
        ExitCall::Wait {
            reason: crate::exits::WaitReason::Ci,
        },
        NOW + 13,
    );
    poll(&store, &state, NOW + 20);
    let states = store.load_pr_states().unwrap();
    let record = states.get(&Binding::job(&run.job)).unwrap();
    assert_eq!(
        record.external_wait.as_ref().unwrap().snapshot,
        state.normalize()
    );
    assert_eq!(record.state_seq, 1);
    assert!(record.exit.is_none());
    assert!(crate::capacity::candidates(&store, NOW + 22)
        .unwrap()
        .is_empty());
}

#[test]
fn wait_terminal_observation_retires_without_new_work() {
    for (lifecycle, expected) in [
        ("CLOSED", crate::queue::State::Closed),
        ("MERGED", crate::queue::State::Merged),
    ] {
        let (_root, store, _, run, mut state) = waiting_ci();
        state.state = lifecycle.into();
        poll(&store, &state, NOW + 20);
        let states = store.load_pr_states().unwrap();
        assert!(states
            .get(&Binding::job(&run.job))
            .unwrap()
            .external_wait
            .is_none());
        assert!(crate::capacity::candidates(&store, NOW + 22)
            .unwrap()
            .is_empty());
        assert_eq!(
            crate::queue::snapshot(&store, vec![]).unwrap().items[0].state,
            expected
        );
    }
}

#[test]
fn wait_green_resume_can_approve_and_merge_only_through_existing_action_gates() {
    let (_root, store, item, _, state) = waiting_ci();
    let mut green = state;
    green.checks[0].status = "COMPLETED".into();
    green.checks[0].conclusion = "SUCCESS".into();
    poll(&store, &green, NOW + 20);
    let mut env = env(&store);
    let (run, prepared) = prepare(&store, &env, &green, true, NOW + 22);
    complete(
        &store,
        &run,
        &prepared,
        ExitCall::ApproveAndMerge {
            comment: "Fresh head and base reviewed after CI completion.".into(),
        },
        NOW + 23,
    );
    synchronize(
        &store,
        &item,
        Ok(env.wire.0.lock().unwrap().observation.clone()),
        NOW + 24,
    )
    .unwrap();
    packet::adopt(&store, &item).unwrap();
    let approval = host::prepare_next_effect(&store, &item, NOW + 25)
        .unwrap()
        .unwrap();
    assert_eq!(approval.action, Action::Approve);
    env.now = NOW + 25;
    host::execute_action(&mut env, &approval).unwrap();
    synchronize(
        &store,
        &item,
        Ok(env.wire.0.lock().unwrap().observation.clone()),
        NOW + 26,
    )
    .unwrap();
    let merge = host::prepare_next_effect(&store, &item, NOW + 27)
        .unwrap()
        .unwrap();
    assert_eq!(merge.action, Action::Merge);
    env.now = NOW + 27;
    host::execute_action(&mut env, &merge).unwrap();
    let server = env.wire.0.lock().unwrap();
    assert_eq!(server.observation.state, "MERGED");
    assert_eq!(server.writes.len(), 2);
}

#[test]
fn approve_only_and_no_permission_primaries_wait_for_ci_then_sign_off() {
    use crate::exits::LgtmAction;
    for (approve, action) in [(true, LgtmAction::Approve), (false, LgtmAction::Ready)] {
        let (_root, store, item) = fixture(1, approve, false);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0].tools = Some(vec![
            "read_changes".into(),
            "ready_for_human".into(),
            "panic_exit".into(),
        ]);
        store.save_settings(&settings).unwrap();
        store.save_reviews(&[]).unwrap();
        let state = snapshot(false, "");
        poll(&store, &state, NOW + 10);
        let mut env = env(&store);
        env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
        let (run, prepared) = prepare(&store, &env, &state, true, NOW + 12);
        // A local Ready sign-off is denied in the packet, callable only so CI settling
        // mid-run can sign off; an approval parks instead.
        assert_eq!(
            prepared
                .packet
                .record
                .exits_denied
                .iter()
                .any(|denial| denial.exit == Exit::Lgtm),
            !approve
        );
        assert!(prepared.setup.offered.contains(&Exit::Wait), "{action:?}");
        assert!(prepared.packet.text.contains(if approve {
            "clean_review_exit: lgtm"
        } else {
            "clean_review_exit: wait"
        }));
        complete(
            &store,
            &run,
            &prepared,
            ExitCall::Wait {
                reason: crate::exits::WaitReason::Ci,
            },
            NOW + 13,
        );
        let binding = Binding::job(&run.job);
        let waited = store.load_pr_states().unwrap().state_seq(&binding);
        // The wait survives polling with its configuration and holds while CI runs.
        poll(&store, &state, NOW + 15);
        let states = store.load_pr_states().unwrap();
        assert!(states.get(&binding).unwrap().external_wait.is_some());
        assert_eq!(states.state_seq(&binding), waited);
        assert!(crate::capacity::candidates(&store, NOW + 16)
            .unwrap()
            .is_empty());

        let green = snapshot(false, "SUCCESS");
        env.wire.0.lock().unwrap().observation.checks = Some("SUCCESS".into());
        poll(&store, &green, NOW + 20);
        assert_eq!(
            store.load_pr_states().unwrap().state_seq(&binding),
            waited + 1
        );
        let (run, prepared) = prepare(&store, &env, &green, true, NOW + 22);
        assert!(prepared.setup.offered.contains(&Exit::Lgtm), "{action:?}");
        complete(
            &store,
            &run,
            &prepared,
            ExitCall::Lgtm {
                comment: "Reviewed; CI green.".into(),
                resolve_own_threads: vec![],
                action,
                dismissed_teammate_findings: vec![],
            },
            NOW + 23,
        );
        synchronize(
            &store,
            &item,
            Ok(env.wire.0.lock().unwrap().observation.clone()),
            NOW + 24,
        )
        .unwrap();
        packet::adopt(&store, &item).unwrap();
        let next = host::prepare_next_effect(&store, &item, NOW + 25).unwrap();
        if approve {
            let approval = next.unwrap();
            assert_eq!(approval.action, Action::Approve);
            env.now = NOW + 25;
            host::execute_action(&mut env, &approval).unwrap();
            assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1);
        } else {
            assert!(next.is_none());
            assert!(env.wire.0.lock().unwrap().writes.is_empty());
            let record = store.load_pr_states().unwrap();
            let record = record.get(&binding).unwrap();
            assert!(record.local_summary.is_some(), "local Ready sign-off");
            assert!(record.external_wait.is_none());
        }
    }
}

fn read_only_primary(approve: bool) -> (tempfile::TempDir, Store, String) {
    let (root, store, item) = fixture(1, approve, false);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].tools = Some(vec![
        "read_changes".into(),
        "ready_for_human".into(),
        "panic_exit".into(),
    ]);
    store.save_settings(&settings).unwrap();
    store.save_reviews(&[]).unwrap();
    (root, store, item)
}

fn sign_off(action: crate::exits::LgtmAction) -> ExitCall {
    ExitCall::Lgtm {
        comment: "Reviewed; CI green.".into(),
        resolve_own_threads: vec![],
        action,
        dismissed_teammate_findings: vec![],
    }
}

#[test]
fn ci_turning_green_mid_run_leaves_the_primary_its_sign_off() {
    use crate::exits::LgtmAction;
    for (approve, action) in [(true, LgtmAction::Approve), (false, LgtmAction::Ready)] {
        let (_root, store, _) = read_only_primary(approve);
        let state = snapshot(false, "");
        poll(&store, &state, NOW + 10);
        let env = env(&store);
        env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
        let (_, prepared) = prepare(&store, &env, &state, true, NOW + 12);
        assert!(prepared.setup.offered.contains(&Exit::Wait), "{action:?}");
        assert!(prepared.setup.offered.contains(&Exit::Lgtm), "{action:?}");
        assert!(prepared.setup.tool_set.iter().any(|t| t == "lgtm"));
        // An approval parks while CI is pending; a local Ready sign-off waits for it.
        assert_eq!(
            prepared
                .packet
                .text
                .contains(crate::exits::gates::CI_PENDING_SIGN_OFF),
            !approve
        );
        if approve {
            assert_eq!((prepared.setup.gate)(&sign_off(action)), Ok(()));
        } else {
            assert!(matches!(
                (prepared.setup.gate)(&sign_off(action)),
                Err(Refusal::Denied(reason)) if reason == crate::exits::gates::CI_PENDING_SIGN_OFF
            ));
        }
        env.wire.0.lock().unwrap().observation.checks = Some("SUCCESS".into());
        assert_eq!(
            (prepared.setup.gate)(&sign_off(action)),
            Ok(()),
            "{action:?}"
        );
        assert!(env.wire.0.lock().unwrap().writes.is_empty());
    }
}

/// A clean primary with merge authority signs off while CI is still running: the
/// sign-off is accepted and parked on this revision, with nothing written.
fn parked_lgtm(self_authored: bool) -> (tempfile::TempDir, String, PrSnapshot) {
    let (root, store, item) = fixture(1, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0]
        .actions
        .as_mut()
        .unwrap()
        .include_self = true;
    store.save_settings(&settings).unwrap();
    store.save_reviews(&[]).unwrap();
    let state = snapshot(false, "");
    poll(&store, &state, NOW + 10);
    let env = env(&store);
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.checks = Some("PENDING".into());
        server.observation.review_decision = None;
        if self_authored {
            server.observation.author_id = server.observation.account_id.clone();
        }
    }
    let (run, prepared) = prepare(&store, &env, &state, true, NOW + 12);
    assert!(prepared.setup.offered.contains(&Exit::Lgtm));
    assert!(prepared.packet.text.contains("clean_review_exit: lgtm"));
    assert!(!prepared
        .packet
        .record
        .exits_denied
        .iter()
        .any(|denial| denial.exit == Exit::Lgtm));
    complete(
        &store,
        &run,
        &prepared,
        sign_off(crate::exits::LgtmAction::Merge),
        NOW + 13,
    );
    let observation = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &item, Ok(observation.clone()), NOW + 14).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert!(host::prepare_next_effect(&store, &item, NOW + 14)
        .unwrap()
        .is_none());
    let ledger = store.load_actions().unwrap();
    let parked = ledger.finals.iter().find(|f| f.packet.is_some()).unwrap();
    // Self-authored: GitHub refuses the vote, so only the merge is wanted.
    let wanted: &[Action] = if self_authored {
        &[Action::Merge]
    } else {
        &[Action::Approve, Action::Merge]
    };
    for &action in wanted {
        let reason = ready(&store, parked, &observation, action).unwrap_err();
        assert!(
            reason.contains("LGTM pending on revision aaaaaaaaaaaa"),
            "{reason}"
        );
    }
    assert!(env.wire.0.lock().unwrap().writes.is_empty());
    let projected = crate::queue::snapshot(&store, vec![]).unwrap();
    let row = projected.items.iter().find(|i| i.id == item).unwrap();
    // Signed off, not handed to a human: the queue shows the LGTM and its parked action.
    assert_eq!(row.state, crate::queue::State::MachineSignedOff);
    assert_eq!(row.exit, Some(Exit::Lgtm));
    assert_eq!(
        row.progress,
        Some(if self_authored {
            crate::queue::Progress::Merging
        } else {
            crate::queue::Progress::Approving
        })
    );
    assert!(row
        .action_status
        .as_ref()
        .unwrap()
        .blockers
        .iter()
        .any(|b| b.starts_with("LGTM pending on revision")));
    (root, item, state)
}

#[test]
fn parked_lgtm_acts_once_ci_is_green_on_the_same_revision_without_another_review() {
    for self_authored in [true, false] {
        let (root, item, state) = parked_lgtm(self_authored);
        // Persisted: a fresh Store over the same data dir holds the parked decision.
        let store = Store::new(root.path().into());
        let mut env = env(&store);
        {
            let mut server = env.wire.0.lock().unwrap();
            server.observation.checks = Some("PENDING".into());
            server.observation.review_decision = None;
            if self_authored {
                server.observation.author_id = server.observation.account_id.clone();
            }
        }
        poll(&store, &state, NOW + 15);
        let mut green = state.clone();
        green.checks[0].status = "COMPLETED".into();
        green.checks[0].conclusion = "SUCCESS".into();
        poll(&store, &green, NOW + 20);
        assert!(crate::capacity::candidates(&store, NOW + 21)
            .unwrap()
            .is_empty());
        env.wire.0.lock().unwrap().observation.checks = Some("SUCCESS".into());
        let mut writes = 0;
        for (step, expected) in [(0, Action::Approve), (1, Action::Merge)] {
            if self_authored && expected == Action::Approve {
                continue;
            }
            let now = NOW + 22 + step * 2;
            synchronize(
                &store,
                &item,
                Ok(env.wire.0.lock().unwrap().observation.clone()),
                now,
            )
            .unwrap();
            packet::adopt(&store, &item).unwrap();
            let effect = host::prepare_next_effect(&store, &item, now)
                .unwrap()
                .unwrap();
            assert_eq!(effect.action, expected, "self_authored={self_authored}");
            env.now = now;
            host::execute_action(&mut env, &effect).unwrap();
            writes += 1;
        }
        let server = env.wire.0.lock().unwrap();
        assert_eq!(server.observation.state, "MERGED");
        assert_eq!(server.writes.len(), writes);
        assert_eq!(
            store.load_reviews().unwrap().len(),
            1,
            "no further AI run: the recorded LGTM acted"
        );
    }
}

#[test]
fn parked_lgtm_is_revoked_by_a_new_head_comment_or_red_ci() {
    use crate::github::state::SnapshotComment;
    for change in ["head", "comment", "red"] {
        let (root, item, mut state) = parked_lgtm(true);
        let store = Store::new(root.path().into());
        let env = env(&store);
        let binding = Binding::job(&store.load_queue().unwrap()[0]);
        let parked_seq = store.load_pr_states().unwrap().state_seq(&binding);
        let rollup = match change {
            "head" => {
                state.head = "c".repeat(40);
                env.wire.0.lock().unwrap().observation.head = "c".repeat(40);
                "SUCCESS"
            }
            "comment" => {
                state.comments.push(SnapshotComment {
                    id: "human".into(),
                    body: "Hold on, one more case.".into(),
                    commit: None,
                });
                state.comment_total = 1;
                "SUCCESS"
            }
            _ => {
                state.checks[0].status = "COMPLETED".into();
                state.checks[0].conclusion = "FAILURE".into();
                "FAILURE"
            }
        };
        poll(&store, &state, NOW + 20);
        {
            let mut server = env.wire.0.lock().unwrap();
            server.observation.author_id = server.observation.account_id.clone();
            server.observation.checks = Some(rollup.into());
        }
        let observation = env.wire.0.lock().unwrap().observation.clone();
        synchronize(&store, &item, Ok(observation.clone()), NOW + 22).unwrap();
        packet::adopt(&store, &item).unwrap();
        assert!(
            host::prepare_next_effect(&store, &item, NOW + 22)
                .unwrap()
                .is_none(),
            "{change}"
        );
        let ledger = store.load_actions().unwrap();
        let parked = ledger.finals.iter().find(|f| f.packet.is_some()).unwrap();
        let reason = ready(&store, parked, &observation, Action::Merge).unwrap_err();
        let expected = match change {
            "head" => "moved",
            "comment" => packet::LGTM_REVOKED,
            _ => packet::LGTM_REVOKED_RED,
        };
        assert!(reason.contains(expected), "{change}: {reason}");
        assert!(env.wire.0.lock().unwrap().writes.is_empty(), "{change}");
        if change != "head" {
            let settles = packet::settle(&store, &item, &observation, NOW + 23).unwrap();
            assert!(settles.is_empty(), "{change}");
            assert!(
                store.load_actions().unwrap().finals[0]
                    .packet
                    .as_ref()
                    .unwrap()
                    .settled,
                "{change}"
            );
        }
        // The revoked decision sends the PR back through a normal review.
        assert!(
            store.load_pr_states().unwrap().state_seq(&binding) > parked_seq,
            "{change}"
        );
        if change != "head" {
            // The next scan queues it; CI alone never would.
            poll(&store, &state, NOW + 24);
            let work = crate::capacity::candidates(&store, NOW + 26).unwrap();
            assert_eq!(work.len(), 1, "{change}");
            assert!(work[0].reason.is_none(), "{change}: {:?}", work[0].reason);
        }
    }
}

#[test]
fn parked_lgtm_is_revoked_by_a_human_comment_seen_before_the_next_poll() {
    let (root, item, _) = parked_lgtm(true);
    let store = Store::new(root.path().into());
    let env = env(&store);
    let binding = Binding::job(&store.load_queue().unwrap()[0]);
    let parked_seq = store.load_pr_states().unwrap().state_seq(&binding);
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.author_id = server.observation.account_id.clone();
        // The owner, from the acting account itself, before CI turns green.
        let account = server.observation.account_id.clone();
        server
            .observation
            .comments
            .push(crate::provider::TopComment {
                id: "IC_hold".into(),
                body: "Please hold, I have a concern.".into(),
                author_id: Some(account),
                author_login: Some("actor".into()),
                created_at: "2026-10-01T00:00:00Z".into(),
                updated_at: "2026-10-01T00:00:00Z".into(),
            });
        server.observation.checks = Some("SUCCESS".into());
    }
    // No poll: only the action heartbeat observes the PR.
    let observation = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &item, Ok(observation.clone()), NOW + 20).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert!(host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .is_none());
    let ledger = store.load_actions().unwrap();
    let parked = ledger.finals.iter().find(|f| f.packet.is_some()).unwrap();
    assert_eq!(
        ready(&store, parked, &observation, Action::Merge),
        Err(packet::LGTM_REVOKED.into())
    );
    packet::settle(&store, &item, &observation, NOW + 21).unwrap();
    assert!(
        store.load_actions().unwrap().finals[0]
            .packet
            .as_ref()
            .unwrap()
            .settled
    );
    assert!(store.load_pr_states().unwrap().state_seq(&binding) > parked_seq);
    assert!(env.wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn parked_lgtm_revocation_waits_for_its_own_resolutions_to_be_consumed() {
    let (root, item, state) = parked_lgtm(true);
    let store = Store::new(root.path().into());
    let env = env(&store);
    let job = store.load_queue().unwrap()[0].clone();
    let binding = Binding::job(&job);
    let run = store.load_reviews().unwrap().last().unwrap().clone();
    // The sign-off's own resolutions are still being written.
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes.push(crate::exit_writes::ExitWrite {
        id: "sign-off-write".into(),
        review_operation_id: run.operation.id.clone(),
        job: run.job.clone(),
        assignment_id: run.assignment_id.clone(),
        comments_first: false,
        published: false,
        steps: vec![],
        phase: crate::exit_writes::Phase::Verified,
        operation: crate::monitoring::JobOperation::review(&run.job, NOW + 15),
        uncertain: false,
        error: None,
        consumed: false,
        reviewed_base_sha: None,
        sign_off: Some(crate::exits::LgtmAction::Merge),
    });
    store.save_feedback(&ledger).unwrap();
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.author_id = server.observation.account_id.clone();
        server.observation.checks = Some("FAILURE".into());
    }
    let red = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &item, Ok(red.clone()), NOW + 20).unwrap();
    let parked_seq = store.load_pr_states().unwrap().state_seq(&binding);
    let settled = |store: &Store| {
        store.load_actions().unwrap().finals[0]
            .packet
            .as_ref()
            .unwrap()
            .settled
    };
    for phase in ["running", "completed, not consumed"] {
        packet::settle(&store, &item, &red, NOW + 21).unwrap();
        assert!(!settled(&store), "{phase}");
        assert_eq!(
            store.load_pr_states().unwrap().state_seq(&binding),
            parked_seq,
            "{phase}"
        );
        let mut ledger = store.load_feedback().unwrap();
        ledger.exit_writes[0].phase = crate::exit_writes::Phase::Completed;
        store.save_feedback(&ledger).unwrap();
    }
    // The resolutions' consume baselines the state; only then does red CI revoke.
    let hash = store
        .load_pr_states()
        .unwrap()
        .get(&binding)
        .unwrap()
        .hash
        .clone()
        .unwrap();
    crate::pr_state::consume(&store, &binding, parked_seq, &hash).unwrap();
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes[0].consumed = true;
    store.save_feedback(&ledger).unwrap();
    packet::settle(&store, &item, &red, NOW + 22).unwrap();
    assert!(settled(&store));
    assert!(store.load_pr_states().unwrap().state_seq(&binding) > parked_seq);
    // An unchanged snapshot still queues the fresh review.
    poll(&store, &state, NOW + 24);
    let work = crate::capacity::candidates(&store, NOW + 26).unwrap();
    assert_eq!(work.len(), 1);
    assert!(work[0].reason.is_none(), "{:?}", work[0].reason);
    assert!(env.wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn parked_lgtm_in_flight_waits_out_red_ci_without_revocation_then_merges_once_green() {
    let (root, item, mut state) = parked_lgtm(false);
    let store = Store::new(root.path().into());
    let mut env = env(&store);
    env.wire.0.lock().unwrap().observation.review_decision = None;
    let binding = Binding::job(&store.load_queue().unwrap()[0]);
    let observe = |env: &Env<'_>, rollup: &str, now: i64| {
        env.wire.0.lock().unwrap().observation.checks = Some(rollup.into());
        let observation = env.wire.0.lock().unwrap().observation.clone();
        synchronize(&store, &item, Ok(observation.clone()), now).unwrap();
        packet::adopt(&store, &item).unwrap();
        observation
    };
    observe(&env, "SUCCESS", NOW + 20);
    let approval = host::prepare_next_effect(&store, &item, NOW + 20)
        .unwrap()
        .unwrap();
    assert_eq!(approval.action, Action::Approve);
    env.now = NOW + 20;
    host::execute_action(&mut env, &approval).unwrap();
    // Our own approval is a review-revision change: the next scan bumps the state.
    poll(&store, &state, NOW + 21);
    let seq = store.load_pr_states().unwrap().state_seq(&binding);
    // Red after the first effect: no revocation, no extra review; merge refused.
    state.checks[0].status = "COMPLETED".into();
    state.checks[0].conclusion = "FAILURE".into();
    poll(&store, &state, NOW + 22);
    let red = observe(&env, "FAILURE", NOW + 23);
    assert!(packet::settle(&store, &item, &red, NOW + 23)
        .unwrap()
        .is_empty());
    assert!(host::prepare_next_effect(&store, &item, NOW + 23)
        .unwrap()
        .is_none());
    assert_eq!(store.load_pr_states().unwrap().state_seq(&binding), seq);
    observe(&env, "SUCCESS", NOW + 24);
    let merge = host::prepare_next_effect(&store, &item, NOW + 24)
        .unwrap()
        .unwrap();
    assert_eq!(merge.action, Action::Merge);
    env.now = NOW + 24;
    host::execute_action(&mut env, &merge).unwrap();
    assert_eq!(env.wire.0.lock().unwrap().observation.state, "MERGED");
}

#[test]
fn parked_lgtm_revocation_fails_closed_when_only_the_state_bump_was_saved() {
    let (root, item, _) = parked_lgtm(true);
    let store = Store::new(root.path().into());
    let env = env(&store);
    let binding = Binding::job(&store.load_queue().unwrap()[0]);
    let mut states = store.load_pr_states().unwrap();
    let seq = states.state_seq(&binding);
    // The crash window: the revoke reached pr-state.json, the settled final did not.
    states.revoke(&binding, seq, NOW + 20);
    store.save_pr_states(&states).unwrap();
    {
        let mut server = env.wire.0.lock().unwrap();
        server.observation.author_id = server.observation.account_id.clone();
        server.observation.checks = Some("SUCCESS".into());
    }
    let observation = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &item, Ok(observation.clone()), NOW + 21).unwrap();
    assert!(host::prepare_next_effect(&store, &item, NOW + 21)
        .unwrap()
        .is_none());
    let ledger = store.load_actions().unwrap();
    let parked = ledger.finals.iter().find(|f| f.packet.is_some()).unwrap();
    assert_eq!(
        ready(&store, parked, &observation, Action::Merge),
        Err(packet::LGTM_REVOKED.into())
    );
    packet::settle(&store, &item, &observation, NOW + 22).unwrap();
    assert!(
        store.load_actions().unwrap().finals[0]
            .packet
            .as_ref()
            .unwrap()
            .settled
    );
    assert!(env.wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn wait_offered_beside_a_primarys_own_open_thread_is_accepted_at_exit() {
    use crate::github::actions::HumanThread;
    for approve in [true, false] {
        let (_root, store, _) = read_only_primary(approve);
        let state = snapshot(false, "");
        poll(&store, &state, NOW + 10);
        let env = env(&store);
        {
            let mut server = env.wire.0.lock().unwrap();
            server.observation.checks = Some("PENDING".into());
            server.observation.threads = vec![HumanThread {
                id: "own".into(),
                resolved: false,
                outdated: false,
                resolved_by: None,
                comments: vec![
                    json!({"fullDatabaseId":41,"body":"Finding.","path":"source.rs","line":1}),
                ],
            }];
        }
        let proof = |run: &ReviewRun| crate::github::threads::Ownership {
            publication_id: "publication".into(),
            configuration_id: "configuration".into(),
            account_id: run.job.account_id.clone(),
            repository_id: "100".into(),
            repository_name: "example/repo".into(),
            pull_request_id: run.job.pull_request_id.clone(),
            number: run.job.number,
            head_sha: "a".repeat(40),
            assignment_id: run.assignment_id.clone(),
            agent_id: "agent".into(),
            review_id: "review".into(),
            root_ids: vec!["41".into()],
            body_hashes: Default::default(),
        };
        let wait = ExitCall::Wait {
            reason: crate::exits::WaitReason::Ci,
        };
        let (_, prepared) =
            prepare_owned(&store, &env, &state, true, NOW + 12, |run| vec![proof(run)]);
        assert!(prepared.setup.offered.contains(&Exit::Wait), "{approve}");
        assert_eq!((prepared.setup.gate)(&wait), Ok(()), "{approve}");
        // Someone else's open thread: neither offered nor accepted.
        store.save_reviews(&[]).unwrap();
        let (_, prepared) = prepare(&store, &env, &state, true, NOW + 14);
        assert!(!prepared.setup.offered.contains(&Exit::Wait), "{approve}");
        assert!((prepared.setup.gate)(&wait).is_err(), "{approve}");
    }
}

#[test]
fn local_sign_off_is_tossed_when_ci_is_pending_again_at_the_final_read() {
    let (_root, store, _) = read_only_primary(false);
    let green = snapshot(false, "SUCCESS");
    poll(&store, &green, NOW + 10);
    let env = env(&store);
    let (run, _) = prepare(&store, &env, &green, true, NOW + 12);
    let finalize = |state: PrSnapshot| {
        let mut result = output();
        result.output = ReviewOutput::from_exit(
            sign_off(crate::exits::LgtmAction::Ready)
                .outcome()
                .unwrap()
                .0,
            None,
        );
        review_host::finalize_local_exit(
            &GithubClient::new(SnapshotWire {
                state,
                required: vec![],
            }),
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            run.job.number,
            run.packet.as_ref().unwrap(),
            &mut result,
        )
    };
    assert!(finalize(green.clone()).is_ok());
    // A check re-ran after the exit call: the review revision holds, CI does not.
    for rerun in [snapshot(false, ""), snapshot(false, "FAILURE")] {
        assert_eq!(
            finalize(rerun).unwrap_err().kind,
            crate::monitoring::OperationFailure::Tossed
        );
    }
}

#[test]
fn ready_sign_off_resolving_own_threads_retries_when_ci_is_pending_at_write_completion() {
    use crate::github::state::{SnapshotComment, SnapshotThread};
    let thread = |resolved| SnapshotThread {
        id: "own".into(),
        resolved,
        outdated: false,
        comments: vec![SnapshotComment {
            id: "41".into(),
            body: "Finding.".into(),
            commit: None,
        }],
        comment_total: 1,
    };
    let with = |conclusion: &str, resolved| PrSnapshot {
        threads: vec![thread(resolved)],
        thread_total: 1,
        ..snapshot(false, conclusion)
    };
    for (conclusion, retried) in [("SUCCESS", false), ("", true), ("FAILURE", true)] {
        let (_root, store, _) = read_only_primary(false);
        let read = with("SUCCESS", false);
        poll(&store, &read, NOW + 10);
        let env = env(&store);
        let (run, _) = prepare(&store, &env, &read, true, NOW + 12);
        let mut result = output();
        result.output = ReviewOutput::from_exit(
            crate::exits::ExitOutcome::Lgtm {
                comment: "Reviewed; CI green.".into(),
                resolve_own_threads: vec!["own".into()],
                action: crate::exits::LgtmAction::Ready,
                dismissed_teammate_findings: vec![],
            },
            None,
        );
        review_host::complete(&store, &run.operation.id, Ok(result), NOW + 13).unwrap();
        assert_eq!(crate::exit_writes::admit(&store, NOW + 14).unwrap(), 1);
        let mut ledger = store.load_feedback().unwrap();
        let write = &mut ledger.exit_writes[0];
        assert_eq!(write.sign_off, Some(crate::exits::LgtmAction::Ready));
        // Addressed threads resolve silently: no audit reply (no status chatter).
        assert!(write
            .steps
            .iter()
            .all(|step| step.kind == crate::exit_writes::StepKind::Resolve));
        for step in &mut write.steps {
            step.state = crate::exit_writes::StepState::Done;
        }
        write.phase = crate::exit_writes::Phase::Completed;
        write.operation.state = OperationState::Completed;
        write.consumed = true;
        store.save_feedback(&ledger).unwrap();
        // Only the own resolution and CI changed during the write window.
        let fresh = with(conclusion, true);
        crate::exits::consume::settle_fresh(&store, &run.job, &run.operation.id, &fresh).unwrap();
        let baseline = store
            .load_pr_states()
            .unwrap()
            .get(&Binding::job(&run.job))
            .unwrap()
            .baseline_hash
            .clone()
            .unwrap();
        let expected = if retried {
            run.packet.as_ref().unwrap().state_hash.clone().unwrap()
        } else {
            fresh.normalize().hash()
        };
        assert_eq!(baseline, expected, "{conclusion:?}");
        assert_ne!(read.normalize().hash(), fresh.normalize().hash());
        if retried {
            // The next heartbeat dispatches the primary again.
            poll(&store, &fresh, NOW + 30);
            prepare(&store, &env, &fresh, true, NOW + 32);
        }
    }
}

#[test]
fn published_comments_end_the_revision_so_ci_turning_green_never_reruns_it() {
    use crate::github::state::{SnapshotComment, SnapshotThread};
    let with = |conclusion: &str, resolved| PrSnapshot {
        threads: vec![SnapshotThread {
            id: "own".into(),
            resolved,
            outdated: false,
            comments: vec![SnapshotComment {
                id: "41".into(),
                body: "Finding.".into(),
                commit: None,
            }],
            comment_total: 1,
        }],
        thread_total: 1,
        ..snapshot(false, conclusion)
    };
    let (_root, store, _) = read_only_primary(false);
    let read = with("", false);
    poll(&store, &read, NOW + 10);
    let env = env(&store);
    let (run, _) = prepare(&store, &env, &read, true, NOW + 12);
    let mut result = output();
    result.output = ReviewOutput::from_exit(
        crate::exits::ExitOutcome::Comments {
            new_comments: vec![],
            body: None,
            reply_comments: vec![],
            resolve_own_threads: vec!["own".into()],
        },
        None,
    );
    review_host::complete(&store, &run.operation.id, Ok(result), NOW + 13).unwrap();
    assert_eq!(crate::exit_writes::admit(&store, NOW + 14).unwrap(), 1);
    let mut ledger = store.load_feedback().unwrap();
    let write = &mut ledger.exit_writes[0];
    for step in &mut write.steps {
        step.state = crate::exit_writes::StepState::Done;
    }
    write.phase = crate::exit_writes::Phase::Completed;
    write.operation.state = OperationState::Completed;
    write.consumed = true;
    store.save_feedback(&ledger).unwrap();
    // CI was still pending when the comments were written.
    let fresh = with("", true);
    crate::exits::consume::settle_fresh(&store, &run.job, &run.operation.id, &fresh).unwrap();
    let binding = Binding::job(&run.job);
    assert!(
        !store
            .load_pr_states()
            .unwrap()
            .get(&binding)
            .unwrap()
            .ci_wake
    );
    // CI turning green on the same revision queues nothing: no status chatter.
    env.wire.0.lock().unwrap().observation.checks = Some("SUCCESS".into());
    poll(&store, &with("SUCCESS", true), NOW + 30);
    assert!(crate::capacity::candidates(&store, NOW + 31)
        .unwrap()
        .is_empty());
}

struct SnapshotWire {
    state: PrSnapshot,
    required: Vec<String>,
}

impl Transport for SnapshotWire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        panic!("Unexpected non-snapshot read {path}")
    }
}

impl QueryTransport for SnapshotWire {
    fn query(&self, query: &str, _: Value) -> Result<Response, ConnectionError> {
        assert!(query.contains("fragment S"));
        assert!(query.contains("requiredStatusCheckContexts"));
        let s = &self.state;
        let pull = json!({
            "fullDatabaseId":s.pull_request_id,"number":s.number,"title":s.title,"body":s.body,
            "state":s.state,"isDraft":s.draft,"mergeable":s.mergeable,"reviewDecision":s.review_decision,
            "baseRefOid":s.base,"headRefOid":s.head,"headRefName":s.head_ref,
            "baseRef":{"branchProtectionRule":{"requiresStatusChecks":!self.required.is_empty(),
                "requiredStatusCheckContexts":self.required}},
            "labels":{"nodes":s.labels.iter().map(|n|json!({"name":n})).collect::<Vec<_>>()},
            "latestOpinionatedReviews":{"nodes":[]},
            "comments":{"totalCount":s.comments.len(),"nodes":s.comments.iter().map(|c|
                json!({"fullDatabaseId":c.id,"body":c.body})).collect::<Vec<_>>()},
            "commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{
                "totalCount":s.checks.len(),"nodes":s.checks.iter().map(|c|json!({
                    "__typename":"CheckRun","name":c.name,"status":c.status,"conclusion":c.conclusion
                })).collect::<Vec<_>>()
            }}}}]},
            "reviewThreads":{"totalCount":0,"nodes":[]}
        });
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&json!({"data":{"repository":{
                "databaseId":100,(format!("p{}",s.number)):pull
            }}}))
            .unwrap(),
        })
    }
}

fn final_wait_read(
    run: &ReviewRun,
    state: PrSnapshot,
    required: Vec<String>,
) -> Result<crate::review::ReviewResult, crate::review::Failure> {
    let client = GithubClient::new(SnapshotWire { state, required });
    let mut result = output();
    result.output = ReviewOutput::from_exit(
        crate::exits::ExitOutcome::Wait {
            reason: crate::exits::WaitReason::Ci,
        },
        None,
    );
    review_host::finalize_local_exit(
        &client,
        &RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        },
        run.job.number,
        run.packet.as_ref().unwrap(),
        &mut result,
    )?;
    Ok(result)
}

#[test]
fn review_fix_ci_progress_survives_the_production_final_read_and_an_intervening_poll() {
    for concurrent_poll in [false, true] {
        let (_root, store, _) = fixture(1, true, true);
        store.save_reviews(&[]).unwrap();
        let state = snapshot(false, "");
        poll(&store, &state, NOW + 10);
        let env = env(&store);
        env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
        let (run, _) = prepare(&store, &env, &state, true, NOW + 12);
        let original_hash = run.packet.as_ref().unwrap().state_hash.clone();
        let mut progressed = state;
        progressed.checks[0].status = "COMPLETED".into();
        progressed.checks[0].conclusion = "SUCCESS".into();
        progressed.checks.push(SnapshotCheck {
            name: "integration".into(),
            status: "IN_PROGRESS".into(),
            conclusion: "".into(),
        });
        progressed.check_total = 2;
        if concurrent_poll {
            poll(&store, &progressed, NOW + 14);
        }
        let result = final_wait_read(&run, progressed.clone(), vec![])
            .expect("pending-only progress during inference must not toss the Wait");
        review_host::complete(&store, &run.operation.id, Ok(result), NOW + 16).unwrap();
        let saved = store.load_reviews().unwrap();
        let saved = saved
            .iter()
            .find(|r| r.operation.id == run.operation.id)
            .unwrap();
        assert_eq!(saved.operation.state, OperationState::Completed);
        assert_eq!(
            saved.packet.as_ref().unwrap().state_hash,
            original_hash,
            "packet evidence is immutable"
        );
        let states = store.load_pr_states().unwrap();
        let record = states.get(&Binding::job(&run.job)).unwrap();
        assert_eq!(record.error_count, 0);
        assert_eq!(
            record.external_wait.as_ref().unwrap().snapshot,
            progressed.normalize()
        );
        assert!(record.exit.is_none());
        poll(&store, &progressed, NOW + 18);
        assert!(crate::capacity::candidates(&store, NOW + 20)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn review_fix_missing_required_ci_waits_without_reported_pending_checks() {
    for (no_reported_checks, conclusion) in [
        (false, "SUCCESS"),
        (false, "FAILURE"),
        (true, "SUCCESS"),
        (true, "FAILURE"),
    ] {
        let (_root, store, _) = fixture(1, true, true);
        store.save_reviews(&[]).unwrap();
        let mut state = snapshot(false, "SUCCESS");
        if no_reported_checks {
            state.checks.clear();
            state.check_total = 0;
        }
        let required = vec!["required-build".to_string()];
        let client = GithubClient::new(SnapshotWire {
            state,
            required: required.clone(),
        });
        let state = client
            .pr_snapshot(
                &RemoteRepository {
                    id: "100".into(),
                    name: "example/repo".into(),
                },
                1,
            )
            .unwrap();
        poll(&store, &state, NOW + 10);
        let env = env(&store);
        {
            let mut server = env.wire.0.lock().unwrap();
            if no_reported_checks {
                server.observation.checks = None;
            }
            server.observation.protection = json!({
                "requiresStatusChecks":true,"requiredStatusCheckContexts":required,
                "requiresApprovingReviews":false,"requiredApprovingReviewCount":0,
                "requiresCodeOwnerReviews":false,"requiresConversationResolution":false,
                "requiresStrictStatusChecks":false
            });
        }
        let (run, prepared) = prepare(&store, &env, &state, true, NOW + 12);
        assert!(prepared.setup.offered.contains(&Exit::Wait));
        assert_eq!(
            (prepared.setup.gate)(&ExitCall::Wait {
                reason: crate::exits::WaitReason::Ci
            }),
            Ok(())
        );
        let result = final_wait_read(&run, state.clone(), required.clone()).unwrap();
        review_host::complete(&store, &run.operation.id, Ok(result), NOW + 14).unwrap();
        assert_eq!(
            store
                .load_reviews()
                .unwrap()
                .last()
                .unwrap()
                .operation
                .state,
            OperationState::Completed,
            "an offered missing-required-check Wait must not become a counted toss"
        );
        let restored = Store::new(_root.path().into()).load_pr_states().unwrap();
        assert_eq!(
            restored
                .get(&Binding::job(&run.job))
                .unwrap()
                .external_wait
                .as_ref()
                .unwrap()
                .snapshot
                .required_checks,
            ["required-build"]
        );
        poll(&store, &state, NOW + 16);
        assert!(crate::capacity::candidates(&store, NOW + 18)
            .unwrap()
            .is_empty());
        let mut reported = state;
        reported.checks.push(SnapshotCheck {
            name: "required-build".into(),
            status: "IN_PROGRESS".into(),
            conclusion: "".into(),
        });
        reported.check_total += 1;
        poll(&store, &reported, NOW + 20);
        assert!(crate::capacity::candidates(&store, NOW + 22)
            .unwrap()
            .is_empty());
        let check = reported.checks.last_mut().unwrap();
        check.status = "COMPLETED".into();
        check.conclusion = conclusion.into();
        poll(&store, &reported, NOW + 24);
        poll(&store, &reported, NOW + 26);
        assert_eq!(
            crate::capacity::candidates(&store, NOW + 28).unwrap().len(),
            1
        );
        assert!(store
            .load_pr_states()
            .unwrap()
            .get(&Binding::job(&run.job))
            .unwrap()
            .exit
            .is_none());
    }
}

#[test]
fn review_fix_final_wait_still_rejects_non_ci_changes_and_completed_or_failed_checks() {
    let (_root, store, _) = fixture(1, true, true);
    store.save_reviews(&[]).unwrap();
    let state = snapshot(false, "");
    poll(&store, &state, NOW + 10);
    let env = env(&store);
    env.wire.0.lock().unwrap().observation.checks = Some("PENDING".into());
    let (run, _) = prepare(&store, &env, &state, true, NOW + 12);
    for change in [
        "head",
        "base",
        "body",
        "comment",
        "failure",
        "green",
        "required_checks",
    ] {
        let mut fresh = state.clone();
        let mut required = vec![];
        match change {
            "head" => fresh.head = "d".repeat(40),
            "base" => fresh.base = "d".repeat(40),
            "body" => fresh.body = "New review context".into(),
            "comment" => fresh.comments.push(crate::github::state::SnapshotComment {
                id: "new".into(),
                body: "New concern".into(),
                commit: None,
            }),
            "label" => fresh.labels.push("changed".into()),
            "required_checks" => required.push("new-required-job".into()),
            _ => {
                fresh.checks[0].status = "COMPLETED".into();
                fresh.checks[0].conclusion = if change == "failure" {
                    "FAILURE"
                } else {
                    "SUCCESS"
                }
                .into();
            }
        }
        assert_eq!(
            final_wait_read(&run, fresh, required).unwrap_err().kind,
            crate::monitoring::OperationFailure::Tossed,
            "{change}"
        );
    }
    let mut labelled = state.clone();
    labelled.labels.push("changed".into());
    labelled.mergeable = "CONFLICTING".into();
    assert!(
        final_wait_read(&run, labelled, vec![]).is_ok(),
        "labels and mergeability are not review changes"
    );
    let mut result = output();
    result.output = ReviewOutput::from_exit(
        crate::exits::ExitOutcome::Wait {
            reason: crate::exits::WaitReason::Ci,
        },
        None,
    );
    result.finding_dispositions = serde_json::from_value(json!([{
        "thread_id":"existing","disposition":"still_applies","head_sha":state.head,
        "base_sha":state.base,"rationale":"Known concern remains.","evidence":[]
    }]))
    .unwrap();
    let client = GithubClient::new(SnapshotWire {
        state,
        required: vec![],
    });
    assert!(review_host::finalize_local_exit(
        &client,
        &RemoteRepository {
            id: "100".into(),
            name: "example/repo".into()
        },
        1,
        run.packet.as_ref().unwrap(),
        &mut result
    )
    .is_err());
    assert!(result.wait_snapshot.is_none());
    assert!(store
        .load_pr_states()
        .unwrap()
        .get(&Binding::job(&run.job))
        .unwrap()
        .external_wait
        .is_none());
}

fn prepare(
    store: &Store,
    env: &Env<'_>,
    state: &PrSnapshot,
    primary: bool,
    now: i64,
) -> (ReviewRun, packet_run::Prepared) {
    prepare_owned(store, env, state, primary, now, |_| vec![])
}

fn prepare_owned(
    store: &Store,
    env: &Env<'_>,
    state: &PrSnapshot,
    primary: bool,
    now: i64,
    ownership: impl FnOnce(&ReviewRun) -> Vec<crate::github::threads::Ownership>,
) -> (ReviewRun, packet_run::Prepared) {
    let assignment = store.load_settings().unwrap().repositories[0].assignments
        [usize::from(!primary)]
    .id
    .clone();
    let candidate = review_host::candidates(store)
        .unwrap()
        .into_iter()
        .rev()
        .find(|c| c.assignment_id == assignment)
        .unwrap();
    let work = crate::capacity::candidates(store, now)
        .unwrap()
        .into_iter()
        .find(|w| w.key.id == candidate.key)
        .expect("heartbeat must dispatch a new review, not reuse the old exit");
    assert!(work.reason.is_none(), "{:?}", work.reason);
    let mut run = review_host::prepare_dispatch(store, &candidate.key, now)
        .unwrap()
        .unwrap();
    let mut context = context(state);
    if env.wire.0.lock().unwrap().observation.author_id == run.job.account_id {
        context.pull.author = Some(Identity {
            id: run.job.account_id.clone(),
            login: "actor".into(),
        });
    }
    let prepared = packet_run::prepare(
        &(Arc::new(GithubClient::new(env.wire.clone()))
            as Arc<dyn crate::provider::ProviderReads + Send + Sync>),
        &RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        },
        &run,
        &context,
        state,
        packet_run::Caller {
            resolutions: Vec::new(),
            receipts: Default::default(),
            review_again: false,
            acting_login: "actor",
            write_capability: true,
            teammates: crate::review::teammates::entries(store, &run).unwrap(),
            ownership: ownership(&run),
            iterations: &[],
            last_consumed_at: None,
        },
    )
    .unwrap();
    run.packet = Some(prepared.packet.record.clone());
    let mut reviews = store.load_reviews().unwrap();
    *reviews
        .iter_mut()
        .find(|r| r.operation.id == run.operation.id)
        .unwrap() = run.clone();
    store.save_reviews(&reviews).unwrap();
    (run, prepared)
}

fn complete(
    store: &Store,
    run: &ReviewRun,
    prepared: &packet_run::Prepared,
    call: ExitCall,
    now: i64,
) {
    assert_eq!((prepared.setup.gate)(&call), Ok(()));
    let (exit, report) = call.outcome().unwrap();
    let mut result = output();
    result.output = ReviewOutput::from_exit(exit.clone(), report);
    review_host::complete(store, &run.operation.id, Ok(result), now).unwrap();
    let saved = store.load_reviews().unwrap();
    let evidence = saved
        .iter()
        .find(|r| r.operation.id == run.operation.id)
        .unwrap();
    assert_eq!(evidence.operation.state, OperationState::Completed);
    assert_eq!(evidence.packet, run.packet);
    assert_eq!(evidence.result.as_ref().unwrap().output.exit, Some(exit));
}

#[test]
fn readiness_include_self_reaches_the_agent_and_merges_without_an_approval_vote() {
    for include_self in [true, false] {
        let (_root, store, item) = fixture(1, true, true);
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0]
            .actions
            .as_mut()
            .unwrap()
            .include_self = include_self;
        store.save_settings(&settings).unwrap();
        store.save_reviews(&[]).unwrap();
        let state = snapshot(false, "SUCCESS");
        poll(&store, &state, NOW + 10);
        let mut env = env(&store);
        {
            let mut server = env.wire.0.lock().unwrap();
            server.observation.author_id = server.observation.account_id.clone();
        }
        let (run, prepared) = prepare(&store, &env, &state, true, NOW + 12);
        assert!(prepared
            .packet
            .text
            .contains(&format!("include_self: {include_self}")));
        assert!(!prepared.setup.offered.contains(&Exit::ApprovePullRequest));
        if !include_self {
            assert!(!prepared.setup.offered.contains(&Exit::ApproveAndMerge));
            assert!(!prepared.setup.offered.contains(&Exit::Lgtm));
            assert!(prepared.packet.text.contains("Include self is off."));
            assert!(!prepared
                .packet
                .text
                .contains("clean_review_exit: approve_and_merge"));
            assert!(!prepared.packet.text.contains("clean_review_exit: lgtm"));
            assert!(env.wire.0.lock().unwrap().writes.is_empty());
            continue;
        }
        assert!(prepared.setup.offered.contains(&Exit::ApproveAndMerge));
        assert!(prepared.setup.offered.contains(&Exit::Lgtm));
        assert!(prepared.packet.text.contains("clean_review_exit: lgtm"));
        assert!(prepared
            .packet
            .text
            .contains("merges without an approval vote"));
        complete(
            &store,
            &run,
            &prepared,
            ExitCall::ApproveAndMerge {
                comment: "Current changes reviewed; configured own-PR merge permitted.".into(),
            },
            NOW + 13,
        );
        synchronize(
            &store,
            &item,
            Ok(env.wire.0.lock().unwrap().observation.clone()),
            NOW + 14,
        )
        .unwrap();
        packet::adopt(&store, &item).unwrap();
        let effect = host::prepare_next_effect(&store, &item, NOW + 15)
            .unwrap()
            .unwrap();
        assert_eq!(effect.action, Action::Merge);
        env.now = NOW + 15;
        host::execute_action(&mut env, &effect).unwrap();
        let server = env.wire.0.lock().unwrap();
        assert_eq!(server.observation.state, "MERGED");
        assert_eq!(server.writes.len(), 1);
        assert!(server.observation.reviews.is_empty());
        assert!(
            store
                .load_actions()
                .unwrap()
                .effects
                .iter()
                .all(|effect| effect.action == Action::Merge
                    && effect.state == EffectState::Confirmed)
        );
    }
}

#[test]
fn readiness_draft_then_ready_waits_then_green_wakes_the_primary_and_merges() {
    let (_root, store, item) = fixture(2, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].review_drafts = true;
    store.save_settings(&settings).unwrap();
    store.save_reviews(&[]).unwrap();
    let mut env = env(&store);
    for (i, (draft, conclusion, rollup, blocker)) in [
        (true, "", "PENDING", Some("PR is draft. Do not approve.")),
        (
            false,
            "",
            "PENDING",
            Some("CI is pending. Cannot merge yet."),
        ),
        (false, "SUCCESS", "SUCCESS", None),
    ]
    .into_iter()
    .enumerate()
    {
        let now = NOW + 10 + i as i64 * 20;
        let state = snapshot(draft, conclusion);
        {
            let mut server = env.wire.0.lock().unwrap();
            server.observation.draft = draft;
            server.observation.checks = Some(rollup.into());
        }
        poll(&store, &state, now);
        let binding = Binding::job(&store.load_queue().unwrap()[0]);
        let seq = store.load_pr_states().unwrap().state_seq(&binding);
        assert_eq!(seq, i as u64 + 1);
        let work = crate::capacity::candidates(&store, now + 2).unwrap();
        // Green CI wakes the primary's CI wait on the same review revision:
        // the teammate's report for it still holds.
        let woken = conclusion == "SUCCESS";
        assert_eq!(
            work.len(),
            if woken { 1 } else { 2 },
            "a changed review state needs both assignments"
        );
        assert_eq!(
            work.iter().filter(|w| w.blocked_by.is_some()).count(),
            usize::from(!woken),
            "the prior state's report must not bypass the primary barrier"
        );
        if !woken {
            let (teammate, prepared) = prepare(&store, &env, &state, false, now + 2);
            complete(
                &store,
                &teammate,
                &prepared,
                ExitCall::SubmitReport {
                    report: crate::exits::TeammateReport { findings: vec![] },
                },
                now + 3,
            );
        }
        let (primary, prepared) = prepare(&store, &env, &state, true, now + 4);
        assert_eq!(primary.job.work.as_ref().unwrap().state_seq, seq);
        assert!(prepared.packet.text.contains("Agent 2"));
        let chosen = if let Some(blocker) = blocker {
            assert!(!prepared.setup.offered.contains(&Exit::ApproveAndMerge));
            assert!(prepared
                .packet
                .record
                .exits_denied
                .iter()
                .any(|denial| denial.exit == Exit::ApproveAndMerge && denial.reason == blocker));
            if draft {
                ExitCall::ReadyForHuman {
                    summary: "No code concerns; merge gate not yet satisfied.".into(),
                }
            } else {
                // CI alone is no new state: the primary holds on it instead.
                assert!(prepared.setup.offered.contains(&Exit::Wait));
                ExitCall::Wait {
                    reason: crate::exits::WaitReason::Ci,
                }
            }
        } else {
            assert!(prepared.setup.offered.contains(&Exit::ApproveAndMerge));
            ExitCall::ApproveAndMerge {
                comment: "Reviewed the current diff and teammate report.".into(),
            }
        };
        complete(&store, &primary, &prepared, chosen, now + 5);
        synchronize(
            &store,
            &item,
            Ok(env.wire.0.lock().unwrap().observation.clone()),
            now + 6,
        )
        .unwrap();
        packet::adopt(&store, &item).unwrap();
        if blocker.is_some() {
            assert!(host::prepare_next_effect(&store, &item, now + 7)
                .unwrap()
                .is_none());
            poll(&store, &state, now + 8);
            assert_eq!(store.load_pr_states().unwrap().state_seq(&binding), seq);
            assert!(crate::capacity::candidates(&store, now + 9)
                .unwrap()
                .is_empty());
        }
    }
    let approve = host::prepare_next_effect(&store, &item, NOW + 80)
        .unwrap()
        .unwrap();
    assert_eq!(approve.action, Action::Approve);
    env.now = NOW + 80;
    host::execute_action(&mut env, &approve).unwrap();
    synchronize(
        &store,
        &item,
        Ok(env.wire.0.lock().unwrap().observation.clone()),
        NOW + 81,
    )
    .unwrap();
    let merge = host::prepare_next_effect(&store, &item, NOW + 82)
        .unwrap()
        .unwrap();
    assert_eq!(merge.action, Action::Merge);
    env.now = NOW + 82;
    host::execute_action(&mut env, &merge).unwrap();
    let server = env.wire.0.lock().unwrap();
    assert_eq!(server.observation.state, "MERGED");
    assert_eq!(server.writes.len(), 2);
    assert!(store
        .load_actions()
        .unwrap()
        .effects
        .iter()
        .all(|e| e.state == EffectState::Confirmed));
    assert!(packet::settle(&store, &item, &server.observation, NOW + 83)
        .unwrap()
        .is_empty());
    assert!(!packet::open(&store.load_actions().unwrap(), &item));
}

#[test]
fn readiness_ci_green_after_ready_for_human_wakes_only_the_primary_once() {
    for (rollup, conclusion) in [(None, None), (Some("FAILURE"), Some("FAILURE"))] {
        let (_root, store, _item) = fixture(2, true, true);
        store.save_reviews(&[]).unwrap();
        let env = env(&store);
        let mut state = snapshot(false, conclusion.unwrap_or_default());
        if conclusion.is_none() {
            // Right after a push GitHub may report no check rollup at all.
            state.checks.clear();
            state.check_total = 0;
        }
        env.wire.0.lock().unwrap().observation.checks = rollup.map(String::from);
        poll(&store, &state, NOW + 10);
        let binding = Binding::job(&store.load_queue().unwrap()[0]);
        let (teammate, prepared) = prepare(&store, &env, &state, false, NOW + 12);
        complete(
            &store,
            &teammate,
            &prepared,
            ExitCall::SubmitReport {
                report: crate::exits::TeammateReport { findings: vec![] },
            },
            NOW + 13,
        );
        let (primary, prepared) = prepare(&store, &env, &state, true, NOW + 14);
        assert!(!prepared.setup.offered.contains(&Exit::Wait), "{rollup:?}");
        complete(
            &store,
            &primary,
            &prepared,
            ExitCall::ReadyForHuman {
                summary: "CI not verified green.".into(),
            },
            NOW + 15,
        );
        poll(&store, &state, NOW + 16);
        assert!(crate::capacity::candidates(&store, NOW + 17)
            .unwrap()
            .is_empty());

        let green = snapshot(false, "SUCCESS");
        env.wire.0.lock().unwrap().observation.checks = Some("SUCCESS".into());
        poll(&store, &green, NOW + 20);
        let work = crate::capacity::candidates(&store, NOW + 21).unwrap();
        assert_eq!(work.len(), 1, "{rollup:?}: the primary only");
        assert!(work[0].blocked_by.is_none(), "teammate report holds");
        let (primary, prepared) = prepare(&store, &env, &green, true, NOW + 22);
        assert_eq!(primary.job.work.as_ref().unwrap().state_seq, 2);
        assert!(prepared.setup.offered.contains(&Exit::Lgtm));
        complete(
            &store,
            &primary,
            &prepared,
            ExitCall::ReadyForHuman {
                summary: "Human judgment needed.".into(),
            },
            NOW + 23,
        );
        poll(&store, &green, NOW + 24);
        assert_eq!(store.load_pr_states().unwrap().state_seq(&binding), 2);
        assert!(crate::capacity::candidates(&store, NOW + 25)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn readiness_green_does_not_override_the_primarys_ready_for_human_exit() {
    let (_root, store, item) = fixture(1, true, true);
    store.save_reviews(&[]).unwrap();
    let state = snapshot(false, "SUCCESS");
    poll(&store, &state, NOW + 10);
    let env = env(&store);
    let (primary, prepared) = prepare(&store, &env, &state, true, NOW + 12);
    assert!(prepared.setup.offered.contains(&Exit::ApproveAndMerge));
    complete(
        &store,
        &primary,
        &prepared,
        ExitCall::ReadyForHuman {
            summary: "Human judgment needed.".into(),
        },
        NOW + 13,
    );
    synchronize(&store, &item, Ok(observed()), NOW + 14).unwrap();
    packet::adopt(&store, &item).unwrap();
    assert!(host::prepare_next_effect(&store, &item, NOW + 15)
        .unwrap()
        .is_none());
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(env.wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn readiness_green_packet_never_authorizes_a_stale_or_newly_blocked_merge() {
    for blocker in [
        "draft",
        "head",
        "base",
        "red",
        "thread",
        "queue",
        "permission",
    ] {
        let (_root, store, item) = fixture(1, true, true);
        store.save_reviews(&[]).unwrap();
        let state = snapshot(false, "SUCCESS");
        poll(&store, &state, NOW + 10);
        let mut env = env(&store);
        env.wire
            .0
            .lock()
            .unwrap()
            .observation
            .reviews
            .push(super::fixwave::peer_approval());
        let (primary, prepared) = prepare(&store, &env, &state, true, NOW + 12);
        let call = ExitCall::ApproveAndMerge {
            comment: "Reviewed this exact head and base.".into(),
        };
        assert!(prepared.setup.offered.contains(&Exit::ApproveAndMerge));
        complete(&store, &primary, &prepared, call.clone(), NOW + 13);
        synchronize(
            &store,
            &item,
            Ok(env.wire.0.lock().unwrap().observation.clone()),
            NOW + 14,
        )
        .unwrap();
        packet::adopt(&store, &item).unwrap();
        let effect = host::prepare_next_effect(&store, &item, NOW + 15)
            .unwrap()
            .unwrap();
        assert_eq!(effect.action, Action::Merge);
        {
            let mut server = env.wire.0.lock().unwrap();
            let o = &mut server.observation;
            match blocker {
                "draft" => o.draft = true,
                "head" => o.head = "d".repeat(40),
                "base" => o.base = "d".repeat(40),
                "red" => o.checks = Some("FAILURE".into()),
                "thread" => o.threads.push(HumanThread {
                    id: "unresolved".into(),
                    resolved: false,
                    outdated: true,
                    resolved_by: None,
                    comments: vec![],
                }),
                "queue" => o.in_merge_queue = true,
                "permission" => o.permission = "READ".into(),
                _ => unreachable!(),
            }
        }
        assert!(
            matches!(
                (prepared.setup.gate)(&call),
                Err(Refusal::Moved(_) | Refusal::Denied(_))
            ),
            "{blocker}: exit must re-read, not trust the offered exit"
        );
        assert!(
            host::execute_action(&mut env, &effect).is_err(),
            "{blocker}: native execution must re-read even an accepted exit"
        );
        assert!(env.wire.0.lock().unwrap().writes.is_empty(), "{blocker}");
        assert!(store
            .load_actions()
            .unwrap()
            .effects
            .iter()
            .all(|e| e.receipt.is_none()));
    }
}
