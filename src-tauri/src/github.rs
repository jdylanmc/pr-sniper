pub mod credential_records;
pub mod http;
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
pub mod review;
pub mod threads;
pub mod token_store;

use serde::Serialize;
use serde_json::Value;
pub mod credentials;
mod identity;
pub use identity::Identity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionError {
    WrongIdentity,
    InvalidResponse,
    MissingCli,
    BrokenCli,
    SignedOut,
    Timeout,
    MissingReadPermission,
    RepositoryUnavailable,
    MissingScope,
    ScopeUnverified,
    OrganizationPolicyDenied,
    OrganizationPolicyDeniedWithMissingScope,
    RateLimited,
    RateLimitedAfter(i64),
    RateLimitedWithContext {
        retry_after_seconds: Option<i64>,
        reset_at: Option<i64>,
        organization_access_incomplete: bool,
        missing_repo_scope: bool,
    },
    Network,
    ProviderFailure,
    ProviderRejected,
    ProviderRejectedStatus(u16),
    ProviderFailureAfter(i64),
    IncompleteRead,
    RevisionChanged,
    InvalidRepository,
    RepositoryChanged,
    Configuration,
}

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
