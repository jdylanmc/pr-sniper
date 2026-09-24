pub mod discovery;
pub mod github;
pub mod policy;
pub mod startup;
pub mod storage;

use github::{metadata::PullRequest, provider::Connection, ConnectionError};
use serde::Serialize;
use startup::{LoginRegistration, RegistrationStatus};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::SystemTime;
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
    github_legacy_credentials:
        github::token_store::RotationSafeStore<github::macos_keychain::MacKeychainStore>,
}

struct GithubAuth {
    next_attempt_id: u64,
    active: Option<ActiveGithubAuth>,
    pending: Option<PendingGithubAccount>,
    pending_failure: Option<GithubAuthFailure>,
    failure: Option<GithubAuthFailure>,
    accounts: BTreeMap<String, GithubAccountState>,
}

struct ActiveGithubAuth {
    id: u64,
    expected_account_id: Option<String>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    user_code: Option<zeroize::Zeroizing<String>>,
    verification_uri: Option<String>,
}

struct PendingGithubAccount {
    identity: github::Identity,
    pair: github::oauth::TokenPair,
}

enum GithubAccountState {
    Connected(github::Identity),
    ReconnectRequired {
        identity: github::Identity,
        reason: GithubAuthFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum GithubAuthFailure {
    Expired,
    Denied,
    DeviceFlowDisabled,
    Network,
    Provider,
    InvalidResponse,
    BrowserOpen,
    Cancelled,
    Timeout,
    WrongIdentity,
    MissingScope,
    AuthenticationChanged,
    CredentialsUnavailable,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum GithubFlowView {
    Idle,
    Connecting {
        #[serde(skip_serializing_if = "Option::is_none")]
        expected_account_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        verification_uri: Option<String>,
    },
    PendingAccountConfirmation {
        account_id: String,
        login: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        confirmation_error: Option<GithubAuthFailure>,
    },
    Failed {
        reason: GithubAuthFailure,
    },
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum GithubAccountView {
    Connected {
        provider: &'static str,
        account_id: String,
        login: String,
    },
    ReconnectRequired {
        provider: &'static str,
        account_id: String,
        login: String,
        reason: GithubAuthFailure,
    },
}

#[derive(Clone, Serialize)]
struct GithubAuthView {
    accounts: Vec<GithubAccountView>,
    flow: GithubFlowView,
}

impl GithubAuth {
    fn new() -> Self {
        Self {
            next_attempt_id: 0,
            active: None,
            pending: None,
            pending_failure: None,
            failure: None,
            accounts: BTreeMap::new(),
        }
    }

    fn view(&self) -> GithubAuthView {
        let accounts = self
            .accounts
            .values()
            .map(|state| match state {
                GithubAccountState::Connected(identity) => GithubAccountView::Connected {
                    provider: "github",
                    account_id: identity.id.clone(),
                    login: identity.login.clone(),
                },
                GithubAccountState::ReconnectRequired { identity, reason } => {
                    GithubAccountView::ReconnectRequired {
                        provider: "github",
                        account_id: identity.id.clone(),
                        login: identity.login.clone(),
                        reason: *reason,
                    }
                }
            })
            .collect();
        let flow = if let Some(pending) = self.pending.as_ref() {
            GithubFlowView::PendingAccountConfirmation {
                account_id: pending.identity.id.clone(),
                login: pending.identity.login.clone(),
                confirmation_error: self.pending_failure,
            }
        } else if let Some(active) = self.active.as_ref() {
            GithubFlowView::Connecting {
                expected_account_id: active.expected_account_id.clone(),
                user_code: active.user_code.as_deref().map(ToString::to_string),
                verification_uri: active.verification_uri.clone(),
            }
        } else if let Some(reason) = self.failure {
            GithubFlowView::Failed { reason }
        } else {
            GithubFlowView::Idle
        };
        GithubAuthView { accounts, flow }
    }

    fn set_failure(&mut self, account_id: &str, reason: GithubAuthFailure) {
        if let Some(state) = self.accounts.get_mut(account_id) {
            let identity = match state {
                GithubAccountState::Connected(identity)
                | GithubAccountState::ReconnectRequired { identity, .. } => identity.clone(),
            };
            *state = GithubAccountState::ReconnectRequired { identity, reason };
        }
    }

    fn apply_connection_result<T>(
        &mut self,
        account_id: &str,
        result: &Result<T, ConnectionError>,
    ) {
        if matches!(result, Err(ConnectionError::MissingScope)) {
            self.set_failure(account_id, GithubAuthFailure::MissingScope);
        }
    }

    fn start_attempt(
        &mut self,
        expected_account_id: Option<String>,
        cancel: tokio::sync::oneshot::Sender<()>,
    ) -> u64 {
        self.cancel_attempt();
        self.pending = None;
        self.pending_failure = None;
        self.failure = None;
        self.next_attempt_id = self.next_attempt_id.wrapping_add(1);
        let id = self.next_attempt_id;
        self.active = Some(ActiveGithubAuth {
            id,
            expected_account_id,
            cancel: Some(cancel),
            user_code: None,
            verification_uri: None,
        });
        id
    }

    fn is_active_attempt(&self, attempt_id: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.id == attempt_id)
    }

    fn set_device_authorization(
        &mut self,
        attempt_id: u64,
        user_code: String,
        verification_uri: String,
    ) -> bool {
        let Some(active) = self
            .active
            .as_mut()
            .filter(|active| active.id == attempt_id)
        else {
            return false;
        };
        active.user_code = Some(zeroize::Zeroizing::new(user_code));
        active.verification_uri = Some(verification_uri);
        true
    }

    fn finish_attempt(
        &mut self,
        attempt_id: u64,
        outcome: Result<(github::Identity, github::oauth::TokenPair), GithubAuthFailure>,
    ) {
        if !self
            .active
            .as_ref()
            .is_some_and(|active| active.id == attempt_id)
        {
            return;
        }
        let expected_account_id = self
            .active
            .take()
            .and_then(|active| active.expected_account_id);
        match outcome {
            Ok((identity, pair)) => {
                let wrong_identity = expected_account_id
                    .as_ref()
                    .is_some_and(|expected| expected != &identity.id);
                let duplicate_account =
                    expected_account_id.is_none() && self.accounts.contains_key(&identity.id);
                if wrong_identity || duplicate_account {
                    if let Some(expected) = expected_account_id {
                        self.set_failure(&expected, GithubAuthFailure::WrongIdentity);
                    }
                    self.failure = Some(GithubAuthFailure::WrongIdentity);
                } else {
                    self.pending = Some(PendingGithubAccount { identity, pair });
                    self.pending_failure = None;
                    self.failure = None;
                }
            }
            Err(reason) => {
                if let Some(expected) = expected_account_id {
                    self.set_failure(&expected, reason);
                }
                if reason != GithubAuthFailure::Cancelled {
                    self.failure = Some(reason);
                }
            }
        }
    }

    fn cancel_attempt(&mut self) -> bool {
        let mut cancelled_active = false;
        if let Some(mut active) = self.active.take() {
            cancelled_active = true;
            if let Some(cancel) = active.cancel.take() {
                let _ = cancel.send(());
            }
        }
        self.pending = None;
        self.pending_failure = None;
        self.failure = None;
        cancelled_active
    }

    fn confirm_with<F, T>(&mut self, persist: F) -> Result<T, String>
    where
        F: FnOnce(&github::Identity, &github::oauth::TokenPair) -> Result<T, ()>,
    {
        let pending = self
            .pending
            .take()
            .ok_or("No GitHub account is awaiting confirmation.")?;
        let persisted = match persist(&pending.identity, &pending.pair) {
            Ok(persisted) => persisted,
            Err(()) => {
                self.pending = Some(pending);
                self.pending_failure = Some(GithubAuthFailure::CredentialsUnavailable);
                return Err("GitHub credentials could not be saved securely.".into());
            }
        };
        self.accounts.insert(
            pending.identity.id.clone(),
            GithubAccountState::Connected(pending.identity),
        );
        self.pending_failure = None;
        self.failure = None;
        Ok(persisted)
    }

    fn restore(
        store: &github::token_store::RotationSafeStore<github::macos_keychain::MacKeychainStore>,
        legacy_store: &github::token_store::RotationSafeStore<
            github::macos_keychain::MacKeychainStore,
        >,
    ) -> Self {
        use github::token_store::{ProviderAccountId, RotationError};
        let mut auth = Self::new();
        let accounts = match store.accounts() {
            Ok(accounts) => accounts,
            Err(_) => return auth,
        };
        for account in accounts {
            if account.provider.as_str() != "github" {
                continue;
            }
            let identity = github::Identity {
                id: account.account_id.clone(),
                login: account.login.clone(),
            };
            let id = ProviderAccountId::github(&account.account_id);
            let transport = match github::oauth::GithubOAuthHttp::new() {
                Ok(transport) => transport,
                Err(error) => {
                    auth.accounts.insert(
                        identity.id.clone(),
                        GithubAccountState::ReconnectRequired {
                            identity,
                            reason: failure_from_oauth_error(error),
                        },
                    );
                    continue;
                }
            };
            let pair = match store.refresh_if_needed(&id, SystemTime::now(), |current| {
                transport
                    .refresh(current.refresh_token())
                    .map_err(|error| match error {
                        github::oauth::OAuthError::Network => RotationError::Network,
                        _ => RotationError::Provider,
                    })
            }) {
                Ok(pair) => pair,
                Err(error) => {
                    auth.accounts.insert(
                        identity.id.clone(),
                        GithubAccountState::ReconnectRequired {
                            identity,
                            reason: match error {
                                RotationError::Network => GithubAuthFailure::Network,
                                RotationError::Provider => GithubAuthFailure::Provider,
                                RotationError::ReconnectRequired => GithubAuthFailure::Expired,
                                RotationError::Store(_) => {
                                    GithubAuthFailure::CredentialsUnavailable
                                }
                            },
                        },
                    );
                    continue;
                }
            };
            let current = github::http::HttpTransport::from_token_pair(&pair)
                .map(github::provider::GithubClient::new)
                .and_then(|client| client.current_identity());
            match current {
                Ok(current) if current.id == identity.id => {
                    auth.accounts
                        .insert(current.id.clone(), GithubAccountState::Connected(current));
                }
                Ok(_) => {
                    auth.accounts.insert(
                        identity.id.clone(),
                        GithubAccountState::ReconnectRequired {
                            identity,
                            reason: GithubAuthFailure::Provider,
                        },
                    );
                }
                Err(error) => {
                    auth.accounts.insert(
                        identity.id.clone(),
                        GithubAccountState::ReconnectRequired {
                            identity,
                            reason: failure_from_connection_error(error),
                        },
                    );
                }
            }
        }
        match legacy_store.accounts() {
            Ok(legacy_accounts) => {
                for account in legacy_accounts {
                    if account.provider.as_str() != "github" {
                        continue;
                    }
                    if auth.accounts.contains_key(&account.account_id) {
                        if let Err(error) = legacy_store
                            .remove_account(&ProviderAccountId::github(&account.account_id))
                        {
                            eprintln!(
                                "GitHub OAuth credential migration remains pending: {error:?}"
                            );
                        }
                        continue;
                    }
                    let identity = github::Identity {
                        id: account.account_id.clone(),
                        login: account.login,
                    };
                    auth.accounts.insert(
                        identity.id.clone(),
                        GithubAccountState::ReconnectRequired {
                            identity,
                            reason: GithubAuthFailure::AuthenticationChanged,
                        },
                    );
                }
            }
            Err(error) => {
                eprintln!("GitHub legacy credential cleanup remains pending: {error:?}");
            }
        }
        auth
    }
}

fn failure_from_oauth_error(error: github::oauth::OAuthError) -> GithubAuthFailure {
    match error {
        github::oauth::OAuthError::InvalidResponse => GithubAuthFailure::InvalidResponse,
        github::oauth::OAuthError::Network => GithubAuthFailure::Network,
        github::oauth::OAuthError::Provider => GithubAuthFailure::Provider,
        github::oauth::OAuthError::BrowserOpen => GithubAuthFailure::BrowserOpen,
        github::oauth::OAuthError::Cancelled => GithubAuthFailure::Cancelled,
        github::oauth::OAuthError::Timeout => GithubAuthFailure::Timeout,
        github::oauth::OAuthError::Denied => GithubAuthFailure::Denied,
        github::oauth::OAuthError::DeviceFlowDisabled => GithubAuthFailure::DeviceFlowDisabled,
        github::oauth::OAuthError::Expired => GithubAuthFailure::Expired,
    }
}

fn failure_from_connection_error(error: ConnectionError) -> GithubAuthFailure {
    match error {
        ConnectionError::Network | ConnectionError::Timeout => GithubAuthFailure::Network,
        ConnectionError::InvalidResponse => GithubAuthFailure::InvalidResponse,
        ConnectionError::SignedOut => GithubAuthFailure::Expired,
        ConnectionError::MissingScope => GithubAuthFailure::MissingScope,
        ConnectionError::Configuration => GithubAuthFailure::CredentialsUnavailable,
        _ => GithubAuthFailure::Provider,
    }
}

fn apply_account_connection_failure<T>(
    host: &Host,
    account_id: &str,
    result: &Result<T, ConnectionError>,
) {
    if !matches!(result, Err(ConnectionError::MissingScope)) {
        return;
    }
    if let Ok(mut auth) = host.github_auth.lock() {
        auth.apply_connection_result(account_id, result);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OAuthAccountPersistence {
    Saved,
    SavedWithLegacyCleanupPending,
}

fn persist_oauth_account_with_cleanup<S, C>(
    save: S,
    cleanup_legacy: C,
) -> Result<OAuthAccountPersistence, ()>
where
    S: FnOnce() -> Result<(), ()>,
    C: FnOnce() -> Result<(), ()>,
{
    save()?;
    Ok(if cleanup_legacy().is_ok() {
        OAuthAccountPersistence::Saved
    } else {
        OAuthAccountPersistence::SavedWithLegacyCleanupPending
    })
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
    account_id: &str,
) -> Result<
    (
        github::Identity,
        github::provider::GithubClient<github::http::HttpTransport>,
    ),
    ConnectionError,
> {
    use github::token_store::ProviderAccountId;
    let result = (|| {
        let transport = github::oauth::GithubOAuthHttp::new().map_err(|error| match error {
            github::oauth::OAuthError::Network => ConnectionError::Network,
            github::oauth::OAuthError::InvalidResponse => ConnectionError::InvalidResponse,
            _ => ConnectionError::ProviderFailure,
        })?;
        let key = ProviderAccountId::github(account_id);
        let pair = host
            .github_credentials
            .refresh_if_needed(&key, SystemTime::now(), |current| {
                transport
                    .refresh(current.refresh_token())
                    .map_err(|error| match error {
                        github::oauth::OAuthError::Network => {
                            github::token_store::RotationError::Network
                        }
                        _ => github::token_store::RotationError::Provider,
                    })
            })
            .map_err(rotation_connection_error)?;
        let client = github::provider::GithubClient::new(
            github::http::HttpTransport::from_token_pair(&pair)?,
        );
        let identity = client.current_identity()?;
        if identity.id != account_id {
            return Err(ConnectionError::WrongIdentity);
        }
        Ok((identity, client))
    })();
    if let Err(error) = result {
        if let Ok(mut auth) = host.github_auth.lock() {
            auth.set_failure(account_id, failure_from_connection_error(error));
        }
    }
    result
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
    repositories: Vec<github::provider::RemoteRepository>,
}

#[derive(Serialize)]
struct GithubRepositoryResolution {
    identity: github::Identity,
    repository: github::provider::RemoteRepository,
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
async fn resolve_provider_person(
    app: tauri::AppHandle,
    provider: storage::ProviderId,
    account_id: String,
    login: String,
) -> Result<github::Identity, ConnectionError> {
    if !matches!(provider, storage::ProviderId::Github) {
        return Err(ConnectionError::Configuration);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        github_session(&host, &account_id)?.1.resolve_person(&login)
    })
    .await
    .map_err(|_| ConnectionError::ProviderFailure)?
}

#[tauri::command]
async fn list_provider_repositories(
    app: tauri::AppHandle,
    provider: storage::ProviderId,
    account_id: String,
) -> Result<GithubRepositories, ConnectionError> {
    if !matches!(provider, storage::ProviderId::Github) {
        return Err(ConnectionError::Configuration);
    }
    let account_for_failure = account_id.clone();
    let app_for_read = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_read.state::<Host>();
        let (identity, client) = github_session(&host, &account_id)?;
        Ok(GithubRepositories {
            identity,
            repositories: client.accessible_repositories()?,
        })
    })
    .await
    .map_err(|_| ConnectionError::ProviderFailure)?;
    apply_account_connection_failure(&app.state::<Host>(), &account_for_failure, &result);
    result
}

#[tauri::command]
async fn resolve_provider_repository(
    app: tauri::AppHandle,
    provider: storage::ProviderId,
    account_id: String,
    repository: String,
) -> Result<GithubRepositoryResolution, ConnectionError> {
    if !matches!(provider, storage::ProviderId::Github) {
        return Err(ConnectionError::Configuration);
    }
    let account_for_failure = account_id.clone();
    let app_for_read = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_read.state::<Host>();
        let (identity, client) = github_session(&host, &account_id)?;
        let connection = client.connect(&repository, Some(&identity.id))?;
        Ok(GithubRepositoryResolution {
            identity,
            repository: connection.repository,
        })
    })
    .await
    .map_err(|_| ConnectionError::ProviderFailure)?;
    apply_account_connection_failure(&app.state::<Host>(), &account_for_failure, &result);
    result
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
fn start_github_browser_auth(
    app: tauri::AppHandle,
    expected_account_id: Option<String>,
    select_account: Option<bool>,
) -> Result<GithubAuthView, String> {
    let host = app.state::<Host>();
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    auth.cancel_attempt();
    let _ = select_account;
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let attempt_id = auth.start_attempt(expected_account_id, cancel_tx);
    let view = auth.view();
    drop(auth);
    tauri::async_runtime::spawn(complete_github_device_auth(
        app.clone(),
        attempt_id,
        cancel_rx,
    ));
    Ok(view)
}

async fn complete_github_device_auth(
    app: tauri::AppHandle,
    attempt_id: u64,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = Arc::clone(&cancelled);
    tauri::async_runtime::spawn(async move {
        if cancel_rx.await.is_ok() {
            cancellation.store(true, Ordering::SeqCst);
        }
    });
    let work_app = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let transport = github::oauth::GithubOAuthHttp::new().map_err(failure_from_oauth_error)?;
        let browser_app = work_app.clone();
        let browser_cancelled = Arc::clone(&cancelled);
        let authorization = transport
            .request_device_authorization(|url| {
                if browser_cancelled.load(Ordering::SeqCst)
                    || !browser_app
                        .state::<Host>()
                        .github_auth
                        .lock()
                        .ok()
                        .is_some_and(|auth| auth.is_active_attempt(attempt_id))
                {
                    return Err(github::oauth::OAuthError::Cancelled);
                }
                webbrowser::open(url.as_str()).map_err(|_| github::oauth::OAuthError::BrowserOpen)
            })
            .map_err(failure_from_oauth_error)?;
        if cancelled.load(Ordering::SeqCst) {
            return Err(GithubAuthFailure::Cancelled);
        }
        {
            let host = work_app.state::<Host>();
            let mut auth = host
                .github_auth
                .lock()
                .map_err(|_| GithubAuthFailure::InvalidResponse)?;
            if !auth.set_device_authorization(
                attempt_id,
                authorization.user_code().to_owned(),
                authorization.verification_uri().to_owned(),
            ) {
                return Err(GithubAuthFailure::Cancelled);
            }
        }
        eprintln!("[github-auth] stage=device_authorization outcome=ready");
        let pair = transport
            .poll_device_authorization(authorization, &cancelled)
            .map_err(failure_from_oauth_error)?;
        let identity = github::provider::GithubClient::new(
            github::http::HttpTransport::from_token_pair(&pair)
                .map_err(failure_from_connection_error)?,
        )
        .current_identity()
        .map_err(failure_from_connection_error)?;
        eprintln!("[github-auth] stage=identity_lookup outcome=success");
        Ok((identity, pair))
    })
    .await
    .unwrap_or(Err(GithubAuthFailure::InvalidResponse));
    let host = app.state::<Host>();
    let Ok(mut auth) = host.github_auth.lock() else {
        return;
    };
    eprintln!(
        "[github-auth] stage=attempt_complete attempt={attempt_id} outcome={}",
        if outcome.is_ok() {
            "success"
        } else {
            "failure"
        }
    );
    auth.finish_attempt(attempt_id, outcome);
}

