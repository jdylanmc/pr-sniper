use super::*;
use crate::{
    follow_up::{FollowUp, Phase as FollowPhase},
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{Capabilities, CommentCapability, Connection, RemoteRepository},
        threads::{Comment, Thread},
        Identity,
    },
    monitoring::{
        ActivationMode, JobOperation, Monitor, MonitoringActivation, OperationState, PollResult,
    },
    publication::{Batch, InlineComment, Receipt as PublicationReceipt, RemoteState},
    review::{ReviewRun, Selection},
    storage::Settings,
};
use serde_json::json;

mod remediation;

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ASSIGNMENT: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const NOW: i64 = 1_800_000_000;

fn fixture() -> (tempfile::TempDir, Store, Monitor) {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/retention-delivery/fixtures");
    std::fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    let store = Store::new(root.path().into());
    let settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,"doctrines":[],
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,
                "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"fixture"}]
    })).unwrap();
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "fixture".into(),
            repository_id: REPO.into(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: 0,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    scan(&store, &mut monitor, vec![pull(1, Lifecycle::Open)], NOW);
    (root, store, monitor)
}

fn pull(number: u64, state: Lifecycle) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: number.to_string(),
        number,
        title: "Synthetic retention fixture".into(),
        author: Some(Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: Vec::new(),
        requested_teams: Vec::new(),
        state,
        draft: false,
        head_sha: "a".repeat(40),
        base_sha: "b".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-10-02T00:00:00Z".into(),
        files: Vec::new(),
    }
}

fn scan(store: &Store, monitor: &mut Monitor, pulls: Vec<PullRequest>, now: i64) {
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
                pull_requests: pulls,
            }),
            now + 1,
        )
        .unwrap();
}

fn completed(store: &Store, job: &QueueJob) -> ReviewRun {
    let mut operation = JobOperation::review(job, NOW);
    operation.state = OperationState::Completed;
    ReviewRun {
        feedback_context: None, key: crate::review::key(job, ASSIGNMENT), assignment_id: ASSIGNMENT.into(),
        job: job.clone(), selection: Selection::resolve(&store.load_settings().unwrap(), job, ASSIGNMENT).unwrap(),
        operation, manual_start: true, trust_confirmed: true, phase: "Complete".into(), error: None,
        result: Some(serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),
            "output":{"synopsis":"BULKY-SYNOPSIS", "files":[{"path":"source.rs","explanation":"BULKY-GUIDE","order":1}],
                "findings":[{"path":"source.rs","side":"RIGHT","line":1,"severity":"high","title":"Concern",
                    "explanation":"BULKY-FINDING","confidence":99}], "decision":"human_input_required"},
            "session_id":"BULKY-SESSION","model":"model","runtime_version":"fixture",
            "input_tokens":1,"output_tokens":1,"tool_calls":1
        })).unwrap()),
    }
}

fn evidence(store: &Store) -> (ReviewRun, Publication, FollowUp) {
    let review = completed(store, &store.load_queue().unwrap()[0]);
    store.save_reviews(std::slice::from_ref(&review)).unwrap();
    let mut publication = Publication::new(review.clone(), false, true, NOW).unwrap();
    publication.batch = Some(Batch {
        commit_id: review.job.head_sha.clone(),
        body: "BULKY-SUMMARY".into(),
        comments: vec![InlineComment {
            path: "source.rs".into(),
            line: 1,
            side: "RIGHT".into(),
            body: "Concern: BULKY-ROOT".into(),
        }],
        unmappable: Vec::new(),
    });
    publication.operation.state = OperationState::Completed;
    publication.phase = crate::publication::Phase::Published;
    publication.receipts.push(PublicationReceipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["100".into()],
    });
    store
        .save_publications(std::slice::from_ref(&publication))
        .unwrap();
    let mut follow = FollowUp::new(&publication, thread()).unwrap();
    follow.phase = FollowPhase::Quiet;
    follow.analysis = Some(follow.operation("reply_analysis", NOW));
    follow.analysis.as_mut().unwrap().state = OperationState::Completed;
    follow.enqueue_order = Some(store.allocate_enqueue_order().unwrap());
    follow.reply_ordinal = Some(1);
    store
        .save_follow_ups(std::slice::from_ref(&follow))
        .unwrap();
    let mut feedback = store.load_feedback().unwrap();
    feedback
        .observe(&publication, &review.job.head_sha, &[thread()])
        .unwrap();
    store.save_feedback(&feedback).unwrap();
    (review, publication, follow)
}

