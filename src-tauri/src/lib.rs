pub mod actions;
pub mod capacity;
mod copilot;
mod doctrine_seeds;
pub mod feedback;
pub mod follow_up;
pub mod github;
pub mod monitoring;
pub mod notifications;
pub mod panel;
pub mod policy;
mod process_path;
pub mod publication;
pub mod queue;
pub mod retention;
pub mod review;
pub mod startup;
pub mod storage;
mod storage_state;

// Some helpers are consumed only by separate integration-test crates.
#[cfg(all(test, windows))]
#[allow(dead_code)]
#[path = "../tests/support/windows_permissions.rs"]
pub(crate) mod windows_permissions;

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
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, State,
};

struct Host {
    panel: panel::Panel,
    store: Mutex<Store>,
    monitor: Mutex<monitoring::Monitor>,
    error: Mutex<Option<String>>,
    isolated: bool,
    quitting: AtomicBool,
    registration: LoginRegistration,
    github_auth: Mutex<GithubAuth>,
    github_generations: Mutex<BTreeMap<String, u64>>,
    github_credentials: github::token_store::RotationSafeStore<github::NativeCredentialStore>,
    github_legacy_credentials:
        Option<github::token_store::RotationSafeStore<github::NativeCredentialStore>>,
    copilot: Arc<copilot::Integration>,
    ai: capacity::Coordinator,
    publications: publication::host::Coordinator,
    follow_ups: follow_up::host::Coordinator,
    actions: actions::host::Coordinator,
    mutations: actions::host::MutationOwner,
    notifications: notifications::host::Coordinator,
}

#[derive(Clone, Copy)]
enum ConnectionRole {
    Repository,
    Copilot,
}

impl ConnectionRole {
    fn auth(self, host: &Host) -> &Mutex<GithubAuth> {
        match self {
            Self::Repository => &host.github_auth,
            Self::Copilot => &host.copilot.auth,
        }
    }
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
    VerificationPending,
    VerificationRequired,
    Disconnected,
    DisconnectPending,
    DisconnectFailed,
    Expired,
    Denied,
    DeviceFlowDisabled,
    Network,
    RateLimited,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        warning: Option<GithubAuthFailure>,
        #[serde(skip_serializing_if = "Option::is_none")]
        connection_generation: Option<u64>,
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
                    warning: None,
                    connection_generation: None,
                },
                GithubAccountState::ReconnectRequired { identity, reason }
                    if reason.retryable_without_reconnect() =>
                {
                    GithubAccountView::Connected {
                        provider: "github",
                        account_id: identity.id.clone(),
                        login: identity.login.clone(),
                        warning: Some(*reason),
                        connection_generation: None,
                    }
                }
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
        GithubAuthView {
            accounts,
            flow: self.flow_view(),
        }
    }

    fn repository_view(&self, generations: &BTreeMap<String, u64>) -> GithubAuthView {
        let mut view = self.view();
        for account in &mut view.accounts {
            if let GithubAccountView::Connected {
                account_id,
                connection_generation,
                ..
            } = account
            {
                *connection_generation = Some(generations.get(account_id).copied().unwrap_or(0));
            }
        }
        view
    }

    fn flow_view(&self) -> GithubFlowView {
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
        flow
    }

    fn copilot_view(&self) -> GithubAuthView {
        let accounts = self
            .accounts
            .values()
            .map(|state| match state {
                GithubAccountState::Connected(identity) => GithubAccountView::Connected {
                    provider: "copilot",
                    account_id: identity.id.clone(),
                    login: identity.login.clone(),
                    warning: None,
                    connection_generation: None,
                },
                GithubAccountState::ReconnectRequired { identity, reason } => {
                    GithubAccountView::ReconnectRequired {
                        provider: "copilot",
                        account_id: identity.id.clone(),
                        login: identity.login.clone(),
                        reason: *reason,
                    }
                }
            })
            .collect();
        GithubAuthView {
            accounts,
            flow: self.flow_view(),
        }
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

    fn account_session_allowed(&self, account_id: &str) -> Result<(), ConnectionError> {
        match self.accounts.get(account_id) {
            Some(GithubAccountState::Connected(_)) => Ok(()),
            Some(GithubAccountState::ReconnectRequired { reason, .. })
                if reason.retryable_without_reconnect() =>
            {
                Ok(())
            }
            Some(GithubAccountState::ReconnectRequired { reason, .. }) => {
                Err(reason.connection_error())
            }
            None => Err(ConnectionError::SignedOut),
        }
    }

    fn publish_session_success(
        &mut self,
        account_id: &str,
        identity: github::Identity,
    ) -> Result<(), ConnectionError> {
        self.account_session_allowed(account_id)?;
        if identity.id != account_id {
            self.publish_session_failure(account_id, ConnectionError::WrongIdentity);
            return Err(ConnectionError::WrongIdentity);
        }
        self.accounts
            .insert(account_id.into(), GithubAccountState::Connected(identity));
        Ok(())
    }

    fn publish_session_failure(&mut self, account_id: &str, error: ConnectionError) {
        self.publish_account_failure(account_id, failure_from_connection_error(error));
    }

    fn publish_acquisition_failure(&mut self, account_id: &str, error: ConnectionError) {
        self.publish_account_failure(account_id, failure_from_acquisition_error(error));
    }

    fn publish_account_failure(&mut self, account_id: &str, reason: GithubAuthFailure) {
        let Some(state) = self.accounts.get(account_id) else {
            return;
        };
        if matches!(
            state,
            GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::Disconnected
                    | GithubAuthFailure::DisconnectPending
                    | GithubAuthFailure::DisconnectFailed,
                ..
            }
        ) || (reason.retryable_without_reconnect()
            && matches!(
                state,
                GithubAccountState::ReconnectRequired {
                    reason: current,
                    ..
                } if !current.retryable_without_reconnect()
            ))
        {
            return;
        }
        self.set_failure(account_id, reason);
    }

    fn publish_restored_identity(
        &mut self,
        expected: github::Identity,
        result: Result<github::Identity, ConnectionError>,
    ) {
        let state = match result {
            Ok(identity) if identity.id == expected.id => GithubAccountState::Connected(identity),
            Ok(_) => GithubAccountState::ReconnectRequired {
                identity: expected,
                reason: GithubAuthFailure::WrongIdentity,
            },
            Err(error) => GithubAccountState::ReconnectRequired {
                identity: expected,
                reason: failure_from_connection_error(error),
            },
        };
        let id = match &state {
            GithubAccountState::Connected(identity)
            | GithubAccountState::ReconnectRequired { identity, .. } => identity.id.clone(),
        };
        self.accounts.insert(id, state);
    }

    fn monitoring_accounts(&self) -> BTreeMap<String, monitoring::AccountAvailability> {
        self.accounts
            .iter()
            .map(|(id, state)| {
                let (identity, connected) = match state {
                    GithubAccountState::Connected(identity) => (identity, true),
                    GithubAccountState::ReconnectRequired { identity, reason } => {
                        (identity, reason.retryable_without_reconnect())
                    }
                };
                (
                    id.clone(),
                    monitoring::AccountAvailability {
                        login: identity.login.clone(),
                        connected,
                    },
                )
            })
            .collect()
    }

    fn setup_accounts(&self) -> BTreeMap<String, monitoring::AccountAvailability> {
        let mut accounts = self.monitoring_accounts();
        for (id, account) in &mut accounts {
            account.connected = matches!(
                self.accounts.get(id),
                Some(GithubAccountState::Connected(_))
            );
        }
        accounts
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
                    self.failure = Some(GithubAuthFailure::WrongIdentity);
                } else {
                    self.pending = Some(PendingGithubAccount { identity, pair });
                    self.pending_failure = None;
                    self.failure = None;
                }
            }
            Err(reason) => {
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

    fn confirm_repository_with<F, T>(
        &mut self,
        generations: &mut BTreeMap<String, u64>,
        persist: F,
    ) -> Result<T, String>
    where
        F: FnOnce(&github::Identity, &github::oauth::TokenPair) -> Result<T, ()>,
    {
        self.confirm_with(|identity, pair| {
            let result = persist(identity, pair)?;
            *generations.entry(identity.id.clone()).or_default() += 1;
            Ok(result)
        })
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

    fn restore_accounts(
        store: &github::token_store::RotationSafeStore<github::NativeCredentialStore>,
        provider: github::token_store::ProviderId,
    ) -> Result<Self, github::token_store::StoreError> {
        use github::token_store::{ProviderAccountId, RotationError};
        let mut auth = Self::new();
        let accounts = store.accounts()?;
        for account in accounts {
            if account.provider != provider {
                continue;
            }
            let identity = github::Identity {
                id: account.account_id.clone(),
                login: account.login.clone(),
            };
            let id = ProviderAccountId::new(provider.clone(), &account.account_id)?;
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
                    .map_err(oauth_refresh_rotation_error)
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
            auth.publish_restored_identity(identity, current);
        }
        Ok(auth)
    }

    fn restore(
        store: &github::token_store::RotationSafeStore<github::NativeCredentialStore>,
        legacy_store: Option<
            &github::token_store::RotationSafeStore<github::NativeCredentialStore>,
        >,
    ) -> Self {
        use github::token_store::ProviderAccountId;
        let mut auth =
            match Self::restore_accounts(store, github::token_store::ProviderId::github()) {
                Ok(auth) => auth,
                Err(_) => {
                    eprintln!("GitHub credential registry is unavailable.");
                    Self::new()
                }
            };
        let Some(legacy_store) = legacy_store else {
            return auth;
        };
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

impl GithubAuthFailure {
    fn retryable_without_reconnect(self) -> bool {
        matches!(
            self,
            Self::Network
                | Self::RateLimited
                | Self::Provider
                | Self::InvalidResponse
                | Self::Timeout
        )
    }

    fn connection_error(self) -> ConnectionError {
        match self {
            Self::Expired
            | Self::Disconnected
            | Self::DisconnectPending
            | Self::DisconnectFailed => ConnectionError::SignedOut,
            Self::WrongIdentity => ConnectionError::WrongIdentity,
            Self::MissingScope => ConnectionError::MissingScope,
            Self::CredentialsUnavailable
            | Self::AuthenticationChanged
            | Self::VerificationPending
            | Self::VerificationRequired => ConnectionError::Configuration,
            Self::Network => ConnectionError::Network,
            Self::RateLimited => ConnectionError::RateLimited,
            Self::InvalidResponse => ConnectionError::InvalidResponse,
            Self::Timeout => ConnectionError::Timeout,
            Self::Provider
            | Self::Denied
            | Self::DeviceFlowDisabled
            | Self::BrowserOpen
            | Self::Cancelled => ConnectionError::ProviderFailure,
        }
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
        github::oauth::OAuthError::RefreshRejected => GithubAuthFailure::Expired,
    }
}

fn failure_from_connection_error(error: ConnectionError) -> GithubAuthFailure {
    match error {
        ConnectionError::RateLimitedWithContext {
            missing_repo_scope: true,
            ..
        } => GithubAuthFailure::MissingScope,
        ConnectionError::Network => GithubAuthFailure::Network,
        ConnectionError::RateLimited
        | ConnectionError::RateLimitedAfter(_)
        | ConnectionError::RateLimitedWithContext { .. } => GithubAuthFailure::RateLimited,
        ConnectionError::Timeout => GithubAuthFailure::Timeout,
        ConnectionError::InvalidResponse => GithubAuthFailure::InvalidResponse,
        ConnectionError::SignedOut => GithubAuthFailure::Expired,
        ConnectionError::WrongIdentity => GithubAuthFailure::WrongIdentity,
        ConnectionError::MissingScope
        | ConnectionError::OrganizationPolicyDeniedWithMissingScope => {
            GithubAuthFailure::MissingScope
        }
        ConnectionError::Configuration => GithubAuthFailure::CredentialsUnavailable,
        _ => GithubAuthFailure::Provider,
    }
}

fn failure_from_acquisition_error(error: ConnectionError) -> GithubAuthFailure {
    match error {
        ConnectionError::BrokenCli | ConnectionError::Configuration => {
            GithubAuthFailure::CredentialsUnavailable
        }
        _ => failure_from_connection_error(error),
    }
}

fn apply_account_connection_failure<T>(
    host: &Host,
    account_id: &str,
    result: &Result<T, ConnectionError>,
) {
    if let Err(error) = result {
        if matches!(
            error,
            ConnectionError::SignedOut
                | ConnectionError::WrongIdentity
                | ConnectionError::MissingScope
                | ConnectionError::OrganizationPolicyDeniedWithMissingScope
                | ConnectionError::Network
                | ConnectionError::Timeout
                | ConnectionError::RateLimited
                | ConnectionError::RateLimitedAfter(_)
                | ConnectionError::RateLimitedWithContext { .. }
                | ConnectionError::ProviderFailure
                | ConnectionError::ProviderFailureAfter(_)
                | ConnectionError::InvalidResponse
        ) {
            if let Ok(mut auth) = host.github_auth.lock() {
                auth.publish_session_failure(account_id, *error);
            }
        }
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

fn oauth_refresh_rotation_error(
    error: github::oauth::OAuthError,
) -> github::token_store::RotationError {
    match error {
        github::oauth::OAuthError::Network => github::token_store::RotationError::Network,
        github::oauth::OAuthError::RefreshRejected => {
            github::token_store::RotationError::ReconnectRequired
        }
        _ => github::token_store::RotationError::Provider,
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
    let result = acquire_github_session(host, account_id).and_then(|(identity, client)| {
        host.github_auth
            .lock()
            .map_err(|_| ConnectionError::Configuration)?
            .publish_session_success(account_id, identity.clone())?;
        Ok((identity, client))
    });
    if let Err(error) = result {
        if let Ok(mut auth) = host.github_auth.lock() {
            auth.publish_session_failure(account_id, error);
        }
    }
    result
}

// Intake owns publication at its generation-fenced completion boundary.
fn acquire_github_session(
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
    host.github_auth
        .lock()
        .map_err(|_| ConnectionError::Configuration)?
        .account_session_allowed(account_id)?;
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
                .map_err(oauth_refresh_rotation_error)
        })
        .map_err(rotation_connection_error)?;
    let client =
        github::provider::GithubClient::new(github::http::HttpTransport::from_token_pair(&pair)?);
    let identity = client.current_identity()?;
    Ok((identity, client))
}

#[cfg(test)]
mod repository_save_account_tests {
    use super::*;

    #[test]
    fn monitoring_enable_rechecks_account_and_generation_but_disable_needs_no_provider_session() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().to_path_buf());
        let mut settings = store.load_settings().unwrap();
        let repository: storage::Repository = serde_json::from_value(serde_json::json!({
            "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "provider":"github",
            "name":"owner/repo", "enabled":false,
            "provider_account_id":"22", "provider_repository_id":"100",
            "assignments":[{
                "id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                "agent_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},
                "comment":false,"approve":false
            }]
        }))
        .unwrap();
        settings.repositories.push(repository.clone());
        settings.agents.push(
            serde_json::from_value(serde_json::json!({
                "id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "name":"Reviewer","model":"fixture-model","prompt":"Find defects.","signature":"fixture",
                "ai_account":{"provider":"copilot","account_id":"33"}
            }))
            .unwrap(),
        );
        store.save_settings(&settings).unwrap();
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(github::Identity {
                id: "22".into(),
                login: "actor".into(),
            }),
        );
        let generations = BTreeMap::from([("22".into(), 3)]);
        let mut enabled = repository.clone();
        enabled.enabled = true;
        let enable = storage::ResourceEdit::Repository {
            id: repository.id.clone(),
            expected: Some(Box::new(repository.clone())),
            value: Some(Box::new(enabled.clone())),
        };
        assert!(validate_repository_save_account(&auth, &generations, &enable, Some(2)).is_err());
        auth.set_failure("22", GithubAuthFailure::Expired);
        assert!(validate_repository_save_account(&auth, &generations, &enable, Some(3)).is_err());
        assert_eq!(store.load_settings().unwrap(), settings);
        auth.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(github::Identity {
                id: "22".into(),
                login: "actor".into(),
            }),
        );
        validate_repository_save_account(&auth, &generations, &enable, Some(3)).unwrap();
        let committed = store.save_resource(enable).unwrap();
        auth.set_failure("22", GithubAuthFailure::Expired);
        // Account failure is operational state, never a user disablement.
        assert!(store.load_settings().unwrap().repositories[0].enabled);
        let disable = storage::ResourceEdit::Repository {
            id: repository.id.clone(),
            expected: Some(Box::new(enabled)),
            value: Some(Box::new(repository)),
        };
        validate_repository_save_account(&auth, &generations, &disable, None).unwrap();
        let disabled = store.save_resource(disable).unwrap();
        assert!(!disabled.repositories[0].enabled);
        assert_eq!(disabled.agents, committed.agents);
        assert!(disabled
            .repository_authorizations
            .values()
            .all(Option::is_none));
    }

    #[test]
    fn resolved_binding_generation_and_connected_identity_are_checked_at_commit() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(github::Identity {
                id: "22".into(),
                login: "actor".into(),
            }),
        );
        let generations = BTreeMap::from([("22".into(), 3)]);
        let repository: storage::Repository = serde_json::from_value(serde_json::json!({
            "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "provider":"github",
            "name":"owner/repo", "enabled":false,
            "provider_account_id":"22", "provider_repository_id":"100"
        }))
        .unwrap();
        let mut edit = storage::ResourceEdit::Repository {
            id: repository.id.clone(),
            expected: None,
            value: Some(Box::new(repository.clone())),
        };
        assert!(validate_repository_save_account(&auth, &generations, &edit, None).is_err());
        assert!(validate_repository_save_account(&auth, &generations, &edit, Some(2)).is_err());
        assert_eq!(
            serde_json::to_value(
                validate_repository_save_account(&auth, &generations, &edit, Some(2)).unwrap_err()
            )
            .unwrap(),
            serde_json::json!("authentication_changed")
        );
        assert_eq!(
            serde_json::to_value(resource_session_error(
                &edit,
                "Account coordination is unavailable."
            ))
            .unwrap(),
            serde_json::json!({"stage":"session","account_id":"22","error":"configuration"})
        );
        assert_eq!(
            serde_json::to_value(ResourceSaveError::from("Resource changed")).unwrap(),
            serde_json::json!("Resource changed")
        );
        assert!(validate_repository_save_account(&auth, &generations, &edit, Some(3)).is_ok());
        for invalid in ["unbound", "enabled", "unsupported"] {
            let mut value = repository.clone();
            match invalid {
                "unbound" => {
                    value.provider_account_id = None;
                    value.provider_repository_id = None;
                }
                "enabled" => value.enabled = true,
                _ => value.provider = storage::ProviderId::AzureDevops,
            }
            let rejected = storage::ResourceEdit::Repository {
                id: value.id.clone(),
                expected: None,
                value: Some(Box::new(value)),
            };
            assert!(
                validate_repository_save_account(&auth, &generations, &rejected, Some(3)).is_err()
            );
        }
        auth.accounts.clear();
        assert!(validate_repository_save_account(&auth, &generations, &edit, Some(3)).is_err());
        if let storage::ResourceEdit::Repository { expected, .. } = &mut edit {
            *expected = Some(Box::new(repository));
        }
        // Pausing an existing binding remains possible while disconnected.
        assert!(validate_repository_save_account(&auth, &generations, &edit, None).is_ok());
    }

    #[test]
    fn late_account_loss_rejects_the_native_commit_before_store_persistence() {
        for failure in [
            GithubAuthFailure::Expired,
            GithubAuthFailure::MissingScope,
            GithubAuthFailure::WrongIdentity,
        ] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::new(root.path().to_path_buf());
            let mut auth = GithubAuth::new();
            for (id, login) in [("22", "fixture_corp"), ("44", "neighbor")] {
                auth.accounts.insert(
                    id.into(),
                    GithubAccountState::Connected(github::Identity {
                        id: id.into(),
                        login: login.into(),
                    }),
                );
            }
            let generations = BTreeMap::from([("22".into(), 3), ("44".into(), 7)]);
            let repository: storage::Repository = serde_json::from_value(serde_json::json!({
                "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "provider": "github",
                "name": "fixture_corp/repository", "enabled": false,
                "provider_account_id": "22", "provider_repository_id": "100"
            }))
            .unwrap();
            let edit = storage::ResourceEdit::Repository {
                id: repository.id.clone(),
                expected: None,
                value: Some(Box::new(repository)),
            };
            assert!(validate_repository_save_account(&auth, &generations, &edit, Some(3)).is_ok());
            auth.set_failure("22", failure);
            let result = validate_repository_save_account(&auth, &generations, &edit, Some(3))
                .and_then(|()| store.save_resource(edit).map_err(ResourceSaveError::from));
            assert_eq!(
                serde_json::to_value(result.err().unwrap()).unwrap(),
                serde_json::json!({
                    "stage": "session", "account_id": "22",
                    "error": match failure {
                        GithubAuthFailure::Expired => "signed_out",
                        GithubAuthFailure::MissingScope => "missing_scope",
                        GithubAuthFailure::WrongIdentity => "wrong_identity",
                        _ => panic!("Unexpected test failure class"),
                    }
                })
            );
            assert!(store.load_settings().unwrap().repositories.is_empty());
            assert!(auth.account_session_allowed("44").is_ok());
            assert_eq!(generations["22"], 3);
            assert_eq!(generations["44"], 7);
        }
    }
}

