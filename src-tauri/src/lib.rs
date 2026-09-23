pub mod discovery;
pub mod github;
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
use std::time::{Instant, SystemTime};
use storage::{Diagnostic, DiagnosticEvent, SavedSettings, Settings, Store};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, State, WebviewUrl, WebviewWindowBuilder,
};

struct Host {
    store: Mutex<Store>,
    error: Mutex<Option<String>>,
    isolated: bool,
    quitting: AtomicBool,
    registration: LoginRegistration,
    github_auth: Mutex<GithubAuth>,
    github_credentials:
        github::token_store::RotationSafeStore<github::macos_keychain::MacKeychainStore>,
}

struct GithubAuth {
    started: Instant,
    flow: Option<github::device_flow::DeviceFlow<github::device_http::GithubDeviceHttp>>,
    state: GithubAuthState,
}

enum GithubAuthState {
    Disconnected,
    Connecting(github::device_flow::DevicePrompt),
    Connected(github::Identity),
    ReconnectRequired(GithubAuthFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum GithubAuthFailure {
    Denied,
    Expired,
    Network,
    Provider,
    InvalidResponse,
    CredentialsUnavailable,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum GithubAuthView {
    Disconnected,
    Connecting {
        user_code: String,
        verification_uri: String,
        expires_in_seconds: u64,
        interval_seconds: u64,
    },
    Connected {
        account_id: String,
        login: String,
    },
    ReconnectRequired {
        reason: GithubAuthFailure,
    },
}

impl GithubAuth {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            flow: None,
            state: GithubAuthState::Disconnected,
        }
    }

    fn view(&self) -> GithubAuthView {
        match &self.state {
            GithubAuthState::Disconnected => GithubAuthView::Disconnected,
            GithubAuthState::Connecting(prompt) => GithubAuthView::Connecting {
                user_code: prompt.user_code.clone(),
                verification_uri: prompt.verification_uri.clone(),
                expires_in_seconds: prompt.expires_in.as_secs(),
                interval_seconds: prompt.interval.as_secs(),
            },
            GithubAuthState::Connected(identity) => GithubAuthView::Connected {
                account_id: identity.id.clone(),
                login: identity.login.clone(),
            },
            GithubAuthState::ReconnectRequired(reason) => {
                GithubAuthView::ReconnectRequired { reason: *reason }
            }
        }
    }

    fn restore(
        store: &github::token_store::RotationSafeStore<github::macos_keychain::MacKeychainStore>,
    ) -> Self {
        use github::token_store::RotationError;
        let mut auth = Self::new();
        let transport = match github::device_http::GithubDeviceHttp::new() {
            Ok(transport) => transport,
            Err(error) => {
                auth.state = GithubAuthState::ReconnectRequired(failure_from_device_error(error));
                return auth;
            }
        };
        let restored = match store.refresh_active_if_needed(SystemTime::now(), |current| {
            transport
                .refresh(current.refresh_token())
                .map_err(|error| match error {
                    github::device_flow::DeviceFlowError::Network => RotationError::Network,
                    _ => RotationError::Provider,
                })
        }) {
            Ok(Some(restored)) => restored,
            Ok(None) => return auth,
            Err(error) => {
                auth.state = GithubAuthState::ReconnectRequired(match error {
                    RotationError::Network => GithubAuthFailure::Network,
                    RotationError::Provider => GithubAuthFailure::Provider,
                    RotationError::ReconnectRequired => GithubAuthFailure::Expired,
                    RotationError::Store(_) => GithubAuthFailure::CredentialsUnavailable,
                });
                return auth;
            }
        };
        let identity = github::http::HttpTransport::from_token_pair(&restored.pair)
            .map(github::provider::GithubClient::new)
            .and_then(|client| client.current_identity());
        match identity {
            Ok(identity) if identity.id == restored.account.account_id => {
                auth.state = GithubAuthState::Connected(identity);
            }
            Ok(_) => {
                auth.state = GithubAuthState::ReconnectRequired(GithubAuthFailure::Provider);
            }
            Err(error) => {
                auth.state =
                    GithubAuthState::ReconnectRequired(failure_from_connection_error(error));
            }
        }
        auth
    }
}

fn failure_from_device_error(error: github::device_flow::DeviceFlowError) -> GithubAuthFailure {
    match error {
        github::device_flow::DeviceFlowError::InvalidResponse => GithubAuthFailure::InvalidResponse,
        github::device_flow::DeviceFlowError::Network => GithubAuthFailure::Network,
        github::device_flow::DeviceFlowError::Provider => GithubAuthFailure::Provider,
    }
}

fn failure_from_connection_error(error: ConnectionError) -> GithubAuthFailure {
    match error {
        ConnectionError::Network | ConnectionError::Timeout => GithubAuthFailure::Network,
        ConnectionError::InvalidResponse => GithubAuthFailure::InvalidResponse,
        ConnectionError::SignedOut => GithubAuthFailure::Expired,
        _ => GithubAuthFailure::Provider,
    }
}

fn rotation_connection_error(error: github::token_store::RotationError) -> ConnectionError {
    match error {
        github::token_store::RotationError::Network => ConnectionError::Network,
        github::token_store::RotationError::ReconnectRequired => ConnectionError::SignedOut,
        github::token_store::RotationError::Provider => ConnectionError::ProviderFailure,
        github::token_store::RotationError::Store(_) => ConnectionError::Configuration,
    }
}

fn github_session(
    host: &Host,
) -> Result<
    (
        github::Identity,
        github::provider::GithubClient<github::http::HttpTransport>,
    ),
    ConnectionError,
> {
    let transport = github::device_http::GithubDeviceHttp::new().map_err(|error| match error {
        github::device_flow::DeviceFlowError::Network => ConnectionError::Network,
        github::device_flow::DeviceFlowError::InvalidResponse => ConnectionError::InvalidResponse,
        github::device_flow::DeviceFlowError::Provider => ConnectionError::ProviderFailure,
    })?;
    let active = host
        .github_credentials
        .refresh_active_if_needed(SystemTime::now(), |current| {
            transport
                .refresh(current.refresh_token())
                .map_err(|error| match error {
                    github::device_flow::DeviceFlowError::Network => {
                        github::token_store::RotationError::Network
                    }
                    _ => github::token_store::RotationError::Provider,
                })
        })
        .map_err(rotation_connection_error)?
        .ok_or(ConnectionError::SignedOut)?;
    let client = github::provider::GithubClient::new(github::http::HttpTransport::from_token_pair(
        &active.pair,
    )?);
    let identity = client.current_identity()?;
    if identity.id != active.account.account_id {
        return Err(ConnectionError::WrongIdentity);
    }
    Ok((identity, client))
}

fn failure_from_terminal_poll(
    result: &github::device_flow::DeviceFlowPoll,
) -> Option<GithubAuthFailure> {
    match result {
        github::device_flow::DeviceFlowPoll::Denied => Some(GithubAuthFailure::Denied),
        github::device_flow::DeviceFlowPoll::Expired => Some(GithubAuthFailure::Expired),
        github::device_flow::DeviceFlowPoll::Failed(error) => {
            Some(failure_from_device_error(*error))
        }
        _ => None,
    }
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

#[derive(Serialize)]
struct GithubRepositories {
    identity: github::Identity,
    repositories: Vec<github::provider::InstalledRepository>,
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
async fn resolve_github_person(
    app: tauri::AppHandle,
    login: String,
) -> Result<github::Identity, ConnectionError> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        github_session(&host)?.1.resolve_person(&login)
    })
    .await
    .map_err(|_| ConnectionError::ProviderFailure)?
}

