pub mod discovery;
pub mod github;
pub mod monitoring;
pub mod policy;
pub mod startup;
pub mod storage;

use github::{metadata::PullRequest, provider::Connection, ConnectionError};
use serde::Serialize;
use startup::{LoginRegistration, RegistrationStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use storage::{Diagnostic, DiagnosticEvent, SavedSettings, Settings, Store};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, State, WebviewUrl, WebviewWindowBuilder,
};

struct Host {
    store: Mutex<Store>,
    monitor: Mutex<monitoring::Monitor>,
    error: Mutex<Option<String>>,
    isolated: bool,
    quitting: AtomicBool,
    registration: LoginRegistration,
}

#[derive(Serialize)]
struct MonitoringSnapshot {
    health: Vec<monitoring::ScheduleHealth>,
    jobs: Vec<monitoring::QueueJob>,
}

#[tauri::command]
fn monitoring_snapshot(host: State<'_, Host>) -> Result<MonitoringSnapshot, String> {
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let monitor = host
        .monitor
        .lock()
        .map_err(|_| "Monitoring is unavailable.")?;
    Ok(MonitoringSnapshot {
        health: monitor.snapshot(),
        jobs: store.load_queue()?,
    })
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs() as i64)
        .unwrap_or(0)
}

fn start_checks(app: &tauri::AppHandle, immediate: bool) -> Result<(), String> {
    let host = app.state::<Host>();
    if host.quitting.load(Ordering::SeqCst) {
        return Err("PR Sniper is quitting.".into());
    }
    let tickets = {
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        let mut monitor = host
            .monitor
            .lock()
            .map_err(|_| "Monitoring is unavailable.")?;
        monitor.prepare_checks(&store, now_seconds(), immediate)?
    };
    for ticket in tickets {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = monitoring::poll(&ticket, github::client);
            let host = app.state::<Host>();
            if host.quitting.load(Ordering::SeqCst) {
                return;
            }
            let saved = (|| {
                let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
                let mut monitor = host
                    .monitor
                    .lock()
                    .map_err(|_| "Monitoring is unavailable.")?;
                monitor.finish(&store, ticket, result, now_seconds())
            })();
            if let Err(error) = saved {
                report(&app, error);
            }
        });
    }
    Ok(())
}

#[tauri::command]
fn check_now(app: tauri::AppHandle) -> Result<(), String> {
    start_checks(&app, true)
}

#[derive(Serialize)]
struct Snapshot {
    settings: Option<Settings>,
    login_registration: Option<RegistrationStatus>,
    isolated: bool,
    error: Option<String>,
    version: &'static str,
    settings_persisted: bool,
}

#[derive(Serialize)]
struct GithubMetadata {
    connection: Connection,
    pull_requests: Vec<PullRequest>,
}

fn record(app: &tauri::AppHandle, event: DiagnosticEvent) {
    let host = app.state::<Host>();
    let result = host
        .store
        .lock()
        .map_err(|_| "Storage is unavailable.".to_string())
        .and_then(|store| store.record(event));
    if let Err(error) = result {
        report(app, error);
    }
}

fn report(app: &tauri::AppHandle, error: String) {
    // Callers supply only fixed, safe messages; never forward platform errors.
    eprintln!("PR Sniper: {error}");
    if let Ok(mut current) = app.state::<Host>().error.lock() {
        *current = Some(error);
    }
}

#[tauri::command]
fn snapshot(host: State<'_, Host>) -> Result<Snapshot, String> {
    let mut error = host
        .error
        .lock()
        .map_err(|_| "Host status is unavailable.")?
        .clone();
    let settings = match host
        .store
        .lock()
        .map_err(|_| "Storage is unavailable.")?
        .load_settings()
    {
        Ok(settings) => Some(settings),
        Err(message) => {
            error = Some(message);
            None
        }
    };
    let login_registration = match host.registration.status() {
        Ok(status) => Some(status),
        Err(message) => {
            error = Some(message);
            None
        }
    };
    Ok(Snapshot {
        settings,
        login_registration,
        isolated: host.isolated,
        error,
        version: env!("CARGO_PKG_VERSION"),
        settings_persisted: host
            .store
            .lock()
            .map_err(|_| "Storage is unavailable.")?
            .has_saved_settings(),
    })
}

#[tauri::command]
fn canonical_repository_name(repository: String) -> Result<String, String> {
    storage::canonical_repository(&repository)
}

#[tauri::command]
fn save_preferences(
    host: State<'_, Host>,
    settings: serde_json::Value,
    expected: serde_json::Value,
) -> Result<SavedSettings, String> {
    let settings =
        serde_json::from_value(settings).map_err(|_| "Unsupported settings configuration.")?;
    let expected =
        serde_json::from_value(expected).map_err(|_| "Unsupported settings snapshot.")?;
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let saved = store.save_preferences(settings, &expected)?;
    Ok(store.finish_settings_save(saved))
}

