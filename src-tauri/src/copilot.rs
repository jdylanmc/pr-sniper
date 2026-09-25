mod runtime;

use crate::{
    complete_github_device_auth, failure_from_connection_error, failure_from_oauth_error,
    github::{
        self,
        macos_keychain::MacKeychainStore,
        oauth::TokenPair,
        token_store::{
            ActiveAccount, ProviderAccountId, ProviderId, RotationError, RotationSafeStore,
        },
    },
    ConnectionRole, GithubAccountState, GithubAuth, GithubAuthFailure, GithubAuthView, Host,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::SystemTime,
};
use tauri::{Manager, State};

pub(super) struct Integration {
    pub auth: Mutex<GithubAuth>,
    credentials: RotationSafeStore<MacKeychainStore>,
    operations: tokio::sync::Mutex<()>,
    restored: AtomicBool,
    lookups: Mutex<BTreeMap<(String, String), Arc<AtomicBool>>>,
}

impl Integration {
    pub fn new(isolated: bool) -> Result<Self, String> {
        let service = if isolated {
            // The host validates this test-owned namespace before constructing us.
            format!(
                "{}.copilot",
                std::env::var("PR_SNIPER_KEYCHAIN_SERVICE")
                    .map_err(|_| "Copilot secure-storage namespace is unavailable.")?
            )
        } else {
            "com.jdylanmc.pr-sniper.copilot.oauth-app.v1".into()
        };
        Ok(Self {
            auth: Mutex::new(GithubAuth::new()),
            credentials: RotationSafeStore::new(MacKeychainStore::with_service(service)),
            operations: tokio::sync::Mutex::new(()),
            restored: AtomicBool::new(false),
            lookups: Mutex::new(BTreeMap::new()),
        })
    }