fn thread() -> Thread {
    let comment = |id: &str, body: &str, parent: Option<&str>| Comment {
        id: id.into(),
        body: body.into(),
        author_id: Some(if parent.is_none() { "22" } else { "11" }.into()),
        author_login: Some("fixture".into()),
        reply_to: parent.map(String::from),
        review_id: Some("42".into()),
        original_commit: Some("a".repeat(40)),
        created_at: "2026-10-02T00:00:00Z".into(),
        published_at: "2026-10-02T00:00:00Z".into(),
    };
    Thread {
        id: "root-thread".into(),
        resolved: false,
        can_reply: true,
        comments: vec![
            comment("100", "Concern: BULKY-ROOT", None),
            comment("101", "BULKY-EXTERNAL", Some("100")),
        ],
    }
}

#[test]
fn thousands_of_open_results_have_bounded_stable_tie_pages_and_no_poll_duplicates() {
    let (_root, store, mut monitor) = fixture();
    scan(
        &store,
        &mut monitor,
        (1..=3007).map(|n| pull(n, Lifecycle::Open)).collect(),
        NOW + 5,
    );
    let jobs = store.load_queue().unwrap();
    let reviews: Vec<_> = jobs.iter().map(|j| completed(&store, j)).collect();
    store.save_reviews(&reviews).unwrap();
    let before = serde_json::to_value(
        page(
            &store,
            PageRequest {
                limit: 37,
                cursor: None,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap();
    scan(
        &store,
        &mut monitor,
        (1..=3007).rev().map(|n| pull(n, Lifecycle::Open)).collect(),
        NOW + 10,
    );
    let restarted = Store::new(_root.path().into());
    assert_eq!(
        before,
        serde_json::to_value(
            page(
                &restarted,
                PageRequest {
                    limit: 37,
                    cursor: None,
                    ..Default::default()
                }
            )
            .unwrap()
        )
        .unwrap()
    );
    let mut cursor = None;
    let mut found = BTreeSet::new();
    loop {
        let result = page(
            &store,
            PageRequest {
                limit: 37,
                cursor,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.results.len() <= 37);
        for row in &result.results {
            assert_eq!(row.completed_passes, 1);
            assert!(found.insert(row.item_id.clone()));
        }
        cursor = result.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(found.len(), 3007);
    assert_eq!(store.load_reviews().unwrap(), reviews);
    assert!(page(
        &store,
        PageRequest {
            limit: 0,
            cursor: None,
            ..Default::default()
        }
    )
    .is_err());
    assert!(page(
        &store,
        PageRequest {
            limit: 201,
            cursor: None,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn meaningful_completion_moves_only_its_result_and_cursor_never_repeats_it() {
    let (_root, store, mut monitor) = fixture();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open), pull(2, Lifecycle::Open)],
        NOW + 5,
    );
    let first = page(
        &store,
        PageRequest {
            limit: 1,
            cursor: None,
            ..Default::default()
        },
    )
    .unwrap();
    let tail = page(
        &store,
        PageRequest {
            limit: 1,
            cursor: first.next_cursor.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    let review = completed(&store, &tail.results[0].job);
    store.save_reviews(&[review]).unwrap();
    let newest = page(
        &store,
        PageRequest {
            limit: 1,
            cursor: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(newest.results[0].item_id, tail.results[0].item_id);
    assert!(page(
        &store,
        PageRequest {
            limit: 1,
            cursor: first.next_cursor,
            ..Default::default()
        }
    )
    .unwrap()
    .results
    .is_empty());
}

#[test]
fn only_provider_terminal_observation_admits_cleanup_and_active_evidence_is_unchanged() {
    for gate in [
        "absent",
        "disabled",
        "deleted",
        "account",
        "finished",
        "legacy_closed",
    ] {
        let (_root, store, mut monitor) = fixture();
        let (review, publication, follow) = evidence(&store);
        match gate {
            "absent" => scan(&store, &mut monitor, Vec::new(), NOW + 5),
            "disabled" | "deleted" | "account" => {
                let mut settings = store.load_settings().unwrap();
                match gate {
                    "disabled" => settings.repositories[0].enabled = false,
                    "deleted" => settings.repositories.clear(),
                    _ => settings.repositories[0].provider_account_id = None,
                }
                store.save_settings(&settings).unwrap();
            }
            "legacy_closed" => {
                let mut queue = store.load_queue_state().unwrap();
                queue.tracked[0].lifecycle = Lifecycle::Closed;
                queue.tracked[0].terminal_observed = false;
                store.save_queue_state(&queue).unwrap();
            }
            _ => {}
        }
        assert_eq!(maintain(&store, true).unwrap(), 0, "{gate}");
        assert_eq!(store.load_reviews().unwrap(), vec![review]);
        assert_eq!(store.load_publications().unwrap(), vec![publication]);
        assert_eq!(store.load_follow_ups().unwrap(), vec![follow]);
    }
}

#[test]
fn cleanup_removes_every_bulky_copy_retains_receipts_and_preserves_settings_bytes() {
    let (root, store, mut monitor) = fixture();
    let (review, publication, follow) = evidence(&store);
    let settings = std::fs::read(root.path().join("config/settings.json")).unwrap();
    let item = crate::queue::item_id(&review.job);
    let mut notices = store.load_notifications().unwrap();
    notices.enabled = true;
    let notice = notices
        .enqueue_test(
            crate::notifications::Destination::QueueItem {
                item_id: item.clone(),
            },
            NOW,
        )
        .unwrap();
    store.save_notifications(&notices).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 10,
    );
    assert_eq!(maintain(&store, true).unwrap(), 1);
    assert_eq!(maintain(&store, true).unwrap(), 0);
    assert!(store.review_evidence().unwrap().is_empty());
    assert!(store.load_queue().unwrap().is_empty());
    assert!(store.load_follow_ups().unwrap().is_empty());
    assert!(store.load_feedback().unwrap().records.is_empty());
    for entry in std::fs::read_dir(root.path().join("state")).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains("BULKY-"));
    }
    assert_eq!(
        settings,
        std::fs::read(root.path().join("config/settings.json")).unwrap()
    );
    let receipts = load(&store).unwrap();
    assert_eq!(
        receipts.receipts[0].owned[0].proof.publication_id,
        publication.id
    );
    assert!(receipts.receipts[0].follow_up_keys.contains(&follow.key));
    assert!(receipts.receipts[0]
        .operations
        .iter()
        .any(|o| o.id == review.operation.id));
    assert!(matches!(
        detail(
            &store,
            crate::panel::Detail::Item {
                item_id: item.clone()
            }
        )
        .unwrap(),
        DetailResult::Cleaned { .. }
    ));
    assert!(matches!(
        detail(
            &store,
            crate::panel::Detail::Item {
                item_id: "unknown".into()
            }
        )
        .unwrap(),
        DetailResult::Missing { .. }
    ));
    assert_eq!(
        crate::notifications::destination(&store, &notice).unwrap(),
        crate::notifications::Destination::QueueItem {
            item_id: item.clone()
        }
    );
    assert!(
        crate::panel::missing(&store, &crate::panel::Route::item(item))
            .unwrap()
            .unwrap()
            .contains("cleaned")
    );
    assert!(store.save_reviews(&[review]).is_err());
    assert!(store.save_publications(&[publication]).is_err());
    assert!(store.save_follow_ups(&[follow]).is_err());
}

#[test]
fn running_workers_and_uncertain_provider_intents_are_not_settled_by_local_stop() {
    for gate in [
        "worker",
        "review",
        "publication",
        "pending",
        "reply",
        "reply_running",
    ] {
        let (_root, store, mut monitor) = fixture();
        let (mut review, mut publication, mut follow) = evidence(&store);
        match gate {
            "review" => {
                review.operation.state = OperationState::Running;
                store.save_reviews(&[review]).unwrap();
            }
            "publication" => {
                publication.uncertain = true;
                publication.operation.state = OperationState::Failed;
                store.save_publications(&[publication]).unwrap();
            }
            "pending" => {
                publication.receipts[0].state = RemoteState::Pending;
                store.save_publications(&[publication]).unwrap();
            }
            "reply" => {
                follow.uncertain = true;
                follow.phase = FollowPhase::Stopped;
                store.save_follow_ups(&[follow]).unwrap();
            }
            "reply_running" => {
                follow.analysis.as_mut().unwrap().state = OperationState::Running;
                store.save_follow_ups(&[follow]).unwrap();
            }
            _ => {}
        }
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Merged)],
            NOW + 10,
        );
        assert_eq!(maintain(&store, gate != "worker").unwrap(), 0, "{gate}");
        assert!(!store.review_evidence().unwrap().is_empty());
        assert!(load(&store).unwrap().receipts.is_empty());
    }
}

#[test]
fn every_partial_write_and_final_commit_failure_recovers_without_false_success() {
    for (file, occurrence) in [
        ("retention.json", 1),
        ("retention.json", 2),
        ("follow-ups.json", 1),
        ("publications.json", 1),
        ("reviews.json", 1),
        ("actions.json", 1),
        ("feedback.json", 1),
        ("notifications.json", 1),
        ("queue.json", 1),
        ("retention.json", 3),
        ("result-index.json", 1),
    ] {
        let (root, store, mut monitor) = fixture();
        evidence(&store);
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Closed)],
            NOW + 10,
        );
        store.fail_state_write(file, occurrence);
        assert!(maintain(&store, true).is_err(), "{file}/{occurrence}");
        assert!(load(&store).unwrap().receipts.is_empty());
        if load(&store).unwrap().pending.is_some_and(|p| p.applying) {
            assert!(store.review_evidence().is_err());
            assert!(store.save_feedback(&Default::default()).is_err());
        }
        let fresh = Store::new(root.path().into());
        recover(&fresh).unwrap();
        maintain(&fresh, true).unwrap();
        assert!(fresh.review_evidence().unwrap().is_empty(), "{file}");
        assert_eq!(load(&fresh).unwrap().receipts.len(), 1);
        assert!(load(&fresh).unwrap().pending.is_none());
    }
}

#[test]
fn reopening_before_cleanup_keeps_detail_and_reopening_after_cleanup_admits_same_head() {
    for after in [false, true] {
        let (_root, store, mut monitor) = fixture();
        let (review, _, _) = evidence(&store);
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Closed)],
            NOW + 5,
        );
        if after {
            assert_eq!(maintain(&store, true).unwrap(), 1);
        }
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Open)],
            NOW + 10,
        );
        assert_eq!(maintain(&store, true).unwrap(), 0);
        let jobs = store.load_queue().unwrap();
        let latest = jobs.last().unwrap();
        assert_eq!(latest.head_sha, review.job.head_sha);
        assert_eq!(latest.work.as_ref().unwrap().iteration, 2);
        assert_eq!(latest.work.as_ref().unwrap().pass_ordinal, 2);
        assert_ne!(
            crate::queue::item_id(latest),
            crate::queue::item_id(&review.job)
        );
        assert_ne!(crate::review::key(latest, ASSIGNMENT), review.key);
        assert_eq!(store.load_reviews().unwrap().is_empty(), after);
    }
}

