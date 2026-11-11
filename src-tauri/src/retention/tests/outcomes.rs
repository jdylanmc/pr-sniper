use super::*;
use crate::{
    actions::{Basis, Effect, FinalReview, Ledger, PacketApproval, Settlement},
    exit_writes::{Phase as WritePhase, StepState},
    exits::{ExitOutcome, LgtmAction, WaitReason},
    review::outcome::{Evidence, Outcome},
};

fn with_exit(store: &Store, job: &QueueJob, exit: ExitOutcome) -> ReviewRun {
    let mut run = completed(store, job);
    run.operation.id = format!("run-{}", job.pull_request_id);
    run.result.as_mut().unwrap().output.exit = Some(exit);
    run
}

fn comments() -> ExitOutcome {
    serde_json::from_value(json!({
        "exit":"comments","new_comments":[],"reply_comments":[],"resolve_own_threads":[]
    }))
    .unwrap()
}

fn effect(final_id: &str, item_id: &str, action: &str, state: &str) -> Effect {
    let head = "a".repeat(40);
    serde_json::from_value(json!({
        "id":format!("{final_id}-{action}"),"final_id":final_id,"item_id":item_id,"action":action,
        "operation":{
            "id":format!("{final_id}-{action}-operation"),"provider":"github","account_id":"22",
            "configuration_id":REPO,"repository_id":"100","pull_request_id":"1",
            "head_sha":head,"trigger_policy":"policy","operation_type":"github_approve",
            "state":"completed","attempt_count":1,"initial_attempt_at":NOW,
            "retry_deadline":NOW,"next_attempt_at":null,"failure":null,
            "attempted_mutation":null,"pending_review_id":null,"owned_thread_id":null,
            "triggering_external_comment_id":null,"confirmed_receipt":null
        },
        "observation":{
            "write_capability":true,"node_id":"node","repository_id":"100",
            "pull_request_id":"1","account_id":"22","author_id":"11",
            "head_repository_id":null,"head":head,"base":"b".repeat(40),
            "base_name":"main","merge_rules":null,"merge_rules_error":null,
            "state":"OPEN","draft":false,"permission":"WRITE",
            "mergeable":"MERGEABLE","merge_state":"CLEAN","review_decision":null,
            "checks":null,"check_contexts":[],"in_merge_queue":false,"method":null,
            "protection":null,"threads":[],"reviews":[],"comments":[],
            "merged_by":null,"merged_at":null,"merge_commit":null
        },
        "body":"","state":state,"error":null,"receipt":null
    }))
    .unwrap()
}

/// The packet final an approve exit creates, and the effects it cast.
fn approval(store: &Store, run: &ReviewRun, effects: &[(&str, &str)]) -> Ledger {
    let item_id = crate::queue::item_id(&run.job);
    let id = format!("final-packet-{}", run.operation.id);
    let effects: Vec<_> = effects
        .iter()
        .map(|(action, state)| effect(&id, &item_id, action, state))
        .collect();
    let final_review = FinalReview {
        id,
        enqueue_order: 1,
        basis: Basis {
            activation_version: "fixture".into(),
            item_id: item_id.clone(),
            iteration_id: item_id,
            repository: store.load_settings().unwrap().repositories[0].clone(),
            job: run.job.clone(),
            selection: (*run.selection).clone(),
            peers: vec![],
            feedback: vec![],
            conversations: vec![],
            permissions: Default::default(),
            trust_confirmed: false,
        },
        observation: effect("x", "x", "approve", "confirmed").observation,
        execution: run.clone(),
        cancelled: false,
        attempts: vec![],
        packet: Some(PacketApproval {
            run_id: run.operation.id.clone(),
            state_seq: 1,
            comment: "Looks right.".into(),
            merge: false,
            settled: false,
            merge_pending: None,
            settlement: None,
        }),
    };
    Ledger {
        finals: vec![final_review],
        effects,
        observations: vec![],
    }
}

