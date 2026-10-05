use super::*;
use crate::{
    capacity::{Coordinator as Capacity, Dispatch, Kind},
    github::{
        actions::{HumanThread, MergeMethod, ProviderReview},
        metadata::{Lifecycle, PullRequest},
        provider::{
            Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository, Response,
            Transport,
        },
        publication::{MutationTransport, Request},
        threads::QueryTransport,
        ConnectionError, Identity,
    },
    monitoring::{ActivationMode, Monitor, MonitoringActivation, PollResult},
    review::{
        runtime::{FullReview, Task},
        ReviewResult,
    },
    storage::Settings,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const NOW: i64 = 1_800_000_000;
fn output() -> ReviewResult {
    serde_json::from_value(json!({"reviewed_base_sha":"b".repeat(40),"output":{"synopsis":"Review completed.",
        "files":[{"path":"source.rs","order":1,"explanation":"Complete source review."}],"findings":[],"decision":"machine_sign_off"},
        "session_id":"fixture","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":1})).unwrap()
}
fn fixture(count: usize, approve: bool, merge: bool) -> (tempfile::TempDir, Store, String) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut settings:Settings=serde_json::from_value(json!({"launch_at_login":false,"doctrines":[],
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,"provider_account_id":"22",
            "provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}]}]})).unwrap();
    settings.defaults.automatic_agent_start = true;
    for i in 1..=count {
        let agent = format!("aaaaaaaa-aaaa-4aaa-8aaa-{i:012}");
        settings.agents.push(serde_json::from_value(json!({"id":agent,"name":format!("Agent {i}"),"model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"machine"})).unwrap());
        settings.repositories[0].assignments.push(
            serde_json::from_value(
                json!({"id":format!("cccccccc-cccc-4ccc-8ccc-{i:012}"),"agent_id":agent,
            "schedule":settings.defaults.schedule,"comment":true,"approve":true,
            "actions":{"approve":i==1&&approve,"merge":i==1&&merge}}),
            )
            .unwrap(),
        );
    }
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "scope".into(),
            repository_id: REPO.into(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: 0,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW - 1,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    let ticket = monitor.prepare_checks(&store, NOW, true).unwrap().remove(0);
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
                pull_requests: vec![PullRequest {
                    id: "9".into(),
                    number: 1,
                    title: "Review".into(),
                    author: Some(Identity {
                        id: "11".into(),
                        login: "author".into(),
                    }),
                    requested_reviewers: vec![],
                    requested_teams: vec![],
                    state: Lifecycle::Open,
                    draft: false,
                    head_sha: "a".repeat(40),
                    base_sha: "b".repeat(40),
                    head_repository_id: Some("100".into()),
                    base_repository_id: "100".into(),
                    updated_at: "2026-09-30T00:00:00Z".into(),
                    files: vec![],
                }],
            }),
            NOW + 1,
        )
        .unwrap();
    let jobs = store.load_queue().unwrap();
    let item = queue::item_id(&jobs[0]);
    let peers = jobs
        .iter()
        .map(|job| {
            let assignment = job.assignment_id.as_ref().unwrap();
            let mut operation = JobOperation::review(job, NOW + 2);
            operation.state = OperationState::Completed;
            ReviewRun {
                feedback_context: None,
                key: crate::review::key(job, assignment),
                assignment_id: assignment.clone(),
                job: job.clone(),
                selection: Selection::resolve(&settings, job, assignment).unwrap(),
                operation,
                manual_start: false,
                trust_confirmed: false,
                phase: "Complete".into(),
                error: None,
                result: Some(output()),
            }
        })
        .collect::<Vec<_>>();
    store.save_reviews(&peers).unwrap();
    (root, store, item)
}
fn observed() -> Observation {
    Observation {
        write_capability: true,
        node_id: "PR_node".into(),
        repository_id: "100".into(),
        pull_request_id: "9".into(),
        account_id: "22".into(),
        author_id: "11".into(),
        head_repository_id: Some("100".into()),
        head: "a".repeat(40),
        base: "b".repeat(40),
        base_name: "main".into(),
        merge_rules: Some(vec![]),
        merge_rules_error: None,
        state: "OPEN".into(),
        draft: false,
        permission: "WRITE".into(),
        mergeable: "MERGEABLE".into(),
        merge_state: "CLEAN".into(),
        review_decision: Some("APPROVED".into()),
        checks: Some("SUCCESS".into()),
        check_contexts: vec![],
        in_merge_queue: false,
        method: Some(MergeMethod::Squash),
        protection: Value::Null,
        threads: vec![],
        reviews: vec![],
        comments: vec![],
        merged_by: None,
        merged_at: None,
        merge_commit: None,
    }
}
fn completed_final(store: &Store, item: &str) -> FinalReview {
    synchronize(store, item, Ok(observed()), NOW + 10).unwrap();
    let coordinator = Capacity::default();
    let mut batch = coordinator.dispatch(store, NOW + 11).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let dispatch = batch.dispatched.remove(0);
    assert_eq!(dispatch.key().kind, Kind::PrimaryFinal);
    let Dispatch::Review(run, _) = dispatch else {
        panic!("Full final-review execution required")
    };
    let prompt = host::prompt_context(store, &run.key).unwrap();
    assert!(!prompt["normal_passes"].as_array().unwrap().is_empty());
    let context = crate::github::review::ReviewContext {
        pull: PullRequest {
            id: "9".into(),
            number: 1,
            title: "Review".into(),
            author: Some(Identity {
                id: "11".into(),
                login: "author".into(),
            }),
            requested_reviewers: vec![],
            requested_teams: vec![],
            state: Lifecycle::Open,
            draft: false,
            head_sha: "a".repeat(40),
            base_sha: "b".repeat(40),
            head_repository_id: Some("100".into()),
            base_repository_id: "100".into(),
            updated_at: "2026-09-30T00:00:00Z".into(),
            files: vec![],
        },
        base_revision: "b".repeat(40),
        files: vec![crate::github::review::ReviewFile {
            path: "source.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: None,
        }],
        head: BTreeMap::new(),
        base: BTreeMap::new(),
    };
    let task = FullReview {
        feedback: run.feedback_context.clone().unwrap(),
        owner_agent_id: run.selection.agent.id.clone(),
        final_context: Some(prompt),
    };
    let result = output();
    let parsed = task
        .validate(
            &serde_json::to_string(&result.output).unwrap(),
            &context,
            &GithubClient::new(NoReads),
            "example/repo",
        )
        .unwrap();
    assert_eq!(parsed.files.len(), 1);
    let mut missing = result.output.clone();
    missing.files.clear();
    assert!(task
        .validate(
            &serde_json::to_string(&missing).unwrap(),
            &context,
            &GithubClient::new(NoReads),
            "example/repo"
        )
        .is_err());
    host::complete(store, &run, Ok(result), NOW + 12).unwrap();
    coordinator
        .release(
            &crate::capacity::WorkId {
                kind: Kind::PrimaryFinal,
                id: run.key.clone(),
            },
            &run.operation.id,
        )
        .unwrap();
    store.load_actions().unwrap().finals.remove(0)
}

#[test]
fn fork_final_review_and_permitted_action_need_no_revision_consent() {
    let (_root, store, item) = fixture(1, true, true);
    let mut observation = observed();
    observation.head_repository_id = Some("fork-repository".into());
    observation.author_id = "unwatched-author".into();
    synchronize(&store, &item, Ok(observation.clone()), NOW + 10).unwrap();
    let coordinator = Capacity::default();
    let mut batch = coordinator.dispatch(&store, NOW + 11).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let dispatch = batch.dispatched.remove(0);
    assert_eq!(dispatch.key().kind, Kind::PrimaryFinal);
    let Dispatch::Review(run, _) = dispatch else {
        panic!("Expected final review")
    };
    assert!(!run.trust_confirmed);
    host::complete(&store, &run, Ok(output()), NOW + 12).unwrap();
    let final_review = store.load_actions().unwrap().finals.remove(0);
    assert!(ready(&store, &final_review, &observation, Action::Approve).is_ok());
    assert!(ready(&store, &final_review, &observation, Action::Merge).is_ok());
    observation.head = "c".repeat(40);
    assert!(ready(&store, &final_review, &observation, Action::Approve).is_err());
}

#[test]
fn retention_waits_for_uncertain_actions_then_discards_all_final_detail_copies() {
    let (root, store, item) = fixture(1, true, false);
    let run = completed_final(&store, &item);
    let effect = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    mark_intent(&store, &effect.id, NOW + 21).unwrap();
    finish_effect(
        &store,
        &effect.id,
        &effect.operation.id,
        Err(crate::publication::WriteFailure {
            failure: Failure::permanent("Synthetic lost response"),
            uncertain: true,
        }),
    )
    .unwrap();
    let mut closed = observed();
    closed.state = "CLOSED".into();
    synchronize(&store, &item, Ok(closed.clone()), NOW + 22).unwrap();
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 0);
    assert!(!store.load_actions().unwrap().finals.is_empty());
    let receipt = Receipt {
        id: "confirmed-fixture-vote".into(),
        actor_id: "22".into(),
        head: observed().head,
        action: Action::Approve,
        merge_commit: None,
    };
    finish_effect(
        &store,
        &effect.id,
        &effect.operation.id,
        Ok(receipt.clone()),
    )
    .unwrap();
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0]
        .execution
        .result
        .as_mut()
        .unwrap()
        .output
        .synopsis = "RETENTION-BULK-ONLY".into();
    ledger.finals[0].basis.peers[0]
        .result
        .as_mut()
        .unwrap()
        .output
        .synopsis = "RETENTION-BULK-ONLY".into();
    ledger.effects[0].body = "RETENTION-BULK-ONLY".into();
    store.save_actions(&ledger).unwrap();
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 1);
    let actions = store.load_actions().unwrap();
    assert!(
        actions.finals.is_empty() && actions.effects.is_empty() && actions.observations.is_empty()
    );
    let retained = crate::retention::load(&store).unwrap();
    assert_eq!(
        retained.receipts[0].effects[0].receipt.as_ref(),
        Some(&receipt)
    );
    for file in std::fs::read_dir(root.path().join("state")).unwrap() {
        assert!(
            !String::from_utf8(std::fs::read(file.unwrap().path()).unwrap())
                .unwrap()
                .contains("RETENTION-BULK-ONLY")
        );
    }
    assert!(finish_effect(&store, &effect.id, &effect.operation.id, Ok(receipt)).is_err());
    assert!(crate::actions::prepare_effect(
        &store,
        &run.id,
        Action::Approve,
        &observed(),
        NOW + 25
    )
    .is_err());
}

#[test]
fn retention_does_not_treat_a_running_final_as_settled() {
    let (_root, store, item) = fixture(1, true, false);
    let run = completed_final(&store, &item);
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0].execution.operation.state = OperationState::Running;
    store.save_actions(&ledger).unwrap();
    let mut closed = observed();
    closed.state = "CLOSED".into();
    synchronize(&store, &item, Ok(closed), NOW + 20).unwrap();
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 0);
    assert_eq!(store.load_actions().unwrap().finals[0].id, run.id);
}

