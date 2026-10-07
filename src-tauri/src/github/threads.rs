use super::{
    provider::{parse_response, GithubClient, Response, Transport},
    publication::{MutationTransport, Request},
    ConnectionError,
};
use crate::{
    publication::{Publication, RemoteState, WriteFailure},
    review::Failure,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Enough to authenticate the original roots without retaining their text or
/// pretending a compact receipt is a historical execution configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ownership {
    pub publication_id: String,
    pub configuration_id: String,
    pub account_id: String,
    pub repository_id: String,
    pub repository_name: String,
    pub pull_request_id: String,
    pub number: u64,
    pub head_sha: String,
    pub assignment_id: String,
    pub agent_id: String,
    pub review_id: String,
    pub root_ids: Vec<String>,
    pub body_hashes: BTreeSet<String>,
}

pub trait Provenance {
    fn ownership(&self) -> Result<Ownership, ConnectionError>;
}

impl Provenance for Ownership {
    fn ownership(&self) -> Result<Ownership, ConnectionError> {
        Ok(self.clone())
    }
}

impl Provenance for Publication {
    fn ownership(&self) -> Result<Ownership, ConnectionError> {
        let receipt = self
            .receipts
            .last()
            .filter(|r| r.state == RemoteState::Commented)
            .ok_or(ConnectionError::InvalidResponse)?;
        let batch = self
            .batch
            .as_ref()
            .ok_or(ConnectionError::InvalidResponse)?;
        let job = &self.review.job;
        Ok(Ownership {
            publication_id: self.id.clone(),
            configuration_id: job.configuration_id.clone(),
            account_id: job.account_id.clone(),
            repository_id: job.repository_id.clone(),
            repository_name: job.repository_name.clone(),
            pull_request_id: job.pull_request_id.clone(),
            number: job.number,
            head_sha: job.head_sha.clone(),
            assignment_id: self.review.assignment_id.clone(),
            agent_id: self.review.selection.agent.id.clone(),
            review_id: receipt.review_id.clone(),
            root_ids: receipt.comment_ids.clone(),
            body_hashes: batch.comments.iter().map(|c| body_hash(&c.body)).collect(),
        })
    }
}

fn body_hash(body: &str) -> String {
    format!("{:x}", Sha256::digest(body.as_bytes()))
}

pub trait QueryTransport: Transport {
    fn query(&self, query: &str, variables: Value) -> Result<Response, ConnectionError>;
}

const COMMENTS: &str = r#"comments(first:100,after:$commentCursor) {
  totalCount pageInfo { hasNextPage endCursor }
  nodes { fullDatabaseId body createdAt publishedAt state
    author { login ... on User { databaseId } ... on Bot { databaseId } }
    replyTo { fullDatabaseId } pullRequestReview { fullDatabaseId } originalCommit { oid }
  }
}"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    pub id: String,
    pub body: String,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub reply_to: Option<String>,
    pub review_id: Option<String>,
    pub original_commit: Option<String>,
    pub created_at: String,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thread {
    pub id: String,
    pub resolved: bool,
    pub can_reply: bool,
    pub comments: Vec<Comment>,
}

impl Thread {
    pub fn root(&self) -> Result<&Comment, ConnectionError> {
        self.comments
            .first()
            .filter(|c| c.reply_to.is_none())
            .ok_or(ConnectionError::InvalidResponse)
    }

    pub fn latest_external(&self, account: &str) -> Option<&Comment> {
        self.comments
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, comment)| {
                !(comment.author_id.as_deref() == Some(account)
                    && comment.body.contains("<!-- pr-sniper:reply:"))
            })
            .max_by_key(|(index, comment)| (&comment.published_at, *index))
            .map(|(_, comment)| comment)
    }

    pub fn latest_other_user(&self, account: &str) -> Option<&Comment> {
        self.comments
            .iter()
            .filter(|comment| {
                comment.author_id.as_deref().is_some_and(|id| id != account)
                    && !comment.body.contains("<!-- pr-sniper:")
            })
            .max_by_key(|comment| (&comment.published_at, &comment.id))
    }

    pub fn owned_by(&self, publication: &impl Provenance) -> bool {
        let Ok(root) = self.root() else {
            return false;
        };
        let Ok(proof) = publication.ownership() else {
            return false;
        };
        proof.root_ids.contains(&root.id)
            && root.author_id.as_deref() == Some(&proof.account_id)
            && root.review_id.as_deref() == Some(&proof.review_id)
            && root.original_commit.as_deref() == Some(&proof.head_sha)
            && proof.body_hashes.contains(&body_hash(&root.body))
    }
}

