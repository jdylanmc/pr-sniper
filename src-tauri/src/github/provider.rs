use super::{verify_identity, ConnectionError, Identity};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub trait Transport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError>;
}

pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub identity: Identity,
    pub repository: RemoteRepository,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteRepository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub read: bool,
    pub comment: CommentCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentCapability {
    Available,
    Unavailable,
    Unknown,
}

pub struct GithubClient<T: Transport> {
    pub(super) transport: T,
}

impl<T: Transport> GithubClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn resolve_person(&self, login: &str) -> Result<Identity, ConnectionError> {
        if login.is_empty()
            || login.len() > 39
            || login.starts_with('-')
            || login.ends_with('-')
            || !login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(ConnectionError::InvalidResponse);
        }
        let (user, _) = self.read(&format!("/users/{login}"))?;
        verify_identity(&user, None)
    }

    pub fn connect(
        &self,
        repository: &str,
        expected_account_id: Option<&str>,
    ) -> Result<Connection, ConnectionError> {
        let name = crate::storage::canonical_repository(repository)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (user, _) = self.read("/user")?;
        let identity = verify_identity(&user, expected_account_id)?;
        let (repo, response) = self.read(&format!("/repos/{name}"))?;
        let id = decimal_id(&repo["id"])?;
        let remote_name = repo["full_name"]
            .as_str()
            .ok_or(ConnectionError::InvalidResponse)?;
        if remote_name.to_ascii_lowercase() != name {
            return Err(ConnectionError::RepositoryChanged);
        }
        let private = boolean(&repo, "private")?;
        let archived = boolean(&repo, "archived")?;
        let disabled = boolean(&repo, "disabled")?;
        if repo["permissions"]["pull"].as_bool() == Some(false) || disabled {
            return Err(ConnectionError::MissingReadPermission);
        }
        let (pulls, _) = self.read(&format!("/repos/{name}/pulls?state=open&per_page=1"))?;
        if !pulls.is_array() {
            return Err(ConnectionError::InvalidResponse);
        }
        let comment = if archived {
            CommentCapability::Unavailable
        } else if let Some(scopes) = response.headers.get("x-oauth-scopes") {
            if scopes
                .split(',')
                .map(str::trim)
                .any(|scope| scope == "repo" || (!private && scope == "public_repo"))
            {
                CommentCapability::Available
            } else {
                CommentCapability::Unavailable
            }
        } else {
            CommentCapability::Unknown
        };
        Ok(Connection {
            identity,
            repository: RemoteRepository { id, name },
            capabilities: Capabilities {
                read: true,
                comment,
            },
        })
    }

    pub(super) fn read(&self, path: &str) -> Result<(Value, Response), ConnectionError> {
        let response = self.transport.get(path)?;
        let rate_limit_message = response.status == 403
            && serde_json::from_slice::<Value>(&response.body)
                .ok()
                .and_then(|body| body["message"].as_str().map(str::to_ascii_lowercase))
                .is_some_and(|message| {
                    message.contains("secondary rate limit")
                        || message.contains("api rate limit exceeded")
                });
        match response.status {
            200 => (),
            401 => return Err(ConnectionError::SignedOut),
            429 => return Err(ConnectionError::RateLimited),
            403 if response
                .headers
                .get("x-ratelimit-remaining")
                .is_some_and(|v| v == "0")
                || response.headers.contains_key("retry-after")
                || rate_limit_message =>
            {
                return Err(ConnectionError::RateLimited)
            }
            403 | 404 => return Err(ConnectionError::MissingReadPermission),
            _ => return Err(ConnectionError::ProviderFailure),
        }
        let value =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        Ok((value, response))
    }
}

pub(super) fn decimal_id(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_u64()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or(ConnectionError::InvalidResponse)
}

pub(super) fn boolean(value: &Value, key: &str) -> Result<bool, ConnectionError> {
    value[key].as_bool().ok_or(ConnectionError::InvalidResponse)
}