#[test]
fn post_cleanup_human_closure_is_verified_and_never_resurrected_on_reopen() {
    let (_root, store, mut monitor) = fixture();
    evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    maintain(&store, true).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let job = store.load_queue().unwrap().last().unwrap().clone();
    assert!(crate::feedback::contexts(&store, &job, AGENT).is_err());
    let origin = load(&store).unwrap().receipts[0].owned[0].clone();
    let mut closed = thread();
    closed.resolved = true;
    admit_observations(
        &store,
        vec![Observed {
            origin: origin.clone(),
            head: job.head_sha.clone(),
            threads: vec![closed],
        }],
        NOW + 11,
    )
    .unwrap();
    let contexts = crate::feedback::contexts(&store, &job, AGENT).unwrap();
    assert!(contexts[0].closed);
    assert_eq!(contexts[0].path, "source.rs");
    assert!(store.load_follow_ups().unwrap().is_empty());
    // An unresolved-looking later read cannot undo a human closure.
    admit_observations(
        &store,
        vec![Observed {
            origin,
            head: job.head_sha.clone(),
            threads: vec![thread()],
        }],
        NOW + 12,
    )
    .unwrap();
    assert!(crate::feedback::contexts(&store, &job, AGENT).unwrap()[0].closed);
    let mut output = completed(&store, &job).result.unwrap().output;
    crate::feedback::suppress_closed_overlap(&mut output, &contexts, &[]);
    assert!(output.findings.is_empty());
    assert_eq!(output.held_findings.len(), 1);
    assert!(output.feedback_conflict);
}