impl<T: QueryTransport> GithubClient<T> {
    pub(super) fn graph(&self, query: &str, variables: Value) -> Result<Value, ConnectionError> {
        let (value, response) = parse_response(self.transport.query(query, variables)?)?;
        if let Some(errors) = value.get("errors").filter(|errors| !errors.is_null()) {
            let errors = errors.as_array().ok_or(ConnectionError::InvalidResponse)?;
            if errors.iter().any(|error| error["type"] == "RATE_LIMITED") {
                return Err(response
                    .headers
                    .get("retry-after")
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|n| *n >= 0)
                    .map_or(
                        ConnectionError::RateLimited,
                        ConnectionError::RateLimitedAfter,
                    ));
            }
            if !errors.is_empty() {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        if !value["data"].is_object() {
            return Err(ConnectionError::IncompleteRead);
        }
        Ok(value["data"].clone())
    }

    pub fn owned_threads(
        &self,
        publication: &impl Provenance,
    ) -> Result<Vec<Thread>, ConnectionError> {
        self.owned_threads_at(publication, &publication.ownership()?.head_sha)
    }

    pub fn owned_threads_at(
        &self,
        publication: &impl Provenance,
        current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError> {
        let publication = publication.ownership()?;
        self.review_threads(
            &publication.repository_name,
            &publication.repository_id,
            &publication.pull_request_id,
            publication.number,
            current_head,
            Some(&publication),
        )
    }

    pub fn review_threads_at(
        &self,
        repository_name: &str,
        repository_id: &str,
        pull_request_id: &str,
        number: u64,
        current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError> {
        self.review_threads(
            repository_name,
            repository_id,
            pull_request_id,
            number,
            current_head,
            None,
        )
    }

    fn review_threads(
        &self,
        repository_name: &str,
        repository_id: &str,
        pull_request_id: &str,
        number: u64,
        current_head: &str,
        ownership: Option<&Ownership>,
    ) -> Result<Vec<Thread>, ConnectionError> {
        let name = crate::storage::canonical_repository(repository_name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (owner, name) = name
            .split_once('/')
            .ok_or(ConnectionError::InvalidRepository)?;
        let query = format!(
            r#"query($owner:String!,$name:String!,$number:Int!,$cursor:String,$commentCursor:String) {{
          repository(owner:$owner,name:$name) {{ databaseId pullRequest(number:$number) {{
            fullDatabaseId headRefOid reviewThreads(first:100,after:$cursor) {{
              totalCount pageInfo {{ hasNextPage endCursor }} nodes {{ id isResolved viewerCanReply {COMMENTS} }}
            }}
          }} }}
        }}"#
        );
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut expected = None;
        let mut count = 0;
        let mut bytes = 0;
        let mut result = Vec::new();
        loop {
            let data = self.graph(
                &query,
                json!({"owner":owner,"name":name,"number":number,
                "cursor":cursor,"commentCursor":Value::Null}),
            )?;
            let repo = &data["repository"];
            let pull = &repo["pullRequest"];
            if self::number(&repo["databaseId"])? != repository_id
                || self::number(&pull["fullDatabaseId"])? != pull_request_id
            {
                return Err(ConnectionError::RepositoryChanged);
            }
            if pull["headRefOid"].as_str() != Some(current_head) {
                return Err(ConnectionError::RevisionChanged);
            }
            let connection = &pull["reviewThreads"];
            let nodes = page(connection, &mut expected)?;
            for node in nodes {
                if !ids.insert(text(&node["id"])?) {
                    return Err(ConnectionError::IncompleteRead);
                }
                count += 1;
                if count > 1000 {
                    return Err(ConnectionError::IncompleteRead);
                }
                let comments = node["comments"]["nodes"]
                    .as_array()
                    .ok_or(ConnectionError::InvalidResponse)?;
                let Some(first) = comments.first() else {
                    if node["comments"]["totalCount"].as_u64() != Some(0) {
                        return Err(ConnectionError::IncompleteRead);
                    }
                    continue;
                };
                let root = self::number(&first["fullDatabaseId"])?;
                if ownership.is_some_and(|proof| !proof.root_ids.contains(&root)) {
                    continue;
                }
                let thread = self.complete_thread(node)?;
                if ownership.is_some_and(|proof| !thread.owned_by(proof)) {
                    return Err(ConnectionError::InvalidResponse);
                }
                bytes += thread
                    .comments
                    .iter()
                    .map(|comment| comment.body.len())
                    .sum::<usize>();
                if bytes > 2 * 1024 * 1024 {
                    return Err(ConnectionError::IncompleteRead);
                }
                result.push(thread);
            }
            cursor = next(connection, &mut seen)?;
            if cursor.is_none() {
                break;
            }
        }
        if expected != Some(count) {
            return Err(ConnectionError::IncompleteRead);
        }
        Ok(result)
    }

    pub fn owned_thread(
        &self,
        publication: &impl Provenance,
        id: &str,
    ) -> Result<Option<Thread>, ConnectionError> {
        let publication = publication.ownership()?;
        let thread =
            self.review_thread(&publication.repository_id, &publication.pull_request_id, id)?;
        if thread
            .as_ref()
            .is_some_and(|thread| !thread.owned_by(&publication))
        {
            return Err(ConnectionError::InvalidResponse);
        }
        Ok(thread)
    }

    pub fn review_thread(
        &self,
        repository_id: &str,
        pull_request_id: &str,
        id: &str,
    ) -> Result<Option<Thread>, ConnectionError> {
        let query = format!(
            r#"query($id:ID!,$commentCursor:String) {{
          node(id:$id) {{ ... on PullRequestReviewThread {{
            id isResolved viewerCanReply repository {{ databaseId }}
            pullRequest {{ fullDatabaseId }} {COMMENTS}
          }} }}
        }}"#
        );
        let data = self.graph(&query, json!({"id":id,"commentCursor":Value::Null}))?;
        let node = &data["node"];
        if node.is_null() {
            return Ok(None);
        }
        if number(&node["repository"]["databaseId"])? != repository_id
            || number(&node["pullRequest"]["fullDatabaseId"])? != pull_request_id
        {
            return Err(ConnectionError::RepositoryChanged);
        }
        let thread = self.complete_thread(node)?;
        if thread.id != id {
            return Err(ConnectionError::InvalidResponse);
        }
        Ok(Some(thread))
    }

