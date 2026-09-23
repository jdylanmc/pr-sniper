use super::{
    oauth::TokenPair,
    token_store::{
        AccountRegistry, AccountRegistryStore, ActiveAccount, ActiveCredentialStore, CredentialKey,
        CredentialStore, ProviderAccountId, ProviderId, RestoredCredentials, StoreError,
    },
};
use std::{
    ffi::{c_char, c_void},
    ptr,
    time::{Duration, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};

type OsStatus = i32;
type SecKeychainItemRef = *mut c_void;

const ERR_SEC_ITEM_NOT_FOUND: OsStatus = -25300;
const LEGACY_ACTIVE_ACCOUNT: &str = "github:active-account";
const ACCOUNT_REGISTRY: &str = "accounts:registry";
const REGISTRY_RECORD_VERSION: &[u8; 8] = b"PRSNREG1";
const ACTIVE_RECORD_VERSION: &[u8; 8] = b"PRSNAUTH";
const OAUTH_APP_SERVICE: &str = "com.jdylanmc.pr-sniper.github.oauth-app.v1";
const LEGACY_GITHUB_APP_SERVICE: &str = "com.jdylanmc.pr-sniper.github.credentials";

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecKeychainFindGenericPassword(
        keychain_or_array: *const c_void,
        service_name_length: u32,
        service_name: *const c_char,
        account_name_length: u32,
        account_name: *const c_char,
        password_length: *mut u32,
        password_data: *mut *mut c_void,
        item_ref: *mut SecKeychainItemRef,
    ) -> OsStatus;
    fn SecKeychainAddGenericPassword(
        keychain: *const c_void,
        service_name_length: u32,
        service_name: *const c_char,
        account_name_length: u32,
        account_name: *const c_char,
        password_length: u32,
        password_data: *const c_void,
        item_ref: *mut SecKeychainItemRef,
    ) -> OsStatus;
    fn SecKeychainItemModifyAttributesAndData(
        item_ref: SecKeychainItemRef,
        attr_list: *const c_void,
        length: u32,
        data: *const c_void,
    ) -> OsStatus;
    fn SecKeychainItemDelete(item_ref: SecKeychainItemRef) -> OsStatus;
    fn SecKeychainItemFreeContent(attr_list: *const c_void, data: *mut c_void) -> OsStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}

pub struct MacKeychainStore {
    service: String,
}

impl MacKeychainStore {
    pub fn production() -> Self {
        Self::with_service(OAUTH_APP_SERVICE)
    }

    pub fn legacy_production() -> Self {
        Self::with_service(LEGACY_GITHUB_APP_SERVICE)
    }

    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn account(key: &CredentialKey) -> Result<String, StoreError> {
        ProviderAccountId::new(key.provider().clone(), key.account_id().to_string())?;
        Ok(format!(
            "account:{}:{}",
            key.provider().as_str(),
            key.account_id()
        ))
    }

    fn item(
        &self,
        account: &str,
        include_password: bool,
    ) -> Result<Option<KeychainItem>, StoreError> {
        let service = self.service.as_bytes();
        let account = account.as_bytes();
        let mut length = 0;
        let mut data = ptr::null_mut();
        let mut item = ptr::null_mut();
        let status = unsafe {
            SecKeychainFindGenericPassword(
                ptr::null(),
                checked_length(service)?,
                service.as_ptr().cast(),
                checked_length(account)?,
                account.as_ptr().cast(),
                if include_password {
                    &mut length
                } else {
                    ptr::null_mut()
                },
                if include_password {
                    &mut data
                } else {
                    ptr::null_mut()
                },
                &mut item,
            )
        };
        if status == ERR_SEC_ITEM_NOT_FOUND {
            return Ok(None);
        }
        if status != 0 || item.is_null() {
            return Err(StoreError::Unavailable);
        }
        let password = if include_password {
            if data.is_null() {
                release(item);
                return Err(StoreError::InvalidData);
            }
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length as usize) };
            let copy = Zeroizing::new(bytes.to_vec());
            let free_status = unsafe { SecKeychainItemFreeContent(ptr::null(), data) };
            if free_status != 0 {
                release(item);
                return Err(StoreError::Unavailable);
            }
            Some(copy)
        } else {
            None
        };
        Ok(Some(KeychainItem {
            reference: item,
            password,
        }))
    }

    fn load_bytes(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError> {
        self.item(account, true)?
            .map(|mut item| item.password.take().ok_or(StoreError::InvalidData))
            .transpose()
    }

    fn save_bytes(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        if let Some(item) = self.item(account, false)? {
            return status_result(unsafe {
                SecKeychainItemModifyAttributesAndData(
                    item.reference,
                    ptr::null(),
                    checked_length(secret)?,
                    secret.as_ptr().cast(),
                )
            });
        }
        let service = self.service.as_bytes();
        let account = account.as_bytes();
        status_result(unsafe {
            SecKeychainAddGenericPassword(
                ptr::null(),
                checked_length(service)?,
                service.as_ptr().cast(),
                checked_length(account)?,
                account.as_ptr().cast(),
                checked_length(secret)?,
                secret.as_ptr().cast(),
                ptr::null_mut(),
            )
        })
    }

    fn delete_named(&self, account: &str) -> Result<(), StoreError> {
        let Some(item) = self.item(account, false)? else {
            return Ok(());
        };
        status_result(unsafe { SecKeychainItemDelete(item.reference) })
    }
}

