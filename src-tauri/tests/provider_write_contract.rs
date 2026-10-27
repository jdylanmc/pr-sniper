//! Contract: a provider's thread and comment interfaces read and write the
//! neutral thread, comment and top-level comment shapes, and a guarded session
//! checks its guard before every read. The GitHub implementation is driven here
//! over a fake wire.

use pr_sniper_lib::github::{
    provider::{GithubClient, Response, Transport},
    publication::{MutationTransport, Request},
    threads::QueryTransport,
};
use pr_sniper_lib::provider::{ConnectionError, ProviderSession, RemoteRepository};
use pr_sniper_lib::publication::WriteFailure;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct Server {
    resolved: bool,
    replies: Vec<String>,
}

#[derive(Clone, Default)]
struct Wire(Arc<Mutex<Server>>);

fn response(status: u16, value: Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}

fn comment(id: u64, author: u64, body: &str, reply_to: Option<u64>) -> Value {
    json!({
        "fullDatabaseId": id.to_string(), "body": body,
        "createdAt": "2026-10-26T00:00:00Z", "publishedAt": "2026-10-26T00:00:00Z",
        "state": "SUBMITTED", "author": {"databaseId": author, "login": "person"},
        "replyTo": reply_to.map(|id| json!({"fullDatabaseId": id.to_string()})),
        "pullRequestReview": {"fullDatabaseId": "42"}, "originalCommit": {"oid": HEAD}
    })
}

impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        assert_eq!(
            path,
            "/repos/example/repo/issues/7/comments?per_page=100&page=1"
        );
        Ok(response(
            200,
            json!([{
                "id": 300, "body": "@acting please look", "user": {"id": 11, "login": "author"},
                "created_at": "2026-10-26T00:00:00Z", "updated_at": "2026-10-26T00:00:00Z",
                "issue_url": "https://api.github.com/repos/example/repo/issues/7"
            }]),
        ))
    }
}

impl QueryTransport for Wire {
    fn query(&self, _: &str, variables: Value) -> Result<Response, ConnectionError> {
        assert_eq!(variables["id"], "thread-node");
        let server = self.0.lock().unwrap();
        let mut comments = vec![comment(100, 22, "Root finding", None)];
        comments.extend(
            server
                .replies
                .iter()
                .enumerate()
                .map(|(n, body)| comment(200 + n as u64, 22, body, Some(100))),
        );
        let total = comments.len();
        Ok(response(
            200,
            json!({"data": {"node": {
                "id": "thread-node", "isResolved": server.resolved, "viewerCanReply": true,
                "repository": {"databaseId": 100}, "pullRequest": {"fullDatabaseId": "9"},
                "comments": {"totalCount": total, "nodes": comments,
                    "pageInfo": {"hasNextPage": false, "endCursor": null}}
            }}}),
        ))
    }
}

impl MutationTransport for Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        let Request::Post(body) = request else {
            panic!("no deletion expected");
        };
        let mut server = self.0.lock().unwrap();
        Ok(match path {
            "/repos/example/repo/pulls/7/comments/100/replies" => {
                server.replies.push(body["body"].as_str().unwrap().into());
                response(
                    201,
                    json!({"id": 199 + server.replies.len(), "body": body["body"],
                        "user": {"id": 22}, "in_reply_to_id": 100,
                        "pull_request_url": "https://api.github.com/repos/example/repo/pulls/7"}),
                )
            }
            "/repos/example/repo/issues/7/comments" => response(
                201,
                json!({"id": 301, "body": body["body"], "user": {"id": 22, "login": "acting"},
                    "created_at": "2026-10-26T00:01:00Z", "updated_at": "2026-10-26T00:01:00Z",
                    "issue_url": "https://api.github.com/repos/example/repo/issues/7"}),
            ),
            "/graphql" => {
                let input = &body["variables"]["input"];
                let resolve = body["query"]
                    .as_str()
                    .unwrap()
                    .contains("mutation($input:ResolveReviewThreadInput!)");
                server.resolved = resolve;
                let field = if resolve {
                    "resolveReviewThread"
                } else {
                    "unresolveReviewThread"
                };
                response(
                    200,
                    json!({"data": {field: {"clientMutationId": input["clientMutationId"],
                        "thread": {"id": input["threadId"], "isResolved": resolve}}}}),
                )
            }
            _ => panic!("unexpected write: {path}"),
        })
    }
}

fn written<T>(result: Result<T, WriteFailure>) -> T {
    result.unwrap_or_else(|_| panic!("write failed"))
}

fn thread_contract(session: &dyn ProviderSession) {
    let repository = RemoteRepository {
        id: "100".into(),
        name: "example/repo".into(),
    };

    let thread = session.review_thread("100", "9", "thread-node").unwrap();
    let thread = thread.expect("thread is present");
    assert!(!thread.resolved && thread.can_reply);
    let root = thread.root().unwrap();
    assert_eq!(
        (root.id.as_str(), root.author_id.as_deref()),
        ("100", Some("22"))
    );

    let reply = written(session.reply_to_review_thread(
        "example/repo",
        7,
        "22",
        "100",
        "Fixed in the next push.",
    ));
    assert_eq!(reply, "200");
    written(session.resolve_review_thread("thread-node", "request-1"));
    let thread = session
        .review_thread("100", "9", "thread-node")
        .unwrap()
        .unwrap();
    assert!(thread.resolved);
    assert_eq!(thread.comments[1].reply_to.as_deref(), Some("100"));
    assert_eq!(thread.comments[1].body, "Fixed in the next push.");
    written(session.unresolve_review_thread("thread-node", "request-2"));
    assert!(
        !session
            .review_thread("100", "9", "thread-node")
            .unwrap()
            .unwrap()
            .resolved
    );

    let comments = session.top_comments(&repository, 7).unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].eligible_other_user("22"));
    assert!(comments[0].mentions("acting", "22"));
    assert_eq!(
        written(session.reply_to_mention(&repository, 7, "22", "Looking now.")),
        "301"
    );
}

#[test]
fn github_satisfies_the_thread_and_comment_interfaces() {
    let session: Box<dyn ProviderSession> = Box::new(GithubClient::new(Wire::default()));
    thread_contract(&*session);
}

#[test]
fn a_guarded_session_checks_its_guard_before_every_read() {
    let open = Arc::new(AtomicBool::new(true));
    let gate = open.clone();
    let session: Box<dyn ProviderSession> = Box::new(GithubClient::new(Wire::default()));
    let reads = session.guarded(Arc::new(move || {
        if gate.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(ConnectionError::Configuration)
        }
    }));
    assert!(reads
        .review_thread("100", "9", "thread-node")
        .unwrap()
        .is_some());
    open.store(false, Ordering::SeqCst);
    assert_eq!(
        reads.review_thread("100", "9", "thread-node").unwrap_err(),
        ConnectionError::Configuration
    );
}