#[test]
fn retention_stale_same_head_action_observation_cannot_retire_reopened_iteration() {
    let (_root, store, item) = fixture(1, true, false);
    completed_final(&store, &item);
    let mut queue = store.load_queue_state().unwrap();
    queue.tracked[0].iteration = 2;
    queue.tracked[0].iteration_id = "reopened-iteration".into();
    queue.tracked[0].item_id = "reopened-item".into();
    store.save_queue_state(&queue).unwrap();
    let mut closed = observed();
    closed.state = "CLOSED".into();
    assert!(synchronize(&store, &item, Ok(closed), NOW + 20).is_err());
    let queue = store.load_queue_state().unwrap();
    assert_eq!(queue.tracked[0].lifecycle, Lifecycle::Open);
    assert!(!queue.tracked[0].terminal_observed);
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 0);
}

struct NoReads;
impl Transport for NoReads {
    fn get(&self, _: &str) -> Result<Response, ConnectionError> {
        panic!("No unconfigured provider read")
    }
}

#[test]
fn correction_cancel_retry_final_teardown_releases_only_old_owner_and_refills_capacity_one() {
    use std::{sync::mpsc, time::Duration};

    let (root, store, item) = fixture(1, true, false);
    let mut settings = store.load_settings().unwrap();
    settings.capacity = 1;
    store.save_settings(&settings).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let capacity = Capacity::default();
    let mut batch = capacity.dispatch(&store, NOW + 11).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let work = batch.dispatched.remove(0);
    let key = work.key();
    assert_eq!(key.kind, Kind::PrimaryFinal);
    let Dispatch::Review(old, token) = work else {
        panic!("Final worker expected")
    };
    let old_id = old.operation.id.clone();
    let reviews = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    let mut queue = store.load_queue_state().unwrap();
    let mut unrelated = queue.jobs[0].clone();
    unrelated.pull_request_id = "10".into();
    unrelated.number = 2;
    let other_work = unrelated.work.as_mut().unwrap();
    other_work.id = "unrelated-normal".into();
    other_work.item_id = "unrelated-item".into();
    other_work.iteration_id = "unrelated-iteration".into();
    other_work.enqueue_order = store.allocate_enqueue_order().unwrap();
    queue.jobs.push(unrelated);
    store.save_queue_state(&queue).unwrap();
    let store = Mutex::new(store);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let accepted = std::thread::scope(|scope| {
        let store = &store;
        let capacity = &capacity;
        let old = &old;
        let worker = scope.spawn(move || {
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let store = store.lock().unwrap();
            assert!(host::phase(&store, old, "Late A phase").is_err());
            assert!(host::complete(&store, old, Ok(output()), NOW + 15).is_err());
            host::finish_final_worker(&store, capacity, old, Err(Failure::cancelled()), NOW + 15)
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let accepted = {
            let store = store.lock().unwrap();
            host::cancel_final_in_store(&store, capacity, &key.id, NOW + 12).unwrap();
            assert!(token.load(std::sync::atomic::Ordering::SeqCst));
            let cancelled = store.load_actions().unwrap().finals.remove(0);
            assert_eq!(cancelled.execution.operation.state, OperationState::Failed);
            request_final(&store, &key.id, NOW + 13).unwrap();
            let accepted = store.load_actions().unwrap().finals.remove(0);
            assert_ne!(accepted.execution.operation.id, old_id);
            assert_eq!(accepted.execution.operation.state, OperationState::Queued);
            assert_eq!(accepted.attempts, vec![cancelled.execution]);
            assert!(accepted.execution.manual_start);
            assert!(!accepted.cancelled);
            assert!(capacity
                .dispatch(&store, NOW + 14)
                .unwrap()
                .dispatched
                .is_empty());
            let snapshot = capacity.snapshot(&store, NOW + 14).unwrap();
            assert_eq!((snapshot.active, snapshot.stopping), (1, 1));
            accepted
        };
        release_tx.send(()).unwrap();
        worker.join().unwrap().unwrap();
        accepted
    });
    let store = store.lock().unwrap();
    assert_eq!(store.load_actions().unwrap().finals[0], accepted);
    assert!(capacity.finished());
    let mut batch = capacity.dispatch(&store, NOW + 16).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let Dispatch::Review(retry, _) = batch.dispatched.remove(0) else {
        panic!("Final retry expected")
    };
    assert_eq!(retry.operation.id, accepted.execution.operation.id);
    assert_eq!(retry.operation.attempt_count, 1);
    assert!(retry.manual_start);
    let running = store.load_actions().unwrap();
    assert_eq!(running.finals[0].attempts, accepted.attempts);
    assert_eq!(running.finals[0].basis, accepted.basis);
    assert!(host::finish_final_worker(&store, &capacity, &old, Ok(output()), NOW + 17).is_err());
    assert_eq!(store.load_actions().unwrap(), running);
    assert_eq!(capacity.snapshot(&store, NOW + 17).unwrap().active, 1);
    assert!(capacity
        .dispatch(&store, NOW + 17)
        .unwrap()
        .dispatched
        .is_empty());
    host::finish_final_worker(&store, &capacity, &retry, Ok(output()), NOW + 18).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        reviews
    );
    let mut batch = capacity.dispatch(&store, NOW + 19).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let other_key = batch.dispatched[0].key();
    assert_eq!(other_key.kind, Kind::Normal);
    let Dispatch::Review(other, _) = batch.dispatched.remove(0) else {
        panic!("Unrelated normal work expected")
    };
    assert_eq!(other.job.pull_request_id, "10");
    crate::review::host::complete(&store, &other.operation.id, Ok(output()), NOW + 20).unwrap();
    capacity.release(&other_key, &other.operation.id).unwrap();
    assert!(capacity
        .dispatch(&store, NOW + 21)
        .unwrap()
        .dispatched
        .is_empty());
    let saved = store.load_actions().unwrap().finals.remove(0);
    assert_eq!(saved.execution.operation.id, retry.operation.id);
    assert_eq!(saved.execution.operation.attempt_count, 1);
    assert_eq!(saved.attempts, accepted.attempts);
}

#[test]
fn correction_action_read_error_opt_out_restores_personal_handoff_after_restart() {
    let (root, store, item) = fixture(1, true, true);
    synchronize(
        &store,
        &item,
        Err(Failure::permanent("Optional action evidence unavailable.")),
        NOW + 10,
    )
    .unwrap();
    let before = store.load_actions().unwrap();
    assert!(before.finals.is_empty());
    assert!(before.effects.is_empty());
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::Blocked
    );
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
        approve: false,
        merge: false,
    });
    store.save_settings(&settings).unwrap();
    drop(store);
    let store = Store::new(root.path().into());
    restore(&store).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].state, State::MachineSignedOff);
    let status = snapshot.items[0].action_status.as_ref().unwrap();
    assert!(status.machine_clear);
    assert!(status.personal_review.starts_with("Required:"));
    assert!(status
        .blockers
        .iter()
        .any(|b| b == "Optional action evidence unavailable."));
    assert!(status
        .observation_retry_blocker
        .as_ref()
        .unwrap()
        .contains("No enabled provider action"));
    assert!(request_observation_retry(&store, &item, NOW + 19)
        .unwrap_err()
        .contains("No enabled provider action"));
    assert_eq!(store.load_actions().unwrap(), before);
    synchronize(&store, &item, Ok(observed()), NOW + 20).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(store.load_actions().unwrap().effects.is_empty());
    assert!(Capacity::default()
        .dispatch(&store, NOW + 21)
        .unwrap()
        .dispatched
        .is_empty());
}