trait ActiveRecordBackend {
    fn load_record(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError>;
    fn save_record(&self, account: &str, secret: &[u8]) -> Result<(), StoreError>;
    fn delete_record(&self, account: &str) -> Result<(), StoreError>;
}

impl ActiveRecordBackend for MacKeychainStore {
    fn load_record(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError> {
        self.load_bytes(account)
    }

    fn save_record(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        self.save_bytes(account, secret)
    }

    fn delete_record(&self, account: &str) -> Result<(), StoreError> {
        self.delete_named(account)
    }
}

impl CredentialStore for MacKeychainStore {
    fn load(&self, key: &CredentialKey) -> Result<Option<TokenPair>, StoreError> {
        let account = Self::account(key)?;
        self.load_bytes(&account)?
            .map(|bytes| decode(&bytes))
            .transpose()
    }

    fn save(&self, key: &CredentialKey, pair: &TokenPair) -> Result<(), StoreError> {
        let account = Self::account(key)?;
        let secret = encode(pair)?;
        self.save_bytes(&account, &secret)
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
        let account = Self::account(key)?;
        self.delete_named(&account)
    }
}

impl ActiveCredentialStore for MacKeychainStore {
    fn load_active_credentials(&self) -> Result<Option<RestoredCredentials>, StoreError> {
        let registry = self.load_registry()?;
        let Some(active) = registry.active else {
            return Ok(None);
        };
        let account = registry
            .accounts
            .into_iter()
            .find(|account| account.provider_account_id() == active)
            .ok_or(StoreError::InvalidData)?;
        let pair = self.load(&active)?.ok_or(StoreError::InvalidData)?;
        Ok(Some(RestoredCredentials { account, pair }))
    }

    fn save_active_credentials(&self, credentials: &RestoredCredentials) -> Result<(), StoreError> {
        let id = credentials.account.provider_account_id();
        self.save(&id, &credentials.pair)?;
        let mut registry = self.load_registry()?;
        if let Some(account) = registry
            .accounts
            .iter_mut()
            .find(|account| account.provider_account_id() == id)
        {
            *account = credentials.account.clone();
        } else {
            registry.accounts.push(credentials.account.clone());
        }
        registry.active = Some(id);
        self.save_registry(&registry)
    }

    fn delete_active_credentials(&self) -> Result<(), StoreError> {
        let mut registry = self.load_registry()?;
        let Some(active) = registry.active.take() else {
            return Ok(());
        };
        registry
            .accounts
            .retain(|account| account.provider_account_id() != active);
        registry.pending_secret_deletions.push(active.clone());
        self.save_registry(&registry)?;
        self.delete(&active)?;
        registry.pending_secret_deletions.clear();
        self.save_registry(&registry)
    }
}

fn legacy_account_name(account_id: &str) -> Result<String, StoreError> {
    let id = ProviderAccountId::github(account_id);
    Ok(format!("{}:{}", id.provider().as_str(), id.account_id()))
}

impl AccountRegistryStore for MacKeychainStore {
    fn load_registry(&self) -> Result<AccountRegistry, StoreError> {
        load_registry(self)
    }

