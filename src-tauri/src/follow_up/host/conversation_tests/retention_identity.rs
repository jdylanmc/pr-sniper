use super::*;
use crate::retention::{page, PageRequest};

fn results(store: &Store) -> Result<crate::retention::Page, String> {
    page(
        store,
        PageRequest {
            limit: 200,
            cursor: None,
        },
    )
}

fn close_pr(store: &Store, now: i64) {
    let mut monitor = Monitor::restore(store).unwrap();
    let ticket = monitor.prepare_checks(store, now, true).unwrap().remove(0);
    let mut closed = pull('a');
    closed.head_sha = store.load_queue_state().unwrap().tracked[0]
        .head_sha
        .clone();
    closed.state = Lifecycle::Closed;
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
                pull_requests: vec![closed],
            }),
            now + 1,
        )
        .unwrap();
}

fn add_agent(store: &Store) {
    let mut settings = store.load_settings().unwrap();
    let agent = "aaaaaaaa-aaaa-4aaa-8aaa-000000000001";
    settings.agents.push(
        serde_json::from_value(json!({
            "id":agent,"name":"Agent 1","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"machine"
        }))
        .unwrap(),
    );
    settings.repositories[0].assignments.push(
        serde_json::from_value(json!({
            "id":"cccccccc-cccc-4ccc-8ccc-000000000001","agent_id":agent,
            "schedule":settings.defaults.schedule,"comment":true
        }))
        .unwrap(),
    );
    store.save_settings(&settings).unwrap();
}

#[test]
fn r7_tracked_without_agents_or_jobs_retains_intent_through_restart_and_assignment() {
    let (root, store) = tracking_fixture(0);
    assert!(store.load_queue().unwrap().is_empty());
    assert!(store.load_settings().unwrap().agents.is_empty());
    let ticket = poll(&store, 'a', NOW + 10);
    admit_scan(
        &store,
        &ticket,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 12,
    )
    .unwrap();
    let intent = store.load_feedback().unwrap().mentions.remove(0);
    let tracked = store.load_queue_state().unwrap().tracked.remove(0);
    assert_eq!(intent.item_id.as_ref(), Some(&tracked.item_id));
    assert!(intent.follow_up_id.is_none());
    assert!(store.load_follow_ups().unwrap().is_empty());
    assert!(results(&store)
        .unwrap_err()
        .contains("no review job exists"));
    let store = Store::new(root.path().into());
    Monitor::restore(&store).unwrap();
    assert_eq!(store.load_feedback().unwrap().mentions[0], intent);
    add_agent(&store);
    let ticket = poll(&store, 'a', NOW + 20);
    admit_scan(&store, &ticket, Scan::default(), NOW + 22).unwrap();
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, intent.work_id);
    assert_eq!(runs[0].key, intent.key);
    assert_eq!(runs[0].enqueue_order, Some(intent.enqueue_order));
    assert_eq!(runs[0].enqueued_at, Some(intent.enqueued_at));
    assert_eq!(crate::queue::item_id(&runs[0].context.job), tracked.item_id);
    assert!(
        store.load_reviews().unwrap().is_empty(),
        "No synthetic review"
    );
    let rows = results(&store).unwrap().results;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].conversations, 1);
    assert_eq!(rows[0].review_attempts, 0);
    admit_scan(
        &store,
        &ticket,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 23,
    )
    .unwrap();
    assert_eq!(store.load_follow_ups().unwrap(), runs);
}

