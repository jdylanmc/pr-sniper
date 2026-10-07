use pr_sniper_lib::{
    follow_up::{
        self, Environment, Evidence, FollowUp, Observation, Phase, ReplyDecision, ReplyOutput,
    },
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{GithubClient, Response, Transport},
        publication::{MutationTransport, Request},
        threads::{Comment, QueryTransport, Thread},
        ConnectionError, Identity,
    },
    monitoring::{JobOperation, OperationState, QueueJob},
    publication::{Batch, InlineComment, Publication, Receipt, RemoteState, WriteFailure},
    review::{Failure, ReviewResult, ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const REPOSITORY: &str = "00000000-0000-4000-8000-000000000001";
const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "00000000-0000-4000-8000-000000000003";

fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,"repositories":[{"id":REPOSITORY,"provider":"github","name":"example/repo","enabled":true,
        "provider_account_id":"22","provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}],
        "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true,
            "actions":{"reply":true,"approve":false,"merge":false}}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model","ai_account":{"provider":"copilot","account_id":"33"},
        "prompt":"Review correctness.","signature":"stored"}]
    })).unwrap()
}
fn origin() -> Publication {
    let job:QueueJob = serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":REPOSITORY,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author","watched_author":true,
        "requested_reviewer":false,"waiting":"human_start","detected_at":100
    })).unwrap();
    let mut op = JobOperation::review(&job, 100);
    op.state = OperationState::Completed;
    let review=ReviewRun {
        feedback_context: None,
        key:pr_sniper_lib::review::key(&job,ASSIGNMENT),selection:Selection::resolve(&settings(),&job,ASSIGNMENT).unwrap(),
        job,assignment_id:ASSIGNMENT.into(),operation:op,manual_start:true,trust_confirmed:true,phase:"Complete".into(),error:None,
        result:Some(serde_json::from_value(json!({"reviewed_base_sha":"b".repeat(40),
            "output":{"synopsis":"Review completed.","files":[],"findings":[],"decision":"machine_sign_off"},
            "session_id":"session","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":1})).unwrap()),
    };
    let mut origin = Publication::new(review, false, true, 100).unwrap();
    origin.batch = Some(Batch {
        commit_id: "a".repeat(40),
        body: "Summary".into(),
        comments: vec![InlineComment {
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
        comment_ids: vec!["100".into()],
    }];
    origin
}
fn comment(id: &str, body: &str, reply_to: Option<&str>) -> Comment {
    Comment {
        id: id.into(),
        body: body.into(),
        author_id: Some(if reply_to.is_none() { "22" } else { "11" }.into()),
        author_login: Some("person".into()),
        reply_to: reply_to.map(String::from),
        review_id: Some("42".into()),
        original_commit: Some("a".repeat(40)),
        created_at: "2026-09-27T00:00:00Z".into(),
        published_at: "2026-09-27T00:00:00Z".into(),
    }
}
fn thread() -> Thread {
    Thread {
        id: "thread-node".into(),
        resolved: false,
        can_reply: true,
        comments: vec![
            comment("100", "Root finding", None),
            comment("101", "Why is this required?", Some("100")),
        ],
    }
}
fn pull() -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 1,
        title: "Review".into(),
        author: Some(Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: "a".repeat(40),
        base_sha: "b".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "time".into(),
        files: vec![],
    }
}
fn prepared(origin: &Publication) -> FollowUp {
    let mut run = FollowUp::new(origin, thread()).unwrap();
    run.result = Some(ReviewResult {
        reviewed_base_sha: Some("b".repeat(40)),
        output: ReplyOutput {
            feedback_assessments: Vec::new(),
            decision: ReplyDecision::Reply,
            body: "The function returns 42.".into(),
            new_information: "The function returns 42.".into(),
            reason: "Source evidence.".into(),
            evidence: vec![Evidence {
                path: "source.rs".into(),
                side: "head".into(),
                line: 1,
                quote: "return 42;".into(),
            }],
        },
        session_id: "session".into(),
        model: "model".into(),
        runtime_version: "fixture".into(),
        intelligence: None,
        input_tokens: 1,
        output_tokens: 1,
        tool_calls: 1,
    });
    run.analysis = Some(run.operation("thread_analysis", 100));
    run.analysis.as_mut().unwrap().state = OperationState::Completed;
    run.publication = Some(run.operation("thread_reply", 100));
    run.confirmed = true;
    run.body = Some(run.reply_body().unwrap());
    run.phase = Phase::WaitingPublication;
    run
}

