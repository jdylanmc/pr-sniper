use pr_sniper_lib::github::{
    device_flow::TokenPair,
    token_store::{CredentialKey, CredentialStore, RotationError, RotationSafeStore, StoreError},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier, Mutex},
    thread,
    time::Duration,
};

#[derive(Default)]
struct MemoryStore {
    values: Mutex<BTreeMap<CredentialKey, TokenPair>>,
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

fn pair(access: &str, refresh: &str) -> TokenPair {
    TokenPair::new(
        access,
        refresh,
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
