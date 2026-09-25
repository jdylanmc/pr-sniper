mod backend;
mod operation;
mod runtime;

use crate::{
    complete_github_device_auth,
    github::{
        self,
        oauth::TokenPair,
        token_store::{ActiveAccount, ProviderAccountId, ProviderId},
    },
    ConnectionRole, GithubAccountState, GithubAuth, GithubAuthFailure, GithubAuthView, Host,
};
use backend::{Backend, NativeBackend};
use operation::{AccountWork, Operation, OPERATION_LIMIT};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime},
};
use tauri::{Manager, State};

pub(super) struct Integration<B: Backend = NativeBackend> {
    pub auth: Mutex<GithubAuth>,
    backend: B,
    restored: tokio::sync::OnceCell<()>,
    accounts: Mutex<BTreeMap<String, Arc<AccountWork>>>,
    quitting: Arc<AtomicBool>,
    lookups: Mutex<BTreeMap<(String, String), Operation>>,
}

impl Integration {
    pub fn new(isolated: bool) -> Result<Arc<Self>, String> {
        let service = if isolated {
            format!(
                "{}.copilot",
                std::env::var("PR_SNIPER_KEYCHAIN_SERVICE")
                    .map_err(|_| "Copilot secure-storage namespace is unavailable.")?
            )
        } else {
            "com.jdylanmc.pr-sniper.copilot.oauth-app.v1".into()
        };
        Ok(Arc::new(Self::with_backend(NativeBackend::new(service))))
    }
}

impl<B: Backend> Integration<B> {
    fn with_backend(backend: B) -> Self {
        Self {
            auth: Mutex::new(GithubAuth::new()),
            backend,
            restored: tokio::sync::OnceCell::new(),
            accounts: Mutex::new(BTreeMap::new()),
            quitting: Arc::new(AtomicBool::new(false)),
            lookups: Mutex::new(BTreeMap::new()),
        }
    }

    fn account(&self, id: &str) -> Result<Arc<AccountWork>, String> {
        account_key(id)?;
        Ok(self
            .accounts
            .lock()
            .map_err(|_| "Copilot account state is unavailable.")?
            .entry(id.into())
            .or_default()
            .clone())
    }

    fn operation(&self, id: &str, deadline: Instant) -> Result<Operation, String> {
        Operation::new(self.account(id)?, self.quitting.clone(), deadline)
    }

    async fn restore(self: &Arc<Self>) -> Result<(), String> {
        // Only registry metadata is shared initialization. No network request
        // holds this barrier; each saved identity verifies independently.
        self.restored
            .get_or_try_init(|| async {
                let accounts = self.backend.accounts().await?;
                if self.quitting.load(Ordering::SeqCst) {
                    return Err("PR Sniper is quitting.".into());
                }
                let mut restored = Vec::new();
                {
                    let mut auth = self
                        .auth
                        .lock()
                        .map_err(|_| "Copilot state is unavailable.")?;
                    for account in accounts
                        .into_iter()
                        .filter(|a| a.provider == ProviderId::copilot())
                    {
                        account_key(&account.account_id)?;
                        auth.accounts.insert(
                            account.account_id.clone(),
                            GithubAccountState::ReconnectRequired {
                                identity: github::Identity {
                                    id: account.account_id.clone(),
                                    login: account.login,
                                },
                                reason: GithubAuthFailure::VerificationPending,
                            },
                        );
                        restored.push(account.account_id);
                    }
                }
                for id in restored {
                    let integration = self.clone();
                    let operation = self.operation(&id, Instant::now() + OPERATION_LIMIT)?;
                    tokio::spawn(async move {
                        if let Err(error) = integration.credential(&id, &operation).await {
                            eprintln!("[copilot] stage=restore outcome=verification_incomplete");
                            // Errors are also represented in account state; never
                            // emit provider/credential data in diagnostics.
                            let _ = error;
                        }
                    });
                }
                Ok::<_, String>(())
            })
            .await?;
        Ok(())
    }

    fn view(&self) -> Result<GithubAuthView, String> {
        Ok(self
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?
            .copilot_view())
    }

