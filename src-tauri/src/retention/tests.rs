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
    finish_scan(store, monitor, ticket, pulls, now);
}

fn finish_scan(
    store: &Store,
    monitor: &mut Monitor,
    ticket: crate::monitoring::PollTicket,
    pulls: Vec<PullRequest>,
    now: i64,
) {
    monitor
        .finish(
            store,
            ticket,
            Ok(PollResult {
                states: Default::default(),
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
        mode: crate::review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None, key: crate::review::key(job, ASSIGNMENT), assignment_id: ASSIGNMENT.into(),
        job: job.clone(), selection: Selection::resolve(&store.load_settings().unwrap(), job, ASSIGNMENT).unwrap().into(),
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
            diff_position: None,
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
fn clean_selects_one_terminal_pr_and_drops_its_pr_state() {
    let (_root, store, mut monitor) = fixture();
    let (review, _, _) = evidence(&store);
    let binding = Binding::job(&review.job);
    let mut states = crate::pr_state::PrStates::default();
    states.consume(&binding, 0, "baseline");
    store.save_pr_states(&states).unwrap();
    // An open PR never qualifies, selected or not.
    assert_eq!(maintain_where(&store, true, |_| true).unwrap(), 0);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 10,
    );
    assert_eq!(maintain_where(&store, true, |_| false).unwrap(), 0);
    assert!(store.load_pr_states().unwrap().get(&binding).is_some());
    assert_eq!(
        maintain_where(&store, true, |scope| scope.number == 1).unwrap(),
        1
    );
    assert!(store.load_pr_states().unwrap().get(&binding).is_none());
    assert!(store.load_queue().unwrap().is_empty());
}

#[test]
fn cleanup_removes_every_bulky_copy_retains_receipts_and_preserves_settings_bytes() {
    let (root, store, mut monitor) = fixture();
    let (review, publication, follow) = evidence(&store);
    let settings = std::fs::read(root.path().join("config/settings.json")).unwrap();
    let item = crate::queue::item_id(&review.job);
    let mut notices = store.load_notifications().unwrap();
    notices.enable(&[], NOW).unwrap();
    notices
        .observe(
            &[crate::notifications::Frame {
                source: format!("queue:{item}"),
                event: Some(crate::notifications::Event {
                    category: crate::notifications::Category::Ready,
                    destination: crate::notifications::Destination::QueueItem {
                        item_id: item.clone(),
                    },
                    cause: "review".into(),
                    group: "group".into(),
                }),
            }],
            NOW,
        )
        .unwrap();
    let notice = notices.notices.last().unwrap().id.clone();
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
    for file in Store::saved_files(&root.path().join("state")) {
        let bytes = std::fs::read(file).unwrap();
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
        .operation_ids
        .contains(&review.operation.id));
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
        offline: None,
        key: key.clone(),
        enqueue_order: 1,
        state: "waiting",
        reason: None,
        failure: None,
        delayed_until: None,
        blocked_by: None,
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
    let (mut review, _, _) = evidence(&store);
    let old_operation = review.operation.id.clone();
    let coordinator = crate::capacity::Coordinator::default();
    let key = WorkId {
        kind: crate::capacity::Kind::Normal,
        id: review.key.clone(),
    };
    let work = crate::capacity::Work {
        offline: None,
        key: key.clone(),
        enqueue_order: 2,
        state: "waiting",
        reason: None,
        failure: None,
        delayed_until: None,
        blocked_by: None,
    };
    coordinator
        .reserve(&store, &work, |_| Ok((old_operation.clone(), ())))
        .unwrap()
        .unwrap();
    review.operation.id = "new-local-attempt".into();
    store.save_reviews(&[review]).unwrap();
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

#[test]
fn automatic_prune_drops_closed_and_merged_prs_only_and_reopen_starts_fresh() {
    for lifecycle in [Lifecycle::Closed, Lifecycle::Merged] {
        let (_root, store, mut monitor) = fixture();
        let (review, publication, follow) = evidence(&store);
        // Open: never pruned.
        assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
        assert_eq!(store.load_reviews().unwrap(), vec![review.clone()]);
        assert_eq!(store.load_publications().unwrap(), vec![publication]);
        assert_eq!(store.load_follow_ups().unwrap(), vec![follow]);
        scan(
            &store,
            &mut monitor,
            vec![pull(1, lifecycle.clone())],
            NOW + 5,
        );
        // Workers not settled: nothing yet; a zero budget defers it.
        assert_eq!(prune_terminal(&store, false, PRUNE_PER_PASS).unwrap(), 0);
        assert_eq!(prune_terminal(&store, true, 0).unwrap(), 0);
        assert!(!store.review_evidence().unwrap().is_empty());
        assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 1);
        assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
        assert!(store.review_evidence().unwrap().is_empty());
        assert!(store.load_queue().unwrap().is_empty());
        assert!(store.load_follow_ups().unwrap().is_empty());
        assert_eq!(load(&store).unwrap().receipts.len(), 1);
        // A reopened PR is a new iteration with fresh state.
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Open)],
            NOW + 10,
        );
        let latest = store.load_queue().unwrap().last().unwrap().clone();
        assert_eq!(latest.work.as_ref().unwrap().iteration, 2);
        assert_ne!(crate::review::key(&latest, ASSIGNMENT), review.key);
    }
}