#[tauri::command]
async fn list_github_repositories(
    app: tauri::AppHandle,
) -> Result<GithubRepositories, ConnectionError> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let (identity, client) = github_session(&host)?;
        Ok(GithubRepositories {
            identity,
            repositories: client.installed_repositories()?,
        })
    })
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

#[tauri::command]
fn github_auth_state(host: State<'_, Host>) -> Result<GithubAuthView, String> {
    Ok(host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?
        .view())
}

#[tauri::command]
async fn begin_github_auth(app: tauri::AppHandle) -> Result<GithubAuthView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let mut auth = host
            .github_auth
            .lock()
            .map_err(|_| "GitHub connection state is unavailable.")?;
        let transport = match github::device_http::GithubDeviceHttp::new() {
            Ok(transport) => transport,
            Err(error) => {
                auth.state = GithubAuthState::ReconnectRequired(failure_from_device_error(error));
                return Ok(auth.view());
            }
        };
        let flow = match github::device_flow::DeviceFlow::begin(transport, auth.started.elapsed()) {
            Ok(flow) => flow,
            Err(error) => {
                auth.state = GithubAuthState::ReconnectRequired(failure_from_device_error(error));
                return Ok(auth.view());
            }
        };
        auth.state = GithubAuthState::Connecting(flow.prompt().clone());
        auth.flow = Some(flow);
        Ok(auth.view())
    })
    .await
    .map_err(|_| "Could not start GitHub sign-in.".to_string())?
}