    fn complete_thread(&self, node: &Value) -> Result<Thread, ConnectionError> {
        let id = text(&node["id"])?;
        let resolved = node["isResolved"]
            .as_bool()
            .ok_or(ConnectionError::InvalidResponse)?;
        let can_reply = node["viewerCanReply"]
            .as_bool()
            .ok_or(ConnectionError::InvalidResponse)?;
        let query = format!(
            r#"query($id:ID!,$commentCursor:String) {{
            node(id:$id) {{ ... on PullRequestReviewThread {{ id isResolved viewerCanReply {COMMENTS} }} }}
        }}"#
        );
        let mut connection = node["comments"].clone();
        let mut expected = None;
        let mut seen = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut comments = Vec::new();
        let mut bytes = 0_usize;
        loop {
            for value in page(&connection, &mut expected)? {
                if !ids.insert(number(&value["fullDatabaseId"])?) {
                    return Err(ConnectionError::IncompleteRead);
                }
                match value["state"].as_str() {
                    Some("SUBMITTED") => {
                        let comment = parse_comment(value)?;
                        bytes = bytes
                            .checked_add(
                                serde_json::to_vec(&comment)
                                    .map_err(|_| ConnectionError::InvalidResponse)?
                                    .len(),
                            )
                            .ok_or(ConnectionError::IncompleteRead)?;
                        if bytes > 1024 * 1024 {
                            return Err(ConnectionError::IncompleteRead);
                        }
                        comments.push(comment);
                    }
                    Some("PENDING") => (),
                    _ => return Err(ConnectionError::InvalidResponse),
                }
            }
            let Some(cursor) = next(&connection, &mut seen)? else {
                break;
            };
            let data = self.graph(&query, json!({"id":id,"commentCursor":cursor}))?;
            let node = &data["node"];
            if node["id"].as_str() != Some(&id)
                || node["isResolved"].as_bool() != Some(resolved)
                || node["viewerCanReply"].as_bool() != Some(can_reply)
            {
                return Err(ConnectionError::RevisionChanged);
            }
            connection = node["comments"].clone();
        }
        if expected != Some(ids.len()) {
            return Err(ConnectionError::IncompleteRead);
        }
        let thread = Thread {
            id,
            resolved,
            can_reply,
            comments,
        };
        if serde_json::to_vec(&thread)
            .map_err(|_| ConnectionError::InvalidResponse)?
            .len()
            > 1024 * 1024
        {
            return Err(ConnectionError::IncompleteRead);
        }
        Ok(thread)
    }
}

