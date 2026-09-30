use super::{windows_native as native, Adapter, Notice, Permission, SendError};
use crate::{storage::Store, Host};
use native::{ActivationServer, Apartment, Registration};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
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
    root: PathBuf,
    identity: Mutex<Result<Registration, String>>,
    server: Mutex<Option<ActivationServer>>,
    stopped: AtomicBool,
}

impl Native {
    pub(super) fn new(app: &tauri::AppHandle, store: &Store, root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|_| "Notification data root is unavailable.")?;
        Ok(Self {
            app: app.clone(),
            identity: Mutex::new(saved_registration(store, &root)),
            root,
            server: Mutex::new(None),
            stopped: AtomicBool::new(false),
        })
    }

    pub(super) fn registration(&self) -> Result<Registration, String> {
        self.identity
            .lock()
            .map_err(|_| "Notification identity unavailable.")?
            .clone()
    }

    fn start(&self, registration: &Registration) -> Result<(), String> {
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
            let registration = registration.clone();
            *server = Some(ActivationServer::start(
                registration.clone(),
                Arc::new(move |id| dispatch(&app, registration.clone(), id)),
            )?);
        }
        Ok(())
    }

    pub(super) fn ready(&self) -> Result<(), String> {
        let _apartment = Apartment::new()?;
        let registration = self.registration()?;
        validate_saved(&registration, None)?;
        if registration.registered()? {
            self.start(&registration)?;
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
        let registration = self.registration()?;
        validate_saved(&registration, None)?;
        if !registration.registered()? {
            return Ok(Permission {
                authorization: "not_registered".into(),
                alerts_enabled: None,
                center_enabled: None,
            });
        }
        native::permission(&registration)
            .map(permission)
            .map_err(|e| e.message)
    }

    fn request_permission(&self) -> Result<Permission, String> {
        let _apartment = Apartment::new()?;
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| "Notification storage unavailable.")?;
        let registration = {
            let mut identity = self
                .identity
                .lock()
                .map_err(|_| "Notification identity unavailable.")?;
            recover_identity(&mut identity, &store, &self.root)?
        };
        validate_saved(&registration, None)?;
        // Windows has no desktop authorization prompt. Opt-in installs only our
        // owned identity, then reads the actual OS block; it changes no OS setting.
        registration.install()?;
        self.start(&registration)?;
        request_saved_permission(
            &store,
            &registration,
            || native::permission(&registration),
            |notice| {
                native::submit_setup(
                    &registration,
                    &notice.id,
                    notice.event.category.title(),
                    notice.event.category.body(),
                )
            },
            |id| native::remove_setup(&registration, id),
        )
        .map(permission)
    }

    fn send(&self, notice: &Notice) -> Result<(), SendError> {
        let _apartment = Apartment::new().map_err(|message| SendError {
            message,
            uncertain: false,
        })?;
        let registration = self
            .registration()
            .and_then(|registration| {
                validate_saved(&registration, Some(&notice.id))?;
                Ok(registration)
            })
            .map_err(|message| SendError {
                message,
                uncertain: false,
            })?;
        if !registration.registered().map_err(|message| SendError {
            message,
            uncertain: false,
        })? {
            return Err(SendError {
                message: "Notification registration is missing; opt in explicitly to set it up."
                    .into(),
                uncertain: false,
            });
        }
        native::send(
            &registration,
            &notice.id,
            notice.event.category.title(),
            notice.event.category.body(),
        )
        .map_err(|(message, uncertain)| SendError { message, uncertain })
    }
}

