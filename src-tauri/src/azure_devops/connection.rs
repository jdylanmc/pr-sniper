//! Signed-in Azure DevOps identity from
//! `GET https://dev.azure.com/{organization}/_apis/connectionData`.
//! The identity binds the provider account (FR-15).

use super::tenant::{NetworkError, TenantId};
use super::token_source::{with_token, AccessToken, AuthorizedError, Rejection, TokenSource};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Azure DevOps identity ID (GUID); the provider account ID.
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionError {
    Network,
    /// Non-success status other than 401.
    Failed,
    Malformed,
}

const BODY_LIMIT: usize = 1024 * 1024;

#[async_trait]
pub trait ConnectionTransport: Send + Sync {
    /// Authenticated GET without following redirects.
    async fn connection_data(
        &self,
        organization: &str,
        token: &AccessToken,
    ) -> Result<ConnectionResponse, NetworkError>;
}

pub struct HttpConnectionTransport(reqwest::Client);

impl HttpConnectionTransport {
    pub fn new() -> Result<Self, NetworkError> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map(Self)
            .map_err(|_| NetworkError)
    }
}

#[async_trait]
impl ConnectionTransport for HttpConnectionTransport {
    async fn connection_data(
        &self,
        organization: &str,
        token: &AccessToken,
    ) -> Result<ConnectionResponse, NetworkError> {
        let response = self
            .0
            .get(format!(
                "https://dev.azure.com/{organization}/_apis/connectionData"
            ))
            .bearer_auth(token.secret())
            .header("Accept", "application/json")
            .header("X-TFS-FedAuthRedirect", "Suppress")
            .send()
            .await
            .map_err(|_| NetworkError)?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|_| NetworkError)?;
        Ok(ConnectionResponse {
            status,
            body: if body.len() > BODY_LIMIT {
                Vec::new()
            } else {
                body.to_vec()
            },
        })
    }
}

/// `None` for an anonymous (all-zero or missing) user.
pub fn parse_identity(body: &[u8]) -> Result<Option<Identity>, ConnectionError> {
    #[derive(Deserialize)]
    struct Value {
        #[serde(rename = "$value")]
        value: Option<String>,
    }
    #[derive(Deserialize)]
    struct Properties {
        #[serde(rename = "Account")]
        account: Option<Value>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct User {
        id: Option<String>,
        provider_display_name: Option<String>,
        properties: Option<Properties>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        authenticated_user: Option<User>,
    }
    let raw: Raw = serde_json::from_slice(body).map_err(|_| ConnectionError::Malformed)?;
    let Some(user) = raw.authenticated_user else {
        return Ok(None);
    };
    let Some(id) = user.id.as_deref().and_then(TenantId::parse) else {
        return Ok(None);
    };
    if id.as_str().bytes().all(|b| b == b'0' || b == b'-') {
        return Ok(None);
    }
    let bounded = |text: String| (!text.is_empty() && text.len() <= 256).then_some(text);
    Ok(Some(Identity {
        id: id.as_str().to_owned(),
        display_name: user
            .provider_display_name
            .and_then(bounded)
            .ok_or(ConnectionError::Malformed)?,
        unique_name: user
            .properties
            .and_then(|properties| properties.account)
            .and_then(|account| account.value)
            .and_then(bounded),
    }))
}

/// Reads the identity the tenant token belongs to; a 401 invalidates the
/// token and retries once.
pub async fn signed_in_identity(
    source: &(impl TokenSource + ?Sized),
    tenant: &TenantId,
    transport: &(impl ConnectionTransport + ?Sized),
    organization: &str,
) -> Result<Identity, AuthorizedError<ConnectionError>> {
    with_token(source, tenant, |token| async move {
        let response = transport
            .connection_data(organization, &token)
            .await
            .map_err(|_| Rejection::Failed(ConnectionError::Network))?;
        match response.status {
            200 => match parse_identity(&response.body).map_err(Rejection::Failed)? {
                Some(identity) => Ok(identity),
                None => Err(Rejection::Unauthorized),
            },
            401 => Err(Rejection::Unauthorized),
            _ => Err(Rejection::Failed(ConnectionError::Failed)),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_authenticated_user() {
        let identity = parse_identity(
            br#"{"authenticatedUser":{"id":"0000000A-0000-0000-0000-000000000002","providerDisplayName":"Avery Example","properties":{"Account":{"$type":"System.String","$value":"avery@contoso.example"}}},"authorizedUser":{}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            identity,
            Identity {
                id: "0000000a-0000-0000-0000-000000000002".into(),
                display_name: "Avery Example".into(),
                unique_name: Some("avery@contoso.example".into()),
            }
        );
    }

    #[test]
    fn anonymous_users_are_not_identities_and_bad_bodies_are_malformed() {
        for body in [
            r#"{"authenticatedUser":{"id":"00000000-0000-0000-0000-000000000000","providerDisplayName":"Anonymous"}}"#,
            r#"{"authenticatedUser":{"providerDisplayName":"x"}}"#,
            r#"{}"#,
        ] {
            assert_eq!(parse_identity(body.as_bytes()), Ok(None), "{body}");
        }
        for body in [
            "not json",
            r#"{"authenticatedUser":{"id":"00000000-0000-0000-0000-000000000002"}}"#,
        ] {
            assert_eq!(
                parse_identity(body.as_bytes()),
                Err(ConnectionError::Malformed),
                "{body}"
            );
        }
    }
}