impl<T: MutationTransport> GithubClient<T> {
    pub fn reply_to_thread(
        &self,
        publication: &impl Provenance,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        let publication = publication.ownership().map_err(|error| WriteFailure {
            failure: error.into(),
            uncertain: false,
        })?;
        self.reply_to_review_thread(
            &publication.repository_name,
            publication.number,
            &publication.account_id,
            root_id,
            body,
        )
    }

    pub fn reply_to_review_thread(
        &self,
        repository_name: &str,
        number: u64,
        account_id: &str,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        let request = (|| {
            let name = crate::storage::canonical_repository(repository_name)
                .map_err(Failure::permanent)?;
            if root_id.parse::<u64>().is_err()
                || root_id == "0"
                || number == 0
                || body.chars().count() > 65_536
            {
                return Err(Failure::permanent("Invalid owned root comment identity."));
            }
            Ok(format!(
                "/repos/{name}/pulls/{}/comments/{root_id}/replies",
                number
            ))
        })()
        .map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        let response = self
            .transport
            .mutate(&request, Request::Post(json!({"body":body})))
            .map_err(|error| WriteFailure {
                failure: error.into(),
                uncertain: true,
            })?;
        if response.status != 201 {
            let uncertain = !(400..500).contains(&response.status) || response.status == 408;
            let failure = parse_response(response)
                .err()
                .map(Failure::from)
                .unwrap_or_else(|| {
                    Failure::permanent("GitHub did not confirm creation of the reply.")
                });
            return Err(WriteFailure { failure, uncertain });
        }
        let parsed = (|| {
            let value: Value = serde_json::from_slice(&response.body)
                .map_err(|_| ConnectionError::InvalidResponse)?;
            if value["body"].as_str() != Some(body)
                || self::number(&value["user"]["id"])? != account_id
                || self::number(&value["in_reply_to_id"])? != root_id
                || value["pull_request_url"].as_str()
                    != Some(&format!(
                        "https://api.github.com/repos/{}/pulls/{}",
                        repository_name, number
                    ))
            {
                return Err(ConnectionError::InvalidResponse);
            }
            self::number(&value["id"])
        })();
        parsed.map_err(|error| WriteFailure {
            failure: error.into(),
            uncertain: true,
        })
    }
}

fn number(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .ok_or(ConnectionError::InvalidResponse)
}
fn text(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or(ConnectionError::InvalidResponse)
}
fn parse_comment(value: &Value) -> Result<Comment, ConnectionError> {
    let optional_id = |value: &Value| {
        if value.is_null() {
            Ok(None)
        } else {
            number(value).map(Some)
        }
    };
    Ok(Comment {
        id: number(&value["fullDatabaseId"])?,
        body: text(&value["body"])?,
        author_id: optional_id(&value["author"]["databaseId"])?,
        author_login: value["author"]["login"].as_str().map(String::from),
        reply_to: optional_id(&value["replyTo"]["fullDatabaseId"])?,
        review_id: optional_id(&value["pullRequestReview"]["fullDatabaseId"])?,
        original_commit: value["originalCommit"]["oid"].as_str().map(String::from),
        created_at: text(&value["createdAt"])?,
        published_at: text(&value["publishedAt"])?,
    })
}
fn page<'a>(
    connection: &'a Value,
    expected: &mut Option<usize>,
) -> Result<&'a Vec<Value>, ConnectionError> {
    let count = connection["totalCount"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(ConnectionError::InvalidResponse)?;
    if expected.is_some_and(|old| old != count) {
        return Err(ConnectionError::IncompleteRead);
    }
    *expected = Some(count);
    let nodes = connection["nodes"]
        .as_array()
        .ok_or(ConnectionError::InvalidResponse)?;
    if nodes.len() > 100 {
        return Err(ConnectionError::InvalidResponse);
    }
    Ok(nodes)
}
fn next(
    connection: &Value,
    seen: &mut BTreeSet<String>,
) -> Result<Option<String>, ConnectionError> {
    if !connection["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or(ConnectionError::InvalidResponse)?
    {
        return Ok(None);
    }
    let cursor = text(&connection["pageInfo"]["endCursor"])?;
    if !seen.insert(cursor.clone()) || seen.len() > 10_000 {
        return Err(ConnectionError::IncompleteRead);
    }
    Ok(Some(cursor))
}