#[tauri::command]
async fn poll_github_auth(app: tauri::AppHandle) -> Result<GithubAuthView, String> {
    use github::{device_flow::DeviceFlowPoll, token_store::ActiveAccount};
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let mut auth = host
            .github_auth
            .lock()
            .map_err(|_| "GitHub connection state is unavailable.")?;
        let now = auth.started.elapsed();
        let result = auth
            .flow
            .as_mut()
            .ok_or("Start GitHub sign-in before checking authorization.")?
            .poll(now);
        match result {
            DeviceFlowPoll::WaitUntil(_) | DeviceFlowPoll::Pending { .. } => {}
            DeviceFlowPoll::Authorized(pair) => {
                let client = github::provider::GithubClient::new(
                    github::http::HttpTransport::from_token_pair(&pair)
                        .map_err(|_| "GitHub credentials could not be verified.")?,
                );
                let identity = match client.current_identity() {
                    Ok(identity) => identity,
                    Err(error) => {
                        auth.state = GithubAuthState::ReconnectRequired(
                            failure_from_connection_error(error),
                        );
                        auth.flow = None;
                        return Ok(auth.view());
                    }
                };
                let account = ActiveAccount::new(&identity.id, &identity.login).map_err(|_| {
                    auth.state =
                        GithubAuthState::ReconnectRequired(GithubAuthFailure::InvalidResponse);
                    auth.flow = None;
                    "GitHub identity could not be stored safely."
                })?;
                host.github_credentials
                    .replace_active_account(&account, &pair)
                    .map_err(|_| {
                        auth.state = GithubAuthState::ReconnectRequired(
                            GithubAuthFailure::CredentialsUnavailable,
                        );
                        auth.flow = None;
                        "GitHub credentials could not be saved securely."
                    })?;
                auth.state = GithubAuthState::Connected(identity);
                auth.flow = None;
            }
            DeviceFlowPoll::Cancelled => {
                auth.state = GithubAuthState::Disconnected;
                auth.flow = None;
            }
            terminal @ (DeviceFlowPoll::Denied
            | DeviceFlowPoll::Expired
            | DeviceFlowPoll::Failed(_)) => {
                auth.state = GithubAuthState::ReconnectRequired(
                    failure_from_terminal_poll(&terminal)
                        .expect("terminal device result must preserve a failure reason"),
                );
                auth.flow = None;
            }
        }
        Ok(auth.view())
    })
    .await
    .map_err(|_| "Could not check GitHub authorization.".to_string())?
}

#[tauri::command]
fn cancel_github_auth(host: State<'_, Host>) -> Result<GithubAuthView, String> {
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    if let Some(flow) = &mut auth.flow {
        flow.cancel();
    }
    auth.flow = None;
    auth.state = GithubAuthState::Disconnected;
    Ok(auth.view())
}

#[tauri::command]
fn disconnect_github_auth(host: State<'_, Host>) -> Result<GithubAuthView, String> {
    use github::token_store::ActiveAccount;
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    let GithubAuthState::Connected(identity) = &auth.state else {
        return Err("No connected GitHub account is available to disconnect.".into());
    };
    let account = ActiveAccount::new(&identity.id, &identity.login)
        .map_err(|_| "GitHub identity is invalid.")?;
    host.github_credentials
        .disconnect(&account)
        .map_err(|_| "GitHub credentials could not be deleted securely.")?;
    auth.state = GithubAuthState::Disconnected;
    auth.flow = None;
    Ok(auth.view())
}

fn configured_repository(host: &Host, id: &str) -> Result<storage::Repository, ConnectionError> {
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
        .ok_or(ConnectionError::Configuration)
}

fn verify_installed_repository(
    client: &github::provider::GithubClient<github::http::HttpTransport>,
    repository: &storage::Repository,
) -> Result<(), ConnectionError> {
    let installation_id = repository
        .installation_id
        .as_deref()
        .ok_or(ConnectionError::Configuration)?;
    let repository_id = repository
        .provider_repository_id
        .as_deref()
        .ok_or(ConnectionError::Configuration)?;
    if client.installed_repositories()?.iter().any(|installed| {
        installed.installation_id == installation_id
            && installed.repository.id == repository_id
            && installed.repository.name == repository.name
    }) {
        Ok(())
    } else {
        Err(ConnectionError::MissingReadPermission)
    }
}