#[test]
fn automatic_prune_keeps_detail_needing_repair_without_failing_the_pass() {
    let (root, store, mut monitor) = fixture();
    evidence(&store);
    std::fs::remove_file(root.path().join("state/publications.json")).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    assert!(!store.load_follow_ups().unwrap().is_empty());
    assert!(!store.review_evidence().unwrap().is_empty());
    assert!(load(&store).unwrap().pending.is_none());
}

#[test]
fn an_older_closed_read_finishing_after_a_newer_reopen_is_ignored() {
    let (_root, store, mut monitor) = fixture();
    let (review, _, _) = evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    // Overlapping reads: the older still sees Closed, the newer the reopen.
    let older = monitor
        .prepare_checks(&store, NOW + 10, true)
        .unwrap()
        .remove(0);
    let mut newer = older.clone();
    newer.prepared_at = NOW + 20;
    finish_scan(
        &store,
        &mut monitor,
        newer,
        vec![pull(1, Lifecycle::Open)],
        NOW + 20,
    );
    finish_scan(
        &store,
        &mut monitor,
        older,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 30,
    );
    let tracked = store.load_queue_state().unwrap().tracked.remove(0);
    assert_eq!(tracked.lifecycle, Lifecycle::Open);
    assert!(!tracked.terminal_observed);
    assert_eq!(tracked.iteration, 2);
    assert_eq!(tracked.observed_at, NOW + 20);
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    assert!(store
        .review_evidence()
        .unwrap()
        .iter()
        .any(|run| run.operation.id == review.operation.id));
}

fn exit_write(
    job: &QueueJob,
    phase: crate::exit_writes::Phase,
    step: crate::exit_writes::StepState,
) -> crate::exit_writes::ExitWrite {
    crate::exit_writes::ExitWrite {
        id: "exit-1".into(),
        review_operation_id: "operation".into(),
        job: job.clone(),
        assignment_id: ASSIGNMENT.into(),
        comments_first: false,
        published: false,
        steps: vec![crate::exit_writes::Step {
            kind: crate::exit_writes::StepKind::Reply,
            thread_id: "thread".into(),
            body: "Reply".into(),
            root_id: Some("root".into()),
            revision: None,
            state: step,
            receipt: (step == crate::exit_writes::StepState::Done).then(|| "comment".into()),
            resolution_evidence: None,
        }],
        phase,
        operation: JobOperation::review(job, NOW),
        uncertain: false,
        error: None,
        consumed: false,
        reviewed_base_sha: None,
        sign_off: None,
    }
}

fn save_exit_write(store: &Store, write: &crate::exit_writes::ExitWrite) -> Result<(), String> {
    // As the exit-write worker saves: load, replace by id, save.
    let mut ledger = store.load_feedback()?;
    ledger.exit_writes.retain(|w| w.id != write.id);
    ledger.exit_writes.push(write.clone());
    store.save_feedback(&ledger)
}