#[test]
fn correction_final_true_storage_failure_retains_visible_reservation() {
    let (_root, store, item) = fixture(1, true, false);
    let mut settings = store.load_settings().unwrap();
    settings.capacity = 1;
    store.save_settings(&settings).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let capacity = Capacity::default();
    let mut batch = capacity.dispatch(&store, NOW + 11).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let key = batch.dispatched[0].key();
    let Dispatch::Review(run, token) = batch.dispatched.remove(0) else {
        panic!("Final worker expected")
    };
    let before = store.load_actions().unwrap();
    store.fail_state_write("actions.json", 1);
    let error =
        host::finish_final_worker(&store, &capacity, &run, Ok(output()), NOW + 12).unwrap_err();
    assert_eq!(error, "Injected actions.json write failure.");
    // The callback's error branch reports this error and marks only its operation.
    capacity.persistence_failed(&run.operation.id).unwrap();
    assert!(token.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(store.load_actions().unwrap(), before);
    let snapshot = capacity.snapshot(&store, NOW + 13).unwrap();
    assert_eq!((snapshot.active, snapshot.stopping), (1, 1));
    let reservation = snapshot.work.iter().find(|w| w.key == key).unwrap();
    assert!(reservation
        .reason
        .as_ref()
        .unwrap()
        .contains("Slot retained; repair storage and restart"));
    assert!(capacity
        .dispatch(&store, NOW + 13)
        .unwrap()
        .dispatched
        .is_empty());
    assert!(!capacity.finished());
}

#[test]
fn correction_opt_out_preserves_pending_unknown_and_unverified_receipts_for_reconciliation() {
    for state in [
        EffectState::Prepared,
        EffectState::Uncertain,
        EffectState::Confirmed,
    ] {
        let (root, store, item) = fixture(1, true, true);
        let run = completed_final(&store, &item);
        let effect =
            prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
        if state != EffectState::Prepared {
            mark_intent(&store, &effect.id, NOW + 21).unwrap();
        }
        if state == EffectState::Confirmed {
            finish_effect(
                &store,
                &effect.id,
                &effect.operation.id,
                Ok(Receipt {
                    id: "retained-receipt".into(),
                    actor_id: "22".into(),
                    head: observed().head,
                    action: Action::Approve,
                    merge_commit: None,
                }),
            )
            .unwrap();
            verify_after_effect(
                &store,
                &effect,
                Err(Failure::permanent("Post-action read failed.")),
            )
            .unwrap_err();
        }
        synchronize(
            &store,
            &item,
            Err(Failure::permanent("Optional action evidence unavailable.")),
            NOW + 22,
        )
        .unwrap();
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
            approve: false,
            merge: false,
        });
        store.save_settings(&settings).unwrap();
        let before = store.load_actions().unwrap();
        drop(store);
        let store = Store::new(root.path().into());
        restore(&store).unwrap();
        let snapshot = queue::snapshot(&store, vec![]).unwrap();
        let status = snapshot.items[0].action_status.as_ref().unwrap();
        assert!(!status.machine_clear, "{state:?}");
        assert_eq!(status.personal_review, "Not inferred.");
        assert!(matches!(
            snapshot.items[0].state,
            State::Blocked | State::Failed
        ));
        assert_eq!(store.load_actions().unwrap(), before);
        assert!(ready(&store, &run, &observed(), Action::Approve).is_err());
        assert!(ready(&store, &run, &observed(), Action::Merge).is_err());
        request_observation_retry(&store, &item, NOW + 23).unwrap();
        assert_eq!(store.load_actions().unwrap().effects, before.effects);
        if state != EffectState::Prepared {
            let mut ledger = store.load_actions().unwrap();
            ledger.effects[0].reconcile_attempts = 3;
            store.save_actions(&ledger).unwrap();
            assert!(request_observation_retry(&store, &item, NOW + 24).is_err());
            request_reconciliation(&store, &effect.id, NOW + 24).unwrap();
            let mut expected = ledger.effects[0].clone();
            expected.reconcile_requested = true;
            assert_eq!(store.load_actions().unwrap().effects[0], expected);
            assert!(observation_pending(
                &queue::normal_snapshot(&store, vec![]).unwrap().items[0],
                &settings,
                &store.load_actions().unwrap(),
                NOW + 24,
            ));
        }
        assert!(Capacity::default()
            .dispatch(&store, NOW + 25)
            .unwrap()
            .dispatched
            .is_empty());
    }
}

