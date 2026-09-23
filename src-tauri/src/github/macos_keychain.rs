use super::{
    device_flow::TokenPair,
    token_store::{
        ActiveAccount, ActiveCredentialStore, CredentialKey, CredentialStore, Provider,
        RestoredCredentials, StoreError,
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
const ACTIVE_ACCOUNT: &str = "github:active-account";
const ACTIVE_RECORD_VERSION: &[u8; 8] = b"PRSNAUTH";

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
        Self::with_service("com.jdylanmc.pr-sniper.github.credentials")
    }

    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn account(key: &CredentialKey) -> Result<String, StoreError> {
        if key.account_id.is_empty() || key.account_id.len() > 128 {
            return Err(StoreError::InvalidData);
        }
        match key.provider {
            Provider::Github => Ok(format!("github:{}", key.account_id)),
        }
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
        load_active_credentials(self)
    }

    fn save_active_credentials(&self, credentials: &RestoredCredentials) -> Result<(), StoreError> {
        save_active_credentials(self, credentials)
    }

    fn delete_active_credentials(&self) -> Result<(), StoreError> {
        delete_active_credentials(self)
    }
}

fn legacy_account_name(account_id: &str) -> Result<String, StoreError> {
    MacKeychainStore::account(&CredentialKey::github(account_id))
}

fn load_active_credentials(
    backend: &impl ActiveRecordBackend,
) -> Result<Option<RestoredCredentials>, StoreError> {
    let Some(bytes) = backend.load_record(ACTIVE_ACCOUNT)? else {
        return Ok(None);
    };
    let mut stored = if let Ok(stored) = decode_stored_active_credentials(&bytes) {
        stored
    } else if let Ok(credentials) = decode_active_credentials(&bytes) {
        let stored = StoredActiveCredentials {
            legacy_cleanup_account_id: Some(credentials.account.account_id.clone()),
            credentials,
        };
        backend.save_record(ACTIVE_ACCOUNT, &encode_stored_active_credentials(&stored)?)?;
        stored
    } else {
        let account = decode_legacy_active_account(&bytes)?;
        let legacy_name = legacy_account_name(&account.account_id)?;
        let pair = backend
            .load_record(&legacy_name)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .ok_or(StoreError::InvalidData)?;
        let stored = StoredActiveCredentials {
            legacy_cleanup_account_id: Some(account.account_id.clone()),
            credentials: RestoredCredentials { account, pair },
        };
        backend.save_record(ACTIVE_ACCOUNT, &encode_stored_active_credentials(&stored)?)?;
        stored
    };
    finish_legacy_cleanup(backend, &mut stored)?;
    Ok(Some(stored.credentials))
}

fn save_active_credentials(
    backend: &impl ActiveRecordBackend,
    credentials: &RestoredCredentials,
) -> Result<(), StoreError> {
    let pending = backend.load_record(ACTIVE_ACCOUNT)?.and_then(|bytes| {
        decode_stored_active_credentials(&bytes)
            .ok()
            .and_then(|stored| stored.legacy_cleanup_account_id)
            .or_else(|| {
                decode_active_credentials(&bytes)
                    .ok()
                    .map(|credentials| credentials.account.account_id)
            })
            .or_else(|| {
                decode_legacy_active_account(&bytes)
                    .ok()
                    .map(|account| account.account_id)
            })
    });
    let mut stored = StoredActiveCredentials {
        credentials: credentials.clone(),
        legacy_cleanup_account_id: pending,
    };
    backend.save_record(ACTIVE_ACCOUNT, &encode_stored_active_credentials(&stored)?)?;
    finish_legacy_cleanup(backend, &mut stored)
}

fn delete_active_credentials(backend: &impl ActiveRecordBackend) -> Result<(), StoreError> {
    if let Err(error) = load_active_credentials(backend) {
        if error != StoreError::InvalidData {
            return Err(error);
        }
    }
    backend.delete_record(ACTIVE_ACCOUNT)
}