fn request_saved_permission(
    store: &Store,
    registration: &Registration,
    mut read: impl FnMut() -> Result<NotificationSetting, native::PermissionError>,
    submit: impl FnOnce(&Notice) -> Result<(), (String, bool)>,
    mut cleanup: impl FnMut(&str) -> Result<(), String>,
) -> Result<NotificationSetting, String> {
    let mut ledger = super::persisted_ledger(store)?;
    if ledger.profile_id != registration.profile {
        return Err(
            "Notification profile changed before Windows setup; nothing was submitted.".into(),
        );
    }
    // A crash may have interrupted exact cleanup after Show. Only explicit
    // opt-in reaches this retry, and it never resubmits an uncertain notice.
    let mut cleanup_recorded = false;
    for notice in ledger.notices.iter_mut().filter(|n| {
        n.source == super::WINDOWS_SETUP_SOURCE
            && !matches!(n.phase, super::Phase::Failed | super::Phase::NotSent)
    }) {
        registration.notice_id(&notice.id)?;
        if let Err(error) = cleanup(&notice.id) {
            notice.phase = super::Phase::OutcomeUnknown;
            notice.error = Some(match &notice.error {
                Some(previous) if !previous.contains(&error) => format!("{previous} {error}"),
                Some(previous) => previous.clone(),
                None => error.clone(),
            });
            store.save_notifications(&ledger)?;
            return Err(error);
        }
        if matches!(
            notice.phase,
            super::Phase::Submitting | super::Phase::OutcomeUnknown
        ) {
            notice.phase = super::Phase::OutcomeUnknown;
            let receipt = "Exact setup cleanup succeeded on explicit retry; prior delivery outcome is unchanged.";
            if !notice
                .error
                .as_deref()
                .is_some_and(|error| error.contains(receipt))
            {
                notice.error = Some(match &notice.error {
                    Some(previous) => format!("{previous} {receipt}"),
                    None => receipt.into(),
                });
                cleanup_recorded = true;
            }
        }
    }
    if cleanup_recorded {
        store.save_notifications(&ledger)?;
    }
    match read() {
        Ok(setting) => return Ok(setting),
        Err(error) if !error.needs_initialization => return Err(error.message),
        Err(_) => {}
    }
    let notice = ledger.begin_permission_setup(crate::now_seconds()?)?;
    registration.notice_id(&notice.id)?;
    store.save_notifications(&ledger)?;
    let sent = submit(&notice);
    let removed = if !matches!(sent, Err((_, false))) {
        cleanup(&notice.id)
    } else {
        Ok(())
    };
    let setting = read();
    let phase = if removed.is_err() || matches!(sent, Err((_, true))) {
        super::Phase::OutcomeUnknown
    } else if sent.is_err() {
        super::Phase::Failed
    } else {
        super::Phase::AcceptedUnconfirmed
    };
    let mut errors = Vec::new();
    if let Err((message, _)) = &sent {
        errors.push(message.clone());
    }
    if let Err(message) = &removed {
        errors.push(message.clone());
    }
    if let Err(error) = &setting {
        errors.push(error.message.clone());
    }
    let message = if errors.is_empty() {
        format!("Explicit Windows permission setup submitted with popup suppression and short expiry; its exact notification was removed. OS setting: {}. No visible delivery is claimed.",
                native::authorization(*setting.as_ref().unwrap()))
    } else {
        errors.join(" ")
    };
    let mut saved = super::persisted_ledger(store)?;
    if saved.profile_id != registration.profile {
        return Err("Notification profile changed after Windows setup; send/cleanup outcome could not be saved. No further submission was made.".into());
    }
    saved.finish(&notice.id, phase, Some(message.clone()))?;
    store.save_notifications(&saved)?;
    if sent.is_err() || removed.is_err() {
        return Err(message);
    }
    setting.map_err(|error| error.message)
}

pub(super) fn saved_registration(store: &Store, root: &Path) -> Result<Registration, String> {
    let registration = Registration {
        version: 1,
        profile: super::persisted_ledger(store)?.profile_id,
        root: root.to_owned(),
        executable: std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map_err(|_| "Notification executable is unavailable.")?,
        credential_service: std::env::var("PR_SNIPER_KEYCHAIN_SERVICE").ok(),
    };
    registration.validate()?;
    Ok(registration)
}

