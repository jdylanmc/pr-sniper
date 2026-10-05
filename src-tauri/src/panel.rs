use crate::{capacity::Kind, queue, storage::Store, Host};
use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "panel";
pub const EVENT: &str = "pr-sniper:panel";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    Queue,
    Running,
    Reviewed,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Detail {
    Item { item_id: String },
    Job { kind: Kind, id: String },
    Status,
    Diagnostics,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub tab: Tab,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
}
impl Route {
    pub fn tab(tab: Tab) -> Self {
        Self { tab, detail: None }
    }
    pub fn item(id: String) -> Self {
        Self {
            tab: Tab::Queue,
            detail: Some(Detail::Item { item_id: id }),
        }
    }
    pub fn utility(diagnostics: bool) -> Self {
        Self {
            tab: Tab::Settings,
            detail: Some(if diagnostics {
                Detail::Diagnostics
            } else {
                Detail::Status
            }),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let id = match &self.detail {
            Some(Detail::Item { item_id }) => Some(item_id),
            Some(Detail::Job { id, .. }) => Some(id),
            _ => None,
        };
        if id.is_some_and(|id| {
            id.is_empty() || id.len() > 16_384 || id.chars().any(char::is_control)
        }) {
            return Err(
                "Panel destination identity is invalid; no substitute was selected.".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub route: Route,
    pub revision: u64,
    pub visible: bool,
    pub missing: Option<String>,
    pub placement_warning: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    route: Route,
    initialized: bool,
    revision: u64,
    visible: bool,
    #[serde(skip)]
    focus_holds: usize,
    #[serde(skip)]
    blur_from_tray: Option<Instant>,
    #[serde(skip)]
    tray_down_visible: Option<bool>,
    placement_warning: Option<String>,
}
impl Session {
    pub fn navigate(&mut self, store: &Store, route: Option<Route>) -> Result<(), String> {
        let route = if route.is_some() {
            route
        } else if !self.initialized {
            store.load_queue_selection()?.map(Route::item)
        } else {
            None
        };
        if let Some(route) = route {
            save_selection(store, &route)?;
            self.route = route;
        }
        self.initialized = true;
        self.revision += 1;
        self.blur_from_tray = None;
        Ok(())
    }
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.revision += 1;
    }
    fn dismiss(&mut self, from_focus_loss: bool) {
        self.set_visible(false);
        if !from_focus_loss {
            self.blur_from_tray = None;
            self.tray_down_visible = None;
        }
    }
    pub fn snapshot(&self, store: &Store) -> Snapshot {
        let missing = match missing(store, &self.route) {
            Ok(value) => value,
            Err(error) => Some(format!("Saved destination could not be read. {error}")),
        };
        Snapshot {
            route: self.route.clone(),
            revision: self.revision,
            visible: self.visible,
            missing,
            placement_warning: self.placement_warning.clone(),
        }
    }
    fn blur_can_hide(&self, epoch: u64) -> bool {
        self.revision == epoch && self.visible && self.focus_holds == 0
    }
    fn should_toggle_closed(&mut self, now: Instant) -> bool {
        self.tray_down_visible.take().unwrap_or(
            self.visible
                || self.blur_from_tray.is_some_and(|at| {
                    now.saturating_duration_since(at) < Duration::from_millis(500)
                }),
        )
    }
}

#[derive(Default)]
pub(crate) struct Panel {
    session: Mutex<Session>,
    opening: tokio::sync::Mutex<()>,
}

pub fn missing(store: &Store, route: &Route) -> Result<Option<String>, String> {
    route.validate()?;
    let Some(detail) = &route.detail else {
        return Ok(None);
    };
    if matches!(detail, Detail::Status | Detail::Diagnostics) {
        return Ok(None);
    }
    if let Some(message) = crate::retention::cleaned(store, detail)? {
        return Ok(Some(message.into()));
    }
    let snapshot = queue::snapshot(store, vec![])?;
    let available = match detail {
        Detail::Item { item_id } => snapshot
            .items
            .iter()
            .any(|item| item.id == *item_id || item.aliases.contains(item_id)),
        Detail::Job {
            kind: Kind::Normal,
            id,
        } => snapshot.reviews.iter().any(|r| r.key == *id),
        Detail::Job {
            kind: Kind::Reply | Kind::Mention,
            id,
        } => {
            snapshot.follow_ups.iter().any(|f| {
                f.run.id == *id
                    && matches!(detail, Detail::Job { kind, .. } if *kind == f.run.kind())
            }) || matches!(
                detail,
                Detail::Job {
                    kind: Kind::Mention,
                    ..
                }
            ) && snapshot
                .mentions
                .iter()
                .any(|mention| mention.work_id == *id)
        }
        Detail::Job {
            kind: Kind::PrimaryFinal,
            id,
        } => snapshot.items.iter().any(|item| {
            item.action_status
                .as_ref()
                .and_then(|s| s.final_review.as_ref())
                .is_some_and(|f| f.id == *id)
        }),
        _ => true,
    };
    Ok((!available).then(|| "This exact saved destination is not available in the current evidence view. No other PR, iteration or job was selected.".into()))
}

pub fn save_selection(store: &Store, route: &Route) -> Result<(), String> {
    route.validate()?;
    if let Some(Detail::Item { item_id }) = &route.detail {
        // Keep the requested identity even when it is unavailable; never select a substitute.
        store.save_queue_selection(Some(item_id))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
}

/// Inputs and output are physical coordinates. Desired dimensions are logical.
pub fn placement(work: Rect, tray: Rect, scale: f64) -> Result<Rect, String> {
    if !work.valid()
        || work.width < 4.0
        || work.height < 4.0
        || !tray.valid()
        || !scale.is_finite()
        || scale <= 0.0
    {
        return Err("Monitor or tray geometry is invalid.".into());
    }
    let gap = (8.0 * scale).min(work.width / 4.0).min(work.height / 4.0);
    let width = (408.0 * scale).min(work.width - 2.0 * gap).max(1.0);
    let height = (744.0 * scale).min(work.height - 2.0 * gap).max(1.0);
    let center_x = tray.x + tray.width / 2.0;
    let center_y = tray.y + tray.height / 2.0;
    let (x, y) = if tray.x + tray.width <= work.x {
        (work.x + gap, center_y - height / 2.0)
    } else if tray.x >= work.x + work.width {
        (work.x + work.width - width - gap, center_y - height / 2.0)
    } else if center_y >= work.y + work.height / 2.0 {
        (center_x - width / 2.0, tray.y - height - gap)
    } else {
        (center_x - width / 2.0, tray.y + tray.height + gap)
    };
    Ok(Rect {
        x: x.clamp(work.x + gap, work.x + work.width - width - gap),
        y: y.clamp(work.y + gap, work.y + work.height - height - gap),
        width,
        height,
    })
}

fn physical(rect: tauri::Rect) -> Result<Rect, String> {
    match (rect.position, rect.size) {
        (tauri::Position::Physical(position), tauri::Size::Physical(size)) => Ok(Rect {
            x: position.x as f64,
            y: position.y as f64,
            width: size.width as f64,
            height: size.height as f64,
        }),
        _ => Err("Tray geometry is not physical; placement cannot be safely inferred.".into()),
    }
}

fn position(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    let tray = app
        .tray_by_id("pr-sniper")
        .ok_or("Tray is unavailable.")?
        .rect()
        .map_err(|_| "Tray rectangle unavailable.")?;
    let (anchor, warning) = if let Some(rect) = tray {
        (physical(rect)?, None)
    } else {
        let cursor = window
            .cursor_position()
            .map_err(|_| "Pointer monitor unavailable.")?;
        (
            Rect {
                x: cursor.x,
                y: cursor.y,
                width: 1.0,
                height: 1.0,
            },
            Some(
                "Tray rectangle unavailable; the panel is clamped to the pointer's monitor.".into(),
            ),
        )
    };
    let monitor = window
        .monitor_from_point(
            anchor.x + anchor.width / 2.0,
            anchor.y + anchor.height / 2.0,
        )
        .map_err(|_| "Tray monitor unavailable.")?;
    let monitor = match monitor {
        Some(monitor) => Some(monitor),
        None => window
            .current_monitor()
            .map_err(|_| "Current monitor unavailable.")?,
    }
    .ok_or("No monitor is available for the panel.")?;
    let work = monitor.work_area();
    let frame = placement(
        Rect {
            x: work.position.x as f64,
            y: work.position.y as f64,
            width: work.size.width as f64,
            height: work.size.height as f64,
        },
        anchor,
        monitor.scale_factor(),
    )?;
    window
        .set_size(PhysicalSize::new(
            frame.width.round() as u32,
            frame.height.round() as u32,
        ))
        .map_err(|_| "Cannot size the panel.")?;
    window
        .set_position(PhysicalPosition::new(
            frame.x.round() as i32,
            frame.y.round() as i32,
        ))
        .map_err(|_| "Cannot position the panel.")?;
    Ok(warning)
}

pub(crate) fn snapshot(app: &tauri::AppHandle) -> Result<Snapshot, String> {
    let host = app.state::<Host>();
    let store = host
        .store
        .lock()
        .map_err(|_| "Panel storage unavailable.")?;
    let session = host
        .panel
        .session
        .lock()
        .map_err(|_| "Panel navigation unavailable.")?;
    Ok(session.snapshot(&store))
}

fn emit(app: &tauri::AppHandle) -> Result<(), String> {
    app.emit_to(LABEL, EVENT, snapshot(app)?)
        .map_err(|_| "Panel route delivery failed.".into())
}

pub(crate) async fn show(app: &tauri::AppHandle, route: Option<Route>) -> Result<Snapshot, String> {
    let host = app.state::<Host>();
    if host.quitting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("PR Sniper is quitting.".into());
    }
    let epoch = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Panel storage unavailable.")?;
        let mut session = host
            .panel
            .session
            .lock()
            .map_err(|_| "Panel navigation unavailable.")?;
        session.navigate(&store, route)?;
        session.revision
    };
    let _opening = host.panel.opening.lock().await;
    if host
        .panel
        .session
        .lock()
        .map_err(|_| "Panel navigation unavailable.")?
        .revision
        != epoch
    {
        return Err("Panel opening was replaced by a newer navigation or dismissal.".into());
    }
    // WebView2 creation must not run in a synchronous command/event callback.
    let window = if let Some(window) = app.get_webview_window(LABEL) {
        window
    } else {
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
            .title("PR Sniper")
            .inner_size(408.0, 744.0)
            .decorations(false)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .visible(false)
            .build()
            .map_err(|_| "Cannot create the application panel.")?
    };
    let (send, receive) = tokio::sync::oneshot::channel();
    let present_app = app.clone();
    app.run_on_main_thread(move || {
        let outcome = (|| {
            let host = present_app.state::<Host>();
            {
                let session = host
                    .panel
                    .session
                    .lock()
                    .map_err(|_| "Panel navigation unavailable.")?;
                if session.revision != epoch
                    || host.quitting.load(std::sync::atomic::Ordering::SeqCst)
                {
                    return Err("Panel opening was replaced by a newer dismissal or Quit.".into());
                }
            }
            let warning = position(&present_app, &window)?;
            window
                .show()
                .map_err(|_| "Cannot show the application panel.")?;
            window
                .unminimize()
                .map_err(|_| "Cannot restore the application panel.")?;
            window
                .set_focus()
                .map_err(|_| "Cannot focus the application panel.")?;
            {
                let mut session = host
                    .panel
                    .session
                    .lock()
                    .map_err(|_| "Panel navigation unavailable.")?;
                session.set_visible(true);
                session.placement_warning = warning;
            }
            let state = snapshot(&present_app)?;
            present_app
                .emit_to(LABEL, EVENT, &state)
                .map_err(|_| "Panel route delivery failed.")?;
            crate::record(&present_app, crate::storage::DiagnosticEvent::WindowOpened);
            Ok(state)
        })();
        let _ = send.send(outcome);
    })
    .map_err(|_| "Panel opening could not reach the native UI thread.")?;
    receive
        .await
        .map_err(|_| "Panel opening was interrupted.")?
}

pub(crate) fn hide(app: &tauri::AppHandle) -> Result<(), String> {
    dismiss(app, false)
}

fn dismiss(app: &tauri::AppHandle, from_focus_loss: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window
            .hide()
            .map_err(|_| "Cannot hide the application panel.")?;
    }
    {
        let host = app.state::<Host>();
        let mut session = host
            .panel
            .session
            .lock()
            .map_err(|_| "Panel navigation unavailable.")?;
        session.dismiss(from_focus_loss);
    }
    emit(app)?;
    crate::record(app, crate::storage::DiagnosticEvent::WindowHidden);
    Ok(())
}

pub(crate) fn tray(app: &tauri::AppHandle, down: bool) -> Result<(), String> {
    let host = app.state::<Host>();
    let close = {
        let mut session = host
            .panel
            .session
            .lock()
            .map_err(|_| "Panel navigation unavailable.")?;
        if down {
            session.tray_down_visible = Some(
                session.visible
                    || session
                        .blur_from_tray
                        .is_some_and(|at| at.elapsed() < Duration::from_millis(500)),
            );
            return Ok(());
        }
        session.should_toggle_closed(Instant::now())
    };
    if close {
        hide(app)
    } else {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = show(&app, None).await {
                crate::report(&app, error);
            }
        });
        Ok(())
    }
}