#[tauri::command]
async fn verify_github_connection(
    app: tauri::AppHandle,
    host: State<'_, Host>,
    id: String,
) -> Result<Connection, ConnectionError> {
    let repository = configured_repository(&host, &id)?;
    let expected = repository.clone();
    let app_for_read = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_read.state::<Host>();
        let (identity, client) = github_session(&host)?;
        verify_installed_repository(&client, &expected)?;
        let connection = client.connect(&expected.name, Some(&identity.id))?;
        if connection.repository.id
            != expected
                .provider_repository_id
                .as_deref()
                .ok_or(ConnectionError::Configuration)?
        {
            return Err(ConnectionError::RepositoryChanged);
        }
        Ok(connection)
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
    let expected = repository.clone();
    let app_for_read = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_read.state::<Host>();
        let (identity, client) = github_session(&host)?;
        if identity.id != expected_account_id {
            return Err(ConnectionError::WrongIdentity);
        }
        verify_installed_repository(&client, &expected)?;
        let connection = client.connect(&expected.name, Some(&identity.id))?;
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
            list_github_repositories,
            save_login,
            save_repository,
            update_repository,
            remove_repository,
            save_defaults,
            save_repository_policy,
            verify_github_connection,
            read_github_metadata,
            github_auth_state,
            begin_github_auth,
            poll_github_auth,
            cancel_github_auth,
            disconnect_github_auth,
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
            let github_credentials = github::token_store::RotationSafeStore::new(
                github::macos_keychain::MacKeychainStore::production(),
            );
            let github_auth = GithubAuth::restore(&github_credentials);
            app.manage(Host {
                store: Mutex::new(Store::new(root)),
                error: Mutex::new(None),
                isolated,
                quitting: AtomicBool::new(false),
                registration: LoginRegistration::new(
                    app.path()
                        .home_dir()?
                        .join("Library/LaunchAgents/PR Sniper.plist"),
                    std::env::current_exe()?.canonicalize()?,
                ),
                github_auth: Mutex::new(github_auth),
                github_credentials,
            });
            record(app.handle(), DiagnosticEvent::SessionStarted);
            let status = MenuItem::with_id(app, "status", "Status", true, None::<&str>)?;
            let queue = MenuItem::with_id(app, "queue", "Review Queue", true, None::<&str>)?;
            let check = MenuItem::with_id(
                app,
                "check",
                "Check Now (not implemented)",
                false,
                None::<&str>,
            )?;
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

#[cfg(test)]
mod github_auth_tests {
    use super::{
        failure_from_connection_error, failure_from_device_error, failure_from_terminal_poll,
        rotation_connection_error, GithubAuthFailure,
    };
    use crate::github::device_flow::{DeviceFlowError, DeviceFlowPoll};
    use crate::github::token_store::RotationError;
    use crate::github::ConnectionError;

    #[test]
    fn terminal_device_failures_preserve_their_host_reason() {
        for (poll, expected) in [
            (DeviceFlowPoll::Denied, GithubAuthFailure::Denied),
            (DeviceFlowPoll::Expired, GithubAuthFailure::Expired),
            (
                DeviceFlowPoll::Failed(DeviceFlowError::Network),
                GithubAuthFailure::Network,
            ),
            (
                DeviceFlowPoll::Failed(DeviceFlowError::Provider),
                GithubAuthFailure::Provider,
            ),
            (
                DeviceFlowPoll::Failed(DeviceFlowError::InvalidResponse),
                GithubAuthFailure::InvalidResponse,
            ),
        ] {
            assert_eq!(failure_from_terminal_poll(&poll), Some(expected));
        }
    }

    #[test]
    fn begin_identity_refresh_and_restore_failures_keep_network_distinct() {
        assert_eq!(
            failure_from_device_error(DeviceFlowError::Network),
            GithubAuthFailure::Network
        );
        assert_eq!(
            failure_from_connection_error(ConnectionError::Network),
            GithubAuthFailure::Network
        );
        assert_eq!(
            failure_from_connection_error(ConnectionError::Timeout),
            GithubAuthFailure::Network
        );
        assert_eq!(
            rotation_connection_error(RotationError::Network),
            ConnectionError::Network
        );
        assert_eq!(
            failure_from_connection_error(rotation_connection_error(RotationError::Provider)),
            GithubAuthFailure::Provider
        );
    }
}
