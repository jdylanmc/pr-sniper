use super::provider::{GithubClient, RemoteRepository, Transport};
use super::{ConnectionError, Identity};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullRequest {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub author: Option<Identity>,
    pub requested_reviewers: Vec<Identity>,
    pub requested_teams: Vec<RequestedTeam>,
    pub state: Lifecycle,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub head_repository_id: Option<String>,
    pub base_repository_id: String,
    pub updated_at: String,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestedTeam {
    pub id: String,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub changes: u64,
    pub sha: String,
}

impl<T: Transport> GithubClient<T> {
    pub fn pull_requests(
        &self,
        _repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        Err(ConnectionError::NotImplemented)
    }
}