#[test]
fn pruning_waits_for_exit_writes_and_a_late_save_cannot_restore_them() {
    use crate::exit_writes::{Phase, StepState};
    let (_root, store, mut monitor) = fixture();
    let (review, _, _) = evidence(&store);
    save_exit_write(
        &store,
        &exit_write(&review.job, Phase::Planned, StepState::Planned),
    )
    .unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Merged)],
        NOW + 5,
    );
    // Before the provider mutation, and after its intent was saved.
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    save_exit_write(
        &store,
        &exit_write(&review.job, Phase::Verified, StepState::Attempted),
    )
    .unwrap();
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    let mut uncertain = exit_write(&review.job, Phase::Failed, StepState::Done);
    uncertain.uncertain = true;
    save_exit_write(&store, &uncertain).unwrap();
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    assert!(!store.review_evidence().unwrap().is_empty());
    // Every effect confirmed but not yet consumed: still owed locally.
    let mut done = exit_write(&review.job, Phase::Completed, StepState::Done);
    save_exit_write(&store, &done).unwrap();
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    // Consumed: settled, pruned once, receipt retained.
    done.consumed = true;
    save_exit_write(&store, &done).unwrap();
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 1);
    assert_eq!(load(&store).unwrap().receipts.len(), 1);
    assert!(store.load_feedback().unwrap().exit_writes.is_empty());
    // A worker finishing late cannot resurrect the cleaned write.
    assert!(save_exit_write(&store, &done).is_err());
    assert!(store.load_feedback().unwrap().exit_writes.is_empty());
}

#[test]
fn pruning_waits_for_an_lgtm_sign_off_resolving_its_threads() {
    use crate::exit_writes::{Phase, Step, StepKind, StepState};
    let (_root, store, mut monitor) = fixture();
    let (review, _, _) = evidence(&store);
    let mut write = exit_write(&review.job, Phase::Verified, StepState::Done);
    write.sign_off = Some(crate::exits::LgtmAction::Approve);
    write.steps.push(Step {
        kind: StepKind::Resolve,
        body: String::new(),
        root_id: None,
        state: StepState::Attempted,
        receipt: None,
        ..write.steps[0].clone()
    });
    save_exit_write(&store, &write).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Merged)],
        NOW + 5,
    );
    // A resolution may be landing; its approve final has not been adopted yet.
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    write.phase = Phase::Completed;
    write.steps[1].state = StepState::Done;
    write.steps[1].receipt = Some("resolved".into());
    save_exit_write(&store, &write).unwrap();
    // Resolved but unconsumed: its approve/merge final is not adopted yet.
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    write.consumed = true;
    save_exit_write(&store, &write).unwrap();
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 1);
    assert!(store.load_feedback().unwrap().exit_writes.is_empty());
}

#[test]
fn a_completed_unconsumed_sign_off_survives_a_restart_before_its_consume() {
    use crate::exit_writes::{Phase, StepState};
    let (root, store, mut monitor) = fixture();
    let (review, _, _) = evidence(&store);
    let mut write = exit_write(&review.job, Phase::Completed, StepState::Done);
    write.sign_off = Some(crate::exits::LgtmAction::Merge);
    save_exit_write(&store, &write).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Merged)],
        NOW + 5,
    );
    drop(store);
    // Restart: maintenance runs before the exit-write coordinator consumes it.
    let store = Store::new(root.path().into());
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
    assert!(!store.review_evidence().unwrap().is_empty());
    let saved = store.load_feedback().unwrap().exit_writes;
    assert!(saved.iter().any(|w| w.id == write.id && !w.consumed));
    // The consume is durable: now the PR is settled and pruned.
    write.consumed = true;
    save_exit_write(&store, &write).unwrap();
    let store = Store::new(root.path().into());
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 1);
    assert!(store.load_feedback().unwrap().exit_writes.is_empty());
}

#[test]
fn evicted_detail_keeps_the_safety_tombstone_of_a_reopened_pr() {
    let (_root, store, mut monitor) = fixture();
    let (review, publication, follow) = evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Merged)],
        NOW + 5,
    );
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 1);
    let mut ledger = load(&store).unwrap();
    let pruned = ledger.receipts[0].clone();
    assert!(pruned.operations.is_empty() && !pruned.operation_ids.is_empty());
    // 600 newer cleanups push this PR's detail out of the detail window.
    for n in 0..600 {
        let mut receipt = pruned.clone();
        receipt.scope.pull_request_id = format!("newer-{n}");
        receipt.scope.iteration_id = format!("iteration-newer-{n}");
        receipt.cleaned_at = pruned.cleaned_at + 1 + n;
        ledger.receipts.push(receipt);
    }
    save(&store, &ledger).unwrap();
    let saved = load(&store).unwrap();
    assert_eq!(
        saved.receipts.len(),
        601,
        "no tombstone is evicted by count"
    );
    let tombstone = saved
        .receipts
        .iter()
        .find(|r| r.scope.pull_request_id == review.job.pull_request_id)
        .unwrap();
    assert!(!tombstone.has_detail());
    assert_eq!(tombstone.scope, pruned.scope);
    assert_eq!(tombstone.owned, pruned.owned);
    assert_eq!(tombstone.items, pruned.items);
    assert!(tombstone.follow_up_keys.contains(&follow.key));
    assert_eq!(
        saved.receipts.iter().filter(|r| r.has_detail()).count(),
        DETAIL_RECEIPTS
    );
    // A late save for the old iteration is still refused.
    assert!(store.save_reviews(std::slice::from_ref(&review)).is_err());
    // Reopened: a new iteration whose review still recognizes PR Sniper's
    // earlier comments as its own (no re-posting them as new findings).
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let reopened = store.load_queue().unwrap().last().unwrap().clone();
    assert_eq!(reopened.work.as_ref().unwrap().iteration, 2);
    let proofs = crate::review::findings::proofs(&store, &reopened, ASSIGNMENT).unwrap();
    assert!(proofs
        .iter()
        .any(|proof| proof.publication_id == publication.id && proof.root_ids == ["100"]));
}

