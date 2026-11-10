//! Azure DevOps pull request reads (FR-31, FR-32): active pull requests per
//! repository, detail, iterations, changed files per iteration and content at
//! commits. A pull request's revision is its latest iteration's source commit
//! (`lastMergeSourceCommit`), so every new iteration is a new revision.

use super::{diff, Client, List, Reader, Repository, RepositoryKey, Request, API_VERSION};
use crate::provider::{
    listing, ChangedFile, ConnectionError, Lifecycle, Listing, ListingCursor, ListingScope, Person,
    PolledPullRequests, ProviderResponse, PullRequest, PullRequestReader, RemoteRepository,
    RequestedTeam, ReviewContext, ReviewFile, Since, TreeDiff, TreeEntry,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const PAGE_SIZE: u64 = 100;
/// Listing pages one poll reads; the pass resumes on the next poll (ADR 0028).
const BATCH_PAGES: u64 = 10;
/// Pull requests re-read when a pass resumes: closures since the last poll
/// move later ones forward by as many places.
const RESUME_OVERLAP: u64 = 20;
/// Azure DevOps has no mention detection (`CommentReader::top_comments` is
/// empty), so Mentioning me never admits and never widens a listing.
const DETECTS_MENTIONS: bool = false;
const CHANGES_PAGE_SIZE: u64 = 1_000;
/// Files diffed for review; later files get no patch, like GitHub's.
const MAX_PATCH_FILES: usize = 300;

/// A changed file's content on one side of the diff.
enum Side {
    Absent,
    Blob(Vec<u8>),
    /// Not a blob, or too large to diff.
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIdentity {
    id: Option<String>,
    display_name: Option<String>,
    #[serde(default)]
    is_container: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCommit {
    commit_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRepositoryRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFork {
    repository: Option<RawRepositoryRef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawPullRequest {
    pull_request_id: u64,
    status: String,
    title: Option<String>,
    created_by: Option<RawIdentity>,
    creation_date: String,
    closed_date: Option<String>,
    #[serde(default)]
    is_draft: bool,
    repository: RawRepositoryRef,
    last_merge_source_commit: Option<RawCommit>,
    last_merge_target_commit: Option<RawCommit>,
    #[serde(default)]
    reviewers: Vec<RawIdentity>,
    fork_source: Option<RawFork>,
}

/// One pull request iteration: a push to the source branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iteration {
    pub id: u64,
    pub source: String,
    pub target: Option<String>,
    /// Merge base of source and target.
    pub common: Option<String>,
}

fn commit(value: &str) -> Result<String, ConnectionError> {
    let valid = value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit());
    valid
        .then(|| value.to_ascii_lowercase())
        .ok_or(ConnectionError::InvalidResponse)
}

fn timestamp(value: &str) -> Result<chrono::DateTime<chrono::FixedOffset>, ConnectionError> {
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| ConnectionError::InvalidResponse)
}

fn identity(raw: RawIdentity) -> Option<Person> {
    let id = crate::azure_devops::tenant::TenantId::parse(raw.id.as_deref()?)?;
    Some(Person {
        id: id.as_str().to_owned(),
        login: super::bounded(raw.display_name)?,
    })
}

/// Repository-relative path without the leading `/`.
fn relative(path: &str) -> Result<String, ConnectionError> {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(ConnectionError::InvalidResponse);
    }
    Ok(path.to_owned())
}

impl RawPullRequest {
    pub(super) fn into_pull_request(
        self,
        key: &RepositoryKey,
    ) -> Result<PullRequest, ConnectionError> {
        let repository = crate::azure_devops::tenant::TenantId::parse(&self.repository.id)
            .ok_or(ConnectionError::InvalidResponse)?;
        if repository != key.repository_id {
            return Err(ConnectionError::RepositoryChanged);
        }
        let state = match self.status.as_str() {
            "active" => Lifecycle::Open,
            "completed" => Lifecycle::Merged,
            "abandoned" => Lifecycle::Closed,
            _ => return Err(ConnectionError::InvalidResponse),
        };
        let created = timestamp(&self.creation_date)?;
        let updated = match self.closed_date.as_deref() {
            Some(closed) => timestamp(closed)?.max(created),
            None => created,
        };
        let head_sha = commit(
            &self
                .last_merge_source_commit
                .ok_or(ConnectionError::InvalidResponse)?
                .commit_id,
        )?;
        let base_sha = commit(
            &self
                .last_merge_target_commit
                .ok_or(ConnectionError::InvalidResponse)?
                .commit_id,
        )?;
        let head_repository_id = match self.fork_source {
            None => Some(key.id()),
            Some(fork) => fork
                .repository
                .and_then(|repository| crate::azure_devops::tenant::TenantId::parse(&repository.id))
                .map(|repository| {
                    RepositoryKey {
                        repository_id: repository,
                        ..key.clone()
                    }
                    .id()
                }),
        };
        let mut requested_reviewers = Vec::new();
        let mut requested_teams = Vec::new();
        for reviewer in self.reviewers {
            let container = reviewer.is_container;
            let Some(person) = identity(reviewer) else {
                continue;
            };
            if container {
                requested_teams.push(RequestedTeam {
                    id: person.id,
                    slug: person.login,
                });
            } else {
                requested_reviewers.push(person);
            }
        }
        Ok(PullRequest {
            mentioned: false,
            id: self.pull_request_id.to_string(),
            number: self.pull_request_id,
            title: super::bounded(self.title).unwrap_or_default(),
            author: self.created_by.and_then(identity),
            requested_reviewers,
            requested_teams,
            state,
            draft: self.is_draft,
            head_sha,
            base_sha,
            head_repository_id,
            base_repository_id: key.id(),
            updated_at: updated.to_rfc3339(),
            files: Vec::new(),
        })
    }
}

/// One server-side query of a listing scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query<'a> {
    All,
    Creator(&'a str),
    Reviewer(&'a str),
}

/// The queries whose union holds every pull request `scope` may admit.
fn queries(scope: &ListingScope) -> Vec<Query<'_>> {
    match scope {
        ListingScope::All => vec![Query::All],
        ListingScope::Narrowed { mentions: true, .. } if DETECTS_MENTIONS => vec![Query::All],
        ListingScope::Narrowed {
            authors, reviewer, ..
        } => {
            // Only identity IDs can be authors or reviewers here; others
            // match no Azure DevOps pull request, listed or not.
            let identity = |id: &&str| {
                crate::azure_devops::tenant::TenantId::parse(id)
                    .is_some_and(|parsed| parsed.as_str() == *id)
            };
            let authors: BTreeSet<&str> = authors
                .iter()
                .map(String::as_str)
                .filter(identity)
                .collect();
            authors
                .into_iter()
                .map(Query::Creator)
                .chain(reviewer.as_deref().filter(identity).map(Query::Reviewer))
                .collect()
        }
    }
}

/// Adds a page's open pull requests not seen yet. A pull request that does
/// not parse is skipped, not fatal: it cannot be reviewed either, and one
/// such pull request must not stop a listing of thousands.
fn collect(
    key: &RepositoryKey,
    page: Vec<Value>,
    pulls: &mut BTreeMap<u64, PullRequest>,
) -> Result<(), ConnectionError> {
    for value in page {
        let Ok(raw) = serde_json::from_value::<RawPullRequest>(value) else {
            continue;
        };
        match raw.into_pull_request(key) {
            Ok(pull) if pull.state == Lifecycle::Open => {
                pulls.entry(pull.number).or_insert(pull);
            }
            Ok(_) => {}
            Err(ConnectionError::RepositoryChanged) => {
                return Err(ConnectionError::RepositoryChanged)
            }
            Err(_) => {}
        }
    }
    Ok(())
}

/// A whole-tree read that a very large repository cannot serve: past the
/// body cap, or not downloaded within the request timeout (which the
/// transport reports as a lost connection). A real outage fails the next
/// read anyway.
fn tree_too_large(error: ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::LimitExceeded | ConnectionError::Network | ConnectionError::Timeout
    )
}