#[test]
fn retained_roots_route_new_external_comments_to_primary_without_rewriting_owner() {
    let (_root, store, mut monitor) = fixture();
    let (_, _, old_follow) = evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    maintain(&store, true).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let origin = load(&store).unwrap().receipts[0].owned[0].clone();
    let head = "a".repeat(40);
    primary_retained_scan(
        &store,
        &mut monitor,
        vec![Observed {
            origin: origin.clone(),
            head: head.clone(),
            threads: vec![thread()],
        }],
        NOW + 11,
    );
    assert!(store.load_follow_ups().unwrap().is_empty());
    assert!(known_key(&store, &old_follow.key).unwrap());
    let mut updated = thread();
    let mut comment = updated.comments[1].clone();
    comment.id = "102".into();
    comment.body = "New evidence".into();
    comment.published_at = "2026-10-02T00:01:00Z".into();
    updated.comments.push(comment);
    primary_retained_scan(
        &store,
        &mut monitor,
        vec![Observed {
            origin,
            head,
            threads: vec![updated.clone()],
        }],
        NOW + 12,
    );
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].context.selection.agent.id, AGENT);
    assert_eq!(
        store.load_feedback().unwrap().records[0]
            .context
            .owner_agent_id,
        AGENT
    );
    assert_eq!(runs[0].reply_ordinal, Some(2));
    assert!(matches!(
        runs[0].target,
        crate::follow_up::ConversationTarget::Thread { .. }
    ));
    assert!(runs[0].fresh_thread(&updated));
    assert!(store.review_evidence().unwrap().is_empty());
    let snapshot = serde_json::to_value(crate::queue::snapshot(&store, vec![]).unwrap()).unwrap();
    assert_eq!(
        snapshot["follow_ups"][0]["run"]["target"]["thread"]["id"],
        "root-thread"
    );
    assert!(snapshot["follow_ups"][0]["run"]["target"]
        .get("review")
        .is_none());
}

