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
    assert!(prepared.packet.text.contains("clean_review_exit: wait"));
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
fn wait_exit_rechecks_ci_and_disabled_tool_never_grants_wait() {
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
        assert_eq!(prepared.setup.offered.contains(&Exit::Wait), !custom);
        let call = ExitCall::Wait {
            reason: crate::exits::WaitReason::Ci,
        };
        if custom {
            assert!(matches!(
                (prepared.setup.gate)(&call),
                Err(Refusal::Denied(_))
            ));
            assert!(
                !store.load_settings().unwrap().repositories[0].assignments[0]
                    .tools
                    .as_ref()
                    .unwrap()
                    .contains(&"wait".into())
            );
        } else {
            for rollup in ["SUCCESS", "FAILURE"] {
                env.wire.0.lock().unwrap().observation.checks = Some(rollup.into());
                assert!(matches!(
                    (prepared.setup.gate)(&call),
                    Err(Refusal::Denied(_))
                ));
            }
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

fn prepare(
    store: &Store,
    env: &Env<'_>,
    state: &PrSnapshot,
    primary: bool,
    now: i64,
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
    let mut run = review_host::prepare_dispatch(store, &candidate.key, now).unwrap();
    let mut context = context(state);
    if env.wire.0.lock().unwrap().observation.author_id == run.job.account_id {
        context.pull.author = Some(Identity {
            id: run.job.account_id.clone(),
            login: "actor".into(),
        });
    }
    let prepared = packet_run::prepare(
        &Arc::new(GithubClient::new(env.wire.clone())),
        &RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        },
        &run,
        &context,
        state,
        packet_run::Caller {
            acting_login: "actor",
            write_capability: true,
            teammates: crate::review::teammates::entries(store, &run).unwrap(),
            ownership: vec![],
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
            assert!(prepared.packet.text.contains("Include self is off."));
            assert!(!prepared
                .packet
                .text
                .contains("clean_review_exit: approve_and_merge"));
            assert!(env.wire.0.lock().unwrap().writes.is_empty());
            continue;
        }
        assert!(prepared.setup.offered.contains(&Exit::ApproveAndMerge));
        assert!(prepared
            .packet
            .text
            .contains("clean_review_exit: approve_and_merge"));
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
fn readiness_draft_then_ready_then_green_dispatches_and_executes_the_chosen_merge() {
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
        assert_eq!(work.len(), 2, "each changed state needs both assignments");
        assert_eq!(
            work.iter().filter(|w| w.blocked_by.is_some()).count(),
            1,
            "the prior state's report must not bypass the primary barrier"
        );
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
            ExitCall::ReadyForHuman {
                summary: "No code concerns; merge gate not yet satisfied.".into(),
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
