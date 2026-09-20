use serde_json::Value;
pub mod credentials;

#[derive(Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub login: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectionError {
    NotImplemented,
    WrongIdentity,
    InvalidResponse,
    MissingCli,
    BrokenCli,
    SignedOut,
    Timeout,
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
