//! The Azure DevOps provider session: every read, write and readiness
//! interface for one bound identity. Review publication (FR-33, FR-40) has no
//! pending review to stage: `Create` posts one inline thread per frozen
//! comment, each carrying its PR Sniper marker; `Submit` only reads them back
//! and never posts again; `Discard` deletes the exact owned root comments.
//! Approve (a later delivery) and completion are never performed here.

use super::readiness::Readiness;
use super::threads::{AdoThread, ThreadRef};
use super::{Reader, RepositoryKey};
use crate::monitoring::OperationFailure;
use crate::provider::{
    AccountReader, ChangedFile, CheckLogReader, CommentCapability, CommentReader, CommentWriter,
    Connection, ConnectionError, Guardable, JobLog, Observation, OutcomeReceipt, Person,
    PrSnapshot, ProviderReads, ProviderReview, ProviderSession, Provenance,
    PublicationDiagnostics, PublicationReader, PullRequest, PullRequestReader,
    PullRequestStateReader, ReadGuard, ReadinessReader, RemoteRepository, RepositoryBrowser,
    RepositoryReader, ResolvedTarget, ReviewContext, ReviewPublisher, ReviewerOutcome,
    SessionControl, Thread, ThreadReader, ThreadWriter, TopComment, TreeEntry,
};
use crate::publication::{InlineComment, Mutation, Publication, Receipt, RemoteState, WriteFailure};
use crate::review::Failure;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Every provider interface of one Azure DevOps identity.
pub struct Session(Reader);

impl Session {
    pub fn new(reader: Reader) -> Self {
        Self(reader)
    }

    fn identity(&self) -> &Person {
        self.0.identity()
    }
}

/// Whether a failed write may have taken effect.
fn ambiguous(error: ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::Network
            | ConnectionError::Timeout
            | ConnectionError::ProviderFailure
            | ConnectionError::ProviderFailureAfter(_)
            | ConnectionError::InvalidResponse
            | ConnectionError::IncompleteRead
    )
}

fn write_failure(error: ConnectionError) -> WriteFailure {
    WriteFailure {
        failure: error.into(),
        uncertain: ambiguous(error),
    }
}

fn refused(message: &str) -> WriteFailure {
    WriteFailure {
        failure: Failure::permanent(message),
        uncertain: false,
    }
}

/// Read-back not yet consistent: verification stays pending on the retry
/// budget; nothing is posted again.
fn unconfirmed() -> Failure {
    Failure {
        kind: OperationFailure::Network,
        ..Failure::permanent(
            "Azure DevOps does not show every published thread yet. Reconciling; nothing is posted again.",
        )
    }
}

fn job_key(run: &Publication) -> Result<RepositoryKey, Failure> {
    RepositoryKey::parse(&run.review.job.repository_id)
        .ok_or_else(|| Failure::permanent("Invalid Azure DevOps repository identity."))
}

/// The thread payload of one frozen inline comment: file, line range on its
/// side and the iteration it was reviewed on.
pub(super) fn thread_payload(
    comment: &InlineComment,
    iteration: u64,
    change_tracking_id: Option<u64>,
) -> Value {
    let (start, end) = if comment.side == "LEFT" {
        ("leftFileStart", "leftFileEnd")
    } else {
        ("rightFileStart", "rightFileEnd")
    };
    let position = json!({"line": comment.line, "offset": 1});
    let mut context = json!({
        "iterationContext": {
            "firstComparingIteration": 1,
            "secondComparingIteration": iteration,
        }
    });
    if let Some(id) = change_tracking_id {
        context["changeTrackingId"] = json!(id);
    }
    json!({
        "comments": [{"parentCommentId": 0, "content": comment.body, "commentType": 1}],
        "status": "active",
        "threadContext": {
            "filePath": format!("/{}", comment.path),
            start: position,
            end: position,
        },
        "pullRequestThreadContext": context,
    })
}