#[test]
fn reply_work_ordinals_and_fifo_metadata_are_independent_of_retry_attempts() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let origin = origin();
    let mut runs = Vec::new();
    assert!(follow_up::admit(&mut runs, &origin, thread()).unwrap());
    runs[0].enqueue_order = Some(store.allocate_enqueue_order().unwrap());
    runs[0].enqueued_at = Some(100);
    let mut operation = runs[0].operation("thread_analysis", 100);
    operation.begin_attempt(100).unwrap();
    operation.fail(&Failure::timeout().monitoring(), 101);
    operation
        .begin_attempt(operation.next_attempt_at.unwrap())
        .unwrap();
    runs[0].analysis = Some(operation);
    store.save_follow_ups(&runs).unwrap();
    let mut runs = store.load_follow_ups().unwrap();
    assert_eq!(runs[0].reply_ordinal, Some(1));
    assert_eq!(runs[0].enqueue_order, Some(1));
    assert_eq!(runs[0].analysis.as_ref().unwrap().attempt_count, 2);
    assert!(!follow_up::admit(&mut runs, &origin, thread()).unwrap());
    let mut next = thread();
    let mut reply = comment("102", "Here is new context.", Some("100"));
    reply.published_at = "2026-09-27T00:01:00Z".into();
    next.comments.push(reply);
    assert!(follow_up::admit(&mut runs, &origin, next).unwrap());
    runs[1].enqueue_order = Some(store.allocate_enqueue_order().unwrap());
    assert_eq!(runs[1].reply_ordinal, Some(2));
    assert_eq!(runs[1].enqueue_order, Some(2));
    assert_eq!(runs[1].analysis, None);
    store.save_follow_ups(&runs).unwrap();
    std::fs::remove_file(root.path().join("state/queue.json")).unwrap();
    assert_eq!(store.allocate_enqueue_order().unwrap(), 3);
}