    fn save_registry(&self, registry: &AccountRegistry) -> Result<(), StoreError> {
        registry.validate()?;
        let legacy_cleanup = self
            .load_record(ACCOUNT_REGISTRY)?
            .map(|bytes| decode_stored_registry(&bytes))
            .transpose()?
            .map(|stored| stored.legacy_cleanup)
            .unwrap_or_default();
        self.save_record(
            ACCOUNT_REGISTRY,
            &encode_stored_registry(&StoredRegistry {
                registry: registry.clone(),
                legacy_cleanup,
            })?,
        )
    }
}

fn load_registry(backend: &impl ActiveRecordBackend) -> Result<AccountRegistry, StoreError> {
    if let Some(bytes) = backend.load_record(ACCOUNT_REGISTRY)? {
        let mut stored = decode_stored_registry(&bytes)?;
        finish_legacy_cleanup(backend, &mut stored)?;
        return Ok(stored.registry);
    }
    let Some(bytes) = backend.load_record(LEGACY_ACTIVE_ACCOUNT)? else {
        return Ok(AccountRegistry::default());
    };
    let (credentials, mut legacy_cleanup) =
        if let Ok(stored) = decode_stored_active_credentials(&bytes) {
            let mut cleanup = vec![LEGACY_ACTIVE_ACCOUNT.to_string()];
            if let Some(account_id) = stored.legacy_cleanup_account_id {
                cleanup.push(legacy_account_name(&account_id)?);
            }
            (stored.credentials, cleanup)
        } else if let Ok(credentials) = decode_active_credentials(&bytes) {
            let cleanup = vec![
                LEGACY_ACTIVE_ACCOUNT.to_string(),
                legacy_account_name(&credentials.account.account_id)?,
            ];
            (credentials, cleanup)
        } else {
            let account = decode_legacy_active_account(&bytes)?;
            let legacy_name = legacy_account_name(&account.account_id)?;
            let pair = backend
                .load_record(&legacy_name)?
                .map(|bytes| decode(&bytes))
                .transpose()?
                .ok_or(StoreError::InvalidData)?;
            (
                RestoredCredentials { account, pair },
                vec![LEGACY_ACTIVE_ACCOUNT.to_string(), legacy_name],
            )
        };
    legacy_cleanup.sort();
    legacy_cleanup.dedup();
    let id = credentials.account.provider_account_id();
    backend.save_record(
        &MacKeychainStore::account(&id)?,
        &encode(&credentials.pair)?,
    )?;
    let mut stored = StoredRegistry {
        registry: AccountRegistry {
            accounts: vec![credentials.account],
            active: Some(id),
            pending_account_additions: Vec::new(),
            pending_secret_deletions: Vec::new(),
        },
        legacy_cleanup,
    };
    backend.save_record(ACCOUNT_REGISTRY, &encode_stored_registry(&stored)?)?;
    finish_legacy_cleanup(backend, &mut stored)?;
    Ok(stored.registry)
}

fn finish_legacy_cleanup(
    backend: &impl ActiveRecordBackend,
    stored: &mut StoredRegistry,
) -> Result<(), StoreError> {
    if stored.legacy_cleanup.is_empty() {
        return Ok(());
    }
    for account in &stored.legacy_cleanup {
        backend.delete_record(account)?;
    }
    stored.legacy_cleanup.clear();
    backend.save_record(ACCOUNT_REGISTRY, &encode_stored_registry(stored)?)
}

struct KeychainItem {
    reference: SecKeychainItemRef,
    password: Option<Zeroizing<Vec<u8>>>,
}

impl Drop for KeychainItem {
    fn drop(&mut self) {
        release(self.reference);
    }
}

fn release(reference: SecKeychainItemRef) {
    if !reference.is_null() {
        unsafe { CFRelease(reference) };
    }
}

fn status_result(status: OsStatus) -> Result<(), StoreError> {
    if status == 0 {
        Ok(())
    } else {
        Err(StoreError::Unavailable)
    }
}

fn checked_length(bytes: &[u8]) -> Result<u32, StoreError> {
    bytes.len().try_into().map_err(|_| StoreError::InvalidData)
}

fn encode(pair: &TokenPair) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let access = pair.access_token().as_bytes();
    let refresh = pair.refresh_token().as_bytes();
    let mut bytes = Zeroizing::new(Vec::with_capacity(24 + access.len() + refresh.len()));
    bytes.extend_from_slice(&checked_length(access)?.to_be_bytes());
    bytes.extend_from_slice(access);
    bytes.extend_from_slice(&checked_length(refresh)?.to_be_bytes());
    bytes.extend_from_slice(refresh);
    bytes.extend_from_slice(
        &pair
            .access_expires_at()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::InvalidData)?
            .as_secs()
            .to_be_bytes(),
    );
    bytes.extend_from_slice(
        &pair
            .refresh_expires_at()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::InvalidData)?
            .as_secs()
            .to_be_bytes(),
    );
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<TokenPair, StoreError> {
    let mut cursor = 0;
    let access = take_string(bytes, &mut cursor)?;
    let refresh = take_string(bytes, &mut cursor)?;
    let access_expires_at = take_u64(bytes, &mut cursor)?;
    let refresh_expires_at = take_u64(bytes, &mut cursor)?;
    if cursor != bytes.len() || access.is_empty() || refresh.is_empty() {
        return Err(StoreError::InvalidData);
    }
    let pair = TokenPair::from_expirations(
        access.as_str(),
        refresh.as_str(),
        UNIX_EPOCH + Duration::from_secs(access_expires_at),
        UNIX_EPOCH + Duration::from_secs(refresh_expires_at),
    );
    Ok(pair)
}

