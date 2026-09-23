use super::{
    device_flow::TokenPair,
    token_store::{
        ActiveAccount, ActiveAccountStore, CredentialKey, CredentialStore, Provider, StoreError,
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

impl ActiveAccountStore for MacKeychainStore {
    fn load_active_account(&self) -> Result<Option<ActiveAccount>, StoreError> {
        self.load_bytes(ACTIVE_ACCOUNT)?
            .map(|bytes| decode_active_account(&bytes))
            .transpose()
    }

    fn save_active_account(&self, account: &ActiveAccount) -> Result<(), StoreError> {
        self.save_bytes(ACTIVE_ACCOUNT, &encode_active_account(account)?)
    }

    fn delete_active_account(&self) -> Result<(), StoreError> {
        self.delete_named(ACTIVE_ACCOUNT)
    }
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

fn encode_active_account(account: &ActiveAccount) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let id = account.account_id.as_bytes();
    let login = account.login.as_bytes();
    let mut bytes = Zeroizing::new(Vec::with_capacity(8 + id.len() + login.len()));
    bytes.extend_from_slice(&checked_length(id)?.to_be_bytes());
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&checked_length(login)?.to_be_bytes());
    bytes.extend_from_slice(login);
    Ok(bytes)
}

fn decode_active_account(bytes: &[u8]) -> Result<ActiveAccount, StoreError> {
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