/// A tree entry of one object kind (`blob`, or `commit` for a submodule).
fn entry(path: &str, kind: &str, sha: &str) -> TreeEntry {
    TreeEntry {
        path: path.to_owned(),
        mode: if kind == "commit" { "160000" } else { "100644" }.into(),
        sha: sha.to_owned(),
        kind: kind.to_owned(),
    }
}

/// One changed file with its earlier object (`originalObjectId`) when the
/// provider names one, and its object kind.
struct ChangeEntry {
    file: ChangedFile,
    original: Option<String>,
    kind: String,
}

impl Client {
    pub(super) fn git_url(
        &self,
        key: &RepositoryKey,
        path: &[&str],
        query: &[(&str, &str)],
    ) -> String {
        let mut segments = vec![
            key.project_id.as_str(),
            "_apis",
            "git",
            "repositories",
            key.repository_id.as_str(),
        ];
        segments.extend_from_slice(path);
        let mut query = query.to_vec();
        query.push(("api-version", API_VERSION));
        self.url(&segments, &query)
    }

    pub(super) fn owns(&self, key: &RepositoryKey) -> Result<(), ConnectionError> {
        if key.organization == self.organization {
            Ok(())
        } else {
            Err(ConnectionError::RepositoryUnavailable)
        }
    }