pub(crate) fn lost_focus(app: &tauri::AppHandle) {
    let app = app.clone();
    let epoch = (|| -> Result<Option<u64>, String> {
        let host = app.state::<Host>();
        let mut session = host
            .panel
            .session
            .lock()
            .map_err(|_| "Panel navigation unavailable.")?;
        if !session.visible || session.focus_holds > 0 {
            return Ok(None);
        }
        if let Some(window) = app.get_webview_window(LABEL) {
            if let (Ok(cursor), Some(tray)) =
                (window.cursor_position(), app.tray_by_id("pr-sniper"))
            {
                if let Some(rect) = tray.rect().map_err(|_| "Tray rectangle unavailable.")? {
                    if physical(rect)?.contains(cursor.x, cursor.y) {
                        session.blur_from_tray = Some(Instant::now());
                    }
                }
            }
        }
        Ok(Some(session.revision))
    })();
    let epoch = match epoch {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(error) => {
            crate::report(&app, error);
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        let dismiss_app = app.clone();
        if app
            .run_on_main_thread(move || {
                let outcome = (|| -> Result<(), String> {
                    let host = dismiss_app.state::<Host>();
                    {
                        let session = host
                            .panel
                            .session
                            .lock()
                            .map_err(|_| "Panel navigation unavailable.")?;
                        if !session.blur_can_hide(epoch) {
                            return Ok(());
                        }
                    }
                    if let Some(window) = dismiss_app.get_webview_window(LABEL) {
                        if !window
                            .is_focused()
                            .map_err(|_| "Panel focus unavailable.")?
                        {
                            dismiss(&dismiss_app, true)?;
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = outcome {
                    crate::report(&dismiss_app, error);
                }
            })
            .is_err()
        {
            crate::report(
                &app,
                "Panel dismissal could not reach the native UI thread.".into(),
            );
        }
    });
}

#[tauri::command]
pub(crate) fn panel_snapshot(app: tauri::AppHandle) -> Result<Snapshot, String> {
    snapshot(&app)
}
#[tauri::command]
pub(crate) async fn panel_navigate(
    app: tauri::AppHandle,
    route: Route,
) -> Result<Snapshot, String> {
    show(&app, Some(route)).await
}
#[tauri::command]
pub(crate) fn hide_panel(app: tauri::AppHandle) -> Result<(), String> {
    hide(&app)
}

#[cfg(test)]
mod tests;
