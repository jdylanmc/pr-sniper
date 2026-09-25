use crate::{
    failure_from_connection_error, failure_from_oauth_error,
    github::{
        self,
        macos_keychain::MacKeychainStore,
        oauth::TokenPair,
        token_store::{ActiveAccount, CredentialStore, ProviderAccountId, RotationSafeStore},
    },
    GithubAuthFailure,
};
use std::{future::Future, sync::Arc};

pub(crate) trait Backend: Send + Sync + 'static {
    fn accounts(&self) -> impl Future<Output = Result<Vec<ActiveAccount>, String>> + Send;
    fn load(
        &self,
        key: ProviderAccountId,
    ) -> impl Future<Output = Result<Option<TokenPair>, String>> + Send;
    fn save_pair(
        &self,
        key: ProviderAccountId,
        pair: TokenPair,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn clear(&self, key: ProviderAccountId) -> impl Future<Output = Result<(), String>> + Send;
    fn refresh(
        &self,
        pair: &TokenPair,
    ) -> impl Future<Output = Result<TokenPair, GithubAuthFailure>> + Send;
    fn identity(
        &self,
        pair: &TokenPair,
    ) -> impl Future<Output = Result<github::Identity, GithubAuthFailure>> + Send;
}

pub(crate) struct NativeBackend {
    store: Arc<RotationSafeStore<MacKeychainStore>>,
}

impl NativeBackend {
    pub fn new(service: String) -> Self {
        Self {
            store: Arc::new(RotationSafeStore::new(MacKeychainStore::with_service(
                service,
            ))),
        }
    }

    pub fn save_account(&self, account: &ActiveAccount, pair: &TokenPair) -> Result<(), ()> {
        self.store
            .save_account(account, pair, false)
            .map_err(|_| ())
    }

    async fn storage<T: Send + 'static>(
        &self,
        work: impl FnOnce(
                &RotationSafeStore<MacKeychainStore>,
            ) -> Result<T, github::token_store::StoreError>
            + Send
            + 'static,
    ) -> Result<T, String> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || work(&store))
            .await
            .map_err(|_| "Copilot secure storage operation could not finish.")?
            .map_err(|_| "Copilot secure storage is unavailable. Retry.")
            .map_err(String::from)
    }
}

impl Backend for NativeBackend {
    async fn accounts(&self) -> Result<Vec<ActiveAccount>, String> {
        self.storage(|store| store.accounts()).await
    }

    async fn load(&self, key: ProviderAccountId) -> Result<Option<TokenPair>, String> {
        self.storage(move |store| store.load(&key)).await
    }

    async fn save_pair(&self, key: ProviderAccountId, pair: TokenPair) -> Result<(), String> {
        self.storage(move |store| store.save(&key, &pair)).await
    }

    async fn clear(&self, key: ProviderAccountId) -> Result<(), String> {
        self.storage(move |store| store.clear_account_credentials(&key))
            .await
    }

    async fn refresh(&self, pair: &TokenPair) -> Result<TokenPair, GithubAuthFailure> {
        github::oauth::refresh_token_async(pair.refresh_token())
            .await
            .map_err(failure_from_oauth_error)
    }

    async fn identity(&self, pair: &TokenPair) -> Result<github::Identity, GithubAuthFailure> {
        github::http::current_identity_async(pair)
            .await
            .map_err(failure_from_connection_error)
    }
}