    /// Every active pull request, drafts included, by number. Pages that
    /// moved while read are de-duplicated; nothing is capped.
    pub async fn active_pull_requests(
        &self,
        key: &RepositoryKey,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        self.owns(key)?;
        let mut pulls = BTreeMap::new();
        let mut skip = 0;
        loop {
            let page = self.active_page(key, Query::All, skip).await?;
            let count = page.len() as u64;
            collect(key, page, &mut pulls)?;
            if count < PAGE_SIZE {
                return Ok(pulls.into_values().collect());
            }
            skip += count;
        }
    }

    /// One page of active pull requests of `query`, newest first, unparsed.
    async fn active_page(
        &self,
        key: &RepositoryKey,
        query: Query<'_>,
        skip: u64,
    ) -> Result<Vec<Value>, ConnectionError> {
        let (top, skip) = (PAGE_SIZE.to_string(), skip.to_string());
        let mut criteria = match query {
            Query::All => Vec::new(),
            Query::Creator(id) => vec![("searchCriteria.creatorId", id)],
            Query::Reviewer(id) => vec![("searchCriteria.reviewerId", id)],
        };
        criteria.extend([
            ("searchCriteria.status", "active"),
            ("$top", top.as_str()),
            ("$skip", skip.as_str()),
        ]);
        let url = self.git_url(key, &["pullrequests"], &criteria);
        let (list, _): (List<Value>, _) = self.get(ProviderResponse::PullRequests, url).await?;
        if list.value.len() as u64 > PAGE_SIZE {
            return Err(ConnectionError::MalformedResponse(
                ProviderResponse::PullRequests,
            ));
        }
        Ok(list.value)
    }

    /// One poll's batch (ADR 0028): up to `BATCH_PAGES` listing pages of
    /// `scope` from `cursor`, then tracked pull requests the batch missed and
    /// a slice of closed ones (a reopen the scope may not list). Scale is
    /// never a failure: a listing read that fails for a reason a later poll
    /// can clear ends the batch where it stopped; only access, identity and
    /// repository failures surface. A throttled listing is the poll's rate
    /// limit: shared backpressure, nothing committed.
    pub async fn poll_batch(
        &self,
        key: &RepositoryKey,
        tracked: &[(String, u64)],
        closed: &[(String, u64)],
        scope: &ListingScope,
        cursor: Option<&ListingCursor>,
        now: i64,
    ) -> Result<PolledPullRequests, ConnectionError> {
        self.owns(key)?;
        let queries = queries(scope);
        let mut next = cursor
            .filter(|cursor| cursor.query < queries.len().max(1))
            .cloned()
            .unwrap_or_default();
        let mut pulls = BTreeMap::new();
        if next.retry_at.is_none_or(|at| now >= at) {
            next.retry_at = None;
            self.list_batch(key, &queries, &mut next, &mut pulls, now)
                .await?;
        }
        let (open, closed) = listing::tracked_reads(
            |id| pulls.values().any(|pull: &PullRequest| pull.id == id),
            tracked,
            closed,
            &mut next,
        );
        for ((id, number), reopen) in open
            .iter()
            .map(|pull| (pull, false))
            .chain(closed.iter().map(|pull| (pull, true)))
        {
            let pull = self.pull_request(key, *number).await?;
            if &pull.id != id {
                return Err(ConnectionError::RepositoryChanged);
            }
            // A closed probe matters only when it reopened.
            if !reopen || pull.state == Lifecycle::Open {
                pulls.insert(pull.number, pull);
            }
        }
        Ok(PolledPullRequests {
            pulls: pulls.into_values().collect(),
            cursor: (next != ListingCursor::default()).then_some(next),
        })
    }

