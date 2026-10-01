use super::*;
use crate::monitoring::QueueJob;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../tests/support/windows_permissions.rs"]
mod windows_permissions;

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
    fn send(&self, notice: &Notice) -> Result<(), SendError> {
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
    ledger.enabled = true;
    let id = ledger.enqueue_test(Destination::Settings, 101).unwrap();
    store.save_notifications(&ledger).unwrap();
    let wire = Wire {
        store: Store::new(root.path().into()),
        calls: AtomicUsize::new(0),
        failure: None,
    };
    let notice = prepare(&store, &[], &id, 102).unwrap();
    registration.notice_id(&notice.id).unwrap();
    assert_eq!(notice.id, id);
    assert_eq!(submit(&wire, &notice), (Phase::AcceptedUnconfirmed, None));
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
    let (phase, error) = submit(&wire, &notice);
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
            let (phase, error) = submit(&wire, &notice);
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
    let (phase, error) = submit(&wire, &notice);
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

#[test]
fn exact_destinations_and_profile_identity_survive_restart_without_fallback() {
    let (root, store) = store();
    store.save_queue(&[job()]).unwrap();
    let item_id = queue::item_id(&job());
    let mut ledger = store.load_notifications().unwrap();
    let id = ledger
        .enqueue_test(
            Destination::QueueItem {
                item_id: item_id.clone(),
            },
            100,
        )
        .unwrap();
    store.save_notifications(&ledger).unwrap();
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
    assert_eq!(
        prepare(&store, &[], &id, 101).unwrap().phase,
        Phase::NotSent
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
        (queue::State::Blocked, Some(Category::Failure)),
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
            feedback: Default::default(),
            mentions: vec![],
            global_scan: None,
            tracked: vec![],
            health: vec![],
            jobs: vec![],
            reviews: vec![],
            publications: vec![],
            follow_ups: vec![],
            items: vec![queue::Item {
                feedback: vec![],
                aliases: vec![],
                id: queue::item_id(&job()),
                job: job(),
                state,
                summary: "",
                review_keys: vec![],
                follow_up_ids: vec![],
                warnings: vec![],
            }],
        };
        let frame = frames(&snapshot).pop().unwrap();
        assert_eq!(frame.event.as_ref().map(|e| e.category), category);
        if let Some(event) = frame.event {
            assert!(!event.category.title().contains("Sensitive"));
            assert!(!event.category.body().contains("example/repo"));
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
        feedback: Default::default(),
        mentions: vec![],
        global_scan: None,
        tracked: vec![],
        health: vec![health],
        jobs: vec![],
        reviews: vec![],
        publications: vec![],
        follow_ups: vec![],
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
fn opt_in_and_dedup_are_independent_of_permission_queries_and_test_notifications() {
    let mut ledger = Ledger::default();
    assert!(!ledger.enabled);
    assert!(!ledger
        .observe(&[frame(Category::Ready, "review")], 1)
        .unwrap());
    assert!(ledger.notices.is_empty());
    assert!(ledger.enqueue_test(Destination::Settings, 2).is_err());
    ledger.enabled = true;
    let first = ledger.enqueue_test(Destination::Settings, 3).unwrap();
    let second = ledger.enqueue_test(Destination::Settings, 4).unwrap();
    assert_ne!(first, second);
    ledger
        .observe(&[frame(Category::Ready, "review")], 5)
        .unwrap();
    assert_eq!(ledger.next().unwrap().id, first);
    assert_eq!(ledger.notices.len(), 3);
}
