use super::{center, resolve_placement, Rect, Screen};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

fn main_thread() -> Result<MainThreadMarker, String> {
    MainThreadMarker::new().ok_or("Panel geometry requires the native UI thread.".into())
}

// AppKit uses one global point space with y increasing upwards. Reflect it
// around zero, not a potentially changed Core Graphics primary-display height.
fn top_down(rect: NSRect) -> Rect {
    Rect {
        x: rect.origin.x,
        y: -rect.origin.y - rect.size.height,
        width: rect.size.width,
        height: rect.size.height,
    }
}

pub(super) fn pointer() -> Result<Rect, String> {
    main_thread()?;
    let point = NSEvent::mouseLocation();
    Ok(Rect {
        x: point.x,
        y: -point.y,
        width: 1.0,
        height: 1.0,
    })
}

fn tray_rect(app: &tauri::AppHandle) -> Result<Option<Rect>, String> {
    let tray = app.tray_by_id("pr-sniper").ok_or("Tray is unavailable.")?;
    tray.with_inner_tray_icon(|tray| {
        let mtm = main_thread()?;
        Ok(tray
            .ns_status_item()
            .and_then(|item| item.button(mtm))
            .and_then(|button| button.window())
            .map(|window| top_down(window.frame())))
    })
    .map_err(|_| "Native tray geometry unavailable.")?
}

pub(super) fn pointer_on_tray(app: &tauri::AppHandle) -> Result<bool, String> {
    let (x, y) = center(pointer()?);
    Ok(tray_rect(app)?.is_some_and(|tray| tray.contains(x, y)))
}

pub(super) fn position(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    clicked: Option<Rect>,
) -> Result<Option<String>, String> {
    let mtm = main_thread()?;
    let screens: Vec<_> = NSScreen::screens(mtm)
        .iter()
        .map(|screen| Screen {
            bounds: top_down(screen.frame()),
            work: top_down(screen.visibleFrame()),
            scale: 1.0,
        })
        .collect();
    let tray = tray_rect(app)?;
    let (frame, degraded) = resolve_placement(&screens, clicked, tray, pointer()?)?;
    let native = window
        .ns_window()
        .map_err(|_| "Native panel unavailable.")?;
    if native.is_null() {
        return Err("Native panel unavailable.".into());
    }
    // Tauri owns this NSWindow; this borrow never leaves its main-thread call.
    let native: &NSWindow = unsafe { &*native.cast() };
    native.setFrame_display(
        NSRect::new(
            NSPoint::new(frame.x, -frame.y - frame.height),
            NSSize::new(frame.width, frame.height),
        ),
        false,
    );
    let actual = top_down(native.frame());
    if [
        actual.x - frame.x,
        actual.y - frame.y,
        actual.width - frame.width,
        actual.height - frame.height,
    ]
    .iter()
    .any(|delta| !delta.is_finite() || delta.abs() > 1.0)
    {
        return Err("Native panel did not accept the current screen placement.".into());
    }
    if std::env::var_os("PR_SNIPER_PANEL_DEBUG").is_some() {
        eprintln!(
            "[panel] appkit monitors={} tray={tray:?} clicked={clicked:?} frame={frame:?}",
            screens.len()
        );
    }
    Ok(degraded
        .then(|| "Tray geometry unavailable; the panel is clamped to an available monitor.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_point_space_does_not_depend_on_old_primary_height_or_screen_scale() {
        let work = NSRect::new(NSPoint::new(0.0, 90.0), NSSize::new(1728.0, 994.0));
        let tray = NSRect::new(NSPoint::new(1085.0, 1063.0), NSSize::new(63.0, 19.0));
        let screens = [Screen {
            bounds: top_down(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(1728.0, 1117.0),
            )),
            work: top_down(work),
            scale: 1.0,
        }];
        let (frame, warning) = resolve_placement(
            &screens,
            None,
            Some(top_down(tray)),
            Rect {
                x: 1100.0,
                y: -1070.0,
                width: 1.0,
                height: 1.0,
            },
        )
        .unwrap();
        assert!(!warning);
        let native_y = -frame.y - frame.height;
        assert_eq!(frame.width, 408.0);
        assert_eq!(frame.height, 744.0);
        assert!(native_y >= 90.0);
        assert!(native_y + frame.height <= 1084.0);
        // Old transport used a CG-global height to flip the final logical y.
        // A differing primary/topology reference displaces an otherwise-clamped frame.
        let cg_top_y = 1117.0 - (native_y + frame.height);
        let old_native_y = 1440.0 - cg_top_y - frame.height;
        assert!(old_native_y + frame.height > 1084.0);
    }

    #[test]
    fn native_negative_origins_and_mixed_backing_scales_share_one_point_space() {
        let left = NSRect::new(NSPoint::new(-1920.0, -200.0), NSSize::new(1920.0, 1080.0));
        let work = NSRect::new(NSPoint::new(-1920.0, -160.0), NSSize::new(1920.0, 1010.0));
        let tray = NSRect::new(NSPoint::new(-100.0, 856.0), NSSize::new(24.0, 24.0));
        let (frame, _) = resolve_placement(
            &[Screen {
                bounds: top_down(left),
                work: top_down(work),
                scale: 1.0,
            }],
            Some(top_down(tray)),
            Some(top_down(tray)),
            top_down(tray),
        )
        .unwrap();
        let native_y = -frame.y - frame.height;
        assert!(frame.x >= -1920.0 && frame.x + frame.width <= 0.0);
        assert!(native_y >= -160.0 && native_y + frame.height <= 850.0);
    }

    #[test]
    fn retina_tray_lookup_uses_appkit_points_not_per_display_physical_pixels() {
        let primary = Screen {
            bounds: Rect {
                x: 0.0,
                y: -1080.0,
                width: 1728.0,
                height: 1080.0,
            },
            work: Rect {
                x: 0.0,
                y: -1048.0,
                width: 1728.0,
                height: 1000.0,
            },
            scale: 1.0,
        };
        let retina = Screen {
            bounds: Rect {
                x: 1728.0,
                y: -1080.0,
                width: 1920.0,
                height: 1080.0,
            },
            work: Rect {
                x: 1728.0,
                y: -1048.0,
                width: 1920.0,
                height: 1000.0,
            },
            scale: 1.0,
        };
        let tray = top_down(NSRect::new(
            NSPoint::new(3000.0, 1056.0),
            NSSize::new(24.0, 24.0),
        ));
        let screens = [primary, retina];
        // tray-icon scales the global point origin by the tray screen's scale;
        // Tao's CGDisplayBounds-based monitor_from_point expects unscaled points.
        let old_lookup_x = (3000.0 + 12.0) * 2.0;
        assert!(!screens
            .iter()
            .any(|s| s.bounds.contains(old_lookup_x, tray.y + 12.0)));
        let (frame, warning) = resolve_placement(
            &screens,
            Some(tray),
            Some(tray),
            Rect {
                x: 100.0,
                y: -100.0,
                width: 1.0,
                height: 1.0,
            },
        )
        .unwrap();
        assert!(!warning);
        assert!(frame.x >= retina.work.x);
        assert!(frame.x + frame.width <= retina.work.x + retina.work.width);
        assert_eq!(frame.width, 408.0);
    }
}