    fn publish_verification(
        &self,
        id: &str,
        operation: &Operation,
        result: &Result<(github::Identity, TokenPair), GithubAuthFailure>,
    ) -> Result<(), String> {
        operation.publish(|| {
            let mut auth = self
                .auth
                .lock()
                .map_err(|_| "Copilot state is unavailable.")?;
            match result {
                Ok((identity, _)) => {
                    if matches!(
                        auth.accounts.get(id),
                        Some(GithubAccountState::ReconnectRequired {
                            reason: GithubAuthFailure::Disconnected,
                            ..
                        })
                    ) {
                        return Err(
                            "Reconnect this Copilot account before verifying or loading models."
                                .into(),
                        );
                    }
                    auth.accounts
                        .insert(id.into(), GithubAccountState::Connected(identity.clone()));
                }
                Err(reason) => {
                    let was_verified = matches!(
                        auth.accounts.get(id),
                        Some(GithubAccountState::Connected(_))
                    );
                    if !was_verified
                        || !matches!(
                            reason,
                            GithubAuthFailure::Network | GithubAuthFailure::Provider
                        )
                    {
                        auth.set_failure(id, *reason);
                    }
                }
            }
            Ok(())
        })
    }

    async fn credential(
        &self,
        id: &str,
        operation: &Operation,
    ) -> Result<(github::Identity, TokenPair), String> {
        self.require_credential_connection(id)?;
        let _guard = match operation.wait(operation.account.gate.lock()).await {
            Ok(guard) => guard,
            Err(error) => {
                self.finish_incomplete_verification(id, operation, &error);
                return Err(error);
            }
        };
        self.require_credential_connection(id)?;
        let key = account_key(id)?;
        let result = async {
            let mut pair = match operation.wait(self.backend.load(key.clone())).await? {
                Ok(Some(pair)) => pair,
                Ok(None) => return Ok(Err(GithubAuthFailure::Expired)),
                Err(_) => return Ok(Err(GithubAuthFailure::CredentialsUnavailable)),
            };
            if pair.refresh_is_expired(SystemTime::now()) {
                return Ok(Err(GithubAuthFailure::Expired));
            }
            if pair.access_is_expired(SystemTime::now()) {
                operation.check()?;
                // Once sent, finish rotation and persist the returned pair even
                // if cancelled. Keep the account lease until that transaction
                // ends; cancellation must not strand a rotated refresh token.
                let refreshed = tokio::time::timeout_at(
                    operation.deadline.into(),
                    self.backend.refresh(&pair),
                )
                .await
                .map_err(|_| {
                    "Copilot operation timed out during credential refresh. Retry or reconnect."
                })?;
                pair = match refreshed {
                    Ok(pair) => pair,
                    Err(reason) => return Ok(Err(reason)),
                };
                self.backend.save_pair(key, pair.clone()).await?;
                operation.check()?;
            }
            let identity = match operation.wait(self.backend.identity(&pair)).await? {
                Ok(identity) => identity,
                Err(reason) => return Ok(Err(reason)),
            };
            if identity.id != id {
                return Ok(Err(GithubAuthFailure::WrongIdentity));
            }
            Ok::<_, String>(Ok((identity, pair)))
        }
        .await;
        match result {
            Ok(result) => {
                self.publish_verification(id, operation, &result)?;
                result.map_err(verification_error)
            }
            Err(error) => {
                self.finish_incomplete_verification(id, operation, &error);
                Err(error)
            }
        }
    }

    fn require_credential_connection(&self, id: &str) -> Result<(), String> {
        let auth = self
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        match auth.accounts.get(id) {
            None => Err("Connect and confirm this Copilot account first.".into()),
            Some(GithubAccountState::ReconnectRequired {
                reason: GithubAuthFailure::Disconnected,
                ..
            }) => Err("Reconnect this Copilot account before verifying or loading models.".into()),
            Some(_) => Ok(()),
        }
    }

