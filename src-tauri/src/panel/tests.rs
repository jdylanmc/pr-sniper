use super::*;
use crate::storage::Store;
use serde_json::json;

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn point(x: f64, y: f64) -> Rect {
    rect(x, y, 1.0, 1.0)
}

fn contained(frame: Rect, work: Rect) {
    assert!(frame.x >= work.x && frame.y >= work.y);
    assert!(frame.x + frame.width <= work.x + work.width);
    assert!(frame.y + frame.height <= work.y + work.height);
}

#[test]
fn removed_display_regression_keeps_the_whole_panel_on_an_attached_screen() {
    let attached = Screen {
        bounds: rect(0.0, 0.0, 1728.0, 1117.0),
        work: rect(0.0, 33.0, 1728.0, 994.0),
        scale: 1.0,
    };
    let stale_tray = rect(3200.0, -445.0, 24.0, 24.0);
    let (frame, warning) =
        resolve_placement(&[attached], None, Some(stale_tray), point(1110.0, 40.0)).unwrap();
    contained(frame, attached.work);
    assert!(warning);
}

#[test]
fn every_open_reselects_current_displays_after_docking_and_removal() {
    let laptop = Screen {
        bounds: rect(0.0, 0.0, 1728.0, 1117.0),
        work: rect(0.0, 33.0, 1728.0, 994.0),
        scale: 1.0,
    };
    let dock = Screen {
        bounds: rect(1728.0, -445.0, 2560.0, 1440.0),
        work: rect(1728.0, -412.0, 2560.0, 1350.0),
        scale: 1.0,
    };
    let old_tray = rect(3200.0, -445.0, 24.0, 24.0);
    let (before, _) = resolve_placement(
        &[laptop, dock],
        Some(old_tray),
        Some(old_tray),
        point(3205.0, -440.0),
    )
    .unwrap();
    contained(before, dock.work);
    // Both a delayed tray click and the queried icon can name a removed screen.
    // The old path fell back to the retained window's monitor here.
    let (after, warning) = resolve_placement(
        &[laptop],
        Some(old_tray),
        Some(old_tray),
        point(1110.0, 40.0),
    )
    .unwrap();
    contained(after, laptop.work);
    assert!(warning);
    let tray = rect(1085.0, 35.0, 63.0, 19.0);
    let (reopened, warning) =
        resolve_placement(&[laptop], None, Some(tray), point(200.0, 600.0)).unwrap();
    contained(reopened, laptop.work);
    assert!(!warning);
    assert_eq!(reopened.x, 912.5);
    assert_eq!(reopened.y, 62.0);
    // A fresh click on the dock after reattachment must not reuse the laptop.
    let (redocked, _) = resolve_placement(
        &[laptop, dock],
        Some(old_tray),
        Some(tray),
        point(10.0, 10.0),
    )
    .unwrap();
    contained(redocked, dock.work);
}

#[test]
fn clicked_screen_wins_over_other_tray_and_pointer_and_small_work_areas_fit() {
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let primary = Screen {
            bounds: rect(0.0, 0.0, 1920.0, 1080.0),
            work: rect(0.0, 32.0, 1920.0, 1000.0),
            scale: 1.0,
        };
        let clicked = Screen {
            bounds: rect(-640.0, -300.0, 640.0, 360.0),
            work: rect(-640.0, -270.0, 640.0, 300.0),
            scale,
        };
        let (frame, warning) = resolve_placement(
            &[primary, clicked],
            Some(point(-200.0, -285.0)),
            Some(rect(1500.0, 0.0, 24.0, 24.0)),
            point(1700.0, 800.0),
        )
        .unwrap();
        contained(frame, clicked.work);
        assert!(warning);
        assert!(frame.height < 744.0 * scale);
        let (no_tray, warning) =
            resolve_placement(&[primary, clicked], None, None, point(-200.0, -285.0)).unwrap();
        contained(no_tray, clicked.work);
        assert!(warning);
        let (no_pointer_display, warning) =
            resolve_placement(&[primary], None, None, point(9000.0, -9000.0)).unwrap();
        contained(no_pointer_display, primary.work);
        assert!(warning);
    }
    assert!(resolve_placement(&[], None, None, point(0.0, 0.0)).is_err());
    let invalid = Screen {
        bounds: rect(0.0, 0.0, 100.0, 100.0),
        work: rect(0.0, 0.0, f64::NAN, 100.0),
        scale: 1.0,
    };
    assert!(resolve_placement(&[invalid], None, None, point(1.0, 1.0)).is_err());
}

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
                assert!(frame.width <= 408.0 * scale && frame.height <= 744.0 * scale);
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
    assert_eq!(placed.width, 816.0);
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
fn retained_native_navigation_and_stale_blur_do_not_change_execution_state() {
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
    session.navigate(&store, Some(route.clone())).unwrap();
    assert!(!session.blur_can_hide(revision));
    assert!(session.blur_can_hide(session.revision));
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
