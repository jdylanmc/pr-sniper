use super::*;
use crate::storage::Store;
use serde_json::json;

fn laptop_display(scale: f64) -> Display {
    Display {
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        },
        work: Rect {
            x: 0.0,
            y: 30.0,
            width: 1920.0,
            height: 1010.0,
        },
        scale,
    }
}

fn detached_display() -> Rect {
    Rect {
        x: 2400.0,
        y: -900.0,
        width: 3840.0,
        height: 2160.0,
    }
}

#[test]
fn undocking_recovers_stale_tray_and_detached_window_to_connected_primary() {
    let primary = laptop_display(1.25);
    let tray = Rect {
        x: 6000.0,
        y: -900.0,
        width: 48.0,
        height: 48.0,
    };
    let (frame, recovered) = connected_placement(
        &[primary],
        Some(tray),
        Some(detached_display()),
        Some(primary.bounds),
    )
    .unwrap();
    assert!(recovered);
    assert_eq!(
        frame,
        Rect {
            x: 1400.0,
            y: 40.0,
            width: 510.0,
            height: 930.0,
        }
    );
}

#[test]
fn missing_tray_pointer_and_current_monitor_still_open_on_primary() {
    let primary = laptop_display(1.0);
    let secondary = Display {
        bounds: detached_display(),
        work: detached_display(),
        scale: 2.0,
    };
    let (frame, recovered) =
        connected_placement(&[secondary, primary], None, None, Some(primary.bounds)).unwrap();
    assert!(recovered);
    assert_eq!(
        frame,
        Rect {
            x: 1504.0,
            y: 39.0,
            width: 408.0,
            height: 744.0,
        }
    );
}

#[test]
fn stale_primary_lookup_uses_a_connected_display_not_the_detached_frame() {
    let display = laptop_display(1.0);
    let (frame, recovered) = connected_placement(
        &[display],
        None,
        Some(detached_display()),
        Some(detached_display()),
    )
    .unwrap();
    assert!(recovered);
    assert!(frame.x >= 0.0 && frame.x + frame.width <= 1920.0);
    assert!(frame.y >= 30.0 && frame.y + frame.height <= 1040.0);
}

#[test]
fn connected_tray_monitor_wins_over_retained_window_and_primary() {
    let primary = laptop_display(1.0);
    let secondary = Display {
        bounds: Rect {
            x: -2560.0,
            y: -200.0,
            width: 2560.0,
            height: 1440.0,
        },
        work: Rect {
            x: -2560.0,
            y: -150.0,
            width: 2560.0,
            height: 1390.0,
        },
        scale: 1.5,
    };
    let (frame, recovered) = connected_placement(
        &[primary, secondary],
        Some(Rect {
            x: -100.0,
            y: -200.0,
            width: 30.0,
            height: 40.0,
        }),
        Some(primary.bounds),
        Some(primary.bounds),
    )
    .unwrap();
    assert!(!recovered);
    assert_eq!(
        frame,
        Rect {
            x: -624.0,
            y: -138.0,
            width: 612.0,
            height: 1116.0,
        }
    );
}

#[test]
fn connected_current_monitor_uses_fresh_work_area_and_scale_on_every_reopen() {
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let current = laptop_display(scale);
        let other = Display {
            bounds: detached_display(),
            work: detached_display(),
            scale: 1.0,
        };
        let (frame, recovered) = connected_placement(
            &[other, current],
            Some(Rect {
                x: -9000.0,
                y: -9000.0,
                width: 40.0,
                height: 40.0,
            }),
            Some(current.bounds),
            Some(other.bounds),
        )
        .unwrap();
        assert!(recovered);
        assert_eq!(frame.width, 408.0 * scale);
        assert!(frame.height <= 744.0 * scale);
        assert!(frame.x >= 0.0 && frame.x + frame.width <= 1920.0);
        assert!(frame.y >= 30.0 && frame.y + frame.height <= 1040.0);
    }
}

#[test]
fn no_usable_connected_geometry_is_an_explicit_opening_failure() {
    assert!(connected_placement(&[], None, None, None)
        .unwrap_err()
        .contains("No connected display"));
    for scale in [0.0, f64::NAN, f64::INFINITY] {
        assert!(connected_placement(&[laptop_display(scale)], None, None, None).is_err());
    }
}

#[test]
fn panel_failure_and_recovery_evidence_is_durable_and_closed_schema() {
    use crate::storage::DiagnosticEvent;

    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.record(DiagnosticEvent::WindowOpenFailed).unwrap();
    store
        .record(DiagnosticEvent::WindowPlacementRecovered)
        .unwrap();
    let saved = std::fs::read_to_string(root.path().join("state/diagnostics.jsonl")).unwrap();
    let events: Vec<serde_json::Value> = saved
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], "window_open_failed");
    assert_eq!(events[1]["event"], "window_placement_recovered");
    for event in &events {
        assert_eq!(event.as_object().unwrap().len(), 2);
        assert!(event["timestamp_secs"].is_u64());
    }
    assert_eq!(
        Store::new(root.path().into()).diagnostics().unwrap().len(),
        2
    );
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