    fn finish_incomplete_verification(&self, id: &str, operation: &Operation, error: &str) {
        // Stale/cancelled generations cannot change state. A current restore
        // that exhausts its deadline must not remain "checking" indefinitely.
        let _ = operation.complete(|| {
            let mut auth = self
                .auth
                .lock()
                .map_err(|_| "Copilot state is unavailable.")?;
            if matches!(
                auth.accounts.get(id),
                Some(GithubAccountState::ReconnectRequired {
                    reason: GithubAuthFailure::VerificationPending,
                    ..
                })
            ) {
                auth.set_failure(
                    id,
                    if error.contains("timed out") {
                        GithubAuthFailure::Timeout
                    } else {
                        GithubAuthFailure::CredentialsUnavailable
                    },
                );
            }
            Ok(())
        });
    }

    fn cancel_lookup(&self, id: &str) -> Result<(), String> {
        for ((account, _), operation) in self
            .lookups
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?
            .iter()
        {
            if account == id {
                operation.cancelled.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    fn invalidate(&self, id: &str) -> Result<(), String> {
        self.account(id)?.invalidate(|| {
            self.cancel_lookup(id)?;
            let mut auth = self
                .auth
                .lock()
                .map_err(|_| "Copilot state is unavailable.")?;
            if matches!(
                auth.accounts.get(id),
                Some(GithubAccountState::ReconnectRequired {
                    reason: GithubAuthFailure::VerificationPending,
                    ..
                })
            ) {
                auth.set_failure(id, GithubAuthFailure::VerificationRequired);
            }
            Ok(())
        })
    }

    async fn disconnect(&self, id: &str) -> Result<GithubAuthView, String> {
        let account = self.account(id)?;
        account.invalidate(|| {
            self.cancel_lookup(id)?;
            let mut auth = self
                .auth
                .lock()
                .map_err(|_| "Copilot state is unavailable.")?;
            if auth.pending.as_ref().is_some_and(|p| p.identity.id == id)
                || auth
                    .active
                    .as_ref()
                    .is_some_and(|a| a.expected_account_id.as_deref() == Some(id))
            {
                auth.cancel_attempt();
            }
            auth.set_failure(id, GithubAuthFailure::Disconnected);
            Ok(())
        })?;
        let operation = self.operation(id, Instant::now() + OPERATION_LIMIT)?;
        let _guard = operation.wait(account.gate.lock()).await?;
        self.backend
            .clear(account_key(id)?)
            .await
            .map_err(|_| "Copilot credentials could not be deleted securely. Retry disconnect.")?;
        operation.check()?;
        self.view()
    }

    pub fn request_shutdown(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        if let Ok(lookups) = self.lookups.lock() {
            for operation in lookups.values() {
                operation.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn shutdown(&self) {
        self.request_shutdown();
        if let Ok(mut auth) = self.auth.lock() {
            auth.cancel_attempt();
        }
    }

    pub fn lookups_finished(&self) -> bool {
        self.lookups.lock().is_ok_and(|lookups| lookups.is_empty())
    }
}

fn verification_error(reason: GithubAuthFailure) -> String {
    match reason {
        GithubAuthFailure::Network => "Cannot reach GitHub to verify this Copilot identity. Retry.",
        GithubAuthFailure::Provider => {
            "GitHub could not verify this Copilot identity. Retry or reconnect."
        }
        GithubAuthFailure::WrongIdentity => {
            "The credential belongs to a different GitHub identity. Reconnect the selected account."
        }
        GithubAuthFailure::CredentialsUnavailable => {
            "Copilot credentials are unavailable in secure storage. Retry or reconnect."
        }
        _ => "This Copilot account needs to be reconnected.",
    }
    .into()
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

#[tauri::command]
pub(super) async fn copilot_auth_state(app: tauri::AppHandle) -> Result<GithubAuthView, String> {
    let integration = app.state::<Host>().copilot.clone();
    integration.restore().await?;
    integration.view()
}

#[tauri::command]
pub(super) async fn start_copilot_auth(
    app: tauri::AppHandle,
    expected_account_id: Option<String>,
) -> Result<GithubAuthView, String> {
    let integration = app.state::<Host>().copilot.clone();
    integration.restore().await?;
    if let Some(id) = &expected_account_id {
        integration.invalidate(id)?;
    }
    if integration.quitting.load(Ordering::SeqCst) {
        return Err("PR Sniper is quitting.".into());
    }
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (attempt, view) = {
        let mut auth = integration
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        let attempt = auth.start_attempt(expected_account_id, cancel_tx);
        (attempt, auth.copilot_view())
    };
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
    let integration = app.state::<Host>().copilot.clone();
    integration.restore().await?;
    let (identity, pair) = {
        let auth = integration
            .auth
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        let pending = auth
            .pending
            .as_ref()
            .ok_or("No Copilot account is awaiting confirmation.")?;
        (pending.identity.clone(), pending.pair.clone())
    };
    integration.invalidate(&identity.id)?;
    let operation = integration.operation(&identity.id, Instant::now() + OPERATION_LIMIT)?;
    let guard = operation
        .wait(operation.account.gate.clone().lock_owned())
        .await?;
    if pair.access_is_expired(SystemTime::now()) {
        return Err("Copilot sign-in expired before confirmation. Cancel and reconnect.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        operation.publish(|| {
            let mut auth = integration
                .auth
                .lock()
                .map_err(|_| "Copilot state is unavailable.")?;
            if !auth
                .pending
                .as_ref()
                .is_some_and(|p| p.identity == identity && p.pair == pair)
            {
                return Err(
                    "Copilot confirmation changed. Confirm the current identity instead.".into(),
                );
            }
            // Confirmation is one atomic local persistence boundary, as in
            // repository OAuth. No remote call holds this auth-state lock.
            auth.confirm_with(|identity, pair| {
                let account = ActiveAccount::for_provider(
                    ProviderId::copilot(),
                    &identity.id,
                    &identity.login,
                )
                .map_err(|_| ())?;
                integration.backend.save_account(&account, pair)
            })
            .map_err(|_| {
                "Copilot credentials could not be saved securely. Retry confirmation or cancel."
            })?;
            Ok(auth.copilot_view())
        })
    })
    .await
    .map_err(|_| "Copilot confirmation could not finish. Retry.")?
}

#[tauri::command]
pub(super) async fn cancel_copilot_auth(app: tauri::AppHandle) -> Result<GithubAuthView, String> {
    let integration = app.state::<Host>().copilot.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut auth = integration
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
    let integration = app.state::<Host>().copilot.clone();
    integration.restore().await?;
    integration.disconnect(&account_id).await
}

#[tauri::command]
pub(super) async fn verify_copilot_account(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<GithubAuthView, String> {
    let integration = app.state::<Host>().copilot.clone();
    let operation = integration.operation(&account_id, Instant::now() + OPERATION_LIMIT)?;
    operation.wait(integration.restore()).await??;
    integration.credential(&account_id, &operation).await?;
    integration.view()
}

#[tauri::command]
pub(super) fn cancel_copilot_models(
    host: State<'_, Host>,
    account_id: String,
    request_id: String,
) -> Result<(), String> {
    if let Some(operation) = host
        .copilot
        .lookups
        .lock()
        .map_err(|_| "Copilot state is unavailable.")?
        .get(&(account_id, request_id))
    {
        operation.cancelled.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn list_copilot_models(
    app: tauri::AppHandle,
    account_id: String,
    request_id: String,
) -> Result<Vec<github_copilot_sdk::Model>, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "Invalid Copilot lookup identity.")?;
    let integration = app.state::<Host>().copilot.clone();
    let operation = integration.operation(&account_id, Instant::now() + OPERATION_LIMIT)?;
    let key = (account_id.clone(), request_id);
    {
        let mut lookups = integration
            .lookups
            .lock()
            .map_err(|_| "Copilot state is unavailable.")?;
        if lookups.contains_key(&key) {
            return Err("Copilot lookup already running.".into());
        }
        lookups.insert(key.clone(), operation.clone());
    }
    let result = async {
        operation.wait(integration.restore()).await??;
        let (identity, pair) = integration.credential(&account_id, &operation).await?;
        operation.check()?;
        let runtime_operation = operation.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            runtime::models(
                &identity,
                &pair,
                &runtime_operation.cancelled,
                runtime_operation.deadline,
            )
        })
        .await
        .map_err(|_| "Copilot model lookup could not finish. Retry.")?;
        operation.publish(|| outcome)
    }
    .await;
    integration
        .lookups
        .lock()
        .map_err(|_| "Copilot state is unavailable.")?
        .remove(&key);
    result
}

#[cfg(test)]
mod tests;
