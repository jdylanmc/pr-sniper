use super::{
    provider::{GithubClient, Transport},
    ConnectionError,
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

use crate::provider::{
    CheckLogReader, Guardable, ProviderReads, ProviderSession, PublicationDiagnostics, ReadGuard,
    SessionControl,
};
pub use crate::provider::{JobLog, ReviewContext, ReviewFile, TreeEntry};

pub struct GuardedTransport<T> {
    transport: T,
    guard: Arc<dyn Fn() -> Result<(), ConnectionError> + Send + Sync>,
}

impl<T: Transport> Transport for GuardedTransport<T> {
    fn get(&self, path: &str) -> Result<super::provider::Response, ConnectionError> {
        (self.guard)()?;
        let result = self.transport.get(path)?;
        (self.guard)()?;
        Ok(result)
    }
    fn get_unauthenticated_tail(
        &self,
        url: &str,
        tail: usize,
    ) -> Result<(super::provider::Response, bool), ConnectionError> {
        (self.guard)()?;
        let result = self.transport.get_unauthenticated_tail(url, tail)?;
        (self.guard)()?;
        Ok(result)
    }
}

impl<T: super::threads::QueryTransport> super::threads::QueryTransport for GuardedTransport<T> {
    fn query(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<super::provider::Response, ConnectionError> {
        (self.guard)()?;
        let result = self.transport.query(query, variables)?;
        (self.guard)()?;
        Ok(result)
    }
}

impl<T: super::threads::QueryTransport + Send + Sync + 'static> Guardable for GithubClient<T> {
    fn guarded(self: Box<Self>, guard: ReadGuard) -> Box<dyn ProviderReads + Send + Sync> {
        Box::new(GithubClient::guarded(*self, guard))
    }
}

impl<T> SessionControl for GithubClient<T>
where
    T: super::publication::MutationTransport
        + super::threads::QueryTransport
        + Send
        + Sync
        + 'static,
{
    fn with_publication_diagnostics(
        self: Box<Self>,
        sink: PublicationDiagnostics,
    ) -> Box<dyn ProviderSession + Send + Sync> {
        Box::new(GithubClient::with_publication_diagnostics(*self, sink))
    }
}

impl<T: Transport> GithubClient<T> {
    pub fn guarded(self, guard: ReadGuard) -> GithubClient<GuardedTransport<T>> {
        let mut client = GithubClient::new(GuardedTransport {
            transport: self.transport,
            guard,
        });
        client.publication_diagnostics = self.publication_diagnostics;
        client
    }

    pub(super) fn review_tree(
        &self,
        name: &str,
        revision: &str,
    ) -> Result<BTreeMap<String, TreeEntry>, ConnectionError> {
        let (value, _) = self.read(&format!("/repos/{name}/git/trees/{revision}?recursive=1"))?;
        if value["truncated"].as_bool() == Some(true) {
            return Err(ConnectionError::LimitExceeded);
        }
        if value["truncated"].as_bool() != Some(false) {
            return Err(ConnectionError::IncompleteRead);
        }
        let entries: Vec<TreeEntry> = serde_json::from_value(value["tree"].clone())
            .map_err(|_| ConnectionError::InvalidResponse)?;
        let mut result = BTreeMap::new();
        for entry in entries {
            if entry.path.is_empty()
                || entry.sha.len() != 40
                || !entry.sha.bytes().all(|b| b.is_ascii_hexdigit())
                || !matches!(entry.kind.as_str(), "blob" | "tree" | "commit")
            {
                return Err(ConnectionError::InvalidResponse);
            }
            if result.insert(entry.path.clone(), entry).is_some() {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        Ok(result)
    }
}

impl<T: Transport> CheckLogReader for GithubClient<T> {
    /// One GitHub Actions job log, at most the last `tail` bytes. `None`: there is no log to
    /// read (absent, expired or not permitted). The API answers with a redirect to a signed
    /// blob URL, fetched without credentials.
    fn job_log(
        &self,
        name: &str,
        job_id: u64,
        tail: usize,
    ) -> Result<Option<JobLog>, ConnectionError> {
        let name = crate::storage::canonical_repository(name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let response = self
            .transport
            .get(&format!("/repos/{name}/actions/jobs/{job_id}/logs"))?;
        let keep = |mut bytes: Vec<u8>, mut truncated_head: bool| {
            if bytes.len() > tail {
                bytes.drain(..bytes.len() - tail);
                truncated_head = true;
            }
            Some(JobLog {
                bytes,
                truncated_head,
            })
        };
        match response.status {
            301 | 302 | 303 | 307 | 308 => {
                let location = response
                    .headers
                    .get("location")
                    .ok_or(ConnectionError::InvalidResponse)?;
                let (blob, dropped) = self.transport.get_unauthenticated_tail(location, tail)?;
                match blob.status {
                    200 => Ok(keep(blob.body, dropped)),
                    403 | 404 | 410 => Ok(None),
                    500..=599 => Err(ConnectionError::ProviderFailure),
                    status => Err(ConnectionError::ProviderRejectedStatus(status)),
                }
            }
            200 => Ok(keep(response.body, false)),
            404 | 410 => Ok(None),
            _ => match super::provider::parse_response(response) {
                Err(
                    ConnectionError::MissingReadPermission
                    | ConnectionError::OrganizationPolicyDenied,
                ) => Ok(None),
                Err(error) => Err(error),
                Ok(_) => Err(ConnectionError::InvalidResponse),
            },
        }
    }
}
