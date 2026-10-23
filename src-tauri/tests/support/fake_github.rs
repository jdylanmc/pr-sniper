//! Fake GitHub for host-write tests: review-thread reads, thread replies and
//! `resolveReviewThread`, with one-shot fault injection per write kind.
#![allow(dead_code)]

use pr_sniper_lib::{
    github::{
        provider::{Response, Transport},
        publication::{MutationTransport, Request},
        threads::{Comment, QueryTransport, Thread},
        ConnectionError,
    },
    monitoring::{JobOperation, OperationState, QueueJob},
    publication::{Batch, InlineComment, Publication, Receipt, RemoteState},
    review::{ReviewRun, Selection},
    storage::Settings,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub const REPOSITORY: &str = "00000000-0000-4000-8000-000000000001";
pub const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
pub const AGENT: &str = "00000000-0000-4000-8000-000000000003";
pub const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// Transport error; nothing landed.
    Before,
    /// The write landed, then the transport failed.
    After,
    /// Definite 4xx rejection.
    Reject,
    /// Server error; nothing landed.
    ServerError,
}

pub struct Server {
    pub threads: Vec<Thread>,
    pub head: String,
    /// Ordered log of write attempts that reached the server: `reply:<root>` / `resolve:<thread>`.
    pub log: Vec<String>,
    pub fault: Option<(&'static str, Fault)>,
    pub resolve_override: Option<Value>,
    next_id: u64,
}

#[derive(Clone)]
pub struct Wire(pub Arc<Mutex<Server>>);

impl Wire {
    pub fn new(threads: Vec<Thread>) -> Self {
        Self(Arc::new(Mutex::new(Server {
            threads,
            head: HEAD.into(),
            log: vec![],
            fault: None,
            resolve_override: None,
            next_id: 500,
        })))
    }
    pub fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().log.clone()
    }
    pub fn fault(&self, kind: &'static str, fault: Fault) {
        self.0.lock().unwrap().fault = Some((kind, fault));
    }
    pub fn thread(&self, id: &str) -> Thread {
        self.0
            .lock()
            .unwrap()
            .threads
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .clone()
    }
}

pub fn comment(
    id: &str,
    body: &str,
    author: &str,
    reply_to: Option<&str>,
    review: Option<&str>,
) -> Comment {
    Comment {
        id: id.into(),
        body: body.into(),
        author_id: Some(author.into()),
        author_login: Some("person".into()),
        reply_to: reply_to.map(String::from),
        review_id: review.map(String::from),
        original_commit: Some(HEAD.into()),
        created_at: "2026-09-27T00:00:00Z".into(),
        published_at: "2026-09-27T00:00:00Z".into(),
    }
}

/// A thread rooted in the agent's published comment (account 22, review 42).
pub fn owned_thread(id: &str, root: &str) -> Thread {
    Thread {
        id: id.into(),
        resolved: false,
        can_reply: true,
        comments: vec![
            comment(root, "Root finding", "22", None, Some("42")),
            comment(
                &(root.parse::<u64>().unwrap() + 1).to_string(),
                "Why?",
                "11",
                Some(root),
                Some("43"),
            ),
        ],
    }
}

/// A thread rooted by someone else.
pub fn foreign_thread(id: &str, root: &str) -> Thread {
    Thread {
        id: id.into(),
        resolved: false,
        can_reply: true,
        comments: vec![comment(root, "Please rename", "11", None, Some("77"))],
    }
}

pub fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,"repositories":[{"id":REPOSITORY,"provider":"github","name":"example/repo","enabled":true,
        "provider_account_id":"22","provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}],
        "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true,
            "tools":["get_thread"],"actions":{"reply":true,"approve":false,"merge":false}}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
        "prompt":"Review correctness.","signature":"stored"}]
    }))
    .unwrap()
}

pub fn job() -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":REPOSITORY,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":HEAD,
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author","watched_author":true,
        "requested_reviewer":false,"waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

/// A completed Packet-mode run whose exit is `leave_comments` with the given payload.
pub fn review(
    new_comments: Value,
    replies: Value,
    resolutions: Value,
    selection: &Settings,
) -> ReviewRun {
    let job = job();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    ReviewRun {
        failed_duck: None,
        attempt_ducks: Vec::new(),
        mode: pr_sniper_lib::review::RunMode::Packet,
        packet: None,
        feedback_context: None,
        key: pr_sniper_lib::review::key(&job, ASSIGNMENT),
        selection: Selection::resolve(selection, &job, ASSIGNMENT)
            .unwrap()
            .into(),
        job,
        assignment_id: ASSIGNMENT.into(),
        operation,
        manual_start: false,
        trust_confirmed: true,
        phase: "Complete".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({"reviewed_base_sha":"b".repeat(40),
                "output":{"exit":{"exit":"comments","new_comments":new_comments,
                    "reply_comments":replies,"resolve_own_threads":resolutions}},
                "session_id":"session","model":"model","runtime_version":"fixture",
                "input_tokens":1,"output_tokens":1,"tool_calls":1}))
            .unwrap(),
        ),
    }
}