#[test]
fn reopened_prepared_cleanup_is_abandoned_without_losing_any_detail() {
    let (root, store, mut monitor) = fixture();
    let (review, publication, follow) = evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    store.fail_state_write("retention.json", 2);
    assert!(maintain(&store, true).is_err());
    assert!(!load(&store).unwrap().pending.unwrap().applying);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let restarted = Store::new(root.path().into());
    recover(&restarted).unwrap();
    assert_eq!(maintain(&restarted, true).unwrap(), 0);
    assert_eq!(restarted.load_reviews().unwrap(), vec![review]);
    assert_eq!(restarted.load_publications().unwrap(), vec![publication]);
    assert_eq!(restarted.load_follow_ups().unwrap(), vec![follow]);
    assert!(load(&restarted).unwrap().pending.is_none());
}

#[test]
fn live_reservation_fences_cleanup_even_after_a_completed_local_result() {
    let (_root, store, mut monitor) = fixture();
    let (mut review, _, _) = evidence(&store);
    review.operation.state = OperationState::Running;
    store.save_reviews(&[review.clone()]).unwrap();
    let coordinator = crate::capacity::Coordinator::default();
    let key = WorkId {
        kind: crate::capacity::Kind::Normal,
        id: review.key.clone(),
    };
    let work = crate::capacity::Work {
        key: key.clone(),
        enqueue_order: 1,
        state: "waiting",
        reason: None,
    };
    let flag = coordinator
        .reserve(&store, &work, |flag| {
            Ok((review.operation.id.clone(), flag))
        })
        .unwrap()
        .unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    assert!(!coordinator.settle_terminal(&store).unwrap());
    assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
    review.operation.state = OperationState::Completed;
    store.save_reviews(&[review.clone()]).unwrap();
    assert_eq!(
        maintain(&store, coordinator.settle_terminal(&store).unwrap()).unwrap(),
        0
    );
    coordinator.release(&key, &review.operation.id).unwrap();
    assert_eq!(
        maintain(&store, coordinator.settle_terminal(&store).unwrap()).unwrap(),
        1
    );
    assert!(crate::review::host::complete(
        &store,
        &review.operation.id,
        Ok(review.result.unwrap()),
        NOW + 6
    )
    .is_err());
}