    async fn list_batch(
        &self,
        key: &RepositoryKey,
        queries: &[Query<'_>],
        next: &mut ListingCursor,
        pulls: &mut BTreeMap<u64, PullRequest>,
        now: i64,
    ) -> Result<(), ConnectionError> {
        // Listings are newest first: when a pass resumes, the head page shows
        // pull requests opened since it began.
        let mut batch = listing::Batch::start(next, queries.len(), RESUME_OVERLAP);
        while let Some((query, skip)) = batch.next(BATCH_PAGES) {
            match self.active_page(key, queries[query], skip).await {
                Ok(page) => {
                    let count = page.len() as u64;
                    collect(key, page, pulls)?;
                    batch.read(count, PAGE_SIZE);
                }
                Err(error) => {
                    // Throttling is backpressure (ADR 0026), never a scale
                    // failure: the poll waits under the shared gate, commits
                    // nothing, and resumes from the last committed cursor.
                    if let Some(seconds) =
                        listing::stated_wait(error).filter(|_| listing::throttled(error))
                    {
                        self.pacer.hold(
                            u64::try_from(seconds)
                                .unwrap_or_default()
                                .saturating_mul(1_000),
                        );
                    }
                    return batch.failed(error, now, &key.id());
                }
            }
        }
        batch.finish();
        Ok(())
    }

    pub async fn pull_request(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<PullRequest, ConnectionError> {
        self.owns(key)?;
        let url = self.git_url(key, &["pullrequests", &number.to_string()], &[]);
        let (raw, _): (RawPullRequest, _) = self.get(ProviderResponse::PullRequest, url).await?;
        let pull = raw
            .into_pull_request(key)
            .map_err(|error| error.from_response(ProviderResponse::PullRequest))?;
        if pull.number != number {
            return Err(ConnectionError::RepositoryChanged);
        }
        Ok(pull)
    }

    /// Iterations in push order.
    pub async fn iterations(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<Vec<Iteration>, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            id: u64,
            source_ref_commit: RawCommit,
            target_ref_commit: Option<RawCommit>,
            common_ref_commit: Option<RawCommit>,
        }
        self.owns(key)?;
        let url = self.git_url(
            key,
            &["pullRequests", &number.to_string(), "iterations"],
            &[],
        );
        let (list, _): (List<Raw>, _) = self.get(ProviderResponse::Iterations, url).await?;
        let optional = |raw: Option<RawCommit>| raw.map(|raw| commit(&raw.commit_id)).transpose();
        let mut iterations = list
            .value
            .into_iter()
            .map(|raw| {
                Ok(Iteration {
                    id: raw.id,
                    source: commit(&raw.source_ref_commit.commit_id)?,
                    target: optional(raw.target_ref_commit)?,
                    common: optional(raw.common_ref_commit)?,
                })
            })
            .collect::<Result<Vec<_>, ConnectionError>>()
            .map_err(|error| error.from_response(ProviderResponse::Iterations))?;
        iterations.sort_by_key(|iteration| iteration.id);
        if iterations.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ConnectionError::IncompleteRead);
        }
        Ok(iterations)
    }

    /// The latest iteration, which must be the revision `head`.
    pub async fn latest_iteration(
        &self,
        key: &RepositoryKey,
        number: u64,
        head: &str,
    ) -> Result<Iteration, ConnectionError> {
        let latest =
            self.iterations(key, number)
                .await?
                .pop()
                .ok_or(ConnectionError::MalformedResponse(
                    ProviderResponse::Iterations,
                ))?;
        if latest.source != head {
            return Err(ConnectionError::RevisionChanged);
        }
        Ok(latest)
    }

    /// Files changed by the pull request up to `iteration`, against its base.
    pub async fn iteration_changes(
        &self,
        key: &RepositoryKey,
        number: u64,
        iteration: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        self.iteration_changes_since(key, number, iteration, 0)
            .await
    }

    /// Files changed from iteration `compare_to` (0: the base) to `iteration`,
    /// paged to the end. No file-count limit.
    pub async fn iteration_changes_since(
        &self,
        key: &RepositoryKey,
        number: u64,
        iteration: u64,
        compare_to: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        Ok(self
            .iteration_change_entries(key, number, iteration, compare_to)
            .await?
            .into_iter()
            .map(|change| change.file)
            .collect())
    }

    /// Like `iteration_changes_since`, with each file's earlier object and
    /// object kind.
    async fn iteration_change_entries(
        &self,
        key: &RepositoryKey,
        number: u64,
        iteration: u64,
        compare_to: u64,
    ) -> Result<Vec<ChangeEntry>, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawItem {
            path: Option<String>,
            object_id: Option<String>,
            original_object_id: Option<String>,
            #[serde(default)]
            is_folder: bool,
            git_object_type: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawChange {
            item: RawItem,
            /// `none`, the enum default, may be omitted.
            #[serde(default)]
            change_type: String,
            original_path: Option<String>,
            source_server_item: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            change_entries: Vec<RawChange>,
            next_skip: Option<u64>,
        }
        self.owns(key)?;
        let (number, iteration) = (number.to_string(), iteration.to_string());
        let compare_to = compare_to.to_string();
        let top = CHANGES_PAGE_SIZE.to_string();
        let mut files = Vec::new();
        let mut paths = BTreeSet::new();
        let mut skip = 0;
        loop {
            let skip_text = skip.to_string();
            let url = self.git_url(
                key,
                &["pullRequests", &number, "iterations", &iteration, "changes"],
                &[
                    ("$top", &top),
                    ("$skip", &skip_text),
                    ("$compareTo", &compare_to),
                ],
            );
            let (raw, _): (Raw, _) = self.get(ProviderResponse::IterationChanges, url).await?;
            let malformed = ConnectionError::MalformedResponse(ProviderResponse::IterationChanges);
            for change in raw.change_entries {
                let item = change.item;
                if item.is_folder || item.git_object_type.as_deref() == Some("tree") {
                    continue;
                }
                let path = relative(item.path.as_deref().ok_or(malformed)?)
                    .map_err(|error| error.from_response(ProviderResponse::IterationChanges))?;
                if !paths.insert(path.clone()) {
                    return Err(ConnectionError::IncompleteRead);
                }
                let kind = change.change_type.to_ascii_lowercase();
                let status = if kind.contains("delete") {
                    "removed"
                } else if kind.contains("rename") {
                    "renamed"
                } else if kind.contains("add") {
                    "added"
                } else {
                    "modified"
                };
                let previous_path = (status == "renamed")
                    .then(|| change.original_path.or(change.source_server_item))
                    .flatten()
                    .map(|path| relative(&path))
                    .transpose()
                    .map_err(|error| error.from_response(ProviderResponse::IterationChanges))?;
                if status == "renamed" && previous_path.is_none() {
                    return Err(ConnectionError::IncompleteRead);
                }
                let original = item
                    .original_object_id
                    .as_deref()
                    .and_then(|id| commit(id).ok());
                let sha = if status == "removed" {
                    item.original_object_id.or(item.object_id)
                } else {
                    item.object_id
                };
                let kind = item.git_object_type.unwrap_or_else(|| "blob".into());
                files.push(ChangeEntry {
                    file: ChangedFile {
                        path,
                        previous_path,
                        status: status.into(),
                        additions: 0,
                        deletions: 0,
                        changes: 0,
                        sha: commit(&sha.ok_or(malformed)?).map_err(|_| malformed)?,
                    },
                    original,
                    kind,
                });
            }
            // `nextSkip` absent or 0 ends the listing; one that does not
            // advance would end it short.
            match raw.next_skip.filter(|next| *next != 0) {
                Some(next) if next > skip => skip = next,
                Some(_) => return Err(ConnectionError::IncompleteRead),
                None => return Ok(files),
            }
        }
    }

    /// The full tree at `commit`, keyed by repository-relative path.
    pub async fn tree(
        &self,
        key: &RepositoryKey,
        commit_id: &str,
    ) -> Result<BTreeMap<String, TreeEntry>, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawItem {
            path: String,
            object_id: String,
            git_object_type: String,
        }
        self.owns(key)?;
        let url = self.git_url(
            key,
            &["items"],
            &[
                ("scopePath", "/"),
                ("recursionLevel", "Full"),
                ("versionDescriptor.version", commit_id),
                ("versionDescriptor.versionType", "commit"),
            ],
        );
        let response = self.call(Request::get(url)).await?;
        if response.oversize {
            return Err(ConnectionError::LimitExceeded);
        }
        let list: List<RawItem> = super::parse(ProviderResponse::Tree, &response.body)?;
        let malformed = ConnectionError::MalformedResponse(ProviderResponse::Tree);
        let mut tree = BTreeMap::new();
        for item in list.value {
            if item.path == "/" {
                continue;
            }
            let mode = match item.git_object_type.as_str() {
                "blob" => "100644",
                "tree" => "040000",
                "commit" => "160000",
                _ => return Err(malformed),
            };
            let path = relative(&item.path).map_err(|_| malformed)?;
            let entry = TreeEntry {
                path: path.clone(),
                mode: mode.into(),
                sha: commit(&item.object_id).map_err(|_| malformed)?,
                kind: item.git_object_type,
            };
            if tree.insert(path, entry).is_some() {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        Ok(tree)
    }

    /// Review trees of only the changed files (ADR 0028): head holds what
    /// the PR leaves, base what it changed. An earlier object the iteration
    /// changes do not name is read at `base`.
    async fn changed_trees(
        &self,
        key: &RepositoryKey,
        entries: &[ChangeEntry],
        base: &str,
    ) -> Result<(BTreeMap<String, TreeEntry>, BTreeMap<String, TreeEntry>), ConnectionError> {
        let (mut head_tree, mut base_tree) = (BTreeMap::new(), BTreeMap::new());
        for ChangeEntry {
            file,
            original,
            kind,
        } in entries
        {
            if file.status != "removed" {
                head_tree.insert(file.path.clone(), entry(&file.path, kind, &file.sha));
            }
            let path = file.previous_path.as_deref().unwrap_or(&file.path);
            let before = match (file.status.as_str(), original) {
                ("added", _) => None,
                ("removed", _) => Some(entry(path, kind, &file.sha)),
                (_, Some(original)) => Some(entry(path, kind, original)),
                (_, None) => Some(self.item(key, path, base).await?),
            };
            if let Some(before) = before {
                base_tree.insert(path.to_owned(), before);
            }
        }
        Ok((head_tree, base_tree))
    }

    /// One tree entry at `commit_id` by repository-relative path.
    async fn item(
        &self,
        key: &RepositoryKey,
        path: &str,
        commit_id: &str,
    ) -> Result<TreeEntry, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawItem {
            object_id: String,
            git_object_type: String,
        }
        let absolute = format!("/{path}");
        let url = self.git_url(
            key,
            &["items"],
            &[
                ("path", absolute.as_str()),
                ("versionDescriptor.version", commit_id),
                ("versionDescriptor.versionType", "commit"),
                ("$format", "json"),
            ],
        );
        let (raw, _): (RawItem, _) = self.get(ProviderResponse::Tree, url).await?;
        let malformed = ConnectionError::MalformedResponse(ProviderResponse::Tree);
        if !matches!(raw.git_object_type.as_str(), "blob" | "commit") {
            return Err(malformed);
        }
        Ok(entry(
            path,
            &raw.git_object_type,
            &commit(&raw.object_id).map_err(|_| malformed)?,
        ))
    }

    /// One blob's bytes by object ID.
    pub async fn blob(
        &self,
        key: &RepositoryKey,
        object_id: &str,
    ) -> Result<Vec<u8>, ConnectionError> {
        self.owns(key)?;
        let object_id = commit(object_id)?;
        let url = self.git_url(key, &["blobs", &object_id], &[("$format", "octetstream")]);
        let response = self.call(Request::bytes(url)).await?;
        if response.oversize {
            return Err(ConnectionError::LimitExceeded);
        }
        Ok(response.body)
    }
}