#[derive(Clone, Copy)]
enum Fault {
    Before,
    After,
    Reject,
    RateLimit,
}
struct Server {
    threads: Vec<Thread>,
    writes: usize,
    queries: usize,
    fault: Option<Fault>,
    resolved_after: bool,
    changed_head: bool,
    external_after: bool,
    graphql_error: bool,
    wrong_count: bool,
    draft_replies: bool,
}
#[derive(Clone)]
struct Wire(Arc<Mutex<Server>>);
fn response(value: Value) -> Response {
    Response {
        status: 200,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}
fn connection(nodes: Vec<Value>, cursor: usize, wrong: bool) -> Value {
    json!({"totalCount":nodes.len()+usize::from(wrong),"nodes":nodes.iter().skip(cursor).take(100).collect::<Vec<_>>(),
        "pageInfo":{"hasNextPage":cursor+100<nodes.len(),"endCursor":(cursor+100).to_string()}})
}
fn node(thread: &Thread, cursor: usize, wrong: bool, drafts: bool) -> Value {
    let comments=thread.comments.iter().map(|c| json!({
        "fullDatabaseId":c.id,"body":c.body,"createdAt":c.created_at,
        "publishedAt":if drafts && c.reply_to.is_some() {Value::Null} else {json!(c.published_at)},
        "state":if drafts && c.reply_to.is_some() {"PENDING"} else {"SUBMITTED"},
        "author":{"databaseId":c.author_id.as_ref().map(|id| id.parse::<u64>().unwrap()),"login":c.author_login},
        "replyTo":c.reply_to.as_ref().map(|id| json!({"fullDatabaseId":id})),
        "pullRequestReview":c.review_id.as_ref().map(|id| json!({"fullDatabaseId":id})),
        "originalCommit":{"oid":c.original_commit}
    })).collect();
    json!({"id":thread.id,"isResolved":thread.resolved,"viewerCanReply":thread.can_reply,
        "repository":{"databaseId":100},"pullRequest":{"fullDatabaseId":"9"},"comments":connection(comments,cursor,wrong)})
}
impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        panic!("Unexpected GET {path}")
    }
}
impl QueryTransport for Wire {
    fn query(&self, query: &str, variables: Value) -> Result<Response, ConnectionError> {
        assert!(!query.contains("mutation"));
        let mut server = self.0.lock().unwrap();
        server.queries += 1;
        if server.graphql_error {
            return Ok(response(
                json!({"data":{},"errors":[{"message":"private provider detail"}]}),
            ));
        }
        let cursor = variables["commentCursor"]
            .as_str()
            .unwrap_or("0")
            .parse::<usize>()
            .unwrap();
        let value = if query.contains("reviewThreads") {
            let page = variables["cursor"]
                .as_str()
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap();
            let nodes = server
                .threads
                .iter()
                .map(|t| node(t, 0, server.wrong_count, server.draft_replies))
                .collect();
            json!({"repository":{"databaseId":100,"pullRequest":{"fullDatabaseId":"9","headRefOid":if server.changed_head {"c".repeat(40)} else {"a".repeat(40)},
                "reviewThreads":connection(nodes,page,server.wrong_count)}}})
        } else {
            json!({"node":server.threads.iter().find(|t| Some(t.id.as_str())==variables["id"].as_str())            .map(|t|node(t,cursor,server.wrong_count,server.draft_replies))})
        };
        Ok(response(json!({"data":value})))
    }
}
impl MutationTransport for Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        assert_eq!(path, "/repos/example/repo/pulls/1/comments/100/replies");
        let Request::Post(body) = request else {
            panic!("No thread deletion permitted");
        };
        assert_eq!(body.as_object().unwrap().len(), 1);
        let mut server = self.0.lock().unwrap();
        server.writes += 1;
        let fault = server.fault.take();
        if matches!(fault, Some(Fault::Before)) {
            return Err(ConnectionError::Timeout);
        }

        if matches!(fault, Some(Fault::Reject)) {
            return Ok(Response {
                status: 422,
                ..response(json!({"message":"private detail"}))
            });
        }
        if matches!(fault, Some(Fault::RateLimit)) {
            return Ok(Response {
                status: 429,
                headers: BTreeMap::from([("retry-after".into(), "17".into())]),
                ..response(json!({}))
            });
        }
        let mut reply = comment("200", body["body"].as_str().unwrap(), Some("100"));
        reply.author_id = Some("22".into());
        server.threads[0].comments.push(reply);
        if server.resolved_after {
            server.threads[0].resolved = true;
        }
        if server.external_after {
            server.threads[0]
                .comments
                .push(comment("201", "Another question", Some("100")));
        }
        if matches!(fault, Some(Fault::After)) {
            return Err(ConnectionError::Timeout);
        }
        Ok(Response {
            status: 201,
            ..response(
                json!({"id":200,"body":body["body"],"user":{"id":22},"in_reply_to_id":100,
            "pull_request_url":"https://api.github.com/repos/example/repo/pulls/1"}),
            )
        })
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    store: Store,
    origin: Publication,
    wire: Wire,
    now: i64,
    fail_save: bool,
    withdrawn: bool,
    pause_after_reply: bool,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self {
            store: Store::new(root.path().into()),
            _root: root,
            origin: origin(),
            now: 100,
            fail_save: false,
            withdrawn: false,
            pause_after_reply: false,
            wire: Wire(Arc::new(Mutex::new(Server {
                threads: vec![thread()],
                writes: 0,
                queries: 0,
                fault: None,
                resolved_after: false,
                changed_head: false,
                external_after: false,
                graphql_error: false,
                wrong_count: false,
                draft_replies: false,
            }))),
        }
    }
    fn resume(&mut self) -> FollowUp {
        follow_up::restore(&self.store).unwrap();
        let run = self.store.load_follow_ups().unwrap().remove(0);
        self.now = run
            .publication
            .as_ref()
            .unwrap()
            .next_attempt_at
            .unwrap_or(self.now)
            .max(self.now);
        run
    }
    fn writes(&self) -> usize {
        self.wire.0.lock().unwrap().writes
    }
}
impl Environment for Fixture {
    fn now(&self) -> Result<i64, Failure> {
        Ok(self.now)
    }
    fn save(&mut self, run: &mut FollowUp) -> Result<(), Failure> {
        if self.fail_save && self.writes() > 0 {
            return Err(Failure::permanent("Injected storage failure."));
        }
        self.store
            .save_follow_ups(std::slice::from_ref(run))
            .map_err(Failure::permanent)
    }
    fn observe(&mut self, run: &FollowUp) -> Result<Observation, Failure> {
        Ok(Observation::Owned(
            GithubClient::new(self.wire.clone())
                .owned_thread(&self.origin, &run.owned().unwrap().thread.id)?
                .ok_or_else(|| Failure::permanent("Thread missing."))?,
        ))
    }
    fn gate(&mut self, run: &FollowUp, observed: &Observation) -> Result<Option<String>, Failure> {
        let Observation::Owned(current) = observed else {
            panic!("Owned fixture expected")
        };
        if let Err(error) = pr_sniper_lib::capacity::publication_gate(&self.store) {
            return Ok(Some(error));
        }
        if !run.fresh_thread(current) {
            return Ok(Some("Thread changed or resolved.".into()));
        }
        let mut pull = pull();
        if self.wire.0.lock().unwrap().changed_head {
            pull.head_sha = "c".repeat(40);
        }
        if self.withdrawn {
            return Ok(Some("Publication cancelled.".into()));
        }
        Ok(run
            .validate_current(&settings(), &run.context.job, &pull, true, true)
            .err()
            .map(|error| error.message))
    }
    fn reply(&mut self, run: &FollowUp) -> Result<String, WriteFailure> {
        let thread = self.observe(run).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        if let Some(reason) = self.gate(run, &thread).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })? {
            return Err(WriteFailure {
                failure: Failure::permanent(reason),
                uncertain: false,
            });
        }
        let result = GithubClient::new(self.wire.clone()).reply_to_thread(
            &self.origin,
            "100",
            run.body.as_deref().unwrap(),
        );
        if self.pause_after_reply {
            self.store
                .save_automation(&pr_sniper_lib::capacity::Automation { paused: true })
                .unwrap();
        }
        result
    }
}

