use super::{windows_native as native, Adapter, Notice, Permission, SendError};
use crate::{storage::Store, Host};
use native::{ActivationServer, Apartment, Registration};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::Manager;
use windows::{
    core::{w, HSTRING},
    Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
    UI::Notifications::NotificationSetting,
};

pub(super) struct Native {
    app: tauri::AppHandle,
    pub(super) registration: Registration,
    server: Mutex<Option<ActivationServer>>,
    stopped: AtomicBool,
}

impl Native {
    pub(super) fn new(app: &tauri::AppHandle, store: &Store, root: &Path) -> Result<Self, String> {
        let registration = Registration {
            version: 1,
            profile: store.load_notifications()?.profile_id,
            root: root
                .canonicalize()
                .map_err(|_| "Notification data root is unavailable.")?,
            executable: std::env::current_exe()
                .and_then(|p| p.canonicalize())
                .map_err(|_| "Notification executable is unavailable.")?,
            credential_service: std::env::var("PR_SNIPER_KEYCHAIN_SERVICE").ok(),
        };
        registration.validate()?;
        Ok(Self {
            app: app.clone(),
            registration,
            server: Mutex::new(None),
            stopped: AtomicBool::new(false),
        })
    }

    fn start(&self) -> Result<(), String> {
        let mut server = self
            .server
            .lock()
            .map_err(|_| "Notification activation unavailable.")?;
        if self.stopped.load(Ordering::SeqCst)
            || self.app.state::<Host>().quitting.load(Ordering::SeqCst)
        {
            return Err("PR Sniper is quitting; notification activation was not started.".into());
        }
        if server.is_none() {
            let app = self.app.clone();
            let registration = self.registration.clone();
            *server = Some(ActivationServer::start(
                registration.clone(),
                Arc::new(move |id| dispatch(&app, registration.clone(), id)),
            )?);
        }
        Ok(())
    }

    pub(super) fn ready(&self) -> Result<(), String> {
        let _apartment = Apartment::new()?;
        if self.registration.registered()? {
            self.start()?;
        }
        Ok(())
    }

    pub(super) fn shutdown(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Ok(mut server) = self.server.lock() {
            if let Some(server) = server.take() {
                if let Err(error) = server.stop() {
                    crate::report(&self.app, error);
                }
            }
        }
    }
}

fn permission(setting: NotificationSetting) -> Permission {
    Permission {
        authorization: native::authorization(setting).into(),
        alerts_enabled: None,
        center_enabled: None,
    }
}

impl Adapter for Native {
    fn permission(&self) -> Result<Permission, String> {
        let _apartment = Apartment::new()?;
        if !self.registration.registered()? {
            return Ok(Permission {
                authorization: "not_registered".into(),
                alerts_enabled: None,
                center_enabled: None,
            });
        }
        native::permission(&self.registration).map(permission)
    }

    fn request_permission(&self) -> Result<Permission, String> {
        let _apartment = Apartment::new()?;
        // Windows has no desktop authorization prompt. Opt-in installs only our
        // owned identity, then reads the actual OS block; it changes no OS setting.
        self.registration.install()?;
        self.start()?;
        native::permission(&self.registration).map(permission)
    }

    fn send(&self, notice: &Notice) -> Result<(), SendError> {
        let _apartment = Apartment::new().map_err(|message| SendError {
            message,
            uncertain: false,
        })?;
        if !self
            .registration
            .registered()
            .map_err(|message| SendError {
                message,
                uncertain: false,
            })?
        {
            return Err(SendError {
                message: "Notification registration is missing; opt in explicitly to set it up."
                    .into(),
                uncertain: false,
            });
        }
        native::send(
            &self.registration,
            &notice.id,
            notice.event.category.title(),
            notice.event.category.body(),
        )
        .map_err(|(message, uncertain)| SendError { message, uncertain })
    }
}

fn validate_saved(registration: &Registration, id: Option<&str>) -> Result<(), String> {
    registration.validate()?;
    let executable = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|_| "Notification executable is unavailable.")?;
    if executable != registration.executable
        || registration.root.canonicalize().ok().as_ref() != Some(&registration.root)
        || (registration.credential_service.is_none()
            && native::production_root()?.canonicalize().ok().as_ref() != Some(&registration.root))
    {
        return Err("Notification executable/profile moved; no alternate application or profile was opened.".into());
    }
    let store = Store::new(registration.root.clone());
    let ledger = store.load_notifications()?;
    if ledger.profile_id != registration.profile {
        return Err(
            "Notification profile is missing or was replaced; no alternate profile was opened."
                .into(),
        );
    }
    if let Some(id) = id {
        registration.notice_id(id)?;
        super::destination(&store, id)?;
    }
    Ok(())
}