#[test]
fn correction_optional_action_diagnostic_does_not_clear_normal_feedback_or_revision_blockers() {
    for case in ["normal", "feedback", "revision", "human"] {
        let (_root, store, item) = fixture(1, false, false);
        synchronize(
            &store,
            &item,
            Err(Failure::permanent("Optional action evidence unavailable.")),
            NOW + 10,
        )
        .unwrap();
        match case {
            "normal" => {
                let mut reviews = store.load_reviews().unwrap();
                reviews[0].operation.state = OperationState::Running;
                reviews[0].result = None;
                store.save_reviews(&reviews).unwrap();
            }
            "feedback" => {
                let mut monitoring = store.load_monitoring_state().unwrap();
                monitoring
                    .health
                    .get_mut(REPO)
                    .unwrap()
                    .conversation_admission_pending = true;
                store.save_monitoring_state(&monitoring).unwrap();
            }
            "revision" => {
                let mut jobs = store.load_queue().unwrap();
                jobs[0].waiting = monitoring::WAITING_SUPERSEDED.into();
                store.save_queue(&jobs).unwrap();
            }
            "human" => {
                let mut reviews = store.load_reviews().unwrap();
                reviews[0].result.as_mut().unwrap().output.decision =
                    crate::review::Decision::HumanInputRequired;
                store.save_reviews(&reviews).unwrap();
            }
            _ => unreachable!(),
        }
        let normal = queue::normal_snapshot(&store, vec![]).unwrap();
        assert_ne!(normal.items[0].state, State::MachineSignedOff, "{case}");
        let projected = queue::snapshot(&store, vec![]).unwrap();
        assert_eq!(projected.items[0].state, normal.items[0].state, "{case}");
        assert!(
            !projected.items[0]
                .action_status
                .as_ref()
                .unwrap()
                .machine_clear
        );
        assert!(request_observation_retry(&store, &item, NOW + 11).is_err());
        assert!(store.load_actions().unwrap().effects.is_empty());
    }
}

#[test]
fn all_current_agents_clear_and_local_only_handoff_precede_distinct_shared_final() {
    let (root, store, item) = fixture(7, true, true);
    let mut peers = store.load_reviews().unwrap();
    let original = peers.clone();
    peers[6].operation.state = OperationState::Running;
    peers[6].result = None;
    store.save_reviews(&peers).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 3).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    store.save_reviews(&original).unwrap();
    let bytes = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    let final_review = completed_final(&store, &item);
    assert_eq!(final_review.basis.peers.len(), 7);
    assert_eq!(
        final_review.execution.operation.operation_type,
        "primary_final_review"
    );
    assert_ne!(final_review.execution.key, original[0].key);
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        bytes
    );
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::MachineSignedOff
    );
    assert!(store.load_publications().unwrap().is_empty());
    let mut settings = store.load_settings().unwrap();
    let mut new = settings.repositories[0].assignments[0].clone();
    new.id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into();
    settings.repositories[0].assignments.push(new);
    store.save_settings(&settings).unwrap();
    assert!(ready(&store, &final_review, &observed(), Action::Approve).is_err());
    assert_eq!(store.load_reviews().unwrap(), original);
}

#[test]
fn opt_ins_are_independent_legacy_flags_inert_and_no_primary_has_no_actions() {
    for (approve, merge) in [(false, false), (true, false), (false, true), (true, true)] {
        let (_root, store, item) = fixture(2, approve, merge);
        synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
        assert_eq!(
            !store.load_actions().unwrap().finals.is_empty(),
            approve || merge
        );
        if approve || merge {
            let final_review = completed_final(&store, &item);
            assert_eq!(
                ready(&store, &final_review, &observed(), Action::Approve).is_ok(),
                approve
            );
            assert_eq!(
                ready(&store, &final_review, &observed(), Action::Merge).is_ok(),
                merge
            );
        }
    }
    let (_root, store, item) = fixture(2, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].primary_assignment_id = None;
    store.save_settings(&settings).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(queue::snapshot(&store, vec![]).unwrap().items[0]
        .action_status
        .as_ref()
        .unwrap()
        .blockers
        .iter()
        .any(|s| s.contains("No repository primary")));
}

#[test]
fn draft_self_approval_quorum_ci_outdated_threads_and_provider_method_have_distinct_gates() {
    let (_root, store, item) = fixture(1, true, true);
    let final_review = completed_final(&store, &item);
    let mut current = observed();
    current.review_decision = Some("REVIEW_REQUIRED".into());
    current.merge_state = "BLOCKED".into();
    current.checks = Some("PENDING".into());
    assert!(ready(&store, &final_review, &current, Action::Approve).is_ok());
    assert!(ready(&store, &final_review, &current, Action::Merge).is_err());
    current = observed();
    current.author_id = "22".into();
    assert!(current.blocker(Action::Approve).unwrap().contains("author"));
    assert!(current.blocker(Action::Merge).is_none());
    for kind in [
        "draft",
        "head",
        "base",
        "review",
        "thread",
        "permission",
        "method",
        "queue",
        "checks",
        "hooks",
    ] {
        let mut current = observed();
        match kind {
            "draft" => current.draft = true,
            "head" => current.head = "c".repeat(40),
            "base" => current.base = "c".repeat(40),
            "review" => current.reviews.push(ProviderReview {
                id: "44".into(),
                actor_id: "55".into(),
                head: current.head.clone(),
                state: "CHANGES_REQUESTED".into(),
                body: "Concern".into(),
                submitted_at: Some("2026-09-30T00:00:00Z".into()),
            }),
            "thread" => current.threads.push(HumanThread {
                id: "human-thread".into(),
                resolved: false,
                outdated: true,
                resolved_by: None,
                comments: vec![json!({"body":"Still blocking"})],
            }),
            "permission" => current.permission = "READ".into(),
            "method" => current.method = None,
            "queue" => current.in_merge_queue = true,
            "checks" => current.checks = Some("FAILURE".into()),
            _ => current.merge_state = "HAS_HOOKS".into(),
        }
        assert!(
            ready(&store, &final_review, &current, Action::Merge).is_err(),
            "{kind}"
        );
    }
}

