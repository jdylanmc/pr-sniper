#[cfg(any(target_os = "macos", test))]
const CORNER_RADIUS: f64 = 19.0;

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(crate) enum SurfaceError {
    UiThread,
    Handle(tauri::Error),
    MissingHandle,
    MissingLayer,
    InvalidScale,
    RejectedClip,
}

#[cfg(target_os = "macos")]
impl std::fmt::Display for SurfaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UiThread => formatter.write_str("Panel clipping requires the native UI thread."),
            Self::Handle(_) => formatter.write_str("Cannot access the native panel surface."),
            Self::MissingHandle => formatter.write_str("The native panel surface is unavailable."),
            Self::MissingLayer => {
                formatter.write_str("The native panel clipping layer is unavailable.")
            }
            Self::InvalidScale => formatter.write_str("The native panel backing scale is invalid."),
            Self::RejectedClip => formatter
                .write_str("The native panel did not retain its transparent rounded surface."),
        }
    }
}

#[cfg(target_os = "macos")]
impl std::error::Error for SurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Handle(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn refresh(window: &tauri::Window) -> Result<(), SurfaceError> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSView, NSWindow};

    let _main_thread = MainThreadMarker::new().ok_or(SurfaceError::UiThread)?;
    let native = window.ns_window().map_err(SurfaceError::Handle)?;
    let view = window.ns_view().map_err(SurfaceError::Handle)?;
    // Tauri owns both objects for this callback; no pointer survives the call.
    // ns_view is the content-view ancestor of the WKWebView, not a DOM layer.
    let native =
        unsafe { native.cast::<NSWindow>().as_ref() }.ok_or(SurfaceError::MissingHandle)?;
    let view = unsafe { view.cast::<NSView>().as_ref() }.ok_or(SurfaceError::MissingHandle)?;
    let scale = native.backingScaleFactor();
    if !scale.is_finite() || scale <= 0.0 {
        return Err(SurfaceError::InvalidScale);
    }
    view.setWantsLayer(true);
    let layer = view.layer().ok_or(SurfaceError::MissingLayer)?;
    layer.setContentsScale(scale);
    // Core Animation uses points; multiplying the radius by backing scale
    // would make Retina corners twice as large as the approved CSS silhouette.
    layer.setCornerRadius(CORNER_RADIUS);
    layer.setMasksToBounds(true);
    native.invalidateShadow();
    if native.isOpaque()
        || !native.hasShadow()
        || !layer.masksToBounds()
        || layer.cornerRadius() != CORNER_RADIUS
        || layer.contentsScale() != scale
    {
        return Err(SurfaceError::RejectedClip);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn refresh(_window: &tauri::Window) -> Result<(), std::convert::Infallible> {
    // Windows keeps Tauri/Tao's undecorated-shadow/DWM frame mechanism.
    // WebView2 transparency and the clipped document supply the inner contour.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_surface_config_and_document_contract() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(config["app"]["macOSPrivateApi"], true);
        let css = include_str!("../../../src/panel.css");
        assert_eq!(CORNER_RADIUS, 19.0);
        assert!(css.contains("--panel-radius: 19px;"));
        assert!(css.contains("clip-path: inset(0 round var(--panel-radius));"));
        assert!(css.contains("border-radius: var(--panel-radius);"));
        assert!(css.contains(":root[data-surface=\"panel\"]"));
        assert!(css.contains("background: transparent;"));
        let host = include_str!("../panel.rs");
        assert!(host.contains(".transparent(true)"));
        assert!(host.contains(".background_color(tauri::utils::config::Color(0, 0, 0, 0))"));
        assert!(host.contains(".shadow(true)"));
        assert!(
            host.find("refresh_surface(&window.as_ref().window())")
                .unwrap()
                < host.find(".show()").unwrap()
        );
    }
}
