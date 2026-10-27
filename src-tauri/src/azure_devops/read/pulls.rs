//! Azure DevOps pull request reads (FR-31, FR-32): active pull requests per
//! repository, detail, iterations, changed files per iteration and content at
//! commits. A pull request's revision is its latest iteration's source commit
//! (`lastMergeSourceCommit`), so every new iteration is a new revision.

use super::{diff, Client, List, Reader, Repository, RepositoryKey, Request, API_VERSION};
use crate::provider::{
    ChangedFile, ConnectionError, Lifecycle, Person, PullRequest, PullRequestReader,
    RemoteRepository, RequestedTeam, ReviewContext, ReviewFile, TreeEntry,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const PAGE_SIZE: u64 = 100;
/// Active pull requests read per repository before the listing is cut off.
const MAX_PULL_REQUESTS: u64 = 5_000;
const CHANGES_PAGE_SIZE: u64 = 1_000;
/// Same file bound as GitHub reads.
const MAX_FILES: usize = 3_000;
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
    pub(super) fn into_pull_request(self, key: &RepositoryKey) -> Result<PullRequest, ConnectionError> {
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

impl Client {
    pub(super) fn git_url(&self, key: &RepositoryKey, path: &[&str], query: &[(&str, &str)]) -> String {
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

    /// Every active pull request, drafts included, in stable creation order.
    /// A pull request seen twice across pages means the listing moved.
    pub async fn active_pull_requests(
        &self,
        key: &RepositoryKey,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        self.owns(key)?;
        let mut pulls = Vec::new();
        let mut seen = HashSet::new();
        let top = PAGE_SIZE.to_string();
        let mut skip = 0;
        loop {
            if skip >= MAX_PULL_REQUESTS {
                return Err(ConnectionError::LimitExceeded);
            }
            let skip_text = skip.to_string();
            let url = self.git_url(
                key,
                &["pullrequests"],
                &[
                    ("searchCriteria.status", "active"),
                    ("$top", &top),
                    ("$skip", &skip_text),
                ],
            );
            let (list, _): (List<RawPullRequest>, _) = self.get(url).await?;
            let count = list.value.len() as u64;
            for raw in list.value {
                let pull = raw.into_pull_request(key)?;
                if pull.state != Lifecycle::Open || !seen.insert(pull.number) {
                    return Err(ConnectionError::IncompleteRead);
                }
                pulls.push(pull);
            }
            if count < PAGE_SIZE {
                pulls.sort_by_key(|pull| pull.number);
                return Ok(pulls);
            }
            skip += count;
        }
    }

    pub async fn pull_request(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<PullRequest, ConnectionError> {
        self.owns(key)?;
        let url = self.git_url(key, &["pullrequests", &number.to_string()], &[]);
        let (raw, _): (RawPullRequest, _) = self.get(url).await?;
        let pull = raw.into_pull_request(key)?;
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
        let (list, _): (List<Raw>, _) = self.get(url).await?;
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
            .collect::<Result<Vec<_>, ConnectionError>>()?;
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
        let latest = self
            .iterations(key, number)
            .await?
            .pop()
            .ok_or(ConnectionError::InvalidResponse)?;
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
        let top = CHANGES_PAGE_SIZE.to_string();
        let mut files = Vec::new();
        let mut paths = BTreeSet::new();
        let mut skip = 0;
        loop {
            let skip_text = skip.to_string();
            let url = self.git_url(
                key,
                &["pullRequests", &number, "iterations", &iteration, "changes"],
                &[("$top", &top), ("$skip", &skip_text), ("$compareTo", "0")],
            );
            let (raw, _): (Raw, _) = self.get(url).await?;
            for change in raw.change_entries {
                let item = change.item;
                if item.is_folder || item.git_object_type.as_deref() == Some("tree") {
                    continue;
                }
                let path = relative(
                    item.path
                        .as_deref()
                        .ok_or(ConnectionError::InvalidResponse)?,
                )?;
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
                    .transpose()?;
                if status == "renamed" && previous_path.is_none() {
                    return Err(ConnectionError::IncompleteRead);
                }
                let sha = if status == "removed" {
                    item.original_object_id.or(item.object_id)
                } else {
                    item.object_id
                };
                files.push(ChangedFile {
                    path,
                    previous_path,
                    status: status.into(),
                    additions: 0,
                    deletions: 0,
                    changes: 0,
                    sha: commit(&sha.ok_or(ConnectionError::InvalidResponse)?)?,
                });
                if files.len() > MAX_FILES {
                    return Err(ConnectionError::LimitExceeded);
                }
            }
            match raw.next_skip.filter(|next| *next > skip) {
                Some(next) => skip = next,
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
        let list: List<RawItem> =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        let mut tree = BTreeMap::new();
        for item in list.value {
            if item.path == "/" {
                continue;
            }
            let mode = match item.git_object_type.as_str() {
                "blob" => "100644",
                "tree" => "040000",
                "commit" => "160000",
                _ => return Err(ConnectionError::InvalidResponse),
            };
            let path = relative(&item.path)?;
            let entry = TreeEntry {
                path: path.clone(),
                mode: mode.into(),
                sha: commit(&item.object_id)?,
                kind: item.git_object_type,
            };
            if tree.insert(path, entry).is_some() {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        Ok(tree)
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
        let response = self.call(Request::get(url)).await?;
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
        let (list, _): (List<RawPullRequest>, _) = self.runtime.block_on(self.client.get(url))?;
        match list.value.as_slice() {
            [] => Ok(0),
            [_] => Ok(list
                .value
                .into_iter()
                .next()
                .map(|raw| raw.into_pull_request(&key))
                .transpose()?
                .map_or(0, |pull| pull.number)),
            _ => Err(ConnectionError::InvalidResponse),
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
                .ok_or(ConnectionError::InvalidResponse)?;
            let changes = self
                .client
                .iteration_changes(&key, number, iteration.id)
                .await?;
            let head_tree = self.client.tree(&key, head).await?;
            let base_tree = self.client.tree(&key, &base_revision).await?;
            let mut files = Vec::with_capacity(changes.len());
            for (index, file) in changes.iter().enumerate() {
                let patch = if index < MAX_PATCH_FILES {
                    let base_path = file.previous_path.as_deref().unwrap_or(&file.path);
                    let old = self.side(&key, base_tree.get(base_path)).await?;
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