#[test]
fn final_findings_and_same_head_reopen_never_authorize_provider_actions() {
    let (_root, store, item) = fixture(1, true, true);
    let mut final_review = completed_final(&store, &item);
    final_review
        .execution
        .result
        .as_mut()
        .unwrap()
        .output
        .decision = crate::review::Decision::HumanInputRequired;
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0] = final_review.clone();
    store.save_actions(&ledger).unwrap();
    assert!(ready(&store, &final_review, &observed(), Action::Approve).is_err());
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::WaitingForHuman
    );
    let mut queue = store.load_queue_state().unwrap();
    queue.tracked[0].iteration_id = "reopened".into();
    queue.tracked[0].item_id = "new-item".into();
    queue.tracked[0].iteration += 1;
    queue.jobs[0].work.as_mut().unwrap().iteration_id = "reopened".into();
    queue.jobs[0].work.as_mut().unwrap().item_id = "new-item".into();
    store.save_queue_state(&queue).unwrap();
    assert!(validate_local(&store, &final_review).is_err());
}

#[derive(Clone)]
struct Wire(Arc<Mutex<Server>>);
struct Server {
    observation: Observation,
    writes: Vec<(String, Value)>,
    lost: bool,
    reject: bool,
}
impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let server = self.0.lock().unwrap();
        let value = if path.contains("/reviews?") {
            json!(server.observation.reviews.iter().map(|r|json!({
            "id":r.id.parse::<u64>().unwrap(),"user":{"id":r.actor_id.parse::<u64>().unwrap()},"commit_id":r.head,"state":r.state,"body":r.body,"submitted_at":r.submitted_at
        })).collect::<Vec<_>>())
        } else if path.contains("/comments?") || path == "/repos/example/repo/rules/branches/main" {
            json!([])
        } else {
            panic!("Unexpected read {path}")
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}
impl QueryTransport for Wire {
    fn query(&self, query: &str, _: Value) -> Result<Response, ConnectionError> {
        assert!(!query.contains("viewerCanMergeAsAdmin"));
        assert!(!query.starts_with("mutation"));
        let o = &self.0.lock().unwrap().observation;
        let mut pull = json!({"id":o.node_id,"fullDatabaseId":9,"baseRefOid":o.base,"headRefOid":o.head,"isDraft":o.draft,"state":o.state,
                "mergeStateStatus":o.merge_state,"mergeable":o.mergeable,"reviewDecision":o.review_decision,"isInMergeQueue":o.in_merge_queue,
                "author":{"databaseId":o.author_id.parse::<u64>().unwrap()},"mergedBy":o.merged_by.as_ref().map(|id|json!({"databaseId":id.parse::<u64>().unwrap()})),
                "mergedAt":o.merged_at,"mergeCommit":o.merge_commit.as_ref().map(|id|json!({"oid":id})),
                "baseRef":{"branchProtectionRule":o.protection}});
        pull["headRepository"] = json!(o
            .head_repository_id
            .as_ref()
            .map(|id| json!({"databaseId":id.parse::<u64>().unwrap()})));
        pull["baseRefName"] = json!(o.base_name);
        pull["commits"] = json!({"nodes":[{"commit":{"oid":o.head,"statusCheckRollup":{"state":o.checks,"contexts":{
                    "totalCount":1,"pageInfo":{"hasNextPage":false},"nodes":[{"__typename":"CheckRun","name":"CI","status":"COMPLETED","conclusion":"SUCCESS"}]}}}}]});
        pull["reviewThreads"] = json!({"totalCount":o.threads.len(),"pageInfo":{"hasNextPage":false},"nodes":o.threads.iter().map(|t|json!({
                    "id":t.id,"isResolved":t.resolved,"isOutdated":t.outdated,"resolvedBy":t.resolved_by.as_ref().map(|login|json!({"login":login})),
                    "comments":{"totalCount":t.comments.len(),"pageInfo":{"hasNextPage":false},"nodes":t.comments}
                })).collect::<Vec<_>>()});
        let value = json!({"data":{"viewer":{"databaseId":22},"repository":{"databaseId":100,"viewerPermission":o.permission,
            "viewerDefaultMergeMethod":o.method,"mergeCommitAllowed":true,"rebaseMergeAllowed":true,"squashMergeAllowed":true,"pullRequest":pull}}});
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}
impl MutationTransport for Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        let Request::Post(body) = request else {
            panic!("No deletion or bypass allowed")
        };
        let mut server = self.0.lock().unwrap();
        server.writes.push((path.into(), body.clone()));
        if server.reject {
            return Ok(Response {
                status: 403,
                headers: BTreeMap::new(),
                body: b"{}".to_vec(),
            });
        }
        let value = if path == "/graphql" {
            let input = &body["variables"]["input"];
            assert_eq!(input["expectedHeadOid"], server.observation.head);
            assert_eq!(input["mergeMethod"], "SQUASH");
            assert!(input.get("admin").is_none());
            server.observation.state = "MERGED".into();
            server.observation.merged_by = Some("22".into());
            server.observation.merged_at = Some("2026-09-30T00:02:00Z".into());
            server.observation.merge_commit = Some("c".repeat(40));
            json!({"data":{"mergePullRequest":{"clientMutationId":input["clientMutationId"],"pullRequest":{"id":"PR_node","headRefOid":server.observation.head,
                "merged":true,"mergedAt":"2026-09-30T00:02:00Z","mergedBy":{"databaseId":22},"mergeCommit":{"oid":"c".repeat(40)}}}}})
        } else {
            assert_eq!(path, "/repos/example/repo/pulls/1/reviews");
            assert_eq!(body["event"], "APPROVE");
            assert_eq!(body["commit_id"], server.observation.head);
            let r = ProviderReview {
                id: "101".into(),
                actor_id: "22".into(),
                head: server.observation.head.clone(),
                state: "APPROVED".into(),
                body: body["body"].as_str().unwrap().into(),
                submitted_at: Some("2026-09-30T00:02:00Z".into()),
            };
            server.observation.reviews.push(r.clone());
            json!({"id":101,"user":{"id":22},"commit_id":r.head,"state":"APPROVED","body":r.body,"submitted_at":r.submitted_at})
        };
        if server.lost {
            server.lost = false;
            return Err(ConnectionError::Timeout);
        }
        Ok(Response {
            status: if path == "/graphql" { 200 } else { 201 },
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}

struct Env<'a> {
    store: &'a Store,
    wire: Wire,
    now: i64,
    cancel_after_intent: bool,
    pause_after_intent: bool,
    fail_save: bool,
}
impl host::ActionEnvironment for Env<'_> {
    fn observe(&mut self, _: &Effect) -> Result<Observation, Failure> {
        let mut o = GithubClient::new(self.wire.clone()).action_observation(
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            1,
        )?;
        o.write_capability = true;
        Ok(o)
    }
    fn authorize(&mut self, effect: &Effect, current: &Observation) -> Result<(), Failure> {
        crate::capacity::publication_gate(self.store).map_err(Failure::permanent)?;
        let ledger = self.store.load_actions().map_err(Failure::permanent)?;
        if ledger
            .effects
            .iter()
            .any(|e| e.id == effect.id && e.cancelled)
        {
            return Err(Failure::permanent("Cancelled."));
        }
        ready(
            self.store,
            ledger
                .finals
                .iter()
                .find(|f| f.id == effect.final_id)
                .unwrap(),
            current,
            effect.action,
        )
        .map_err(Failure::permanent)
    }
    fn intent(&mut self, id: &str) -> Result<Effect, Failure> {
        let effect = mark_intent(self.store, id, self.now).map_err(Failure::permanent)?;
        if self.cancel_after_intent {
            cancel_effect(self.store, id).unwrap();
        }
        if self.pause_after_intent {
            self.store
                .save_automation(&crate::capacity::Automation { paused: true })
                .unwrap();
        }
        Ok(effect)
    }
    fn mutate(&mut self, effect: &Effect) -> Result<Receipt, crate::publication::WriteFailure> {
        assert_eq!(
            self.store
                .load_actions()
                .unwrap()
                .effects
                .last()
                .unwrap()
                .state,
            EffectState::Uncertain
        );
        let client = GithubClient::new(self.wire.clone());
        match effect.action {
            Action::Approve => client.approve_exact(
                &RemoteRepository {
                    id: "100".into(),
                    name: "example/repo".into(),
                },
                1,
                "22",
                &effect.observation.head,
                &effect.body,
            ),
            Action::Merge => client.merge_exact(&effect.observation, &effect.id),
        }
    }
    fn finish(
        &mut self,
        effect: &Effect,
        result: Result<Receipt, crate::publication::WriteFailure>,
    ) -> Result<(), Failure> {
        if self.fail_save {
            return Err(Failure::permanent("Injected receipt save failure."));
        }
        finish_effect(self.store, &effect.id, &effect.operation.id, result)
            .map_err(Failure::permanent)
    }
    fn post_check(&mut self, effect: &Effect) -> Result<(), Failure> {
        let current = self.observe(effect);
        verify_after_effect(self.store, effect, current)
    }
}
fn env(store: &Store) -> Env<'_> {
    Env {
        store,
        wire: Wire(Arc::new(Mutex::new(Server {
            observation: observed(),
            writes: vec![],
            lost: false,
            reject: false,
        }))),
        now: NOW + 20,
        cancel_after_intent: false,
        pause_after_intent: false,
        fail_save: false,
    }
}

