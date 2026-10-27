//! Provider-neutral repository interfaces. Feature code reads accounts,
//! repositories, pull requests, changed files and file content, and reads and
//! writes review threads, publications, reviewer outcomes and merge readiness
//! only through these traits; each provider supplies one implementation.

mod capability;
mod readiness;
mod threads;
mod types;
pub use capability::*;
pub use readiness::{
    Action, HumanThread, JobLog, MergeMethod, NormalizedComment, NormalizedSnapshot, Observation,
    OutcomeReceipt, OwnWrites, PrSnapshot, ProviderReview, SnapshotCheck, SnapshotComment,
    SnapshotReview, SnapshotThread, StateMarks, BEHIND_BASE,
};
pub use threads::{Comment, Ownership, Provenance, Thread, TopComment};
pub use types::*;

use crate::{
    publication::{Mutation, Publication, Receipt, WriteFailure},
    review::Failure,
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

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
    /// Every pull request, open and closed. Unlike `poll_pull_requests`, each
    /// one carries its changed files.
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

/// Review threads and top-level comments on one pull request. Threads are
/// created by publication ([`ReviewPublisher`]).
pub trait ThreadReader {
    /// Threads rooted in one publication, at that publication's head.
    fn owned_threads(&self, publication: &dyn Provenance) -> Result<Vec<Thread>, ConnectionError>;
    /// Threads rooted in one publication, judged at `current_head`.
    fn owned_threads_at(
        &self,
        publication: &dyn Provenance,
        current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError>;
    /// Every review thread on the pull request, judged at `current_head`.
    fn review_threads_at(
        &self,
        repository_name: &str,
        repository_id: &str,
        pull_request_id: &str,
        number: u64,
        current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError>;
    /// One thread rooted in a publication; `None` when it is gone.
    fn owned_thread(
        &self,
        publication: &dyn Provenance,
        id: &str,
    ) -> Result<Option<Thread>, ConnectionError>;
    /// One review thread by ID; `None` when it is gone.
    fn review_thread(
        &self,
        repository_id: &str,
        pull_request_id: &str,
        id: &str,
    ) -> Result<Option<Thread>, ConnectionError>;
}

/// Replies to and resolution of review threads.
pub trait ThreadWriter {
    /// Replies to a thread rooted in one publication. Returns the reply ID.
    fn reply_to_thread(
        &self,
        publication: &dyn Provenance,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure>;
    /// Replies to any review thread as `account_id`. Returns the reply ID.
    fn reply_to_review_thread(
        &self,
        repository_name: &str,
        number: u64,
        account_id: &str,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure>;
    fn resolve_review_thread(&self, thread_id: &str, request_id: &str) -> Result<(), WriteFailure>;
    fn unresolve_review_thread(
        &self,
        thread_id: &str,
        request_id: &str,
    ) -> Result<(), WriteFailure>;
}

/// Top-level pull request comments, outside any review thread.
pub trait CommentReader {
    fn top_comments(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<TopComment>, ConnectionError>;
}

pub trait CommentWriter {
    /// Answers a top-level mention of `account`. Returns the comment ID.
    fn reply_to_mention(
        &self,
        repository: &RemoteRepository,
        number: u64,
        account: &str,
        body: &str,
    ) -> Result<String, WriteFailure>;
}

/// Verification of one review publication against the provider.
pub trait PublicationReader {
    /// The provider review this publication already created, if any.
    fn reconcile_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure>;
    /// A provider review for this publication that no longer matches it.
    fn divergent_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure>;
}

/// Review publication: creates a review with its inline threads, submits or
/// discards it.
pub trait ReviewPublisher {
    fn mutate_publication(
        &self,
        run: &Publication,
        mutation: Mutation,
    ) -> Result<Receipt, WriteFailure>;
}

/// Reviewer outcomes on one exact head. Whether a provider offers each one is
/// its [`ProviderCapabilities`]; `merge_exact` is approve-and-merge's merge.
pub trait ReviewerOutcome {
    fn approve_exact(
        &self,
        repository: &RemoteRepository,
        number: u64,
        account: &str,
        head: &str,
        body: &str,
    ) -> Result<OutcomeReceipt, WriteFailure>;
    fn merge_exact(
        &self,
        observation: &Observation,
        request_id: &str,
    ) -> Result<OutcomeReceipt, WriteFailure>;
}

/// Merge readiness and check status of pull requests.
pub trait ReadinessReader {
    /// Everything an approve or merge decision is gated on, read fresh.
    fn action_observation(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Observation, ConnectionError>;
    fn action_reviews(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ProviderReview>, ConnectionError>;
}

/// Pull request state (reviews, checks, threads, comments) for change detection.
pub trait PullRequestStateReader {
    /// One pull request's state, read fresh.
    fn pr_snapshot(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<PrSnapshot, ConnectionError>;
    /// Several pull requests' state; one the provider no longer returns is absent.
    fn pr_snapshots(
        &self,
        repository: &RemoteRepository,
        numbers: &[u64],
    ) -> Result<BTreeMap<u64, PrSnapshot>, ConnectionError>;
}

/// Logs of one check, for the review's check tools.
pub trait CheckLogReader {
    /// At most the last `tail` bytes of one check's log. `None`: there is no log
    /// to read (absent, expired or not permitted).
    fn job_log(
        &self,
        repository: &str,
        job_id: u64,
        tail: usize,
    ) -> Result<Option<JobLog>, ConnectionError>;
}

/// What a review run reads from the provider: pull requests, their file
/// content and check logs.
pub trait ReviewSource: PullRequestReader + CheckLogReader {}

impl<P: PullRequestReader + CheckLogReader + ?Sized> ReviewSource for P {}

/// Every read interface of one provider account session.
pub trait ProviderReads:
    ProviderReader
    + ReviewSource
    + ThreadReader
    + CommentReader
    + PublicationReader
    + ReadinessReader
    + PullRequestStateReader
{
}

impl<P> ProviderReads for P where
    P: ProviderReader
        + ReviewSource
        + ThreadReader
        + CommentReader
        + PublicationReader
        + ReadinessReader
        + PullRequestStateReader
        + ?Sized
{
}

/// Checked before every provider request of a guarded session.
pub type ReadGuard = Arc<dyn Fn() -> Result<(), ConnectionError> + Send + Sync>;

/// Receives publication diagnostics events.
pub type PublicationDiagnostics = Arc<dyn Fn(crate::storage::diagnostics::Event) + Send + Sync>;

/// A session whose reads can be guarded.
pub trait Guardable {
    /// A read-only session that checks `guard` before every provider request.
    fn guarded(self: Box<Self>, guard: ReadGuard) -> Box<dyn ProviderReads + Send + Sync>;
}

/// Wrappers over one provider account session.
pub trait SessionControl {
    /// The same session, reporting publication diagnostics to `sink`.
    fn with_publication_diagnostics(
        self: Box<Self>,
        sink: PublicationDiagnostics,
    ) -> Box<dyn ProviderSession + Send + Sync>;
}

/// Every read, write and readiness interface of one provider account session.
pub trait ProviderSession:
    ProviderReads
    + ThreadWriter
    + CommentWriter
    + ReviewPublisher
    + ReviewerOutcome
    + Guardable
    + SessionControl
{
}

impl<P> ProviderSession for P where
    P: ProviderReads
        + ThreadWriter
        + CommentWriter
        + ReviewPublisher
        + ReviewerOutcome
        + Guardable
        + SessionControl
        + ?Sized
{
}

/// Brings the provider interfaces into scope for method-call syntax.
pub mod prelude {
    pub use super::{
        AccountReader, CheckLogReader, CommentReader, CommentWriter, Guardable, ProviderReader,
        ProviderReads, ProviderSession, PublicationReader, PullRequestReader,
        PullRequestStateReader, ReadinessReader, RepositoryReader, ReviewPublisher, ReviewSource,
        ReviewerOutcome, SessionControl, ThreadReader, ThreadWriter,
    };
}