#[test]
fn pause_during_lost_reply_response_retains_the_original_intent_and_reconciles_once() {
    let mut fixture = Fixture::new();
    let mut run = prepared(&fixture.origin);
    fixture.pause_after_reply = true;
    fixture.wire.0.lock().unwrap().fault = Some(Fault::After);
    assert!(follow_up::publish(&mut fixture, &mut run).is_err());
    assert!(run.uncertain);
    assert_eq!(fixture.writes(), 1);
    assert!(fixture.store.load_automation().unwrap().paused);
    let id = run.id.clone();
    fixture
        .store
        .save_automation(&pr_sniper_lib::capacity::Automation { paused: false })
        .unwrap();
    let mut restored = fixture.resume();
    follow_up::publish(&mut fixture, &mut restored).unwrap();
    assert_eq!(restored.id, id);
    assert_eq!(restored.receipt.as_deref(), Some("200"));
    assert_eq!(fixture.writes(), 1);
}

#[test]
fn current_head_thread_observation_keeps_original_commit_provenance_and_rejects_tampered_roots() {
    let fixture = Fixture::new();
    fixture.wire.0.lock().unwrap().changed_head = true;
    let client = GithubClient::new(fixture.wire.clone());
    assert!(client.owned_threads(&fixture.origin).is_err());
    let threads = client
        .owned_threads_at(&fixture.origin, &"c".repeat(40))
        .unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(
        threads[0].root().unwrap().original_commit.as_deref(),
        Some("a".repeat(40).as_str())
    );
    fixture.wire.0.lock().unwrap().threads[0].comments[0].body = "Tampered root".into();
    assert!(client
        .owned_threads_at(&fixture.origin, &"c".repeat(40))
        .is_err());
}