#[derive(Serialize)]
struct Snapshot {
    settings: Option<Settings>,
    doctrine_catalog: Option<storage::DoctrineCatalog>,
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg(test)]
struct ApplyMonitoringActivation {
    repository_id: String,
    preview_id: String,
    mode: monitoring::ActivationMode,
    selected_pull_request_ids: Vec<String>,
}

#[cfg(test)]
fn stage_monitoring_activation(
    generations: &Mutex<BTreeMap<String, u64>>,
    auth: &Mutex<GithubAuth>,
    store: &Mutex<Store>,
    monitor: &Mutex<monitoring::Monitor>,
    evidence: monitoring::ActivationPreviewEvidence,
) -> Result<monitoring::ActivationPreviewView, ConnectionError> {
    let generations = generations
        .lock()
        .map_err(|_| ConnectionError::Configuration)?;
    let auth = auth.lock().map_err(|_| ConnectionError::Configuration)?;
    auth.account_session_allowed(&evidence.context.account_id)?;
    let store = store.lock().map_err(|_| ConnectionError::Configuration)?;
    let settings = store
        .load_settings()
        .map_err(|_| ConnectionError::Configuration)?;
    let current_generation = generations
        .get(&evidence.context.account_id)
        .copied()
        .unwrap_or(0);
    monitor
        .lock()
        .map_err(|_| ConnectionError::Configuration)?
        .stage_activation_preview(&settings, evidence, current_generation)
}

#[cfg(test)]
fn apply_staged_monitoring_activation(
    generations: &Mutex<BTreeMap<String, u64>>,
    auth: &Mutex<GithubAuth>,
    store: &Mutex<Store>,
    monitor: &Mutex<monitoring::Monitor>,
    request: ApplyMonitoringActivation,
    now: i64,
) -> Result<monitoring::ActivationStatus, String> {
    let preview_context = monitor
        .lock()
        .map_err(|_| "Monitoring is unavailable.")?
        .activation_preview_context(&request.preview_id)
        .ok_or("Monitoring scope preview expired. Preview again.")?;
    if preview_context.repository_id != request.repository_id {
        return Err("Monitoring scope preview belongs to another repository.".into());
    }
    let generations = generations
        .lock()
        .map_err(|_| "Monitoring coordination is unavailable.")?;
    let auth = auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    let store = store.lock().map_err(|_| "Storage is unavailable.")?;
    let settings = store.load_settings()?;
    let context =
        monitoring::Monitor::activation_context(&settings, &preview_context.repository_id)
            .map_err(|_| "Repository monitoring configuration changed. Preview again.")?;
    if context != preview_context {
        return Err("Repository monitoring configuration changed. Preview again.".into());
    }
    auth.account_session_allowed(&context.account_id)
        .map_err(|_| "Reconnect the acting GitHub account, then preview again.")?;
    let generation = generations.get(&context.account_id).copied().unwrap_or(0);
    monitor
        .lock()
        .map_err(|_| "Monitoring is unavailable.")?
        .apply_activation(
            &store,
            &settings,
            monitoring::ActivationApplication {
                repository_id: &request.repository_id,
                preview_id: &request.preview_id,
                mode: request.mode,
                selected_pull_request_ids: &request.selected_pull_request_ids,
                account_generation: generation,
                now,
            },
        )
}

#[tauri::command]
async fn monitoring_snapshot(app: tauri::AppHandle) -> Result<queue::Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        queue_snapshot(&host)
    })
    .await
    .map_err(|_| "Monitoring state could not be read.".to_string())?
}

fn queue_snapshot(host: &Host) -> Result<queue::Snapshot, String> {
    let _generation = host
        .github_generations
        .lock()
        .map_err(|_| "Monitoring coordination is unavailable.")?;
    let accounts = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?
        .monitoring_accounts();
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let mut monitor = host
        .monitor
        .lock()
        .map_err(|_| "Monitoring is unavailable.")?;
    monitor.synchronize_configuration(&store, &accounts, now_seconds()?)?;
    queue::snapshot(&store, monitor.snapshot())
}

#[tauri::command]
async fn result_page(
    app: tauri::AppHandle,
    request: retention::PageRequest,
) -> Result<retention::Page, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| "Result storage unavailable.")?;
        retention::page(&store, request)
    })
    .await
    .map_err(|_| "Result query failed.".to_string())?
}

#[tauri::command]
async fn result_detail(
    app: tauri::AppHandle,
    destination: panel::Detail,
) -> Result<retention::DetailResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| "Result storage unavailable.")?;
        retention::detail(&store, destination)
    })
    .await
    .map_err(|_| "Result detail query failed.".to_string())?
}

fn retention_maintenance(app: &tauri::AppHandle) -> Result<(), String> {
    let host = app.state::<Host>();
    // Existing launchers take these locks before Store. Nonblocking acquisition
    // avoids inversion with a worker finishing its saved outcome.
    macro_rules! idle {
        ($mutex:expr, $busy:expr) => {
            match $mutex.try_lock() {
                Ok(guard) => {
                    if $busy(&*guard) {
                        return Ok(());
                    } else {
                        guard
                    }
                }
                Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
                Err(_) => return Err("Cleanup worker coordination unavailable.".into()),
            }
        };
    }
    let _publications = idle!(host.publications.active, |p: &Option<String>| p.is_some());
    let _follows = idle!(host.follow_ups.active, |p: &Option<(
        String,
        Arc<AtomicBool>
    )>| p.is_some());
    let _actions = idle!(host.actions.active, |active: &bool| *active);
    let store = host
        .store
        .lock()
        .map_err(|_| "Cleanup storage unavailable.")?;
    if host.notifications.active.load(Ordering::SeqCst) || host.quitting.load(Ordering::SeqCst) {
        return Ok(());
    }
    // Applying journals already excluded all related workers before cutover.
    retention::recover(&store)?;
    if host.ai.settle_terminal(&store)? {
        retention::maintain(&store, true)?;
    }
    Ok(())
}

#[tauri::command]
async fn retry_monitoring_operation(
    app: tauri::AppHandle,
    operation_id: String,
) -> Result<(), String> {
    let app_for_retry = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let host = app_for_retry.state::<Host>();
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        let result = host
            .monitor
            .lock()
            .map_err(|_| "Monitoring is unavailable.")?
            .manual_retry_operation(&store, &operation_id, now_seconds()?);
        result
    })
    .await
    .map_err(|_| "The monitoring operation could not be retried.".to_string())??;
    start_checks(&app, false)
}

fn now_seconds() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|_| "System clock precedes the Unix epoch.".into())
}

fn synchronize_monitoring_configuration(
    generations: &Mutex<BTreeMap<String, u64>>,
    auth: &Mutex<GithubAuth>,
    store: &Mutex<Store>,
    monitor: &Mutex<monitoring::Monitor>,
) -> Result<(), String> {
    let _generation = generations
        .lock()
        .map_err(|_| "Monitoring coordination is unavailable.")?;
    let accounts = auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?
        .monitoring_accounts();
    let store = store.lock().map_err(|_| "Storage is unavailable.")?;
    monitor
        .lock()
        .map_err(|_| "Monitoring is unavailable.")?
        .synchronize_configuration(&store, &accounts, now_seconds()?)
}

fn finish_committed_settings(
    generations: &Mutex<BTreeMap<String, u64>>,
    auth: &Mutex<GithubAuth>,
    store: &Mutex<Store>,
    monitor: &Mutex<monitoring::Monitor>,
    settings: Settings,
) -> SavedSettings {
    let monitoring_warning = synchronize_monitoring_configuration(generations, auth, store, monitor).err().map(|_| {
        "Settings saved, but monitoring scope could not be synchronized. New detections remain paused until local monitoring storage is available."
            .to_string()
    });
    let mut saved = match store.lock() {
        Ok(store) => store.finish_settings_save(settings),
        Err(_) => SavedSettings {
            doctrine_catalog: settings.doctrine_catalog(),
            settings,
            warning: Some(
                "Settings saved, but host diagnostics could not be recorded. Check local storage permissions."
                    .into(),
            ),
        },
    };
    saved.warning = match (monitoring_warning, saved.warning) {
        (Some(monitoring), Some(diagnostics)) => Some(format!("{monitoring} {diagnostics}")),
        (Some(monitoring), None) => Some(monitoring),
        (None, warning) => warning,
    };
    saved
}

fn prepare_monitoring_checks(
    host: &Host,
    now: i64,
    immediate: bool,
) -> Result<Vec<monitoring::PollTicket>, String> {
    let generations = host
        .github_generations
        .lock()
        .map_err(|_| "Monitoring coordination is unavailable.")?;
    let accounts = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?
        .monitoring_accounts();
    let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
    let mut monitor = host
        .monitor
        .lock()
        .map_err(|_| "Monitoring is unavailable.")?;
    let mut tickets = monitor.prepare_checks_with_accounts(&store, &accounts, now, immediate)?;
    for ticket in &mut tickets {
        ticket.account_generation = generations
            .get(&ticket.provider_account_id)
            .copied()
            .unwrap_or(0);
    }
    Ok(tickets)
}

fn finish_monitored_ticket(
    generations: &Mutex<BTreeMap<String, u64>>,
    auth: &Mutex<GithubAuth>,
    store: &Mutex<Store>,
    monitor: &Mutex<monitoring::Monitor>,
    ticket: monitoring::PollTicket,
    result: Result<(monitoring::PollResult, follow_up::host::Scan), ConnectionError>,
    now: i64,
) -> Result<(), monitoring::MonitoringError> {
    let generations = generations.lock().map_err(|_| {
        monitoring::MonitoringError::Storage("Monitoring coordination is unavailable.".into())
    })?;
    let auth = auth.lock().map_err(|_| {
        monitoring::MonitoringError::Storage("GitHub connection state is unavailable.".into())
    })?;
    let account_available = auth
        .account_session_allowed(&ticket.provider_account_id)
        .is_ok();
    let accounts = auth.monitoring_accounts();
    drop(auth);
    let current_generation = generations
        .get(&ticket.provider_account_id)
        .copied()
        .unwrap_or(0);
    let store = store
        .lock()
        .map_err(|_| monitoring::MonitoringError::Storage("Storage is unavailable.".into()))?;
    let mut monitor = monitor
        .lock()
        .map_err(|_| monitoring::MonitoringError::Storage("Monitoring is unavailable.".into()))?;
    if current_generation != ticket.account_generation || !account_available {
        monitor.discard_account_result(&store, &accounts, ticket, now)
    } else {
        let (result, observations) = match result {
            Ok((result, observations)) => (Ok(result), observations),
            Err(error) => (Err(error), follow_up::host::Scan::default()),
        };
        monitor.finish_with_admission(&store, &accounts, ticket, result, now, |store, ticket| {
            follow_up::host::admit_scan(store, ticket, observations, now)
        })
    }
}