#[test]
fn r7_new_mention_uses_advanced_tracking_not_removed_assignments_historical_jobs() {
    let (root, store, origin, thread) = cleared_fixture();
    let saved = store.load_settings().unwrap();
    let old_reviews = store.load_reviews().unwrap();
    let old_item = crate::queue::item_id(&old_reviews[0].job);
    let mut removed = saved.clone();
    removed.repositories[0].assignments.clear();
    removed.agents.clear();
    store.save_settings(&removed).unwrap();
    observe(
        &store,
        &origin,
        &thread,
        'c',
        vec![mention("501", "@actor explain new revision")],
        NOW + 20,
    );
    let intent = store.load_feedback().unwrap().mentions.remove(0);
    let tracked = store.load_queue_state().unwrap().tracked.remove(0);
    assert_eq!(tracked.iteration, 2);
    assert_eq!(intent.item_id.as_ref(), Some(&tracked.item_id));
    assert_ne!(intent.item_id.as_ref(), Some(&old_item));
    assert!(store
        .load_queue()
        .unwrap()
        .iter()
        .all(|j| crate::queue::item_id(j) == old_item));
    assert!(store.load_follow_ups().unwrap().is_empty());
    assert!(results(&store)
        .unwrap_err()
        .contains("no review job exists"));
    let store = Store::new(root.path().into());
    Monitor::restore(&store).unwrap();
    store.save_settings(&saved).unwrap();
    let ticket = poll(&store, 'c', NOW + 30);
    admit_scan(&store, &ticket, Scan::default(), NOW + 32).unwrap();
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, intent.work_id);
    assert_eq!(runs[0].enqueue_order, Some(intent.enqueue_order));
    assert_eq!(crate::queue::item_id(&runs[0].context.job), tracked.item_id);
    assert_eq!(runs[0].context.job.head_sha, "c".repeat(40));
    assert_eq!(store.load_reviews().unwrap(), old_reviews);
    assert_eq!(
        results(&store)
            .unwrap()
            .results
            .iter()
            .map(|r| r.conversations)
            .sum::<usize>(),
        1
    );
}

fn clear_current_passes(store: &Store, now: i64) {
    let capacity = Capacity::default();
    let batch = capacity.dispatch(store, now).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 2);
    for work in batch.dispatched {
        let key = work.key();
        let Dispatch::Review(run, _) = work else {
            panic!("Only current normal passes may start");
        };
        assert_eq!(run.job.work.as_ref().unwrap().iteration, 2);
        let task = FullReview {
            final_context: None,
            feedback: run.feedback_context.clone().unwrap(),
            owner_agent_id: run.selection.agent.id.clone(),
        };
        let output = task
            .validate(
                &json!({"synopsis":"Current revision is clear.",
                "files":[{"path":"source.rs","explanation":"Source.","order":1}],
                "findings":[],"decision":"machine_sign_off","feedback_assessments":[]})
                .to_string(),
                &source_context('c'),
                &GithubClient::new(Source),
                "example/repo",
            )
            .unwrap();
        crate::review::host::complete(
            store,
            &run.operation.id,
            Ok(crate::review::ReviewResult {
                reviewed_base_sha: Some("b".repeat(40)),
                output,
                session_id: "fixture".into(),
                model: "model".into(),
                runtime_version: "fixture".into(),
                input_tokens: 1,
                output_tokens: 1,
                tool_calls: 1,
            }),
            now + 1,
        )
        .unwrap();
        capacity.release(&key, &run.operation.id).unwrap();
    }
}