#[test]
fn retention_compact_provenance_preserves_full_thread_and_comment_pagination() {
    use pr_sniper_lib::github::threads::Provenance;
    let mut fixture = Fixture::new();
    let threads: Vec<_> = (0..205)
        .map(|index| {
            let mut value = thread();
            value.id = format!("thread-{index}");
            let root = (100 + index * 1000).to_string();
            value.comments[0].id = root.clone();
            value.comments[1].reply_to = Some(root.clone());
            value.comments[1].id = (101 + index * 1000).to_string();
            if index == 0 {
                for next in 2..207 {
                    value.comments.push(comment(
                        &(100 + next).to_string(),
                        "External evidence",
                        Some(&root),
                    ));
                }
            }
            value
        })
        .collect();
    fixture.origin.receipts[0].comment_ids =
        threads.iter().map(|t| t.comments[0].id.clone()).collect();
    fixture.wire.0.lock().unwrap().threads = threads.clone();
    let proof = fixture.origin.ownership().unwrap();
    let client = GithubClient::new(fixture.wire.clone());
    assert_eq!(client.owned_threads(&fixture.origin).unwrap(), threads);
    assert_eq!(client.owned_threads(&proof).unwrap(), threads);
    assert_eq!(
        client
            .owned_thread(&proof, "thread-0")
            .unwrap()
            .unwrap()
            .comments
            .len(),
        207
    );
    assert!(fixture.wire.0.lock().unwrap().queries >= 10);
    fixture.wire.0.lock().unwrap().threads[204].comments[0].body = "Changed root".into();
    assert!(client.owned_threads(&proof).is_err());
}

#[test]
fn retention_compact_provenance_never_weakens_identity_or_body_checks() {
    use pr_sniper_lib::github::threads::Provenance;
    let fixture = Fixture::new();
    let client = GithubClient::new(fixture.wire.clone());
    let proof = fixture.origin.ownership().unwrap();
    assert_eq!(
        client.owned_thread(&proof, "thread-node").unwrap(),
        Some(thread())
    );
    for field in ["account", "repository", "pull", "review", "commit", "body"] {
        let mut changed = proof.clone();
        match field {
            "account" => changed.account_id = "44".into(),
            "repository" => changed.repository_id = "200".into(),
            "pull" => changed.pull_request_id = "10".into(),
            "review" => changed.review_id = "43".into(),
            "commit" => changed.head_sha = "c".repeat(40),
            "body" => changed.body_hashes.clear(),
            _ => unreachable!(),
        }
        assert!(
            client.owned_thread(&changed, "thread-node").is_err(),
            "{field}"
        );
    }
    fixture.wire.0.lock().unwrap().wrong_count = true;
    assert!(client.owned_threads(&proof).is_err());
}

#[test]
fn retention_compact_origin_reconciles_a_lost_reply_without_reposting() {
    use pr_sniper_lib::github::threads::Provenance;
    let fixture = Fixture::new();
    let proof = fixture.origin.ownership().unwrap();
    fixture.wire.0.lock().unwrap().fault = Some(Fault::After);
    let client = GithubClient::new(fixture.wire.clone());
    let body = "Evidence-backed reply <!-- pr-sniper:reply:retention-fixture -->";
    assert!(client.reply_to_thread(&proof, "100", body).is_err());
    let current = client.owned_thread(&proof, "thread-node").unwrap().unwrap();
    assert_eq!(
        current
            .comments
            .iter()
            .filter(|c| c.body == body && c.author_id.as_deref() == Some("22"))
            .count(),
        1
    );
    assert_eq!(fixture.writes(), 1);
}

#[test]
fn last_local_check_rejects_withdrawn_or_changed_publication_grants() {
    let mut run = prepared(&origin());
    let mut current = run.clone();
    assert!(run.check_publication_grant(&current, false).is_err());
    current.confirmed = false;
    assert!(run.check_publication_grant(&current, false).is_err());
    current.confirmed = true;
    assert!(run.check_publication_grant(&current, true).is_ok());
    run.automatic_publication = true;
    current.confirmed = false;
    assert!(run.check_publication_grant(&current, true).is_ok());
    current.cancelled = true;
    assert!(run.check_publication_grant(&current, true).is_err());
}

