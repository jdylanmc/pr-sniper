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
use std::collections::BTreeSet;

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

    pub fn owned_by(&self, publication: &Publication) -> bool {
        let Ok(root) = self.root() else {
            return false;
        };
        let Some(receipt) = publication
            .receipts
            .last()
            .filter(|r| r.state == RemoteState::Commented)
        else {
            return false;
        };
        let Some(batch) = publication.batch.as_ref() else {
            return false;
        };
        receipt.comment_ids.contains(&root.id)
            && root.author_id.as_deref() == Some(&publication.review.job.account_id)
            && root.review_id.as_deref() == Some(&receipt.review_id)
            && root.original_commit.as_deref() == Some(&publication.review.job.head_sha)
            && batch.comments.iter().any(|c| c.body == root.body)
    }
}

impl<T: QueryTransport> GithubClient<T> {
    fn graph(&self, query: &str, variables: Value) -> Result<Value, ConnectionError> {
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

    pub fn owned_threads(&self, publication: &Publication) -> Result<Vec<Thread>, ConnectionError> {
        self.owned_threads_at(publication, &publication.review.job.head_sha)
    }

    pub fn owned_threads_at(
        &self,
        publication: &Publication,
        current_head: &str,
    ) -> Result<Vec<Thread>, ConnectionError> {
        let name = crate::storage::canonical_repository(&publication.review.job.repository_name)
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
        let mut result = Vec::new();
        loop {
            let data = self.graph(
                &query,
                json!({"owner":owner,"name":name,"number":publication.review.job.number,
                "cursor":cursor,"commentCursor":Value::Null}),
            )?;
            let repo = &data["repository"];
            let pull = &repo["pullRequest"];
            if number(&repo["databaseId"])? != publication.review.job.repository_id
                || number(&pull["fullDatabaseId"])? != publication.review.job.pull_request_id
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
                let comments = node["comments"]["nodes"]
                    .as_array()
                    .ok_or(ConnectionError::InvalidResponse)?;
                let Some(first) = comments.first() else {
                    if node["comments"]["totalCount"].as_u64() != Some(0) {
                        return Err(ConnectionError::IncompleteRead);
                    }
                    continue;
                };
                let root_id = number(&first["fullDatabaseId"])?;
                if !publication
                    .receipts
                    .last()
                    .is_some_and(|r| r.comment_ids.contains(&root_id))
                {
                    continue;
                }
                let thread = self.complete_thread(node)?;
                if !thread.owned_by(publication) {
                    return Err(ConnectionError::InvalidResponse);
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
        publication: &Publication,
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
        if number(&node["repository"]["databaseId"])? != publication.review.job.repository_id
            || number(&node["pullRequest"]["fullDatabaseId"])?
                != publication.review.job.pull_request_id
        {
            return Err(ConnectionError::RepositoryChanged);
        }
        let thread = self.complete_thread(node)?;
        if thread.id != id || !thread.owned_by(publication) {
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
        publication: &Publication,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        let request = (|| {
            let name =
                crate::storage::canonical_repository(&publication.review.job.repository_name)
                    .map_err(Failure::permanent)?;
            if root_id.parse::<u64>().is_err() || root_id == "0" {
                return Err(Failure::permanent("Invalid owned root comment identity."));
            }
            Ok(format!(
                "/repos/{name}/pulls/{}/comments/{root_id}/replies",
                publication.review.job.number
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
                || number(&value["user"]["id"])? != publication.review.job.account_id
                || number(&value["in_reply_to_id"])? != root_id
                || value["pull_request_url"].as_str()
                    != Some(&format!(
                        "https://api.github.com/repos/{}/pulls/{}",
                        publication.review.job.repository_name, publication.review.job.number
                    ))
            {
                return Err(ConnectionError::InvalidResponse);
            }
            number(&value["id"])
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
