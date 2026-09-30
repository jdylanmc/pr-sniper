use super::*;
use crate::{now_seconds, storage::DiagnosticEvent, Host};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use tauri::Manager;

#[cfg(target_os = "macos")]
use super::macos::Native;
#[cfg(windows)]
use super::windows::Native;

#[cfg(target_os = "macos")]
const PERMISSION_GUIDANCE: &str = "Check System Settings > Notifications > PR Sniper.";
#[cfg(windows)]
const PERMISSION_GUIDANCE: &str = "Check Windows Settings > System > Notifications > PR Sniper. Banner and notification center switches are not exposed separately by the Windows API.";

pub(crate) struct Coordinator {
    native: Result<Native, String>,
    active: AtomicBool,
    configuration: AtomicU64,
    configuration_gate: Mutex<()>,
}

impl Coordinator {
    pub(crate) fn new(app: &tauri::AppHandle, _store: &Store, _root: &std::path::Path) -> Self {
        Self {
            #[cfg(target_os = "macos")]
            native: Native::new(app),
            #[cfg(windows)]
            native: Native::new(app, _store, _root),
            active: AtomicBool::new(false),
            configuration: AtomicU64::new(0),
            configuration_gate: Mutex::new(()),
        }
    }

    #[cfg(windows)]
    pub(crate) fn ready(&self) -> Result<(), String> {
        self.native.as_ref().map_err(Clone::clone)?.ready()
    }

    #[cfg(windows)]
    pub(crate) fn shutdown(&self) {
        if let Ok(native) = &self.native {
            native.shutdown();
        }
    }

    #[cfg(windows)]
    pub(crate) fn matches_registration(
        &self,
        registration: &super::windows_native::Registration,
    ) -> bool {
        self.native
            .as_ref()
            .is_ok_and(|native| &native.registration == registration)
    }

    pub(crate) fn pump(app: &tauri::AppHandle) {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst)
            || host
                .notifications
                .active
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) = run(&app) {
                crate::report(&app, error);
            }
            app.state::<Host>()
                .notifications
                .active
                .store(false, Ordering::SeqCst);
        });
    }
}

fn run(app: &tauri::AppHandle) -> Result<(), String> {
    let host = app.state::<Host>();
    for _ in 0..4 {
        if host.quitting.load(Ordering::SeqCst) {
            break;
        }
        if !host
            .store
            .lock()
            .map_err(|_| "Notification storage unavailable.")?
            .load_notifications()?
            .enabled
        {
            break;
        }
        let snapshot = crate::queue_snapshot(&host)?;
        let next = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Notification storage unavailable.")?;
            let mut ledger = store.load_notifications()?;
            if ledger.observe(&frames(&snapshot), now_seconds()?)? {
                store.save_notifications(&ledger)?;
            }
            ledger.next().cloned()
        };
        let Some(next) = next else {
            break;
        };
        let permission = host
            .notifications
            .native
            .as_ref()
            .map_err(Clone::clone)
            .and_then(Adapter::permission);
        let fresh = crate::queue_snapshot(&host)?;
        let notice = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Notification storage unavailable.")?;
            prepare(&store, &frames(&fresh), &next.id, now_seconds()?)?
        };
        if notice.phase != Phase::Submitting {
            continue;
        }
        let (phase, error) = match permission {
            Err(error) => (Phase::Failed, Some(error)),
            Ok(permission) if !permission.allowed() => (Phase::PermissionDenied,
                Some(format!("The operating system has not authorized notifications. {PERMISSION_GUIDANCE} This event remains in the queue and history."))),
            Ok(_) => {
                let enabled = host.store.lock().map_err(|_| "Notification storage unavailable.")?.load_notifications()?.enabled;
                if !enabled || host.quitting.load(Ordering::SeqCst) {
                    (Phase::NotSent, Some("Notifications were disabled or the application is quitting.".into()))
                } else {
                    match host.notifications.native.as_ref() {
                        Ok(native) => submit(native, &notice),
                        Err(error) => (Phase::Failed, Some(error.clone())),
                    }
                }
            }
        };
        {
            let store = host
                .store
                .lock()
                .map_err(|_| "Notification storage unavailable.")?;
            complete(&store, &notice.id, phase, error)?;
        }
        crate::record(
            app,
            if phase == Phase::AcceptedUnconfirmed {
                DiagnosticEvent::NotificationAccepted
            } else {
                DiagnosticEvent::NotificationFailed
            },
        );
    }
    Ok(())
}

#[derive(Serialize)]
pub struct NoticeView {
    #[serde(flatten)]
    notice: Notice,
    title: &'static str,
    body: &'static str,
}

