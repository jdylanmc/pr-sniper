//! Live window refresh for committed Store changes from any origin: UI
//! commands, the local control plane and background coordinators.
use crate::storage::{Change, Observer};
use serde::Serialize;
use tauri::Emitter;

pub const SETTINGS: &str = "pr-sniper:settings-changed";
pub const STATE: &str = "pr-sniper:state-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Payload {
    /// Write count of the changed file since launch; newer is larger.
    pub revision: u64,
}

pub(crate) fn event(change: Change) -> &'static str {
    match change {
        Change::Settings => SETTINGS,
        Change::State => STATE,
    }
}

/// Broadcasts each change to every window; delivery is best effort.
pub(crate) fn emitter(app: tauri::AppHandle) -> Observer {
    std::sync::Arc::new(move |change, revision| {
        let _ = app.emit(event(change), Payload { revision });
    })
}
