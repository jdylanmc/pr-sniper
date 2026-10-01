use super::*;
use crate::storage::Store;
use serde_json::json;

#[test]
fn physical_placement_clamps_all_tray_edges_and_mixed_dpi_negative_monitors() {
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        for work in [
            Rect {
                x: 0.0,
                y: 48.0,
                width: 2560.0,
                height: 1352.0,
            },
            Rect {
                x: -1920.0,
                y: -200.0,
                width: 1920.0,
                height: 1040.0,
            },
            Rect {
                x: 1000.0,
                y: 0.0,
                width: 640.0,
                height: 360.0,
            },
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 220.0,
            },
        ] {
            for tray in [
                Rect {
                    x: work.x + work.width - 30.0,
                    y: work.y - 24.0,
                    width: 24.0,
                    height: 24.0,
                },
                Rect {
                    x: work.x + work.width - 30.0,
                    y: work.y + work.height,
                    width: 24.0,
                    height: 24.0,
                },
                Rect {
                    x: work.x - 24.0,
                    y: work.y + 60.0,
                    width: 24.0,
                    height: 24.0,
                },
                Rect {
                    x: work.x + work.width,
                    y: work.y + 60.0,
                    width: 24.0,
                    height: 24.0,
                },
            ] {
                let frame = placement(work, tray, scale).unwrap();
                assert!(frame.x >= work.x && frame.y >= work.y);
                assert!(frame.x + frame.width <= work.x + work.width);
                assert!(frame.y + frame.height <= work.y + work.height);
                assert!(frame.width <= 400.0 * scale && frame.height <= 680.0 * scale);
            }
        }
    }
    let work = Rect {
        x: 0.0,
        y: 48.0,
        width: 2560.0,
        height: 1352.0,
    };
    let placed = placement(
        work,
        Rect {
            x: 2000.0,
            y: 0.0,
            width: 40.0,
            height: 48.0,
        },
        2.0,
    )
    .unwrap();
    assert_eq!(placed.width, 800.0);
    assert_eq!(placed.y, 64.0);
    assert!(placement(work, work, 0.0).is_err());
    assert!(placement(work, work, f64::NAN).is_err());
}

#[test]
fn tray_press_and_focus_loss_do_not_reopen_a_just_dismissed_panel() {
    let now = Instant::now();
    let mut session = Session {
        visible: true,
        ..Default::default()
    };
    session.tray_down_visible = Some(session.visible);
    session.visible = false;
    session.blur_from_tray = Some(now);
    assert!(session.should_toggle_closed(now + Duration::from_millis(100)));
    session.blur_from_tray = None;
    assert!(!session.should_toggle_closed(now + Duration::from_secs(1)));
    session.visible = true;
    assert!(session.should_toggle_closed(now + Duration::from_secs(2)));
    session.visible = false;
    session.blur_from_tray = Some(now);
    assert!(session.should_toggle_closed(now + Duration::from_millis(100)));
    assert!(!session.should_toggle_closed(now + Duration::from_secs(1)));
}

#[test]
fn retained_native_navigation_close_and_focus_lease_do_not_change_execution_state() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store
        .save_queue_selection(Some("saved-exact-destination"))
        .unwrap();
    let mut session = Session::default();
    assert!(!session.snapshot(&store).visible);
    session.navigate(&store, None).unwrap();
    session.set_visible(true);
    assert_eq!(
        session.snapshot(&store).route,
        Route::item("saved-exact-destination".into())
    );
    let route = Route {
        tab: Tab::Reviewed,
        detail: Some(Detail::Job {
            kind: Kind::Normal,
            id: "exact-job".into(),
        }),
    };
    session.navigate(&store, Some(route.clone())).unwrap();
    let revision = session.revision;
    assert!(session.blur_can_hide(revision));
    session.focus_holds += 1;
    assert!(!session.blur_can_hide(revision));
    session.focus_holds -= 1;
    session.set_visible(false);
    assert!(!session.blur_can_hide(revision));
    session.navigate(&store, None).unwrap();
    session.set_visible(true);
    assert_eq!(session.snapshot(&store).route, route);
    assert!(session.snapshot(&store).missing.is_some());
    let before = session.snapshot(&store);
    assert!(session
        .navigate(&store, Some(Route::item("\ninvalid".into())))
        .is_err());
    assert_eq!(session.snapshot(&store).route, before.route);
    assert_eq!(session.snapshot(&store).revision, before.revision);
    assert_eq!(
        store.load_queue_selection().unwrap().as_deref(),
        Some("saved-exact-destination")
    );
    assert!(store.load_reviews().unwrap().is_empty());
    assert!(store.load_actions().unwrap().effects.is_empty());
}

