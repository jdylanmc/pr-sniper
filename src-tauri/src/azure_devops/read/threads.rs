//! Azure DevOps pull request threads (FR-33): read, create inline threads,
//! reply and set status. Thread and comment IDs are global, so a write needs
//! no other context: `{repository}/pullRequests/{n}/threads/{t}` and
//! `{thread}/comments/{c}`.

use super::{Client, List, Reader, RepositoryKey, Request};
use crate::azure_devops::tenant::TenantId;
use crate::provider::{Comment, ConnectionError, Thread};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CHANGES_PAGE_SIZE: u64 = 1_000;
/// Every thread of one pull request, bounded like GitHub's thread reads.
const MAX_THREADS: usize = 1_000;
const COMMENT_MARKER: &str = "<!-- pr-sniper:comment:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThreadRef {
    pub key: RepositoryKey,
    pub number: u64,
    pub thread: u64,
}

fn number(value: &str) -> Option<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == value)
}

impl ThreadRef {
    pub fn id(&self) -> String {
        format!(
            "{}/pullRequests/{}/threads/{}",
            self.key.id(),
            self.number,
            self.thread
        )
    }

    pub fn comment_id(&self, comment: u64) -> String {
        format!("{}/comments/{comment}", self.id())
    }

    pub fn parse(id: &str) -> Option<Self> {
        let parts: Vec<&str> = id.split('/').collect();
        match parts.as_slice() {
            [org, project, repository, "pullRequests", n, "threads", t] => {
                Self::of(org, project, repository, n, t)
            }
            _ => None,
        }
    }

    /// The thread and comment number of a comment ID.
    pub fn parse_comment(id: &str) -> Option<(Self, u64)> {
        let parts: Vec<&str> = id.split('/').collect();
        match parts.as_slice() {
            [org, project, repository, "pullRequests", n, "threads", t, "comments", c] => {
                Some((Self::of(org, project, repository, n, t)?, number(c)?))
            }
            _ => None,
        }
    }