#[test]
fn general_provider_threads_keep_original_authorship_and_reply_as_the_acting_identity() {
    let fixture = Fixture::new();
    {
        let mut server = fixture.wire.0.lock().unwrap();
        server.threads[0].comments[0].author_id = Some("44".into());
        server.threads[0].comments[0].body = "An unowned review question".into();
    }
    let client = GithubClient::new(fixture.wire.clone());
    let threads = client
        .review_threads_at("example/repo", "100", "9", 1, &"a".repeat(40))
        .unwrap();
    assert_eq!(threads[0].comments[0].author_id.as_deref(), Some("44"));
    assert!(
        client.owned_threads(&fixture.origin).is_err(),
        "General routing does not forge owned-root provenance."
    );
    assert!(client.review_thread("101", "9", "thread-node").is_err());
    assert!(client.review_thread("100", "10", "thread-node").is_err());
    let body = "Automated follow-up by PR Sniper / Agent Primary / model model.\n\nNew evidence.\n<!-- pr-sniper:reply:general -->\n\nPR Sniper";
    let id = client
        .reply_to_review_thread("example/repo", 1, "22", "100", body)
        .map_err(|error| error.failure.message)
        .unwrap();
    assert_eq!(id, "200");
    let current = client
        .review_thread("100", "9", "thread-node")
        .unwrap()
        .unwrap();
    assert_eq!(current.comments[0].author_id.as_deref(), Some("44"));
    assert_eq!(
        current.comments.last().unwrap().author_id.as_deref(),
        Some("22")
    );
    assert_eq!(
        current.comments.last().unwrap().reply_to.as_deref(),
        Some("100")
    );
    assert_eq!(fixture.writes(), 1);
}

#[test]
fn analysis_result_commit_fences_account_cancellation_and_late_settings_changes() {
    let fixture = Fixture::new();
    let mut run = prepared(&fixture.origin);
    run.publication = None;
    run.manual_start = true;
    fixture.store.save_settings(&settings()).unwrap();
    fixture
        .store
        .save_queue(std::slice::from_ref(&run.context.job))
        .unwrap();
    fixture
        .store
        .save_follow_ups(std::slice::from_ref(&run))
        .unwrap();
    assert!(follow_up::validate_analysis_commit(&fixture.store, &run, true).is_err());
    run.body = None;
    fixture
        .store
        .save_follow_ups(std::slice::from_ref(&run))
        .unwrap();
    assert!(follow_up::validate_analysis_commit(&fixture.store, &run, true).is_ok());
    assert!(follow_up::validate_analysis_commit(&fixture.store, &run, false).is_err());
    let mut cancelled = run.clone();
    cancelled.cancelled = true;
    fixture.store.save_follow_ups(&[cancelled]).unwrap();
    assert!(follow_up::validate_analysis_commit(&fixture.store, &run, true).is_err());
    fixture
        .store
        .save_follow_ups(std::slice::from_ref(&run))
        .unwrap();
    let mut changed = settings();
    changed.agents[0].prompt = "Different review lens.".into();
    fixture.store.save_settings(&changed).unwrap();
    assert!(follow_up::validate_analysis_commit(&fixture.store, &run, true).is_err());
}

#[test]
fn owned_external_comment_is_deduplicated_across_polling_and_restart() {
    let origin = origin();
    let mut runs = vec![];
    assert!(follow_up::admit(&mut runs, &origin, thread()).unwrap());
    assert!(!follow_up::admit(&mut runs, &origin, thread()).unwrap());
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.save_follow_ups(&runs).unwrap();
    follow_up::restore(&store).unwrap();
    let mut runs = store.load_follow_ups().unwrap();
    assert!(!follow_up::admit(&mut runs, &origin, thread()).unwrap());
    let mut later = thread();
    later
        .comments
        .push(comment("102", "A later question", Some("100")));
    assert!(follow_up::admit(&mut runs, &origin, later).unwrap());
    assert_ne!(runs[0].key, runs[1].key);
    let operation = runs[0].operation("thread_reply", 100);
    assert_eq!(operation.owned_thread_id.as_deref(), Some("thread-node"));
    assert_eq!(
        operation.triggering_external_comment_id.as_deref(),
        Some("101")
    );
    assert_eq!(operation.pending_review_id.as_deref(), Some("42"));
}

#[test]
fn unowned_resolved_and_self_only_threads_do_not_create_work() {
    let origin = origin();
    let mut other = thread();
    other.comments[0].author_id = Some("999".into());
    assert!(FollowUp::new(&origin, other).is_err());
    let mut resolved = thread();
    resolved.resolved = true;
    assert!(FollowUp::new(&origin, resolved).is_err());
    let mut own = thread();
    own.comments.truncate(1);
    let mut reply = comment(
        "201",
        "Automated reply <!-- pr-sniper:reply:own -->",
        Some("100"),
    );
    reply.author_id = Some("22".into());
    own.comments.push(reply);
    assert!(FollowUp::new(&origin, own.clone()).is_err());
    own.comments[1].body = "A manual question from the signed-in human".into();
    assert!(FollowUp::new(&origin, own).is_ok());
}

