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
            state: Lifecycle::Open,
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
    let prepared = packet_run::prepare(
        &Arc::new(GithubClient::new(env.wire.clone())),
        &RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        },
        &run,
        &context(state),
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
