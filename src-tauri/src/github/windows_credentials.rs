use super::{
    credential_records::{RecordBackend, RecordStore},
    token_store::StoreError,
};
use std::ptr;
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_NOT_FOUND},
    Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_MAX_CREDENTIAL_BLOB_SIZE,
        CRED_MAX_GENERIC_TARGET_NAME_LENGTH, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    },
};
use zeroize::{Zeroize, Zeroizing};

pub struct WindowsCredentialBackend {
    service: String,
}

pub type WindowsCredentialStore = RecordStore<WindowsCredentialBackend>;

impl WindowsCredentialStore {
    pub fn production() -> Self {
        Self::with_service("com.jdylanmc.pr-sniper.github.oauth-app.v1")
    }

    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            backend: WindowsCredentialBackend {
                service: service.into(),
            },
        }
    }
}

impl WindowsCredentialBackend {
    fn target(&self, account: &str) -> Result<Vec<u16>, StoreError> {
        if self.service.is_empty()
            || !self
                .service
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
        {
            return Err(StoreError::InvalidData);
        }
        if account.contains('\0') {
            return Err(StoreError::InvalidData);
        }
        // Native target matching is case-insensitive; preserve exact identity
        // and namespace keys by encoding each component before storage.
        let hex = |value: &str| {
            value
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let mut target: Vec<_> = format!(
            "com.jdylanmc.pr-sniper:{}:{}",
            hex(&self.service),
            hex(account)
        )
        .encode_utf16()
        .collect();
        if target.len() > CRED_MAX_GENERIC_TARGET_NAME_LENGTH as usize {
            return Err(StoreError::InvalidData);
        }
        target.push(0);
        Ok(target)
    }
}

struct Credential(*mut CREDENTIALW);

impl Drop for Credential {
    fn drop(&mut self) {
        unsafe {
            let credential = &mut *self.0;
            if !credential.CredentialBlob.is_null() {
                std::slice::from_raw_parts_mut(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .zeroize();
            }
            CredFree(self.0.cast());
        }
    }
}

impl RecordBackend for WindowsCredentialBackend {
    fn validate_record(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        self.target(account)?;
        if secret.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            eprintln!("[credentials] stage=write outcome=record_too_large");
            return Err(StoreError::TooLarge);
        }
        Ok(())
    }

    fn load_record(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError> {
        let target = self.target(account)?;
        let mut credential = ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
            return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                Ok(None)
            } else {
                Err(StoreError::Unavailable)
            };
        }
        if credential.is_null() {
            return Err(StoreError::Unavailable);
        }
        let owned = Credential(credential);
        let record = unsafe { &*owned.0 };
        if record.CredentialBlobSize == 0 || record.CredentialBlob.is_null() {
            return Err(StoreError::InvalidData);
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(record.CredentialBlob, record.CredentialBlobSize as usize)
        };
        Ok(Some(Zeroizing::new(bytes.to_vec())))
    }

