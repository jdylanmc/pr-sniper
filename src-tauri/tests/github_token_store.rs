use pr_sniper_lib::github::{
    device_flow::TokenPair,
    token_store::{
        ActiveAccount, ActiveAccountStore, CredentialKey, CredentialStore, RotationError,
        RotationSafeStore, StoreError,
    },
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier, Mutex},
    thread,
    time::{Duration, UNIX_EPOCH},
};

#[derive(Default)]
struct MemoryStore {
    values: Mutex<BTreeMap<CredentialKey, TokenPair>>,
    active: Mutex<Option<ActiveAccount>>,
    fail_next_save: Mutex<bool>,
}

impl CredentialStore for MemoryStore {
    fn load(&self, key: &CredentialKey) -> Result<Option<TokenPair>, StoreError> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn save(&self, key: &CredentialKey, pair: &TokenPair) -> Result<(), StoreError> {
        if std::mem::take(&mut *self.fail_next_save.lock().unwrap()) {
            return Err(StoreError::Unavailable);
        }
        self.values
            .lock()
            .unwrap()
            .insert(key.clone(), pair.clone());
        Ok(())
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

impl ActiveAccountStore for MemoryStore {
    fn load_active_account(&self) -> Result<Option<ActiveAccount>, StoreError> {
        Ok(self.active.lock().unwrap().clone())
    }

    fn save_active_account(&self, account: &ActiveAccount) -> Result<(), StoreError> {
        *self.active.lock().unwrap() = Some(account.clone());
        Ok(())
    }

    fn delete_active_account(&self) -> Result<(), StoreError> {
        *self.active.lock().unwrap() = None;
        Ok(())
    }
}

fn pair(access: &str, refresh: &str) -> TokenPair {
    TokenPair::new_at(
        access,
        refresh,
        UNIX_EPOCH + Duration::from_secs(1_000),
        Duration::from_secs(28_800),
        Duration::from_secs(15_552_000),
    )
}

#[test]
fn provider_and_account_scope_credentials() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let first = CredentialKey::github("1");
    let second = CredentialKey::github("2");
    store.save(&first, &pair("a1", "r1")).unwrap();
    store.save(&second, &pair("a2", "r2")).unwrap();

    assert_eq!(store.load(&first).unwrap().unwrap().access_token(), "a1");
    assert_eq!(store.load(&second).unwrap().unwrap().access_token(), "a2");
}

#[test]
fn refresh_rotation_is_serialized_and_uses_the_latest_pair() {
    let store = Arc::new(RotationSafeStore::new(MemoryStore::default()));
    let key = CredentialKey::github("1");
    store.save(&key, &pair("a0", "r0")).unwrap();
    let barrier = Arc::new(Barrier::new(3));

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let key = key.clone();
            thread::spawn(move || {
                barrier.wait();
                store
                    .rotate(&key, |current| {
                        let generation = current
                            .refresh_token()
                            .strip_prefix('r')
                            .unwrap()
                            .parse::<u8>()
                            .unwrap()
                            + 1;
                        thread::sleep(Duration::from_millis(20));
                        Ok(pair(&format!("a{generation}"), &format!("r{generation}")))
                    })
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    let current = store.load(&key).unwrap().unwrap();
    assert_eq!(current.access_token(), "a2");
    assert_eq!(current.refresh_token(), "r2");
}

#[test]
fn failed_persistence_never_reports_rotation_success_or_loses_the_old_pair() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let key = CredentialKey::github("1");
    store
        .save(&key, &pair("old-access", "old-refresh"))
        .unwrap();
    *store.inner().fail_next_save.lock().unwrap() = true;

    assert_eq!(
        store.rotate(&key, |_| Ok(pair("new-access", "new-refresh"))),
        Err(RotationError::Store(StoreError::Unavailable))
    );
    let current = store.load(&key).unwrap().unwrap();
    assert_eq!(current.access_token(), "old-access");
    assert_eq!(current.refresh_token(), "old-refresh");
}

#[test]
fn missing_or_rejected_refresh_requires_reconnection() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let key = CredentialKey::github("1");
    assert_eq!(
        store.rotate(&key, |_| unreachable!()),
        Err(RotationError::ReconnectRequired)
    );
    store.save(&key, &pair("old", "refresh")).unwrap();
    assert_eq!(
        store.rotate(&key, |_| Err(RotationError::ReconnectRequired)),
        Err(RotationError::ReconnectRequired)
    );
}

#[test]
fn restart_restores_active_account_and_absolute_expirations() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let account = ActiveAccount::new("1", "octocat").unwrap();
    store
        .replace_active_account(&account, &pair("access", "refresh"))
        .unwrap();

    let restored = store.restore_active_account().unwrap().unwrap();
    assert_eq!(restored.account, account);
    assert_eq!(
        restored.pair.access_expires_at(),
        UNIX_EPOCH + Duration::from_secs(29_800)
    );
    assert_eq!(
        restored.pair.refresh_expires_at(),
        UNIX_EPOCH + Duration::from_secs(15_553_000)
    );
}

#[test]
fn account_switch_and_disconnect_remove_account_bound_credentials() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let first = ActiveAccount::new("1", "first").unwrap();
    let second = ActiveAccount::new("2", "second").unwrap();
    store
        .replace_active_account(&first, &pair("a1", "r1"))
        .unwrap();
    store
        .replace_active_account(&second, &pair("a2", "r2"))
        .unwrap();

    assert!(store.load(&CredentialKey::github("1")).unwrap().is_none());
    assert!(store.load(&CredentialKey::github("2")).unwrap().is_some());
    assert_eq!(
        store.load_active_account().unwrap().unwrap().account_id,
        "2"
    );

    store.disconnect(&second).unwrap();
    assert!(store.load(&CredentialKey::github("2")).unwrap().is_none());
    assert!(store.load_active_account().unwrap().is_none());
}

#[test]
fn expired_access_refreshes_once_through_the_shared_rotation_store() {
    let store = Arc::new(RotationSafeStore::new(MemoryStore::default()));
    let account = ActiveAccount::new("1", "octocat").unwrap();
    let expired = TokenPair::new_at(
        "a0",
        "r0",
        UNIX_EPOCH,
        Duration::from_secs(1),
        Duration::from_secs(100),
    );
    store.replace_active_account(&account, &expired).unwrap();
    let refreshes = Arc::new(Mutex::new(0_u8));
    let barrier = Arc::new(Barrier::new(3));

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let refreshes = Arc::clone(&refreshes);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store
                    .refresh_if_needed(
                        &CredentialKey::github("1"),
                        UNIX_EPOCH + Duration::from_secs(2),
                        |current| {
                            *refreshes.lock().unwrap() += 1;
                            Ok(TokenPair::new_at(
                                "a1",
                                current.refresh_token(),
                                UNIX_EPOCH + Duration::from_secs(2),
                                Duration::from_secs(100),
                                Duration::from_secs(100),
                            ))
                        },
                    )
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(*refreshes.lock().unwrap(), 1);
}
