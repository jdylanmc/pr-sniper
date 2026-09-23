#![cfg(target_os = "macos")]

use pr_sniper_lib::github::{
    device_flow::TokenPair,
    macos_keychain::MacKeychainStore,
    token_store::{CredentialKey, CredentialStore},
};
use std::time::Duration;

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
    TokenPair::new(
        access,
        refresh,
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