#[test]
fn actual_action_pipeline_freezes_intent_approves_once_and_reuses_final_for_expected_head_merge() {
    let (_root, store, item) = fixture(3, true, true);
    let final_review = completed_final(&store, &item);
    let effect = prepare_effect(
        &store,
        &final_review.id,
        Action::Approve,
        &observed(),
        NOW + 20,
    )
    .unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &effect).unwrap();
    let approved = store.load_actions().unwrap().effects[0].clone();
    assert_eq!(approved.state, EffectState::Confirmed);
    assert_eq!(approved.receipt.unwrap().actor_id, "22");
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].state, State::MachineSignedOff);
    assert!(snapshot.items[0]
        .action_status
        .as_ref()
        .unwrap()
        .personal_review
        .contains("Required"));
    let current = env.wire.0.lock().unwrap().observation.clone();
    synchronize(&store, &item, Ok(current.clone()), NOW + 21).unwrap();
    assert_eq!(store.load_actions().unwrap().finals.len(), 1);
    let merge =
        prepare_effect(&store, &final_review.id, Action::Merge, &current, NOW + 22).unwrap();
    env.now = NOW + 22;
    host::execute_action(&mut env, &merge).unwrap();
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 2);
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::Merged
    );
    assert_eq!(
        store.load_queue_state().unwrap().tracked[0].lifecycle,
        Lifecycle::Merged
    );
    assert!(prepare_effect(
        &store,
        &final_review.id,
        Action::Approve,
        &observed(),
        NOW + 23
    )
    .is_err());
}

#[test]
fn lost_responses_crash_before_receipt_and_external_merge_never_blindly_repeat() {
    for action in [Action::Approve, Action::Merge] {
        for crash in [false, true] {
            let (_root, store, item) = fixture(1, true, true);
            let run = completed_final(&store, &item);
            let effect = prepare_effect(&store, &run.id, action, &observed(), NOW + 20).unwrap();
            let mut env = env(&store);
            env.wire.0.lock().unwrap().lost = !crash;
            env.fail_save = crash;
            let result = host::execute_action(&mut env, &effect);
            assert_eq!(result.is_err(), crash);
            assert_eq!(
                store.load_actions().unwrap().effects[0].state,
                EffectState::Uncertain
            );
            restore(&store).unwrap();
            let current = env.wire.0.lock().unwrap().observation.clone();
            synchronize(&store, &item, Ok(current), NOW + 30).unwrap();
            let saved = store.load_actions().unwrap().effects.remove(0);
            assert_eq!(saved.id, effect.id);
            if action == Action::Approve {
                assert_eq!(saved.state, EffectState::Confirmed);
                assert!(saved.receipt.is_some());
            } else {
                assert_eq!(saved.state, EffectState::ExternalMerge);
                assert!(saved.receipt.is_none());
            }
            assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1);
            assert!(host::execute_action(&mut env, &saved).is_err());
        }
    }
}

#[test]
fn pause_cancellation_and_permission_rejection_preserve_original_effect_without_writes_or_bypass() {
    for case in ["pause", "cancel", "reject"] {
        let (_root, store, item) = fixture(1, true, false);
        let run = completed_final(&store, &item);
        let effect =
            prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
        let mut env = env(&store);
        env.pause_after_intent = case == "pause";
        env.cancel_after_intent = case == "cancel";
        env.wire.0.lock().unwrap().reject = case == "reject";
        host::execute_action(&mut env, &effect).unwrap();
        let saved = store.load_actions().unwrap().effects.remove(0);
        assert_eq!(saved.state, EffectState::Rejected);
        assert!(saved.receipt.is_none());
        assert!(saved.error.is_some());
        assert_eq!(
            env.wire.0.lock().unwrap().writes.len(),
            usize::from(case == "reject")
        );
        assert!(prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 30).is_err());
    }
}

#[test]
fn final_uses_shared_pause_budget_and_stale_workers_cannot_overwrite_new_attempts() {
    let (_root, store, item) = fixture(1, true, false);
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let order = store.load_actions().unwrap().finals[0].enqueue_order;
    let coordinator = Capacity::default();
    let Dispatch::Review(run, token) = coordinator
        .dispatch(&store, NOW + 11)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Final expected")
    };
    store
        .save_automation(&crate::capacity::Automation { paused: true })
        .unwrap();
    coordinator.dispatch(&store, NOW + 12).unwrap();
    assert!(token.load(std::sync::atomic::Ordering::SeqCst));
    host::complete(&store, &run, Err(Failure::cancelled()), NOW + 13).unwrap();
    let saved = store.load_actions().unwrap().finals.remove(0);
    assert_eq!(saved.execution.operation.attempt_count, 0);
    assert_eq!(saved.enqueue_order, order);
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0].execution.operation.id = "replacement-attempt".into();
    store.save_actions(&ledger).unwrap();
    assert!(host::complete(&store, &run, Ok(output()), NOW + 14).is_err());
    assert!(store.load_actions().unwrap().finals[0]
        .execution
        .result
        .is_none());
}