impl Reader {
    /// One side of a changed file for its patch.
    async fn side(
        &self,
        key: &RepositoryKey,
        entry: Option<&TreeEntry>,
    ) -> Result<Side, ConnectionError> {
        let Some(entry) = entry else {
            return Ok(Side::Absent);
        };
        if entry.kind != "blob" {
            return Ok(Side::Other);
        }
        match self.client.blob(key, &entry.sha).await {
            Ok(bytes) if bytes.len() <= diff::MAX_SIDE_BYTES => Ok(Side::Blob(bytes)),
            Ok(_) | Err(ConnectionError::LimitExceeded) => Ok(Side::Other),
            Err(error) => Err(error),
        }
    }

    fn key(&self, repository: &RemoteRepository) -> Result<RepositoryKey, ConnectionError> {
        let key = RepositoryKey::parse(&repository.id).ok_or(ConnectionError::InvalidRepository)?;
        self.client.owns(&key)?;
        Ok(key)
    }

    /// The stable key of a repository by ID or `{org}/{project}/{repo}` name.
    fn key_of(&self, repository: &str) -> Result<RepositoryKey, ConnectionError> {
        if let Some(key) = RepositoryKey::parse(repository) {
            self.client.owns(&key)?;
            return Ok(key);
        }
        let resolved: Repository = self.runtime.block_on(self.client.resolve(repository))?;
        Ok(resolved.key)
    }

