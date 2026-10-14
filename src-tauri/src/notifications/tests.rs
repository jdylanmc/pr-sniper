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
            global_scan: None,
            tracked: vec![],
            health: vec![],
            jobs: vec![],
            reviews: vec![],
            publications: vec![],
            follow_ups: vec![],
            exit_writes: vec![],
            pr_states: Default::default(),
            items: vec![queue::Item {
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
        pending_threads: vec![],
        feedback: Default::default(),
        mentions: vec![],
        global_scan: None,
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