#[test]
fn each_exit_maps_to_the_outcome_it_completed_and_confirmed() {
    let (_root, store, _monitor) = fixture();
    let job = store.load_queue().unwrap()[0].clone();
    let none = Ledger::default();
    // Every run here shares one operation id; it still awaits adoption.
    let adoptable =
        std::collections::HashSet::from([with_exit(&store, &job, ExitOutcome::SubmitReport)
            .operation
            .id]);
    let plain = Evidence {
        publications: &[],
        exit_writes: &[],
        actions: &none,
        adoptable: &adoptable,
    };
    let outcome = |exit: ExitOutcome| plain.run(&with_exit(&store, &job, exit), false);
    assert_eq!(
        outcome(ExitOutcome::Wait {
            reason: WaitReason::Ci
        }),
        Some(Outcome::Waiting)
    );
    assert_eq!(
        outcome(ExitOutcome::ReadyForHuman {
            summary: "Over to you.".into()
        }),
        Some(Outcome::ReadyForHuman)
    );
    assert_eq!(outcome(ExitOutcome::SubmitReport), Some(Outcome::Report));
    assert_eq!(
        outcome(ExitOutcome::Lgtm {
            comment: String::new(),
            resolve_own_threads: vec![],
            action: LgtmAction::Ready,
            dismissed_teammate_findings: vec![],
        }),
        Some(Outcome::ReadyForHuman)
    );
    // An approve exit is not an approval until its vote is confirmed.
    let approve = with_exit(
        &store,
        &job,
        ExitOutcome::Approve {
            comment: "Looks right.".into(),
        },
    );
    assert_eq!(plain.run(&approve, false), Some(Outcome::Pending));
    // Never adoptable (state moved or PR closed first): it can never act.
    let gone = std::collections::HashSet::new();
    let unadoptable = Evidence {
        publications: &[],
        exit_writes: &[],
        actions: &none,
        adoptable: &gone,
    };
    assert_eq!(unadoptable.run(&approve, false), Some(Outcome::Failed));
    for (effects, expected) in [
        (vec![("approve", "prepared")], Outcome::Pending),
        (vec![("approve", "confirmed")], Outcome::Approved),
        (
            vec![("approve", "confirmed"), ("merge", "confirmed")],
            Outcome::ApprovedAndMerged,
        ),
        (vec![("approve", "rejected")], Outcome::Failed),
    ] {
        let ledger = approval(&store, &approve, &effects);
        let evidence = Evidence {
            publications: &[],
            exit_writes: &[],
            actions: &ledger,
            adoptable: &adoptable,
        };
        assert_eq!(evidence.run(&approve, false), Some(expected), "{effects:?}");
        // Another run's final never confirms this one.
        let mut other = with_exit(&store, &job, ExitOutcome::SubmitReport);
        other.operation.id = "another-run".into();
        assert_eq!(evidence.run(&other, false), Some(Outcome::Report));
    }
    let merge = with_exit(
        &store,
        &job,
        ExitOutcome::Lgtm {
            comment: String::new(),
            resolve_own_threads: vec![],
            action: LgtmAction::Merge,
            dismissed_teammate_findings: vec![],
        },
    );
    let ledger = approval(&store, &merge, &[("merge", "confirmed")]);
    let evidence = Evidence {
        publications: &[],
        exit_writes: &[],
        actions: &ledger,
        adoptable: &adoptable,
    };
    assert_eq!(
        evidence.run(&merge, false),
        Some(Outcome::ApprovedAndMerged)
    );

    // A settled final with no effect still ends the run.
    for (settled, settlement, expected) in [
        (false, None, Outcome::Pending),
        (true, Some(Settlement::Done), Outcome::Approved),
        (true, Some(Settlement::NoAuthority), Outcome::ReadyForHuman),
        (true, Some(Settlement::Revoked), Outcome::Failed),
        (true, Some(Settlement::Moved), Outcome::Failed),
    ] {
        let mut ledger = approval(&store, &approve, &[]);
        let packet = ledger.finals[0].packet.as_mut().unwrap();
        packet.settled = settled;
        packet.settlement = settlement;
        let evidence = Evidence {
            publications: &[],
            exit_writes: &[],
            actions: &ledger,
            adoptable: &adoptable,
        };
        assert_eq!(
            evidence.run(&approve, false),
            Some(expected),
            "{settlement:?}"
        );
        // Its `primary_final` row agrees with the run's row.
        assert_eq!(evidence.final_review(&ledger.finals[0]), expected);
    }
    // An `lgtm` whose own thread resolutions failed never acts.
    let sign_off = with_exit(
        &store,
        &job,
        ExitOutcome::Lgtm {
            comment: String::new(),
            resolve_own_threads: vec!["thread".into()],
            action: LgtmAction::Approve,
            dismissed_teammate_findings: vec![],
        },
    );
    let ledger = approval(&store, &sign_off, &[]);
    let mut write = exit_write(&job, WritePhase::Failed, StepState::Planned);
    write.review_operation_id = sign_off.operation.id.clone();
    let evidence = Evidence {
        publications: &[],
        exit_writes: std::slice::from_ref(&write),
        actions: &ledger,
        adoptable: &adoptable,
    };
    assert_eq!(evidence.run(&sign_off, false), Some(Outcome::Failed));
    // A Ready sign-off whose own resolutions failed is Failed too.
    let ready = with_exit(
        &store,
        &job,
        ExitOutcome::Lgtm {
            comment: String::new(),
            resolve_own_threads: vec!["thread".into()],
            action: LgtmAction::Ready,
            dismissed_teammate_findings: vec![],
        },
    );
    assert_eq!(evidence.run(&ready, false), Some(Outcome::Failed));
    assert_eq!(plain.run(&ready, false), Some(Outcome::ReadyForHuman));

    // Comments count once published, never on the exit alone.
    let run = with_exit(&store, &job, comments());
    assert_eq!(plain.run(&run, false), Some(Outcome::Pending));
    assert_eq!(plain.run(&run, true), Some(Outcome::Report));
    let mut publication = Publication::new(run.clone(), false, true, NOW).unwrap();
    for (phase, expected) in [
        (crate::publication::Phase::Pending, Outcome::Pending),
        (crate::publication::Phase::Published, Outcome::Commented),
        (crate::publication::Phase::Stopped, Outcome::Failed),
        // Stopped before publishing: the head or base moved.
        (crate::publication::Phase::Stale, Outcome::Failed),
        (crate::publication::Phase::Discarded, Outcome::Report),
    ] {
        publication.phase = phase;
        let evidence = Evidence {
            publications: std::slice::from_ref(&publication),
            exit_writes: &[],
            actions: &none,
            adoptable: &adoptable,
        };
        assert_eq!(evidence.run(&run, false), Some(expected));
    }
    // A saved error with a retry ahead (paused, backing off) is still Pending.
    publication.phase = crate::publication::Phase::Pending;
    publication.error = Some("Automation paused.".into());
    for state in [OperationState::Interrupted, OperationState::Queued] {
        publication.operation.state = state;
        publication.operation.next_attempt_at = Some(NOW + 60);
        let evidence = Evidence {
            publications: std::slice::from_ref(&publication),
            exit_writes: &[],
            actions: &none,
            adoptable: &adoptable,
        };
        assert_eq!(evidence.run(&run, false), Some(Outcome::Pending));
    }
    // A parked publication (no retry scheduled) failed, whatever its phase.
    for (phase, state) in [
        (crate::publication::Phase::Preparing, OperationState::Failed),
        (
            crate::publication::Phase::Pending,
            OperationState::ManualRetry,
        ),
    ] {
        publication.phase = phase;
        publication.operation.state = state;
        let evidence = Evidence {
            publications: std::slice::from_ref(&publication),
            exit_writes: &[],
            actions: &none,
            adoptable: &adoptable,
        };
        assert_eq!(evidence.run(&run, false), Some(Outcome::Failed));
    }
    let mut write = exit_write(&job, WritePhase::Completed, StepState::Done);
    write.review_operation_id = run.operation.id.clone();
    let evidence = Evidence {
        publications: &[],
        exit_writes: std::slice::from_ref(&write),
        actions: &none,
        adoptable: &adoptable,
    };
    assert_eq!(evidence.run(&run, false), Some(Outcome::Commented));
    write.phase = WritePhase::Failed;
    let evidence = Evidence {
        publications: &[],
        exit_writes: std::slice::from_ref(&write),
        actions: &none,
        adoptable: &adoptable,
    };
    assert_eq!(evidence.run(&run, false), Some(Outcome::Failed));

    // Unfinished runs (a panic never completes) have no outcome.
    let mut running = with_exit(&store, &job, ExitOutcome::SubmitReport);
    running.operation.state = OperationState::Running;
    assert_eq!(plain.run(&running, false), None);
}