    fn files(
        &self,
        key: &RepositoryKey,
        pull: &PullRequest,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        self.runtime.block_on(async {
            let iteration = self
                .client
                .latest_iteration(key, pull.number, &pull.head_sha)
                .await?;
            self.client
                .iteration_changes(key, pull.number, iteration.id)
                .await
        })
    }
}

impl PullRequestReader for Reader {
    fn latest_pull_request_number(
        &self,
        repository: &RemoteRepository,
    ) -> Result<u64, ConnectionError> {
        let key = self.key(repository)?;
        let url = self.client.git_url(
            &key,
            &["pullrequests"],
            &[("searchCriteria.status", "all"), ("$top", "1")],
        );
        let (list, _): (List<RawPullRequest>, _) = self
            .runtime
            .block_on(self.client.get(ProviderResponse::PullRequests, url))?;
        match list.value.as_slice() {
            [] => Ok(0),
            [_] => Ok(list
                .value
                .into_iter()
                .next()
                .map(|raw| raw.into_pull_request(&key))
                .transpose()
                .map_err(|error| error.from_response(ProviderResponse::PullRequests))?
                .map_or(0, |pull| pull.number)),
            _ => Err(ConnectionError::MalformedResponse(
                ProviderResponse::PullRequests,
            )),
        }
    }

