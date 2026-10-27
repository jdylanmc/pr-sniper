use super::{
    provider::{parse_response, GithubClient, Response, Transport},
    publication::{MutationTransport, Request},
    ConnectionError,
};
pub use crate::provider::{Comment, Ownership, Provenance, Thread};
use crate::provider::{ThreadReader, ThreadWriter};
use crate::{publication::WriteFailure, review::Failure};
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
                    return Err(ConnectionError::LimitExceeded);
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
                    return Err(ConnectionError::LimitExceeded);
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
                            .ok_or(ConnectionError::LimitExceeded)?;
                        if bytes > 1024 * 1024 {
                            return Err(ConnectionError::LimitExceeded);
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
            return Err(ConnectionError::LimitExceeded);
        }
        Ok(thread)
    }
}

impl<T: QueryTransport> ThreadReader for GithubClient<T> {
    fn owned_threads(&self, publication: &dyn Provenance) -> Result<Vec<Thread>, ConnectionError> {
        self.owned_threads_at(publication, &publication.ownership()?.head_sha)
    }

    fn owned_threads_at(
        &self,
        publication: &dyn Provenance,
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

    fn review_threads_at(
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

    fn owned_thread(
        &self,
        publication: &dyn Provenance,
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

    fn review_thread(
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
}

impl<T: MutationTransport> ThreadWriter for GithubClient<T> {
    fn reply_to_thread(
        &self,
        publication: &dyn Provenance,
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

    fn reply_to_review_thread(
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

    /// Resolves one review thread. Callers verify ownership first; this never decides it.
    /// Resolving a resolved thread is a no-op, so an uncertain result is safe to re-verify.
    fn resolve_review_thread(&self, thread_id: &str, request_id: &str) -> Result<(), WriteFailure> {
        self.set_review_thread(thread_id, request_id, true)
    }

    /// Undoes PR Sniper's own resolution when its accepted snapshot changed during the
    /// write. Unresolving an unresolved thread is a no-op.
    fn unresolve_review_thread(
        &self,
        thread_id: &str,
        request_id: &str,
    ) -> Result<(), WriteFailure> {
        self.set_review_thread(thread_id, request_id, false)
    }
}

impl<T: MutationTransport> GithubClient<T> {
    fn set_review_thread(
        &self,
        thread_id: &str,
        request_id: &str,
        resolved: bool,
    ) -> Result<(), WriteFailure> {
        if thread_id.is_empty() || request_id.is_empty() {
            return Err(WriteFailure {
                failure: Failure::permanent("Invalid thread resolution request."),
                uncertain: false,
            });
        }
        let (field, query) = if resolved {
            ("resolveReviewThread", "mutation($input:ResolveReviewThreadInput!){resolveReviewThread(input:$input){clientMutationId thread{id isResolved}}}")
        } else {
            ("unresolveReviewThread", "mutation($input:UnresolveReviewThreadInput!){unresolveReviewThread(input:$input){clientMutationId thread{id isResolved}}}")
        };
        let response = self
            .transport
            .mutate(
                "/graphql",
                Request::Post(
                    json!({"query":query,"variables":{"input":{"threadId":thread_id,"clientMutationId":request_id}}}),
                ),
            )
            .map_err(|error| WriteFailure {
                failure: error.into(),
                uncertain: true,
            })?;
        if response.status != 200 {
            let uncertain = !(400..500).contains(&response.status) || response.status == 408;
            let failure = parse_response(response)
                .err()
                .map(Failure::from)
                .unwrap_or_else(|| {
                    Failure::permanent("GitHub did not confirm the thread resolution.")
                });
            return Err(WriteFailure { failure, uncertain });
        }
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| WriteFailure {
            failure: ConnectionError::InvalidResponse.into(),
            uncertain: true,
        })?;
        if let Some(errors) = value
            .get("errors")
            .filter(|e| !e.is_null() && e.as_array().is_none_or(|a| !a.is_empty()))
        {
            let definite = errors.as_array().is_some_and(|errors| {
                errors.iter().all(|error| {
                    matches!(
                        error["type"].as_str(),
                        Some("FORBIDDEN" | "NOT_FOUND" | "UNPROCESSABLE")
                    )
                })
            });
            return Err(WriteFailure {
                failure: Failure::permanent(
                    "GitHub refused or could not confirm the thread resolution.",
                ),
                uncertain: !definite,
            });
        }
        let data = &value["data"][field];
        if data["clientMutationId"].as_str() != Some(request_id)
            || data["thread"]["id"].as_str() != Some(thread_id)
            || data["thread"]["isResolved"] != resolved
        {
            return Err(WriteFailure {
                failure: ConnectionError::InvalidResponse.into(),
                uncertain: true,
            });
        }
        Ok(())
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