#[test]
fn legacy_queue_and_evidence_reconstruction_preserve_earlier_open_iterations() {
    let (root, store, mut monitor) = fixture();
    let (review, _, _) = evidence(&store);
    let mut revision = pull(1, Lifecycle::Open);
    revision.head_sha = "c".repeat(40);
    scan(&store, &mut monitor, vec![revision], NOW + 5);
    let jobs = store.load_queue().unwrap();
    store.write_state("queue.json", &jobs).unwrap();
    std::fs::remove_file(root.path().join("state/reviews.json")).unwrap();
    let fresh = Store::new(root.path().into());
    assert_eq!(fresh.review_evidence().unwrap(), vec![review.clone()]);
    assert_eq!(fresh.load_queue().unwrap().len(), 2);
    refresh(&fresh).unwrap();
    assert_eq!(
        page(
            &fresh,
            PageRequest {
                limit: 10,
                cursor: None,
                ..Default::default()
            }
        )
        .unwrap()
        .results
        .len(),
        2
    );
    assert_eq!(maintain(&fresh, true).unwrap(), 0);
    let destination = crate::panel::Detail::Job {
        kind: crate::capacity::Kind::Normal,
        id: review.key,
    };
    assert!(matches!(
        detail(&fresh, destination).unwrap(),
        DetailResult::Available { .. }
    ));
}

#[test]
fn failed_activity_index_write_is_visible_and_rebuilds_from_durable_state() {
    let (_root, store, _monitor) = fixture();
    let review = completed(&store, &store.load_queue().unwrap()[0]);
    store.fail_state_write("result-index.json", 1);
    assert!(store.save_reviews(std::slice::from_ref(&review)).is_err());
    assert_eq!(store.load_reviews().unwrap(), vec![review]);
    let page = page(
        &store,
        PageRequest {
            limit: 1,
            cursor: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.results[0].completed_passes, 1);
    assert_eq!(page.results[0].review_attempts, 1);
}

#[test]
fn completed_retries_remain_evidence_not_duplicate_normal_pass_counts() {
    let (_root, store, _monitor) = fixture();
    let first = completed(&store, &store.load_queue().unwrap()[0]);
    let mut retry = first.clone();
    retry.operation.id = "explicit-manual-retry".into();
    store.save_reviews(&[first, retry]).unwrap();
    let page = page(
        &store,
        PageRequest {
            limit: 1,
            cursor: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.results[0].review_attempts, 2);
    assert_eq!(page.results[0].completed_passes, 1);
}

#[test]
fn missing_ownership_provenance_blocks_cleanup_visibly_instead_of_dropping_safety() {
    let (root, store, mut monitor) = fixture();
    evidence(&store);
    std::fs::remove_file(root.path().join("state/publications.json")).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    assert!(maintain(&store, true)
        .unwrap_err()
        .contains("missing owned-publication"));
    assert!(!store.load_follow_ups().unwrap().is_empty());
    assert!(!store.review_evidence().unwrap().is_empty());
    assert!(load(&store).unwrap().pending.is_none());
}

#[test]
fn a_retained_human_judgment_gate_prevents_automatic_thread_reentry() {
    let (_root, store, mut monitor) = fixture();
    let (_, _, mut follow) = evidence(&store);
    follow.phase = FollowPhase::HumanInputRequired;
    store.save_follow_ups(&[follow]).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    maintain(&store, true).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let origin = load(&store).unwrap().receipts[0].owned[0].clone();
    assert!(origin.human_input_threads.contains("root-thread"));
    primary_retained_scan(
        &store,
        &mut monitor,
        vec![Observed {
            origin: origin.clone(),
            head: "a".repeat(40),
            threads: vec![thread()],
        }],
        NOW + 11,
    );
    let mut next = thread();
    next.comments[1].id = "102".into();
    primary_retained_scan(
        &store,
        &mut monitor,
        vec![Observed {
            origin,
            head: "a".repeat(40),
            threads: vec![next],
        }],
        NOW + 12,
    );
    let candidates = crate::follow_up::host::candidates(&store).unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].human_gate);
    assert!(!candidates[0].automatic_start);
}

