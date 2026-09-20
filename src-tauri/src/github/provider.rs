use super::{ConnectionError, Identity};
use std::collections::BTreeMap;

pub trait Transport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError>;
}

pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Connection {
    pub identity: Identity,
    pub repository: RemoteRepository,
    pub capabilities: Capabilities,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RemoteRepository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub read: bool,
    pub comment: CommentCapability,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommentCapability {
    Available,
    Unavailable,
    Unknown,
}

pub struct GithubClient<T: Transport> {
    _transport: T,
}

impl<T: Transport> GithubClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            _transport: transport,
        }
    }

    pub fn connect(
        &self,
        _repository: &str,
        _expected_account_id: Option<&str>,
    ) -> Result<Connection, ConnectionError> {
        Err(ConnectionError::NotImplemented)
    }
}
