use super::*;

fn display(bounds: Rect) -> Display {
    Display {
        work: Rect {
            y: bounds.y + 24.0,
            height: bounds.height - 24.0,
            ..bounds
        },
        bounds,
        scale: 1.0,
    }
}

fn laptop() -> Display {
    display(Rect {
        x: 0.0,
        y: 0.0,
        width: 1728.0,
        height: 1117.0,
    })
}

fn external() -> Display {
    display(Rect {
        x: 1728.0,
        y: 0.0,
        width: 1280.0,
        height: 1024.0,
    })
}

#[test]
fn mixed_scale_tray_uses_native_points_and_screen_in_both_enumeration_orders() {
    let laptop = laptop();
    let external = external();
    for displays in [[laptop, external], [external, laptop]] {
        // Native status-window points, not its independently scaled pixel rect.
        let (actual, recovered) = frame(
            &displays,
            Some(Rect {
                x: 2960.0,
                y: 0.0,
                width: 24.0,
                height: 24.0,
            }),
            Some(external.bounds),
            Some(laptop.bounds),
            Some(laptop.bounds),
        )
        .unwrap();
        assert!(!recovered);
        assert_eq!(
            actual,
            Rect {
                x: 2592.0,
                y: 32.0,
                width: 408.0,
                height: 744.0
            }
        );
    }
}

#[test]
fn reverse_scale_tray_on_laptop_applies_exact_clamped_point_frame() {
    for displays in [[laptop(), external()], [external(), laptop()]] {
        let (requested, recovered) = frame(
            &displays,
            Some(Rect {
                x: 1650.0,
                y: 0.0,
                width: 24.0,
                height: 24.0,
            }),
            Some(laptop().bounds),
            Some(external().bounds),
            Some(laptop().bounds),
        )
        .unwrap();
        assert!(!recovered);
        assert_eq!(
            requested,
            Rect {
                x: 1312.0,
                y: 32.0,
                width: 408.0,
                height: 744.0
            }
        );
        apply_frame(requested, 1117.0, |native| {
            assert_eq!(
                native,
                Rect {
                    x: 1312.0,
                    y: 341.0,
                    width: 408.0,
                    height: 744.0
                }
            );
            Ok(native)
        })
        .unwrap();
    }
}

#[test]
fn native_pointer_points_select_exact_display_without_primary_scale_conversion() {
    for displays in [[laptop(), external()], [external(), laptop()]] {
        let (actual, _) = frame(
            &displays,
            Some(Rect {
                x: 2970.0,
                y: 100.0,
                width: 1.0,
                height: 1.0,
            }),
            None,
            Some(laptop().bounds),
            Some(laptop().bounds),
        )
        .unwrap();
        assert_eq!(
            actual,
            Rect {
                x: 2592.0,
                y: 109.0,
                width: 408.0,
                height: 744.0
            }
        );
    }
}

#[test]
fn synchronous_point_adapter_ignores_old_scale_for_creation_reopen_and_detachment() {
    for (old_scale, target_scale) in [(1.0, 2.0), (2.0, 1.0)] {
        for lifecycle in ["first creation", "retained reopen", "detached fallback"] {
            let target = if target_scale == 2.0 {
                laptop()
            } else {
                external()
            };
            let anchor = Rect {
                x: target.bounds.x + 1000.0,
                y: 0.0,
                width: 24.0,
                height: 24.0,
            };
            let (requested, recovered) = frame(
                &[target],
                Some(anchor),
                (lifecycle != "detached fallback").then_some(target.bounds),
                Some(Rect {
                    x: -1920.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }),
                Some(target.bounds),
            )
            .unwrap();
            assert!(!recovered);
            assert_eq!(requested.width, 408.0);
            assert_eq!(requested.height, 744.0);
            let mut submitted = None;
            apply_frame(requested, 1117.0, |native| {
                // Same contract as NSWindow setFrame: points submitted directly,
                // independent of the retained window's old backing scale.
                submitted = Some(native);
                Ok(native)
            })
            .unwrap();
            assert_eq!(
                submitted.unwrap(),
                Rect {
                    x: target.bounds.x + 808.0,
                    y: 341.0,
                    width: 408.0,
                    height: 744.0,
                },
                "{lifecycle}, old={old_scale}, target={target_scale}"
            );
        }
    }
}

#[test]
fn detached_native_tray_screen_and_window_recover_to_primary_points() {
    let detached = Rect {
        x: 4000.0,
        y: -900.0,
        width: 1280.0,
        height: 1024.0,
    };
    let (actual, recovered) = frame(
        &[laptop()],
        Some(Rect {
            x: 5000.0,
            y: -900.0,
            width: 24.0,
            height: 24.0,
        }),
        Some(detached),
        Some(detached),
        Some(laptop().bounds),
    )
    .unwrap();
    assert!(recovered);
    assert_eq!(
        actual,
        Rect {
            x: 1312.0,
            y: 32.0,
            width: 408.0,
            height: 744.0
        }
    );
}

#[test]
fn primary_flip_roundtrips_negative_and_vertical_screen_points() {
    let native = Rect {
        x: -1280.0,
        y: 1117.0,
        width: 1280.0,
        height: 1024.0,
    };
    let normalized = top_left(native, 1117.0);
    assert_eq!(normalized.y, -1024.0);
    assert_eq!(top_left(normalized, 1117.0), native);
}

#[test]
fn readback_rejects_wrong_size_or_position_not_successful_queued_setters() {
    let requested = Rect {
        x: 1312.0,
        y: 32.0,
        width: 408.0,
        height: 744.0,
    };
    for actual in [
        Rect {
            x: 2624.0,
            y: 64.0,
            width: 816.0,
            height: 1488.0,
        },
        Rect {
            x: 1312.0,
            y: 341.0,
            width: 204.0,
            height: 372.0,
        },
    ] {
        assert!(apply_frame(requested, 1117.0, |_| Ok(actual)).is_err());
    }
}
