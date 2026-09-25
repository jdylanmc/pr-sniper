use pr_sniper_lib::github::{
    oauth::TokenPair,
    token_store::{
        AccountRegistry, AccountRegistryStore, ActiveAccount, CredentialKey, CredentialStore,
        RotationError, RotationSafeStore, StoreError,
    },
};
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Barrier, Mutex},
    thread,
    time::{Duration, UNIX_EPOCH},
};

#[derive(Default)]
struct MemoryStore {
    values: Mutex<BTreeMap<CredentialKey, TokenPair>>,
    registry: Mutex<AccountRegistry>,
    fail_next_save: Mutex<bool>,
    fail_registry_save: Mutex<bool>,
    fail_registry_save_after: Mutex<Option<usize>>,
    fail_delete: Mutex<bool>,
}

#[test]
fn disconnecting_ai_secret_retains_identity_and_other_role_credentials() {
    use pr_sniper_lib::github::token_store::ProviderId;
    let store = RotationSafeStore::new(MemoryStore::default());
    let repo = ActiveAccount::new("101", "mutable-login").unwrap();
    let ai = ActiveAccount::for_provider(ProviderId::copilot(), "101", "mutable-login").unwrap();
    let repo_pair = TokenPair::new(
        "repo-access",
        "repo-refresh",
        Duration::from_secs(60),
        Duration::from_secs(120),
    );
    let ai_pair = TokenPair::new(
        "ai-access",
        "ai-refresh",
        Duration::from_secs(60),
        Duration::from_secs(120),
    );
    store.save_account(&repo, &repo_pair, false).unwrap();
    store.save_account(&ai, &ai_pair, false).unwrap();
    *store.inner().fail_delete.lock().unwrap() = true;
    assert!(store
        .clear_account_credentials(&ai.provider_account_id())
        .is_err());
    assert!(store.load(&ai.provider_account_id()).unwrap().is_some());
    store
        .clear_account_credentials(&ai.provider_account_id())
        .unwrap();
    assert!(store.load(&ai.provider_account_id()).unwrap().is_none());
    assert_eq!(store.accounts().unwrap(), vec![repo.clone(), ai.clone()]);
    assert_eq!(
        store
            .restore_account(&repo.provider_account_id())
            .unwrap()
            .unwrap()
            .pair,
        repo_pair
    );
    assert!(store.inner().registry.lock().unwrap().active.is_none());
    store.save_account(&ai, &ai_pair, false).unwrap();
    assert_eq!(store.accounts().unwrap().len(), 2);
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
        if std::mem::take(&mut *self.fail_delete.lock().unwrap()) {
            return Err(StoreError::Unavailable);
        }
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

impl AccountRegistryStore for MemoryStore {
    fn load_registry(&self) -> Result<AccountRegistry, StoreError> {
        Ok(self.registry.lock().unwrap().clone())
    }

    fn save_registry(&self, registry: &AccountRegistry) -> Result<(), StoreError> {
        if std::mem::take(&mut *self.fail_registry_save.lock().unwrap()) {
            return Err(StoreError::Unavailable);
        }
        let mut fail_after = self.fail_registry_save_after.lock().unwrap();
        if matches!(*fail_after, Some(0)) {
            *fail_after = None;
            return Err(StoreError::Unavailable);
        }
        if let Some(remaining) = fail_after.as_mut() {
            *remaining -= 1;
        }
        *self.registry.lock().unwrap() = registry.clone();
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
fn different_accounts_do_not_share_a_refresh_lock() {
    let store = Arc::new(RotationSafeStore::new(MemoryStore::default()));
    let first = CredentialKey::github("1");
    let second = CredentialKey::github("2");
    store.save(&first, &pair("a1", "r1")).unwrap();
    store.save(&second, &pair("a2", "r2")).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let first_store = Arc::clone(&store);
    let first_handle = thread::spawn(move || {
        first_store
            .rotate(&first, |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(pair("a1-next", "r1-next"))
            })
            .unwrap();
    });
    entered_rx.recv().unwrap();

    let second_store = Arc::clone(&store);
    let (second_done_tx, second_done_rx) = mpsc::channel();
    let second_handle = thread::spawn(move || {
        second_store
            .rotate(&second, |_| Ok(pair("a2-next", "r2-next")))
            .unwrap();
        second_done_tx.send(()).unwrap();
    });
    second_done_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("a different account must rotate independently");
    release_tx.send(()).unwrap();
    first_handle.join().unwrap();
    second_handle.join().unwrap();
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
fn adding_an_account_retains_existing_accounts_and_disconnect_does_not_transfer_active_identity() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let first = ActiveAccount::new("1", "first").unwrap();
    let second = ActiveAccount::new("2", "second").unwrap();
    store
        .replace_active_account(&first, &pair("a1", "r1"))
        .unwrap();
    store
        .replace_active_account(&second, &pair("a2", "r2"))
        .unwrap();

    assert_eq!(
        store
            .restore_active_account()
            .unwrap()
            .unwrap()
            .account
            .account_id,
        "2"
    );

    store.disconnect(&second).unwrap();
    assert!(store.restore_active_account().unwrap().is_none());
    assert_eq!(store.accounts().unwrap(), vec![first]);
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
                    .refresh_active_if_needed(UNIX_EPOCH + Duration::from_secs(2), |current| {
                        *refreshes.lock().unwrap() += 1;
                        Ok(TokenPair::new_at(
                            "a1",
                            current.refresh_token(),
                            UNIX_EPOCH + Duration::from_secs(2),
                            Duration::from_secs(100),
                            Duration::from_secs(100),
                        ))
                    })
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

#[test]
fn failed_account_replacement_preserves_the_previous_record_and_retry_converges() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let first = ActiveAccount::new("1", "first").unwrap();
    let second = ActiveAccount::new("2", "second").unwrap();
    store
        .replace_active_account(&first, &pair("a1", "r1"))
        .unwrap();
    *store.inner().fail_registry_save.lock().unwrap() = true;

    assert_eq!(
        store.replace_active_account(&second, &pair("a2", "r2")),
        Err(StoreError::Unavailable)
    );
    let restored = store.restore_active_account().unwrap().unwrap();
    assert_eq!(restored.account, first);
    assert_eq!(restored.pair.access_token(), "a1");
    assert!(store.load(&second.provider_account_id()).unwrap().is_none());

    store
        .replace_active_account(&second, &pair("a2", "r2"))
        .unwrap();
    assert_eq!(
        store.restore_active_account().unwrap().unwrap().account,
        second
    );
}

#[test]
fn new_account_secret_write_failure_cleans_the_pending_addition_on_restart() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let account = ActiveAccount::new("2", "second").unwrap();
    *store.inner().fail_next_save.lock().unwrap() = true;

    assert_eq!(
        store.save_account(&account, &pair("a2", "r2"), false),
        Err(StoreError::Unavailable)
    );
    assert_eq!(store.accounts().unwrap(), Vec::<ActiveAccount>::new());
    assert!(store
        .load(&account.provider_account_id())
        .unwrap()
        .is_none());
}

#[test]
fn new_account_registry_finalization_failure_removes_the_unregistered_secret_on_restart() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let first = ActiveAccount::new("1", "first").unwrap();
    let second = ActiveAccount::new("2", "second").unwrap();
    store
        .save_account(&first, &pair("a1", "r1"), false)
        .unwrap();
    *store.inner().fail_registry_save_after.lock().unwrap() = Some(1);

    assert_eq!(
        store.save_account(&second, &pair("a2", "r2"), false),
        Err(StoreError::Unavailable)
    );
    assert!(store.load(&second.provider_account_id()).unwrap().is_some());
    assert_eq!(store.accounts().unwrap(), vec![first]);
    assert!(store.load(&second.provider_account_id()).unwrap().is_none());
}