#[test]
fn mutation_owner_serializes_actions_with_review_and_reply_publications() {
    let owner = host::MutationOwner::default();
    assert!(owner.acquire("comment").unwrap());
    assert!(!owner.acquire("approve").unwrap());
    assert!(!owner.acquire("reply").unwrap());
    assert!(owner.release("foreign").is_err());
    owner.release("comment").unwrap();
    assert!(owner.acquire("merge").unwrap());
    owner.release("merge").unwrap();
}

#[test]
fn current_role_account_feedback_and_comment_changes_invalidate_final_evidence() {
    for case in [
        "role",
        "permission",
        "account",
        "replace",
        "reply",
        "admission_pending",
        "provider_comment",
        "repository_prompt",
        "scope_version",
    ] {
        let (_root, store, item) = fixture(2, true, true);
        let final_review = completed_final(&store, &item);
        let mut settings = store.load_settings().unwrap();
        match case {
            "role" => {
                settings.repositories[0].primary_assignment_id =
                    Some(settings.repositories[0].assignments[1].id.clone())
            }
            "permission" => {
                settings.repositories[0].assignments[0]
                    .actions
                    .as_mut()
                    .unwrap()
                    .merge = false
            }
            "account" => settings.repositories[0].provider_account_id = Some("44".into()),
            "replace" => {
                settings.repositories[0].assignments[0].agent_id = settings.agents[1].id.clone()
            }
            "repository_prompt" => {
                settings.repositories[0].overrides.prompt =
                    Some("A materially different review lens.".into())
            }
            "scope_version" => {
                let mut monitor = store.load_monitoring_state().unwrap();
                monitor.activations.get_mut(REPO).unwrap().version = "reconfirmed-scope".into();
                store.save_monitoring_state(&monitor).unwrap();
            }
            "reply" => {
                let run = crate::follow_up::FollowUp::mention(
                    crate::github::conversation::TopComment {
                        id: "701".into(),
                        body: "@actor new concern".into(),
                        author_id: Some("11".into()),
                        author_login: Some("author".into()),
                        created_at: "2026-09-30T00:00:00Z".into(),
                        updated_at: "2026-09-30T00:00:00Z".into(),
                    },
                    crate::follow_up::ConversationContext {
                        assignment_id: final_review.execution.assignment_id.clone(),
                        job: final_review.basis.job.clone(),
                        selection: final_review.basis.selection.clone(),
                        trust_confirmed: true,
                        feedback: vec![],
                        feedback_checked: true,
                    },
                );
                store.save_follow_ups(&[run]).unwrap();
            }
            "admission_pending" => {
                let mut state = store.load_monitoring_state().unwrap();
                state
                    .health
                    .get_mut(REPO)
                    .unwrap()
                    .conversation_admission_pending = true;
                store.save_monitoring_state(&state).unwrap();
            }
            _ => {}
        }
        store.save_settings(&settings).unwrap();
        let mut observation = observed();
        if case == "provider_comment" {
            observation
                .comments
                .push(crate::github::conversation::TopComment {
                    id: "702".into(),
                    body: "A new human question.".into(),
                    author_id: Some("11".into()),
                    author_login: Some("author".into()),
                    created_at: "2026-09-30T00:03:00Z".into(),
                    updated_at: "2026-09-30T00:03:00Z".into(),
                });
        }
        assert!(
            ready(&store, &final_review, &observation, Action::Approve).is_err(),
            "{case}"
        );
        assert!(store.load_actions().unwrap().effects.is_empty());
    }
}

#[test]
fn an_existing_account_approval_is_not_a_reason_to_submit_another_vote() {
    let (_root, store, item) = fixture(1, true, false);
    let mut observation = observed();
    observation.reviews.push(ProviderReview {
        id: "100".into(),
        actor_id: "22".into(),
        head: observation.head.clone(),
        state: "APPROVED".into(),
        body: "Existing account vote.".into(),
        submitted_at: Some("2026-09-30T00:00:00Z".into()),
    });
    synchronize(&store, &item, Ok(observation.clone()), NOW + 10).unwrap();
    let id = store.load_actions().unwrap().finals[0].id.clone();
    let execution = host::prepare_dispatch(&store, &id, NOW + 11).unwrap();
    host::complete(&store, &execution, Ok(output()), NOW + 12).unwrap();
    let run = store.load_actions().unwrap().finals.remove(0);
    assert!(ready(&store, &run, &observation, Action::Approve)
        .unwrap_err()
        .contains("already has an approval"));
}

struct ShapeWire {
    wire: Wire,
    case: &'static str,
}
impl Transport for ShapeWire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        if path.contains("/rules/branches/") {
            if self.case == "rules_unavailable" {
                return Err(ConnectionError::MissingReadPermission);
            }
            if self.case == "queue_rule" {
                return Ok(Response {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: serde_json::to_vec(&json!([{"type":"merge_queue"}])).unwrap(),
                });
            }
        }
        self.wire.get(path)
    }
}
impl QueryTransport for ShapeWire {
    fn query(&self, query: &str, variables: Value) -> Result<Response, ConnectionError> {
        let response = self.wire.query(query, variables)?;
        let mut value: Value = serde_json::from_slice(&response.body).unwrap();
        let pull = &mut value["data"]["repository"]["pullRequest"];
        match self.case {
            "partial_threads" => pull["reviewThreads"]["pageInfo"]["hasNextPage"] = json!(true),
            "partial_checks" => {
                pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]
                    ["totalCount"] = json!(2)
            }
            "wrong_check_head" => {
                pull["commits"]["nodes"][0]["commit"]["oid"] = json!("d".repeat(40))
            }
            "missing_policy" => {
                pull["baseRef"]
                    .as_object_mut()
                    .unwrap()
                    .remove("branchProtectionRule");
            }
            "nonpassing" => {
                pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"][0]
                    ["conclusion"] = json!("FAILURE")
            }
            "no_checks" => {
                pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"] = Value::Null;
            }
            "errors" => value["errors"] = json!([{"message":"unsupported provider capability"}]),
            "queue_rule" | "rules_unavailable" => {}
            _ => panic!("Unknown fixture"),
        }
        Ok(Response {
            body: serde_json::to_vec(&value).unwrap(),
            ..response
        })
    }
}
#[test]
fn provider_gate_reads_fail_closed_on_partial_policy_and_ci_evidence() {
    let (_root, store, _) = fixture(1, true, true);
    let wire = env(&store).wire;
    for case in [
        "partial_threads",
        "partial_checks",
        "wrong_check_head",
        "missing_policy",
        "errors",
        "nonpassing",
        "no_checks",
        "queue_rule",
        "rules_unavailable",
    ] {
        let result = GithubClient::new(ShapeWire {
            wire: wire.clone(),
            case,
        })
        .action_observation(
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            1,
        );
        if matches!(
            case,
            "nonpassing" | "no_checks" | "queue_rule" | "rules_unavailable"
        ) {
            let mut observation = result.unwrap();
            observation.write_capability = true;
            assert!(observation.blocker(Action::Merge).is_some());
            assert!(observation.blocker(Action::Approve).is_none());
        } else {
            assert!(result.is_err(), "{case}");
        }
    }
    assert!(wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn provider_receipts_produce_distinct_approval_and_merge_notifications_without_human_review_claims()
{
    let (_root, store, item) = fixture(1, true, true);
    let run = completed_final(&store, &item);
    let approval = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &approval).unwrap();
    let frames = crate::notifications::frames(&queue::snapshot(&store, vec![]).unwrap());
    assert!(frames.iter().any(|f| f
        .event
        .as_ref()
        .is_some_and(|e| e.category == crate::notifications::Category::Approved)));
    let mut external = env.wire.0.lock().unwrap().observation.clone();
    external.state = "MERGED".into();
    external.merged_by = Some("77".into());
    external.merged_at = Some("2026-09-30T00:04:00Z".into());
    external.merge_commit = Some("c".repeat(40));
    synchronize(&store, &item, Ok(external), NOW + 30).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].state, State::Merged);
    assert!(snapshot.items[0]
        .action_status
        .as_ref()
        .unwrap()
        .effects
        .iter()
        .all(|e| e.action != Action::Merge));
    assert!(crate::notifications::frames(&snapshot).iter().any(|f| f
        .event
        .as_ref()
        .is_some_and(|e| e.category == crate::notifications::Category::Merged)));
}