#[cfg(test)]
fn encode_active_credentials(
    credentials: &RestoredCredentials,
) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let id = credentials.account.account_id.as_bytes();
    let login = credentials.account.login.as_bytes();
    let pair = encode(&credentials.pair)?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(12 + id.len() + login.len() + pair.len()));
    bytes.extend_from_slice(&checked_length(id)?.to_be_bytes());
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&checked_length(login)?.to_be_bytes());
    bytes.extend_from_slice(login);
    bytes.extend_from_slice(&checked_length(&pair)?.to_be_bytes());
    bytes.extend_from_slice(&pair);
    Ok(bytes)
}

fn decode_active_credentials(bytes: &[u8]) -> Result<RestoredCredentials, StoreError> {
    let mut cursor = 0;
    let id = take_string(bytes, &mut cursor)?;
    let login = take_string(bytes, &mut cursor)?;
    let pair_length = take_u32(bytes, &mut cursor)? as usize;
    let pair_end = cursor
        .checked_add(pair_length)
        .ok_or(StoreError::InvalidData)?;
    let pair = decode(bytes.get(cursor..pair_end).ok_or(StoreError::InvalidData)?)?;
    cursor = pair_end;
    if cursor != bytes.len() {
        return Err(StoreError::InvalidData);
    }
    Ok(RestoredCredentials {
        account: ActiveAccount::new(id.as_str(), login.as_str())?,
        pair,
    })
}

#[derive(Clone)]
struct StoredRegistry {
    registry: AccountRegistry,
    legacy_cleanup: Vec<String>,
}

fn encode_stored_registry(stored: &StoredRegistry) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    stored.registry.validate()?;
    let registry = encode_registry(&stored.registry)?;
    let mut bytes = Zeroizing::new(Vec::new());
    bytes.extend_from_slice(REGISTRY_RECORD_VERSION);
    bytes.extend_from_slice(&checked_count(stored.legacy_cleanup.len())?.to_be_bytes());
    for account in &stored.legacy_cleanup {
        append_string(&mut bytes, account)?;
    }
    bytes.extend_from_slice(&checked_length(&registry)?.to_be_bytes());
    bytes.extend_from_slice(&registry);
    Ok(bytes)
}

