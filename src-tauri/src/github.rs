pub mod credential_records;
pub mod http;
pub mod intake;
#[cfg(target_os = "macos")]
pub mod macos_keychain;
#[cfg(windows)]
pub mod windows_credentials;
#[cfg(target_os = "macos")]
pub use macos_keychain::MacKeychainStore as NativeCredentialStore;
#[cfg(windows)]
pub use windows_credentials::WindowsCredentialStore as NativeCredentialStore;
pub mod actions;
pub mod conversation;
pub mod metadata;
pub mod oauth;
pub mod provider;
pub mod publication;
mod read;
pub mod review;
pub mod state;
pub mod threads;
pub mod token_store;

pub use crate::provider::ConnectionError;
use serde_json::Value;
pub mod credentials;
mod identity;
pub use identity::Identity;

pub fn client() -> Result<provider::GithubClient<http::HttpTransport>, ConnectionError> {
    use credentials::CredentialSource;
    let credential = credentials::GhCredentialSource::discover()?.acquire()?;
    Ok(provider::GithubClient::new(http::HttpTransport::new(
        credential,
    )?))
}

pub fn verify_identity(
    response: &Value,
    expected_account_id: Option<&str>,
) -> Result<Identity, ConnectionError> {
    let id = response["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or(ConnectionError::InvalidResponse)?
        .to_string();
    let login = response["login"]
        .as_str()
        .filter(|login| !login.is_empty() && !login.chars().any(char::is_whitespace))
        .ok_or(ConnectionError::InvalidResponse)?;
    if expected_account_id.is_some_and(|expected| expected != id) {
        return Err(ConnectionError::WrongIdentity);
    }
    Ok(Identity {
        id,
        login: login.into(),
    })
}