impl Session {
    /// Owned threads of this publication by frozen comment index. A marked
    /// root whose text or place differs from the batch fails verification.
    fn published(&self, run: &Publication) -> Result<Vec<Option<AdoThread>>, Failure> {
        let batch = run
            .batch
            .as_ref()
            .ok_or_else(|| Failure::permanent("Publication has no frozen batch."))?;
        let key = job_key(run)?;
        let threads = self
            .0
            .runtime
            .block_on(self.0.client.threads(&key, run.review.job.number))?;
        let marker = format!("<!-- pr-sniper:comment:{}:", run.id);
        let mut found = vec![None; batch.comments.len()];
        for thread in owned_marked(&threads, &run.review.job.account_id, &marker) {
            let root = thread.root().expect("owned threads have a root");
            let index = batch
                .comments
                .iter()
                .position(|c| c.body == root.content)
                .ok_or_else(|| {
                    Failure::permanent("An Azure DevOps thread of this publication differs from it.")
                })?;
            let comment = &batch.comments[index];
            if thread.path.as_deref() != Some(comment.path.as_str())
                || thread.line != Some((side(comment), comment.line))
            {
                return Err(Failure::permanent(
                    "An Azure DevOps thread of this publication is not where it was placed.",
                ));
            }
            if found[index].replace(thread.clone()).is_some() {
                return Err(Failure::permanent(
                    "Several Azure DevOps threads match one comment of this publication; manual reconciliation required.",
                ));
            }
        }
        Ok(found)
    }

    fn receipt(run: &Publication, found: &[Option<AdoThread>]) -> Option<Receipt> {
        let comment_ids: Vec<_> = found
            .iter()
            .flatten()
            .map(|t| t.reference.comment_id(t.root().expect("root").id))
            .collect();
        if comment_ids.is_empty() {
            return None;
        }
        Some(Receipt {
            review_id: run.id.clone(),
            state: if found.iter().all(Option::is_some) {
                RemoteState::Commented
            } else {
                RemoteState::Pending
            },
            comment_ids,
        })
    }

    fn create(&self, run: &Publication) -> Result<Receipt, WriteFailure> {
        let batch = run
            .batch
            .as_ref()
            .ok_or_else(|| refused("Publication has no frozen batch."))?;
        let key = job_key(run).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        let number = run.review.job.number;
        let account = &run.review.job.account_id;
        if &self.identity().id != account {
            return Err(write_failure(ConnectionError::WrongIdentity));
        }
        let client = &self.0.client;
        self.0.runtime.block_on(async {
            let iteration = client
                .latest_iteration(&key, number, &batch.commit_id)
                .await
                .map_err(|error| WriteFailure {
                    failure: error.into(),
                    uncertain: false,
                })?;
            let tracking = client
                .change_tracking_ids(&key, number, iteration.id)
                .await
                .map_err(|error| WriteFailure {
                    failure: error.into(),
                    uncertain: false,
                })?;
            let mut comment_ids = Vec::new();
            for comment in &batch.comments {
                let payload =
                    thread_payload(comment, iteration.id, tracking.get(&comment.path).copied());
                let created = match client.create_thread(&key, number, payload).await {
                    Ok(thread) => thread,
                    Err(error) => {
                        return Err(WriteFailure {
                            // A refused first thread cannot succeed on retry.
                            failure: match error {
                                ConnectionError::ProviderRejectedStatus(400)
                                    if comment_ids.is_empty() =>
                                {
                                    Failure::tossed(
                                        "Azure DevOps rejected the thread payload; the review was tossed.",
                                    )
                                }
                                _ => error.into(),
                            },
                            uncertain: !comment_ids.is_empty() || ambiguous(error),
                        })
                    }
                };
                let root = created
                    .root()
                    .filter(|root| {
                        root.author_id.as_deref() == Some(account) && root.content == comment.body
                    })
                    .ok_or_else(|| WriteFailure {
                        failure: Failure::permanent(
                            "Azure DevOps did not confirm the created thread.",
                        ),
                        uncertain: true,
                    })?;
                comment_ids.push(created.reference.comment_id(root.id));
            }
            Ok(Receipt {
                review_id: run.id.clone(),
                state: RemoteState::Pending,
                comment_ids,
            })
        })
    }