fn start_checks(app: &tauri::AppHandle, immediate: bool) -> Result<(), String> {
    let host = app.state::<Host>();
    if host.quitting.load(Ordering::SeqCst) {
        return Err("PR Sniper is quitting.".into());
    }
    let tickets = prepare_monitoring_checks(&host, now_seconds()?, immediate)?;
    for ticket in tickets {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let ticket_for_poll = ticket.clone();
            let account_id = ticket.provider_account_id.clone();
            let result = (|| {
                let host = app.state::<Host>();
                let (identity, client) = github_session(&host, &account_id)?;
                if identity.id != ticket_for_poll.provider_account_id {
                    return Err(ConnectionError::WrongIdentity);
                }
                let connection = client.connect(
                    &ticket_for_poll.name,
                    Some(&ticket_for_poll.provider_account_id),
                )?;
                if connection.repository.id != ticket_for_poll.provider_repository_id {
                    return Err(ConnectionError::RepositoryChanged);
                }
                let mut pull_requests = client
                    .poll_tracked_pull_requests(&connection.repository, &ticket_for_poll.tracked)?;
                let follow_ups = follow_up::host::scan(
                    &app,
                    &ticket_for_poll,
                    &mut pull_requests,
                    &connection.identity,
                )?;
                Ok((
                    monitoring::PollResult {
                        connection,
                        pull_requests,
                    },
                    follow_ups,
                ))
            })();
            let connection_failure = result.as_ref().err().copied();
            let host = app.state::<Host>();
            if let Some(error) = connection_failure {
                apply_account_connection_failure(&host, &account_id, &Err::<(), _>(error));
            }
            let saved = match now_seconds() {
                Ok(now) => finish_monitored_ticket(
                    &host.github_generations,
                    &host.github_auth,
                    &host.store,
                    &host.monitor,
                    ticket,
                    result,
                    now,
                ),
                Err(error) => Err(monitoring::MonitoringError::Storage(error)),
            };
            let continue_checks = match &saved {
                Ok(()) => true,
                Err(error) if error.requires_host_report() => {
                    report(&app, error.message().into());
                    false
                }
                Err(error) => {
                    eprintln!("PR Sniper: {}", error.message());
                    true
                }
            };
            if continue_checks && !host.quitting.load(Ordering::SeqCst) {
                if let Err(error) = retention_maintenance(&app) {
                    report(&app, error);
                }
                if let Err(error) = start_checks(&app, false) {
                    report(&app, error);
                }
            }
        });
    }
    Ok(())
}

#[tauri::command]
async fn check_now(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start_checks(&app, true))
        .await
        .map_err(|_| "The immediate repository check could not start.".to_string())?
}

#[tauri::command]
async fn monitoring_activation_status(
    app: tauri::AppHandle,
    repository_id: String,
) -> Result<monitoring::ActivationStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        let settings = store.load_settings()?;
        let monitor = host
            .monitor
            .lock()
            .map_err(|_| "Monitoring is unavailable.")?;
        Ok(monitor.activation_status(&settings, &repository_id))
    })
    .await
    .map_err(|_| "Monitoring scope status could not be read.".to_string())?
}

#[tauri::command]
fn monitoring_setup_review(host: State<'_, Host>) -> Result<monitoring::SetupReview, String> {
    let generations = host
        .github_generations
        .lock()
        .map_err(|_| "Monitoring coordination is unavailable.")?;
    let auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    host.copilot
        .with_setup_accounts(|ai_accounts, ai_generations| {
            let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
            let monitor = host
                .monitor
                .lock()
                .map_err(|_| "Monitoring is unavailable.")?;
            monitor.setup_review(
                &store,
                auth.setup_accounts(),
                ai_accounts,
                &generations,
                &ai_generations,
            )
        })
}

#[derive(Serialize)]
struct GithubRepositories {
    identity: github::Identity,
    owners: Vec<github::provider::RepositoryOwner>,
    repositories: Vec<github::provider::RemoteRepository>,
    warnings: Vec<github::provider::RepositoryBrowseWarning>,
}

#[derive(Serialize)]
struct GithubRepositoryResolution {
    identity: github::Identity,
    repository: github::provider::RemoteRepository,
    account_generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_request: Option<github::metadata::PullRequest>,
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
        doctrine_catalog: settings.as_ref().map(Settings::doctrine_catalog),
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
    let saved = {
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        store.save_preferences(settings, &expected)?
    };
    Ok(finish_committed_settings(
        &host.github_generations,
        &host.github_auth,
        &host.store,
        &host.monitor,
        saved,
    ))
}

#[tauri::command]
fn saved_resources(host: State<'_, Host>) -> Result<storage::SavedResources, String> {
    host.store
        .lock()
        .map_err(|_| "Storage is unavailable.")?
        .saved_resources()
}

#[tauri::command]
fn validate_resource(
    host: State<'_, Host>,
    edit: storage::ResourceEdit,
) -> Result<storage::ResourceReadiness, String> {
    Ok(host
        .store
        .lock()
        .map_err(|_| "Storage is unavailable.")?
        .validate_resource(edit)?
        .readiness())
}

#[tauri::command]
fn save_resource(
    app: tauri::AppHandle,
    host: State<'_, Host>,
    edit: storage::ResourceEdit,
    account_generation: Option<u64>,
) -> Result<SavedSettings, ResourceSaveError> {
    let intake_only = matches!(&edit,
        storage::ResourceEdit::Repository { expected: None, value: Some(repository), .. }
            if !repository.enabled);
    let (saved, dispatched) = {
        let generations = host
            .github_generations
            .lock()
            .map_err(|_| resource_session_error(&edit, "Account coordination is unavailable."))?;
        let auth = host.github_auth.lock().map_err(|_| {
            resource_session_error(&edit, "GitHub connection state is unavailable.")
        })?;
        validate_repository_save_account(&auth, &generations, &edit, account_generation)?;
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        let saved = store.save_resource(edit)?;
        let dispatched =
            (!intake_only).then(|| now_seconds().and_then(|now| host.ai.dispatch(&store, now)));
        (saved, dispatched)
    };
    let mut result = finish_committed_settings(
        &host.github_generations,
        &host.github_auth,
        &host.store,
        &host.monitor,
        saved,
    );
    match dispatched {
        Some(Ok(batch)) => capacity::launch_batch(&app, batch),
        Some(Err(error)) => {
            result.warning = Some(format!(
                "Settings saved; AI coordination requires attention: {error}"
            ))
        }
        None => {}
    }
    Ok(result)
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum ResourceSaveError {
    Repository(RepositoryReadError),
    Message(String),
}

impl From<String> for ResourceSaveError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for ResourceSaveError {
    fn from(message: &str) -> Self {
        Self::Message(message.into())
    }
}

impl From<RepositoryReadError> for ResourceSaveError {
    fn from(error: RepositoryReadError) -> Self {
        Self::Repository(error)
    }
}

fn resource_session_error(edit: &storage::ResourceEdit, message: &str) -> ResourceSaveError {
    if let storage::ResourceEdit::Repository {
        value: Some(repository),
        ..
    } = edit
    {
        if repository.provider == storage::ProviderId::Github {
            if let Some(account_id) = &repository.provider_account_id {
                return RepositoryReadError::session(account_id, ConnectionError::Configuration)
                    .into();
            }
        }
    }
    message.into()
}

fn validate_repository_save_account(
    auth: &GithubAuth,
    generations: &BTreeMap<String, u64>,
    edit: &storage::ResourceEdit,
    resolved_generation: Option<u64>,
) -> Result<(), ResourceSaveError> {
    if let storage::ResourceEdit::Repository {
        expected,
        value: Some(repository),
        ..
    } = edit
    {
        if repository.provider != storage::ProviderId::Github {
            return Err("Azure DevOps repository configuration is coming soon.".into());
        }
        if expected.is_none() && repository.account_binding().is_none() {
            return Err(
                "Choose a connected GitHub account and resolve this repository before adding it."
                    .into(),
            );
        }
        if expected.is_none() && (repository.enabled || !repository.assignments.is_empty()) {
            return Err(
                "Add this repository with monitoring disabled, then save its configuration.".into(),
            );
        }
        let changed_binding = expected.as_ref().is_none_or(|old| {
            old.account_binding() != repository.account_binding() || old.name != repository.name
        });
        if let Some(account_id) = &repository.provider_account_id {
            if repository.enabled || changed_binding || resolved_generation.is_some() {
                auth.account_session_allowed(account_id)
                    .map_err(|error| RepositoryReadError::session(account_id, error))?;
            }
            let current_generation = generations.get(account_id).copied().unwrap_or(0);
            if (changed_binding && resolved_generation.is_none())
                || resolved_generation.is_some_and(|generation| generation != current_generation)
            {
                return Err(RepositoryReadError::Superseded.into());
            }
        }
    }
    Ok(())
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
    owner: String,
) -> Result<GithubRepositories, RepositoryReadError> {
    if !matches!(provider, storage::ProviderId::Github) {
        return Err(ConnectionError::Configuration.into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        repository_read(
            &host.github_generations,
            &host.github_auth,
            &account_id,
            || acquire_github_session(&host, &account_id),
            |identity, client, _| {
                let browser = client.owner_repository_browser(identity, &owner)?;
                Ok(GithubRepositories {
                    identity: identity.clone(),
                    owners: browser.owners,
                    repositories: browser.repositories,
                    warnings: browser.warnings,
                })
            },
        )
    })
    .await
    .map_err(|_| RepositoryReadError::Connection(ConnectionError::ProviderFailure))?
}

#[derive(Serialize)]
struct GithubRepositoryOwners {
    identity: github::Identity,
    owners: Vec<github::provider::RepositoryOwner>,
    warnings: Vec<github::provider::RepositoryBrowseWarning>,
}

#[tauri::command]
async fn list_provider_repository_owners(
    app: tauri::AppHandle,
    provider: storage::ProviderId,
    account_id: String,
) -> Result<GithubRepositoryOwners, RepositoryReadError> {
    if provider != storage::ProviderId::Github {
        return Err(ConnectionError::Configuration.into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        repository_read(
            &host.github_generations,
            &host.github_auth,
            &account_id,
            || acquire_github_session(&host, &account_id),
            |identity, client, _| {
                let browser = client.repository_browser(identity)?;
                Ok(GithubRepositoryOwners {
                    identity: identity.clone(),
                    owners: browser.owners,
                    warnings: browser.warnings,
                })
            },
        )
    })
    .await
    .map_err(|_| RepositoryReadError::Connection(ConnectionError::ProviderFailure))?
}

#[tauri::command]
async fn resolve_provider_repository(
    app: tauri::AppHandle,
    provider: storage::ProviderId,
    account_id: String,
    repository: String,
) -> Result<GithubRepositoryResolution, RepositoryReadError> {
    if !matches!(provider, storage::ProviderId::Github) {
        return Err(ConnectionError::Configuration.into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        repository_read(
            &host.github_generations,
            &host.github_auth,
            &account_id,
            || acquire_github_session(&host, &account_id),
            |identity, client, generation| {
                let target = client.resolve_repository_target(&repository, &identity.id)?;
                Ok(GithubRepositoryResolution {
                    identity: identity.clone(),
                    repository: target.connection.repository,
                    account_generation: generation,
                    pull_request: target.pull_request,
                })
            },
        )
    })
    .await
    .map_err(|_| RepositoryReadError::Connection(ConnectionError::ProviderFailure))?
}

#[tauri::command]
async fn admit_explicit_pull_request(
    app: tauri::AppHandle,
    expected: storage::Repository,
    number: u64,
    account_generation: u64,
) -> Result<monitoring::ExplicitAdmission, ResourceSaveError> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let account_id = expected
            .provider_account_id
            .as_deref()
            .ok_or("Choose a connected GitHub repository account.")?;
        if expected.provider != storage::ProviderId::Github || number == 0 {
            return Err("Use a supported GitHub pull request URL.".into());
        }
        let (resolved, read_generation) = repository_read(
            &host.github_generations,
            &host.github_auth,
            account_id,
            || acquire_github_session(&host, account_id),
            |identity, client, generation| {
                let target = client.resolve_repository_target(
                    &format!("https://github.com/{}/pull/{number}", expected.name),
                    &identity.id,
                )?;
                Ok((target, generation))
            },
        )?;
        let generations = host
            .github_generations
            .lock()
            .map_err(|_| "GitHub account coordination is unavailable.")?;
        let auth = host
            .github_auth
            .lock()
            .map_err(|_| "GitHub connection state is unavailable.")?;
        validate_explicit_intake_generation(
            &auth,
            &generations,
            account_id,
            account_generation,
            read_generation,
        )?;
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        let now = now_seconds()?;
        let admission = host
            .monitor
            .lock()
            .map_err(|_| "Monitoring is unavailable.")?
            .admit_explicit_pull_request(
                &store,
                &auth.monitoring_accounts(),
                &expected,
                resolved,
                now,
            )?;
        let batch = if admission.queued {
            Some(host.ai.dispatch(&store, now)?)
        } else {
            None
        };
        drop(store);
        drop(auth);
        drop(generations);
        if let Some(batch) = batch {
            capacity::launch_batch(&app, batch);
        }
        Ok(admission)
    })
    .await
    .map_err(|_| {
        ResourceSaveError::Message(
            "Pull request intake could not finish. Retry; existing queued work will be reused."
                .into(),
        )
    })?
}

fn validate_explicit_intake_generation(
    auth: &GithubAuth,
    generations: &BTreeMap<String, u64>,
    account_id: &str,
    requested: u64,
    observed: u64,
) -> Result<(), RepositoryReadError> {
    if requested != observed || generations.get(account_id).copied().unwrap_or(0) != requested {
        return Err(RepositoryReadError::Superseded);
    }
    auth.account_session_allowed(account_id)
        .map_err(|error| RepositoryReadError::session(account_id, error))
}

#[cfg(test)]
mod explicit_intake_account_tests {
    use super::*;

    #[test]
    fn queue_commit_requires_the_resolved_connected_actor_generation() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(github::Identity {
                id: "22".into(),
                login: "selected".into(),
            }),
        );
        auth.accounts.insert(
            "44".into(),
            GithubAccountState::Connected(github::Identity {
                id: "44".into(),
                login: "neighbor".into(),
            }),
        );
        let mut generations = BTreeMap::from([("22".into(), 3), ("44".into(), 9)]);
        assert!(validate_explicit_intake_generation(&auth, &generations, "22", 3, 3).is_ok());
        assert_eq!(
            validate_explicit_intake_generation(&auth, &generations, "22", 2, 3),
            Err(RepositoryReadError::Superseded)
        );
        generations.insert("22".into(), 4);
        assert_eq!(
            validate_explicit_intake_generation(&auth, &generations, "22", 3, 3),
            Err(RepositoryReadError::Superseded)
        );
        auth.accounts.remove("22");
        assert!(
            matches!(validate_explicit_intake_generation(&auth, &generations, "22", 4, 4), Err(RepositoryReadError::Session { account_id, .. }) if account_id == "22")
        );
        assert!(validate_explicit_intake_generation(&auth, &generations, "44", 9, 9).is_ok());
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RepositoryReadError {
    Superseded,
    Session {
        account_id: String,
        error: ConnectionError,
    },
    Connection(ConnectionError),
}

impl RepositoryReadError {
    pub fn session(account_id: &str, error: ConnectionError) -> Self {
        Self::Session {
            account_id: account_id.into(),
            error,
        }
    }
    pub fn catalog(account_id: &str, error: ConnectionError) -> Self {
        match error {
            ConnectionError::SignedOut
            | ConnectionError::WrongIdentity
            | ConnectionError::MissingScope
            | ConnectionError::OrganizationPolicyDeniedWithMissingScope => {
                Self::session(account_id, error)
            }
            ConnectionError::RateLimitedWithContext {
                missing_repo_scope: true,
                ..
            } => Self::session(account_id, error),
            _ => Self::Connection(error),
        }
    }
}

impl From<ConnectionError> for RepositoryReadError {
    fn from(error: ConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl Serialize for RepositoryReadError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Superseded => serializer.serialize_str("authentication_changed"),
            Self::Session { account_id, error } => {
                use serde::ser::SerializeStruct;
                let mut fields = serializer.serialize_struct("RepositorySessionFailure", 3)?;
                fields.serialize_field("stage", "session")?;
                fields.serialize_field("account_id", account_id)?;
                fields.serialize_field("error", error)?;
                fields.end()
            }
            Self::Connection(error) => error.serialize(serializer),
        }
    }
}

fn repository_read<T, C>(
    generations: &Mutex<BTreeMap<String, u64>>,
    auth: &Mutex<GithubAuth>,
    account_id: &str,
    acquire: impl FnOnce() -> Result<(github::Identity, C), ConnectionError>,
    read: impl FnOnce(&github::Identity, C, u64) -> Result<T, ConnectionError>,
) -> Result<T, RepositoryReadError> {
    let generation = {
        let generations = generations.lock().map_err(|_| {
            RepositoryReadError::session(account_id, ConnectionError::Configuration)
        })?;
        auth.lock()
            .map_err(|_| RepositoryReadError::session(account_id, ConnectionError::Configuration))?
            .account_session_allowed(account_id)
            .map_err(|error| RepositoryReadError::session(account_id, error))?;
        generations.get(account_id).copied().unwrap_or(0)
    };
    let (identity, result) = match acquire() {
        Ok((identity, client)) => {
            let result = if identity.id == account_id {
                read(&identity, client, generation)
            } else {
                Err(ConnectionError::WrongIdentity)
            };
            (Some(identity), result)
        }
        Err(error) => (None, Err(error)),
    };
    // Validation and publication are one linearization point. Neither an old
    // successful read nor an old auth failure owns a replacement connection.
    let generations = generations
        .lock()
        .map_err(|_| RepositoryReadError::session(account_id, ConnectionError::Configuration))?;
    let mut auth = auth
        .lock()
        .map_err(|_| RepositoryReadError::session(account_id, ConnectionError::Configuration))?;
    if generations.get(account_id).copied().unwrap_or(0) != generation {
        return Err(RepositoryReadError::Superseded);
    }
    let session_failed = identity.is_none();
    if let Some(identity) = identity {
        auth.publish_session_success(account_id, identity)
            .map_err(|error| RepositoryReadError::session(account_id, error))?;
    }
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let failure = if session_failed {
                RepositoryReadError::session(account_id, error)
            } else {
                RepositoryReadError::catalog(account_id, error)
            };
            if matches!(&failure, RepositoryReadError::Session { .. }) {
                if session_failed {
                    auth.publish_acquisition_failure(account_id, error);
                } else {
                    auth.publish_session_failure(account_id, error);
                }
            }
            Err(failure)
        }
    }
}