fn primary_retained_scan(store: &Store, monitor: &mut Monitor, observed: Vec<Observed>, now: i64) {
    let ticket = monitor.prepare_checks(store, now, true).unwrap().remove(0);
    let accounts = BTreeMap::from([(
        "22".into(),
        crate::monitoring::AccountAvailability {
            login: "actor".into(),
            connected: true,
        },
    )]);
    monitor
        .finish_with_admission(
            store,
            &accounts,
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
                pull_requests: vec![pull(1, Lifecycle::Open)],
            }),
            now + 1,
            |store, ticket| {
                crate::follow_up::host::admit_scan(
                    store,
                    ticket,
                    crate::follow_up::host::Scan {
                        retained: observed,
                        ..Default::default()
                    },
                    now + 1,
                )
            },
        )
        .unwrap();
}

#[test]
fn receipt_recovers_iteration_and_fifo_watermark_when_queue_file_is_missing() {
    let (root, store, mut monitor) = fixture();
    evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    maintain(&store, true).unwrap();
    let watermark = load(&store).unwrap().receipts[0].enqueue_watermark;
    std::fs::remove_file(root.path().join("state/queue.json")).unwrap();
    let fresh = Store::new(root.path().into());
    let mut monitor = Monitor::restore(&fresh).unwrap();
    scan(
        &fresh,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let jobs = fresh.load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    let work = jobs[0].work.as_ref().unwrap();
    assert_eq!(work.iteration, 2);
    assert!(work.enqueue_order > watermark);
}

#[test]
fn a_new_local_attempt_cannot_hide_the_old_worker_reservation_from_cleanup() {
    let (_root, store, mut monitor) = fixture();
    let (_, _, mut follow) = evidence(&store);
    let old_operation = follow.analysis.as_ref().unwrap().id.clone();
    let coordinator = crate::capacity::Coordinator::default();
    let key = WorkId {
        kind: follow.kind(),
        id: follow.id.clone(),
    };
    let work = crate::capacity::Work {
        key: key.clone(),
        enqueue_order: 2,
        state: "waiting",
        reason: None,
    };
    coordinator
        .reserve(&store, &work, |_| Ok((old_operation.clone(), ())))
        .unwrap()
        .unwrap();
    follow.analysis.as_mut().unwrap().id = "new-local-attempt".into();
    store.save_follow_ups(&[follow]).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    assert!(!coordinator.settle_terminal(&store).unwrap());
    assert_eq!(maintain(&store, false).unwrap(), 0);
    coordinator.release(&key, &old_operation).unwrap();
    assert!(coordinator.settle_terminal(&store).unwrap());
    assert_eq!(maintain(&store, true).unwrap(), 1);
}