#[tauri::command]
fn confirm_github_account(host: State<'_, Host>) -> Result<GithubAuthView, String> {
    use github::token_store::ActiveAccount;
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    let persistence = auth.confirm_with(|identity, pair| {
        use github::token_store::ProviderAccountId;
        let account = ActiveAccount::new(&identity.id, &identity.login).map_err(|_| ())?;
        persist_oauth_account_with_cleanup(
            || {
                host.github_credentials
                    .save_account(&account, pair, false)
                    .map_err(|_| ())
            },
            || {
                host.github_legacy_credentials
                    .remove_account(&ProviderAccountId::github(&identity.id))
                    .map_err(|_| ())
            },
        )
    })?;
    if persistence == OAuthAccountPersistence::SavedWithLegacyCleanupPending {
        eprintln!("GitHub OAuth account connected; legacy credential cleanup remains pending.");
    }
    Ok(auth.view())
}

#[tauri::command]
fn cancel_github_auth(host: State<'_, Host>) -> Result<GithubAuthView, String> {
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    auth.cancel_attempt();
    Ok(auth.view())
}

#[tauri::command]
fn disconnect_github_auth(
    host: State<'_, Host>,
    account_id: String,
) -> Result<GithubAuthView, String> {
    use github::token_store::ProviderAccountId;
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    if !auth.accounts.contains_key(&account_id) {
        return Err("No connected GitHub account is available to disconnect.".into());
    }
    host.github_credentials
        .remove_account(&ProviderAccountId::github(&account_id))
        .map_err(|_| "GitHub credentials could not be deleted securely.")?;
    host.github_legacy_credentials
        .remove_account(&ProviderAccountId::github(&account_id))
        .map_err(|_| "Superseded GitHub credentials could not be deleted securely.")?;
    auth.accounts.remove(&account_id);
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

#[tauri::command]
async fn verify_provider_connection(
    app: tauri::AppHandle,
    host: State<'_, Host>,
    id: String,
) -> Result<Connection, ConnectionError> {
    let repository = configured_repository(&host, &id)?;
    let binding = repository
        .account_binding()
        .ok_or(ConnectionError::Configuration)?;
    if !matches!(binding.account.provider, storage::ProviderId::Github) {
        return Err(ConnectionError::Configuration);
    }
    let expected = repository.clone();
    let account_id = binding.account.account_id;
    let account_for_failure = account_id.clone();
    let app_for_read = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_read.state::<Host>();
        let (identity, client) = github_session(&host, &account_id)?;
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
    apply_account_connection_failure(&host, &account_for_failure, &result);
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
async fn read_provider_metadata(
    app: tauri::AppHandle,
    host: State<'_, Host>,
    id: String,
    expected_account_id: String,
    expected_repository_id: String,
) -> Result<GithubMetadata, ConnectionError> {
    let repository = configured_repository(&host, &id)?;
    let binding = repository
        .account_binding()
        .ok_or(ConnectionError::Configuration)?;
    if !matches!(binding.account.provider, storage::ProviderId::Github)
        || binding.account.account_id != expected_account_id
    {
        return Err(ConnectionError::WrongIdentity);
    }
    let expected = repository.clone();
    let account_id = binding.account.account_id;
    let account_for_failure = account_id.clone();
    let app_for_read = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_read.state::<Host>();
        let (identity, client) = github_session(&host, &account_id)?;
        if identity.id != expected_account_id {
            return Err(ConnectionError::WrongIdentity);
        }
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
    apply_account_connection_failure(&host, &account_for_failure, &result);
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

fn github_keychain_stores(
    isolated: bool,
    override_service: Option<std::ffi::OsString>,
) -> Result<
    (
        github::macos_keychain::MacKeychainStore,
        github::macos_keychain::MacKeychainStore,
    ),
    std::io::Error,
> {
    match (isolated, override_service) {
        (false, None) => Ok((
            github::macos_keychain::MacKeychainStore::production(),
            github::macos_keychain::MacKeychainStore::legacy_production(),
        )),
        (true, Some(service)) => {
            let service = service.to_str().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "PR_SNIPER_KEYCHAIN_SERVICE must be valid UTF-8.",
                )
            })?;
            if !service.starts_with("com.jdylanmc.pr-sniper.tests.")
                || service.len() > 200
                || service.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'_')
                })
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "PR_SNIPER_KEYCHAIN_SERVICE must be a test-owned service beginning with com.jdylanmc.pr-sniper.tests.",
                ));
            }
            Ok((
                github::macos_keychain::MacKeychainStore::with_service(service),
                github::macos_keychain::MacKeychainStore::with_service(format!("{service}.legacy")),
            ))
        }
        (true, None) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "PR_SNIPER_KEYCHAIN_SERVICE is required with PR_SNIPER_DATA_DIR.",
        )),
        (false, Some(_)) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "PR_SNIPER_KEYCHAIN_SERVICE requires PR_SNIPER_DATA_DIR.",
        )),
    }
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
            resolve_provider_person,
            list_provider_repositories,
            resolve_provider_repository,
            save_login,
            save_repository,
            update_repository,
            remove_repository,
            save_defaults,
            save_repository_policy,
            verify_provider_connection,
            read_provider_metadata,
            github_auth_state,
            start_github_browser_auth,
            confirm_github_account,
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
            let (github_keychain, legacy_github_keychain) =
                github_keychain_stores(isolated, std::env::var_os("PR_SNIPER_KEYCHAIN_SERVICE"))?;
            let github_credentials = github::token_store::RotationSafeStore::new(github_keychain);
            let github_legacy_credentials =
                github::token_store::RotationSafeStore::new(legacy_github_keychain);
            let github_auth = GithubAuth::restore(&github_credentials, &github_legacy_credentials);
            let store = Store::new(root);
            app.manage(Host {
                store: Mutex::new(store),
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
                github_legacy_credentials,
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
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit PR Sniper", true, Some("CmdOrCtrl+Q"))?;
            let menu = Menu::with_items(
                app,
                &[
                    &status, &queue, &check, &settings, &separator, &quit,
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
                        let host = app.state::<Host>();
                        host.quitting.store(true, Ordering::SeqCst);
                        if let Ok(mut auth) = host.github_auth.lock() {
                            auth.cancel_attempt();
                        }
                        app.exit(0);
                        return;
                    }

                    let target = match event.id.as_ref() {
                        "status" => ("status", "Status"),
                        "queue" => ("queue", "Review Queue"),
                        "settings" => ("settings", "Settings"),
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
        failure_from_connection_error, failure_from_oauth_error, github_keychain_stores,
        persist_oauth_account_with_cleanup, rotation_connection_error, GithubAccountState,
        GithubAuth, GithubAuthFailure, OAuthAccountPersistence,
    };
    use crate::github::oauth::OAuthError;
    use crate::github::token_store::{
        AccountRegistry, AccountRegistryStore, CredentialKey, CredentialStore, RotationError,
        StoreError,
    };
    use crate::github::ConnectionError;
    use std::{
        collections::BTreeMap,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::{Duration, SystemTime},
    };

    #[derive(Clone, Default)]
    struct FaultStore {
        registry: Arc<Mutex<AccountRegistry>>,
        credentials: Arc<
            Mutex<
                BTreeMap<
                    crate::github::token_store::ProviderAccountId,
                    crate::github::oauth::TokenPair,
                >,
            >,
        >,
        fail_delete: Arc<AtomicBool>,
        credential_loads: Arc<AtomicUsize>,
    }

    impl CredentialStore for FaultStore {
        fn load(
            &self,
            key: &CredentialKey,
        ) -> Result<Option<crate::github::oauth::TokenPair>, StoreError> {
            self.credential_loads.fetch_add(1, Ordering::SeqCst);
            Ok(self.credentials.lock().unwrap().get(key).cloned())
        }

        fn save(
            &self,
            key: &CredentialKey,
            pair: &crate::github::oauth::TokenPair,
        ) -> Result<(), StoreError> {
            self.credentials
                .lock()
                .unwrap()
                .insert(key.clone(), pair.clone());
            Ok(())
        }

        fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err(StoreError::Unavailable);
            }
            self.credentials.lock().unwrap().remove(key);
            Ok(())
        }
    }

    impl AccountRegistryStore for FaultStore {
        fn load_registry(&self) -> Result<AccountRegistry, StoreError> {
            Ok(self.registry.lock().unwrap().clone())
        }

        fn save_registry(&self, registry: &AccountRegistry) -> Result<(), StoreError> {
            *self.registry.lock().unwrap() = registry.clone();
            Ok(())
        }
    }

    fn identity(id: &str, login: &str) -> crate::github::Identity {
        crate::github::Identity {
            id: id.into(),
            login: login.into(),
        }
    }

    fn pair(label: &str) -> crate::github::oauth::TokenPair {
        crate::github::oauth::TokenPair::new(
            format!("access-{label}"),
            format!("refresh-{label}"),
            Duration::from_secs(60),
            Duration::from_secs(120),
        )
    }

    #[test]
    fn oauth_failures_preserve_their_host_reason() {
        for (error, expected) in [
            (OAuthError::Network, GithubAuthFailure::Network),
            (OAuthError::Provider, GithubAuthFailure::Provider),
            (
                OAuthError::InvalidResponse,
                GithubAuthFailure::InvalidResponse,
            ),
            (OAuthError::BrowserOpen, GithubAuthFailure::BrowserOpen),
            (OAuthError::Cancelled, GithubAuthFailure::Cancelled),
            (OAuthError::Timeout, GithubAuthFailure::Timeout),
            (OAuthError::Denied, GithubAuthFailure::Denied),
            (
                OAuthError::DeviceFlowDisabled,
                GithubAuthFailure::DeviceFlowDisabled,
            ),
            (OAuthError::Expired, GithubAuthFailure::Expired),
        ] {
            assert_eq!(failure_from_oauth_error(error), expected);
        }
    }

    #[test]
    fn begin_identity_refresh_and_restore_failures_keep_network_distinct() {
        assert_eq!(
            failure_from_oauth_error(OAuthError::Network),
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

    #[test]
    fn account_failure_does_not_change_other_accounts() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            GithubAccountState::Connected(crate::github::Identity {
                id: "101".into(),
                login: "account-a".into(),
            }),
        );
        auth.accounts.insert(
            "202".into(),
            GithubAccountState::Connected(crate::github::Identity {
                id: "202".into(),
                login: "account-b".into(),
            }),
        );

        auth.set_failure("101", GithubAuthFailure::Expired);

        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::Expired,
                ..
            })
        ));
        assert!(matches!(
            auth.accounts.get("202"),
            Some(GithubAccountState::Connected(identity)) if identity.login == "account-b"
        ));
    }

    #[test]
    fn pending_confirmation_exposes_identity_without_persisting_or_serializing_tokens() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            GithubAccountState::Connected(crate::github::Identity {
                id: "101".into(),
                login: "account-a".into(),
            }),
        );
        auth.pending = Some(super::PendingGithubAccount {
            identity: crate::github::Identity {
                id: "202".into(),
                login: "account-b".into(),
            },
            pair: crate::github::oauth::TokenPair::new(
                "pending-access-secret",
                "pending-refresh-secret",
                Duration::from_secs(60),
                Duration::from_secs(120),
            ),
        });

        let serialized = serde_json::to_string(&auth.view()).unwrap();
        assert!(serialized.contains("\"account_id\":\"202\""));
        assert!(serialized.contains("\"login\":\"account-b\""));
        assert!(!serialized.contains("pending-access-secret"));
        assert!(!serialized.contains("pending-refresh-secret"));
        assert_eq!(auth.accounts.len(), 1);
        assert!(GithubAuth::new().pending.is_none());
    }

    #[test]
    fn replacing_an_attempt_cancels_it_and_ignores_its_late_completion() {
        let mut auth = GithubAuth::new();
        let (first_cancel, mut first_cancelled) = tokio::sync::oneshot::channel();
        let first = auth.start_attempt(None, first_cancel);
        let (second_cancel, _second_cancelled) = tokio::sync::oneshot::channel();
        let second = auth.start_attempt(None, second_cancel);

        assert!(first_cancelled.try_recv().is_ok());
        auth.finish_attempt(first, Ok((identity("101", "first"), pair("first"))));
        assert!(auth.pending.is_none());

        auth.finish_attempt(second, Ok((identity("202", "second"), pair("second"))));
        assert_eq!(
            auth.pending
                .as_ref()
                .map(|pending| pending.identity.id.as_str()),
            Some("202")
        );
    }

    #[test]
    fn device_user_code_is_transient_and_the_secret_device_code_never_enters_the_view() {
        let mut auth = GithubAuth::new();
        let (cancel, _) = tokio::sync::oneshot::channel();
        let attempt = auth.start_attempt(None, cancel);
        assert!(auth.set_device_authorization(
            attempt,
            "ABCD-EFGH".into(),
            "https://github.com/login/device".into(),
        ));

        let serialized = serde_json::to_string(&auth.view()).unwrap();
        assert!(serialized.contains("ABCD-EFGH"));
        assert!(serialized.contains("https://github.com/login/device"));
        assert!(!serialized.contains("device_code"));
        assert!(!serialized.contains("secret-device-code"));

        auth.cancel_attempt();
        let serialized = serde_json::to_string(&auth.view()).unwrap();
        assert!(!serialized.contains("ABCD-EFGH"));
        assert!(!serialized.contains("github.com/login/device"));
    }

    #[test]
    fn cancel_and_shutdown_cleanup_discard_active_and_pending_authentication() {
        let mut auth = GithubAuth::new();
        let (cancel, mut cancelled) = tokio::sync::oneshot::channel();
        let attempt = auth.start_attempt(None, cancel);
        auth.finish_attempt(attempt, Ok((identity("101", "first"), pair("first"))));
        assert!(auth.pending.is_some());

        auth.cancel_attempt();

        assert!(auth.active.is_none());
        assert!(auth.pending.is_none());
        assert!(matches!(
            cancelled.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
    }

    #[test]
    fn wrong_identity_duplicate_and_timeout_remain_typed_terminal_states() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            GithubAccountState::Connected(identity("101", "existing")),
        );

        let (cancel, _) = tokio::sync::oneshot::channel();
        let reconnect = auth.start_attempt(Some("101".into()), cancel);
        auth.finish_attempt(reconnect, Ok((identity("202", "wrong"), pair("wrong"))));
        assert_eq!(auth.failure, Some(GithubAuthFailure::WrongIdentity));
        assert!(auth.pending.is_none());

        let (cancel, _) = tokio::sync::oneshot::channel();
        let duplicate = auth.start_attempt(None, cancel);
        auth.finish_attempt(
            duplicate,
            Ok((identity("101", "existing"), pair("duplicate"))),
        );
        assert_eq!(auth.failure, Some(GithubAuthFailure::WrongIdentity));

        let (cancel, _) = tokio::sync::oneshot::channel();
        let timed_out = auth.start_attempt(None, cancel);
        auth.finish_attempt(timed_out, Err(GithubAuthFailure::Timeout));
        assert_eq!(auth.failure, Some(GithubAuthFailure::Timeout));
    }

    #[test]
    fn confirmation_is_the_only_persistence_boundary_and_failure_is_retryable() {
        let mut auth = GithubAuth::new();
        let (cancel, _) = tokio::sync::oneshot::channel();
        let attempt = auth.start_attempt(None, cancel);
        auth.finish_attempt(attempt, Ok((identity("101", "octocat"), pair("one"))));
        assert!(auth.accounts.is_empty());

        let mut persistence_attempts = 0;
        assert!(auth
            .confirm_with(|_, _| {
                persistence_attempts += 1;
                Err::<(), ()>(())
            })
            .is_err());
        assert_eq!(persistence_attempts, 1);
        assert!(auth.pending.is_some());
        assert_eq!(
            auth.pending_failure,
            Some(GithubAuthFailure::CredentialsUnavailable)
        );
        assert!(auth.accounts.is_empty());

        auth.confirm_with(|_, _| {
            persistence_attempts += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(persistence_attempts, 2);
        assert!(auth.pending.is_none());
        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::Connected(identity)) if identity.login == "octocat"
        ));
        assert!(GithubAuth::new().pending.is_none());
    }

    #[test]
    fn use_different_account_discards_pending_credentials_without_touching_connected_accounts() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            GithubAccountState::Connected(identity("101", "existing")),
        );
        let (first_cancel, _) = tokio::sync::oneshot::channel();
        let first = auth.start_attempt(None, first_cancel);
        auth.finish_attempt(first, Ok((identity("202", "pending"), pair("pending"))));

        let (replacement_cancel, _) = tokio::sync::oneshot::channel();
        auth.start_attempt(None, replacement_cancel);

        assert!(auth.pending.is_none());
        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::Connected(identity)) if identity.login == "existing"
        ));
    }

    #[test]
    fn missing_scope_invalidates_only_the_affected_account() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            GithubAccountState::Connected(identity("101", "first")),
        );
        auth.accounts.insert(
            "202".into(),
            GithubAccountState::Connected(identity("202", "second")),
        );

        auth.apply_connection_result::<()>("101", &Err(ConnectionError::MissingScope));

        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::MissingScope,
                ..
            })
        ));
        assert!(matches!(
            auth.accounts.get("202"),
            Some(GithubAccountState::Connected(identity)) if identity.login == "second"
        ));

        auth.apply_connection_result::<()>("202", &Err(ConnectionError::OrganizationPolicyDenied));
        assert!(matches!(
            auth.accounts.get("202"),
            Some(GithubAccountState::Connected(_))
        ));
    }

    #[test]
    fn confirmed_oauth_account_stays_connected_while_legacy_cleanup_retries() {
        use crate::github::token_store::{ActiveAccount, ProviderAccountId, RotationSafeStore};

        let current_storage = FaultStore::default();
        let legacy_storage = FaultStore::default();
        let current = RotationSafeStore::new(current_storage.clone());
        let legacy = RotationSafeStore::new(legacy_storage.clone());
        let account = ActiveAccount::new("101", "octocat").unwrap();
        let account_id = ProviderAccountId::github("101");
        legacy
            .save_account(&account, &pair("legacy"), false)
            .unwrap();
        legacy_storage.fail_delete.store(true, Ordering::SeqCst);

        let mut auth = GithubAuth::new();
        let (cancel, _) = tokio::sync::oneshot::channel();
        let attempt = auth.start_attempt(None, cancel);
        auth.finish_attempt(attempt, Ok((identity("101", "octocat"), pair("oauth"))));
        let persistence = auth
            .confirm_with(|identity, token_pair| {
                let confirmed =
                    ActiveAccount::new(&identity.id, &identity.login).map_err(|_| ())?;
                persist_oauth_account_with_cleanup(
                    || {
                        current
                            .save_account(&confirmed, token_pair, false)
                            .map_err(|_| ())
                    },
                    || legacy.remove_account(&account_id).map_err(|_| ()),
                )
            })
            .unwrap();

        assert_eq!(
            persistence,
            OAuthAccountPersistence::SavedWithLegacyCleanupPending
        );
        assert!(auth.pending.is_none());
        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::Connected(identity)) if identity.login == "octocat"
        ));
        auth.cancel_attempt();
        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::Connected(_))
        ));

        legacy_storage.fail_delete.store(false, Ordering::SeqCst);
        let restarted_current = RotationSafeStore::new(current_storage);
        let restarted_legacy = RotationSafeStore::new(legacy_storage.clone());
        assert_eq!(
            restarted_current
                .restore_account(&account_id)
                .unwrap()
                .unwrap()
                .pair
                .access_token(),
            "access-oauth"
        );
        assert!(restarted_legacy.accounts().unwrap().is_empty());
        assert!(restarted_legacy
            .restore_account(&account_id)
            .unwrap()
            .is_none());
        assert_eq!(legacy_storage.credential_loads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn legacy_github_app_credentials_require_reconnect_without_provider_use() {
        use crate::github::token_store::{ActiveAccount, ProviderAccountId, RotationSafeStore};
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let current = RotationSafeStore::new(
            crate::github::macos_keychain::MacKeychainStore::with_service(format!(
                "com.jdylanmc.pr-sniper.tests.oauth-current-{nonce}"
            )),
        );
        let legacy = RotationSafeStore::new(
            crate::github::macos_keychain::MacKeychainStore::with_service(format!(
                "com.jdylanmc.pr-sniper.tests.oauth-legacy-{nonce}"
            )),
        );
        let account = ActiveAccount::new("101", "legacy").unwrap();
        legacy
            .save_account(&account, &pair("legacy"), false)
            .unwrap();

        let auth = GithubAuth::restore(&current, &legacy);

        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::AuthenticationChanged,
                ..
            })
        ));
        assert!(current
            .restore_account(&ProviderAccountId::github("101"))
            .unwrap()
            .is_none());
        assert!(legacy
            .restore_account(&ProviderAccountId::github("101"))
            .unwrap()
            .is_some());
        legacy
            .remove_account(&ProviderAccountId::github("101"))
            .unwrap();
    }

    #[test]
    fn isolated_keychain_configuration_is_explicit_and_test_owned() {
        use crate::github::token_store::{ActiveAccount, ProviderAccountId, RotationSafeStore};
        assert!(github_keychain_stores(true, None).is_err());
        assert!(
            github_keychain_stores(false, Some("com.jdylanmc.pr-sniper.tests.invalid".into()))
                .is_err()
        );
        assert!(github_keychain_stores(true, Some("personal.service".into())).is_err());
        assert!(github_keychain_stores(
            true,
            Some("com.jdylanmc.pr-sniper.tests.native-123".into())
        )
        .is_ok());

        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let service = format!("com.jdylanmc.pr-sniper.tests.restart-{nonce}");
        let (first_store, _) = github_keychain_stores(true, Some(service.clone().into())).unwrap();
        let first = RotationSafeStore::new(first_store);
        let account = ActiveAccount::new("101", "isolated").unwrap();
        first
            .save_account(&account, &pair("isolated"), false)
            .unwrap();
        drop(first);

        let (restarted_store, _) =
            github_keychain_stores(true, Some(service.clone().into())).unwrap();
        let restarted = RotationSafeStore::new(restarted_store);
        assert_eq!(
            restarted
                .restore_account(&ProviderAccountId::github("101"))
                .unwrap()
                .unwrap()
                .pair
                .access_token(),
            "access-isolated"
        );

        let (other_store, _) = github_keychain_stores(
            true,
            Some(format!("com.jdylanmc.pr-sniper.tests.other-{nonce}").into()),
        )
        .unwrap();
        assert!(RotationSafeStore::new(other_store)
            .restore_account(&ProviderAccountId::github("101"))
            .unwrap()
            .is_none());
        restarted
            .remove_account(&ProviderAccountId::github("101"))
            .unwrap();
    }
}
