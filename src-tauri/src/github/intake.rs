use super::{
    metadata::PullRequest,
    provider::{Connection, GithubClient, Transport},
    ConnectionError,
};
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct RepositoryTarget {
    pub name: String,
    pub pull_request_number: Option<u64>,
}

pub fn repository_target(input: &str) -> Result<RepositoryTarget, ConnectionError> {
    let input = input.trim();
    if let Ok(name) = crate::storage::canonical_repository(input) {
        return Ok(RepositoryTarget {
            name,
            pull_request_number: None,
        });
    }
    let url = reqwest::Url::parse(input).map_err(|_| ConnectionError::InvalidRepository)?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(ConnectionError::InvalidRepository);
    }
    let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
    if !(parts.len() == 5
        || parts.len() == 6 && matches!(parts[5], "files" | "commits" | "checks" | "changes"))
        || parts[3] != "pull"
        || !parts[4].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ConnectionError::InvalidRepository);
    }
    let number = parts[4]
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or(ConnectionError::InvalidRepository)?;
    let name = crate::storage::canonical_repository(&format!("{}/{}", parts[1], parts[2]))
        .map_err(|_| ConnectionError::InvalidRepository)?;
    Ok(RepositoryTarget {
        name,
        pull_request_number: Some(number),
    })
}

#[derive(Debug, Serialize)]
pub struct ResolvedTarget {
    pub connection: Connection,
    pub pull_request: Option<PullRequest>,
}

impl<T: Transport> GithubClient<T> {
    pub fn resolve_repository_target(
        &self,
        input: &str,
        account_id: &str,
    ) -> Result<ResolvedTarget, ConnectionError> {
        let target = repository_target(input)?;
        let connection = self.connect(&target.name, Some(account_id))?;
        let pull_request = target
            .pull_request_number
            .map(|number| self.review_pull(&connection.repository, number))
            .transpose()?;
        Ok(ResolvedTarget {
            connection,
            pull_request,
        })
    }
}
