use serde_json::Value;

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
}

pub fn verify_identity(
    _response: &Value,
    _expected_account_id: Option<&str>,
) -> Result<Identity, ConnectionError> {
    Err(ConnectionError::NotImplemented)
}