fn dispatch(app: &tauri::AppHandle, registration: Registration, id: String) -> Result<(), String> {
    registration.notice_id(&id)?;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let host = handle.try_state::<Host>().ok_or("Notification host is not ready; nothing was opened.")?;
            if host.quitting.load(Ordering::SeqCst) {
                return Err("PR Sniper is quitting; try opening the notification again.".into());
            }
            if !host.notifications.matches_registration(&registration) {
                return Err("Another PR Sniper profile is running. Quit it and retry this notification; no substitute destination was opened.".into());
            }
            validate_saved(&registration, Some(&id))?;
            crate::record(&handle, crate::storage::DiagnosticEvent::NotificationActivated);
            super::host::open(&handle, &id)
        })();
        if let Err(error) = result {
            crate::report(&handle, error.clone());
            show_error(&error);
        }
    }).map_err(|_| "Notification navigation could not reach the application window.".into())
}

pub(crate) fn show_error(error: &str) {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(error),
            w!("PR Sniper notification"),
            MB_OK | MB_ICONERROR,
        );
    }
}

// The single-instance plugin runs before Host setup. Store forwarded activation
// without touching Host, then drain on the UI thread after manage.
static PENDING: Mutex<Vec<(Registration, String)>> = Mutex::new(Vec::new());

pub(crate) fn forward(app: &tauri::AppHandle, args: &[String]) {
    match parse_open(args) {
        Ok(Some((registration, Some(id)))) => {
            if app.try_state::<Host>().is_some() {
                if let Err(error) = dispatch(app, registration, id) {
                    show_error(&error);
                }
            } else if let Ok(mut pending) = PENDING.lock() {
                if pending.len() < 16 {
                    pending.push((registration, id));
                } else {
                    show_error("Notification activation queue is full; retry after PR Sniper finishes starting.");
                }
            }
        }
        Ok(_) => {}
        Err(error) => show_error(&error),
    }
}

pub(crate) fn ready(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = handle.state::<Host>().notifications.ready() {
            crate::report(&handle, error);
        }
    });
    forward(app, &std::env::args().collect::<Vec<_>>());
    if let Ok(mut pending) = PENDING.lock() {
        for (registration, id) in pending.drain(..) {
            if let Err(error) = dispatch(app, registration, id) {
                show_error(&error);
            }
        }
    }
}

fn parse_open(args: &[String]) -> Result<Option<(Registration, Option<String>)>, String> {
    if args.get(1).map(String::as_str) != Some(native::OPEN_ARG) {
        return Ok(None);
    }
    if !(3..=4).contains(&args.len()) {
        return Err("Invalid notification launch arguments.".into());
    }
    let registration = Registration::decode(&args[2])?;
    let id = args.get(3).cloned();
    validate_saved(&registration, id.as_deref())?;
    Ok(Some((registration, id)))
}