fn finish_legacy_cleanup(
    backend: &impl ActiveRecordBackend,
    stored: &mut StoredActiveCredentials,
) -> Result<(), StoreError> {
    let Some(account_id) = stored.legacy_cleanup_account_id.clone() else {
        return Ok(());
    };
    backend.delete_record(&legacy_account_name(&account_id)?)?;
    stored.legacy_cleanup_account_id = None;
    backend.save_record(ACTIVE_ACCOUNT, &encode_stored_active_credentials(stored)?)
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
struct StoredActiveCredentials {
    credentials: RestoredCredentials,
    legacy_cleanup_account_id: Option<String>,
}

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
        backend.insert(ACTIVE_ACCOUNT, &marker);
        let legacy_name = legacy_account_name(&credentials.account.account_id).unwrap();
        backend.insert(&legacy_name, &encode(&credentials.pair).unwrap());
        legacy_name
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
        store.save(&key, &pair).unwrap();
        let id = account.account_id.as_bytes();
        let login = account.login.as_bytes();
        let mut marker = Zeroizing::new(Vec::new());
        marker.extend_from_slice(&checked_length(id).unwrap().to_be_bytes());
        marker.extend_from_slice(id);
        marker.extend_from_slice(&checked_length(login).unwrap().to_be_bytes());
        marker.extend_from_slice(login);
        store.save_bytes(ACTIVE_ACCOUNT, &marker).unwrap();

        let restored = store.load_active_credentials().unwrap().unwrap();

        assert_eq!(restored.account, account);
        assert_eq!(restored.pair.access_token(), "legacy-access");
        assert!(store.load(&key).unwrap().is_none());
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
    }

    #[test]
    fn migration_write_failure_preserves_legacy_records_and_retries() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_save_at.set(Some(1));

        assert_eq!(
            load_active_credentials(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        assert!(backend.contains(ACTIVE_ACCOUNT));
        assert!(backend.contains(&legacy_name));

        assert_credentials(load_active_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
    }

    #[test]
    fn unversioned_combined_record_removes_the_matching_legacy_pair() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        backend.insert(
            ACTIVE_ACCOUNT,
            &encode_active_credentials(&expected).unwrap(),
        );
        let legacy_name = legacy_account_name(&expected.account.account_id).unwrap();
        backend.insert(&legacy_name, &encode(&expected.pair).unwrap());

        assert_credentials(load_active_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
        let stored = decode_stored_active_credentials(
            &backend.load_record(ACTIVE_ACCOUNT).unwrap().unwrap(),
        )
        .unwrap();
        assert!(stored.legacy_cleanup_account_id.is_none());
    }

    #[test]
    fn migration_delete_failure_keeps_pending_cleanup_and_retries() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_delete_at.set(Some(1));

        assert_eq!(
            load_active_credentials(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        let stored = decode_stored_active_credentials(
            &backend.load_record(ACTIVE_ACCOUNT).unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(
            stored.legacy_cleanup_account_id.as_deref(),
            Some(expected.account.account_id.as_str())
        );
        assert!(backend.contains(&legacy_name));

        assert_credentials(load_active_credentials(&backend).unwrap(), &expected);
        assert!(!backend.contains(&legacy_name));
    }

    #[test]
    fn migration_finalization_failure_retries_without_an_orphan() {
        let backend = MemoryBackend::default();
        let expected = credentials("42", "octocat");
        let legacy_name = seed_legacy(&backend, &expected);
        backend.fail_save_at.set(Some(2));

        assert_eq!(
            load_active_credentials(&backend).unwrap_err(),
            StoreError::Unavailable
        );
        assert!(!backend.contains(&legacy_name));
        assert_credentials(load_active_credentials(&backend).unwrap(), &expected);
        let stored = decode_stored_active_credentials(
            &backend.load_record(ACTIVE_ACCOUNT).unwrap().unwrap(),
        )
        .unwrap();
        assert!(stored.legacy_cleanup_account_id.is_none());
    }

    #[test]
    fn account_switch_and_disconnect_finish_pending_legacy_cleanup() {
        let backend = MemoryBackend::default();
        let legacy = credentials("42", "octocat");
        let replacement = credentials("84", "hubot");
        let legacy_name = seed_legacy(&backend, &legacy);
        backend.fail_delete_at.set(Some(1));
        assert_eq!(
            load_active_credentials(&backend).unwrap_err(),
            StoreError::Unavailable
        );

        save_active_credentials(&backend, &replacement).unwrap();
        assert!(!backend.contains(&legacy_name));
        assert_credentials(load_active_credentials(&backend).unwrap(), &replacement);

        delete_active_credentials(&backend).unwrap();
        assert!(!backend.contains(ACTIVE_ACCOUNT));
        assert!(!backend.contains(&legacy_name));
    }

    #[test]
    fn disconnect_removes_an_unreadable_active_record() {
        let backend = MemoryBackend::default();
        backend.insert(ACTIVE_ACCOUNT, b"corrupt");

        delete_active_credentials(&backend).unwrap();

        assert!(!backend.contains(ACTIVE_ACCOUNT));
    }
}