#[test]
fn explicit_close_clears_tray_blur_without_delaying_the_next_real_click() {
    let now = Instant::now();
    let mut session = Session {
        visible: true,
        blur_from_tray: Some(now),
        tray_down_visible: Some(true),
        ..Default::default()
    };
    session.dismiss(false);
    assert!(!session.should_toggle_closed(now + Duration::from_millis(1)));

    session.visible = true;
    session.blur_from_tray = Some(now);
    session.tray_down_visible = Some(true);
    session.dismiss(true);
    assert!(session.should_toggle_closed(now + Duration::from_millis(1)));
}

#[test]
fn unavailable_exact_routes_remain_requested_and_profile_scoped_without_substitution() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let other = Store::new(other.path().into());
    let route = Route::item("missing-exact-iteration".into());
    save_selection(&store, &route).unwrap();
    assert!(missing(&store, &route)
        .unwrap()
        .unwrap()
        .contains("No other"));
    assert_eq!(
        store.load_queue_selection().unwrap().as_deref(),
        Some("missing-exact-iteration")
    );
    assert_eq!(other.load_queue_selection().unwrap(), None);
    let job = Route {
        tab: Tab::Running,
        detail: Some(Detail::Job {
            kind: Kind::Mention,
            id: "missing-job".into(),
        }),
    };
    assert!(missing(&store, &job).unwrap().is_some());
    assert_eq!(
        store.load_queue_selection().unwrap().as_deref(),
        Some("missing-exact-iteration")
    );
    assert!(missing(&store, &Route::utility(true)).unwrap().is_none());
    assert!(Route::item(String::new()).validate().is_err());
    assert!(serde_json::from_value::<Route>(
        json!({"tab":"queue","url":"https://external.example/"})
    )
    .is_err());
}

#[test]
fn exact_iteration_and_normal_job_routing_uses_the_real_native_projection() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let settings=serde_json::from_value(json!({"launch_at_login":false,"doctrines":[],
        "repositories":[{"id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100","assignments":[{
                "id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","agent_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "schedule":{"kind":"cron","expression":"*/15 * * * *","timezone":"UTC"},"comment":false}]}],
        "agents":[{"id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","name":"Agent","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review.","signature":"fixture"}]})).unwrap();
    store.save_settings(&settings).unwrap();
    let job:crate::monitoring::QueueJob=serde_json::from_value(json!({
        "assignment_id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","provider":"github","account_id":"22","account_login":"actor",
        "configuration_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Exact","head_sha":"a".repeat(40),"observed_base_sha":"b".repeat(40),
        "trigger_policy":"[[],true]","author_id":"11","author_login":"author","watched_author":false,"all_authors":true,
        "requested_reviewer":false,"waiting":"trust_confirmation","detected_at":100})).unwrap();
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    assert!(missing(&store, &Route::item(queue::item_id(&job)))
        .unwrap()
        .is_none());
    let key = crate::review::key(&job, job.assignment_id.as_ref().unwrap());
    assert!(missing(
        &store,
        &Route {
            tab: Tab::Running,
            detail: Some(Detail::Job {
                kind: Kind::Normal,
                id: key.clone()
            })
        }
    )
    .unwrap()
    .is_none());
    assert!(missing(
        &store,
        &Route {
            tab: Tab::Running,
            detail: Some(Detail::Job {
                kind: Kind::Mention,
                id: key
            })
        }
    )
    .unwrap()
    .is_some());
    assert!(store.load_reviews().unwrap().is_empty());
    assert!(store.load_actions().unwrap().effects.is_empty());
}