fn decode_stored_registry(bytes: &[u8]) -> Result<StoredRegistry, StoreError> {
    if !bytes.starts_with(REGISTRY_RECORD_VERSION) {
        return Err(StoreError::InvalidData);
    }
    let mut cursor = REGISTRY_RECORD_VERSION.len();
    let cleanup_count = take_u32(bytes, &mut cursor)? as usize;
    let mut legacy_cleanup = Vec::with_capacity(cleanup_count);
    for _ in 0..cleanup_count {
        legacy_cleanup.push(take_string(bytes, &mut cursor)?.to_string());
    }
    let registry_length = take_u32(bytes, &mut cursor)? as usize;
    let end = cursor
        .checked_add(registry_length)
        .ok_or(StoreError::InvalidData)?;
    let registry = decode_registry(bytes.get(cursor..end).ok_or(StoreError::InvalidData)?)?;
    if end != bytes.len() {
        return Err(StoreError::InvalidData);
    }
    Ok(StoredRegistry {
        registry,
        legacy_cleanup,
    })
}

fn encode_registry(registry: &AccountRegistry) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let mut bytes = Zeroizing::new(Vec::new());
    bytes.extend_from_slice(&checked_count(registry.accounts.len())?.to_be_bytes());
    for account in &registry.accounts {
        append_string(&mut bytes, account.provider.as_str())?;
        append_string(&mut bytes, &account.account_id)?;
        append_string(&mut bytes, &account.login)?;
    }
    match &registry.active {
        Some(active) => {
            bytes.push(1);
            append_string(&mut bytes, active.provider().as_str())?;
            append_string(&mut bytes, active.account_id())?;
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&checked_count(registry.pending_secret_deletions.len())?.to_be_bytes());
    for pending in &registry.pending_secret_deletions {
        append_string(&mut bytes, pending.provider().as_str())?;
        append_string(&mut bytes, pending.account_id())?;
    }
    bytes
        .extend_from_slice(&checked_count(registry.pending_account_additions.len())?.to_be_bytes());
    for account in &registry.pending_account_additions {
        append_string(&mut bytes, account.provider.as_str())?;
        append_string(&mut bytes, &account.account_id)?;
        append_string(&mut bytes, &account.login)?;
    }
    Ok(bytes)
}

fn decode_registry(bytes: &[u8]) -> Result<AccountRegistry, StoreError> {
    let mut cursor = 0;
    let account_count = take_u32(bytes, &mut cursor)? as usize;
    let mut accounts = Vec::with_capacity(account_count);
    for _ in 0..account_count {
        let provider = ProviderId::new(take_string(bytes, &mut cursor)?.to_string())?;
        let account_id = take_string(bytes, &mut cursor)?;
        let login = take_string(bytes, &mut cursor)?;
        accounts.push(ActiveAccount::for_provider(
            provider,
            account_id.as_str(),
            login.as_str(),
        )?);
    }
    let active = match *bytes.get(cursor).ok_or(StoreError::InvalidData)? {
        0 => {
            cursor += 1;
            None
        }
        1 => {
            cursor += 1;
            let provider = ProviderId::new(take_string(bytes, &mut cursor)?.to_string())?;
            let account_id = take_string(bytes, &mut cursor)?;
            Some(ProviderAccountId::new(provider, account_id.as_str())?)
        }
        _ => return Err(StoreError::InvalidData),
    };
    let pending_count = take_u32(bytes, &mut cursor)? as usize;
    let mut pending_secret_deletions = Vec::with_capacity(pending_count);
    for _ in 0..pending_count {
        let provider = ProviderId::new(take_string(bytes, &mut cursor)?.to_string())?;
        let account_id = take_string(bytes, &mut cursor)?;
        pending_secret_deletions.push(ProviderAccountId::new(provider, account_id.as_str())?);
    }
    let mut pending_account_additions = Vec::new();
    if cursor < bytes.len() {
        let pending_addition_count = take_u32(bytes, &mut cursor)? as usize;
        pending_account_additions.reserve(pending_addition_count);
        for _ in 0..pending_addition_count {
            let provider = ProviderId::new(take_string(bytes, &mut cursor)?.to_string())?;
            let account_id = take_string(bytes, &mut cursor)?;
            let login = take_string(bytes, &mut cursor)?;
            pending_account_additions.push(ActiveAccount::for_provider(
                provider,
                account_id.as_str(),
                login.as_str(),
            )?);
        }
    }
    if cursor != bytes.len() {
        return Err(StoreError::InvalidData);
    }
    let registry = AccountRegistry {
        accounts,
        active,
        pending_account_additions,
        pending_secret_deletions,
    };
    registry.validate()?;
    Ok(registry)
}