#[tauri::command]
async fn choose_repository_folder(
    app: tauri::AppHandle,
) -> Result<Option<discovery::Discovery>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(folder) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = folder.into_path().map_err(|_| "Choose a local folder.")?;
        discovery::discover(&path).map(Some)
    })
    .await
    .map_err(|_| "Folder selection failed. Try again.".to_string())?
}

#[tauri::command]
async fn discover_repositories(root: String) -> Result<discovery::Discovery, String> {
    tauri::async_runtime::spawn_blocking(move || discovery::discover(std::path::Path::new(&root)))
        .await
        .map_err(|_| "Folder discovery failed. Choose the folder again.".to_string())?
}

#[tauri::command]
async fn resolve_github_person(login: String) -> Result<github::Identity, ConnectionError> {
    tauri::async_runtime::spawn_blocking(move || github::client()?.resolve_person(&login))
        .await
        .map_err(|_| ConnectionError::ProviderFailure)?
}

#[tauri::command]
fn save_login(host: State<'_, Host>, enabled: bool) -> Result<(), String> {
    if host.isolated {
        return Err("Launch at login cannot be changed in an isolated development run.".into());
    }
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    host.registration.set_enabled(&store, enabled)?;
    store.record(DiagnosticEvent::SettingsSaved)?;
    Ok(())
}

#[tauri::command]
fn save_repository(host: State<'_, Host>, repository: String) -> Result<SavedSettings, String> {
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let settings = store.add_repository(&repository)?;
    Ok(store.finish_settings_save(settings))
}

#[tauri::command]
fn update_repository(
    host: State<'_, Host>,
    id: String,
    repository: String,
    enabled: bool,
) -> Result<SavedSettings, String> {
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let settings = store.update_repository(&id, &repository, enabled)?;
    Ok(store.finish_settings_save(settings))
}

#[tauri::command]
fn remove_repository(host: State<'_, Host>, id: String) -> Result<SavedSettings, String> {
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let settings = store.remove_repository(&id)?;
    Ok(store.finish_settings_save(settings))
}

#[tauri::command]
fn save_defaults(
    host: State<'_, Host>,
    policy: serde_json::Value,
) -> Result<SavedSettings, String> {
    let policy = serde_json::from_value(policy).map_err(|_| "Unsupported policy configuration.")?;
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let settings = store.save_defaults(policy)?;
    Ok(store.finish_settings_save(settings))
}

#[tauri::command]
fn save_repository_policy(
    host: State<'_, Host>,
    id: String,
    overrides: serde_json::Value,
) -> Result<SavedSettings, String> {
    let overrides = serde_json::from_value(overrides)
        .map_err(|_| "Unsupported policy override configuration.")?;
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let settings = store.save_repository_policy(&id, overrides)?;
    Ok(store.finish_settings_save(settings))
}

#[tauri::command]
fn diagnostics(host: State<'_, Host>) -> Result<Vec<Diagnostic>, String> {
    host.store
        .lock()
        .map_err(|_| "Storage is unavailable.")?
        .diagnostics()
}

fn configured_repository(host: &Host, id: &str) -> Result<String, ConnectionError> {
    let settings = host
        .store
        .lock()
        .map_err(|_| ConnectionError::Configuration)?
        .load_settings()
        .map_err(|_| ConnectionError::Configuration)?;
    settings
        .repositories
        .into_iter()
        .find(|repository| repository.id == id)
        .map(|repository| repository.name)
        .ok_or(ConnectionError::Configuration)
}

#[tauri::command]
async fn verify_github_connection(
    app: tauri::AppHandle,
    host: State<'_, Host>,
    id: String,
    expected_account_id: Option<String>,
) -> Result<Connection, ConnectionError> {
    let repository = configured_repository(&host, &id)?;
    let name = repository.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        github::client()?.connect(&name, expected_account_id.as_deref())
    })
    .await
    .map_err(|_| ConnectionError::ProviderFailure)?;
    if configured_repository(&host, &id)? != repository {
        return Err(ConnectionError::RepositoryChanged);
    }
    record(
        &app,
        if result.is_ok() {
            DiagnosticEvent::GithubConnectionChecked
        } else {
            DiagnosticEvent::GithubConnectionFailed
        },
    );
    result
}

#[tauri::command]
async fn read_github_metadata(
    app: tauri::AppHandle,
    host: State<'_, Host>,
    id: String,
    expected_account_id: String,
    expected_repository_id: String,
) -> Result<GithubMetadata, ConnectionError> {
    let repository = configured_repository(&host, &id)?;
    let name = repository.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let client = github::client()?;
        let connection = client.connect(&name, Some(&expected_account_id))?;
        if connection.repository.id != expected_repository_id {
            return Err(ConnectionError::RepositoryChanged);
        }
        let pull_requests = client.pull_requests(&connection.repository)?;
        Ok(GithubMetadata {
            connection,
            pull_requests,
        })
    })
    .await
    .map_err(|_| ConnectionError::ProviderFailure)?;
    if configured_repository(&host, &id)? != repository {
        return Err(ConnectionError::RepositoryChanged);
    }
    record(
        &app,
        if result.is_ok() {
            DiagnosticEvent::GithubMetadataRead
        } else {
            DiagnosticEvent::GithubReadFailed
        },
    );
    result
}