#[test]
fn one_signed_reply_has_a_confirmed_receipt_and_cannot_repeat() {
    let mut fixture = Fixture::new();
    let mut run = prepared(&fixture.origin);
    follow_up::publish(&mut fixture, &mut run).unwrap();
    assert_eq!(fixture.writes(), 1);
    assert_eq!(run.phase, Phase::Published);
    assert_eq!(run.receipt.as_deref(), Some("200"));
    assert!(run.body.as_ref().unwrap().ends_with("\u{f05b} PR Sniper"));
    assert!(follow_up::publish(&mut fixture, &mut run).is_err());
    assert_eq!(fixture.writes(), 1);
    let Observation::Owned(current) = fixture.observe(&run).unwrap() else {
        panic!("Owned fixture expected")
    };
    assert_eq!(current.latest_external("22").unwrap().id, "101");
}

#[test]
fn accepted_but_lost_response_and_crash_before_receipt_storage_reconcile_once() {
    for crash in [false, true] {
        let mut fixture = Fixture::new();
        let mut run = prepared(&fixture.origin);
        if crash {
            fixture.fail_save = true;
        } else {
            fixture.wire.0.lock().unwrap().fault = Some(Fault::After);
        }
        assert!(follow_up::publish(&mut fixture, &mut run).is_err());
        fixture.fail_save = false;
        run = fixture.resume();
        follow_up::publish(&mut fixture, &mut run).unwrap();
        assert_eq!(fixture.writes(), 1);
        assert_eq!(run.receipt.as_deref(), Some("200"));
    }
}

#[test]
fn missing_uncertain_reply_never_reposts_after_restart_or_new_budget() {
    let mut fixture = Fixture::new();
    fixture.wire.0.lock().unwrap().fault = Some(Fault::Before);
    let mut run = prepared(&fixture.origin);
    assert!(follow_up::publish(&mut fixture, &mut run).is_err());
    for _ in 0..3 {
        run = fixture.resume();
        assert!(follow_up::publish(&mut fixture, &mut run).is_err());
    }
    assert_eq!(
        run.publication.as_ref().unwrap().state,
        OperationState::ManualRetry
    );
    fixture.now = 2000;
    run.publication = Some(run.operation("thread_reply", 2000));
    assert!(follow_up::publish(&mut fixture, &mut run).is_err());
    assert_eq!(fixture.writes(), 1);
    assert_eq!(run.phase, Phase::Unresolved);
}

#[test]
fn resolution_permission_withdrawal_and_head_changes_stop_before_or_after_reply() {
    for change in 0..5 {
        let mut fixture = Fixture::new();
        let mut run = prepared(&fixture.origin);
        match change {
            0 => fixture.wire.0.lock().unwrap().threads[0].resolved = true,
            1 => fixture.withdrawn = true,
            2 => fixture.wire.0.lock().unwrap().changed_head = true,
            3 => fixture.wire.0.lock().unwrap().threads[0].can_reply = false,
            _ => fixture.wire.0.lock().unwrap().threads[0]
                .comments
                .push(comment("102", "Later input", Some("100"))),
        }
        assert!(follow_up::publish(&mut fixture, &mut run).is_err());
        assert_eq!(fixture.writes(), 0);
    }
    for external in [false, true] {
        let mut fixture = Fixture::new();
        let mut run = prepared(&fixture.origin);
        {
            let mut server = fixture.wire.0.lock().unwrap();
            server.resolved_after = !external;
            server.external_after = external;
        }
        follow_up::publish(&mut fixture, &mut run).unwrap();
        assert_eq!(fixture.writes(), 1);
        assert_eq!(run.phase, Phase::StaleAfterPublication);
        assert_eq!(run.receipt.as_deref(), Some("200"));
    }
}