#[test]
fn pending_addition_delete_failure_retries_without_registering_the_secret() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let account = ActiveAccount::new("2", "second").unwrap();
    *store.inner().fail_registry_save_after.lock().unwrap() = Some(1);
    assert_eq!(
        store.save_account(&account, &pair("a2", "r2"), false),
        Err(StoreError::Unavailable)
    );
    *store.inner().fail_delete.lock().unwrap() = true;

    assert_eq!(store.accounts(), Err(StoreError::Unavailable));
    assert!(store
        .load(&account.provider_account_id())
        .unwrap()
        .is_some());
    assert_eq!(store.accounts().unwrap(), Vec::<ActiveAccount>::new());
    assert!(store
        .load(&account.provider_account_id())
        .unwrap()
        .is_none());
}

#[test]
fn pending_addition_finalization_failure_retries_after_secret_cleanup() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let account = ActiveAccount::new("2", "second").unwrap();
    *store.inner().fail_registry_save_after.lock().unwrap() = Some(1);
    assert_eq!(
        store.save_account(&account, &pair("a2", "r2"), false),
        Err(StoreError::Unavailable)
    );
    *store.inner().fail_registry_save.lock().unwrap() = true;

    assert_eq!(store.accounts(), Err(StoreError::Unavailable));
    assert!(store
        .load(&account.provider_account_id())
        .unwrap()
        .is_none());
    assert_eq!(store.accounts().unwrap(), Vec::<ActiveAccount>::new());
}

#[test]
fn failed_disconnect_preserves_the_record_and_retry_converges() {
    let store = RotationSafeStore::new(MemoryStore::default());
    let account = ActiveAccount::new("1", "octocat").unwrap();
    store
        .replace_active_account(&account, &pair("a1", "r1"))
        .unwrap();
    *store.inner().fail_delete.lock().unwrap() = true;

    assert_eq!(store.disconnect(&account), Err(StoreError::Unavailable));
    assert!(store
        .load(&account.provider_account_id())
        .unwrap()
        .is_some());
    assert!(store.accounts().unwrap().is_empty());
    assert!(store.restore_active_account().unwrap().is_none());
    assert!(store
        .load(&account.provider_account_id())
        .unwrap()
        .is_none());
}