    fn save_record(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        self.validate_record(account, secret)?;
        let mut target = self.target(account)?;
        // Registry and token-pair records are single atomic native replacements.
        // Never truncate, split a pair, or spill an oversized record to plaintext.
        let mut secret = Zeroizing::new(secret.to_vec());
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: secret.len() as u32,
            CredentialBlob: secret.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            Err(StoreError::Unavailable)
        } else {
            Ok(())
        }
    }

    fn delete_record(&self, account: &str) -> Result<(), StoreError> {
        let target = self.target(account)?;
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0
            || unsafe { GetLastError() } == ERROR_NOT_FOUND
        {
            Ok(())
        } else {
            Err(StoreError::Unavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{
        oauth::TokenPair,
        token_store::{
            AccountRegistry, AccountRegistryStore, ActiveAccount, CredentialStore,
            ProviderAccountId, ProviderId, RotationError, RotationSafeStore,
        },
    };
    use std::time::{Duration, UNIX_EPOCH};

    struct Fixture {
        store: RotationSafeStore<WindowsCredentialStore>,
        keys: Vec<ProviderAccountId>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                store: RotationSafeStore::new(WindowsCredentialStore::with_service(format!(
                    "com.jdylanmc.pr-sniper.tests.accounts-{}",
                    uuid::Uuid::new_v4()
                ))),
                keys: vec![
                    ProviderAccountId::github("101"),
                    ProviderAccountId::new(ProviderId::copilot(), "101").unwrap(),
                    ProviderAccountId::github("202"),
                ],
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            for key in &self.keys {
                self.store
                    .delete(key)
                    .expect("delete owned synthetic credential");
                assert!(self.store.load(key).unwrap().is_none());
            }
            self.store
                .inner()
                .backend
                .delete_record("accounts:registry")
                .expect("delete owned synthetic registry");
            self.store
                .inner()
                .backend
                .delete_record("github:active-account")
                .expect("delete owned synthetic legacy marker");
            assert!(self
                .store
                .inner()
                .backend
                .load_record("accounts:registry")
                .unwrap()
                .is_none());
        }
    }

    fn pair(access: &str, refresh: &str) -> TokenPair {
        TokenPair::new_at(
            access,
            refresh,
            UNIX_EPOCH + Duration::from_secs(1000),
            Duration::from_secs(60),
            Duration::from_secs(120),
        )
    }

    #[test]
    fn native_pairs_restart_rotate_and_disconnect_without_an_active_account() {
        let fixture = Fixture::new();
        let account = ActiveAccount::new("101", "fixture").unwrap();
        fixture
            .store
            .save_account(&account, &pair("first", "refresh-first"), false)
            .unwrap();
        fixture
            .store
            .rotate(&fixture.keys[0], |_| Ok(pair("next", "refresh-next")))
            .unwrap();
        let restarted = RotationSafeStore::new(WindowsCredentialStore::with_service(
            &fixture.store.inner().backend.service,
        ));
        let restored = restarted
            .restore_account(&fixture.keys[0])
            .unwrap()
            .unwrap();
        assert_eq!(restored.account, account);
        assert_eq!(restored.pair, pair("next", "refresh-next"));
        assert_eq!(
            restored.pair.access_expires_at(),
            UNIX_EPOCH + Duration::from_secs(1060)
        );
        assert!(restarted.inner().load_registry().unwrap().active.is_none());
        restarted.remove_account(&fixture.keys[0]).unwrap();
        assert!(restarted.accounts().unwrap().is_empty());
        assert!(restarted.load(&fixture.keys[0]).unwrap().is_none());
    }

    #[test]
    fn role_and_service_namespaces_are_independent_and_disconnect_retains_ai_identity() {
        let fixture = Fixture::new();
        let other = Fixture::new();
        for key in &fixture.keys[..2] {
            let account =
                ActiveAccount::for_provider(key.provider().clone(), "101", "fixture").unwrap();
            fixture
                .store
                .save_account(&account, &pair(key.provider().as_str(), "refresh"), false)
                .unwrap();
        }
        assert!(other.store.accounts().unwrap().is_empty());
        assert!(other.store.load(&fixture.keys[0]).unwrap().is_none());
        fixture
            .store
            .clear_account_credentials(&fixture.keys[1])
            .unwrap();
        assert!(fixture.store.load(&fixture.keys[1]).unwrap().is_none());
        assert_eq!(
            fixture
                .store
                .load(&fixture.keys[0])
                .unwrap()
                .unwrap()
                .access_token(),
            "github"
        );
        assert_eq!(fixture.store.accounts().unwrap().len(), 2);
        assert!(fixture
            .store
            .inner()
            .load_registry()
            .unwrap()
            .active
            .is_none());
    }

    #[test]
    fn oversize_rotation_is_visible_and_preserves_both_old_tokens() {
        let fixture = Fixture::new();
        let before = pair("before", "refresh-before");
        fixture.store.save(&fixture.keys[0], &before).unwrap();
        let result = fixture.store.rotate(&fixture.keys[0], |_| {
            Ok(pair(
                &"x".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize),
                "refresh-after",
            ))
        });
        assert_eq!(result, Err(RotationError::Store(StoreError::TooLarge)));
        assert_eq!(
            fixture.store.load(&fixture.keys[0]).unwrap().unwrap(),
            before
        );
    }

    #[test]
    fn oversize_registry_is_visible_and_does_not_commit_or_orphan_an_account() {
        let fixture = Fixture::new();
        let registry = AccountRegistry {
            accounts: (0..80)
                .map(|i| ActiveAccount::new(i.to_string(), "x".repeat(128)).unwrap())
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            fixture.store.inner().save_registry(&registry),
            Err(StoreError::TooLarge)
        );
        assert!(fixture.store.accounts().unwrap().is_empty());
        let account = ActiveAccount::new("101", "fixture").unwrap();
        assert_eq!(
            fixture.store.save_account(
                &account,
                &pair(
                    &"x".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize),
                    "refresh"
                ),
                false
            ),
            Err(StoreError::TooLarge)
        );
        assert!(fixture.store.accounts().unwrap().is_empty());
        assert!(fixture.store.load(&fixture.keys[0]).unwrap().is_none());
    }

    #[test]
    fn windows_does_not_import_legacy_keychain_records_and_corruption_is_an_error() {
        let fixture = Fixture::new();
        fixture
            .store
            .inner()
            .backend
            .save_record("github:active-account", b"not a Windows registry")
            .unwrap();
        assert!(fixture.store.accounts().unwrap().is_empty());
        fixture
            .store
            .inner()
            .backend
            .save_record("accounts:registry", b"malformed")
            .unwrap();
        assert_eq!(fixture.store.accounts(), Err(StoreError::InvalidData));
        assert_eq!(
            fixture
                .store
                .inner()
                .backend
                .load_record("accounts:registry")
                .unwrap()
                .unwrap()
                .as_slice(),
            b"malformed"
        );
    }

    #[test]
    fn invalid_native_targets_fail_reads_writes_and_deletes_instead_of_aliasing() {
        let fixture = Fixture::new();
        let key = ProviderAccountId::github("101\0suffix");
        assert_eq!(fixture.store.load(&key), Err(StoreError::InvalidData));
        assert_eq!(
            fixture.store.save(&key, &pair("access", "refresh")),
            Err(StoreError::InvalidData)
        );
        assert_eq!(fixture.store.delete(&key), Err(StoreError::InvalidData));
        assert!(fixture.store.load(&fixture.keys[0]).unwrap().is_none());
    }

    #[test]
    fn exact_native_blob_limit_roundtrips_but_one_more_byte_preserves_the_record() {
        let fixture = Fixture::new();
        // Established wire format: two 32-bit lengths and two 64-bit timestamps.
        let access = "a".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize - 24 - 1);
        let before = pair(&access, "r");
        fixture.store.save(&fixture.keys[0], &before).unwrap();
        assert_eq!(
            fixture.store.load(&fixture.keys[0]).unwrap().unwrap(),
            before
        );
        assert_eq!(
            fixture
                .store
                .save(&fixture.keys[0], &pair(&(access + "a"), "r")),
            Err(StoreError::TooLarge)
        );
        assert_eq!(
            fixture.store.load(&fixture.keys[0]).unwrap().unwrap(),
            before
        );
    }

    #[test]
    fn registry_capacity_failure_preserves_existing_accounts_and_has_no_new_secret() {
        let mut fixture = Fixture::new();
        let mut failed = false;
        for index in 0..100 {
            let account = ActiveAccount::new(format!("capacity-{index}"), "x".repeat(128)).unwrap();
            let key = account.provider_account_id();
            fixture.keys.push(key.clone());
            let before = fixture.store.accounts().unwrap();
            match fixture
                .store
                .save_account(&account, &pair("access", "refresh"), false)
            {
                Ok(()) => {}
                Err(StoreError::TooLarge) => {
                    assert!(!before.is_empty());
                    assert_eq!(fixture.store.accounts().unwrap(), before);
                    assert!(fixture.store.load(&key).unwrap().is_none());
                    for existing in &before {
                        assert_eq!(
                            fixture
                                .store
                                .load(&existing.provider_account_id())
                                .unwrap()
                                .unwrap(),
                            pair("access", "refresh")
                        );
                    }
                    failed = true;
                    break;
                }
                Err(error) => panic!("unexpected capacity result: {error:?}"),
            }
        }
        assert!(failed, "registry must surface its native capacity limit");
    }

    #[test]
    fn case_distinct_account_ids_never_alias_in_the_native_target_namespace() {
        let mut fixture = Fixture::new();
        let upper = ProviderAccountId::github("Case");
        let lower = ProviderAccountId::github("case");
        fixture.keys.extend([upper.clone(), lower.clone()]);
        fixture
            .store
            .save(&upper, &pair("upper", "upper-refresh"))
            .unwrap();
        fixture
            .store
            .save(&lower, &pair("lower", "lower-refresh"))
            .unwrap();
        assert_eq!(
            fixture.store.load(&upper).unwrap().unwrap().access_token(),
            "upper"
        );
        assert_eq!(
            fixture.store.load(&lower).unwrap().unwrap().access_token(),
            "lower"
        );
        fixture.store.delete(&upper).unwrap();
        assert!(fixture.store.load(&lower).unwrap().is_some());
    }

    #[test]
    fn existing_account_reconnect_capacity_failure_preserves_pair_and_metadata_then_retries() {
        let mut fixture = Fixture::new();
        let accounts: Vec<_> = (1..=70)
            .map(|index| {
                ActiveAccount::new((100_000_000 + index).to_string(), format!("user{index:05}"))
                    .unwrap()
            })
            .collect();
        let before = pair("before", "refresh-before");
        for account in &accounts {
            fixture.keys.push(account.provider_account_id());
            fixture.store.save_account(account, &before, false).unwrap();
        }
        let original_registry = fixture
            .store
            .inner()
            .backend
            .load_record("accounts:registry")
            .unwrap()
            .unwrap();
        assert_eq!(original_registry.len(), 2549);
        let renamed = ActiveAccount::new(&accounts[0].account_id, "r".repeat(21)).unwrap();
        assert_eq!(
            original_registry.len() + renamed.login.len() - accounts[0].login.len(),
            2561
        );
        let replacement = pair("replacement", "refresh-replacement");

        assert_eq!(
            fixture.store.save_account(&renamed, &replacement, false),
            Err(StoreError::TooLarge)
        );

        let reopened = RotationSafeStore::new(WindowsCredentialStore::with_service(
            &fixture.store.inner().backend.service,
        ));
        assert_eq!(reopened.accounts().unwrap(), accounts);
        assert_eq!(
            reopened
                .load(&renamed.provider_account_id())
                .unwrap()
                .unwrap(),
            before
        );
        assert_eq!(
            reopened
                .inner()
                .backend
                .load_record("accounts:registry")
                .unwrap()
                .unwrap(),
            original_registry
        );

        reopened
            .remove_account(&accounts[69].provider_account_id())
            .unwrap();
        reopened
            .save_account(&renamed, &replacement, false)
            .unwrap();
        let restored = reopened
            .restore_account(&renamed.provider_account_id())
            .unwrap()
            .unwrap();
        assert_eq!(restored.account, renamed);
        assert_eq!(restored.pair, replacement);
        assert_eq!(reopened.accounts().unwrap().len(), 69);
    }
}