#[test]
fn provider_rejection_is_permanent_but_rate_limit_keeps_its_budget() {
    for fault in [Fault::Reject, Fault::RateLimit] {
        let mut fixture = Fixture::new();
        let mut run = prepared(&fixture.origin);
        fixture.wire.0.lock().unwrap().fault = Some(fault);
        follow_up::publish(&mut fixture, &mut run).unwrap_err();
        assert!(!run.uncertain);
        if matches!(fault, Fault::Reject) {
            assert_eq!(
                run.publication.as_ref().unwrap().state,
                OperationState::Failed
            );
            assert!(!run.error.as_ref().unwrap().contains("private detail"));
        } else {
            let id = run.publication.as_ref().unwrap().id.clone();
            assert_eq!(run.publication.as_ref().unwrap().next_attempt_at, Some(117));
            run = fixture.resume();
            follow_up::publish(&mut fixture, &mut run).unwrap();
            assert_eq!(run.publication.as_ref().unwrap().id, id);
            assert_eq!(fixture.writes(), 2);
        }
    }
}

#[test]
fn human_input_and_quiet_cannot_be_published_even_with_an_operation() {
    for decision in [ReplyDecision::Quiet, ReplyDecision::HumanInputRequired] {
        let mut fixture = Fixture::new();
        let mut run = prepared(&fixture.origin);
        run.result.as_mut().unwrap().output.decision = decision;
        assert!(follow_up::publish(&mut fixture, &mut run).is_err());
        assert_eq!(fixture.writes(), 0);
    }
}

#[test]
fn unpublished_drafts_are_never_triggers_and_latest_means_latest_publication() {
    let fixture = Fixture::new();
    fixture.wire.0.lock().unwrap().draft_replies = true;
    let client = GithubClient::new(fixture.wire.clone());
    let owned = client.owned_threads(&fixture.origin).unwrap().remove(0);
    assert_eq!(owned.comments.len(), 1);
    assert!(owned.latest_external("22").is_none());
    assert!(FollowUp::new(&fixture.origin, owned).is_err());
    fixture.wire.0.lock().unwrap().draft_replies = false;
    let mut owned = client.owned_threads(&fixture.origin).unwrap().remove(0);
    let mut newer = comment("102", "Published before the earlier draft", Some("100"));
    newer.published_at = "2026-09-27T00:01:00Z".into();
    owned.comments.push(newer);
    owned.comments[1].published_at = "2026-09-27T00:02:00Z".into();
    assert_eq!(owned.latest_external("22").unwrap().id, "101");
}

#[test]
fn graphql_rate_limit_remains_retryable_without_accepting_partial_data() {
    struct RateLimit;
    impl Transport for RateLimit {
        fn get(&self, _: &str) -> Result<Response, ConnectionError> {
            panic!("Unexpected GET")
        }
    }
    impl QueryTransport for RateLimit {
        fn query(&self, _: &str, _: Value) -> Result<Response, ConnectionError> {
            Ok(Response {
                headers: BTreeMap::from([("retry-after".into(), "23".into())]),
                ..response(
                    json!({"data":{},"errors":[{"type":"RATE_LIMITED","message":"private"}]}),
                )
            })
        }
    }
    assert_eq!(
        GithubClient::new(RateLimit)
            .owned_threads(&origin())
            .unwrap_err(),
        ConnectionError::RateLimitedAfter(23)
    );
}

#[test]
fn graphql_threads_and_comments_are_fully_paginated_and_partial_data_fails_closed() {
    let fixture = Fixture::new();
    {
        let mut server = fixture.wire.0.lock().unwrap();
        server.threads[0]
            .comments
            .extend((102..205).map(|i| comment(&i.to_string(), "External input", Some("100"))));
        server.threads.extend((0..100).map(|i| {
            let mut t = thread();
            t.id = format!("other-{i}");
            t.comments[0].id = format!("{}", 1000 + i);
            t
        }));
    }
    let client = GithubClient::new(fixture.wire.clone());
    let threads = client.owned_threads(&fixture.origin).unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].comments.len(), 105);
    assert_eq!(
        client
            .owned_thread(&fixture.origin, "thread-node")
            .unwrap()
            .unwrap()
            .comments
            .len(),
        105
    );
    fixture.wire.0.lock().unwrap().wrong_count = true;
    assert!(client.owned_threads(&fixture.origin).is_err());
    fixture.wire.0.lock().unwrap().graphql_error = true;
    assert!(client.owned_thread(&fixture.origin, "thread-node").is_err());
}
