use super::{connected_placement, placement, Display, Rect};

// All geometry here is AppKit global screen points, flipped once against the
// primary screen. No Tao per-monitor pixels or retained-window scale enters it.
fn top_left(rect: Rect, primary_top: f64) -> Rect {
    Rect {
        y: primary_top - rect.y - rect.height,
        ..rect
    }
}

fn frame(
    displays: &[Display],
    anchor: Option<Rect>,
    anchor_screen: Option<Rect>,
    current: Option<Rect>,
    primary: Option<Rect>,
) -> Result<(Rect, bool), String> {
    if let (Some(anchor), Some(screen)) = (anchor, anchor_screen) {
        if let Some(display) = displays.iter().find(|display| display.bounds == screen) {
            return placement(display.work, anchor, 1.0).map(|frame| (frame, false));
        }
    }
    connected_placement(displays, anchor, current, primary)
}

fn apply_frame(
    frame: Rect,
    primary_top: f64,
    apply: impl FnOnce(Rect) -> Result<Rect, String>,
) -> Result<(), String> {
    let native = top_left(frame, primary_top);
    let actual = apply(native)?;
    if [
        actual.x - native.x,
        actual.y - native.y,
        actual.width - native.width,
        actual.height - native.height,
    ]
    .iter()
    .any(|difference| !difference.is_finite() || difference.abs() > 1.0)
    {
        return Err(
            "The native panel did not retain its connected-display frame. Try opening it again."
                .into(),
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn native_tray(app: &tauri::AppHandle) -> Option<(Rect, Rect)> {
    use objc2::MainThreadMarker;
    app.tray_by_id("pr-sniper").and_then(|tray| {
        tray.with_inner_tray_icon(move |tray| {
            let mtm = MainThreadMarker::new()?;
            let item = tray.ns_status_item()?;
            let button = item.button(mtm)?;
            let window = button.window()?;
            let anchor =
                window.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
            let screen = window.screen()?;
            Some((native_rect(anchor), native_rect(screen.frame())))
        })
        .ok()
        .flatten()
    })
}

#[cfg(target_os = "macos")]
fn native_rect(value: objc2_foundation::NSRect) -> Rect {
    Rect {
        x: value.origin.x,
        y: value.origin.y,
        width: value.size.width,
        height: value.size.height,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn pointer_over_tray(app: &tauri::AppHandle) -> bool {
    let pointer = objc2_app_kit::NSEvent::mouseLocation();
    native_tray(app).is_some_and(|(tray, _)| tray.contains(pointer.x, pointer.y))
}

#[cfg(target_os = "macos")]
pub(super) fn position(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let mtm = MainThreadMarker::new().ok_or("Panel placement requires the native UI thread.")?;
    let screens = NSScreen::screens(mtm);
    let primary = screens
        .firstObject()
        .ok_or("No connected display is available for the panel.")?;
    let primary_top = primary.frame().origin.y + primary.frame().size.height;
    let primary = top_left(native_rect(primary.frame()), primary_top);
    let displays: Vec<_> = screens
        .iter()
        .map(|screen| Display {
            bounds: top_left(native_rect(screen.frame()), primary_top),
            work: top_left(native_rect(screen.visibleFrame()), primary_top),
            scale: 1.0,
        })
        .collect();
    let handle = window
        .ns_window()
        .map_err(|_| "Cannot access the native panel for placement.")?;
    // Tauri owns this NSWindow throughout the UI-thread callback.
    let native = unsafe { handle.cast::<NSWindow>().as_ref() }
        .ok_or("The native panel is unavailable for placement.")?;
    let tray = native_tray(app);
    let pointer = NSEvent::mouseLocation();
    let anchor = tray
        .map(|(anchor, _)| top_left(anchor, primary_top))
        .or(Some(Rect {
            x: pointer.x,
            y: primary_top - pointer.y,
            width: 1.0,
            height: 1.0,
        }));
    let anchor_screen = tray.map(|(_, screen)| top_left(screen, primary_top));
    let current = native
        .screen()
        .map(|screen| top_left(native_rect(screen.frame()), primary_top));
    let (frame, recovered) = frame(&displays, anchor, anchor_screen, current, Some(primary))?;
    apply_frame(frame, primary_top, |frame| {
        native.setFrame_display(
            NSRect::new(
                NSPoint::new(frame.x, frame.y),
                NSSize::new(frame.width, frame.height),
            ),
            false,
        );
        Ok(native_rect(native.frame()))
    })?;
    Ok((tray.is_none() || recovered).then(|| {
        "Tray or display geometry changed or is unavailable; the panel was clamped to a connected display.".into()
    }))
}

#[cfg(test)]
mod tests;
