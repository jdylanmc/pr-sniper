//! Provider-neutral repository read interfaces. Feature code reads accounts,
//! repositories, pull requests, changed files and file content only through
//! these traits; each provider supplies one implementation.

mod types;
pub use types::*;

use serde_json::Value;

/// The signed-in account and other people known to the provider.
pub trait AccountReader {
    fn current_identity(&self) -> Result<Person, ConnectionError>;
    fn resolve_person(&self, login: &str) -> Result<Person, ConnectionError>;
}

/// Repository discovery and resolution for one connected account.
pub trait RepositoryReader {
    fn accessible_repositories(&self) -> Result<Vec<RemoteRepository>, ConnectionError>;
    fn repository_browser(&self, identity: &Person) -> Result<RepositoryBrowser, ConnectionError>;
    fn owner_repository_browser(
        &self,
        identity: &Person,
        owner: &str,
    ) -> Result<RepositoryBrowser, ConnectionError>;
    /// Verifies the acting identity and its read access to one repository.
    fn connect(
        &self,
        repository: &str,
        expected_account_id: Option<&str>,
    ) -> Result<Connection, ConnectionError>;
    /// Resolves a pasted repository or pull request URL.
    fn resolve_repository_target(
        &self,
        input: &str,
        account_id: &str,
    ) -> Result<ResolvedTarget, ConnectionError>;
}

/// Pull request listing, polling, detail, changed files and file content.
pub trait PullRequestReader {
    fn latest_pull_request_number(
        &self,
        repository: &RemoteRepository,
    ) -> Result<u64, ConnectionError>;
    /// Every pull request, open and closed, without changed files.
    fn pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError>;
    /// Open pull requests, without changed files.
    fn poll_pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError>;
    /// Open pull requests plus every tracked one missing from that listing.
    fn poll_tracked_pull_requests(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
    ) -> Result<Vec<PullRequest>, ConnectionError>;
    fn pull_request(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<PullRequest, ConnectionError>;
    fn pull_request_files(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError>;
    /// Diff, base revision and both trees of one exact head.
    fn review_context(
        &self,
        repository: &RemoteRepository,
        number: u64,
        head: &str,
    ) -> Result<ReviewContext, ConnectionError>;
    /// One tree entry's content as a tool result: text, binary or non-blob.
    fn file_content(&self, repository: &str, entry: &TreeEntry) -> Result<Value, ConnectionError>;
}

/// Every read interface of one provider account session.
pub trait ProviderReader: AccountReader + RepositoryReader + PullRequestReader {}

impl<P: AccountReader + RepositoryReader + PullRequestReader + ?Sized> ProviderReader for P {}

/// Brings the read interfaces into scope for method-call syntax.
pub mod prelude {
    pub use super::{AccountReader, ProviderReader, PullRequestReader, RepositoryReader};
}
