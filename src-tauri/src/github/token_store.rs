use super::oauth::TokenPair;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || value.starts_with('-')
            || value.ends_with('-')
        {
            return Err(StoreError::InvalidData);
        }
        Ok(Self(value))
    }

    pub fn github() -> Self {
        Self("github".into())
    }

    pub fn copilot() -> Self {
        Self("copilot".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderAccountId {
    provider: ProviderId,
    account_id: String,
}

impl ProviderAccountId {
    pub fn new(provider: ProviderId, account_id: impl Into<String>) -> Result<Self, StoreError> {
        let account_id = account_id.into();
        if account_id.is_empty()
            || account_id.len() > 128
            || account_id.chars().any(char::is_whitespace)
        {
            return Err(StoreError::InvalidData);
        }
        Ok(Self {
            provider,
            account_id,
        })
    }

    pub fn github(account_id: impl Into<String>) -> Self {
        Self {
            provider: ProviderId::github(),
            account_id: account_id.into(),
        }
    }

    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveAccount {
    pub provider: ProviderId,
    pub account_id: String,
    pub login: String,
}

impl ActiveAccount {
    pub fn new(
        account_id: impl Into<String>,
        login: impl Into<String>,
    ) -> Result<Self, StoreError> {
        Self::for_provider(ProviderId::github(), account_id, login)
    }

    pub fn for_provider(
        provider: ProviderId,
        account_id: impl Into<String>,
        login: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let account = Self {
            provider,
            account_id: account_id.into(),
            login: login.into(),
        };
        ProviderAccountId::new(account.provider.clone(), account.account_id.clone())?;
        if account.login.is_empty()
            || account.login.len() > 128
            || account.login.chars().any(char::is_whitespace)
        {
            return Err(StoreError::InvalidData);
        }
        Ok(account)
    }

    pub fn provider_account_id(&self) -> ProviderAccountId {
        ProviderAccountId {
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredCredentials {
    pub account: ActiveAccount,
    pub pair: TokenPair,
}

pub type CredentialKey = ProviderAccountId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountRegistry {
    pub accounts: Vec<ActiveAccount>,
    pub active: Option<ProviderAccountId>,
    pub pending_account_additions: Vec<ActiveAccount>,
    pub pending_secret_deletions: Vec<ProviderAccountId>,
}

impl AccountRegistry {
    pub fn validate(&self) -> Result<(), StoreError> {
        let mut ids = BTreeMap::new();
        for account in &self.accounts {
            ActiveAccount::for_provider(
                account.provider.clone(),
                account.account_id.clone(),
                account.login.clone(),
            )?;
            let id = account.provider_account_id();
            if ids.insert(id, ()).is_some() {
                return Err(StoreError::InvalidData);
            }
        }
        if let Some(active) = &self.active {
            ProviderAccountId::new(active.provider.clone(), active.account_id.clone())?;
        }
        let mut pending_ids = BTreeMap::new();
        for account in &self.pending_account_additions {
            ActiveAccount::for_provider(
                account.provider.clone(),
                account.account_id.clone(),
                account.login.clone(),
            )?;
            let id = account.provider_account_id();
            if ids.contains_key(&id) || pending_ids.insert(id, ()).is_some() {
                return Err(StoreError::InvalidData);
            }
        }
        for pending in &self.pending_secret_deletions {
            ProviderAccountId::new(pending.provider.clone(), pending.account_id.clone())?;
            if pending_ids.insert(pending.clone(), ()).is_some() {
                return Err(StoreError::InvalidData);
            }
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| !ids.contains_key(active))
            || self
                .pending_secret_deletions
                .iter()
                .any(|pending| ids.contains_key(pending))
        {
            return Err(StoreError::InvalidData);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    InvalidData,
    Unavailable,
}

pub trait CredentialStore {
    fn load(&self, key: &CredentialKey) -> Result<Option<TokenPair>, StoreError>;
    fn save(&self, key: &CredentialKey, pair: &TokenPair) -> Result<(), StoreError>;
    fn delete(&self, key: &CredentialKey) -> Result<(), StoreError>;
}

pub trait AccountRegistryStore {
    fn load_registry(&self) -> Result<AccountRegistry, StoreError>;
    fn save_registry(&self, registry: &AccountRegistry) -> Result<(), StoreError>;
}

pub trait ActiveCredentialStore {
    fn load_active_credentials(&self) -> Result<Option<RestoredCredentials>, StoreError>;
    fn save_active_credentials(&self, credentials: &RestoredCredentials) -> Result<(), StoreError>;
    fn delete_active_credentials(&self) -> Result<(), StoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationError {
    ReconnectRequired,
    Store(StoreError),
    Provider,
    Network,
}

pub struct RotationSafeStore<S> {
    inner: S,
    registry: Mutex<()>,
    rotations: Mutex<BTreeMap<ProviderAccountId, Arc<Mutex<()>>>>,
}

impl<S> RotationSafeStore<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            registry: Mutex::new(()),
            rotations: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }

    fn account_lock(&self, id: &ProviderAccountId) -> Result<Arc<Mutex<()>>, StoreError> {
        let mut locks = self.rotations.lock().map_err(|_| StoreError::Unavailable)?;
        Ok(locks
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
}

impl<S: CredentialStore> CredentialStore for RotationSafeStore<S> {
    fn load(&self, key: &CredentialKey) -> Result<Option<TokenPair>, StoreError> {
        self.inner.load(key)
    }

    fn save(&self, key: &CredentialKey, pair: &TokenPair) -> Result<(), StoreError> {
        self.inner.save(key, pair)
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
        self.inner.delete(key)
    }
}

impl<S: ActiveCredentialStore> ActiveCredentialStore for RotationSafeStore<S> {
    fn load_active_credentials(&self) -> Result<Option<RestoredCredentials>, StoreError> {
        self.inner.load_active_credentials()
    }

    fn save_active_credentials(&self, credentials: &RestoredCredentials) -> Result<(), StoreError> {
        self.inner.save_active_credentials(credentials)
    }

    fn delete_active_credentials(&self) -> Result<(), StoreError> {
        self.inner.delete_active_credentials()
    }
}

impl<S: AccountRegistryStore + CredentialStore> RotationSafeStore<S> {
    fn load_registry_cleaned(&self) -> Result<AccountRegistry, StoreError> {
        let mut registry = self.inner.load_registry()?;
        registry.validate()?;
        if registry.pending_account_additions.is_empty()
            && registry.pending_secret_deletions.is_empty()
        {
            return Ok(registry);
        }
        for account in &registry.pending_account_additions {
            let id = account.provider_account_id();
            let lock = self.account_lock(&id)?;
            let _guard = lock.lock().map_err(|_| StoreError::Unavailable)?;
            self.inner.delete(&id)?;
        }
        for id in &registry.pending_secret_deletions {
            let lock = self.account_lock(id)?;
            let _guard = lock.lock().map_err(|_| StoreError::Unavailable)?;
            self.inner.delete(id)?;
        }
        registry.pending_account_additions.clear();
        registry.pending_secret_deletions.clear();
        self.inner.save_registry(&registry)?;
        Ok(registry)
    }

    pub fn accounts(&self) -> Result<Vec<ActiveAccount>, StoreError> {
        let _guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
        Ok(self.load_registry_cleaned()?.accounts)
    }

    pub fn restore_account(
        &self,
        id: &ProviderAccountId,
    ) -> Result<Option<RestoredCredentials>, StoreError> {
        let _registry_guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
        let account = self
            .load_registry_cleaned()?
            .accounts
            .into_iter()
            .find(|account| account.provider_account_id() == *id);
        let Some(account) = account else {
            return Ok(None);
        };
        let lock = self.account_lock(id)?;
        let _guard = lock.lock().map_err(|_| StoreError::Unavailable)?;
        let pair = self.inner.load(id)?.ok_or(StoreError::InvalidData)?;
        Ok(Some(RestoredCredentials { account, pair }))
    }

    pub fn save_account(
        &self,
        account: &ActiveAccount,
        pair: &TokenPair,
        make_active: bool,
    ) -> Result<(), StoreError> {
        let id = account.provider_account_id();
        let _registry_guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
        let mut registry = self.load_registry_cleaned()?;
        let lock = self.account_lock(&id)?;
        let _account_guard = lock.lock().map_err(|_| StoreError::Unavailable)?;
        if let Some(stored) = registry
            .accounts
            .iter_mut()
            .find(|stored| stored.provider_account_id() == id)
        {
            self.inner.save(&id, pair)?;
            *stored = account.clone();
        } else {
            registry.pending_account_additions.push(account.clone());
            registry.validate()?;
            self.inner.save_registry(&registry)?;
            self.inner.save(&id, pair)?;
            registry
                .pending_account_additions
                .retain(|pending| pending.provider_account_id() != id);
            registry.accounts.push(account.clone());
        }
        if make_active {
            registry.active = Some(id);
        }
        registry.validate()?;
        self.inner.save_registry(&registry)
    }

    pub fn select_active_account(&self, id: Option<&ProviderAccountId>) -> Result<(), StoreError> {
        let _registry_guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
        let mut registry = self.load_registry_cleaned()?;
        if id.is_some_and(|id| {
            !registry
                .accounts
                .iter()
                .any(|account| account.provider_account_id() == *id)
        }) {
            return Err(StoreError::InvalidData);
        }
        registry.active = id.cloned();
        self.inner.save_registry(&registry)
    }

    pub fn remove_account(&self, id: &ProviderAccountId) -> Result<(), StoreError> {
        let _registry_guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
        let lock = self.account_lock(id)?;
        let _account_guard = lock.lock().map_err(|_| StoreError::Unavailable)?;
        let mut registry = self.inner.load_registry()?;
        registry.validate()?;
        let was_pending = registry.pending_secret_deletions.contains(id);
        let original_len = registry.accounts.len();
        registry
            .accounts
            .retain(|account| account.provider_account_id() != *id);
        if original_len == registry.accounts.len() && !was_pending {
            return if self.inner.load(id)?.is_none() {
                Ok(())
            } else {
                Err(StoreError::InvalidData)
            };
        }
        if registry.active.as_ref() == Some(id) {
            registry.active = None;
        }
        if !was_pending {
            registry.pending_secret_deletions.push(id.clone());
            self.inner.save_registry(&registry)?;
        }
        self.inner.delete(id)?;
        registry
            .pending_secret_deletions
            .retain(|pending| pending != id);
        self.inner.save_registry(&registry)
    }

    /// Retain confirmed identity and references while deleting only this role's secret.
    pub fn clear_account_credentials(&self, id: &ProviderAccountId) -> Result<(), StoreError> {
        let _registry_guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
        if !self
            .load_registry_cleaned()?
            .accounts
            .iter()
            .any(|account| account.provider_account_id() == *id)
        {
            return Err(StoreError::InvalidData);
        }
        let lock = self.account_lock(id)?;
        let _guard = lock.lock().map_err(|_| StoreError::Unavailable)?;
        self.inner.delete(id)
    }
    pub fn restore_active_account(&self) -> Result<Option<RestoredCredentials>, StoreError> {
        let active = {
            let _guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
            self.load_registry_cleaned()?.active
        };
        match active {
            Some(id) => self.restore_account(&id),
            None => Ok(None),
        }
    }

    pub fn replace_active_account(
        &self,
        account: &ActiveAccount,
        pair: &TokenPair,
    ) -> Result<(), StoreError> {
        self.save_account(account, pair, true)
    }

    pub fn disconnect(&self, account: &ActiveAccount) -> Result<(), StoreError> {
        let id = account.provider_account_id();
        let registry = {
            let _guard = self.registry.lock().map_err(|_| StoreError::Unavailable)?;
            self.inner.load_registry()?
        };
        if registry.active.as_ref().is_some_and(|active| active != &id)
            || (registry.active.is_none()
                && registry
                    .accounts
                    .iter()
                    .any(|stored| stored.provider_account_id() == id))
        {
            return Err(StoreError::InvalidData);
        }
        self.remove_account(&id)
    }

    pub fn refresh_active_if_needed<F>(
        &self,
        now: SystemTime,
        refresh: F,
    ) -> Result<Option<RestoredCredentials>, RotationError>
    where
        F: FnOnce(&TokenPair) -> Result<TokenPair, RotationError>,
    {
        let active = {
            let _guard = self
                .registry
                .lock()
                .map_err(|_| RotationError::Store(StoreError::Unavailable))?;
            self.load_registry_cleaned()
                .map_err(RotationError::Store)?
                .active
        };
        let Some(id) = active else {
            return Ok(None);
        };
        let pair = self.refresh_if_needed(&id, now, refresh)?;
        let account = self
            .accounts()
            .map_err(RotationError::Store)?
            .into_iter()
            .find(|account| account.provider_account_id() == id)
            .ok_or(RotationError::Store(StoreError::InvalidData))?;
        Ok(Some(RestoredCredentials { account, pair }))
    }

    pub fn refresh_if_needed<F>(
        &self,
        key: &CredentialKey,
        now: SystemTime,
        refresh: F,
    ) -> Result<TokenPair, RotationError>
    where
        F: FnOnce(&TokenPair) -> Result<TokenPair, RotationError>,
    {
        let lock = self.account_lock(key).map_err(RotationError::Store)?;
        let _guard = lock
            .lock()
            .map_err(|_| RotationError::Store(StoreError::Unavailable))?;
        let current = self
            .inner
            .load(key)
            .map_err(RotationError::Store)?
            .ok_or(RotationError::ReconnectRequired)?;
        if current.refresh_is_expired(now) {
            return Err(RotationError::ReconnectRequired);
        }
        if !current.access_is_expired(now) {
            return Ok(current);
        }
        let replacement = refresh(&current)?;
        self.inner
            .save(key, &replacement)
            .map_err(RotationError::Store)?;
        Ok(replacement)
    }

    pub fn rotate<F>(&self, key: &CredentialKey, refresh: F) -> Result<TokenPair, RotationError>
    where
        F: FnOnce(&TokenPair) -> Result<TokenPair, RotationError>,
    {
        let lock = self.account_lock(key).map_err(RotationError::Store)?;
        let _guard = lock
            .lock()
            .map_err(|_| RotationError::Store(StoreError::Unavailable))?;
        let current = self
            .inner
            .load(key)
            .map_err(RotationError::Store)?
            .ok_or(RotationError::ReconnectRequired)?;
        let replacement = refresh(&current)?;
        self.inner
            .save(key, &replacement)
            .map_err(RotationError::Store)?;
        Ok(replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[derive(Clone, Default)]
    struct MemoryStore {
        registry: Arc<Mutex<AccountRegistry>>,
        credentials: Arc<Mutex<BTreeMap<ProviderAccountId, TokenPair>>>,
    }

    impl CredentialStore for MemoryStore {
        fn load(&self, key: &CredentialKey) -> Result<Option<TokenPair>, StoreError> {
            Ok(self.credentials.lock().unwrap().get(key).cloned())
        }

        fn save(&self, key: &CredentialKey, pair: &TokenPair) -> Result<(), StoreError> {
            self.credentials
                .lock()
                .unwrap()
                .insert(key.clone(), pair.clone());
            Ok(())
        }

        fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
            self.credentials.lock().unwrap().remove(key);
            Ok(())
        }
    }

    impl AccountRegistryStore for MemoryStore {
        fn load_registry(&self) -> Result<AccountRegistry, StoreError> {
            Ok(self.registry.lock().unwrap().clone())
        }

        fn save_registry(&self, registry: &AccountRegistry) -> Result<(), StoreError> {
            *self.registry.lock().unwrap() = registry.clone();
            Ok(())
        }
    }

    fn account(id: &str, login: &str) -> ActiveAccount {
        ActiveAccount::new(id, login).unwrap()
    }

    fn pair(access: &str, refresh: &str) -> TokenPair {
        TokenPair::new(
            access,
            refresh,
            Duration::from_secs(28_800),
            Duration::from_secs(15_552_000),
        )
    }

    #[test]
    fn two_accounts_survive_store_restart() {
        let storage = MemoryStore::default();
        let first = account("42", "octocat");
        let second = account("84", "hubot");
        let store = RotationSafeStore::new(storage.clone());

        store.save_account(&first, &pair("a1", "r1"), true).unwrap();
        store
            .save_account(&second, &pair("a2", "r2"), false)
            .unwrap();

        let restarted = RotationSafeStore::new(storage);
        assert_eq!(
            restarted.accounts().unwrap(),
            vec![first.clone(), second.clone()]
        );
        assert_eq!(
            restarted
                .restore_account(&first.provider_account_id())
                .unwrap()
                .unwrap()
                .pair
                .access_token(),
            "a1"
        );
        assert_eq!(
            restarted
                .restore_account(&second.provider_account_id())
                .unwrap()
                .unwrap()
                .pair
                .access_token(),
            "a2"
        );
        assert_eq!(
            restarted.restore_active_account().unwrap().unwrap().account,
            first
        );
    }

    #[test]
    fn rotation_and_removal_are_isolated_by_provider_account() {
        let storage = MemoryStore::default();
        let first = account("42", "octocat");
        let second = account("84", "hubot");
        let store = RotationSafeStore::new(storage);
        store.save_account(&first, &pair("a1", "r1"), true).unwrap();
        store
            .save_account(&second, &pair("a2", "r2"), false)
            .unwrap();

        store
            .rotate(&first.provider_account_id(), |_| {
                Ok(pair("a1-next", "r1-next"))
            })
            .unwrap();
        assert_eq!(
            store
                .restore_account(&second.provider_account_id())
                .unwrap()
                .unwrap()
                .pair
                .refresh_token(),
            "r2"
        );

        store.remove_account(&first.provider_account_id()).unwrap();
        assert!(store
            .restore_account(&first.provider_account_id())
            .unwrap()
            .is_none());
        assert_eq!(store.accounts().unwrap(), vec![second.clone()]);
        assert!(store.restore_active_account().unwrap().is_none());
        assert_eq!(
            store
                .restore_account(&second.provider_account_id())
                .unwrap()
                .unwrap()
                .pair
                .access_token(),
            "a2"
        );
    }

    #[test]
    fn provider_account_identifiers_do_not_conflate_providers() {
        let github = ProviderAccountId::github("42");
        let future_provider =
            ProviderAccountId::new(ProviderId::new("azure-devops").unwrap(), "42").unwrap();

        assert_ne!(github, future_provider);
        assert_eq!(github.provider().as_str(), "github");
        assert_eq!(github.account_id(), "42");
    }
}