    fn of(org: &str, project: &str, repository: &str, n: &str, t: &str) -> Option<Self> {
        Some(Self {
            key: RepositoryKey::parse(&format!("{org}/{project}/{repository}"))?,
            number: number(n)?,
            thread: number(t)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPosition {
    line: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawThreadContext {
    file_path: Option<String>,
    right_file_start: Option<RawPosition>,
    left_file_start: Option<RawPosition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIterationContext {
    second_comparing_iteration: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPullRequestThreadContext {
    iteration_context: Option<RawIterationContext>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAuthor {
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComment {
    id: u64,
    #[serde(default)]
    parent_comment_id: u64,
    author: Option<RawAuthor>,
    content: Option<String>,
    published_date: Option<String>,
    last_updated_date: Option<String>,
    #[serde(default)]
    comment_type: Value,
    #[serde(default)]
    is_deleted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawThread {
    id: u64,
    #[serde(default)]
    status: Value,
    thread_context: Option<RawThreadContext>,
    pull_request_thread_context: Option<RawPullRequestThreadContext>,
    #[serde(default)]
    comments: Vec<RawComment>,
    #[serde(default)]
    is_deleted: bool,
}

/// One live comment of a thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdoComment {
    pub id: u64,
    pub parent: Option<u64>,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub content: String,
    pub published: String,
    /// Last edit, else publication.
    pub updated: String,
}

/// One review thread with at least one live, non-system comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdoThread {
    pub reference: ThreadRef,
    pub resolved: bool,
    /// Repository-relative path of an inline thread.
    pub path: Option<String>,
    /// `RIGHT` (head) or `LEFT` (base) side and line of an inline thread.
    pub line: Option<(&'static str, u64)>,
    /// The iteration the thread was placed on.
    pub iteration: Option<u64>,
    pub comments: Vec<AdoComment>,
}

/// `active` and `pending` threads are open; every other status (`fixed`,
/// `wontFix`, `closed`, `byDesign`) resolves it.
fn resolved(status: &Value) -> bool {
    match status {
        Value::String(status) => !matches!(status.as_str(), "active" | "pending" | "unknown"),
        Value::Number(status) => !matches!(status.as_u64(), Some(0 | 1 | 6)),
        _ => false,
    }
}

fn system(comment_type: &Value) -> bool {
    comment_type == "system" || comment_type.as_u64() == Some(3)
}

impl RawThread {
    fn into_thread(self, key: &RepositoryKey, number: u64) -> Option<AdoThread> {
        if self.is_deleted || self.id == 0 {
            return None;
        }
        let comments: Vec<_> = self
            .comments
            .into_iter()
            .filter(|c| !c.is_deleted && !system(&c.comment_type) && c.id > 0)
            .map(|c| {
                let author = c.author.unwrap_or(RawAuthor {
                    id: None,
                    display_name: None,
                });
                AdoComment {
                    id: c.id,
                    parent: (c.parent_comment_id > 0).then_some(c.parent_comment_id),
                    author_id: author
                        .id
                        .as_deref()
                        .and_then(TenantId::parse)
                        .map(|id| id.as_str().to_owned()),
                    author_login: super::bounded(author.display_name),
                    content: c.content.unwrap_or_default(),
                    updated: c
                        .last_updated_date
                        .or(c.published_date.clone())
                        .unwrap_or_default(),
                    published: c.published_date.unwrap_or_default(),
                }
            })
            .collect();
        if comments.is_empty() {
            return None;
        }
        let context = self.thread_context;
        let path = context
            .as_ref()
            .and_then(|c| c.file_path.as_deref())
            .and_then(|path| path.strip_prefix('/'))
            .filter(|path| !path.is_empty())
            .map(str::to_owned);
        let line = context.as_ref().and_then(|c| {
            c.right_file_start
                .as_ref()
                .map(|p| ("RIGHT", p.line))
                .or(c.left_file_start.as_ref().map(|p| ("LEFT", p.line)))
        });
        Some(AdoThread {
            reference: ThreadRef {
                key: key.clone(),
                number,
                thread: self.id,
            },
            resolved: resolved(&self.status),
            path,
            line,
            iteration: self
                .pull_request_thread_context
                .and_then(|c| c.iteration_context)
                .and_then(|c| c.second_comparing_iteration),
            comments,
        })
    }
}

/// The publication ID in a PR Sniper comment marker.
pub(super) fn publication_of(content: &str) -> Option<&str> {
    let rest = &content[content.find(COMMENT_MARKER)? + COMMENT_MARKER.len()..];
    let id = &rest[..rest.find(':')?];
    (id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')).then_some(id)
}

impl AdoThread {
    pub fn root(&self) -> Option<&AdoComment> {
        self.comments.first().filter(|c| c.parent.is_none())
    }

    /// The provider-neutral thread. Each comment's commit is the thread's
    /// iteration source commit, from `commits` (iteration -> source).
    pub fn to_thread(&self, commits: &BTreeMap<u64, String>) -> Thread {
        let commit = self.iteration.and_then(|i| commits.get(&i)).cloned();
        Thread {
            id: self.reference.id(),
            resolved: self.resolved,
            can_reply: true,
            comments: self
                .comments
                .iter()
                .map(|c| Comment {
                    id: self.reference.comment_id(c.id),
                    body: c.content.clone(),
                    author_id: c.author_id.clone(),
                    author_login: c.author_login.clone(),
                    reply_to: c.parent.map(|p| self.reference.comment_id(p)),
                    review_id: c
                        .parent
                        .is_none()
                        .then(|| publication_of(&c.content))
                        .flatten()
                        .map(str::to_owned),
                    original_commit: commit.clone(),
                    created_at: c.published.clone(),
                    published_at: c.published.clone(),
                })
                .collect(),
        }
    }
}

impl Client {
    fn threads_url(&self, key: &RepositoryKey, number: u64, path: &[&str]) -> String {
        let number = number.to_string();
        let mut segments = vec!["pullRequests", number.as_str(), "threads"];
        segments.extend_from_slice(path);
        self.git_url(key, &segments, &[])
    }

    /// Every review thread with a live, non-system comment.
    pub(super) async fn threads(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<Vec<AdoThread>, ConnectionError> {
        self.owns(key)?;
        let (list, _): (List<RawThread>, _) = self.get(self.threads_url(key, number, &[])).await?;
        if list.value.len() > MAX_THREADS {
            return Err(ConnectionError::LimitExceeded);
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut threads = Vec::new();
        for raw in list.value {
            if !seen.insert(raw.id) {
                return Err(ConnectionError::IncompleteRead);
            }
            threads.extend(raw.into_thread(key, number));
        }
        Ok(threads)
    }

    /// POSTs one new thread; `body` is the full thread payload.
    pub(super) async fn create_thread(
        &self,
        key: &RepositoryKey,
        number: u64,
        body: Value,
    ) -> Result<AdoThread, ConnectionError> {
        self.owns(key)?;
        let response = self
            .call(Request::post(self.threads_url(key, number, &[]), body))
            .await?;
        let raw: RawThread =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        raw.into_thread(key, number)
            .ok_or(ConnectionError::InvalidResponse)
    }

    /// Replies under `parent`; returns the new comment's number.
    pub(super) async fn reply(
        &self,
        thread: &ThreadRef,
        parent: u64,
        content: &str,
    ) -> Result<u64, ConnectionError> {
        #[derive(Deserialize)]
        struct Raw {
            id: u64,
        }
        self.owns(&thread.key)?;
        let url = self.threads_url(
            &thread.key,
            thread.number,
            &[&thread.thread.to_string(), "comments"],
        );
        let response = self
            .call(Request::post(
                url,
                json!({"parentCommentId": parent, "content": content, "commentType": 1}),
            ))
            .await?;
        let raw: Raw =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        (raw.id > 0)
            .then_some(raw.id)
            .ok_or(ConnectionError::InvalidResponse)
    }

    /// Sets the thread status (`fixed`, `active`, ...) without a comment.
    pub(super) async fn set_thread_status(
        &self,
        thread: &ThreadRef,
        status: &str,
    ) -> Result<(), ConnectionError> {
        self.owns(&thread.key)?;
        let url = self.threads_url(&thread.key, thread.number, &[&thread.thread.to_string()]);
        self.call(Request::patch(url, json!({"status": status})))
            .await
            .map(|_| ())
    }

    pub(super) async fn delete_comment(
        &self,
        thread: &ThreadRef,
        comment: u64,
    ) -> Result<(), ConnectionError> {
        self.owns(&thread.key)?;
        let url = self.threads_url(
            &thread.key,
            thread.number,
            &[&thread.thread.to_string(), "comments", &comment.to_string()],
        );
        self.call(Request::delete(url)).await.map(|_| ())
    }

    /// Change tracking IDs of an iteration's changed files, by repository-relative path.
    pub(super) async fn change_tracking_ids(
        &self,
        key: &RepositoryKey,
        number: u64,
        iteration: u64,
    ) -> Result<BTreeMap<String, u64>, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawItem {
            path: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawChange {
            change_tracking_id: Option<u64>,
            item: Option<RawItem>,
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
        let mut ids = BTreeMap::new();
        let mut skip = 0;
        loop {
            let skip_text = skip.to_string();
            let url = self.git_url(
                key,
                &["pullRequests", &number, "iterations", &iteration, "changes"],
                &[
                    ("$top", top.as_str()),
                    ("$skip", skip_text.as_str()),
                    ("$compareTo", "0"),
                ],
            );
            let (raw, _): (Raw, _) = self.get(url).await?;
            for change in raw.change_entries {
                let path = change
                    .item
                    .and_then(|item| item.path)
                    .and_then(|path| path.strip_prefix('/').map(str::to_owned));
                if let (Some(path), Some(id)) = (path, change.change_tracking_id) {
                    ids.insert(path, id);
                }
            }
            match raw.next_skip.filter(|next| *next > skip) {
                Some(next) => skip = next,
                None => return Ok(ids),
            }
        }
    }
}

impl Reader {
    /// Threads and the iteration source commits they were placed on, read
    /// while `head` is the latest iteration's source: checked before and after.
    pub(super) fn threads_at(
        &self,
        key: &RepositoryKey,
        number: u64,
        head: &str,
    ) -> Result<(Vec<AdoThread>, BTreeMap<u64, String>), ConnectionError> {
        self.runtime.block_on(async {
            self.client.latest_iteration(key, number, head).await?;
            let threads = self.client.threads(key, number).await?;
            let iterations = self.client.iterations(key, number).await?;
            if iterations.last().map(|i| i.source.as_str()) != Some(head) {
                return Err(ConnectionError::RevisionChanged);
            }
            let commits = iterations
                .into_iter()
                .map(|iteration| (iteration.id, iteration.source))
                .collect();
            Ok((threads, commits))
        })
    }

    /// Threads and the iteration source commits they were placed on.
    pub(super) fn threads_with_commits(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<(Vec<AdoThread>, BTreeMap<u64, String>), ConnectionError> {
        self.runtime.block_on(async {
            let threads = self.client.threads(key, number).await?;
            let commits = self
                .client
                .iterations(key, number)
                .await?
                .into_iter()
                .map(|iteration| (iteration.id, iteration.source))
                .collect();
            Ok((threads, commits))
        })
    }
}