/// Runs before plugins, credentials, or any default-profile reads.
pub(crate) fn preflight() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().collect();
    let action = args.get(1).map(String::as_str).unwrap_or("");
    if action == native::SERVER_ARG || action == native::CLEANUP_ARG {
        if args.len() != 3
            && !(action == native::SERVER_ARG && args.len() == 4 && is_embedding(&args[3]))
        {
            return Err("Invalid Windows notification server arguments.".into());
        }
        let registration = Registration::decode(&args[2])?;
        let _apartment = Apartment::new()?;
        if !registration.registered()? {
            return Err("Owned notification registration is missing.".into());
        }
        if action == native::CLEANUP_ARG {
            registration.uninstall()?;
            return Ok(false);
        }
        validate_saved(&registration, None)?;
        // COM may start us while the GUI owner is still initializing. Receive
        // the callback before invoking the plugin that exits duplicate hosts.
        let (tx, rx) = mpsc::sync_channel(1);
        let callback_registration = registration.clone();
        let server = ActivationServer::start(
            registration.clone(),
            Arc::new(move |id| {
                validate_saved(&callback_registration, Some(&id))?;
                tx.try_send(id)
                    .map_err(|_| "Notification activation is already being handled.".into())
            }),
        )?;
        let received = rx.recv_timeout(Duration::from_secs(60));
        match received {
            Ok(id) => {
                std::process::Command::new(&registration.executable)
                    .args([native::OPEN_ARG, &registration.encode()?, &id])
                    .env_remove("PR_SNIPER_DATA_DIR")
                    .env_remove("PR_SNIPER_KEYCHAIN_SERVICE")
                    .spawn()
                    .map_err(|_| "The exact notification application could not start.")?;
            }
            Err(_) => {
                let released = server.wait_released();
                server.stop()?;
                released?;
                return Err(
                    "Windows did not supply a notification activation before timeout.".into(),
                );
            }
        }
        server.stop()?;
        return Ok(false);
    }
    if let Some((registration, _)) = parse_open(&args)? {
        let _apartment = Apartment::new()?;
        if !registration.registered()? {
            return Err("Owned notification registration is missing.".into());
        }
        match &registration.credential_service {
            Some(service) => {
                std::env::set_var("PR_SNIPER_DATA_DIR", &registration.root);
                std::env::set_var("PR_SNIPER_KEYCHAIN_SERVICE", service);
            }
            None => {
                std::env::remove_var("PR_SNIPER_DATA_DIR");
                std::env::remove_var("PR_SNIPER_KEYCHAIN_SERVICE");
            }
        }
    } else if args
        .iter()
        .skip(1)
        .any(|a| a.starts_with("--pr-sniper-notification-") || is_embedding(a))
    {
        return Err("Unrecognized notification activation; no profile was opened.".into());
    }
    Ok(true)
}

fn is_embedding(value: &str) -> bool {
    value.eq_ignore_ascii_case("-embedding") || value.eq_ignore_ascii_case("/embedding")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> (tempfile::TempDir, Registration, Store) {
        let root = tempfile::Builder::new()
            .prefix("notification-profile-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
            .unwrap();
        let store = Store::new(root.path().into());
        super::super::restore(&store).unwrap();
        let registration = Registration {
            version: 1,
            profile: store.load_notifications().unwrap().profile_id,
            root: root.path().canonicalize().unwrap(),
            executable: std::env::current_exe().unwrap().canonicalize().unwrap(),
            credential_service: Some("com.jdylanmc.pr-sniper.tests.cold-profile".into()),
        };
        (root, registration, store)
    }

    #[test]
    fn cold_start_rejects_replaced_or_missing_profile_without_recreating_it() {
        let (root, registration, store) = profile();
        validate_saved(&registration, None).unwrap();
        store
            .save_notifications(&super::super::Ledger::default())
            .unwrap();
        assert!(validate_saved(&registration, None).is_err());
        let path = root.path().to_owned();
        root.close().unwrap();
        assert!(validate_saved(&registration, None).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn cold_start_requires_exact_saved_notice_and_isolated_credential_identity() {
        let (_root, registration, store) = profile();
        let mut ledger = store.load_notifications().unwrap();
        ledger.enabled = true;
        let id = ledger
            .enqueue_test(super::super::Destination::Settings, 100)
            .unwrap();
        store.save_notifications(&ledger).unwrap();
        validate_saved(&registration, Some(&id)).unwrap();
        let missing = format!(
            "pr-sniper:{}:{}",
            registration.profile,
            uuid::Uuid::new_v4()
        );
        assert!(validate_saved(&registration, Some(&missing)).is_err());
        let mut wrong_executable = registration.clone();
        wrong_executable.executable.set_file_name("foreign.exe");
        assert!(validate_saved(&wrong_executable, Some(&id)).is_err());
        let mut production = registration.clone();
        production.credential_service = None;
        assert!(validate_saved(&production, Some(&id)).is_err());
        let args = vec![
            "app.exe".into(),
            native::OPEN_ARG.into(),
            registration.encode().unwrap(),
            id,
        ];
        assert!(parse_open(&args).unwrap().is_some());
        assert!(parse_open(&args[..2]).is_err());
        assert!(parse_open(&[args.clone(), vec!["merge".into()]].concat()).is_err());
    }
}
