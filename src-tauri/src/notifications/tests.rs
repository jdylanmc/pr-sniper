use super::*;
use crate::monitoring::QueueJob;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(windows)]
use crate::windows_permissions;

fn frame(category: Category, cause: &str) -> Frame {
    Frame {
        source: "queue:exact-revision".into(),
        event: Some(Event {
            category,
            destination: Destination::Settings,
            cause: cause.into(),
            group: "repository-group".into(),
        }),
    }
}

fn store() -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    restore(&store).unwrap();
    let mut ledger = store.load_notifications().unwrap();
    ledger.enabled = true;
    store.save_notifications(&ledger).unwrap();
    (root, store)
}

fn admit(store: &Store, frames: &[Frame]) -> String {
    let mut ledger = store.load_notifications().unwrap();
    ledger.observe(frames, 100).unwrap();
    let id = ledger.next().unwrap().id.clone();
    store.save_notifications(&ledger).unwrap();
    id
}

struct Wire {
    store: Store,
    calls: AtomicUsize,
    failure: Option<bool>,
}

impl Adapter for Wire {
    fn permission(&self) -> Result<Permission, String> {
        Ok(Permission {
            authorization: "authorized".into(),
            alerts_enabled: Some(true),
            center_enabled: Some(true),
        })
    }
    fn request_permission(&self) -> Result<Permission, String> {
        self.permission()
    }
    fn send(&self, notice: &Notice, _message: &Message) -> Result<(), SendError> {
        assert_eq!(
            self.store
                .load_notifications()
                .unwrap()
                .notices
                .iter()
                .find(|n| n.id == notice.id)
                .unwrap()
                .phase,
            Phase::Submitting
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(uncertain) = self.failure {
            Err(SendError {
                message: "Native fixture failure.".into(),
                uncertain,
            })
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
#[test]
fn failed_initial_save_recovers_only_from_the_exact_persisted_profile() {
    let root = tempfile::Builder::new()
        .prefix("notification-save-obstruction-")
        .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let store = Store::new(root.path().into());
    store.save_queue_selection(None).unwrap();
    let obstruction = root.path().join("state").join("notifications.json.tmp");
    std::fs::write(&obstruction, b"owned obstruction").unwrap();
    assert!(restore(&store).is_err());
    let canonical = root.path().canonicalize().unwrap();
    let mut identity = windows::saved_registration(&store, &canonical);
    assert!(
        identity.is_err(),
        "Missing ledger must not supply a synthetic identity."
    );
    assert!(windows::recover_identity(&mut identity, &store, &canonical).is_err());
    assert!(identity.is_err());
    std::fs::remove_file(&obstruction).unwrap();
    let registration = windows::recover_identity(&mut identity, &store, &canonical).unwrap();
    let mut ledger = persisted_ledger(&store).unwrap();
    assert_eq!(registration.profile, ledger.profile_id);
    assert!(!ledger.enabled);
    let id = ledger.enable(&[], 101).unwrap().unwrap();
    store.save_notifications(&ledger).unwrap();
    let wire = Wire {
        store: Store::new(root.path().into()),
        calls: AtomicUsize::new(0),
        failure: None,
    };
    let notice = prepare(&store, &[], &id, 102).unwrap();
    registration.notice_id(&notice.id).unwrap();
    assert_eq!(notice.id, id);
    assert_eq!(
        submit(&wire, &notice, &render(&notice, None)),
        (Phase::AcceptedUnconfirmed, None)
    );
    assert_eq!(wire.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        persisted_ledger(&wire.store).unwrap().profile_id,
        registration.profile
    );
    // A subsequent foreign saved identity cannot reuse or install this cache.
    store.save_notifications(&Ledger::default()).unwrap();
    assert!(windows::recover_identity(&mut identity, &store, &canonical).is_err());
}

#[test]
fn windows_aggregate_permission_preserves_unknown_channels() {
    let permission = Permission {
        authorization: "authorized_aggregate".into(),
        alerts_enabled: None,
        center_enabled: None,
    };
    assert!(permission.allowed());
    let json = serde_json::to_value(&permission).unwrap();
    assert!(json["alerts_enabled"].is_null());
    assert!(json["center_enabled"].is_null());
    for authorization in [
        "denied",
        "disabled_for_user",
        "disabled_by_policy",
        "disabled_by_manifest",
        "unknown",
        "not_registered",
    ] {
        assert!(!Permission {
            authorization: authorization.into(),
            ..permission.clone()
        }
        .allowed());
    }
}

#[test]
fn unchanged_poll_and_restart_do_not_repeat_a_transition() {
    let (root, store) = store();
    let frames = [frame(Category::Ready, "review-1")];
    let id = admit(&store, &frames);
    let notice = prepare(&store, &frames, &id, 101).unwrap();
    let wire = Wire {
        store: Store::new(root.path().into()),
        calls: AtomicUsize::new(0),
        failure: None,
    };
    let (phase, error) = submit(&wire, &notice, &render(&notice, None));
    assert_eq!(phase, Phase::AcceptedUnconfirmed);
    complete(&store, &id, phase, error).unwrap();
    for _ in 0..3 {
        let restored = Store::new(root.path().into());
        restore(&restored).unwrap();
        let mut ledger = restored.load_notifications().unwrap();
        assert!(!ledger.observe(&frames, 500).unwrap());
        assert!(ledger.next().is_none());
        assert_eq!(ledger.notices.len(), 1);
        assert_eq!(ledger.notices[0].opened_at, None);
    }
    assert_eq!(wire.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn new_causes_and_genuine_reentry_are_eligible_without_polling_duplicates() {
    let (_root, store) = store();
    let mut ledger = store.load_notifications().unwrap();
    let first = frame(Category::HumanInput, "external-comment-1");
    assert!(ledger.observe(std::slice::from_ref(&first), 1).unwrap());
    assert!(!ledger.observe(std::slice::from_ref(&first), 2).unwrap());
    assert!(ledger
        .observe(&[frame(Category::HumanInput, "external-comment-2")], 3)
        .unwrap());
    assert_eq!(ledger.notices.len(), 2);
    ledger
        .observe(
            &[Frame {
                source: first.source.clone(),
                event: None,
            }],
            4,
        )
        .unwrap();
    ledger
        .observe(&[frame(Category::HumanInput, "external-comment-2")], 5)
        .unwrap();
    assert_eq!(ledger.notices.len(), 3);
    assert_eq!(ledger.notices[2].episode, 3);
    assert_ne!(ledger.notices[0].id, ledger.notices[1].id);
}

#[test]
fn interrupted_and_timed_out_submissions_remain_unknown_without_retries() {
    for failure in [None, Some(true)] {
        let (root, store) = store();
        let frames = [frame(Category::Ready, "review")];
        let id = admit(&store, &frames);
        let notice = prepare(&store, &frames, &id, 100).unwrap();
        if let Some(uncertain) = failure {
            let wire = Wire {
                store: Store::new(root.path().into()),
                calls: AtomicUsize::new(0),
                failure: Some(uncertain),
            };
            let (phase, error) = submit(&wire, &notice, &render(&notice, None));
            complete(&store, &id, phase, error).unwrap();
        }
        restore(&Store::new(root.path().into())).unwrap();
        let mut ledger = store.load_notifications().unwrap();
        assert_eq!(ledger.notices[0].phase, Phase::OutcomeUnknown);
        assert!(ledger.notices[0].error.is_some());
        assert!(ledger.begin(&id).is_err());
        assert!(!ledger.observe(&frames, 500).unwrap());
        assert!(ledger.next().is_none());
    }
}

#[test]
fn stale_or_disabled_transitions_stop_before_native_submission() {
    let (_root, store) = store();
    let frames = [frame(Category::Ready, "old")];
    let id = admit(&store, &frames);
    let notice = prepare(
        &store,
        &[Frame {
            source: frames[0].source.clone(),
            event: None,
        }],
        &id,
        102,
    )
    .unwrap();
    assert_eq!(notice.phase, Phase::NotSent);
    let id = admit(&store, &[frame(Category::Ready, "new")]);
    let mut ledger = store.load_notifications().unwrap();
    ledger.enabled = false;
    store.save_notifications(&ledger).unwrap();
    assert_eq!(
        prepare(&store, &[frame(Category::Ready, "new")], &id, 104)
            .unwrap()
            .phase,
        Phase::NotSent
    );
}

#[test]
fn private_persistence_failure_prevents_native_request_and_corruption_is_not_empty_success() {
    let (root, store) = store();
    let frames = [frame(Category::Ready, "review")];
    let id = admit(&store, &frames);
    let path = root.path().join("state/notifications.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    windows_permissions::assert_private(&path, false);
    std::fs::write(&path, "invalid").unwrap();
    assert!(store.load_notifications().is_err());
    assert!(prepare(&store, &frames, &id, 100).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.save_notifications(&Ledger::default()).is_err());
}

#[test]
fn native_failures_and_permission_denial_remain_visible_without_acknowledgment() {
    let (root, store) = store();
    let frames = [frame(Category::Failure, "operation")];
    let id = admit(&store, &frames);
    let notice = prepare(&store, &frames, &id, 100).unwrap();
    let wire = Wire {
        store: Store::new(root.path().into()),
        calls: AtomicUsize::new(0),
        failure: Some(false),
    };
    let (phase, error) = submit(&wire, &notice, &render(&notice, None));
    complete(&store, &id, phase, error).unwrap();
    assert_eq!(phase, Phase::Failed);
    let id = admit(&store, &[frame(Category::Failure, "new-operation")]);
    prepare(
        &store,
        &[frame(Category::Failure, "new-operation")],
        &id,
        101,
    )
    .unwrap();
    complete(
        &store,
        &id,
        Phase::PermissionDenied,
        Some("Denied by macOS.".into()),
    )
    .unwrap();
    assert!(store
        .load_notifications()
        .unwrap()
        .notices
        .iter()
        .all(|n| n.opened_at.is_none()));
    for authorization in ["denied", "not_determined", "unknown"] {
        assert!(!Permission {
            authorization: authorization.into(),
            alerts_enabled: Some(true),
            center_enabled: Some(true)
        }
        .allowed());
    }
    assert!(!Permission {
        authorization: "authorized".into(),
        alerts_enabled: Some(false),
        center_enabled: Some(false)
    }
    .allowed());
}

fn job() -> QueueJob {
    serde_json::from_value(serde_json::json!({
        "provider":"github","account_id":"22","account_login":"operator","configuration_id":"config",
        "repository_id":"100","repository_name":"example/repo","pull_request_id":"9","number":9,
        "title":"Sensitive PR title","head_sha":"a".repeat(40),"trigger_policy":"policy",
        "author_id":"11","author_login":"author","watched_author":true,"requested_reviewer":false,
        "waiting":"human_start","detected_at":100
    })).unwrap()
}

fn merged_snapshot(count: usize) -> queue::Snapshot {
    queue::Snapshot {
        pending_threads: vec![],
        feedback: Default::default(),
        mentions: vec![],
        tracked: vec![],
        health: vec![],
        jobs: vec![],
        reviews: vec![],
        publications: vec![],
        follow_ups: vec![],
        exit_writes: vec![],
        pr_states: Default::default(),
        items: (0..count)
            .map(|iteration| {
                let mut job = job();
                job.head_sha = format!("{iteration:040x}");
                job.detected_at += iteration as i64;
                job.work = Some(crate::monitoring::NormalWork {
                    request_override: None,
                    id: format!("job-{iteration}"),
                    item_id: format!("iteration-{iteration}"),
                    iteration_id: format!("iteration-{iteration}"),
                    iteration: iteration as u64 + 1,
                    agent_id: "synthetic-agent".into(),
                    enqueue_order: iteration as u64 + 1,
                    pass_ordinal: 1,
                    trigger: crate::monitoring::WorkTrigger::NewRevision,
                    admission: crate::monitoring::Admission {
                        watched_author: true,
                        all_authors: false,
                        requested_reviewer: false,
                    },
                    legacy_item_id: None,
                    state_seq: 1,
                });
                let id = queue::item_id(&job);
                queue::Item {
                    dismiss_token: None,
                    dismissed: false,
                    action_status: Some(crate::actions::Status {
                        item_id: id.clone(),
                        final_valid: false,
                        provider_observed_at: Some(200),
                        observation_retry_blocker: None,
                        primary_assignment_id: None,
                        machine_clear: false,
                        personal_review: "",
                        permissions: Default::default(),
                        provider_approval: None,
                        final_review: None,
                        effects: vec![],
                        blockers: vec![],
                    }),
                    exit: None,
                    exit_run_id: None,
                    progress: None,
                    feedback: vec![],
                    aliases: vec![],
                    id,
                    job,
                    state: queue::State::Merged,
                    failure_class: None,
                    failure_cause: None,
                    failed_teammate: None,
                    failed_duck: None,
                    blocked_by: None,
                    resume_at: None,
                    summary: "",
                    review_keys: vec![],
                    follow_up_ids: vec![],
                    warnings: vec![],
                }
            })
            .collect(),
    }
}

#[test]
fn merged_history_emits_once_and_keeps_the_latest_exact_destination() {
    let snapshot = merged_snapshot(21);
    let expected = snapshot.items.last().unwrap().id.clone();
    let mut ledger = Ledger {
        enabled: true,
        ..Default::default()
    };
    ledger.observe(&frames(&snapshot), 201).unwrap();
    assert_eq!(ledger.notices.len(), 1);
    assert_eq!(
        ledger.notices[0].event.destination,
        Destination::QueueItem { item_id: expected }
    );
    assert!(ledger.current(&ledger.notices[0]));
    // Neither ordering nor retention of the newest iteration changes merge identity.
    let mut snapshot = snapshot;
    snapshot.items.reverse();
    assert!(!ledger.observe(&frames(&snapshot), 202).unwrap());
    snapshot.items.remove(0);
    ledger.observe(&frames(&snapshot), 203).unwrap();
    assert_eq!(ledger.notices.len(), 1);
    assert!(ledger.current(&ledger.notices[0]));
}

#[test]
fn merged_identity_isolates_prs_accounts_repositories_and_providers() {
    let mut snapshot = merged_snapshot(5);
    snapshot.items[1].job.pull_request_id = "other-pr".into();
    snapshot.items[2].job.account_id = "other-account".into();
    snapshot.items[3].job.repository_id = "other-repository".into();
    snapshot.items[4].job.provider = "other-provider".into();
    let mut ledger = Ledger {
        enabled: true,
        ..Default::default()
    };
    ledger.observe(&frames(&snapshot), 201).unwrap();
    assert_eq!(ledger.notices.len(), 5);
    for notice in &ledger.notices {
        assert!(snapshot.items.iter().any(|item| notice.event.destination
            == Destination::QueueItem {
                item_id: item.id.clone()
            }));
    }
    for item in &mut snapshot.items {
        item.job.configuration_id = "replacement-configuration".into();
        item.job.account_login = "renamed-login".into();
        item.job.repository_name = "renamed/repository".into();
    }
    ledger.observe(&frames(&snapshot), 202).unwrap();
    assert_eq!(ledger.notices.len(), 5);
}

#[test]
fn merge_receipt_enrichment_does_not_create_a_second_alert_or_change_navigation() {
    let mut snapshot = merged_snapshot(21);
    let mut ledger = Ledger {
        enabled: true,
        ..Default::default()
    };
    let before = frames(&snapshot);
    ledger.observe(&before, 201).unwrap();
    let effect = serde_json::from_value(serde_json::json!({
        "id":"merge-effect","final_id":"final","item_id":"iteration-20","action":"merge",
        "operation":{
            "id":"merge-operation","provider":"github","account_id":"22",
            "configuration_id":"config","repository_id":"100","pull_request_id":"9",
            "head_sha":"a".repeat(40),"trigger_policy":"policy","operation_type":"merge",
            "state":"completed","attempt_count":1,"initial_attempt_at":100,
            "retry_deadline":200,"next_attempt_at":null,"failure":null,
            "attempted_mutation":null,"pending_review_id":null,"owned_thread_id":null,
            "triggering_external_comment_id":null,"confirmed_receipt":null
        },
        "observation":{
            "write_capability":true,"node_id":"node","repository_id":"100",
            "pull_request_id":"9","account_id":"22","author_id":"11",
            "head_repository_id":null,"head":"a".repeat(40),"base":"b".repeat(40),
            "base_name":"main","merge_rules":null,"merge_rules_error":null,
            "state":"MERGED","draft":false,"permission":"WRITE",
            "mergeable":"MERGEABLE","merge_state":"CLEAN","review_decision":null,
            "checks":null,"check_contexts":[],"in_merge_queue":false,"method":null,
            "protection":null,"threads":[],"reviews":[],"comments":[],
            "merged_by":"22","merged_at":null,"merge_commit":"c".repeat(40)
        },
        "body":"","state":"confirmed","error":null,
        "receipt":{
            "id":"receipt","actor_id":"22","head":"a".repeat(40),
            "action":"merge","merge_commit":"c".repeat(40)
        }
    }))
    .unwrap();
    snapshot
        .items
        .last_mut()
        .unwrap()
        .action_status
        .as_mut()
        .unwrap()
        .effects
        .push(effect);
    let after = frames(&snapshot);
    assert_ne!(
        before[0].event.as_ref().unwrap().cause,
        after[0].event.as_ref().unwrap().cause
    );
    assert!(!ledger.observe(&after, 202).unwrap());
    assert_eq!(ledger.notices.len(), 1);
    assert!(ledger.current(&ledger.notices[0]));
    assert_eq!(
        ledger.notices[0].event.destination,
        before[0].event.as_ref().unwrap().destination
    );
}

#[test]
fn merged_pruning_and_persisted_restart_never_reemit() {
    let snapshot = merged_snapshot(21);
    let (root, store) = store();
    let mut ledger = store.load_notifications().unwrap();
    ledger.observe(&frames(&snapshot), 201).unwrap();
    assert_eq!(ledger.notices.len(), 1);
    ledger.observe(&[], 202).unwrap();
    // Notification detail can be pruned independently of its durable merge tombstone.
    ledger.notices.clear();
    store.save_notifications(&ledger).unwrap();
    let restored = Store::new(root.path().into());
    restore(&restored).unwrap();
    let mut ledger = restored.load_notifications().unwrap();
    ledger.observe(&frames(&snapshot), 203).unwrap();
    assert!(ledger.notices.is_empty());
    assert!(!ledger.observe(&frames(&snapshot), 204).unwrap());
}

#[test]
fn legacy_merge_upgrade_baselines_history_without_replaying_or_deleting_notices() {
    let mut old = Ledger {
        enabled: true,
        ..Default::default()
    };
    old.observe(
        &[queue_frame("historical-item", Category::Merged, "old")],
        1,
    )
    .unwrap();
    let legacy_id = old.notices[0].id.clone();
    let mut json = serde_json::to_value(&old).unwrap();
    json.as_object_mut().unwrap().remove("merged_sources");
    let mut ledger: Ledger = serde_json::from_value(json).unwrap();
    ledger.validate().unwrap();
    let mut snapshot = merged_snapshot(21);
    ledger.observe(&frames(&snapshot), 201).unwrap();
    assert_eq!(ledger.notices.len(), 1);
    assert_eq!(ledger.notices[0].id, legacy_id);
    assert_eq!(ledger.notices[0].phase, Phase::NotSent);
    assert!(ledger.next().is_none());
    snapshot.items[0].job.pull_request_id = "newly-merged-pr".into();
    ledger.observe(&frames(&snapshot), 202).unwrap();
    assert_eq!(ledger.notices.len(), 2);
    assert_eq!(ledger.next().unwrap().event.category, Category::Merged);
}

#[test]
fn merge_upgrade_preserves_other_categories_and_interrupted_delivery_evidence() {
    let mut old = Ledger {
        enabled: true,
        ..Default::default()
    };
    let initial = [
        queue_frame("ready", Category::Ready, "review"),
        queue_frame("approved", Category::Approved, "approval"),
        queue_frame("failure", Category::Failure, "failure"),
        queue_frame("merged", Category::Merged, "merge"),
    ];
    old.observe(&initial, 1).unwrap();
    let merge_id = old.notices[3].id.clone();
    old.begin(&merge_id).unwrap();
    let mut json = serde_json::to_value(&old).unwrap();
    json.as_object_mut().unwrap().remove("merged_sources");
    let mut ledger: Ledger = serde_json::from_value(json).unwrap();
    assert!(ledger.restore());
    let mut current = initial[..3].to_vec();
    current.extend(frames(&merged_snapshot(21)));
    ledger.observe(&current, 201).unwrap();
    assert_eq!(ledger.notices.len(), 4);
    assert!(ledger.notices[..3]
        .iter()
        .all(|notice| notice.phase == Phase::Queued && ledger.current(notice)));
    assert_eq!(ledger.notices[3].phase, Phase::OutcomeUnknown);
    assert!(ledger.notices[3].error.is_some());
    let mut restored: Ledger =
        serde_json::from_slice(&serde_json::to_vec(&ledger).unwrap()).unwrap();
    assert!(!restored.observe(&current, 202).unwrap());
    assert_eq!(restored.notices.len(), 4);
}

#[test]
fn muted_merges_remain_consumed_after_retention_and_unmuting() {
    let snapshot = merged_snapshot(21);
    let mut ledger = Ledger {
        enabled: true,
        ..Default::default()
    };
    ledger.set_category(Category::Merged, false).unwrap();
    ledger.observe(&frames(&snapshot), 201).unwrap();
    ledger.observe(&[], 202).unwrap();
    ledger.set_category(Category::Merged, true).unwrap();
    ledger.observe(&frames(&snapshot), 203).unwrap();
    assert!(ledger.notices.is_empty());
}

#[test]
fn exact_destinations_and_profile_identity_survive_restart_without_fallback() {
    let (root, store) = store();
    store.save_queue(&[job()]).unwrap();
    let item_id = queue::item_id(&job());
    let exact = [queue_frame(&item_id, Category::Ready, "review")];
    let id = admit(&store, &exact);
    assert_eq!(
        destination(&Store::new(root.path().into()), &id).unwrap(),
        Destination::QueueItem {
            item_id: item_id.clone()
        }
    );
    assert_eq!(
        queue::destination(&store, &item_id, None).unwrap().as_str(),
        "https://github.com/example/repo/pull/9"
    );
    let (other_root, other_store) = self::store();
    assert!(destination(&other_store, &id).is_err());
    assert!(destination(&store, "unknown").is_err());
    store.save_queue(&[]).unwrap();
    let missing = prepare(&store, &exact, &id, 101).unwrap();
    assert_eq!(missing.phase, Phase::NotSent);
    assert_ne!(
        missing.error.as_deref(),
        Some("The transition changed or notifications were disabled before submission.")
    );
    assert_ne!(root.path(), other_root.path());
}

#[test]
fn only_operator_attention_maps_to_alerts_and_routine_author_waits_do_not() {
    let states = [
        (
            queue::State::ConfirmationRequired,
            Some(Category::Confirmation),
        ),
        (queue::State::WaitingForHuman, Some(Category::HumanInput)),
        (queue::State::MachineSignedOff, Some(Category::Ready)),
        (queue::State::Failed, Some(Category::Failure)),
        (queue::State::StaleAfterPublication, Some(Category::Failure)),
        (queue::State::WaitingForAuthor, None),
        (queue::State::Reviewing, None),
        (queue::State::Queued, None),
        (queue::State::Stale, None),
        (queue::State::Closed, None),
        (queue::State::Merged, None),
    ];
    for (state, category) in states {
        let snapshot = queue::Snapshot {
            pending_threads: vec![],
            feedback: Default::default(),
            mentions: vec![],
            tracked: vec![],
            health: vec![],
            jobs: vec![],
            reviews: vec![],
            publications: vec![],
            follow_ups: vec![],
            exit_writes: vec![],
            pr_states: Default::default(),
            items: vec![queue::Item {
                dismiss_token: None,
                dismissed: false,
                action_status: None,
                exit: None,
                exit_run_id: None,
                progress: None,
                feedback: vec![],
                aliases: vec![],
                id: queue::item_id(&job()),
                job: job(),
                state,
                failure_class: None,
                failure_cause: None,
                failed_teammate: None,
                failed_duck: None,
                blocked_by: None,
                resume_at: None,
                summary: "",
                review_keys: vec![],
                follow_up_ids: vec![],
                warnings: vec![],
            }],
        };
        let frame = frames(&snapshot).pop().unwrap();
        assert_eq!(frame.event.as_ref().map(|e| e.category), category);
        if let Some(event) = frame.event {
            // The persisted ledger keeps only digests; PR text is rendered at send time.
            let saved = serde_json::to_string(&event).unwrap();
            assert!(!saved.contains("Sensitive") && !saved.contains("example/repo"));
            let message = render(
                &queue_notice(event.category),
                Some(&Subject::of(&snapshot.items[0])),
            );
            assert_eq!(
                message.title,
                format!("{} · repo#9", event.category.state())
            );
            assert!(message.body.starts_with("Sensitive PR title"));
            assert!(!message.body.contains("example/repo"));
        }
    }
}

#[test]
fn authentication_or_schedule_failure_without_a_pr_opens_settings() {
    let health = serde_json::from_value(serde_json::json!({
        "repository_id":"config","name":"example/repo","schedule_key":"interval:5:UTC",
        "provider_account_id":"22","enabled":true,"last_attempt":100,"last_success":null,
        "next_run":0,"schedule_available":false,"last_failure":"account_disconnected","in_flight":false
    })).unwrap();
    let snapshot = queue::Snapshot {
        pending_threads: vec![],
        feedback: Default::default(),
        mentions: vec![],
        tracked: vec![],
        health: vec![health],
        jobs: vec![],
        reviews: vec![],
        publications: vec![],
        follow_ups: vec![],
        exit_writes: vec![],
        pr_states: Default::default(),
        items: vec![],
    };
    let frames = frames(&snapshot);
    assert_eq!(
        frames[0].event.as_ref().unwrap().destination,
        Destination::Settings
    );
    assert_eq!(
        frames[0].event.as_ref().unwrap().category,
        Category::Failure
    );
}

#[test]
fn opt_in_and_dedup_are_independent_of_permission_queries_and_confirmations() {
    let mut ledger = Ledger::default();
    assert!(!ledger.enabled);
    assert!(!ledger
        .observe(&[frame(Category::Ready, "review")], 1)
        .unwrap());
    assert!(ledger.notices.is_empty());
    let first = ledger.enable(&[], 3).unwrap().unwrap();
    assert_eq!(ledger.enable(&[], 4).unwrap(), None);
    ledger
        .observe(&[frame(Category::Ready, "review")], 5)
        .unwrap();
    assert_eq!(ledger.next().unwrap().id, first);
    assert_eq!(ledger.notices.len(), 2);
}

fn queue_frame(item: &str, category: Category, cause: &str) -> Frame {
    Frame {
        source: format!("queue:{item}"),
        event: Some(Event {
            category,
            destination: Destination::QueueItem {
                item_id: item.into(),
            },
            cause: cause.into(),
            group: "repository-group".into(),
        }),
    }
}

#[test]
fn enabling_does_not_flood_existing_queue_events() {
    let existing = [
        queue_frame("a", Category::Ready, "review-a"),
        queue_frame("b", Category::HumanInput, "comment-b"),
        queue_frame("c", Category::Failure, "failure-c"),
    ];
    let mut ledger = Ledger::default();
    assert!(!ledger.observe(&existing, 1).unwrap());
    // The pump observes the same queue right after the preference is saved.
    let confirmation = ledger.enable(&existing, 2).unwrap().unwrap();
    ledger.observe(&existing, 3).unwrap();
    let events: Vec<_> = ledger
        .notices
        .iter()
        .filter(|n| n.event.category != Category::Test)
        .collect();
    assert!(events.is_empty(), "flooded {} notices", events.len());
    assert_eq!(ledger.notices.len(), 1);
    let notice = ledger.next().unwrap();
    assert_eq!(notice.id, confirmation);
    assert_eq!(notice.source, ENABLED_SOURCE);
    assert_eq!(notice.event.destination, Destination::Settings);
    assert!(ledger.current(notice));
}

fn events(ledger: &Ledger) -> Vec<&Notice> {
    ledger
        .notices
        .iter()
        .filter(|n| n.event.category != Category::Test)
        .collect()
}

#[test]
fn only_events_after_enabling_notify_once() {
    let existing = [queue_frame("a", Category::Ready, "review-a")];
    let mut ledger = Ledger::default();
    ledger.enable(&existing, 1).unwrap();
    let later = [
        queue_frame("a", Category::Ready, "review-a"),
        queue_frame("b", Category::HumanInput, "comment-b"),
    ];
    assert!(ledger.observe(&later, 2).unwrap());
    assert!(!ledger.observe(&later, 3).unwrap());
    let fresh = events(&ledger);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].source, "queue:b");
    assert!(ledger.current(fresh[0]));
}

#[test]
fn reenabling_ignores_events_from_while_off_and_drops_stale_unsent_notices() {
    let mut ledger = Ledger::default();
    let confirmation = ledger.enable(&[], 1).unwrap().unwrap();
    let before = [queue_frame("a", Category::Ready, "review-a")];
    ledger.observe(&before, 2).unwrap();
    let stale = events(&ledger)[0].id.clone();
    assert!(ledger.disable());
    assert!(!ledger.disable());
    for id in [&confirmation, &stale] {
        let notice = ledger.notices.iter().find(|n| &n.id == id).unwrap();
        assert_eq!(notice.phase, Phase::NotSent);
        assert_eq!(
            notice.error.as_deref(),
            Some("Notifications were turned off before this was sent.")
        );
    }
    let while_off = [
        queue_frame("a", Category::Ready, "review-a"),
        queue_frame("b", Category::Failure, "failure-b"),
    ];
    assert!(!ledger.observe(&while_off, 3).unwrap());
    let second = ledger.enable(&while_off, 4).unwrap().unwrap();
    ledger.observe(&while_off, 5).unwrap();
    assert_eq!(ledger.next().unwrap().id, second);
    assert_eq!(
        ledger
            .notices
            .iter()
            .filter(|n| n.phase == Phase::Queued)
            .count(),
        1
    );
    assert_eq!(
        events(&ledger).len(),
        1,
        "only the dropped pre-disable notice"
    );
    assert_eq!(ledger.notices.len(), 3, "history is preserved");
}

#[test]
fn legacy_queued_notices_are_not_sent_when_enabling() {
    // Ledgers saved before this fix may hold unsent notices while disabled.
    let mut ledger = Ledger {
        enabled: true,
        ..Default::default()
    };
    let frames = [queue_frame("a", Category::Ready, "review-a")];
    ledger.observe(&frames, 1).unwrap();
    ledger.enabled = false;
    ledger.enable(&frames, 2).unwrap();
    let old = events(&ledger)[0];
    assert_eq!(old.phase, Phase::NotSent);
    assert_eq!(ledger.next().unwrap().event.category, Category::Test);
}

#[test]
fn muted_types_never_notify_and_unmuting_does_not_replay() {
    let mut ledger = Ledger::default();
    ledger.enable(&[], 1).unwrap();
    assert!(ledger.set_category(Category::Ready, false).unwrap());
    assert!(!ledger.set_category(Category::Ready, false).unwrap());
    let frames = [
        queue_frame("a", Category::Ready, "review-a"),
        queue_frame("b", Category::Failure, "failure-b"),
    ];
    ledger.observe(&frames, 2).unwrap();
    let sent: Vec<_> = events(&ledger).iter().map(|n| n.source.clone()).collect();
    assert_eq!(sent, ["queue:b"], "other types keep notifying");
    assert!(ledger.set_category(Category::Ready, true).unwrap());
    assert!(!ledger.set_category(Category::Ready, true).unwrap());
    assert!(!ledger.observe(&frames, 3).unwrap());
    assert_eq!(events(&ledger).len(), 1, "existing Ready is not replayed");
    let next = [
        queue_frame("a", Category::Ready, "review-a2"),
        queue_frame("b", Category::Failure, "failure-b"),
    ];
    ledger.observe(&next, 4).unwrap();
    let ready: Vec<_> = events(&ledger)
        .into_iter()
        .filter(|n| n.event.category == Category::Ready)
        .collect();
    assert_eq!(ready.len(), 1);
    assert!(ledger.current(ready[0]));
}

#[test]
fn muting_drops_only_that_types_unsent_notices() {
    let mut ledger = Ledger::default();
    ledger.enable(&[], 1).unwrap();
    let frames = [
        queue_frame("a", Category::Ready, "review-a"),
        queue_frame("b", Category::Failure, "failure-b"),
    ];
    ledger.observe(&frames, 2).unwrap();
    ledger.set_category(Category::Failure, false).unwrap();
    let failure = ledger
        .notices
        .iter()
        .find(|n| n.event.category == Category::Failure)
        .unwrap();
    assert_eq!(failure.phase, Phase::NotSent);
    assert_eq!(
        failure.error.as_deref(),
        Some("This notification type was turned off before this was sent.")
    );
    let ready = ledger
        .notices
        .iter()
        .find(|n| n.event.category == Category::Ready)
        .unwrap();
    assert_eq!(ready.phase, Phase::Queued);
    assert!(ledger.current(ready));
    assert!(ledger.set_category(Category::Test, false).is_err());
    assert!(ledger.set_category(Category::Test, true).is_err());
}

#[test]
fn mute_racing_a_send_stops_before_native_submission() {
    let (_root, store) = store();
    let id = admit(&store, &[frame(Category::Ready, "review")]);
    let mut ledger = store.load_notifications().unwrap();
    ledger.muted.insert(Category::Ready);
    store.save_notifications(&ledger).unwrap();
    assert_eq!(
        prepare(&store, &[frame(Category::Ready, "review")], &id, 101)
            .unwrap()
            .phase,
        Phase::NotSent
    );
}

#[test]
fn ledgers_without_type_preferences_load_with_every_type_on() {
    let (root, store) = store();
    let path = root.path().join("state/notifications.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(value.as_object_mut().unwrap().remove("muted").is_some());
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let ledger = store.load_notifications().unwrap();
    assert!(ledger.muted.is_empty());
    let snapshot = snapshot(&store, Err("fixture".into())).unwrap();
    assert_eq!(
        snapshot
            .categories
            .iter()
            .map(|c| (c.category, c.enabled))
            .collect::<Vec<_>>(),
        Category::SELECTABLE.map(|c| (c, true))
    );
    let mut ledger = ledger;
    ledger.set_category(Category::Merged, false).unwrap();
    store.save_notifications(&ledger).unwrap();
    let reloaded = Store::new(root.path().into()).load_notifications().unwrap();
    assert_eq!(reloaded.muted, BTreeSet::from([Category::Merged]));
    let mut invalid = reloaded;
    invalid.muted.insert(Category::Test);
    assert!(invalid.validate().is_err());
}

#[test]
fn superseded_opt_in_saves_nothing_and_queues_no_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    restore(&store).unwrap();
    let before = std::fs::read(root.path().join("state/notifications.json")).unwrap();
    let baseline = [queue_frame("a", Category::Ready, "review-a")];
    // A newer request bumped the generation between the queue snapshot and the save.
    let generation = std::sync::atomic::AtomicU64::new(2);
    let current = || generation.load(Ordering::SeqCst) == 1;
    assert_eq!(
        save_preference(&store, true, &baseline, 1, current),
        Err(SUPERSEDED.into())
    );
    assert_eq!(
        std::fs::read(root.path().join("state/notifications.json")).unwrap(),
        before
    );
    let ledger = store.load_notifications().unwrap();
    assert!(!ledger.enabled);
    assert!(ledger.notices.is_empty());
    assert!(ledger.observations.is_empty());

    generation.store(1, Ordering::SeqCst);
    let id = save_preference(&store, true, &baseline, 2, current)
        .unwrap()
        .unwrap();
    let ledger = store.load_notifications().unwrap();
    assert!(ledger.enabled);
    assert_eq!(ledger.next().unwrap().id, id);

    generation.store(3, Ordering::SeqCst);
    assert_eq!(
        save_preference(&store, false, &[], 3, current),
        Err(SUPERSEDED.into())
    );
    assert!(store.load_notifications().unwrap().enabled);
    assert_eq!(
        save_category(&store, Category::Ready, false, || false),
        Err(SUPERSEDED.into())
    );
    assert!(store.load_notifications().unwrap().muted.is_empty());
    save_category(&store, Category::Ready, false, || true).unwrap();
    assert!(store
        .load_notifications()
        .unwrap()
        .muted
        .contains(&Category::Ready));
}

fn notice(category: Category, destination: Destination) -> Notice {
    Notice {
        id: "pr-sniper:profile:notice".into(),
        source: "queue:item".into(),
        fingerprint: "fingerprint".into(),
        episode: 1,
        event: Event {
            category,
            destination,
            cause: "cause".into(),
            group: "group".into(),
        },
        phase: Phase::Submitting,
        created_at: 100,
        error: None,
        opened_at: None,
        navigation_error: None,
        failure_cause: None,
    }
}

fn subject(title: &str, failure_cause: Option<&'static str>) -> Subject {
    Subject {
        repository: "jdylanmc/pr-sniper".into(),
        number: 219,
        title: title.into(),
        failure_cause: failure_cause.map(Into::into),
    }
}

fn queue_notice(category: Category) -> Notice {
    notice(
        category,
        Destination::QueueItem {
            item_id: "item".into(),
        },
    )
}

#[test]
fn copy_is_state_first_and_names_the_pr_once() {
    let pr = subject("Fix flaky intake test", None);
    let cases = [
        (
            Category::Confirmation,
            "Confirm · pr-sniper#219",
            "Fix flaky intake test",
        ),
        (
            Category::HumanInput,
            "Needs you · pr-sniper#219",
            "Fix flaky intake test",
        ),
        (
            Category::Ready,
            "Ready · pr-sniper#219",
            "Fix flaky intake test · Needs your review",
        ),
        (
            Category::FinalReviewCleared,
            "Cleared · pr-sniper#219",
            "Fix flaky intake test · Needs your review",
        ),
        (
            Category::Approved,
            "Approved · pr-sniper#219",
            "Fix flaky intake test · Needs your review",
        ),
        (
            Category::Merged,
            "Merged · pr-sniper#219",
            "Fix flaky intake test",
        ),
        (
            Category::Failure,
            "Failed · pr-sniper#219",
            "Fix flaky intake test",
        ),
    ];
    for (category, title, body) in cases {
        let message = render(&queue_notice(category), Some(&pr));
        assert_eq!(
            (message.title.as_str(), message.body.as_str()),
            (title, body)
        );
    }
    let failed = render(
        &queue_notice(Category::Failure),
        Some(&subject(
            "Fix flaky intake test",
            Some("publication_failed"),
        )),
    );
    assert_eq!(failed.body, "Fix flaky intake test · Publish failed");
    let test = render(&notice(Category::Test, Destination::Settings), None);
    assert_eq!(
        (test.title.as_str(), test.body.as_str()),
        ("Notifications on", "PR alerts show here")
    );
    let health = render(&notice(Category::Failure, Destination::Settings), None);
    assert_eq!(
        (health.title.as_str(), health.body.as_str()),
        ("Failed · Repo check", "Open Settings")
    );
    let missing = render(&queue_notice(Category::HumanInput), None);
    assert_eq!(
        (missing.title.as_str(), missing.body.as_str()),
        ("Needs you", "Open the queue")
    );
}

#[test]
fn every_message_fits_limits_and_starts_with_its_state_word() {
    let long = subject(
        &"Refactor the monitoring scheduler so it never ".repeat(5),
        Some(queue::CAUSE_TOSSED),
    );
    let mut long_repository = long.clone();
    long_repository.repository = format!("org/{}", "very-long-repository-name-".repeat(4));
    long_repository.number = u64::MAX;
    let empty = subject("", None);
    let banned = [
        "PR Sniper",
        "Automated",
        "exit",
        "packet",
        "disposition",
        "operation",
    ];
    for category in [Category::Test].into_iter().chain(Category::SELECTABLE) {
        for destination in [
            Destination::Settings,
            Destination::QueueItem {
                item_id: "item".into(),
            },
        ] {
            for pr in [None, Some(&long), Some(&long_repository), Some(&empty)] {
                let message = render(&notice(category, destination.clone()), pr);
                assert!(message.title.starts_with(category.state()), "{message:?}");
                assert!(message.title.chars().count() <= TITLE_MAX, "{message:?}");
                assert!(message.body.chars().count() <= BODY_MAX, "{message:?}");
                assert!(
                    !message.body.is_empty() && !message.body.starts_with(' '),
                    "{message:?}"
                );
                assert_eq!(message.badge, category.badge());
                for word in banned {
                    assert!(
                        !message.title.contains(word) && !message.body.contains(word),
                        "{message:?}"
                    );
                }
            }
        }
    }
    let message = render(&queue_notice(Category::Ready), Some(&long));
    assert!(
        message.body.ends_with("… · Needs your review"),
        "{message:?}"
    );
}

#[test]
fn provider_text_cannot_break_lines_or_reorder_the_notification() {
    let mut pr = subject("Fix\nthe\u{202E}gnp.exe\u{2066} \t thing\u{7}", None);
    pr.repository = "o/re\u{202E}po\nx".into();
    let message = render(&queue_notice(Category::Merged), Some(&pr));
    assert_eq!(message.title, "Merged · repo x#219");
    assert_eq!(message.body, "Fix thegnp.exe thing");
    // Arabic letter mark (bidi) and the XML-invalid noncharacters a toast rejects.
    let mut pr = subject("Fix \u{FFFF}a\u{061C}b\u{FFFE}", None);
    pr.repository = "o/re\u{061C}po\u{FFFF}".into();
    let message = render(&queue_notice(Category::Merged), Some(&pr));
    assert_eq!(message.title, "Merged · repo#219");
    assert_eq!(message.body, "Fix ab");
}

#[cfg(windows)]
#[test]
fn a_toast_with_xml_invalid_provider_text_still_loads() {
    let pr = subject("Fix \u{FFFF} and \u{FFFE}", None);
    let message = render(&queue_notice(Category::Merged), Some(&pr));
    let xml = ::windows::Data::Xml::Dom::XmlDocument::new().unwrap();
    xml.LoadXml(&::windows::core::HSTRING::from(
        super::windows_native::toast_xml("id", &message.title, &message.body, None),
    ))
    .unwrap();
}

#[test]
fn live_queue_item_supplies_the_subject() {
    let snapshot = merged_snapshot(1);
    let item = &snapshot.items[0];
    let found = Subject::find(
        &snapshot,
        &notice(
            Category::Merged,
            Destination::QueueItem {
                item_id: item.id.clone(),
            },
        ),
    )
    .unwrap();
    assert_eq!(found, Subject::of(item));
    let message = render(&queue_notice(Category::Merged), Some(&found));
    assert_eq!(message.title, "Merged · repo#9");
    assert_eq!(message.body, "Sensitive PR title");
    assert!(Subject::find(&snapshot, &queue_notice(Category::Merged)).is_none());
    // A notice saved under a legacy item id still finds the live item.
    let mut aliased = snapshot.clone();
    aliased.items[0].id = "current".into();
    aliased.items[0].aliases = vec!["legacy".into()];
    let legacy = notice(
        Category::Merged,
        Destination::QueueItem {
            item_id: "legacy".into(),
        },
    );
    let found = Subject::find(&aliased, &legacy).unwrap();
    let message = render(&legacy, Some(&found));
    assert_eq!(message.title, "Merged · repo#9");
    assert_eq!(message.body, "Sensitive PR title");
    assert!(Subject::find(&snapshot, &notice(Category::Failure, Destination::Settings)).is_none());
}

fn failed_snapshot(cause: &'static str, failure: &str) -> queue::Snapshot {
    let mut snapshot = merged_snapshot(1);
    snapshot.items[0].state = queue::State::Failed;
    snapshot.items[0].failure_cause = Some(cause);
    snapshot.health = vec![serde_json::from_value(serde_json::json!({
        "repository_id":"config","name":"example/repo","schedule_key":"interval:5:UTC",
        "provider_account_id":"22","enabled":true,"last_attempt":100,"last_success":null,
        "next_run":0,"schedule_available":true,"last_failure":failure,"in_flight":false
    }))
    .unwrap()];
    snapshot
}

/// Sends every queued notice against `snapshot` as the native loop does.
fn send_all(store: &Store, snapshot: &queue::Snapshot) {
    while let Some(next) = observe_queue(store, snapshot, 200).unwrap() {
        let notice = prepare_queue(store, snapshot, &next.id, 200).unwrap();
        if notice.phase == Phase::Submitting {
            complete(store, &notice.id, Phase::AcceptedUnconfirmed, None).unwrap();
        }
    }
}

fn history_bodies(store: &Store, live: &queue::Snapshot) -> Vec<String> {
    store
        .load_notifications()
        .unwrap()
        .notices
        .iter()
        .filter(|n| n.event.category == Category::Failure)
        .map(|n| render(n, Subject::saved(live, n).as_ref()).body)
        .collect()
}

#[test]
fn history_keeps_each_failure_notice_to_its_own_reason() {
    let (_root, store) = store();
    send_all(&store, &failed_snapshot(queue::CAUSE_PANIC, "first"));
    let live = failed_snapshot("publication_failed", "second");
    send_all(&store, &live);
    assert_eq!(
        history_bodies(&store, &live),
        [
            "Sensitive PR title · Review crashed",
            "Sensitive PR title · Publish failed"
        ]
    );
}

// An uncertain publication reconciled to a definite failure keeps its operation,
// so the item's event and digest stay the same and no new notice is sent.
#[test]
fn history_keeps_the_sent_reason_when_the_cause_changes_without_a_new_event() {
    let (_root, store) = store();
    let unconfirmed = failed_snapshot("publication_unresolved", "same");
    send_all(&store, &unconfirmed);
    let live = failed_snapshot("publication_failed", "same");
    let events = |s: &queue::Snapshot| frames(s).into_iter().map(|f| f.event).collect::<Vec<_>>();
    assert_eq!(events(&unconfirmed), events(&live));
    send_all(&store, &live);
    assert_eq!(
        history_bodies(&store, &live),
        ["Sensitive PR title · Publish unconfirmed"]
    );
}

#[test]
fn states_map_to_bundled_distinct_badges() {
    use Category::*;
    for (category, badge) in [
        (Approved, Badge::Success),
        (Merged, Badge::Success),
        (Confirmation, Badge::Attention),
        (HumanInput, Badge::Attention),
        (Ready, Badge::Ready),
        (FinalReviewCleared, Badge::Ready),
        (Test, Badge::Ready),
        (Failure, Badge::Failure),
    ] {
        assert_eq!(category.badge(), badge, "{category:?}");
    }
    let mut seen = BTreeSet::new();
    for badge in Badge::ALL {
        let png = badge.png();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{badge:?}");
        assert_eq!(&png[12..16], b"IHDR");
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!((width, height), (256, 256), "{badge:?}");
        assert_eq!(png[25], 6, "{badge:?} must be RGBA");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("icons/notifications")
            .join(badge.file_name());
        assert_eq!(std::fs::read(source).unwrap(), png);
        assert!(seen.insert(png), "{badge:?} duplicates another badge");
    }
}

#[test]
fn badge_write_is_idempotent_and_repairs_changed_files() {
    let root = tempfile::tempdir().unwrap();
    let directory = badge_directory(root.path());
    let path = Badge::Failure.write(&directory, "failure.png").unwrap();
    assert_eq!(path, directory.join("failure.png"));
    std::fs::write(&path, b"tampered").unwrap();
    Badge::Failure.write(&directory, "failure.png").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), Badge::Failure.png());
}

#[test]
fn windows_badge_uris_drop_verbatim_prefixes_and_escape_paths() {
    use super::message::windows_file_uri;
    assert_eq!(
        windows_file_uri(r"\\?\C:\Users\A B\AppData\notification-badges\ready.png"),
        "file:///C:/Users/A%20B/AppData/notification-badges/ready.png"
    );
    assert_eq!(
        windows_file_uri(r"\\?\UNC\server\share\é#.png"),
        "file://server/share/%C3%A9%23.png"
    );
    assert_eq!(windows_file_uri(r"D:\x\y.png"), "file:///D:/x/y.png");
}