    fn discard(&self, run: &Publication) -> Result<Receipt, WriteFailure> {
        let found = self.published(run).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        for thread in found.iter().flatten() {
            let root = thread.root().expect("root");
            self.0
                .runtime
                .block_on(self.0.client.delete_comment(&thread.reference, root.id))
                .map_err(write_failure)?;
        }
        Ok(Receipt {
            review_id: run.id.clone(),
            state: RemoteState::Deleted,
            comment_ids: vec![],
        })
    }
}

fn side(comment: &InlineComment) -> &'static str {
    if comment.side == "LEFT" {
        "LEFT"
    } else {
        "RIGHT"
    }
}

/// Threads whose root `account` wrote with `marker`.
fn owned_marked<'a>(
    threads: &'a [AdoThread],
    account: &'a str,
    marker: &'a str,
) -> impl Iterator<Item = &'a AdoThread> {
    threads.iter().filter(move |t| {
        t.root().is_some_and(|root| {
            root.author_id.as_deref() == Some(account) && root.content.contains(marker)
        })
    })
}

impl PublicationReader for Session {
    fn reconcile_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        if run.batch.is_none() {
            return Ok(None);
        }
        Ok(Self::receipt(run, &self.published(run)?))
    }

    /// Visible threads are never taken back: owned threads that no longer
    /// match the batch are the publication's receipt.
    fn divergent_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        let key = job_key(run)?;
        let threads = self
            .0
            .runtime
            .block_on(self.0.client.threads(&key, run.review.job.number))?;
        let marker = format!("<!-- pr-sniper:comment:{}:", run.id);
        let comment_ids: Vec<_> = owned_marked(&threads, &run.review.job.account_id, &marker)
            .map(|t| t.reference.comment_id(t.root().expect("root").id))
            .collect();
        Ok((!comment_ids.is_empty()).then(|| Receipt {
            review_id: run.id.clone(),
            state: RemoteState::Commented,
            comment_ids,
        }))
    }
}

impl ReviewPublisher for Session {
    fn mutate_publication(
        &self,
        run: &Publication,
        mutation: Mutation,
    ) -> Result<Receipt, WriteFailure> {
        match mutation {
            Mutation::Create => self.create(run),
            Mutation::Submit => match self.published(run) {
                Ok(found) => match Self::receipt(run, &found) {
                    Some(receipt) if receipt.state == RemoteState::Commented => Ok(receipt),
                    _ => Err(WriteFailure {
                        failure: unconfirmed(),
                        uncertain: false,
                    }),
                },
                Err(failure) => Err(WriteFailure {
                    failure,
                    uncertain: false,
                }),
            },
            Mutation::Discard => self.discard(run),
        }
    }
}

impl Session {
    fn key(&self, repository_id: &str) -> Result<RepositoryKey, ConnectionError> {
        let key = RepositoryKey::parse(repository_id).ok_or(ConnectionError::InvalidRepository)?;
        self.0.client.owns(&key)?;
        Ok(key)
    }

    fn owned(&self, publication: &dyn Provenance) -> Result<Vec<Thread>, ConnectionError> {
        let proof = publication.ownership()?;
        let key = self.key(&proof.repository_id)?;
        Ok(self
            .0
            .provider_threads(&key, proof.number)?
            .into_iter()
            .filter(|t| t.owned_by(&proof))
            .collect())
    }

    fn reply(&self, root_id: &str, number: Option<u64>, body: &str) -> Result<String, WriteFailure> {
        let (thread, root) = ThreadRef::parse_comment(root_id)
            .filter(|(thread, _)| number.is_none_or(|n| n == thread.number))
            .ok_or_else(|| refused("Unknown Azure DevOps thread."))?;
        self.0.client.owns(&thread.key).map_err(write_failure)?;
        let id = self
            .0
            .runtime
            .block_on(self.0.client.reply(&thread, root, body))
            .map_err(write_failure)?;
        Ok(thread.comment_id(id))
    }

    fn set_status(&self, thread_id: &str, status: &str) -> Result<(), WriteFailure> {
        let thread = ThreadRef::parse(thread_id).ok_or_else(|| refused("Unknown Azure DevOps thread."))?;
        self.0
            .runtime
            .block_on(self.0.client.set_thread_status(&thread, status))
            .map_err(write_failure)
    }