#[cfg(test)]
mod repository_read_tests {
    use super::*;
    use github::provider::{GithubClient, Response, Transport};
    use serde_json::json;
    use std::sync::mpsc;
    const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

    #[derive(Clone, Copy, Debug)]
    enum Operation {
        Owners,
        Repositories,
        Resolve,
    }

    struct HeldProvider {
        operation: Operation,
        signed_out: bool,
        barrier: Option<(mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>)>,
    }

    impl Transport for HeldProvider {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            let completion = match self.operation {
                Operation::Owners | Operation::Repositories => path.starts_with("/user/repos?"),
                Operation::Resolve => path == "/repos/owner/repo/pulls?state=open&per_page=1",
            };
            if completion {
                if let Some((entered, release)) = &self.barrier {
                    entered.send(()).unwrap();
                    release.lock().unwrap().recv_timeout(WAIT).unwrap();
                }
                if self.signed_out {
                    return Err(ConnectionError::SignedOut);
                }
            }
            let body = match path {
                "/user" => json!({"id":22,"login":"original"}),
                "/repos/owner/repo" => json!({
                    "id":100,"full_name":"owner/repo","private":true,
                    "archived":false,"disabled":false,"permissions":{"pull":true}
                }),
                "/repos/owner/repo/pulls?state=open&per_page=1" => json!([]),
                _ if path.starts_with("/user/repos?") => json!([{
                    "id":100,"full_name":"owner/repo","private":true,
                    "owner":{"login":"owner","type":"Organization"}
                }]),
                _ => panic!("Unexpected provider request: {path}"),
            };
            Ok(Response {
                status: 200,
                headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
                body: serde_json::to_vec(&body).unwrap(),
            })
        }
    }

    fn identity(login: &str) -> github::Identity {
        github::Identity {
            id: "22".into(),
            login: login.into(),
        }
    }

    fn acquire(
        provider: HeldProvider,
    ) -> Result<(github::Identity, GithubClient<HeldProvider>), ConnectionError> {
        let client = GithubClient::new(provider);
        let identity = client.current_identity()?;
        Ok((identity, client))
    }

    fn read_operation(
        operation: Operation,
        identity: &github::Identity,
        client: GithubClient<HeldProvider>,
        generation: u64,
    ) -> Result<serde_json::Value, ConnectionError> {
        let result = match operation {
            Operation::Owners => {
                let browser = client.repository_browser(identity)?;
                serde_json::to_value(GithubRepositoryOwners {
                    owners: browser.owners,
                    warnings: browser.warnings,
                    identity: identity.clone(),
                })
            }
            Operation::Repositories => {
                let browser = client.repository_browser(identity)?;
                serde_json::to_value(GithubRepositories {
                    owners: browser.owners,
                    repositories: browser
                        .repositories
                        .into_iter()
                        .filter(|repo| repo.name.starts_with("owner/"))
                        .collect(),
                    warnings: browser.warnings,
                    identity: identity.clone(),
                })
            }
            Operation::Resolve => serde_json::to_value(GithubRepositoryResolution {
                repository: client.connect("owner/repo", Some(&identity.id))?.repository,
                identity: identity.clone(),
                account_generation: generation,
                pull_request: None,
            }),
        }
        .unwrap();
        Ok(result)
    }

    fn assert_reconnected_completion(operation: Operation) {
        for signed_out in [false, true] {
            for hold_after_provider_validation in [false, true] {
                for disconnect_first in [false, true] {
                    let generations = Arc::new(Mutex::new(BTreeMap::from([("22".into(), 0)])));
                    let mut original = GithubAuth::new();
                    original.accounts.insert(
                        "22".into(),
                        GithubAccountState::Connected(identity("original")),
                    );
                    let auth = Arc::new(Mutex::new(original));
                    let (entered_tx, entered_rx) = mpsc::channel();
                    let (release_tx, release_rx) = mpsc::channel();
                    let worker_generations = generations.clone();
                    let worker_auth = auth.clone();
                    let worker = std::thread::spawn(move || {
                        // The same read/publication boundary used by all three commands,
                        // with real provider parsing and a held response/completion.
                        let (barrier, completion_release) = if hold_after_provider_validation {
                            (None, Some(release_rx))
                        } else {
                            (Some((entered_tx.clone(), Mutex::new(release_rx))), None)
                        };
                        repository_read(
                            &worker_generations,
                            &worker_auth,
                            "22",
                            || {
                                acquire(HeldProvider {
                                    operation,
                                    signed_out,
                                    barrier,
                                })
                            },
                            |identity, client, generation| {
                                let result =
                                    read_operation(operation, identity, client, generation);
                                if let Some(release) = completion_release {
                                    entered_tx.send(()).unwrap();
                                    release.recv_timeout(WAIT).unwrap();
                                }
                                result
                            },
                        )
                    });
                    entered_rx.recv_timeout(WAIT).unwrap();
                    {
                        let mut generations = generations.lock().unwrap();
                        let mut auth = auth.lock().unwrap();
                        if disconnect_first {
                            *generations.get_mut("22").unwrap() += 1;
                            auth.set_failure("22", GithubAuthFailure::Disconnected);
                        }
                        auth.pending = Some(PendingGithubAccount {
                            identity: identity("replacement"),
                            pair: github::oauth::TokenPair::new(
                                "fixture-access",
                                "fixture-refresh",
                                std::time::Duration::from_secs(60),
                                std::time::Duration::from_secs(120),
                            ),
                        });
                        auth.confirm_repository_with(&mut generations, |_, _| Ok(()))
                            .unwrap();
                    }
                    release_tx.send(()).unwrap();
                    assert_eq!(worker.join().unwrap(),Err(RepositoryReadError::Superseded),
                    "{operation:?}, signed_out={signed_out}, completed={hold_after_provider_validation}");
                    assert_eq!(
                        generations.lock().unwrap()["22"],
                        if disconnect_first { 2 } else { 1 }
                    );
                    let current = auth.lock().unwrap();
                    assert!(
                        matches!(&current.accounts["22"],GithubAccountState::Connected(identity) if identity.login == "replacement")
                    );
                    assert!(current.account_session_allowed("22").is_ok());
                    drop(current);
                    assert_eq!(
                        repository_read(
                            &generations,
                            &auth,
                            "22",
                            || Ok((identity("replacement"), ())),
                            |_, (), _| Ok("fresh")
                        ),
                        Ok("fresh")
                    );
                }
            }
        }
    }

    #[test]
    fn owners_completion_cannot_expire_a_replacement_connection() {
        assert_reconnected_completion(Operation::Owners);
    }
    #[test]
    fn repositories_completion_cannot_expire_a_replacement_connection() {
        assert_reconnected_completion(Operation::Repositories);
    }

    #[test]
    fn selected_owner_command_carries_the_full_owner_catalog_from_the_same_read() {
        let (identity, client) = acquire(HeldProvider {
            operation: Operation::Repositories,
            signed_out: false,
            barrier: None,
        })
        .unwrap();
        let result = read_operation(Operation::Repositories, &identity, client, 0).unwrap();
        assert_eq!(result["identity"]["id"], "22");
        assert_eq!(
            result["owners"],
            json!([
                {"login": "original", "kind": "personal"},
                {"login": "owner", "kind": "organization"}
            ])
        );
        assert_eq!(result["repositories"].as_array().unwrap().len(), 1);
        assert_eq!(result["warnings"], json!([]));
    }

    struct CorporateProvider {
        fail_second: bool,
        wrong_identity: bool,
    }

    impl Transport for CorporateProvider {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            let first = "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1";
            let second = first.trim_end_matches('1').to_string() + "2";
            if path == second && self.fail_second {
                return Err(ConnectionError::Network);
            }
            let (body, headers) = if path == "/user" {
                (
                    json!({"id": if self.wrong_identity { 44 } else { 22 }, "login": "fixture_corp"}),
                    BTreeMap::new(),
                )
            } else {
                assert!(
                    !self.wrong_identity,
                    "Wrong identities must not read any catalog"
                );
                assert!(path == first || path == second);
                let owner = if path == first {
                    "fixture_corp"
                } else {
                    "orbit"
                };
                let mut headers = BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]);
                if path == first {
                    headers.insert(
                        "link".into(),
                        format!("<https://api.github.com{second}>; rel=\"next\""),
                    );
                }
                (
                    json!([{
                        "id": if path == first { 101 } else { 102 },
                        "full_name": format!("{owner}/repository"),
                        "private": true,
                        "owner": {"login": owner, "type": if path == first { "User" } else { "Organization" }}
                    }]),
                    headers,
                )
            };
            Ok(Response {
                status: 200,
                headers,
                body: serde_json::to_vec(&body).unwrap(),
            })
        }
    }

    fn corporate_read(
        auth: &Mutex<GithubAuth>,
        generations: &Mutex<BTreeMap<String, u64>>,
        fail_second: bool,
        wrong_identity: bool,
    ) -> Result<serde_json::Value, RepositoryReadError> {
        repository_read(
            generations,
            auth,
            "22",
            || {
                let client = GithubClient::new(CorporateProvider {
                    fail_second,
                    wrong_identity,
                });
                Ok((client.current_identity()?, client))
            },
            |identity, client, _| {
                let browser = client.repository_browser(identity)?;
                Ok(
                    json!({"identity": identity, "owners": browser.owners, "repositories": browser.repositories, "warnings": browser.warnings}),
                )
            },
        )
    }

    #[test]
    fn corporate_browser_partial_read_and_fresh_retry_use_the_verified_selected_account() {
        let mut initial = GithubAuth::new();
        initial.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("fixture_corp")),
        );
        initial.accounts.insert(
            "44".into(),
            GithubAccountState::Connected(github::Identity {
                id: "44".into(),
                login: "neighbor".into(),
            }),
        );
        let auth = Mutex::new(initial);
        let generations = Mutex::new(BTreeMap::from([("22".into(), 7), ("44".into(), 9)]));
        let partial = corporate_read(&auth, &generations, true, false).unwrap();
        assert_eq!(partial["identity"]["id"], "22");
        assert_eq!(partial["repositories"].as_array().unwrap().len(), 1);
        assert_eq!(partial["warnings"][0]["error"], "network");
        assert_eq!(partial["warnings"][0]["page"], 2);
        let fresh = corporate_read(&auth, &generations, false, false).unwrap();
        assert_eq!(fresh["repositories"].as_array().unwrap().len(), 2);
        assert_eq!(fresh["warnings"], json!([]));
        assert_eq!(fresh["owners"][0]["login"], "fixture_corp");
        let auth = auth.lock().unwrap();
        assert!(auth.account_session_allowed("22").is_ok());
        assert!(auth.account_session_allowed("44").is_ok());
        assert_eq!(generations.lock().unwrap()["22"], 7);
        assert_eq!(generations.lock().unwrap()["44"], 9);
    }

    #[test]
    fn corporate_browser_identity_mismatch_stops_before_catalog_without_neighbor_fallback() {
        let mut initial = GithubAuth::new();
        initial.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("fixture_corp")),
        );
        initial.accounts.insert(
            "44".into(),
            GithubAccountState::Connected(github::Identity {
                id: "44".into(),
                login: "neighbor".into(),
            }),
        );
        let auth = Mutex::new(initial);
        let generations = Mutex::new(BTreeMap::new());
        assert_eq!(
            corporate_read(&auth, &generations, false, true),
            Err(RepositoryReadError::session(
                "22",
                ConnectionError::WrongIdentity
            ))
        );
        assert!(auth.lock().unwrap().account_session_allowed("44").is_ok());
    }

    #[test]
    fn session_header_failure_blocks_a_previously_resolved_same_generation_save() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().into());
        let mut initial = GithubAuth::new();
        initial.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("original")),
        );
        initial.accounts.insert(
            "44".into(),
            GithubAccountState::Connected(github::Identity {
                id: "44".into(),
                login: "neighbor".into(),
            }),
        );
        let auth = Mutex::new(initial);
        let generations = Mutex::new(BTreeMap::from([("22".into(), 7), ("44".into(), 9)]));
        let resolved = repository_read(
            &generations,
            &auth,
            "22",
            || {
                acquire(HeldProvider {
                    operation: Operation::Resolve,
                    signed_out: false,
                    barrier: None,
                })
            },
            |identity, client, generation| {
                read_operation(Operation::Resolve, identity, client, generation)
            },
        )
        .unwrap();
        assert_eq!(resolved["account_generation"], 7);
        assert_eq!(resolved["repository"]["id"], "100");
        let failure = repository_read(
            &generations,
            &auth,
            "22",
            || Err::<(github::Identity, ()), _>(ConnectionError::BrokenCli),
            |_, (), _| -> Result<(), ConnectionError> {
                panic!("Header failure cannot verify identity or read catalog")
            },
        )
        .unwrap_err();
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            json!({"stage":"session","account_id":"22","error":"broken_cli"})
        );
        let repository: storage::Repository = serde_json::from_value(json!({
            "id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "provider":"github",
            "name":resolved["repository"]["name"], "enabled":false,
            "provider_account_id":"22", "provider_repository_id":"100"
        }))
        .unwrap();
        let edit = storage::ResourceEdit::Repository {
            id: repository.id.clone(),
            expected: None,
            value: Some(Box::new(repository)),
        };
        let result = validate_repository_save_account(
            &auth.lock().unwrap(),
            &generations.lock().unwrap(),
            &edit,
            Some(7),
        )
        .and_then(|()| store.save_resource(edit).map_err(ResourceSaveError::from));
        assert!(
            result.is_err(),
            "Unusable session reached Store under the previously resolved generation"
        );
        assert_eq!(
            serde_json::to_value(result.unwrap_err()).unwrap(),
            json!({"stage":"session","account_id":"22","error":"configuration"})
        );
        assert!(store.load_settings().unwrap().repositories.is_empty());
        let auth = auth.lock().unwrap();
        assert!(matches!(
            &auth.accounts["22"],
            GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::CredentialsUnavailable,
                ..
            }
        ));
        assert!(auth.account_session_allowed("44").is_ok());
        assert_eq!(generations.lock().unwrap()["22"], 7);
        assert_eq!(generations.lock().unwrap()["44"], 9);
    }

    #[test]
    fn acquisition_failure_classes_preserve_native_retry_and_account_boundaries() {
        for (error, reason, retryable) in [
            (
                ConnectionError::BrokenCli,
                GithubAuthFailure::CredentialsUnavailable,
                false,
            ),
            (
                ConnectionError::Configuration,
                GithubAuthFailure::CredentialsUnavailable,
                false,
            ),
            (
                ConnectionError::SignedOut,
                GithubAuthFailure::Expired,
                false,
            ),
            (
                ConnectionError::WrongIdentity,
                GithubAuthFailure::WrongIdentity,
                false,
            ),
            (
                ConnectionError::MissingScope,
                GithubAuthFailure::MissingScope,
                false,
            ),
            (ConnectionError::Network, GithubAuthFailure::Network, true),
            (ConnectionError::Timeout, GithubAuthFailure::Timeout, true),
            (
                ConnectionError::InvalidResponse,
                GithubAuthFailure::InvalidResponse,
                true,
            ),
            (
                ConnectionError::RateLimited,
                GithubAuthFailure::RateLimited,
                true,
            ),
            (
                ConnectionError::RateLimitedAfter(12),
                GithubAuthFailure::RateLimited,
                true,
            ),
            (
                ConnectionError::ProviderFailure,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::ProviderFailureAfter(12),
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::ProviderRejected,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::MissingReadPermission,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::OrganizationPolicyDenied,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::IncompleteRead,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::MissingCli,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::RevisionChanged,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::InvalidRepository,
                GithubAuthFailure::Provider,
                true,
            ),
            (
                ConnectionError::RepositoryChanged,
                GithubAuthFailure::Provider,
                true,
            ),
        ] {
            let mut initial = GithubAuth::new();
            initial.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("original")),
            );
            initial.accounts.insert(
                "44".into(),
                GithubAccountState::Connected(github::Identity {
                    id: "44".into(),
                    login: "neighbor".into(),
                }),
            );
            let auth = Mutex::new(initial);
            let generations = Mutex::new(BTreeMap::from([("22".into(), 7), ("44".into(), 9)]));
            let result = repository_read(
                &generations,
                &auth,
                "22",
                || Err::<(github::Identity, ()), _>(error),
                |_, (), _| -> Result<(), ConnectionError> {
                    panic!("Failed acquisition cannot read catalog")
                },
            );
            assert_eq!(result, Err(RepositoryReadError::session("22", error)));
            let auth = auth.lock().unwrap();
            assert!(
                matches!(&auth.accounts["22"], GithubAccountState::ReconnectRequired {
                reason: actual, ..
            } if *actual == reason),
                "{error:?}"
            );
            assert_eq!(
                auth.account_session_allowed("22").is_ok(),
                retryable,
                "{error:?}"
            );
            assert!(auth.account_session_allowed("44").is_ok());
            assert_eq!(generations.lock().unwrap()["22"], 7);
            assert_eq!(generations.lock().unwrap()["44"], 9);
        }
        let mut generic = GithubAuth::new();
        generic.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("original")),
        );
        generic.publish_session_failure("22", ConnectionError::BrokenCli);
        assert!(matches!(
            &generic.accounts["22"],
            GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::Provider,
                ..
            }
        ));
        assert!(generic.account_session_allowed("22").is_ok());
    }

    #[test]
    fn url_diagnostic_evidence_preserves_selected_native_session_and_precommit_guards() {
        struct UrlResponse {
            status: u16,
            body: serde_json::Value,
            headers: BTreeMap<String, String>,
        }
        impl Transport for UrlResponse {
            fn get(&self, path: &str) -> Result<Response, ConnectionError> {
                let (status, body, headers) = match path {
                    "/user" => (
                        200,
                        json!({"id":22,"login":"fixture_corp"}),
                        BTreeMap::new(),
                    ),
                    "/repos/owner/repo" => (self.status, self.body.clone(), self.headers.clone()),
                    _ => panic!("Rejected URL lookup cannot reach any additional endpoint: {path}"),
                };
                Ok(Response {
                    status,
                    body: serde_json::to_vec(&body).unwrap(),
                    headers,
                })
            }
        }
        let policy = json!({"message":"Although you appear to have the correct authorization credentials, the fixture-org organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited."});
        let valid = json!({"id":100,"full_name":"owner/repo","private":true,"archived":false,"disabled":false});
        for (status, body, headers, expected, unusable) in [
            (200, valid, vec![], ConnectionError::ScopeUnverified, false),
            (
                403,
                json!({}),
                vec![],
                ConnectionError::MissingReadPermission,
                false,
            ),
            (
                422,
                json!({"message":"Validation failed"}),
                vec![],
                ConnectionError::ProviderRejectedStatus(422),
                false,
            ),
            (
                403,
                policy.clone(),
                vec![("x-oauth-scopes", "repo")],
                ConnectionError::OrganizationPolicyDenied,
                false,
            ),
            (
                403,
                policy,
                vec![("x-oauth-scopes", "read:user")],
                ConnectionError::OrganizationPolicyDeniedWithMissingScope,
                true,
            ),
            (
                403,
                json!({"message":"Although you appear to have the correct authorization credentials, the fixture-org organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited."}),
                vec![("x-oauth-scopes", "read:user"), ("retry-after", "60")],
                ConnectionError::RateLimitedWithContext {
                    retry_after_seconds: Some(60),
                    reset_at: None,
                    organization_access_incomplete: true,
                    missing_repo_scope: true,
                },
                true,
            ),
            (
                403,
                json!({"message":"API rate limit exceeded"}),
                vec![
                    ("x-github-sso", "partial-results; organizations=123"),
                    ("x-ratelimit-remaining", "0"),
                    ("retry-after", "60"),
                ],
                ConnectionError::RateLimitedWithContext {
                    retry_after_seconds: Some(60),
                    reset_at: None,
                    organization_access_incomplete: true,
                    missing_repo_scope: false,
                },
                false,
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::new(root.path().into());
            let mut initial = GithubAuth::new();
            initial.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("fixture_corp")),
            );
            initial.accounts.insert(
                "44".into(),
                GithubAccountState::Connected(github::Identity {
                    id: "44".into(),
                    login: "neighbor".into(),
                }),
            );
            let auth = Mutex::new(initial);
            let generations = Mutex::new(BTreeMap::from([("22".into(), 7), ("44".into(), 9)]));
            let result = repository_read(
                &generations,
                &auth,
                "22",
                || {
                    let client = GithubClient::new(UrlResponse {
                        status,
                        body,
                        headers: headers
                            .into_iter()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
                    });
                    Ok((client.current_identity()?, client))
                },
                |actor, client, _| {
                    client.connect("https://github.com/owner/repo/", Some(&actor.id))
                },
            );
            assert_eq!(
                result.unwrap_err(),
                if unusable {
                    RepositoryReadError::session("22", expected)
                } else {
                    RepositoryReadError::Connection(expected)
                }
            );
            let auth = auth.lock().unwrap();
            assert_eq!(auth.account_session_allowed("22").is_err(), unusable);
            assert!(auth.account_session_allowed("44").is_ok());
            assert_eq!(generations.lock().unwrap()["22"], 7);
            assert!(store.load_settings().unwrap().repositories.is_empty());
            if unusable {
                let repository: storage::Repository = serde_json::from_value(json!({
                    "id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","provider":"github",
                    "name":"owner/repo","enabled":false,"provider_account_id":"22","provider_repository_id":"100"
                })).unwrap();
                let edit = storage::ResourceEdit::Repository {
                    id: repository.id.clone(),
                    expected: None,
                    value: Some(Box::new(repository)),
                };
                assert!(validate_repository_save_account(
                    &auth,
                    &generations.lock().unwrap(),
                    &edit,
                    Some(7)
                )
                .is_err());
            } else {
                assert!(matches!(
                    auth.accounts["22"],
                    GithubAccountState::Connected(_)
                ));
            }
        }
    }

    #[test]
    fn catalog_failures_do_not_turn_a_verified_account_into_reconnect_required() {
        for error in [
            ConnectionError::BrokenCli,
            ConnectionError::Configuration,
            ConnectionError::InvalidResponse,
            ConnectionError::Network,
            ConnectionError::Timeout,
            ConnectionError::RateLimited,
            ConnectionError::ProviderFailure,
            ConnectionError::ProviderRejected,
            ConnectionError::MissingReadPermission,
            ConnectionError::RepositoryUnavailable,
            ConnectionError::ScopeUnverified,
            ConnectionError::OrganizationPolicyDenied,
        ] {
            let mut initial = GithubAuth::new();
            initial.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("fixture_corp")),
            );
            let auth = Mutex::new(initial);
            let generations = Mutex::new(BTreeMap::new());
            assert_eq!(
                repository_read(
                    &generations,
                    &auth,
                    "22",
                    || Ok((identity("fixture_corp"), ())),
                    |_, (), _| Err::<(), _>(error),
                ),
                Err(RepositoryReadError::Connection(error))
            );
            assert!(matches!(
                &auth.lock().unwrap().accounts["22"],
                GithubAccountState::Connected(_)
            ));
            let fresh = corporate_read(&auth, &generations, false, false).unwrap();
            assert_eq!(fresh["repositories"].as_array().unwrap().len(), 2);
        }
    }

    #[test]
    fn url_completion_cannot_expire_a_replacement_connection() {
        assert_reconnected_completion(Operation::Resolve);
    }

    #[test]
    fn repository_intake_saves_the_verified_actor_and_rejects_same_login_replacement() {
        for replace_connection in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::new(root.path().into());
            let mut initial = GithubAuth::new();
            initial.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("original")),
            );
            initial.accounts.insert(
                "44".into(),
                GithubAccountState::Connected(github::Identity {
                    id: "44".into(),
                    login: "neighbor".into(),
                }),
            );
            let auth = Mutex::new(initial);
            let generations = Mutex::new(BTreeMap::from([("22".into(), 7), ("44".into(), 9)]));
            let view = serde_json::to_value(
                auth.lock()
                    .unwrap()
                    .repository_view(&generations.lock().unwrap()),
            )
            .unwrap();
            assert_eq!(view["accounts"][0]["connection_generation"], 7);
            assert_eq!(view["accounts"][0]["provider"], "github");
            assert!(
                serde_json::to_value(auth.lock().unwrap().copilot_view()).unwrap()["accounts"][0]
                    .get("connection_generation")
                    .is_none(),
                "AI connections cannot supply repository generation evidence"
            );
            let resolved = repository_read(
                &generations,
                &auth,
                "22",
                || {
                    acquire(HeldProvider {
                        operation: Operation::Resolve,
                        signed_out: false,
                        barrier: None,
                    })
                },
                |identity, client, generation| {
                    read_operation(Operation::Resolve, identity, client, generation)
                },
            )
            .unwrap();
            assert_eq!(resolved["identity"]["id"], "22");
            let repository: storage::Repository = serde_json::from_value(json!({
                "id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "provider":"github",
                "name":resolved["repository"]["name"], "enabled":false,
                "provider_account_id":"22", "provider_repository_id":resolved["repository"]["id"]
            }))
            .unwrap();
            let edit = storage::ResourceEdit::Repository {
                id: repository.id.clone(),
                expected: None,
                value: Some(Box::new(repository)),
            };
            if replace_connection {
                let mut generations = generations.lock().unwrap();
                let mut auth = auth.lock().unwrap();
                auth.pending = Some(PendingGithubAccount {
                    identity: identity("original"),
                    pair: github::oauth::TokenPair::new(
                        "fixture-access",
                        "fixture-refresh",
                        std::time::Duration::from_secs(60),
                        std::time::Duration::from_secs(120),
                    ),
                });
                auth.confirm_repository_with(&mut generations, |_, _| Ok(()))
                    .unwrap();
                let view = serde_json::to_value(auth.repository_view(&generations)).unwrap();
                assert_eq!(view["accounts"][0]["connection_generation"], 8);
            }
            let result = validate_repository_save_account(
                &auth.lock().unwrap(),
                &generations.lock().unwrap(),
                &edit,
                resolved["account_generation"].as_u64(),
            )
            .and_then(|()| store.save_resource(edit).map_err(ResourceSaveError::from));
            let saved = store.load_settings().unwrap();
            if replace_connection {
                assert_eq!(
                    serde_json::to_value(result.unwrap_err()).unwrap(),
                    json!("authentication_changed")
                );
                assert!(saved.repositories.is_empty());
            } else {
                result.unwrap();
                assert_eq!(
                    saved.repositories[0].provider_account_id.as_deref(),
                    Some("22")
                );
                assert_eq!(
                    saved.repositories[0].provider_repository_id.as_deref(),
                    Some("100")
                );
            }
            assert!(auth.lock().unwrap().account_session_allowed("44").is_ok());
            assert_eq!(generations.lock().unwrap()["44"], 9);
        }
    }

    #[test]
    fn failed_replacement_confirmation_does_not_advance_generation_or_replace_account() {
        let mut generations = BTreeMap::from([("22".into(), 7)]);
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("original")),
        );
        auth.pending = Some(PendingGithubAccount {
            identity: identity("replacement"),
            pair: github::oauth::TokenPair::new(
                "fixture-access",
                "fixture-refresh",
                std::time::Duration::from_secs(60),
                std::time::Duration::from_secs(120),
            ),
        });
        assert!(auth
            .confirm_repository_with(&mut generations, |_, _| Err::<(), _>(()))
            .is_err());
        assert_eq!(generations["22"], 7);
        assert!(
            matches!(&auth.accounts["22"],GithubAccountState::Connected(identity) if identity.login == "original")
        );
    }

    #[test]
    fn session_failure_completion_is_generation_owned() {
        for acquisition_error in [ConnectionError::Configuration, ConnectionError::BrokenCli] {
            for supersede in [false, true] {
                let generations = Arc::new(Mutex::new(BTreeMap::new()));
                let mut original = GithubAuth::new();
                original.accounts.insert(
                    "22".into(),
                    GithubAccountState::Connected(identity("original")),
                );
                let auth = Arc::new(Mutex::new(original));
                let (entered_tx, entered_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                let worker_auth = auth.clone();
                let worker_generations = generations.clone();
                let worker = std::thread::spawn(move || {
                    repository_read(
                        &worker_generations,
                        &worker_auth,
                        "22",
                        || {
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(WAIT).unwrap();
                            Err::<(github::Identity, ()), _>(acquisition_error)
                        },
                        |_, (), _| -> Result<(), ConnectionError> {
                            panic!("Failed sessions cannot read provider data")
                        },
                    )
                });
                entered_rx.recv_timeout(WAIT).unwrap();
                if supersede {
                    let mut generations = generations.lock().unwrap();
                    let mut auth = auth.lock().unwrap();
                    auth.pending = Some(PendingGithubAccount {
                        identity: identity("replacement"),
                        pair: github::oauth::TokenPair::new(
                            "fixture-access",
                            "fixture-refresh",
                            std::time::Duration::from_secs(60),
                            std::time::Duration::from_secs(120),
                        ),
                    });
                    auth.confirm_repository_with(&mut generations, |_, _| Ok(()))
                        .unwrap();
                }
                release_tx.send(()).unwrap();
                assert_eq!(
                    worker.join().unwrap(),
                    Err(if supersede {
                        RepositoryReadError::Superseded
                    } else {
                        RepositoryReadError::session("22", acquisition_error)
                    })
                );
                let auth = auth.lock().unwrap();
                if supersede {
                    assert!(
                        matches!(&auth.accounts["22"],GithubAccountState::Connected(identity) if identity.login == "replacement")
                    );
                } else {
                    assert!(matches!(
                        &auth.accounts["22"],
                        GithubAccountState::ReconnectRequired {
                            reason: GithubAuthFailure::CredentialsUnavailable,
                            ..
                        }
                    ));
                }
            }
        }
    }

    #[test]
    fn actual_credential_load_and_refresh_save_failures_keep_the_session_stage() {
        use github::token_store::{
            AccountRegistry, AccountRegistryStore, CredentialKey, CredentialStore,
            ProviderAccountId, RotationSafeStore, StoreError,
        };
        struct FailingCredentials {
            fail_load: bool,
            pair: github::oauth::TokenPair,
        }
        impl CredentialStore for FailingCredentials {
            fn load(
                &self,
                key: &CredentialKey,
            ) -> Result<Option<github::oauth::TokenPair>, StoreError> {
                assert_eq!(key, &ProviderAccountId::github("22"));
                if self.fail_load {
                    Err(StoreError::Unavailable)
                } else {
                    Ok(Some(self.pair.clone()))
                }
            }
            fn save(
                &self,
                key: &CredentialKey,
                _: &github::oauth::TokenPair,
            ) -> Result<(), StoreError> {
                assert_eq!(key, &ProviderAccountId::github("22"));
                Err(StoreError::Unavailable)
            }
            fn delete(&self, _: &CredentialKey) -> Result<(), StoreError> {
                panic!("Acquisition must not delete credentials")
            }
        }
        impl AccountRegistryStore for FailingCredentials {
            fn load_registry(&self) -> Result<AccountRegistry, StoreError> {
                panic!("Selected acquisition must not use the active registry")
            }
            fn save_registry(&self, _: &AccountRegistry) -> Result<(), StoreError> {
                panic!("Acquisition must not change the registry")
            }
        }
        for fail_load in [true, false] {
            let credentials = RotationSafeStore::new(FailingCredentials {
                fail_load,
                pair: github::oauth::TokenPair::new(
                    "fixture-access",
                    "fixture-refresh",
                    std::time::Duration::ZERO,
                    std::time::Duration::from_secs(120),
                ),
            });
            let mut initial = GithubAuth::new();
            initial.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("original")),
            );
            initial.accounts.insert(
                "44".into(),
                GithubAccountState::Connected(github::Identity {
                    id: "44".into(),
                    login: "neighbor".into(),
                }),
            );
            let auth = Mutex::new(initial);
            let generations = Mutex::new(BTreeMap::new());
            let result = repository_read(
                &generations,
                &auth,
                "22",
                || {
                    credentials
                        .refresh_if_needed(
                            &ProviderAccountId::github("22"),
                            SystemTime::now(),
                            |_| {
                                Ok(github::oauth::TokenPair::new(
                                    "fixture-rotated",
                                    "fixture-refresh",
                                    std::time::Duration::from_secs(60),
                                    std::time::Duration::from_secs(120),
                                ))
                            },
                        )
                        .map_err(rotation_connection_error)?;
                    Ok((identity("original"), ()))
                },
                |_, (), _| -> Result<(), ConnectionError> {
                    panic!("Unavailable credentials cannot read identity or catalog")
                },
            );
            assert_eq!(
                serde_json::to_value(result.unwrap_err()).unwrap(),
                json!({
                    "stage": "session", "account_id": "22", "error": "configuration"
                })
            );
            let auth = auth.lock().unwrap();
            assert!(matches!(
                &auth.accounts["22"],
                GithubAccountState::ReconnectRequired {
                    reason: GithubAuthFailure::CredentialsUnavailable,
                    ..
                }
            ));
            assert!(auth.account_session_allowed("44").is_ok());
            assert!(generations.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn repository_access_denial_does_not_invalidate_a_verified_session() {
        let generations = Mutex::new(BTreeMap::new());
        let mut initial = GithubAuth::new();
        initial.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("original")),
        );
        initial.set_failure("22", GithubAuthFailure::Network);
        let auth = Mutex::new(initial);
        let result = repository_read(
            &generations,
            &auth,
            "22",
            || Ok((identity("verified"), ())),
            |_, (), _| Err::<(), _>(ConnectionError::MissingReadPermission),
        );
        assert_eq!(
            result,
            Err(RepositoryReadError::Connection(
                ConnectionError::MissingReadPermission
            ))
        );
        assert!(
            matches!(&auth.lock().unwrap().accounts["22"],GithubAccountState::Connected(identity) if identity.login == "verified")
        );
    }

    #[test]
    fn current_provider_auth_failures_still_update_only_their_account() {
        for operation in [
            Operation::Owners,
            Operation::Repositories,
            Operation::Resolve,
        ] {
            let generations = Mutex::new(BTreeMap::new());
            let mut initial = GithubAuth::new();
            initial.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("original")),
            );
            initial.accounts.insert(
                "44".into(),
                GithubAccountState::Connected(github::Identity {
                    id: "44".into(),
                    login: "neighbor".into(),
                }),
            );
            let auth = Mutex::new(initial);
            assert_eq!(
                repository_read(
                    &generations,
                    &auth,
                    "22",
                    || acquire(HeldProvider {
                        operation,
                        signed_out: true,
                        barrier: None,
                    }),
                    |identity, client, generation| read_operation(
                        operation, identity, client, generation
                    )
                ),
                Err(RepositoryReadError::session(
                    "22",
                    ConnectionError::SignedOut
                ))
            );
            let auth = auth.lock().unwrap();
            assert!(matches!(
                &auth.accounts["22"],
                GithubAccountState::ReconnectRequired {
                    reason: GithubAuthFailure::Expired,
                    ..
                }
            ));
            assert!(auth.account_session_allowed("44").is_ok());
        }
        assert_eq!(
            serde_json::to_value(RepositoryReadError::Superseded).unwrap(),
            json!("authentication_changed")
        );
        assert_eq!(
            serde_json::to_value(RepositoryReadError::Connection(ConnectionError::SignedOut))
                .unwrap(),
            json!("signed_out")
        );
    }
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
fn save_defaults(
    host: State<'_, Host>,
    policy: serde_json::Value,
) -> Result<SavedSettings, String> {
    let policy = serde_json::from_value(policy).map_err(|_| "Unsupported policy configuration.")?;
    let settings = {
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        store.save_defaults(policy)?
    };
    Ok(finish_committed_settings(
        &host.github_generations,
        &host.github_auth,
        &host.store,
        &host.monitor,
        settings,
    ))
}

