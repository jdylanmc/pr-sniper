//! Provider-neutral read types. Every repository provider maps its wire
//! format onto these; feature code consumes only these shapes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A provider-issued account: stable ID plus its current handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub id: String,
    pub login: String,
}

/// The commits one pull request state compares: head and merge base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revision {
    pub head: String,
    pub base: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
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
    /// A provider or host size limit (3,000 files, truncated tree, body cap);
    /// retrying the same revision cannot help.
    LimitExceeded,
    RevisionChanged,
    InvalidRepository,
    RepositoryChanged,
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub identity: Person,
    pub repository: RemoteRepository,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteRepository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryOwner {
    pub login: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryBrowseWarning {
    pub boundary: &'static str,
    pub page: u64,
    pub error: ConnectionError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryBrowser {
    pub owners: Vec<RepositoryOwner>,
    pub repositories: Vec<RemoteRepository>,
    pub warnings: Vec<RepositoryBrowseWarning>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullRequest {
    /// Account-scoped comment observation supplied by the monitoring host,
    /// never inferred from a provider review request or an action permission.
    #[serde(skip_serializing)]
    pub mentioned: bool,
    pub id: String,
    pub number: u64,
    pub title: String,
    pub author: Option<Person>,
    pub requested_reviewers: Vec<Person>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct ReviewFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub mode: String,
    pub sha: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub pull: PullRequest,
    pub base_revision: String,
    pub files: Vec<ReviewFile>,
    pub head: BTreeMap<String, TreeEntry>,
    pub base: BTreeMap<String, TreeEntry>,
}

#[derive(Debug, Serialize)]
pub struct ResolvedTarget {
    pub connection: Connection,
    pub pull_request: Option<PullRequest>,
}

impl PullRequest {
    pub fn revision(&self) -> Revision {
        Revision {
            head: self.head_sha.clone(),
            base: self.base_sha.clone(),
        }
    }
}