#[tauri::command]
fn open_diagnostics(app: tauri::AppHandle) -> Result<(), String> {
    open_window(&app, "diagnostics", "Diagnostics")
}

fn open_window(app: &tauri::AppHandle, label: &str, title: &str) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window(label) {
        window
    } else {
        WebviewWindowBuilder::new(
            app,
            label,
            WebviewUrl::App(format!("index.html?view={label}").into()),
        )
        .title(format!("PR Sniper - {title}"))
        .inner_size(
            if label == "settings" { 1120.0 } else { 640.0 },
            if label == "settings" { 760.0 } else { 520.0 },
        )
        .min_inner_size(390.0, 360.0)
        .visible(false)
        .build()
        .map_err(|_| "Cannot create application window.")?
    };
    window
        .show()
        .map_err(|_| "Cannot show application window.")?;
    window
        .unminimize()
        .map_err(|_| "Cannot restore application window.")?;
    window
        .set_focus()
        .map_err(|_| "Cannot focus application window.")?;
    record(app, DiagnosticEvent::WindowOpened);
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            save_preferences,
            canonical_repository_name,
            choose_repository_folder,
            discover_repositories,
            resolve_github_person,
            monitoring_snapshot,
            check_now,
            save_login,
            save_repository,
            update_repository,
            remove_repository,
            save_defaults,
            save_repository_policy,
            verify_github_connection,
            read_github_metadata,
            diagnostics,
            open_diagnostics
        ])
        .setup(|app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let override_root =
                std::env::var_os("PR_SNIPER_DATA_DIR").map(std::path::PathBuf::from);
            let isolated = override_root.is_some();
            let root = match override_root {
                Some(root) if root.is_absolute() => root,
                Some(_) => return Err("PR_SNIPER_DATA_DIR must be an absolute path.".into()),
                None => app.path().app_data_dir()?,
            };
            let store = Store::new(root);
            let monitor = monitoring::Monitor::restore(&store)?;
            app.manage(Host {
                store: Mutex::new(store),
                monitor: Mutex::new(monitor),
                error: Mutex::new(None),
                isolated,
                quitting: AtomicBool::new(false),
                registration: LoginRegistration::new(
                    app.path()
                        .home_dir()?
                        .join("Library/LaunchAgents/PR Sniper.plist"),
                    std::env::current_exe()?.canonicalize()?,
                ),
            });
            record(app.handle(), DiagnosticEvent::SessionStarted);
            let status = MenuItem::with_id(app, "status", "Status", true, None::<&str>)?;
            let queue = MenuItem::with_id(app, "queue", "Review Queue", true, None::<&str>)?;
            let check = MenuItem::with_id(app, "check", "Check Now", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let doctor = MenuItem::with_id(app, "doctor", "Setup Doctor", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit PR Sniper", true, Some("CmdOrCtrl+Q"))?;
            let menu = Menu::with_items(
                app,
                &[
                    &status, &queue, &check, &settings, &doctor, &separator, &quit,
                ],
            )?;
            TrayIconBuilder::with_id("pr-sniper")
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/tray.png"
                ))?)
                .icon_as_template(true)
                .tooltip("PR Sniper")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "check" {
                        if let Err(error) = start_checks(app, true) {
                            report(app, error);
                        }
                        return;
                    }
                    if event.id.as_ref() == "quit" {
                        record(app, DiagnosticEvent::QuitRequested);
                        app.state::<Host>().quitting.store(true, Ordering::SeqCst);
                        app.exit(0);
                        return;
                    }
                    let target = match event.id.as_ref() {
                        "status" => ("status", "Status"),
                        "queue" => ("queue", "Review Queue"),
                        "settings" => ("settings", "Settings"),
                        "doctor" => ("doctor", "Setup Doctor"),
                        _ => return,
                    };
                    if let Err(error) = open_window(app, target.0, target.1) {
                        report(app, error);
                    }
                })
                .build(app)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    if handle.state::<Host>().quitting.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Err(error) = start_checks(&handle, false) {
                        report(&handle, error);
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                match window.hide() {
                    Ok(()) => record(window.app_handle(), DiagnosticEvent::WindowHidden),
                    Err(_) => report(
                        window.app_handle(),
                        "Cannot hide application window.".into(),
                    ),
                }
            }
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|_| {
            eprintln!("PR Sniper could not start its native host.");
            std::process::exit(1);
        });
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !app.state::<Host>().quitting.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
    });
}
