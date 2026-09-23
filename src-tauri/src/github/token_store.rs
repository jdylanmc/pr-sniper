use super::device_flow::TokenPair;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provider {
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CredentialKey {
    pub provider: Provider,
    pub account_id: String,
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

impl<S: CredentialStore> RotationSafeStore<S> {
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
