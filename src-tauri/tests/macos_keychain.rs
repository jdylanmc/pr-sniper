#![cfg(target_os = "macos")]

use pr_sniper_lib::github::{
    device_flow::TokenPair,
    macos_keychain::MacKeychainStore,
    token_store::{ActiveAccount, CredentialKey, CredentialStore, RotationSafeStore},
};
use std::time::{Duration, UNIX_EPOCH};

struct Fixture {
    store: MacKeychainStore,
    key: CredentialKey,
}

impl Fixture {
    fn new() -> Self {
        let nonce = uuid::Uuid::new_v4();
        Self {
            store: MacKeychainStore::with_service(format!("com.jdylanmc.pr-sniper.tests.{nonce}")),
            key: CredentialKey::github(format!("fixture-{nonce}")),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.store
            .delete(&self.key)
            .expect("clean isolated Keychain fixture");
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
fn native_keychain_round_trip_and_rotation_use_an_isolated_service() {
    let fixture = Fixture::new();
    assert!(fixture.store.load(&fixture.key).unwrap().is_none());

    fixture
        .store
        .save(&fixture.key, &pair("access-one", "refresh-one"))
        .unwrap();
    let first = fixture.store.load(&fixture.key).unwrap().unwrap();
    assert_eq!(first.access_token(), "access-one");
    assert_eq!(first.refresh_token(), "refresh-one");

    fixture
        .store
        .save(&fixture.key, &pair("access-two", "refresh-two"))
        .unwrap();
    let second = fixture.store.load(&fixture.key).unwrap().unwrap();
    assert_eq!(second.access_token(), "access-two");
    assert_eq!(second.refresh_token(), "refresh-two");

    fixture.store.delete(&fixture.key).unwrap();
    assert!(fixture.store.load(&fixture.key).unwrap().is_none());
}

#[test]
fn native_keychain_restores_active_account_and_absolute_expirations_after_restart() {
    let nonce = uuid::Uuid::new_v4();
    let service = format!("com.jdylanmc.pr-sniper.tests.{nonce}");
    let account = ActiveAccount::new(format!("fixture-{nonce}"), "octocat").unwrap();
    let first = RotationSafeStore::new(MacKeychainStore::with_service(&service));
    first
        .replace_active_account(&account, &pair("access", "refresh"))
        .unwrap();

    let restarted = RotationSafeStore::new(MacKeychainStore::with_service(&service));
    let restored = restarted.restore_active_account().unwrap().unwrap();
    assert_eq!(restored.account, account);
    assert_eq!(
        restored.pair.access_expires_at(),
        UNIX_EPOCH + Duration::from_secs(29_800)
    );

    restarted.disconnect(&account).unwrap();
    assert!(restarted.restore_active_account().unwrap().is_none());
}