#[test]
fn ten_thousand_tombstones_stay_small() {
    let (root, store, mut monitor) = fixture();
    evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Merged)],
        NOW + 5,
    );
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 1);
    let mut ledger = load(&store).unwrap();
    let mut tombstone = ledger.receipts[0].clone();
    tombstone.drop_detail();
    // A PR with one published batch: scope, watermarks, ordinals, item and
    // mention identities and the own-comment proof.
    let bytes = serde_json::to_vec(&tombstone).unwrap().len();
    assert!(bytes <= 1_600, "{bytes}");
    ledger.receipts = (0..10_000)
        .map(|n| {
            let mut receipt = tombstone.clone();
            receipt.scope.pull_request_id = format!("{n}");
            receipt
        })
        .collect();
    save(&store, &ledger).unwrap();
    let file = std::fs::metadata(root.path().join("state/retention.json"))
        .unwrap()
        .len();
    assert!(file <= 16_000_000, "{file}");
    assert_eq!(load(&store).unwrap().receipts.len(), 10_000);
    eprintln!("tombstone {bytes} B; 10,000 PRs {file} B");
}

#[test]
fn reads_prepared_in_the_same_second_keep_their_order() {
    // Poll variant: both reads prepared at the same second.
    let (_root, store, mut monitor) = fixture();
    evidence(&store);
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    let older = monitor
        .prepare_checks(&store, NOW + 10, true)
        .unwrap()
        .remove(0);
    let mut newer = older.clone();
    newer.sequence = crate::monitoring::next_observation();
    assert_eq!(newer.prepared_at, older.prepared_at);
    finish_scan(
        &store,
        &mut monitor,
        newer,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    finish_scan(
        &store,
        &mut monitor,
        older,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 10,
    );
    let tracked = store.load_queue_state().unwrap().tracked.remove(0);
    assert_eq!(tracked.lifecycle, Lifecycle::Open);
    assert_eq!(tracked.iteration, 2);
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);

    // Lifecycle-read variant: prepared before a same-second poll that saw
    // the PR still open, finishing after it.
    let (_root, store, mut monitor) = fixture();
    evidence(&store);
    let current = store.load_queue_state().unwrap().tracked.remove(0);
    let lifecycle = crate::monitoring::LifecycleTicket {
        configuration_id: current.configuration_id.clone(),
        name: "example/repo".into(),
        provider_account_id: current.account_id.clone(),
        provider_repository_id: current.repository_id.clone(),
        tracked: vec![(current.pull_request_id.clone(), current.number)],
        iterations: [(current.pull_request_id.clone(), current.iteration)].into(),
        prepared_at: NOW + 10,
        sequence: crate::monitoring::next_observation(),
    };
    let poll = monitor
        .prepare_checks(&store, NOW + 10, true)
        .unwrap()
        .remove(0);
    finish_scan(
        &store,
        &mut monitor,
        poll,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    monitor
        .apply_lifecycle(
            &store,
            &lifecycle,
            Ok(vec![(current.pull_request_id.clone(), Lifecycle::Closed)]),
            NOW + 11,
        )
        .unwrap();
    let tracked = store.load_queue_state().unwrap().tracked.remove(0);
    assert_eq!(tracked.lifecycle, Lifecycle::Open);
    assert!(!tracked.terminal_observed);
    assert_eq!(prune_terminal(&store, true, PRUNE_PER_PASS).unwrap(), 0);
}