fn exits() -> [(ExitOutcome, Outcome); 3] {
    [
        (
            ExitOutcome::Wait {
                reason: WaitReason::Ci,
            },
            Outcome::Waiting,
        ),
        (
            ExitOutcome::ReadyForHuman {
                summary: "Over to you.".into(),
            },
            Outcome::ReadyForHuman,
        ),
        (ExitOutcome::SubmitReport, Outcome::Report),
    ]
}

#[test]
fn outcome_filter_pages_by_cursor_across_ties_without_gaps_or_repeats() {
    let (_root, store, mut monitor) = fixture();
    scan(
        &store,
        &mut monitor,
        (1..=50).map(|n| pull(n, Lifecycle::Open)).collect(),
        NOW + 5,
    );
    let jobs = store.load_queue().unwrap();
    let mut expected: BTreeMap<Outcome, BTreeSet<String>> = BTreeMap::new();
    let reviews: Vec<_> = jobs
        .iter()
        .enumerate()
        .map(|(n, job)| {
            let (exit, outcome) = exits()[n % 3].clone();
            expected
                .entry(outcome)
                .or_default()
                .insert(crate::queue::item_id(job));
            with_exit(&store, job, exit)
        })
        .collect();
    // One save: every row shares one activity sequence, so pages break ties by id.
    store.save_reviews(&reviews).unwrap();
    for wanted in [
        BTreeSet::from([Outcome::Waiting]),
        BTreeSet::from([Outcome::ReadyForHuman, Outcome::Report]),
    ] {
        let mut cursor = None;
        let mut found = BTreeSet::new();
        loop {
            let page = page(
                &store,
                PageRequest {
                    limit: 4,
                    cursor,
                    outcomes: Some(wanted.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(page.results.len() <= 4);
            assert_eq!(
                page.outcome_counts,
                expected.iter().map(|(o, ids)| (*o, ids.len())).collect()
            );
            for row in &page.results {
                assert!(wanted.contains(&row.outcome.unwrap()));
                assert!(found.insert(row.item_id.clone()), "repeated row");
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let all: BTreeSet<_> = wanted
            .iter()
            .flat_map(|o| expected[o].iter().cloned())
            .collect();
        assert_eq!(found, all);
    }
    // Empty selection: every row.
    let every = page(
        &store,
        PageRequest {
            limit: 200,
            outcomes: Some(BTreeSet::new()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(every.results.len(), jobs.len());
}

#[test]
fn an_older_index_without_outcomes_is_backfilled_from_saved_reviews() {
    let (root, store, _monitor) = fixture();
    let job = store.load_queue().unwrap()[0].clone();
    store
        .save_reviews(&[with_exit(&store, &job, ExitOutcome::SubmitReport)])
        .unwrap();
    let first = page(
        &store,
        PageRequest {
            limit: 10,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(first.results[0].outcome, Some(Outcome::Report));
    // As an older build saved it: format 3, rows without an outcome.
    let path = root.path().join("state/result-index.json");
    let mut index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    index["version"] = json!(3);
    for stamp in index["rows"].as_object_mut().unwrap().values_mut() {
        stamp["row"].as_object_mut().unwrap().remove("outcome");
    }
    store.write_state("result-index.json", &index).unwrap();
    let upgraded = page(
        &Store::new(root.path().into()),
        PageRequest {
            limit: 10,
            outcomes: Some(BTreeSet::from([Outcome::Report])),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(upgraded.results.len(), 1);
    assert_eq!(upgraded.results[0].outcome, Some(Outcome::Report));
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], json!(4));
}

#[test]
fn a_revision_takes_its_latest_run_and_a_teammate_report_never_hides_it() {
    let (_root, store, _monitor) = fixture();
    let job = store.load_queue().unwrap()[0].clone();
    let run = |id: &str, exit: ExitOutcome| {
        let mut run = with_exit(&store, &job, exit);
        run.operation.id = id.into();
        run
    };
    let wait = run(
        "wait",
        ExitOutcome::Wait {
            reason: WaitReason::Ci,
        },
    );
    let outcome = |reviews: &[ReviewRun]| {
        store.save_reviews(reviews).unwrap();
        page(
            &store,
            PageRequest {
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap()
        .results[0]
            .outcome
    };
    // Local-only findings (nothing to publish) after a wait: the later run wins.
    assert_eq!(
        outcome(&[wait.clone(), run("comments", comments())]),
        Some(Outcome::Report)
    );
    // A teammate report after it does not.
    assert_eq!(
        outcome(&[wait, run("report", ExitOutcome::SubmitReport)]),
        Some(Outcome::Waiting)
    );
}

#[test]
fn a_reply_kept_local_is_a_report_and_one_with_the_reply_grant_waits_to_publish() {
    let (_root, store, _monitor) = fixture();
    let (_, _, mut follow) = evidence(&store);
    follow.phase = FollowPhase::WaitingPublication;
    follow.publication = None;
    follow.result = Some(
        serde_json::from_value(json!({
            "output":{"decision":"reply","body":"Fixed in the latest push.",
                "new_information":"","reason":"","evidence":[]},
            "session_id":"session","model":"model","runtime_version":"fixture",
            "input_tokens":1,"output_tokens":1,"tool_calls":1
        }))
        .unwrap(),
    );
    let configuration = follow.context.selection.configuration.as_mut().unwrap();
    configuration.authority.reply = false;
    assert_eq!(
        crate::review::outcome::follow_up(&follow),
        Some(Outcome::Report)
    );
    follow
        .context
        .selection
        .configuration
        .as_mut()
        .unwrap()
        .authority
        .reply = true;
    assert_eq!(
        crate::review::outcome::follow_up(&follow),
        Some(Outcome::Pending)
    );
}