#[tauri::command]
fn save_repository_policy(
    host: State<'_, Host>,
    id: String,
    overrides: serde_json::Value,
) -> Result<SavedSettings, String> {
    let overrides = serde_json::from_value(overrides)
        .map_err(|_| "Unsupported policy override configuration.")?;
    let settings = {
        let store = host.store.lock().map_err(|_| "Storage is unavailable.")?;
        store.save_repository_policy(&id, overrides)?
    };
    Ok(finish_committed_settings(
        &host.github_generations,
        &host.github_auth,
        &host.store,
        &host.monitor,
        settings,
    ))
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
    let generations = host
        .github_generations
        .lock()
        .map_err(|_| "GitHub account coordination is unavailable.")?;
    Ok(host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?
        .repository_view(&generations))
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
        ConnectionRole::Repository,
    ));
    Ok(view)
}

async fn complete_github_device_auth(
    app: tauri::AppHandle,
    attempt_id: u64,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    role: ConnectionRole,
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
        let open_browser = |url: &url::Url| {
            if browser_cancelled.load(Ordering::SeqCst)
                || !role
                    .auth(&browser_app.state::<Host>())
                    .lock()
                    .ok()
                    .is_some_and(|auth| auth.is_active_attempt(attempt_id))
            {
                return Err(github::oauth::OAuthError::Cancelled);
            }
            webbrowser::open(url.as_str()).map_err(|_| github::oauth::OAuthError::BrowserOpen)
        };
        let authorization = match role {
            ConnectionRole::Repository => transport.request_device_authorization(open_browser),
            ConnectionRole::Copilot => transport.request_copilot_authorization(open_browser),
        }
        .map_err(failure_from_oauth_error)?;
        if cancelled.load(Ordering::SeqCst) {
            return Err(GithubAuthFailure::Cancelled);
        }
        {
            let host = work_app.state::<Host>();
            let mut auth = role
                .auth(&host)
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
    let Ok(mut auth) = role.auth(&host).lock() else {
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
    let mut generations = host
        .github_generations
        .lock()
        .map_err(|_| "GitHub account coordination is unavailable.")?;
    let mut auth = host
        .github_auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    let persistence = auth.confirm_repository_with(&mut generations, |identity, pair| {
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
                    .as_ref()
                    .map(|store| store.remove_account(&ProviderAccountId::github(&identity.id)))
                    .transpose()
                    .map(|_| ())
                    .map_err(|_| ())
            },
        )
    })?;
    if persistence == OAuthAccountPersistence::SavedWithLegacyCleanupPending {
        eprintln!("GitHub OAuth account connected; legacy credential cleanup remains pending.");
    }
    Ok(auth.repository_view(&generations))
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

fn disconnect_github_account<Current, Legacy>(
    auth: &Mutex<GithubAuth>,
    generations: &Mutex<BTreeMap<String, u64>>,
    store: &Mutex<Store>,
    monitor: &Mutex<monitoring::Monitor>,
    account_id: String,
    remove_current: Current,
    remove_legacy: Legacy,
) -> Result<GithubAuthView, String>
where
    Current: FnOnce() -> Result<(), String>,
    Legacy: FnOnce() -> Result<(), String>,
{
    {
        let mut auth = auth
            .lock()
            .map_err(|_| "GitHub connection state is unavailable.")?;
        if !auth.accounts.contains_key(&account_id) {
            return Err("No connected GitHub account is available to disconnect.".into());
        }
        auth.set_failure(&account_id, GithubAuthFailure::DisconnectPending);
    }
    let mut generations = generations
        .lock()
        .map_err(|_| "Monitoring coordination is unavailable.")?;
    *generations.entry(account_id.clone()).or_default() += 1;
    let monitoring_result = {
        let store = store.lock().map_err(|_| "Storage is unavailable.")?;
        let mut monitor = monitor.lock().map_err(|_| "Monitoring is unavailable.")?;
        monitor.disconnect_account(&store, &account_id)
    };
    if monitoring_result.is_err() {
        if let Ok(mut auth) = auth.lock() {
            auth.set_failure(&account_id, GithubAuthFailure::DisconnectFailed);
        }
        return Err(
            "GitHub disconnect is pending because monitoring state could not be saved. Retry disconnect."
                .into(),
        );
    }
    if let Err(error) = remove_current() {
        if let Ok(mut auth) = auth.lock() {
            auth.set_failure(&account_id, GithubAuthFailure::DisconnectFailed);
        }
        return Err(error);
    }
    if let Err(error) = remove_legacy() {
        if let Ok(mut auth) = auth.lock() {
            auth.set_failure(&account_id, GithubAuthFailure::DisconnectFailed);
        }
        return Err(error);
    }
    drop(generations);
    let mut auth = auth
        .lock()
        .map_err(|_| "GitHub connection state is unavailable.")?;
    auth.accounts.remove(&account_id);
    Ok(auth.view())
}

#[tauri::command]
async fn disconnect_github_auth(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<GithubAuthView, String> {
    use github::token_store::ProviderAccountId;
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let current_key = ProviderAccountId::github(&account_id);
        let legacy_key = ProviderAccountId::github(&account_id);
        disconnect_github_account(
            &host.github_auth,
            &host.github_generations,
            &host.store,
            &host.monitor,
            account_id,
            || {
                host.github_credentials
                    .remove_account(&current_key)
                    .map_err(|_| {
                        "GitHub credentials could not be deleted securely. Retry disconnect."
                            .into()
                    })
            },
            || {
                host.github_legacy_credentials
                    .as_ref()
                    .map(|store| store.remove_account(&legacy_key))
                    .transpose()
                    .map(|_| ())
                    .map_err(|_| {
                        "Superseded GitHub credentials could not be deleted securely. Retry disconnect."
                            .into()
                    })
            },
        )
    })
    .await
    .map_err(|_| "GitHub disconnect could not finish. Retry disconnect.".to_string())?
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
async fn open_diagnostics(app: tauri::AppHandle) -> Result<(), String> {
    panel::show(&app, Some(panel::Route::utility(true)))
        .await
        .map(|_| ())
}

#[tauri::command]
async fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    panel::show(&app, Some(panel::Route::tab(panel::Tab::Settings)))
        .await
        .map(|_| ())
}

