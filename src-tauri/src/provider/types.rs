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

/// One person a people search found: provider ID, display handle and,
/// when the provider returns one, a mail address to tell namesakes apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonMatch {
    pub id: String,
    pub login: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
}

/// Where a people search's matches came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PeopleSource {
    /// The provider's people directory.
    Directory,
    /// Recent pull request authors and reviewers, after the directory failed.
    Recent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct People {
    pub source: PeopleSource,
    pub people: Vec<PersonMatch>,
}

/// The commits one pull request state points at: its head and the target
/// branch tip (not the merge base; see `ReviewContext::base_revision`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revision {
    pub head: String,
    pub base: String,
}

/// The provider response a malformed read came from: typed provenance for
/// `ConnectionError::MalformedResponse`, never its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderResponse {
    Identity,
    Projects,
    Repositories,
    Repository,
    PullRequests,
    PullRequest,
    Iterations,
    IterationChanges,
    Tree,
    Threads,
    Thread,
    Comment,
    Statuses,
    PolicyEvaluations,
    Reviewers,
    People,
}

/// Serialized as the plain `invalid_response` cause so the wire shape stays
/// a string; the provenance lives in the typed value and its `Debug` form.
impl Serialize for ProviderResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("invalid_response")
    }
}

/// Never read back: a stored `invalid_response` is `InvalidResponse`.
impl<'de> Deserialize<'de> for ProviderResponse {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("provider response is not stored"))
    }
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
    /// An `InvalidResponse` that names the response it came from.
    #[serde(untagged)]
    MalformedResponse(ProviderResponse),
}

impl ConnectionError {
    /// `InvalidResponse`, with or without provenance.
    pub fn is_invalid_response(self) -> bool {
        matches!(self, Self::InvalidResponse | Self::MalformedResponse(_))
    }

    /// Names `response` as the origin of an `InvalidResponse`; others pass.
    pub fn from_response(self, response: ProviderResponse) -> Self {
        match self {
            Self::InvalidResponse => Self::MalformedResponse(response),
            other => other,
        }
    }
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

/// A pull request's changed files at its head (ADR 0027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    /// Every changed file can be listed; the list was not requested.
    Visible,
    /// Every changed file, complete.
    Complete(Vec<ChangedFile>),
    /// The provider reports this many changed files but cannot list them all.
    Incomplete(u64),
}

/// Files changed between an earlier head and the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Since {
    Files(Vec<ChangedFile>),
    /// The provider list stopped at its cap: try a tree diff.
    Capped,
    /// Not an incremental change (rebase, force-push, unknown old head):
    /// use the whole pull request.
    WholePullRequest,
}

/// Paths whose tree entry (object or mode) differs between two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeDiff {
    Complete(Vec<ChangedFile>),
    /// A tree read was cut off: the diff would be partial.
    Truncated,
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

/// Which open pull requests a poll lists (ADR 0028). Admission still decides
/// every listed pull request; a scope only skips ones it can never admit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingScope {
    /// Every open pull request.
    All,
    /// Pull requests created by one of `authors` or with `reviewer` among
    /// their reviewers. `mentions`: a mention can also admit; a provider that
    /// detects mentions must then list every open pull request. Nothing set:
    /// nothing is listed, only tracked pull requests are read.
    Narrowed {
        authors: Vec<String>,
        reviewer: Option<String>,
        mentions: bool,
    },
}

/// Where a batched listing pass resumes on the next poll.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListingCursor {
    /// Index of the scope's query the pass is in.
    pub query: usize,
    /// Pull requests of that query already read.
    pub skip: u64,
    /// Tracked pull requests up to this number were read this pass.
    #[serde(default)]
    pub tracked_after: u64,
    /// Closed tracked pull requests up to this number were probed for a
    /// reopen this pass.
    #[serde(default)]
    pub closed_after: u64,
    /// Listing passes completed; counts up.
    #[serde(default)]
    pub passes: u64,
    /// Consecutive polls whose listing could not read a page.
    #[serde(default)]
    pub stalls: u32,
    /// The listing waits until then (shared retry policy); tracked reads go on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<i64>,
}

/// One poll's read: open pull requests of this batch plus tracked ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolledPullRequests {
    pub pulls: Vec<PullRequest>,
    /// `None`: the pass is complete; the next poll starts a new one.
    pub cursor: Option<ListingCursor>,
}
