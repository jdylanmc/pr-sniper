use super::{
    credential_records::{checked_length, RecordBackend, RecordStore},
    token_store::StoreError,
};
use std::{
    ffi::{c_char, c_void},
    ptr,
};
use zeroize::Zeroizing;

type OsStatus = i32;
type SecKeychainItemRef = *mut c_void;
const ERR_SEC_ITEM_NOT_FOUND: OsStatus = -25300;

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

pub struct KeychainBackend {
    service: String,
}

pub type MacKeychainStore = RecordStore<KeychainBackend>;

impl MacKeychainStore {
    pub fn production() -> Self {
        Self::with_service("com.jdylanmc.pr-sniper.github.oauth-app.v1")
    }

    pub fn legacy_production() -> Self {
        Self::with_service("com.jdylanmc.pr-sniper.github.credentials")
    }

    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            backend: KeychainBackend {
                service: service.into(),
            },
        }
    }
}

impl KeychainBackend {
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
}

impl RecordBackend for KeychainBackend {
    const MIGRATE_LEGACY_KEYCHAIN: bool = true;

    fn load_record(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError> {
        self.item(account, true)?
            .map(|mut item| item.password.take().ok_or(StoreError::InvalidData))
            .transpose()
    }

    fn save_record(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
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

    fn delete_record(&self, account: &str) -> Result<(), StoreError> {
        let Some(item) = self.item(account, false)? else {
            return Ok(());
        };
        status_result(unsafe { SecKeychainItemDelete(item.reference) })
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