pub(super) fn recover_identity(
    identity: &mut Result<Registration, String>,
    store: &Store,
    root: &Path,
) -> Result<Registration, String> {
    if identity.is_err() {
        // Only explicit opt-in retries initialization. Failure leaves the adapter
        // unusable; a new identity is cached only after persistence and readback.
        super::restore(store)?;
        *identity = saved_registration(store, root);
    }
    let registration = identity.as_ref().map_err(Clone::clone)?;
    if super::persisted_ledger(store)?.profile_id != registration.profile {
        return Err("Notification profile changed; restart PR Sniper before opting in. No registration was changed.".into());
    }
    Ok(registration.clone())
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
    let ledger = super::persisted_ledger(&store)?;
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
static PENDING: Mutex<native::StartupQueue> = Mutex::new(native::StartupQueue::new());

pub(crate) fn forward(app: &tauri::AppHandle, args: &[String]) {
    match parse_open(args) {
        Ok(Some((registration, Some(id)))) => {
            let result = PENDING
                .lock()
                .map_err(|_| "Notification startup queue unavailable.".to_string())
                .and_then(|mut pending| pending.forward((registration, id)));
            match result {
                Ok(Some((registration, id))) => {
                    if let Err(error) = dispatch(app, registration, id) {
                        show_error(&error);
                    }
                }
                Ok(None) => {}
                Err(error) => show_error(&error),
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
    let pending = PENDING
        .lock()
        .map_err(|_| "Notification startup queue unavailable.".to_string())
        .and_then(|mut pending| pending.ready());
    match pending {
        Ok(pending) => {
            for (registration, id) in pending {
                if let Err(error) = dispatch(app, registration, id) {
                    show_error(&error);
                }
            }
        }
        Err(error) => show_error(&error),
    }
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    let cancelled = PENDING.lock().map(|mut pending| pending.shutdown());
    match cancelled {
        Ok(pending) if pending.is_empty() => {}
        Ok(_) => crate::report(app, "Notification startup navigation was cancelled during shutdown; retry the notification.".into()),
        Err(_) => crate::report(app, "Notification startup queue could not be closed.".into()),
    }
    app.state::<Host>().notifications.shutdown();
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
        let callback_registration = registration.clone();
        let handoff_registration = registration.clone();
        native::run_relay(
            registration.clone(),
            Arc::new(move |id| validate_saved(&callback_registration, Some(&id))),
            Arc::new(move |id| {
                std::process::Command::new(&handoff_registration.executable)
                    .args([native::OPEN_ARG, &handoff_registration.encode()?, &id])
                    .env_remove("PR_SNIPER_DATA_DIR")
                    .env_remove("PR_SNIPER_KEYCHAIN_SERVICE")
                    .spawn()
                    .map_err(|_| "The exact notification application could not start.")?;
                Ok(())
            }),
            Duration::from_secs(60),
        )?;
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

    fn missing_setting() -> native::PermissionError {
        native::PermissionError {
            message: "Windows notification permission Setting read failed (0x80070490).".into(),
            needs_initialization: true,
        }
    }

    #[test]
    fn explicit_setup_persists_private_intent_and_uses_real_rechecked_status() {
        let (_root, registration, store) = profile();
        let reads = std::cell::Cell::new(0);
        let sent = std::cell::RefCell::new(None);
        let result = request_saved_permission(
            &store,
            &registration,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 {
                    Err(missing_setting())
                } else {
                    Ok(NotificationSetting::DisabledForApplication)
                }
            },
            |notice| {
                let saved = super::super::persisted_ledger(&store).unwrap();
                assert!(!saved.enabled);
                assert_eq!(saved.profile_id, registration.profile);
                assert_eq!(saved.notices[0].id, notice.id);
                assert_eq!(saved.notices[0].phase, super::super::Phase::Submitting);
                assert_eq!(notice.event.category, super::super::Category::Test);
                assert_eq!(
                    notice.event.destination,
                    super::super::Destination::Settings
                );
                registration.notice_id(&notice.id).unwrap();
                *sent.borrow_mut() = Some(notice.id.clone());
                Ok(())
            },
            |id| {
                assert_eq!(sent.borrow().as_deref(), Some(id));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, NotificationSetting::DisabledForApplication);
        assert!(!permission(result).allowed());
        let saved = super::super::persisted_ledger(&store).unwrap();
        assert!(!saved.enabled);
        assert_eq!(
            saved.notices[0].phase,
            super::super::Phase::AcceptedUnconfirmed
        );
        assert!(saved.notices[0]
            .error
            .as_deref()
            .unwrap()
            .contains("denied"));
    }

    #[test]
    fn setup_send_and_cleanup_uncertainty_are_persisted_without_resubmission() {
        let (_root, registration, store) = profile();
        let result = request_saved_permission(
            &store,
            &registration,
            || Err(missing_setting()),
            |_| Err(("Native Show outcome unknown.".into(), true)),
            |_| Err("Exact cleanup outcome unknown.".into()),
        );
        let error = result.unwrap_err();
        assert!(error.contains("Show outcome unknown"));
        assert!(error.contains("cleanup outcome unknown"));
        let saved = super::super::persisted_ledger(&store).unwrap();
        assert!(!saved.enabled);
        assert_eq!(saved.notices[0].phase, super::super::Phase::OutcomeUnknown);
        super::super::restore(&store).unwrap();
        assert!(request_saved_permission(
            &store,
            &registration,
            || Err(missing_setting()),
            |_| panic!("Uncertain setup must not resend"),
            |id| {
                assert_eq!(id, saved.notices[0].id);
                Ok(())
            }
        )
        .is_err());
        assert_eq!(
            super::super::persisted_ledger(&store)
                .unwrap()
                .notices
                .len(),
            1
        );
    }

    #[test]
    fn setup_requires_persisted_intent_and_exact_identity_before_native_effects() {
        let (root, mut registration, store) = profile();
        registration.profile = uuid::Uuid::new_v4().to_string();
        assert!(request_saved_permission(
            &store,
            &registration,
            || panic!("Foreign profile must not read native status"),
            |_| panic!("Foreign profile must not submit"),
            |_| panic!("Foreign profile must not remove")
        )
        .is_err());
        registration.profile = super::super::persisted_ledger(&store).unwrap().profile_id;
        let obstruction = root.path().join("state").join("notifications.json.tmp");
        std::fs::write(&obstruction, b"owned setup obstruction").unwrap();
        assert!(request_saved_permission(
            &store,
            &registration,
            || Err(missing_setting()),
            |_| panic!("Failed intent save must not submit"),
            |_| panic!("Failed intent save must not remove")
        )
        .is_err());
        std::fs::remove_file(obstruction).unwrap();
        assert!(super::super::persisted_ledger(&store)
            .unwrap()
            .notices
            .is_empty());
    }

    #[test]
    fn available_or_failed_notifier_status_never_triggers_setup_submission() {
        let (_root, registration, store) = profile();
        assert_eq!(
            request_saved_permission(
                &store,
                &registration,
                || Ok(NotificationSetting::Enabled),
                |_| panic!("Already available"),
                |_| panic!("No setup notice exists")
            )
            .unwrap(),
            NotificationSetting::Enabled
        );
        assert!(request_saved_permission(
            &store,
            &registration,
            || Err(native::PermissionError {
                message: "notifier creation failed".into(),
                needs_initialization: false
            }),
            |_| panic!("Notifier creation error is not first-use Setting error"),
            |_| panic!("No setup notice exists")
        )
        .is_err());
        assert!(super::super::persisted_ledger(&store)
            .unwrap()
            .notices
            .is_empty());
    }

    #[test]
    fn definite_setup_failure_can_retry_but_interrupted_completion_cannot_resend() {
        let (root, registration, store) = profile();
        assert!(request_saved_permission(
            &store,
            &registration,
            || Err(missing_setting()),
            |_| Err(("Definite request preparation failure.".into(), false)),
            |_| panic!("Nothing reached Show")
        )
        .is_err());
        assert_eq!(
            super::super::persisted_ledger(&store).unwrap().notices[0].phase,
            super::super::Phase::Failed
        );
        let obstruction = root.path().join("state").join("notifications.json.tmp");
        let reads = std::cell::Cell::new(0);
        assert!(request_saved_permission(
            &store,
            &registration,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 {
                    Err(missing_setting())
                } else {
                    Ok(NotificationSetting::Enabled)
                }
            },
            |_| {
                std::fs::write(&obstruction, b"owned completion obstruction").unwrap();
                Ok(())
            },
            |_| Ok(())
        )
        .is_err());
        let saved = super::super::persisted_ledger(&store).unwrap();
        assert!(!saved.enabled);
        assert_eq!(saved.notices.len(), 2);
        assert_eq!(saved.notices[1].phase, super::super::Phase::Submitting);
        std::fs::remove_file(obstruction).unwrap();
        super::super::restore(&store).unwrap();
        assert_eq!(
            request_saved_permission(
                &store,
                &registration,
                || Ok(NotificationSetting::Enabled),
                |_| panic!("Interrupted send must not repeat"),
                |_| Ok(())
            )
            .unwrap(),
            NotificationSetting::Enabled
        );
        let saved = super::super::persisted_ledger(&store).unwrap();
        assert_eq!(saved.notices.len(), 2);
        assert_eq!(saved.notices[1].phase, super::super::Phase::OutcomeUnknown);
        assert!(saved.notices[1]
            .error
            .as_deref()
            .unwrap()
            .contains("cleanup succeeded"));
        assert!(!saved.enabled);
    }

    #[test]
    fn setup_native_properties_suppress_popup_and_bound_expiration() {
        let _apartment = Apartment::new().unwrap();
        let id =
            "pr-sniper:5335c011-9951-48b9-a976-f1f824adbc11:91403d6f-f547-46a9-a744-c217c81ff30a";
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let toast = native::setup_toast(
            id,
            super::super::Category::Test.title(),
            super::super::Category::Test.body(),
        )
        .unwrap();
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(toast.SuppressPopup().unwrap());
        let tag = toast.Tag().unwrap().to_string();
        assert_eq!(tag.len(), 16);
        assert!(tag.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(toast.Group().unwrap(), "permission-setup");
        let expiry = toast
            .ExpirationTime()
            .unwrap()
            .Value()
            .unwrap()
            .UniversalTime as u64;
        assert!(expiry >= (before + 15 + 11_644_473_600) * 10_000_000);
        assert!(expiry <= (after + 15 + 11_644_473_600) * 10_000_000);
        let xml = toast.Content().unwrap().GetXml().unwrap().to_string();
        assert!(xml.contains(id));
        assert!(xml.contains("silent=\"true\""));
    }

    #[test]
    #[ignore = "Explicit owned native probe: installs one unique notification identity, submits one suppressed setup toast, reads real Setting, removes exact notification and registration."]
    fn native_first_use_permission_probe() {
        use windows::UI::Notifications::ToastNotificationManager;
        let _apartment = Apartment::new().unwrap();
        let (root, mut registration, store) = profile();
        registration.credential_service = Some(format!(
            "com.jdylanmc.pr-sniper.tests.setup-{}",
            registration.profile
        ));
        println!(
            "OWNED BEFORE NATIVE WRITES profile={} aumid={} HKCU\\{} shortcut={} root={} exe={}",
            registration.profile,
            registration.aumid(),
            registration.key(),
            registration.shortcut().unwrap().display(),
            registration.root.display(),
            registration.executable.display()
        );
        let mut server = None;
        let result = (|| -> Result<(), String> {
            if registration.registered()? {
                return Err("Probe identity unexpectedly exists.".into());
            }
            registration.install()?;
            server = Some(ActivationServer::start(
                registration.clone(),
                Arc::new(|_| Err("Owned probe never opens UI.".into())),
            )?);
            let before = match native::permission(&registration) {
                Err(error) => error,
                Ok(_) => {
                    return Err(
                        "New probe identity did not reproduce first-use Setting failure.".into(),
                    )
                }
            };
            println!(
                "BEFORE Setting={} initialization_required={}",
                before.message, before.needs_initialization
            );
            if !before.needs_initialization {
                return Err(before.message);
            }
            let setting = request_saved_permission(
                &store,
                &registration,
                || native::permission(&registration),
                |notice| {
                    println!(
                        "BEFORE SHOW id={} tag={} group={} destination=Settings phase=Submitting",
                        notice.id,
                        native::setup_tag(&notice.id),
                        native::SETUP_GROUP
                    );
                    let saved = super::super::persisted_ledger(&store).map_err(|e| (e, false))?;
                    if saved.profile_id != registration.profile
                        || saved.notices[0].phase != super::super::Phase::Submitting
                    {
                        return Err(("Probe intent was not persisted.".into(), false));
                    }
                    native::submit_setup(
                        &registration,
                        &notice.id,
                        notice.event.category.title(),
                        notice.event.category.body(),
                    )
                },
                |id| native::remove_setup(&registration, id),
            )?;
            println!(
                "AFTER Setting={} actual_enum={}",
                native::authorization(setting),
                setting.0
            );
            let saved = super::super::persisted_ledger(&store)?;
            if saved.enabled
                || saved.notices.len() != 1
                || saved.notices[0].phase != super::super::Phase::AcceptedUnconfirmed
            {
                return Err("Probe ledger outcome was not the expected opt-in-off receipt.".into());
            }
            println!("LEDGER {}", serde_json::to_string(&saved).unwrap());
            let history = ToastNotificationManager::History()
                .and_then(|h| h.GetHistoryWithId(&HSTRING::from(registration.aumid())))
                .map_err(|e| native::native_error("probe exact history read", e))?;
            for i in 0..history
                .Size()
                .map_err(|e| native::native_error("probe history count", e))?
            {
                let toast = history
                    .GetAt(i)
                    .map_err(|e| native::native_error("probe history entry", e))?;
                if toast
                    .Tag()
                    .map_err(|e| native::native_error("probe history tag", e))?
                    == native::setup_tag(&saved.notices[0].id)
                    && toast
                        .Group()
                        .map_err(|e| native::native_error("probe history group", e))?
                        == native::SETUP_GROUP
                {
                    return Err("Exact setup notification remains after removal.".into());
                }
            }
            Ok(())
        })();
        let cleanup = (|| -> Result<(), String> {
            for notice in super::super::persisted_ledger(&store)?.notices {
                native::remove_setup(&registration, &notice.id)?;
            }
            registration.uninstall()?;
            if registration.registered()?
                || registration
                    .shortcut()?
                    .try_exists()
                    .map_err(|_| "Probe shortcut absence unreadable.")?
            {
                return Err("Probe registration cleanup did not remove exact owned paths.".into());
            }
            Ok(())
        })();
        let stopped = server.map(ActivationServer::stop).transpose();
        println!("PROBE result={result:?} exact_cleanup={cleanup:?}");
        if cleanup.is_err() {
            println!("PRESERVED root={}", root.keep().display());
        } else {
            root.close().unwrap();
            println!("CLEANED exact class/shortcut/history/profile; native registration absence verified.");
        }
        assert!(cleanup.is_ok(), "{cleanup:?}");
        assert!(stopped.is_ok(), "{stopped:?}");
        assert!(result.is_ok(), "{result:?}");
    }
}
