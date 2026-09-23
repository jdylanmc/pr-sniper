use super::device_flow::TokenPair;
use std::{sync::Mutex, time::SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provider {
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CredentialKey {
    pub provider: Provider,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveAccount {
    pub account_id: String,
    pub login: String,
}

impl ActiveAccount {
    pub fn new(
        account_id: impl Into<String>,
        login: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let account = Self {
            account_id: account_id.into(),
            login: login.into(),
        };
        if account.account_id.is_empty()
            || account.account_id.len() > 128
            || account.login.is_empty()
            || account.login.len() > 128
            || account.login.chars().any(char::is_whitespace)
        {
            return Err(StoreError::InvalidData);
        }
        Ok(account)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredCredentials {
    pub account: ActiveAccount,
    pub pair: TokenPair,
}

impl CredentialKey {
    pub fn github(account_id: impl Into<String>) -> Self {
        Self {
            provider: Provider::Github,
            account_id: account_id.into(),
        }
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
    rotation: Mutex<()>,
}

impl<S> RotationSafeStore<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            rotation: Mutex::new(()),
        }
    }

    pub fn inner(&self) -> &S {
        &self.inner
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

impl<S: ActiveCredentialStore + CredentialStore> RotationSafeStore<S> {
    pub fn restore_active_account(&self) -> Result<Option<RestoredCredentials>, StoreError> {
        let _guard = self.rotation.lock().map_err(|_| StoreError::Unavailable)?;
        self.inner.load_active_credentials()
    }

    pub fn replace_active_account(
        &self,
        account: &ActiveAccount,
        pair: &TokenPair,
    ) -> Result<(), StoreError> {
        let _guard = self.rotation.lock().map_err(|_| StoreError::Unavailable)?;
        self.inner.save_active_credentials(&RestoredCredentials {
            account: account.clone(),
            pair: pair.clone(),
        })
    }

    pub fn disconnect(&self, account: &ActiveAccount) -> Result<(), StoreError> {
        let _guard = self.rotation.lock().map_err(|_| StoreError::Unavailable)?;
        let active = self
            .inner
            .load_active_credentials()?
            .ok_or(StoreError::InvalidData)?;
        if active.account.account_id != account.account_id {
            return Err(StoreError::InvalidData);
        }
        self.inner.delete_active_credentials()
    }

    pub fn refresh_active_if_needed<F>(
        &self,
        now: SystemTime,
        refresh: F,
    ) -> Result<Option<RestoredCredentials>, RotationError>
    where
        F: FnOnce(&TokenPair) -> Result<TokenPair, RotationError>,
    {
        let _guard = self
            .rotation
            .lock()
            .map_err(|_| RotationError::Store(StoreError::Unavailable))?;
        let Some(mut active) = self
            .inner
            .load_active_credentials()
            .map_err(RotationError::Store)?
        else {
            return Ok(None);
        };
        if active.pair.refresh_is_expired(now) {
            return Err(RotationError::ReconnectRequired);
        }
        if active.pair.access_is_expired(now) {
            active.pair = refresh(&active.pair)?;
            self.inner
                .save_active_credentials(&active)
                .map_err(RotationError::Store)?;
        }
        Ok(Some(active))
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
        let _guard = self
            .rotation
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
        let _guard = self
            .rotation
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