#[tauri::command]
async fn open_queue_destination(
    app: tauri::AppHandle,
    item_id: String,
    file: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = {
            let host = app.state::<Host>();
            let store = host
                .store
                .lock()
                .map_err(|_| "Queue storage is unavailable.")?;
            queue::destination(&store, &item_id, file.as_deref())?
        };
        webbrowser::open(url.as_str())
            .map_err(|_| "Could not open this GitHub destination. Try again.".into())
    })
    .await
    .map_err(|_| "Could not open the queue destination.".to_string())?
}

#[tauri::command]
async fn open_queue_item(app: tauri::AppHandle, item_id: String) -> Result<(), String> {
    match panel::show(&app, Some(panel::Route::item(item_id)))
        .await?
        .missing
    {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

#[tauri::command]
fn queue_selection(host: State<'_, Host>) -> Result<Option<String>, String> {
    host.store
        .lock()
        .map_err(|_| "Queue storage is unavailable.")?
        .load_queue_selection()
}

#[tauri::command]
fn select_queue_item(host: State<'_, Host>, item_id: Option<String>) -> Result<(), String> {
    let store = host
        .store
        .lock()
        .map_err(|_| "Queue storage is unavailable.")?;
    queue::select(&store, item_id.as_deref())
}

fn github_keychain_stores(
    isolated: bool,
    override_service: Option<std::ffi::OsString>,
) -> Result<
    (
        github::NativeCredentialStore,
        Option<github::NativeCredentialStore>,
    ),
    std::io::Error,
> {
    match (isolated, override_service) {
        (false, None) => Ok((github::NativeCredentialStore::production(), {
            #[cfg(target_os = "macos")]
            {
                Some(github::NativeCredentialStore::legacy_production())
            }
            #[cfg(windows)]
            {
                None
            }
        })),
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
            Ok((github::NativeCredentialStore::with_service(service), {
                #[cfg(target_os = "macos")]
                {
                    Some(github::NativeCredentialStore::with_service(format!(
                        "{service}.legacy"
                    )))
                }
                #[cfg(windows)]
                {
                    None
                }
            }))
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
    #[cfg(windows)]
    match notifications::windows::preflight() {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            notifications::windows::show_error(&error);
            return;
        }
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _| {
            #[cfg(windows)]
            notifications::windows::forward(_app, &_args);
        }))
        .invoke_handler(tauri::generate_handler![
            result_page,
            result_detail,
            snapshot,
            save_preferences,
            saved_resources,
            validate_resource,
            save_resource,
            capacity::set_automation_paused,
            capacity::automation_snapshot,
            actions::host::start_final_review,
            actions::host::cancel_final_review,
            actions::host::cancel_provider_action,
            actions::host::reconcile_provider_action,
            actions::host::retry_action_observation,
            canonical_repository_name,
            resolve_provider_person,
            list_provider_repositories,
            list_provider_repository_owners,
            resolve_provider_repository,
            admit_explicit_pull_request,
            save_login,
            save_defaults,
            save_repository_policy,
            verify_provider_connection,
            read_provider_metadata,
            github_auth_state,
            start_github_browser_auth,
            confirm_github_account,
            cancel_github_auth,
            disconnect_github_auth,
            copilot::copilot_auth_state,
            copilot::start_copilot_auth,
            copilot::confirm_copilot_account,
            copilot::cancel_copilot_auth,
            copilot::disconnect_copilot_account,
            copilot::verify_copilot_account,
            copilot::list_copilot_models,
            copilot::cancel_copilot_models,
            diagnostics,
            open_diagnostics,
            open_settings,
            open_queue_destination,
            open_queue_item,
            panel::panel_snapshot,
            panel::panel_navigate,
            panel::hide_panel,
            queue_selection,
            select_queue_item,
            notifications::host::notification_snapshot,
            notifications::host::set_notifications_enabled,
            notifications::host::test_notification,
            notifications::host::open_notification,
            monitoring_snapshot,
            retry_monitoring_operation,
            review::host::start_review,
            review::host::cancel_review,
            publication::host::publish_review,
            publication::host::cancel_publication,
            follow_up::host::start_follow_up,
            follow_up::host::cancel_follow_up,
            check_now,
            monitoring_activation_status,
            monitoring_setup_review,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let override_root =
                std::env::var_os("PR_SNIPER_DATA_DIR").map(std::path::PathBuf::from);
            let isolated = override_root.is_some();
            let root = match override_root {
                Some(root) if root.is_absolute() => root,
                Some(_) => return Err("PR_SNIPER_DATA_DIR must be an absolute path.".into()),
                None => {
                    #[cfg(windows)]
                    {
                        app.path().app_local_data_dir()?
                    }
                    #[cfg(not(windows))]
                    {
                        app.path().app_data_dir()?
                    }
                }
            };
            let (github_keychain, legacy_github_keychain) =
                github_keychain_stores(isolated, std::env::var_os("PR_SNIPER_KEYCHAIN_SERVICE"))?;
            let github_credentials = github::token_store::RotationSafeStore::new(github_keychain);
            let github_legacy_credentials =
                legacy_github_keychain.map(github::token_store::RotationSafeStore::new);
            let github_auth =
                GithubAuth::restore(&github_credentials, github_legacy_credentials.as_ref());
            let copilot = copilot::Integration::new(isolated)?;
            let store = Store::new(root.clone());
            retention::recover(&store).map_err(std::io::Error::other)?;
            review::restore(&store).map_err(std::io::Error::other)?;
            publication::restore(&store).map_err(std::io::Error::other)?;
            follow_up::restore(&store).map_err(std::io::Error::other)?;
            actions::restore(&store).map_err(std::io::Error::other)?;
            let monitor = monitoring::Monitor::restore(&store).map_err(std::io::Error::other)?;
            let notification_restore = notifications::restore(&store);
            let retention_restore = if notification_restore.is_ok() {
                retention::maintain(&store, true).map(|_| ())
            } else {
                Ok(())
            };
            #[cfg(target_os = "macos")]
            let executable = std::env::current_exe()?.canonicalize()?;
            #[cfg(windows)]
            let executable = std::env::current_exe()?;
            let notifications = notifications::host::Coordinator::new(app.handle(), &store, &root);
            app.manage(Host {
                panel: panel::Panel::default(),
                store: Mutex::new(store),
                monitor: Mutex::new(monitor),
                error: Mutex::new(None),
                isolated,
                quitting: AtomicBool::new(false),
                registration: LoginRegistration::for_app(app.path().home_dir()?, executable),
                github_auth: Mutex::new(github_auth),
                github_generations: Mutex::new(BTreeMap::new()),
                github_credentials,
                github_legacy_credentials,
                copilot,
                ai: capacity::Coordinator::default(),
                publications: publication::host::Coordinator::default(),
                follow_ups: follow_up::host::Coordinator::default(),
                actions: actions::host::Coordinator::default(),
                mutations: actions::host::MutationOwner::default(),
                notifications,
            });
            #[cfg(windows)]
            notifications::windows::ready(app.handle());
            if let Err(error) = notification_restore {
                report(app.handle(), error);
            }
            if let Err(error) = retention_restore {
                report(app.handle(), error);
            }
            record(app.handle(), DiagnosticEvent::SessionStarted);
            let scheduler_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    let host = scheduler_app.state::<Host>();
                    if host.quitting.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Err(error) = start_checks(&scheduler_app, false) {
                        report(&scheduler_app, error);
                    }
                    if let Err(error) = retention_maintenance(&scheduler_app) {
                        report(&scheduler_app, error);
                    }
                    if let Err(error) = capacity::Coordinator::pump(&scheduler_app) {
                        report(&scheduler_app, error);
                    }
                    if let Err(error) = publication::host::Coordinator::pump(&scheduler_app) {
                        report(&scheduler_app, error);
                    }
                    if let Err(error) = follow_up::host::Coordinator::pump(&scheduler_app) {
                        report(&scheduler_app, error);
                    }
                    if let Err(error) = actions::host::Coordinator::pump(&scheduler_app) {
                        report(&scheduler_app, error);
                    }
                    notifications::host::Coordinator::pump(&scheduler_app);
                }
            });
            let status = MenuItem::with_id(app, "status", "Status", true, None::<&str>)?;
            let queue = MenuItem::with_id(app, "queue", "Review Queue", true, None::<&str>)?;
            let check = MenuItem::with_id(app, "check", "Check Now", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let diagnostics =
                MenuItem::with_id(app, "diagnostics", "Diagnostics", true, None::<&str>)?;
            let close_panel =
                MenuItem::with_id(app, "close-panel", "Close Panel", true, None::<&str>)?;
            let recovery = MenuItem::with_id(
                app,
                "retry-panel",
                panel::RECOVERY_LABEL,
                true,
                None::<&str>,
            )?;
            app.state::<Host>()
                .panel
                .recovery
                .set(recovery.clone())
                .map_err(|_| "Panel recovery menu was already initialized.")?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit PR Sniper", true, Some("CmdOrCtrl+Q"))?;
            let menu = Menu::with_items(
                app,
                &[
                    &status,
                    &queue,
                    &check,
                    &settings,
                    &diagnostics,
                    &recovery,
                    &close_panel,
                    &separator,
                    &quit,
                ],
            )?;
            TrayIconBuilder::with_id("pr-sniper")
                .icon(tauri::image::Image::from_bytes(if cfg!(windows) {
                    include_bytes!("../icons/icon.png")
                } else {
                    include_bytes!("../icons/tray.png")
                })?)
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("PR Sniper")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state,
                        ..
                    } = event
                    {
                        if let Err(error) =
                            panel::tray(tray.app_handle(), button_state == MouseButtonState::Down)
                        {
                            report(tray.app_handle(), error);
                        }
                    }
                })
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "retry-panel" {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) = panel::show(&app, None).await {
                                report(&app, error);
                            }
                        });
                        return;
                    }
                    if event.id.as_ref() == "close-panel" {
                        if let Some(window) = app.get_webview_window(panel::LABEL) {
                            if window.close().is_err() {
                                report(app, "Cannot request native panel close.".into());
                            }
                        }
                        return;
                    }
                    if event.id.as_ref() == "check" {
                        let check_app = app.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            if let Err(error) = start_checks(&check_app, true) {
                                report(&check_app, error);
                            }
                        });
                        return;
                    }
                    if event.id.as_ref() == "quit" {
                        let host = app.state::<Host>();
                        if host.quitting.swap(true, Ordering::SeqCst) {
                            return;
                        }
                        record(app, DiagnosticEvent::QuitRequested);
                        host.copilot.request_shutdown();
                        if let Ok(store) = host.store.lock() {
                            if let Err(error) = host.ai.shutdown(&store) {
                                report(app, error);
                            }
                        }
                        host.follow_ups.cancel_all();
                        let shutdown_app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let cancel_app = shutdown_app.clone();
                            if tauri::async_runtime::spawn_blocking(move || {
                                let host = cancel_app.state::<Host>();
                                if let Ok(mut auth) = host.github_auth.lock() {
                                    auth.cancel_attempt();
                                }
                                if let Ok(store) = host.store.lock() {
                                    if let Ok(mut monitor) = host.monitor.lock() {
                                        if monitor.cancel_pending_checks(&store).is_err() {
                                            eprintln!(
                                                "Pending monitoring checks could not be cancelled."
                                            );
                                        }
                                    }
                                }
                                host.copilot.shutdown();
                            })
                            .await
                            .is_err()
                            {
                                eprintln!("Authentication shutdown could not finish.");
                            }
                            let deadline =
                                tokio::time::Instant::now() + std::time::Duration::from_secs(7);
                            while (!shutdown_app.state::<Host>().copilot.lookups_finished()
                                || !shutdown_app.state::<Host>().ai.finished()
                                || !shutdown_app.state::<Host>().publications.finished()
                                || !shutdown_app.state::<Host>().follow_ups.finished()
                                || !shutdown_app.state::<Host>().actions.finished())
                                && tokio::time::Instant::now() < deadline
                            {
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                            shutdown_app.exit(0);
                        });
                        return;
                    }

                    let target = match event.id.as_ref() {
                        "status" => panel::Route::utility(false),
                        "diagnostics" => panel::Route::utility(true),
                        "queue" => panel::Route::tab(panel::Tab::Queue),
                        "settings" => panel::Route::tab(panel::Tab::Settings),
                        _ => return,
                    };
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = panel::show(&app, Some(target)).await {
                            report(&app, error);
                        }
                    });
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != panel::LABEL {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                record(window.app_handle(), DiagnosticEvent::WindowCloseRequested);
                if let Err(error) = panel::hide(window.app_handle()) {
                    report(window.app_handle(), error);
                }
            } else if let tauri::WindowEvent::Focused(false) = event {
                panel::lost_focus(window.app_handle());
            } else if matches!(
                event,
                tauri::WindowEvent::Resized(_)
                    | tauri::WindowEvent::ScaleFactorChanged { .. }
                    | tauri::WindowEvent::ThemeChanged(_)
            ) {
                if let Err(error) = panel::refresh_surface(window) {
                    report(window.app_handle(), error.to_string());
                }
            }
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|_| {
            eprintln!("PR Sniper could not start its native host.");
            std::process::exit(1);
        });
    app.run(|app, event| {
        #[cfg(windows)]
        if let tauri::RunEvent::Exit = event {
            notifications::windows::shutdown(app);
        }
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
        apply_staged_monitoring_activation, disconnect_github_account,
        failure_from_connection_error, failure_from_oauth_error, finish_committed_settings,
        finish_monitored_ticket, github_keychain_stores, persist_oauth_account_with_cleanup,
        rotation_connection_error, stage_monitoring_activation, ApplyMonitoringActivation,
        GithubAccountState, GithubAccountView, GithubAuth, GithubAuthFailure, GithubFlowView,
        OAuthAccountPersistence,
    };
    use crate::github::token_store::{
        AccountRegistry, AccountRegistryStore, CredentialKey, CredentialStore, RotationError,
        StoreError,
    };
    use crate::github::ConnectionError;
    use crate::github::{
        metadata::{Lifecycle, PullRequest},
        oauth::OAuthError,
        provider::{Capabilities, CommentCapability, Connection, RemoteRepository},
    };
    use crate::monitoring::{
        ActivationMode, ActivationPreviewEvidence, Monitor, PollResult,
        WAITING_ACCOUNT_DISCONNECTED, WAITING_AI_CAPACITY,
    };
    use crate::policy::{PolicyOverrides, WatchedIdentity};
    use crate::storage::{ProviderId as SettingsProviderId, Repository, Settings, Store};
    use std::{
        collections::BTreeMap,
        fs,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Barrier, Mutex,
        },
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
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

    fn monitoring_fixture() -> (tempfile::TempDir, Store, Monitor, GithubAuth) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().to_path_buf());
        let mut settings = Settings::default();
        settings.repositories.push(Repository {
            id: "00000000-0000-4000-8000-000000000001".into(),
            provider: SettingsProviderId::Github,
            name: "example/repo".into(),
            enabled: true,
            provider_account_id: Some("22".into()),
            legacy_installation_id: None,
            provider_repository_id: Some("100".into()),
            overrides: PolicyOverrides::default(),
            review_preset: None,
            watched_authors: vec![WatchedIdentity {
                id: "11".into(),
                login: "author".into(),
            }],
            assignments: Vec::new(),
            primary_assignment_id: None,
        });
        settings.agents.push(
            serde_json::from_value(serde_json::json!({
                "id":"eeeeeeee-eeee-4eee-8eee-eeeeeeeeeee1","name":"Polling fixture",
                "model":"fixture-model","ai_account":{"provider":"copilot","account_id":"33"},
                "prompt":"Review safely.","signature":"fixture"
            }))
            .unwrap(),
        );
        settings.repositories[0].assignments.push(
            serde_json::from_value(serde_json::json!({
                "id":"eeeeeeee-eeee-4eee-8eee-eeeeeeeeeee2","agent_id":settings.agents[0].id,
                "schedule":{"kind":"interval","minutes":15,"timezone":"UTC"},"comment":false
            }))
            .unwrap(),
        );
        store.save_settings(&settings).unwrap();
        let context = Monitor::activation_context(&settings, &settings.repositories[0].id).unwrap();
        let mut state = store.load_monitoring_state().unwrap();
        state.activations.insert(
            context.repository_id.clone(),
            crate::monitoring::MonitoringActivation {
                version: "00000000-0000-4000-8000-000000000099".into(),
                repository_id: context.repository_id,
                name: context.name,
                account_id: context.account_id,
                provider_repository_id: context.provider_repository_id,
                trigger_policy: context.trigger_policy,
                creation_watermark: 0,
                mode: ActivationMode::NewOnly,
                selected_existing: 0,
                baseline: BTreeMap::new(),
                confirmed_at: 1_799_999_999,
            },
        );
        store.save_monitoring_state(&state).unwrap();
        let monitor = Monitor::restore(&store).unwrap();
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("22", "current-login")),
        );
        (root, store, monitor, auth)
    }

    fn monitoring_result() -> PollResult {
        PollResult {
            connection: Connection {
                identity: identity("22", "current-login"),
                repository: RemoteRepository {
                    id: "100".into(),
                    name: "example/repo".into(),
                },
                capabilities: Capabilities {
                    read: true,
                    comment: CommentCapability::Available,
                },
            },
            pull_requests: vec![PullRequest {
                mentioned: false,
                id: "1".into(),
                number: 1,
                title: "PR 1".into(),
                author: Some(identity("11", "author")),
                requested_reviewers: Vec::new(),
                requested_teams: Vec::new(),
                state: Lifecycle::Open,
                draft: false,
                head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                base_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                head_repository_id: Some("100".into()),
                base_repository_id: "100".into(),
                updated_at: "2026-09-25T10:00:00Z".into(),
                files: Vec::new(),
            }],
        }
    }

    #[test]
    fn disconnect_between_provider_result_and_commit_cannot_resurrect_work_or_cursor() {
        let (_root, store, mut monitor, auth) = monitoring_fixture();
        let accounts = auth.monitoring_accounts();
        let mut tickets = monitor
            .prepare_checks_with_accounts(&store, &accounts, 1_800_000_000, true)
            .unwrap();
        let ticket = tickets.remove(0);
        let generations = Arc::new(Mutex::new(BTreeMap::new()));
        let auth = Arc::new(Mutex::new(auth));
        let store = Arc::new(Mutex::new(store));
        let monitor = Arc::new(Mutex::new(monitor));
        let result_ready = Arc::new(Barrier::new(2));
        let resume_commit = Arc::new(Barrier::new(2));
        let worker = {
            let generations = generations.clone();
            let auth = auth.clone();
            let store = store.clone();
            let monitor = monitor.clone();
            let result_ready = result_ready.clone();
            let resume_commit = resume_commit.clone();
            thread::spawn(move || {
                let result = monitoring_result();
                result_ready.wait();
                resume_commit.wait();
                finish_monitored_ticket(
                    &generations,
                    &auth,
                    &store,
                    &monitor,
                    ticket,
                    Ok((result, crate::follow_up::host::Scan::default())),
                    1_800_000_002,
                )
            })
        };

        result_ready.wait();
        disconnect_github_account(
            &auth,
            &generations,
            &store,
            &monitor,
            "22".into(),
            || Ok(()),
            || Ok(()),
        )
        .unwrap();
        resume_commit.wait();
        worker.join().unwrap().unwrap();

        let store_guard = store.lock().unwrap();
        assert!(store_guard.load_queue().unwrap().is_empty());
        assert!(store_guard
            .load_monitoring_state()
            .unwrap()
            .cursors
            .is_empty());
        drop(store_guard);
        let restored = {
            let store = store.lock().unwrap();
            Monitor::restore(&store).unwrap()
        };
        let health = restored.snapshot().remove(0);
        assert!(!health.in_flight);
        assert!(!health.manual_pending);
        assert_eq!(
            health.last_failure.as_deref(),
            Some(WAITING_ACCOUNT_DISCONNECTED)
        );
        assert_eq!(health.next_run, 0);
    }

    #[test]
    fn activation_native_boundary_rejects_generation_and_disconnect_races() {
        let (_root, store, monitor, auth) = monitoring_fixture();
        let settings = store.load_settings().unwrap();
        let context = Monitor::activation_context(&settings, &settings.repositories[0].id).unwrap();
        let generations = Mutex::new(BTreeMap::new());
        let auth = Mutex::new(auth);
        let store = Mutex::new(store);
        let monitor = Mutex::new(monitor);
        let preview = stage_monitoring_activation(
            &generations,
            &auth,
            &store,
            &monitor,
            ActivationPreviewEvidence {
                context: context.clone(),
                connection: monitoring_result().connection,
                pull_requests: monitoring_result().pull_requests,
                creation_watermark: 1,
                account_generation: 0,
            },
        )
        .unwrap();
        *generations.lock().unwrap().entry("22".into()).or_default() += 1;
        assert!(apply_staged_monitoring_activation(
            &generations,
            &auth,
            &store,
            &monitor,
            ApplyMonitoringActivation {
                repository_id: context.repository_id.clone(),
                preview_id: preview.preview_id,
                mode: ActivationMode::NewOnly,
                selected_pull_request_ids: Vec::new(),
            },
            1_800_000_000,
        )
        .is_err());

        let preview = stage_monitoring_activation(
            &generations,
            &auth,
            &store,
            &monitor,
            ActivationPreviewEvidence {
                context: context.clone(),
                connection: monitoring_result().connection,
                pull_requests: Vec::new(),
                creation_watermark: 1,
                account_generation: 1,
            },
        )
        .unwrap();
        auth.lock()
            .unwrap()
            .set_failure("22", GithubAuthFailure::Disconnected);
        assert!(apply_staged_monitoring_activation(
            &generations,
            &auth,
            &store,
            &monitor,
            ApplyMonitoringActivation {
                repository_id: context.repository_id,
                preview_id: preview.preview_id,
                mode: ActivationMode::NewOnly,
                selected_pull_request_ids: Vec::new(),
            },
            1_800_000_001,
        )
        .is_err());
    }

    #[test]
    fn activation_request_cannot_apply_another_repository_preview() {
        let (_root, store, _monitor, mut auth) = monitoring_fixture();
        let mut state = store.load_monitoring_state().unwrap();
        state.activations.clear();
        store.save_monitoring_state(&state).unwrap();
        let mut settings = store.load_settings().unwrap();
        let mut second = settings.repositories[0].clone();
        second.id = "00000000-0000-4000-8000-000000000002".into();
        second.name = "example/other".into();
        second.provider_account_id = Some("23".into());
        second.provider_repository_id = Some("200".into());
        settings.repositories.push(second);
        store.save_settings(&settings).unwrap();
        auth.accounts.insert(
            "23".into(),
            GithubAccountState::Connected(identity("23", "other-account")),
        );
        let context =
            Monitor::activation_context(&settings, "00000000-0000-4000-8000-000000000001").unwrap();
        let generations = Mutex::new(BTreeMap::new());
        let auth = Mutex::new(auth);
        let store = Mutex::new(store);
        let monitor = Mutex::new(Monitor::restore(&store.lock().unwrap()).unwrap());
        let preview = stage_monitoring_activation(
            &generations,
            &auth,
            &store,
            &monitor,
            ActivationPreviewEvidence {
                context,
                connection: monitoring_result().connection,
                pull_requests: Vec::new(),
                creation_watermark: 1,
                account_generation: 0,
            },
        )
        .unwrap();
        auth.lock()
            .unwrap()
            .set_failure("22", GithubAuthFailure::Disconnected);
        assert!(apply_staged_monitoring_activation(
            &generations,
            &auth,
            &store,
            &monitor,
            ApplyMonitoringActivation {
                repository_id: "00000000-0000-4000-8000-000000000002".into(),
                preview_id: preview.preview_id,
                mode: ActivationMode::NewOnly,
                selected_pull_request_ids: Vec::new(),
            },
            1_800_000_000,
        )
        .is_err());
        assert!(store
            .lock()
            .unwrap()
            .load_monitoring_state()
            .unwrap()
            .activations
            .is_empty());
    }

    #[test]
    fn committed_settings_sync_failure_returns_saved_state_with_warning() {
        let (root, store, monitor, auth) = monitoring_fixture();
        let mut settings = store.load_settings().unwrap();
        settings.repositories[0].enabled = false;
        store.save_settings(&settings).unwrap();
        fs::create_dir(root.path().join("state/monitoring.json.tmp")).unwrap();
        fs::create_dir(root.path().join("state/diagnostics.jsonl")).unwrap();
        let store = Mutex::new(store);
        let saved = finish_committed_settings(
            &Mutex::new(BTreeMap::new()),
            &Mutex::new(auth),
            &store,
            &Mutex::new(monitor),
            settings.clone(),
        );
        assert_eq!(saved.settings, settings);
        let warning = saved.warning.unwrap();
        assert!(warning.contains("monitoring scope"));
        assert!(warning.contains("diagnostics"));
        assert_eq!(store.lock().unwrap().load_settings().unwrap(), settings);
    }

    #[test]
    fn disconnect_queue_failure_is_visible_and_retryable_before_credential_cleanup() {
        let (root, store, mut monitor, auth) = monitoring_fixture();
        let accounts = auth.monitoring_accounts();
        let mut tickets = monitor
            .prepare_checks_with_accounts(&store, &accounts, 1_800_000_000, true)
            .unwrap();
        monitor
            .finish_with_accounts(
                &store,
                &accounts,
                tickets.remove(0),
                Ok(monitoring_result()),
                1_800_000_001,
            )
            .unwrap();
        fs::create_dir(root.path().join("state/queue.json.tmp")).unwrap();

        let auth = Mutex::new(auth);
        let generations = Mutex::new(BTreeMap::new());
        let store = Mutex::new(store);
        let monitor = Mutex::new(monitor);
        let deletions = AtomicUsize::new(0);
        let error = disconnect_github_account(
            &auth,
            &generations,
            &store,
            &monitor,
            "22".into(),
            || {
                deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || {
                deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .err()
        .unwrap();
        assert!(error.contains("monitoring state could not be saved"));
        assert_eq!(deletions.load(Ordering::SeqCst), 0);
        assert!(matches!(
            auth.lock().unwrap().accounts.get("22"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::DisconnectFailed,
                ..
            })
        ));

        fs::remove_dir(root.path().join("state/queue.json.tmp")).unwrap();
        disconnect_github_account(
            &auth,
            &generations,
            &store,
            &monitor,
            "22".into(),
            || {
                deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || {
                deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(deletions.load(Ordering::SeqCst), 2);
        assert!(!auth.lock().unwrap().accounts.contains_key("22"));
        assert_eq!(
            store.lock().unwrap().load_queue().unwrap()[0].waiting,
            WAITING_ACCOUNT_DISCONNECTED
        );
    }

    #[test]
    fn disconnect_retries_after_current_credentials_were_already_removed() {
        let (_root, store, monitor, auth) = monitoring_fixture();
        let auth = Mutex::new(auth);
        let generations = Mutex::new(BTreeMap::new());
        let store = Mutex::new(store);
        let monitor = Mutex::new(monitor);
        let current_deletions = AtomicUsize::new(0);
        let legacy_deletions = AtomicUsize::new(0);

        let error = disconnect_github_account(
            &auth,
            &generations,
            &store,
            &monitor,
            "22".into(),
            || {
                current_deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || {
                legacy_deletions.fetch_add(1, Ordering::SeqCst);
                Err("Superseded GitHub credentials could not be deleted securely. Retry disconnect.".into())
            },
        )
        .err()
        .unwrap();
        assert!(error.starts_with("Superseded GitHub credentials"));
        assert_eq!(current_deletions.load(Ordering::SeqCst), 1);
        assert_eq!(legacy_deletions.load(Ordering::SeqCst), 1);
        assert!(matches!(
            auth.lock().unwrap().accounts.get("22"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::DisconnectFailed,
                ..
            })
        ));

        disconnect_github_account(
            &auth,
            &generations,
            &store,
            &monitor,
            "22".into(),
            || {
                current_deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || {
                legacy_deletions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(current_deletions.load(Ordering::SeqCst), 2);
        assert_eq!(legacy_deletions.load(Ordering::SeqCst), 2);
        assert!(!auth.lock().unwrap().accounts.contains_key("22"));
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
            (OAuthError::RefreshRejected, GithubAuthFailure::Expired),
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
            GithubAuthFailure::Timeout
        );
        assert_eq!(
            failure_from_connection_error(ConnectionError::RateLimited),
            GithubAuthFailure::RateLimited
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
    fn offline_restore_failures_stay_visible_admitted_and_recoverable() {
        for (error, expected) in [
            (ConnectionError::Network, GithubAuthFailure::Network),
            (ConnectionError::RateLimited, GithubAuthFailure::RateLimited),
            (
                ConnectionError::ProviderFailure,
                GithubAuthFailure::Provider,
            ),
        ] {
            let mut auth = GithubAuth::new();
            auth.publish_restored_identity(identity("22", "stored-login"), Err(error));

            assert_eq!(auth.account_session_allowed("22"), Ok(()));
            assert!(auth.monitoring_accounts()["22"].connected);
            assert!(matches!(
                &auth.view().accounts[0],
                GithubAccountView::Connected {
                    warning: Some(reason),
                    ..
                } if *reason == expected
            ));

            auth.publish_session_success("22", identity("22", "current-login"))
                .unwrap();
            assert!(matches!(
                auth.accounts.get("22"),
                Some(GithubAccountState::Connected(identity))
                    if identity.login == "current-login"
            ));
            assert!(matches!(
                &auth.view().accounts[0],
                GithubAccountView::Connected { warning: None, .. }
            ));
        }
    }

    #[test]
    fn rejected_refresh_blocks_restore_and_future_session_admission() {
        let http = |_request: oauth2::HttpRequest| {
            Ok::<_, std::io::Error>(
                oauth2::http::Response::builder()
                    .status(400)
                    .header("content-type", "application/json")
                    .body(br#"{"error":"invalid_grant"}"#.to_vec())
                    .unwrap(),
            )
        };
        let oauth_error = crate::github::oauth::refresh_token_with(
            oauth2::RefreshToken::new("rejected-refresh".into()),
            UNIX_EPOCH,
            &http,
        )
        .unwrap_err();
        assert_eq!(oauth_error, OAuthError::RefreshRejected);
        let connection_error =
            rotation_connection_error(super::oauth_refresh_rotation_error(oauth_error));
        assert_eq!(connection_error, ConnectionError::SignedOut);

        let mut restored = GithubAuth::new();
        restored.publish_restored_identity(identity("22", "stored-login"), Err(connection_error));
        assert_eq!(
            restored.account_session_allowed("22"),
            Err(ConnectionError::SignedOut)
        );
        assert!(!restored.monitoring_accounts()["22"].connected);
        assert!(matches!(
            restored.accounts.get("22"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::Expired,
                ..
            })
        ));
        let (_root, store, mut monitor, _) = monitoring_fixture();
        assert!(monitor
            .prepare_checks_with_accounts(
                &store,
                &restored.monitoring_accounts(),
                1_800_000_000,
                true,
            )
            .unwrap()
            .is_empty());

        let mut session = GithubAuth::new();
        session.accounts.insert(
            "22".into(),
            GithubAccountState::Connected(identity("22", "current-login")),
        );
        session.publish_session_failure("22", connection_error);
        assert_eq!(
            session.account_session_allowed("22"),
            Err(ConnectionError::SignedOut)
        );
        assert!(matches!(
            &session.view().accounts[0],
            GithubAccountView::ReconnectRequired {
                reason: GithubAuthFailure::Expired,
                ..
            }
        ));
        let (_root, store, mut monitor, _) = monitoring_fixture();
        assert!(monitor
            .prepare_checks_with_accounts(
                &store,
                &session.monitoring_accounts(),
                1_800_000_000,
                true,
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn transient_session_failures_keep_monitoring_admitted_until_scheduled_recovery() {
        for (error, expected) in [
            (ConnectionError::Network, GithubAuthFailure::Network),
            (ConnectionError::RateLimited, GithubAuthFailure::RateLimited),
            (
                ConnectionError::ProviderFailure,
                GithubAuthFailure::Provider,
            ),
        ] {
            let (_root, store, mut monitor, auth) = monitoring_fixture();
            let accounts = auth.monitoring_accounts();
            let initial = monitor
                .prepare_checks_with_accounts(&store, &accounts, 1_800_000_000, true)
                .unwrap()
                .remove(0);
            let generations = Mutex::new(BTreeMap::new());
            let auth = Mutex::new(auth);
            let store = Mutex::new(store);
            let monitor = Mutex::new(monitor);
            finish_monitored_ticket(
                &generations,
                &auth,
                &store,
                &monitor,
                initial,
                Ok((monitoring_result(), crate::follow_up::host::Scan::default())),
                1_800_000_001,
            )
            .unwrap();

            let due = monitor.lock().unwrap().snapshot()[0].next_run;
            auth.lock().unwrap().publish_session_failure("22", error);
            assert!(matches!(
                &auth.lock().unwrap().view().accounts[0],
                GithubAccountView::Connected {
                    warning: Some(reason),
                    ..
                } if *reason == expected
            ));
            let accounts = auth.lock().unwrap().monitoring_accounts();
            let failed_ticket = {
                let store = store.lock().unwrap();
                monitor
                    .lock()
                    .unwrap()
                    .prepare_checks_with_accounts(&store, &accounts, due, false)
                    .unwrap()
                    .remove(0)
            };
            let failure = finish_monitored_ticket(
                &generations,
                &auth,
                &store,
                &monitor,
                failed_ticket,
                Err(error),
                due + 1,
            )
            .unwrap_err();
            assert!(!failure.requires_host_report());
            assert_eq!(
                store.lock().unwrap().load_queue().unwrap()[0].waiting,
                WAITING_AI_CAPACITY
            );
            let health = monitor.lock().unwrap().snapshot().remove(0);
            let expected_failure = format!("{error:?}");
            assert_eq!(
                health.last_failure.as_deref(),
                Some(expected_failure.as_str())
            );
            let next_due = health.next_run;

            let accounts = auth.lock().unwrap().monitoring_accounts();
            let recovery_ticket = {
                let store = store.lock().unwrap();
                monitor
                    .lock()
                    .unwrap()
                    .prepare_checks_with_accounts(&store, &accounts, next_due, false)
                    .unwrap()
                    .remove(0)
            };
            auth.lock()
                .unwrap()
                .publish_session_success("22", identity("22", "recovered-login"))
                .unwrap();
            finish_monitored_ticket(
                &generations,
                &auth,
                &store,
                &monitor,
                recovery_ticket,
                Ok((monitoring_result(), crate::follow_up::host::Scan::default())),
                next_due + 1,
            )
            .unwrap();
            assert!(matches!(
                auth.lock().unwrap().accounts.get("22"),
                Some(GithubAccountState::Connected(_))
            ));
            let recovered = monitor.lock().unwrap().snapshot().remove(0);
            assert_eq!(recovered.last_failure, None);
            assert_eq!(recovered.last_success, Some(next_due + 1));
        }
    }

    #[test]
    fn invalidating_session_failures_block_without_transient_downgrade() {
        for (error, expected) in [
            (ConnectionError::SignedOut, GithubAuthFailure::Expired),
            (
                ConnectionError::WrongIdentity,
                GithubAuthFailure::WrongIdentity,
            ),
            (
                ConnectionError::MissingScope,
                GithubAuthFailure::MissingScope,
            ),
            (
                ConnectionError::Configuration,
                GithubAuthFailure::CredentialsUnavailable,
            ),
        ] {
            let mut auth = GithubAuth::new();
            auth.accounts.insert(
                "22".into(),
                GithubAccountState::Connected(identity("22", "current-login")),
            );
            auth.publish_session_failure("22", error);
            assert!(auth.account_session_allowed("22").is_err());
            assert!(!auth.monitoring_accounts()["22"].connected);
            assert!(matches!(
                &auth.view().accounts[0],
                GithubAccountView::ReconnectRequired { reason, .. } if *reason == expected
            ));

            auth.publish_session_failure("22", ConnectionError::Network);
            assert!(matches!(
                auth.accounts.get("22"),
                Some(GithubAccountState::ReconnectRequired { reason, .. })
                    if *reason == expected
            ));
        }
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
    fn failed_replacement_attempts_preserve_saved_account_authority() {
        for existing in [
            None,
            Some(GithubAuthFailure::Network),
            Some(GithubAuthFailure::Expired),
            Some(GithubAuthFailure::MissingScope),
            Some(GithubAuthFailure::DisconnectFailed),
        ] {
            for attempt_failure in [
                GithubAuthFailure::Denied,
                GithubAuthFailure::Expired,
                GithubAuthFailure::BrowserOpen,
                GithubAuthFailure::Network,
                GithubAuthFailure::Timeout,
            ] {
                let mut auth = GithubAuth::new();
                match existing {
                    None => {
                        auth.accounts.insert(
                            "101".into(),
                            GithubAccountState::Connected(identity("101", "existing")),
                        );
                    }
                    Some(reason) => {
                        auth.accounts.insert(
                            "101".into(),
                            GithubAccountState::ReconnectRequired {
                                identity: identity("101", "existing"),
                                reason,
                            },
                        );
                    }
                }
                let before = serde_json::to_value(auth.copilot_view()).unwrap();
                let (cancel, _) = tokio::sync::oneshot::channel();
                let attempt = auth.start_attempt(Some("101".into()), cancel);
                auth.finish_attempt(attempt, Err(attempt_failure));

                assert_eq!(
                    serde_json::to_value(auth.copilot_view()).unwrap()["accounts"],
                    before["accounts"]
                );
                assert!(matches!(
                    auth.view().flow,
                    GithubFlowView::Failed { reason } if reason == attempt_failure
                ));
            }
        }
    }

    #[test]
    fn mismatched_replacement_cancel_and_successful_confirmation_respect_authority_boundary() {
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            GithubAccountState::ReconnectRequired {
                identity: identity("101", "existing"),
                reason: GithubAuthFailure::MissingScope,
            },
        );
        let before = serde_json::to_value(auth.copilot_view()).unwrap();

        let (cancel, _) = tokio::sync::oneshot::channel();
        let mismatch = auth.start_attempt(Some("101".into()), cancel);
        auth.finish_attempt(mismatch, Ok((identity("202", "wrong"), pair("wrong"))));
        assert_eq!(
            serde_json::to_value(auth.copilot_view()).unwrap()["accounts"],
            before["accounts"]
        );
        assert!(matches!(
            auth.view().flow,
            GithubFlowView::Failed {
                reason: GithubAuthFailure::WrongIdentity
            }
        ));

        let (cancel, _) = tokio::sync::oneshot::channel();
        let cancelled = auth.start_attempt(Some("101".into()), cancel);
        assert!(auth.cancel_attempt());
        auth.finish_attempt(cancelled, Err(GithubAuthFailure::Cancelled));
        assert_eq!(
            serde_json::to_value(auth.copilot_view()).unwrap()["accounts"],
            before["accounts"]
        );
        assert!(matches!(auth.view().flow, GithubFlowView::Idle));

        let (cancel, _) = tokio::sync::oneshot::channel();
        let success = auth.start_attempt(Some("101".into()), cancel);
        auth.finish_attempt(
            success,
            Ok((identity("101", "replacement"), pair("replacement"))),
        );
        assert_eq!(
            serde_json::to_value(auth.copilot_view()).unwrap()["accounts"],
            before["accounts"]
        );
        assert!(matches!(
            auth.view().flow,
            GithubFlowView::PendingAccountConfirmation { .. }
        ));
        auth.confirm_with(|_, _| Ok(())).unwrap();
        assert!(matches!(
            auth.accounts.get("101"),
            Some(GithubAccountState::Connected(identity))
                if identity.login == "replacement"
        ));
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

        auth.publish_session_failure("101", ConnectionError::MissingScope);

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

        auth.publish_session_failure("202", ConnectionError::OrganizationPolicyDenied);
        assert!(matches!(
            auth.accounts.get("202"),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::Provider,
                ..
            })
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
    #[cfg(target_os = "macos")]
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

        let auth = GithubAuth::restore(&current, Some(&legacy));

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