#[derive(Serialize)]
pub struct Target {
    pub id: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub platform: &'static str,
    pub enabled: bool,
    pub permission: Option<Permission>,
    pub error: Option<String>,
    pub notices: Vec<NoticeView>,
    pub targets: Vec<Target>,
}

pub fn view(store: &Store, permission: Result<Permission, String>) -> Result<Snapshot, String> {
    let ledger = store.load_notifications()?;
    let queue = queue::snapshot(store, vec![]);
    let errors: Vec<_> = [permission.as_ref().err(), queue.as_ref().err()]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    Ok(Snapshot {
        platform: std::env::consts::OS,
        enabled: ledger.enabled,
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("\n"))
        },
        permission: permission.ok(),
        notices: ledger
            .notices
            .into_iter()
            .rev()
            .map(|notice| NoticeView {
                title: notice.event.category.title(),
                body: notice.event.category.body(),
                notice,
            })
            .collect(),
        targets: queue
            .map(|snapshot| snapshot.items)
            .unwrap_or_default()
            .into_iter()
            .map(|item| Target {
                id: item.id,
                label: format!(
                    "{} #{} / {} ({}) / {}",
                    item.job.repository_name,
                    item.job.number,
                    item.job.account_login,
                    item.job.account_id,
                    item.job.head_sha
                ),
            })
            .collect(),
    })
}

#[tauri::command]
pub(crate) async fn notification_snapshot(app: tauri::AppHandle) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let permission = host
            .notifications
            .native
            .as_ref()
            .map_err(Clone::clone)
            .and_then(Adapter::permission);
        let store = host
            .store
            .lock()
            .map_err(|_| "Notification storage unavailable.")?;
        view(&store, permission)
    })
    .await
    .map_err(|_| "Notification status could not be read.".to_string())?
}

#[tauri::command]
pub(crate) async fn set_notifications_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let generation = app
        .state::<Host>()
        .notifications
        .configuration
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        if enabled {
            let permission = host.notifications.native.as_ref().map_err(Clone::clone)?.request_permission()?;
            if !permission.allowed() {
                return Err(format!("Notifications are denied or unavailable. Notification opt-in was not enabled. {PERMISSION_GUIDANCE}"));
            }
        }
        let _gate = host.notifications.configuration_gate.lock().map_err(|_| "Notification settings unavailable.")?;
        if host.notifications.configuration.load(Ordering::SeqCst) != generation {
            return Err("A newer notification preference replaced this request.".into());
        }
        let store = host.store.lock().map_err(|_| "Notification storage unavailable.")?;
        let mut ledger = store.load_notifications()?;
        ledger.enabled = enabled;
        store.save_notifications(&ledger)
    }).await.map_err(|_| "Notification preference change failed.".to_string())?
}

#[tauri::command]
pub(crate) fn test_notification(
    app: tauri::AppHandle,
    item_id: Option<String>,
) -> Result<String, String> {
    let host = app.state::<Host>();
    let id = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Notification storage unavailable.")?;
        let destination = if let Some(item_id) = item_id {
            queue::destination(&store, &item_id, None)?;
            Destination::QueueItem { item_id }
        } else {
            Destination::Settings
        };
        let mut ledger = store.load_notifications()?;
        let id = ledger.enqueue_test(destination, now_seconds()?)?;
        store.save_notifications(&ledger)?;
        id
    };
    Coordinator::pump(&app);
    Ok(id)
}

pub(crate) fn open(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let host = app
        .try_state::<Host>()
        .ok_or("Notification host is not ready; no alternative destination was opened.")?;
    let destination = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Notification storage unavailable.")?;
        destination(&store, id)?
    };
    let now = now_seconds()?;
    let outcome = match destination {
        Destination::QueueItem { item_id } => crate::open_queue_item(app.clone(), item_id),
        Destination::Settings => crate::open_settings(app.clone()),
    };
    {
        let store = host
            .store
            .lock()
            .map_err(|_| "Notification storage unavailable.")?;
        let mut ledger = store.load_notifications()?;
        let notice = ledger
            .notices
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Notification disappeared during navigation.")?;
        match &outcome {
            Ok(()) => {
                notice.opened_at = Some(now);
                notice.navigation_error = None;
            }
            Err(error) => notice.navigation_error = Some(error.clone()),
        }
        store.save_notifications(&ledger)?;
    }
    crate::record(
        app,
        if outcome.is_ok() {
            DiagnosticEvent::NotificationOpened
        } else {
            DiagnosticEvent::NotificationFailed
        },
    );
    outcome
}

#[tauri::command]
pub(crate) fn open_notification(app: tauri::AppHandle, id: String) -> Result<(), String> {
    open(&app, &id)
}