#[test]
fn r8_superseded_intent_does_not_block_current_handoff_but_uncertainty_and_humans_do() {
    for prior in ["unstarted", "uncertain", "missing-execution", "human-input"] {
        let (root, store) = tracking_fixture(2);
        let mut settings = store.load_settings().unwrap();
        settings.defaults.automatic_comment_publication = false;
        if prior != "unstarted" {
            settings.repositories[0].primary_assignment_id =
                Some(settings.repositories[0].assignments[0].id.clone());
        }
        store.save_settings(&settings).unwrap();
        let ticket = poll(&store, 'a', NOW + 10);
        admit_scan(
            &store,
            &ticket,
            mention_scan(vec![mention("501", "@actor explain")]),
            NOW + 12,
        )
        .unwrap();
        let intent = store.load_feedback().unwrap().mentions.remove(0);
        let mut runs = store.load_follow_ups().unwrap();
        if prior == "uncertain" {
            runs[0].publication = Some(runs[0].operation("mention_reply", NOW + 13));
            runs[0].publication.as_mut().unwrap().attempted_mutation = Some("mention_reply".into());
            runs[0].uncertain = true;
            runs[0].phase = Phase::Unresolved;
            runs[0].body = Some("Frozen uncertain publication.".into());
            store.save_follow_ups(&runs).unwrap();
        } else if prior == "missing-execution" {
            runs.clear();
            store.save_follow_ups(&runs).unwrap();
        } else if prior == "human-input" {
            runs[0].phase = Phase::HumanInputRequired;
            store.save_follow_ups(&runs).unwrap();
        } else {
            assert!(runs.is_empty());
        }
        poll(&store, 'c', NOW + 20);
        settings.repositories[0].primary_assignment_id =
            Some(settings.repositories[0].assignments[0].id.clone());
        store.save_settings(&settings).unwrap();
        let ticket = poll(&store, 'c', NOW + 30);
        admit_scan(&store, &ticket, Scan::default(), NOW + 32).unwrap();
        clear_current_passes(&store, NOW + 40);
        let store = Store::new(root.path().into());
        let current = store.load_queue_state().unwrap().tracked.remove(0).item_id;
        assert_ne!(Some(&current), intent.item_id.as_ref());
        let snapshot = crate::queue::snapshot(&store, vec![]).unwrap();
        let item = snapshot.items.iter().find(|i| i.id == current).unwrap();
        assert_eq!(
            item.state == crate::queue::State::MachineSignedOff,
            prior == "unstarted",
            "{prior}"
        );
        assert_eq!(
            crate::actions::basis(&store, &current).is_ok(),
            prior == "unstarted",
            "{prior}"
        );
        assert_eq!(
            store.load_follow_ups().unwrap(),
            runs,
            "No prior-iteration replay: {prior}"
        );
        let saved = store.load_feedback().unwrap().mentions.remove(0);
        assert_eq!(saved.item_id, intent.item_id);
        assert_eq!(saved.key, intent.key);
        assert_eq!(saved.work_id, intent.work_id);
        assert_eq!(saved.enqueue_order, intent.enqueue_order);
        if matches!(prior, "uncertain" | "missing-execution") {
            close_pr(&store, NOW + 50);
            let cleanup = crate::retention::maintain(&store, true);
            if prior == "uncertain" {
                assert_eq!(cleanup.unwrap(), 0);
            } else {
                assert!(cleanup.unwrap_err().contains("original mention execution"));
            }
            assert!(crate::retention::load(&store).unwrap().receipts.is_empty());
            assert_eq!(store.load_feedback().unwrap().mentions[0], saved);
            assert_eq!(store.load_follow_ups().unwrap(), runs);
        }
    }
}

fn baseline_mention(store: &Store) -> Value {
    let binding = mention_scan(vec![]).mentions.remove(0).0;
    let order = store.allocate_enqueue_order().unwrap();
    // Exact baseline schema: blocked/unlinked intent has neither item_id nor a
    // revision field. A current serializer with a removed field is not the fixture.
    json!({"records":[],"mentions":[{
        "key":binding.key("501"),"work_id":"dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        "enqueue_order":order,"enqueued_at":NOW+12,"binding":binding,
        "comment":mention("501","@actor explain"),"follow_up_id":null,
        "blocked":"No primary assigned; mention retained without fallback."
    }]})
}

#[test]
fn r5_baseline_unlinked_mention_indexes_once_and_pins_before_failed_routing() {
    let (_root, store) = tracking_fixture(2);
    let raw = baseline_mention(&store);
    store.write_state("feedback.json", &raw).unwrap();
    // Upgrade an already-materialized pre-fix index, not just an empty store.
    let mut index: Value =
        serde_json::from_slice(&store.read_state("result-index.json").unwrap()).unwrap();
    index.as_object_mut().unwrap().remove("version");
    index.as_object_mut().unwrap().remove("unavailable");
    store.write_state("result-index.json", &index).unwrap();
    let before = serde_json::to_value(results(&store).unwrap()).unwrap();
    assert_eq!(before["results"][0]["conversations"], 1);
    let sequence = before["results"][0]["activity_sequence"].clone();
    for now in [NOW + 20, NOW + 30] {
        let ticket = poll(&store, 'a', now);
        admit_scan(
            &store,
            &ticket,
            mention_scan(vec![mention("501", "@actor explain")]),
            now + 2,
        )
        .unwrap();
        let intent = store.load_feedback().unwrap().mentions.remove(0);
        assert_eq!(
            intent.item_id,
            Some(store.load_queue_state().unwrap().tracked[0].item_id.clone())
        );
        assert!(intent.blocked.unwrap().contains("No primary"));
        assert_eq!(
            serde_json::to_value(results(&store).unwrap()).unwrap(),
            before
        );
        assert!(store.load_follow_ups().unwrap().is_empty());
    }
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    store.save_settings(&settings).unwrap();
    let ticket = poll(&store, 'a', NOW + 40);
    admit_scan(&store, &ticket, Scan::default(), NOW + 42).unwrap();
    assert_eq!(store.load_follow_ups().unwrap().len(), 1);
    let after = serde_json::to_value(results(&store).unwrap()).unwrap();
    assert_eq!(after["results"][0]["conversations"], 1);
    assert_eq!(after["results"][0]["activity_sequence"], sequence);
}