    fn restore(&self) -> Result<(), String> {
        if !self.restored.load(Ordering::SeqCst) {
            let auth = GithubAuth::restore_accounts(&self.credentials, ProviderId::copilot())
                .map_err(|_| "Copilot identities could not be read from secure storage. Retry.")?;
            *self
                .auth
                .lock()
                .map_err(|_| "Copilot state is unavailable.")? = auth;
            self.restored.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn view(&self) -> Result<GithubAuthView, String> {
        Ok(self
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?
            .copilot_view())
    }

    fn credential(&self, account_id: &str) -> Result<(github::Identity, TokenPair), String> {
        let key = account_key(account_id)?;
        if !self
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?
            .accounts
            .contains_key(account_id)
        {
            return Err("Connect and confirm this Copilot account first.".into());
        }
        let result = (|| {
            let http = github::oauth::GithubOAuthHttp::new().map_err(failure_from_oauth_error)?;
            let pair = self
                .credentials
                .refresh_if_needed(&key, SystemTime::now(), |current| {
                    http.refresh(current.refresh_token())
                        .map_err(|error| match error {
                            github::oauth::OAuthError::Network => RotationError::Network,
                            _ => RotationError::Provider,
                        })
                })
                .map_err(|error| match error {
                    RotationError::Network => GithubAuthFailure::Network,
                    RotationError::Provider => GithubAuthFailure::Provider,
                    RotationError::ReconnectRequired => GithubAuthFailure::Expired,
                    RotationError::Store(_) => GithubAuthFailure::CredentialsUnavailable,
                })?;
            let identity = github::provider::GithubClient::new(
                github::http::HttpTransport::from_token_pair(&pair)
                    .map_err(failure_from_connection_error)?,
            )
            .current_identity()
            .map_err(failure_from_connection_error)?;
            if identity.id != account_id {
                return Err(GithubAuthFailure::WrongIdentity);
            }
            Ok((identity, pair))
        })();
        let mut auth = self
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        match result {
            Ok((identity, pair)) => {
                auth.accounts.insert(
                    account_id.into(),
                    GithubAccountState::Connected(identity.clone()),
                );
                Ok((identity, pair))
            }
            Err(reason) => {
                // A transport failure cannot establish that a previously verified identity signed out.
                if !matches!(
                    reason,
                    GithubAuthFailure::Network | GithubAuthFailure::Provider
                ) {
                    auth.set_failure(account_id, reason);
                }
                Err(match reason {
                    GithubAuthFailure::Network => "Cannot reach GitHub to verify this Copilot identity. Retry.",
                    GithubAuthFailure::Provider => "GitHub could not verify this Copilot identity. Retry or reconnect.",
                    GithubAuthFailure::CredentialsUnavailable => "Copilot credentials are unavailable in secure storage. Retry or reconnect.",
                    GithubAuthFailure::WrongIdentity => "The credential belongs to a different GitHub identity. Reconnect the selected account.",
                    _ => "This Copilot account needs to be reconnected.",
                }.into())
            }
        }
    }

    fn cancel_lookup(&self, account_id: &str) -> Result<(), String> {
        for ((id, _), cancel) in self
            .lookups
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?
            .iter()
        {
            if id == account_id {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Ok(mut auth) = self.auth.lock() {
            auth.cancel_attempt();
        }

        if let Ok(lookups) = self.lookups.lock() {
            for cancel in lookups.values() {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn lookups_finished(&self) -> bool {
        self.lookups.lock().is_ok_and(|lookups| lookups.is_empty())
    }
}

fn account_key(id: &str) -> Result<ProviderAccountId, String> {
    if id.is_empty()
        || id.len() > 20
        || !id.bytes().all(|b| b.is_ascii_digit())
        || id.parse::<u64>().ok().is_none_or(|id| id == 0)
    {
        return Err("Choose a valid stable Copilot account identity.".into());
    }
    ProviderAccountId::new(ProviderId::copilot(), id)
        .map_err(|_| "Choose a valid stable Copilot account identity.".into())
}

async fn with_accounts<T: Send + 'static>(
    app: tauri::AppHandle,
    work: impl FnOnce(&Integration) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let host = app.state::<Host>();
    if host.quitting.load(Ordering::SeqCst) {
        return Err("PR Sniper is quitting.".into());
    }
    let _guard = host.copilot.operations.lock().await;
    let work_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let host = work_app.state::<Host>();
        host.copilot.restore()?;
        work(&host.copilot)
    })
    .await
    .map_err(|_| "Copilot operation could not finish. Retry.")?
}

#[tauri::command]
pub(super) async fn copilot_auth_state(app: tauri::AppHandle) -> Result<GithubAuthView, String> {
    with_accounts(app, Integration::view).await
}

#[tauri::command]
pub(super) async fn start_copilot_auth(
    app: tauri::AppHandle,
    expected_account_id: Option<String>,
) -> Result<GithubAuthView, String> {
    if let Some(id) = &expected_account_id {
        account_key(id)?;
        app.state::<Host>().copilot.cancel_lookup(id)?;
    }
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (attempt, view) = with_accounts(app.clone(), move |integration| {
        let mut auth = integration
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        let attempt = auth.start_attempt(expected_account_id, cancel_tx);
        Ok((attempt, auth.copilot_view()))
    })
    .await?;
    tauri::async_runtime::spawn(complete_github_device_auth(
        app,
        attempt,
        cancel_rx,
        ConnectionRole::Copilot,
    ));
    Ok(view)
}

#[tauri::command]
pub(super) async fn confirm_copilot_account(
    app: tauri::AppHandle,
) -> Result<GithubAuthView, String> {
    with_accounts(app, |integration| {
        let mut auth = integration
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        if auth
            .pending
            .as_ref()
            .is_some_and(|pending| pending.pair.access_is_expired(SystemTime::now()))
        {
            auth.pending_failure = Some(GithubAuthFailure::Expired);
            return Err(
                "Copilot sign-in expired before confirmation. Cancel and reconnect.".into(),
            );
        }
        auth.confirm_with(|identity, pair| {
            let account =
                ActiveAccount::for_provider(ProviderId::copilot(), &identity.id, &identity.login)
                    .map_err(|_| ())?;
            integration
                .credentials
                .save_account(&account, pair, false)
                .map_err(|_| ())
        })
        .map_err(|_| {
            "Copilot credentials could not be saved securely. Retry confirmation or cancel."
        })?;
        Ok(auth.copilot_view())
    })
    .await
}

#[tauri::command]
pub(super) async fn cancel_copilot_auth(app: tauri::AppHandle) -> Result<GithubAuthView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let mut auth = host
            .copilot
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        auth.cancel_attempt();
        Ok(auth.copilot_view())
    })
    .await
    .map_err(|_| "Copilot cancellation could not finish. Retry.")?
}

#[tauri::command]
pub(super) async fn disconnect_copilot_account(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<GithubAuthView, String> {
    let key = account_key(&account_id)?;
    app.state::<Host>().copilot.cancel_lookup(&account_id)?;
    with_accounts(app, move |integration| {
        let mut auth = integration
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        // A pending replacement must not revive a just-disconnected connection.
        if auth
            .pending
            .as_ref()
            .is_some_and(|p| p.identity.id == account_id)
            || auth
                .active
                .as_ref()
                .is_some_and(|a| a.expected_account_id.as_deref() == Some(&account_id))
        {
            auth.cancel_attempt();
        }
        integration
            .credentials
            .clear_account_credentials(&key)
            .map_err(|_| "Copilot credentials could not be deleted securely. Retry disconnect.")?;
        auth.set_failure(&account_id, GithubAuthFailure::Disconnected);
        Ok(auth.copilot_view())
    })
    .await
}

#[tauri::command]
pub(super) async fn verify_copilot_account(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<GithubAuthView, String> {
    with_accounts(app, move |integration| {
        integration.credential(&account_id)?;
        integration.view()
    })
    .await
}

#[tauri::command]
pub(super) fn cancel_copilot_models(
    host: State<'_, Host>,
    account_id: String,
    request_id: String,
) -> Result<(), String> {
    if let Some(cancel) = host
        .copilot
        .lookups
        .lock()
        .map_err(|_| "Copilot state is unavailable.")?
        .get(&(account_id, request_id))
    {
        cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn list_copilot_models(
    app: tauri::AppHandle,
    account_id: String,
    request_id: String,
) -> Result<Vec<github_copilot_sdk::Model>, String> {
    account_key(&account_id)?;
    uuid::Uuid::parse_str(&request_id).map_err(|_| "Invalid Copilot lookup identity.")?;
    let lookup_key = (account_id.clone(), request_id);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let host = app.state::<Host>();
        let mut lookups = host
            .copilot
            .lookups
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        if lookups.contains_key(&lookup_key) {
            return Err("Copilot lookup already running.".into());
        }
        lookups.insert(lookup_key.clone(), cancel.clone());
    }
    let id = account_id.clone();
    let outcome = async {
        let (identity, pair) =
            with_accounts(app.clone(), move |integration| integration.credential(&id)).await?;
        let work_cancel = cancel.clone();
        tauri::async_runtime::spawn_blocking(move || {
            runtime::models(&identity, &pair, &work_cancel)
        })
        .await
        .map_err(|_| "Copilot model lookup could not finish. Retry.".to_string())?
    }
    .await;
    let host = app.state::<Host>();
    let mut lookups = host
        .copilot
        .lookups
        .lock()
        .map_err(|_| "Copilot state is unavailable.")?;
    lookups.remove(&lookup_key);
    if cancel.load(Ordering::SeqCst) {
        return Err("Copilot model lookup cancelled.".into());
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use github::token_store::CredentialStore;
    use std::time::Duration;

    #[test]
    fn disconnected_identity_survives_native_keychain_restart_without_touching_repository_role() {
        let namespace = format!(
            "com.jdylanmc.pr-sniper.tests.copilot-{}",
            uuid::Uuid::new_v4()
        );
        let repo =
            RotationSafeStore::new(MacKeychainStore::with_service(format!("{namespace}.repo")));
        let ai = RotationSafeStore::new(MacKeychainStore::with_service(format!("{namespace}.ai")));
        let repo_account = ActiveAccount::new("101", "fixture-login").unwrap();
        let ai_account =
            ActiveAccount::for_provider(ProviderId::copilot(), "101", "fixture-login").unwrap();
        let pair = TokenPair::new(
            "fixture-access",
            "fixture-refresh",
            Duration::from_secs(60),
            Duration::from_secs(120),
        );
        repo.save_account(&repo_account, &pair, false).unwrap();
        ai.save_account(&ai_account, &pair, false).unwrap();
        ai.clear_account_credentials(&ai_account.provider_account_id())
            .unwrap();
        let restarted =
            RotationSafeStore::new(MacKeychainStore::with_service(format!("{namespace}.ai")));
        let state = GithubAuth::restore_accounts(&restarted, ProviderId::copilot()).unwrap();
        let view = serde_json::to_value(state.copilot_view()).unwrap();
        let repository_pair = repo
            .load(&repo_account.provider_account_id())
            .unwrap()
            .unwrap();
        repo.remove_account(&repo_account.provider_account_id())
            .unwrap();
        ai.remove_account(&ai_account.provider_account_id())
            .unwrap();
        assert!(matches!(
            state.accounts.get("101"),
            Some(GithubAccountState::ReconnectRequired { .. })
        ));
        assert_eq!(view["accounts"][0]["provider"], "copilot");
        assert_eq!(view["accounts"][0]["login"], "fixture-login");
        assert!(!view.to_string().contains("fixture-access"));
        assert_eq!(repository_pair.access_token(), pair.access_token());
        assert_eq!(repository_pair.refresh_token(), pair.refresh_token());
    }

    #[test]
    fn independent_flows_keep_reconnect_identity_and_cancel_pending_secrets() {
        let mut repo = GithubAuth::new();
        let mut ai = GithubAuth::new();
        let (repo_cancel, _) = tokio::sync::oneshot::channel();
        let repo_attempt = repo.start_attempt(None, repo_cancel);
        let (ai_cancel, _) = tokio::sync::oneshot::channel();
        let ai_attempt = ai.start_attempt(Some("101".into()), ai_cancel);
        let pair = TokenPair::new(
            "fixture",
            "refresh-fixture",
            Duration::from_secs(60),
            Duration::from_secs(120),
        );
        ai.finish_attempt(
            ai_attempt,
            Ok((
                github::Identity {
                    id: "202".into(),
                    login: "wrong".into(),
                },
                pair,
            )),
        );
        assert!(ai.pending.is_none());
        assert_eq!(ai.failure, Some(GithubAuthFailure::WrongIdentity));
        ai.cancel_attempt();
        assert!(repo.is_active_attempt(repo_attempt));
        assert!(repo.failure.is_none());
    }
}