#[test]
fn wrong_provider_identity_cannot_create_a_final_or_mark_another_pr_merged() {
    let (_root, store, item) = fixture(1, true, true);
    for wrong in ["account", "repository", "pull"] {
        let mut current = observed();
        current.state = "MERGED".into();
        match wrong {
            "account" => current.account_id = "44".into(),
            "repository" => current.repository_id = "200".into(),
            _ => current.pull_request_id = "99".into(),
        }
        synchronize(&store, &item, Ok(current), NOW + 10).unwrap();
        let ledger = store.load_actions().unwrap();
        assert!(ledger.observations[0].error.is_some());
        assert!(ledger.finals.is_empty());
        assert!(ledger.effects.is_empty());
        assert_eq!(
            store.load_queue_state().unwrap().tracked[0].lifecycle,
            Lifecycle::Open
        );
        assert_ne!(
            queue::snapshot(&store, vec![]).unwrap().items[0].state,
            State::Merged
        );
    }
}

#[test]
fn observation_retry_after_and_original_mutation_ownership_survive_retry_requests() {
    let (_root, store, item) = fixture(1, true, false);
    let run = completed_final(&store, &item);
    let effect = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    let intent = mark_intent(&store, &effect.id, NOW + 20).unwrap();
    let error = Failure {
        cancelled: false,
        kind: monitoring::OperationFailure::RateLimited,
        message: "Provider read rate limited.".into(),
        retry_after_seconds: Some(60),
    };
    synchronize(&store, &item, Err(error.clone()), NOW + 21).unwrap();
    assert_eq!(
        store.load_actions().unwrap().observations[0].retry_at,
        Some(NOW + 81)
    );
    assert!(request_observation_retry(&store, &item, NOW + 22).is_err());
    for i in 1..=3 {
        synchronize(&store, &item, Err(error.clone()), NOW + 21 + i * 60).unwrap();
    }
    assert_eq!(store.load_actions().unwrap().observations[0].retry_at, None);
    request_observation_retry(&store, &item, NOW + 300).unwrap();
    let saved = store.load_actions().unwrap().effects.remove(0);
    assert_eq!(saved, intent);
    assert!(finish_effect(
        &store,
        &effect.id,
        "obsolete-operation",
        Ok(Receipt {
            id: "101".into(),
            actor_id: "22".into(),
            head: "a".repeat(40),
            action: Action::Approve,
            merge_commit: None
        })
    )
    .is_err());
    assert_eq!(store.load_actions().unwrap().effects[0], intent);
}

#[test]
fn final_human_input_stays_a_blocker_instead_of_rerolling_after_an_unrelated_comment() {
    let (_root, store, item) = fixture(1, true, true);
    let mut final_review = completed_final(&store, &item);
    final_review
        .execution
        .result
        .as_mut()
        .unwrap()
        .output
        .decision = crate::review::Decision::HumanInputRequired;
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0] = final_review;
    store.save_actions(&ledger).unwrap();
    let mut current = observed();
    current
        .comments
        .push(crate::github::conversation::TopComment {
            id: "2".into(),
            body: "Another comment, not a code fix.".into(),
            author_id: Some("11".into()),
            author_login: Some("author".into()),
            created_at: "2026-09-30T00:03:00Z".into(),
            updated_at: "2026-09-30T00:03:00Z".into(),
        });
    synchronize(&store, &item, Ok(current), NOW + 30).unwrap();
    assert_eq!(store.load_actions().unwrap().finals.len(), 1);
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::WaitingForHuman
    );
}

#[test]
fn a_self_authored_pr_can_use_merge_only_but_never_automatic_self_approval() {
    let (_root, store, item) = fixture(1, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors[0].id = "22".into();
    store.save_settings(&settings).unwrap();
    let mut jobs = store.load_queue().unwrap();
    jobs[0].author_id = Some("22".into());
    store.save_queue(&jobs).unwrap();
    let mut observation = observed();
    observation.author_id = "22".into();
    synchronize(&store, &item, Ok(observation.clone()), NOW + 10).unwrap();
    let id = store.load_actions().unwrap().finals[0].id.clone();
    let execution = host::prepare_dispatch(&store, &id, NOW + 11).unwrap();
    host::complete(&store, &execution, Ok(output()), NOW + 12).unwrap();
    let final_review = store.load_actions().unwrap().finals.remove(0);
    assert!(ready(&store, &final_review, &observation, Action::Approve)
        .unwrap_err()
        .contains("author"));
    assert!(ready(&store, &final_review, &observation, Action::Merge).is_ok());
}

#[test]
fn a_post_approval_revision_change_retains_the_vote_but_blocks_merge() {
    let (_root, store, item) = fixture(1, true, true);
    let run = completed_final(&store, &item);
    let effect = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &effect).unwrap();
    let receipt = store.load_actions().unwrap().effects[0].receipt.clone();
    let mut current = env.wire.0.lock().unwrap().observation.clone();
    current.base = "d".repeat(40);
    assert!(verify_after_effect(&store, &effect, Ok(current.clone())).is_err());
    let saved = store.load_actions().unwrap().effects.remove(0);
    assert_eq!(saved.state, EffectState::Confirmed);
    assert_eq!(saved.receipt, receipt);
    assert!(saved.error.unwrap().contains("post-action"));
    assert!(ready(&store, &run, &current, Action::Merge).is_err());
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1);
}

#[test]
fn legacy_inert_flags_and_expired_unattempted_intents_never_become_provider_grants() {
    let (_root, store, item) = fixture(1, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].actions = None;
    assert!(settings.repositories[0].assignments[0].approve);
    store.save_settings(&settings).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0]
            .action_status
            .as_ref()
            .unwrap()
            .permissions,
        ActionPermissions {
            approve: false,
            merge: false
        }
    );
    settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
        approve: true,
        merge: false,
    });
    store.save_settings(&settings).unwrap();
    let run = completed_final(&store, &item);
    let intent = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    expire_prepared(&store, intent.operation.retry_deadline).unwrap();
    let expired = store.load_actions().unwrap().effects.remove(0);
    assert_eq!(expired.state, EffectState::Stale);
    assert!(expired.operation.attempted_mutation.is_none());
    assert!(mark_intent(&store, &expired.id, NOW + 1000).is_err());
}