    /// Active pull requests with their changed files; completed and abandoned
    /// ones are not listed.
    fn pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let key = self.key(repository)?;
        let mut pulls = self
            .runtime
            .block_on(self.client.active_pull_requests(&key))?;
        for pull in &mut pulls {
            pull.files = self.files(&key, pull)?;
        }
        Ok(pulls)
    }

    fn poll_pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let key = self.key(repository)?;
        self.runtime
            .block_on(self.client.active_pull_requests(&key))
    }

    /// The active listing cannot show completion or abandonment: every
    /// tracked pull request missing from it is read on its own.
    fn poll_tracked_pull_requests(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let mut pulls = self.poll_pull_requests(repository)?;
        for (id, number) in tracked {
            if pulls.iter().any(|pull| &pull.id == id) {
                continue;
            }
            let pull = self.pull_request(repository, *number)?;
            if &pull.id != id {
                return Err(ConnectionError::RepositoryChanged);
            }
            pulls.push(pull);
        }
        Ok(pulls)
    }

    fn poll_batch(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
        closed: &[(String, u64)],
        scope: &ListingScope,
        cursor: Option<&ListingCursor>,
        now: i64,
    ) -> Result<PolledPullRequests, ConnectionError> {
        let key = self.key(repository)?;
        self.runtime.block_on(
            self.client
                .poll_batch(&key, tracked, closed, scope, cursor, now),
        )
    }

    fn pull_request(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<PullRequest, ConnectionError> {
        let key = self.key(repository)?;
        self.runtime
            .block_on(self.client.pull_request(&key, number))
    }

    /// Files changed up to the latest iteration.
    fn pull_request_files(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        let key = self.key(repository)?;
        let pull = self
            .runtime
            .block_on(self.client.pull_request(&key, number))?;
        self.files(&key, &pull)
    }

    /// Azure DevOps reports no total change count: the listing paged to the
    /// end is complete. Without `list` nothing is read.
    fn changed_file_listing(
        &self,
        repository: &RemoteRepository,
        pull: &PullRequest,
        list: bool,
    ) -> Result<Listing, ConnectionError> {
        if !list {
            return Ok(Listing::Visible);
        }
        let key = self.key(repository)?;
        self.files(&key, pull).map(Listing::Complete)
    }

    /// The latest iteration's merge base (`commonRefCommit`), else its target.
    fn merge_base(
        &self,
        repository: &RemoteRepository,
        pull: &PullRequest,
    ) -> Result<String, ConnectionError> {
        let key = self.key(repository)?;
        let iteration = self.runtime.block_on(self.client.latest_iteration(
            &key,
            pull.number,
            &pull.head_sha,
        ))?;
        iteration
            .common
            .or(iteration.target)
            .ok_or(ConnectionError::MalformedResponse(
                ProviderResponse::Iterations,
            ))
    }

    /// Both full trees (`items?recursionLevel=Full`); an oversize tree is cut
    /// off. Azure DevOps change listings are never capped, so no scoped walk.
    fn tree_diff(
        &self,
        repository: &RemoteRepository,
        from: &str,
        to: &str,
        scope: &[String],
    ) -> Result<TreeDiff, ConnectionError> {
        if !scope.is_empty() {
            return Ok(TreeDiff::Truncated);
        }
        let key = self.key(repository)?;
        let tree = |commit: &str| match self.runtime.block_on(self.client.tree(&key, commit)) {
            Err(ConnectionError::LimitExceeded) => Ok(None),
            other => other.map(Some),
        };
        Ok(match (tree(from)?, tree(to)?) {
            (Some(old), Some(new)) => TreeDiff::Complete(crate::provider::tree_changes(&old, &new)),
            _ => TreeDiff::Truncated,
        })
    }

    /// Iteration changes `$compareTo` the newest iteration whose source is
    /// `old_head`; none: the whole pull request.
    fn changed_files_since(
        &self,
        repository: &RemoteRepository,
        pull: &PullRequest,
        old_head: &str,
    ) -> Result<Since, ConnectionError> {
        let key = self.key(repository)?;
        self.runtime.block_on(async {
            let iterations = self.client.iterations(&key, pull.number).await?;
            let latest = iterations.last().ok_or(ConnectionError::MalformedResponse(
                ProviderResponse::Iterations,
            ))?;
            if latest.source != pull.head_sha {
                return Err(ConnectionError::RevisionChanged);
            }
            let Some(old) = iterations.iter().rev().find(|i| i.source == old_head) else {
                return Ok(Since::WholePullRequest);
            };
            if old.id == latest.id {
                return Ok(Since::Files(Vec::new()));
            }
            self.client
                .iteration_changes_since(&key, pull.number, latest.id, old.id)
                .await
                .map(Since::Files)
        })
    }

    /// Changed files, each diffed locally from merge base to head (Azure
    /// DevOps serves no patch text), the iteration's merge base and both trees.
    fn review_context(
        &self,
        repository: &RemoteRepository,
        number: u64,
        head: &str,
    ) -> Result<ReviewContext, ConnectionError> {
        let key = self.key(repository)?;
        self.runtime.block_on(async {
            let pull = self.client.pull_request(&key, number).await?;
            if pull.head_sha != head {
                return Err(ConnectionError::RevisionChanged);
            }
            let iteration = self.client.latest_iteration(&key, number, head).await?;
            let base_revision = iteration
                .common
                .clone()
                .or(iteration.target.clone())
                .ok_or(ConnectionError::MalformedResponse(
                    ProviderResponse::Iterations,
                ))?;
            let entries = self
                .client
                .iteration_change_entries(&key, number, iteration.id, 0)
                .await?;
            let whole = async {
                Ok::<_, ConnectionError>((
                    self.client.tree(&key, head).await?,
                    self.client.tree(&key, &base_revision).await?,
                ))
            };
            // Fallback base entries take the head's object kind; a PR can
            // change it (a submodule replaced by a file).
            let mut unverified = BTreeSet::new();
            let (head_tree, mut base_tree) = match whole.await {
                Ok(trees) => trees,
                Err(error) if tree_too_large(error) => {
                    unverified.extend(entries.iter().filter_map(|change| {
                        let kept = !matches!(change.file.status.as_str(), "added" | "removed");
                        (kept && change.original.is_some()).then(|| {
                            change
                                .file
                                .previous_path
                                .clone()
                                .unwrap_or_else(|| change.file.path.clone())
                        })
                    }));
                    self.client
                        .changed_trees(&key, &entries, &base_revision)
                        .await?
                }
                Err(error) => return Err(error),
            };
            let changes: Vec<ChangedFile> = entries.into_iter().map(|change| change.file).collect();
            let mut files = Vec::with_capacity(changes.len());
            for (index, file) in changes.iter().enumerate() {
                let patch = if index < MAX_PATCH_FILES {
                    let base_path = file.previous_path.as_deref().unwrap_or(&file.path);
                    let old = match self.side(&key, base_tree.get(base_path)).await {
                        // A failed read of an unverified base entry: read its
                        // real kind; a non-blob has no patch side.
                        Err(error) if unverified.contains(base_path) => {
                            let entry = self.client.item(&key, base_path, &base_revision).await?;
                            if entry.kind == "blob" {
                                return Err(error);
                            }
                            base_tree.insert(base_path.to_owned(), entry);
                            Side::Other
                        }
                        other => other?,
                    };
                    let new = self.side(&key, head_tree.get(&file.path)).await?;
                    match (old, new) {
                        (Side::Blob(old), Side::Blob(new)) => diff::patch(Some(&old), Some(&new)),
                        (Side::Absent, Side::Blob(new)) => diff::patch(None, Some(&new)),
                        (Side::Blob(old), Side::Absent) => diff::patch(Some(&old), None),
                        _ => None,
                    }
                } else {
                    None
                };
                files.push(ReviewFile {
                    path: file.path.clone(),
                    previous_path: file.previous_path.clone(),
                    status: file.status.clone(),
                    patch,
                });
            }
            Ok(ReviewContext {
                files,
                pull: PullRequest {
                    files: changes,
                    ..pull
                },
                base_revision,
                head: head_tree,
                base: base_tree,
            })
        })
    }

    fn file_content(&self, repository: &str, entry: &TreeEntry) -> Result<Value, ConnectionError> {
        if entry.kind != "blob" {
            return Ok(json!({"kind": entry.kind, "sha": entry.sha, "mode": entry.mode}));
        }
        let key = self.key_of(repository)?;
        let bytes = self.runtime.block_on(self.client.blob(&key, &entry.sha))?;
        let size = bytes.len();
        match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => {
                Ok(json!({"kind":"text","text":text,"size":size,"mode":entry.mode,"sha":entry.sha}))
            }
            _ => Ok(json!({"kind":"binary","size":size,"mode":entry.mode,"sha":entry.sha})),
        }
    }
}