fn append_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), StoreError> {
    bytes.extend_from_slice(&checked_length(value.as_bytes())?.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn checked_count(count: usize) -> Result<u32, StoreError> {
    count.try_into().map_err(|_| StoreError::InvalidData)
}

#[derive(Clone)]
struct StoredActiveCredentials {
    credentials: RestoredCredentials,
    legacy_cleanup_account_id: Option<String>,
}

#[cfg(test)]
fn encode_stored_active_credentials(
    stored: &StoredActiveCredentials,
) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let credentials = encode_active_credentials(&stored.credentials)?;
    let pending = stored
        .legacy_cleanup_account_id
        .as_deref()
        .unwrap_or_default()
        .as_bytes();
    let mut bytes = Zeroizing::new(Vec::with_capacity(
        ACTIVE_RECORD_VERSION.len() + 8 + pending.len() + credentials.len(),
    ));
    bytes.extend_from_slice(ACTIVE_RECORD_VERSION);
    bytes.extend_from_slice(&checked_length(pending)?.to_be_bytes());
    bytes.extend_from_slice(pending);
    bytes.extend_from_slice(&checked_length(&credentials)?.to_be_bytes());
    bytes.extend_from_slice(&credentials);
    Ok(bytes)
}

fn decode_stored_active_credentials(bytes: &[u8]) -> Result<StoredActiveCredentials, StoreError> {
    if !bytes.starts_with(ACTIVE_RECORD_VERSION) {
        return Err(StoreError::InvalidData);
    }
    let mut cursor = ACTIVE_RECORD_VERSION.len();
    let pending = take_string(bytes, &mut cursor)?;
    let credentials_length = take_u32(bytes, &mut cursor)? as usize;
    let end = cursor
        .checked_add(credentials_length)
        .ok_or(StoreError::InvalidData)?;
    let credentials =
        decode_active_credentials(bytes.get(cursor..end).ok_or(StoreError::InvalidData)?)?;
    if end != bytes.len() {
        return Err(StoreError::InvalidData);
    }
    Ok(StoredActiveCredentials {
        credentials,
        legacy_cleanup_account_id: (!pending.is_empty()).then(|| pending.to_string()),
    })
}

fn decode_legacy_active_account(bytes: &[u8]) -> Result<ActiveAccount, StoreError> {
    let mut cursor = 0;
    let id = take_string(bytes, &mut cursor)?;
    let login = take_string(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err(StoreError::InvalidData);
    }
    ActiveAccount::new(id.as_str(), login.as_str())
}

fn take_string(bytes: &[u8], cursor: &mut usize) -> Result<Zeroizing<String>, StoreError> {
    let length = take_u32(bytes, cursor)? as usize;
    let end = cursor.checked_add(length).ok_or(StoreError::InvalidData)?;
    let slice = bytes.get(*cursor..end).ok_or(StoreError::InvalidData)?;
    *cursor = end;
    let mut owned = slice.to_vec();
    let value = String::from_utf8(std::mem::take(&mut owned)).map_err(|error| {
        let mut bytes = error.into_bytes();
        bytes.zeroize();
        StoreError::InvalidData
    })?;
    Ok(Zeroizing::new(value))
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, StoreError> {
    let raw: [u8; 4] = take(bytes, cursor)?;
    Ok(u32::from_be_bytes(raw))
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, StoreError> {
    let raw: [u8; 8] = take(bytes, cursor)?;
    Ok(u64::from_be_bytes(raw))
}

fn take<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], StoreError> {
    let end = cursor.checked_add(N).ok_or(StoreError::InvalidData)?;
    let raw = bytes.get(*cursor..end).ok_or(StoreError::InvalidData)?;
    *cursor = end;
    raw.try_into().map_err(|_| StoreError::InvalidData)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
    };

    #[derive(Default)]
    struct MemoryBackend {
        records: RefCell<HashMap<String, Vec<u8>>>,
        save_count: Cell<usize>,
        delete_count: Cell<usize>,
        fail_save_at: Cell<Option<usize>>,
        fail_delete_at: Cell<Option<usize>>,
    }

    impl MemoryBackend {
        fn insert(&self, account: impl Into<String>, value: &[u8]) {
            self.records
                .borrow_mut()
                .insert(account.into(), value.to_vec());
        }

        fn contains(&self, account: &str) -> bool {
            self.records.borrow().contains_key(account)
        }
    }

    impl ActiveRecordBackend for MemoryBackend {
        fn load_record(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError> {
            Ok(self
                .records
                .borrow()
                .get(account)
                .cloned()
                .map(Zeroizing::new))
        }

        fn save_record(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
            let count = self.save_count.get() + 1;
            self.save_count.set(count);
            if self.fail_save_at.get() == Some(count) {
                self.fail_save_at.set(None);
                return Err(StoreError::Unavailable);
            }
            self.insert(account, secret);
            Ok(())
        }

        fn delete_record(&self, account: &str) -> Result<(), StoreError> {
            let count = self.delete_count.get() + 1;
            self.delete_count.set(count);
            if self.fail_delete_at.get() == Some(count) {
                self.fail_delete_at.set(None);
                return Err(StoreError::Unavailable);
            }
            self.records.borrow_mut().remove(account);
            Ok(())
        }
    }

    fn credentials(account_id: &str, login: &str) -> RestoredCredentials {
        RestoredCredentials {
            account: ActiveAccount::new(account_id, login).unwrap(),
            pair: TokenPair::new(
                format!("{account_id}-access"),
                format!("{account_id}-refresh"),
                Duration::from_secs(28_800),
                Duration::from_secs(15_552_000),
            ),
        }
    }

    fn seed_legacy(backend: &MemoryBackend, credentials: &RestoredCredentials) -> String {
        let id = credentials.account.account_id.as_bytes();
        let login = credentials.account.login.as_bytes();
        let mut marker = Zeroizing::new(Vec::new());
        marker.extend_from_slice(&checked_length(id).unwrap().to_be_bytes());
        marker.extend_from_slice(id);
        marker.extend_from_slice(&checked_length(login).unwrap().to_be_bytes());
        marker.extend_from_slice(login);
        backend.insert(LEGACY_ACTIVE_ACCOUNT, &marker);
        let legacy_name = legacy_account_name(&credentials.account.account_id).unwrap();
        backend.insert(&legacy_name, &encode(&credentials.pair).unwrap());
        legacy_name
    }

    fn migrated_credentials(
        backend: &MemoryBackend,
    ) -> Result<Option<RestoredCredentials>, StoreError> {
        let registry = load_registry(backend)?;
        let Some(id) = registry.active else {
            return Ok(None);
        };
        let account = registry
            .accounts
            .into_iter()
            .find(|account| account.provider_account_id() == id)
            .ok_or(StoreError::InvalidData)?;
        let pair = backend
            .load_record(&MacKeychainStore::account(&id)?)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .ok_or(StoreError::InvalidData)?;
        Ok(Some(RestoredCredentials { account, pair }))
    }

    fn assert_credentials(actual: Option<RestoredCredentials>, expected: &RestoredCredentials) {
        let actual = actual.unwrap();
        assert_eq!(actual.account, expected.account);
        assert_eq!(actual.pair.access_token(), expected.pair.access_token());
        assert_eq!(actual.pair.refresh_token(), expected.pair.refresh_token());
    }

    #[test]
    fn legacy_two_item_layout_migrates_before_deleting_the_old_secret() {
        let nonce = uuid::Uuid::new_v4();
        let store = MacKeychainStore::with_service(format!("com.jdylanmc.pr-sniper.tests.{nonce}"));
        let account = ActiveAccount::new(format!("fixture-{nonce}"), "octocat").unwrap();
        let key = CredentialKey::github(&account.account_id);
        let pair = TokenPair::new(
            "legacy-access",
            "legacy-refresh",
            Duration::from_secs(28_800),
            Duration::from_secs(15_552_000),
        );
        store
            .save_bytes(
                &legacy_account_name(&account.account_id).unwrap(),
                &encode(&pair).unwrap(),
            )
            .unwrap();
        let id = account.account_id.as_bytes();
        let login = account.login.as_bytes();
        let mut marker = Zeroizing::new(Vec::new());
        marker.extend_from_slice(&checked_length(id).unwrap().to_be_bytes());
        marker.extend_from_slice(id);
        marker.extend_from_slice(&checked_length(login).unwrap().to_be_bytes());
        marker.extend_from_slice(login);
        store.save_bytes(LEGACY_ACTIVE_ACCOUNT, &marker).unwrap();

        let restored = store.load_active_credentials().unwrap().unwrap();

        assert_eq!(restored.account, account);
        assert_eq!(restored.pair.access_token(), "legacy-access");
        assert_eq!(
            store.load(&key).unwrap().unwrap().refresh_token(),
            "legacy-refresh"
        );
        assert!(store
            .load_bytes(&legacy_account_name(&account.account_id).unwrap())
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .load_active_credentials()
                .unwrap()
                .unwrap()
                .pair
                .refresh_token(),
            "legacy-refresh"
        );
        store.delete_active_credentials().unwrap();
        assert!(store.load_active_credentials().unwrap().is_none());
        store.delete_named(ACCOUNT_REGISTRY).unwrap();
    }

    #[test]
    fn legacy_migration_credential_write_failure_preserves_source_and_retries() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_save_at.set(Some(1));

        assert_eq!(
            load_registry(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        assert!(backend.contains(LEGACY_ACTIVE_ACCOUNT));
        assert!(backend.contains(&legacy_name));
        assert!(!backend.contains(ACCOUNT_REGISTRY));

        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
        assert!(!backend.contains(LEGACY_ACTIVE_ACCOUNT));
    }

    #[test]
    fn combined_active_record_converges_to_registry_and_account_secret() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        backend.insert(
            LEGACY_ACTIVE_ACCOUNT,
            &encode_stored_active_credentials(&StoredActiveCredentials {
                credentials: expected.clone(),
                legacy_cleanup_account_id: Some(expected.account.account_id.clone()),
            })
            .unwrap(),
        );
        let legacy_name = legacy_account_name(&expected.account.account_id).unwrap();
        backend.insert(&legacy_name, &encode(&expected.pair).unwrap());

        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
        assert!(!backend.contains(LEGACY_ACTIVE_ACCOUNT));
        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
    }

    #[test]
    fn migration_registry_write_failure_preserves_legacy_source_and_retries() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_save_at.set(Some(2));

        assert_eq!(
            load_registry(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        assert!(backend.contains(LEGACY_ACTIVE_ACCOUNT));
        assert!(backend.contains(&legacy_name));
        assert!(!backend.contains(ACCOUNT_REGISTRY));

        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(LEGACY_ACTIVE_ACCOUNT));
        assert!(!backend.contains(&legacy_name));
    }

    #[test]
    fn migration_delete_failure_keeps_pending_cleanup_and_retries() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_delete_at.set(Some(1));

        assert_eq!(
            load_registry(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        let stored =
            decode_stored_registry(&backend.load_record(ACCOUNT_REGISTRY).unwrap().unwrap())
                .unwrap();
        assert!(!stored.legacy_cleanup.is_empty());
        assert!(backend.contains(&legacy_name));

        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
        assert!(!backend.contains(LEGACY_ACTIVE_ACCOUNT));
    }

    #[test]
    fn migration_finalization_failure_retries_without_an_orphan() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_save_at.set(Some(3));

        assert_eq!(
            load_registry(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        assert!(!backend.contains(&legacy_name));
        assert!(!backend.contains(LEGACY_ACTIVE_ACCOUNT));
        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
        let stored =
            decode_stored_registry(&backend.load_record(ACCOUNT_REGISTRY).unwrap().unwrap())
                .unwrap();
        assert!(stored.legacy_cleanup.is_empty());
    }

    #[test]
    fn unversioned_combined_record_also_converges() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        backend.insert(
            LEGACY_ACTIVE_ACCOUNT,
            &encode_active_credentials(&expected).unwrap(),
        );
        let legacy_name = legacy_account_name(&expected.account.account_id).unwrap();
        backend.insert(&legacy_name, &encode(&expected.pair).unwrap());

        assert_credentials(migrated_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
        assert!(!backend.contains(LEGACY_ACTIVE_ACCOUNT));
    }
}
