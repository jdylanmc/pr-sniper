#[path = "../src/github/credential_records.rs"]
pub mod credential_records;
#[path = "../src/github/identity.rs"]
mod identity;
pub use identity::Identity;
#[cfg(target_os = "macos")]
#[path = "../src/github/macos_keychain.rs"]
pub mod macos_keychain;
#[path = "../src/github/oauth.rs"]
pub mod oauth;
#[path = "../src/github/token_store.rs"]
pub mod token_store;
#[cfg(windows)]
#[path = "../src/github/windows_credentials.rs"]
pub mod windows_credentials;