    fn readiness(&self, repository: &RemoteRepository, number: u64) -> Result<Readiness, ConnectionError> {
        let key = self.key(&repository.id)?;
        self.0.readiness(&key, number)
    }
}

impl ThreadReader for Session {
    fn owned_threads(&self, publication: &dyn Provenance) -> Result<Vec<Thread>, ConnectionError> {
        self.owned(publication)
    }

    fn owned_threads_at(
        &self,
        publication: &dyn Provenance,
        _current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError> {
        self.owned(publication)
    }

    fn review_threads_at(
        &self,
        _repository_name: &str,
        repository_id: &str,
        _pull_request_id: &str,
        number: u64,
        _current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError> {
        let key = self.key(repository_id)?;
        self.0.provider_threads(&key, number)
    }

    fn owned_thread(
        &self,
        publication: &dyn Provenance,
        id: &str,
    ) -> Result<Option<Thread>, ConnectionError> {
        Ok(self.owned(publication)?.into_iter().find(|t| t.id == id))
    }

    fn review_thread(
        &self,
        repository_id: &str,
        pull_request_id: &str,
        id: &str,
    ) -> Result<Option<Thread>, ConnectionError> {
        let Some(thread) = ThreadRef::parse(id) else {
            return Ok(None);
        };
        let key = self.key(repository_id)?;
        if thread.key != key || thread.number.to_string() != pull_request_id {
            return Ok(None);
        }
        Ok(self
            .0
            .provider_threads(&key, thread.number)?
            .into_iter()
            .find(|t| t.id == id))
    }
}

impl ThreadWriter for Session {
    fn reply_to_thread(
        &self,
        publication: &dyn Provenance,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        let proof = publication.ownership().map_err(write_failure)?;
        if !proof.root_ids.iter().any(|id| id == root_id) {
            return Err(refused("The thread is not rooted in this publication."));
        }
        self.reply(root_id, Some(proof.number), body)
    }

    fn reply_to_review_thread(
        &self,
        _repository_name: &str,
        number: u64,
        account_id: &str,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        if self.identity().id != account_id {
            return Err(write_failure(ConnectionError::WrongIdentity));
        }
        self.reply(root_id, Some(number), body)
    }

    /// Resolves silently: a status change, no comment.
    fn resolve_review_thread(&self, thread_id: &str, _request_id: &str) -> Result<(), WriteFailure> {
        self.set_status(thread_id, "fixed")
    }

    fn unresolve_review_thread(
        &self,
        thread_id: &str,
        _request_id: &str,
    ) -> Result<(), WriteFailure> {
        self.set_status(thread_id, "active")
    }
}

/// Azure DevOps has no top-level comments outside threads; general threads
/// are review threads.
impl CommentReader for Session {
    fn top_comments(
        &self,
        _repository: &RemoteRepository,
        _number: u64,
    ) -> Result<Vec<TopComment>, ConnectionError> {
        Ok(Vec::new())
    }
}

impl CommentWriter for Session {
    fn reply_to_mention(
        &self,
        _repository: &RemoteRepository,
        _number: u64,
        _account: &str,
        _body: &str,
    ) -> Result<String, WriteFailure> {
        Err(refused(crate::exits::gates::UNSUPPORTED))
    }
}

impl ReviewerOutcome for Session {
    fn approve_exact(
        &self,
        _repository: &RemoteRepository,
        _number: u64,
        _account: &str,
        _head: &str,
        _body: &str,
    ) -> Result<OutcomeReceipt, WriteFailure> {
        Err(refused(crate::exits::gates::UNSUPPORTED))
    }

    fn merge_exact(
        &self,
        _observation: &Observation,
        _request_id: &str,
    ) -> Result<OutcomeReceipt, WriteFailure> {
        Err(refused(crate::exits::gates::UNSUPPORTED))
    }
}

impl ReadinessReader for Session {
    fn action_observation(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Observation, ConnectionError> {
        Ok(self
            .readiness(repository, number)?
            .observation(&self.identity().id))
    }

    fn action_reviews(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ProviderReview>, ConnectionError> {
        Ok(self.action_observation(repository, number)?.reviews)
    }
}

impl PullRequestStateReader for Session {
    fn pr_snapshot(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<PrSnapshot, ConnectionError> {
        Ok(self.readiness(repository, number)?.snapshot())
    }

    fn pr_snapshots(
        &self,
        repository: &RemoteRepository,
        numbers: &[u64],
    ) -> Result<BTreeMap<u64, PrSnapshot>, ConnectionError> {
        let mut snapshots = BTreeMap::new();
        for number in numbers {
            match self.pr_snapshot(repository, *number) {
                Ok(snapshot) => {
                    snapshots.insert(*number, snapshot);
                }
                Err(ConnectionError::RepositoryUnavailable) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(snapshots)
    }
}

/// Build and status logs are not read for Azure DevOps.
impl CheckLogReader for Session {
    fn job_log(
        &self,
        _repository: &str,
        _job_id: u64,
        _tail: usize,
    ) -> Result<Option<JobLog>, ConnectionError> {
        Ok(None)
    }
}

impl Guardable for Session {
    fn guarded(mut self: Box<Self>, guard: ReadGuard) -> Box<dyn ProviderReads + Send + Sync> {
        self.0.client.guard = Some(guard);
        self
    }
}

impl SessionControl for Session {
    fn with_publication_diagnostics(
        self: Box<Self>,
        _sink: PublicationDiagnostics,
    ) -> Box<dyn ProviderSession + Send + Sync> {
        self
    }
}

impl AccountReader for Session {
    fn current_identity(&self) -> Result<Person, ConnectionError> {
        self.0.current_identity()
    }

    fn resolve_person(&self, login: &str) -> Result<Person, ConnectionError> {
        self.0.resolve_person(login)
    }
}

impl RepositoryReader for Session {
    fn accessible_repositories(&self) -> Result<Vec<RemoteRepository>, ConnectionError> {
        self.0.accessible_repositories()
    }

    fn repository_browser(&self, identity: &Person) -> Result<RepositoryBrowser, ConnectionError> {
        self.0.repository_browser(identity)
    }

    fn owner_repository_browser(
        &self,
        identity: &Person,
        owner: &str,
    ) -> Result<RepositoryBrowser, ConnectionError> {
        self.0.owner_repository_browser(identity, owner)
    }

    /// Azure DevOps exposes no cheap per-repository comment check: a reader
    /// may comment, and a refused write reports its permission failure.
    fn connect(
        &self,
        repository: &str,
        expected_account_id: Option<&str>,
    ) -> Result<Connection, ConnectionError> {
        let mut connection = self.0.connect(repository, expected_account_id)?;
        connection.capabilities.comment = CommentCapability::Available;
        Ok(connection)
    }

    fn resolve_repository_target(
        &self,
        input: &str,
        account_id: &str,
    ) -> Result<ResolvedTarget, ConnectionError> {
        self.0.resolve_repository_target(input, account_id)
    }
}

impl PullRequestReader for Session {
    fn latest_pull_request_number(
        &self,
        repository: &RemoteRepository,
    ) -> Result<u64, ConnectionError> {
        self.0.latest_pull_request_number(repository)
    }

    fn pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        self.0.pull_requests(repository)
    }

    fn poll_pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        self.0.poll_pull_requests(repository)
    }

    fn poll_tracked_pull_requests(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        self.0.poll_tracked_pull_requests(repository, tracked)
    }

    fn pull_request(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<PullRequest, ConnectionError> {
        self.0.pull_request(repository, number)
    }

    fn pull_request_files(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        self.0.pull_request_files(repository, number)
    }

    fn review_context(
        &self,
        repository: &RemoteRepository,
        number: u64,
        head: &str,
    ) -> Result<ReviewContext, ConnectionError> {
        self.0.review_context(repository, number, head)
    }

    fn file_content(&self, repository: &str, entry: &TreeEntry) -> Result<Value, ConnectionError> {
        self.0.file_content(repository, entry)
    }
}