/// The publication that rooted the owned threads (root 100, review 42).
pub fn origin() -> Publication {
    let mut origin = Publication::new(
        review(json!([]), json!([]), json!([]), &settings()),
        true,
        true,
        100,
    )
    .unwrap();
    origin.batch = Some(Batch {
        commit_id: HEAD.into(),
        body: "Summary".into(),
        comments: vec![InlineComment {
            diff_position: None,
            path: "source.rs".into(),
            line: 1,
            side: "RIGHT".into(),
            body: "Root finding".into(),
        }],
        unmappable: vec![],
    });
    origin.receipts = vec![Receipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["100".into(), "200".into()],
    }];
    origin
}

fn response(status: u16, value: Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}

fn node(thread: &Thread) -> Value {
    let comments: Vec<_> = thread
        .comments
        .iter()
        .map(|c| {
            json!({"fullDatabaseId":c.id,"body":c.body,"createdAt":c.created_at,
            "publishedAt":c.published_at,"state":"SUBMITTED",
            "author":{"databaseId":c.author_id.as_ref().map(|id| id.parse::<u64>().unwrap()),"login":c.author_login},
            "replyTo":c.reply_to.as_ref().map(|id| json!({"fullDatabaseId":id})),
            "pullRequestReview":c.review_id.as_ref().map(|id| json!({"fullDatabaseId":id})),
            "originalCommit":{"oid":c.original_commit}})
        })
        .collect();
    json!({"id":thread.id,"isResolved":thread.resolved,"viewerCanReply":thread.can_reply,
        "repository":{"databaseId":100},"pullRequest":{"fullDatabaseId":"9"},
        "comments":{"totalCount":comments.len(),"nodes":comments,
            "pageInfo":{"hasNextPage":false,"endCursor":null}}})
}

impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        panic!("Unexpected GET {path}")
    }
}

impl QueryTransport for Wire {
    fn query(&self, query: &str, variables: Value) -> Result<Response, ConnectionError> {
        assert!(!query.contains("mutation"));
        let server = self.0.lock().unwrap();
        let found = server
            .threads
            .iter()
            .find(|t| Some(t.id.as_str()) == variables["id"].as_str());
        Ok(response(200, json!({"data":{"node":found.map(node)}})))
    }
}

impl MutationTransport for Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        let Request::Post(body) = request else {
            panic!("No deletion permitted");
        };
        let mut server = self.0.lock().unwrap();
        if path == "/graphql" {
            let input = &body["variables"]["input"];
            let id = input["threadId"].as_str().unwrap().to_string();
            if body["query"]
                .as_str()
                .is_some_and(|q| q.contains("unresolveReviewThread"))
            {
                server.log.push(format!("unresolve:{id}"));
                if let Some(thread) = server.threads.iter_mut().find(|t| t.id == id) {
                    thread.resolved = false;
                }
                return Ok(response(
                    200,
                    json!({"data":{"unresolveReviewThread":{"clientMutationId":input["clientMutationId"],
                    "thread":{"id":id,"isResolved":false}}}}),
                ));
            }
            server.log.push(format!("resolve:{id}"));
            let fault = server.fault.take_if(|(kind, _)| *kind == "resolve");
            match fault.map(|(_, f)| f) {
                Some(Fault::Before) => return Err(ConnectionError::Timeout),
                Some(Fault::ServerError) => return Ok(response(502, json!({}))),
                Some(Fault::Reject) => {
                    return Ok(response(
                        200,
                        json!({"data":null,"errors":[{"type":"FORBIDDEN","message":"private"}]}),
                    ))
                }
                _ => (),
            }
            if let Some(value) = server.resolve_override.take() {
                return Ok(response(200, value));
            }
            if let Some(thread) = server.threads.iter_mut().find(|t| t.id == id) {
                thread.resolved = true;
            }
            if fault.is_some() {
                return Err(ConnectionError::Timeout);
            }
            return Ok(response(
                200,
                json!({"data":{"resolveReviewThread":{"clientMutationId":input["clientMutationId"],
                "thread":{"id":id,"isResolved":true}}}}),
            ));
        }
        let root = path
            .strip_prefix("/repos/example/repo/pulls/1/comments/")
            .and_then(|rest| rest.strip_suffix("/replies"))
            .unwrap_or_else(|| panic!("Unexpected mutation path {path}"))
            .to_string();
        server.log.push(format!("reply:{root}"));
        let fault = server.fault.take_if(|(kind, _)| *kind == "reply");
        match fault.map(|(_, f)| f) {
            Some(Fault::Before) => return Err(ConnectionError::Timeout),
            Some(Fault::ServerError) => return Ok(response(502, json!({}))),
            Some(Fault::Reject) => return Ok(response(422, json!({"message":"private"}))),
            _ => (),
        }
        server.next_id += 1;
        let id = server.next_id.to_string();
        let text = body["body"].as_str().unwrap().to_string();
        let thread = server
            .threads
            .iter_mut()
            .find(|t| t.comments.first().is_some_and(|c| c.id == root))
            .expect("reply root");
        thread
            .comments
            .push(comment(&id, &text, "22", Some(&root), Some("90")));
        if fault.is_some() {
            return Err(ConnectionError::Timeout);
        }
        Ok(response(
            201,
            json!({"id":id.parse::<u64>().unwrap(),"body":text,"user":{"id":22},
                "in_reply_to_id":root.parse::<u64>().unwrap(),
                "pull_request_url":"https://api.github.com/repos/example/repo/pulls/1"}),
        ))
    }
}