#[test]
fn r5_baseline_intervening_revision_is_explicitly_unavailable_never_latest_job() {
    let (root, store) = tracking_fixture(2);
    let raw = baseline_mention(&store);
    store.write_state("feedback.json", &raw).unwrap();
    let ticket = poll(&store, 'c', NOW + 20);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    store.save_settings(&settings).unwrap();
    let store = Store::new(root.path().into());
    let error = results(&store).unwrap_err();
    assert!(error.contains("Legacy mention iteration is unavailable"));
    let before = store.read_state("result-index.json").unwrap();
    for _ in 0..2 {
        admit_scan(
            &store,
            &ticket,
            mention_scan(vec![mention("501", "@actor explain")]),
            NOW + 22,
        )
        .unwrap();
        assert_eq!(results(&store).unwrap_err(), error);
        assert_eq!(store.read_state("result-index.json").unwrap(), before);
        let intent = store.load_feedback().unwrap().mentions.remove(0);
        assert!(intent.item_id.is_none());
        assert!(intent.follow_up_id.is_none());
        assert!(intent.blocked.unwrap().contains("Legacy mention iteration"));
        assert!(store.load_follow_ups().unwrap().is_empty());
    }
    let current = store.load_queue_state().unwrap().tracked[0].item_id.clone();
    assert!(crate::actions::basis(&store, &current).is_err());
}

#[test]
fn r7_no_job_cleanup_preserves_dedup_and_exact_unavailability_receipts() {
    let (root, store) = tracking_fixture(0);
    let ticket = poll(&store, 'a', NOW + 10);
    admit_scan(
        &store,
        &ticket,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 12,
    )
    .unwrap();
    let saved = store.load_feedback().unwrap();
    let intent = saved.mentions[0].clone();
    close_pr(&store, NOW + 20);
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 1);
    assert!(store.load_feedback().unwrap().mentions.is_empty());
    assert!(results(&store).unwrap().results.is_empty());
    let receipt = crate::retention::load(&store).unwrap().receipts.remove(0);
    assert!(receipt.follow_up_keys.contains(&intent.key));
    assert!(receipt.items.contains(intent.item_id.as_ref().unwrap()));
    assert!(receipt.work.contains(&crate::capacity::WorkId {
        kind: Kind::Mention,
        id: intent.work_id.clone()
    }));
    assert!(store
        .save_feedback(&saved)
        .unwrap_err()
        .contains("cleaned mention"));
    let store = Store::new(root.path().into());
    let ticket = poll(&store, 'a', NOW + 30);
    assert_eq!(store.load_queue_state().unwrap().tracked[0].iteration, 2);
    admit_scan(
        &store,
        &ticket,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 32,
    )
    .unwrap();
    assert!(store.load_feedback().unwrap().mentions.is_empty());
    assert!(store.load_follow_ups().unwrap().is_empty());
    assert!(matches!(
        crate::retention::detail(
            &store,
            crate::panel::Detail::Job {
                kind: Kind::Mention,
                id: intent.work_id
            }
        )
        .unwrap(),
        crate::retention::DetailResult::Cleaned { .. }
    ));
}
