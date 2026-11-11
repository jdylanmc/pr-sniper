use super::*;
use crate::{
    actions::{Basis, Effect, FinalReview, Ledger, PacketApproval},
    exit_writes::{Phase as WritePhase, StepState},
    exits::{ExitOutcome, LgtmAction, WaitReason},
    review::outcome::{Evidence, Outcome},
};

fn with_exit(store: &Store, job: &QueueJob, exit: ExitOutcome) -> ReviewRun {
    let mut run = completed(store, job);
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
    let plain = Evidence {
        publications: &[],
        exit_writes: &[],
        actions: &none,
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
    };
    assert_eq!(
        evidence.run(&merge, false),
        Some(Outcome::ApprovedAndMerged)
    );

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
        };
        assert_eq!(evidence.run(&run, false), Some(expected));
    }
    let mut write = exit_write(&job, WritePhase::Completed, StepState::Done);
    write.review_operation_id = run.operation.id.clone();
    let evidence = Evidence {
        publications: &[],
        exit_writes: std::slice::from_ref(&write),
        actions: &none,
    };
    assert_eq!(evidence.run(&run, false), Some(Outcome::Commented));
    write.phase = WritePhase::Failed;
    let evidence = Evidence {
        publications: &[],
        exit_writes: std::slice::from_ref(&write),
        actions: &none,
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
