//! Review publication, threads and readiness against a fake Azure DevOps.

use super::*;
use crate::azure_devops::read::threads::ThreadRef;
use crate::exits::gates::{self, Facts, GateInput, Grants};
use crate::monitoring::{JobOperation, OperationFailure, OperationState, QueueJob};
use crate::provider::{
    Lifecycle, PublicationReader, PullRequest, PullRequestStateReader, ReadinessReader,
    ReviewContext, ReviewFile, ReviewPublisher, ThreadReader, ThreadWriter,
};
use crate::publication::{self, Batch, Environment, Gate, Phase, Publication, RemoteState};
use crate::review::{Failure, ReviewRun, Selection};
use std::collections::BTreeMap;

fn sha(n: u32) -> String {
    format!("{n:040x}")
}

fn git(path: &str) -> String {
    format!("{ORG}/{PROJECT}/_apis/git/repositories/{REPO}/{path}")
}

fn threads_url() -> String {
    git("pullRequests/7/threads?")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

fn session(wire: &Arc<FakeWire>, runtime: &tokio::runtime::Runtime) -> Session {
    wire.json(
        &format!("{ORG}/_apis/connectionData"),
        identity_body(AVERY, "Avery Example"),
    );
    Session::new(reader(wire, runtime).unwrap())
}

fn iterations(wire: &FakeWire) {
    wire.json(
        &git("pullRequests/7/iterations?"),
        json!({"value": [
            {"id": 1, "sourceRefCommit": {"commitId": sha(1)}, "commonRefCommit": {"commitId": sha(900)}},
            {"id": 2, "sourceRefCommit": {"commitId": sha(2)}, "commonRefCommit": {"commitId": sha(900)}},
        ]}),
    );
}

fn review() -> ReviewRun {
    let assignment_id = "00000000-0000-0000-0000-0000000000e1";
    let agent_id = "00000000-0000-0000-0000-0000000000e2";
    let settings: crate::storage::Settings = serde_json::from_value(json!({
        "launch_at_login": false,
        "repositories": [{"id": "00000000-0000-0000-0000-0000000000e3", "provider": "azure_devops",
            "name": "contoso/Web/app", "enabled": true,
            "provider_account_id": AVERY, "provider_repository_id": key().id(),
            "watched_authors": [{"id": BLAKE, "login": "Blake Example"}],
            "assignments": [{"id": assignment_id, "agent_id": agent_id,
                "schedule": {"kind": "interval", "minutes": 5, "timezone": "UTC"}, "comment": true}]}],
        "agents": [{"id": agent_id, "name": "Reviewer", "model": "model",
            "ai_account": {"provider": "copilot", "account_id": "33"},
            "prompt": "Review correctness.", "signature": "machine"}]
    }))
    .unwrap();
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id": assignment_id, "provider": "azure_devops", "account_id": AVERY,
        "account_login": "Avery Example", "configuration_id": "00000000-0000-0000-0000-0000000000e3",
        "repository_id": key().id(), "repository_name": "contoso/Web/app",
        "pull_request_id": "7", "number": 7, "title": "Change 7", "head_sha": sha(2),
        "trigger_policy": format!("[[\"{BLAKE}\"],true]"), "author_id": BLAKE, "author_login": "Blake Example",
        "watched_author": true, "requested_reviewer": false, "waiting": "human_start",
        "detected_at": 100
    }))
    .unwrap();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    ReviewRun {
        mode: crate::review::RunMode::Packet,
        packet: None,
        failed_duck: None,
        failed_model: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: crate::review::key(&job, assignment_id),
        selection: Selection::resolve(&settings, &job, assignment_id)
            .unwrap()
            .into(),
        job,
        assignment_id: assignment_id.into(),
        operation,
        manual_start: true,
        trust_confirmed: true,
        phase: "Complete".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({
                "reviewed_base_sha": sha(900),
                "output": {"exit": {"exit": "comments", "new_comments": [
                    {"path": "src/main.rs", "line": 2, "body": "Handle the error."},
                    {"path": "src/main.rs", "line": 1, "side": "base", "body": "Keep the old name."}
                ], "reply_comments": [], "resolve_own_threads": []}},
                "session_id": "session", "model": "model", "runtime_version": "fixture",
                "input_tokens": 1, "output_tokens": 1, "tool_calls": 0
            }))
            .unwrap(),
        ),
    }
}

/// A publication with its batch frozen from a locally diffed context.
fn publication() -> Publication {
    let review = review();
    let mut run = Publication::new(review, true, false, 100).unwrap();
    let context = ReviewContext {
        pull: PullRequest {
            mentioned: false,
            id: "7".into(),
            number: 7,
            title: "Change 7".into(),
            author: None,
            requested_reviewers: vec![],
            requested_teams: vec![],
            state: Lifecycle::Open,
            draft: false,
            head_sha: sha(2),
            base_sha: sha(900),
            head_repository_id: Some(key().id()),
            base_repository_id: key().id(),
            updated_at: String::new(),
            files: vec![],
        },
        base_revision: sha(900),
        files: vec![ReviewFile {
            path: "src/main.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: super::super::diff::patch(Some(b"old\nsame\n"), Some(b"new\nfix\nsame\n")),
        }],
        head: BTreeMap::new(),
        base: BTreeMap::new(),
    };
    run.batch = Some(Batch::prepare(&run, &context).unwrap());
    run
}

fn author(id: &str) -> Value {
    json!({"id": id, "displayName": "Someone"})
}

/// A thread as Azure DevOps returns it, with `iteration` as its context.
fn thread(id: u64, path: &str, side: &str, line: u64, author_id: &str, content: &str) -> Value {
    let start = if side == "LEFT" {
        "leftFileStart"
    } else {
        "rightFileStart"
    };
    json!({
        "id": id, "status": "active",
        "threadContext": {"filePath": format!("/{path}"), start: {"line": line, "offset": 1}},
        "pullRequestThreadContext": {"iterationContext": {"firstComparingIteration": 1, "secondComparingIteration": 2}},
        "comments": [{"id": 1, "parentCommentId": 0, "author": author(author_id), "content": content,
            "publishedDate": "2026-10-27T10:00:00Z", "commentType": "text"}]
    })
}

fn published_threads(run: &Publication, count: usize) -> Value {
    let batch = run.batch.as_ref().unwrap();
    let threads: Vec<_> = batch
        .comments
        .iter()
        .take(count)
        .enumerate()
        .map(|(n, c)| thread(41 + n as u64, &c.path, &c.side, c.line, AVERY, &c.body))
        .collect();
    json!({"value": threads})
}

/// Drives the real publication state machine against the session.
struct Env<'a> {
    session: &'a Session,
}

impl Environment for Env<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(200)
    }
    fn save(&mut self, _: &mut Publication) -> Result<(), Failure> {
        Ok(())
    }
    fn inspect(&mut self, _: &Publication) -> Result<Gate, Failure> {
        Ok(Gate {
            stop: None,
            stale: false,
            requeue: false,
            discard: false,
        })
    }
    fn prepare(&mut self, _: &Publication) -> Result<Batch, Failure> {
        unreachable!("the batch is frozen")
    }
    fn reconcile(&mut self, run: &Publication) -> Result<Option<publication::Receipt>, Failure> {
        self.session.reconcile_publication(run)
    }
    fn divergent(&mut self, run: &Publication) -> Result<Option<publication::Receipt>, Failure> {
        self.session.divergent_publication(run)
    }
    fn mutate(
        &mut self,
        run: &Publication,
        mutation: publication::Mutation,
    ) -> Result<publication::Receipt, publication::WriteFailure> {
        self.session.mutate_publication(run, mutation)
    }
    fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
        Ok(())
    }
}

fn posts(wire: &FakeWire) -> Vec<Request> {
    wire.sent
        .lock()
        .unwrap()
        .iter()
        .map(|(request, _)| request.clone())
        .filter(|request| request.method != Method::Get)
        .collect()
}

#[test]
fn publishes_inline_threads_at_file_lines_with_iteration_context_then_confirms_by_read_back() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    let mut run = publication();
    let batch = run.batch.clone().unwrap();
    assert_eq!(batch.comments.len(), 2);
    wire.json(
        &threads_url(),
        json!({"value": [
            // Another account's thread with this publication's marker is not ours.
            thread(9, "src/main.rs", "RIGHT", 2, BLAKE, &batch.comments[0].body),
        ]}),
    );
    iterations(&wire);
    wire.json(
        &git("pullRequests/7/iterations/2/changes?"),
        json!({"changeEntries": [{"changeTrackingId": 5, "item": {"path": "/src/main.rs"}}]}),
    );
    let created = published_threads(&run, 2);
    for thread in created["value"].as_array().unwrap() {
        wire.json(&threads_url(), thread.clone());
    }
    wire.json(&threads_url(), created.clone());

    publication::execute(&mut Env { session: &session }, &mut run).unwrap();

    assert_eq!(run.phase, Phase::Published);
    assert_eq!(run.operation.state, OperationState::Completed);
    let receipt = run.receipts.last().unwrap();
    assert_eq!(receipt.state, RemoteState::Commented);
    assert_eq!(receipt.review_id, run.id);
    assert_eq!(
        receipt.comment_ids,
        [
            format!("{}/pullRequests/7/threads/41/comments/1", key().id()),
            format!("{}/pullRequests/7/threads/42/comments/1", key().id()),
        ]
    );
    let writes = posts(&wire);
    assert_eq!(writes.len(), 2, "one thread per finding, no summary");
    for (request, comment) in writes.iter().zip(&batch.comments) {
        assert_eq!(request.method, Method::Post);
        assert!(request.url.starts_with(&threads_url()), "{}", request.url);
        assert!(request.url.contains("api-version=7.1"));
        let body = request.body.as_ref().unwrap();
        assert_eq!(body["status"], "active");
        assert_eq!(body["comments"][0]["content"], comment.body);
        assert_eq!(body["comments"][0]["parentCommentId"], 0);
        assert!(comment
            .body
            .ends_with(&format!("<!-- pr-sniper:comment:{}:{} -->", run.id, {
                if comment.side == "LEFT" {
                    1
                } else {
                    0
                }
            })));
        let context = &body["pullRequestThreadContext"];
        // Both sides of iteration 2: the left side is the merge base the
        // review diffed, even for base-side lines after several pushes.
        assert_eq!(context["iterationContext"]["firstComparingIteration"], 2);
        assert_eq!(context["iterationContext"]["secondComparingIteration"], 2);
        assert_eq!(context["changeTrackingId"], 5);
        assert_eq!(body["threadContext"]["filePath"], "/src/main.rs");
    }
    let right = &writes[0].body.as_ref().unwrap()["threadContext"];
    assert_eq!(right["rightFileStart"], json!({"line": 2, "offset": 1}));
    assert_eq!(right["rightFileEnd"], json!({"line": 2, "offset": 1}));
    assert!(right.get("leftFileStart").is_none());
    let left = &writes[1].body.as_ref().unwrap()["threadContext"];
    assert_eq!(left["leftFileStart"], json!({"line": 1, "offset": 1}));
    assert_eq!(left["leftFileEnd"], json!({"line": 1, "offset": 1}));
}

#[test]
fn read_back_pending_never_posts_again_and_confirms_once_every_thread_shows() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    let mut run = publication();
    // A create whose outcome was lost: only one thread is visible so far.
    run.uncertain = true;
    for _ in 0..3 {
        wire.json(&threads_url(), published_threads(&run, 1));
    }
    let error = publication::execute(&mut Env { session: &session }, &mut run).unwrap_err();
    assert_eq!(error.kind, OperationFailure::Network);
    assert!(posts(&wire).is_empty(), "nothing is posted again");
    assert_eq!(run.receipts.last().unwrap().state, RemoteState::Pending);
    assert_ne!(run.phase, Phase::Published);
    assert_ne!(run.operation.state, OperationState::Failed);

    // The read-back catches up on the next attempt: confirmed, still no write.
    run.retry(true, 200).unwrap();
    wire.json(&threads_url(), published_threads(&run, 2));
    publication::execute(&mut Env { session: &session }, &mut run).unwrap();
    assert_eq!(run.phase, Phase::Published);
    assert_eq!(run.receipts.last().unwrap().state, RemoteState::Commented);
    assert!(posts(&wire).is_empty());
}

#[test]
fn a_revision_already_published_is_never_published_again() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    let mut run = publication();
    wire.json(&threads_url(), published_threads(&run, 2));
    publication::execute(&mut Env { session: &session }, &mut run).unwrap();
    assert_eq!(run.phase, Phase::Published);
    assert!(posts(&wire).is_empty());
}

#[test]
fn a_refused_first_thread_tosses_the_review_without_any_remote_effect() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    let mut run = publication();
    wire.json(&threads_url(), json!({"value": []}));
    iterations(&wire);
    wire.json(
        &git("pullRequests/7/iterations/2/changes?"),
        json!({"changeEntries": []}),
    )
    .on(&threads_url(), Ok(status(400, None)));
    publication::execute(&mut Env { session: &session }, &mut run).unwrap();
    assert!(run.tossed());
    assert!(!run.uncertain);
    // No change tracking ID known: the thread is placed without one.
    let body = posts(&wire)[0].body.clone().unwrap();
    assert!(body["pullRequestThreadContext"]
        .get("changeTrackingId")
        .is_none());
}

#[test]
fn owned_threads_are_marked_replies_answer_and_resolution_is_silent() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    let mut run = publication();
    wire.json(&threads_url(), published_threads(&run, 2));
    publication::execute(&mut Env { session: &session }, &mut run).unwrap();

    let mut threads = published_threads(&run, 2);
    threads["value"].as_array_mut().unwrap().push(thread(
        50,
        "src/main.rs",
        "RIGHT",
        3,
        BLAKE,
        "A human comment.",
    ));
    iterations(&wire);
    wire.json(&threads_url(), threads.clone());
    iterations(&wire);
    let owned = session.owned_threads(&run).unwrap();
    assert_eq!(owned.len(), 2);
    let root = owned[0].root().unwrap();
    assert_eq!(root.review_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(root.original_commit.as_deref(), Some(sha(2).as_str()));
    assert_eq!(root.author_id.as_deref(), Some(AVERY));

    // The packet reads the same threads: own roots are this assignment's.
    let mut human = threads.clone();
    human["value"][0]["comments"].as_array_mut().unwrap().push(
        json!({"id": 2, "parentCommentId": 1, "author": author(BLAKE),
            "content": "Won't fix.", "publishedDate": "2026-10-27T11:00:00Z",
            "lastUpdatedDate": "2026-10-27T12:00:00Z", "commentType": "text"}),
    );
    readiness_threads(&wire, human);
    let repository = RemoteRepository {
        id: key().id(),
        name: "contoso/Web/app".into(),
    };
    let observation = session.action_observation(&repository, 7).unwrap();
    let mut conversation = crate::packet::reads::conversation(&observation);
    crate::review::findings::stamp_ownership(
        &mut conversation.threads,
        &[crate::provider::Provenance::ownership(&run).unwrap()],
        &run.review.assignment_id,
        AVERY,
    );
    let mine: Vec<_> = conversation.threads.iter().map(|t| t.mine).collect();
    assert_eq!(mine, [true, true, false]);
    let first = &conversation.threads[0];
    assert_eq!(first.path.as_deref(), Some("src/main.rs"));
    assert_eq!(first.line, Some(2));
    assert_eq!(first.comments[0].id, root.id);
    assert_eq!(first.comments[0].author_id.as_deref(), Some(AVERY));
    assert_eq!(first.comments[1].author_id.as_deref(), Some(BLAKE));
    let raw = &observation.threads[0].comments[1];
    assert_eq!(raw["updatedAt"], "2026-10-27T12:00:00Z");
    assert_eq!(raw["originalCommit"]["oid"], sha(2));

    wire.json(
        &git("pullRequests/7/iterations?"),
        json!({"value": [{"id": 3, "sourceRefCommit": {"commitId": sha(3)}}]}),
    );
    assert_eq!(
        session.owned_threads(&run).err(),
        Some(ConnectionError::RevisionChanged)
    );
    // A root of this publication that no longer verifies is not dropped silently.
    let mut tampered = threads.clone();
    tampered["value"][0]["comments"][0]["content"] = json!("Edited.");
    iterations(&wire);
    wire.json(&threads_url(), tampered);
    iterations(&wire);
    assert_eq!(
        session.owned_threads(&run).err(),
        Some(ConnectionError::InvalidResponse)
    );

    let thread_id = owned[0].id.clone();
    assert_eq!(
        thread_id,
        format!("{}/pullRequests/7/threads/41", key().id())
    );
    wire.json(
        &git("pullRequests/7/threads/41/comments?"),
        json!({"id": 2}),
    );
    let reply = session
        .reply_to_review_thread("contoso/Web/app", 7, AVERY, &root.id, "Thanks, fixed.")
        .ok()
        .unwrap();
    assert_eq!(reply, format!("{thread_id}/comments/2"));
    assert!(session
        .reply_to_review_thread("contoso/Web/app", 7, BLAKE, &root.id, "x")
        .is_err());

    wire.json(&git("pullRequests/7/threads/41?"), json!({"id": 41}));
    assert!(session.resolve_review_thread(&thread_id, "request").is_ok());
    let writes = posts(&wire);
    let reply_request = &writes[0];
    assert_eq!(reply_request.method, Method::Post);
    assert_eq!(
        reply_request.body,
        Some(json!({"parentCommentId": 1, "content": "Thanks, fixed.", "commentType": 1}))
    );
    let resolve = writes.last().unwrap();
    assert_eq!(resolve.method, Method::Patch);
    assert!(resolve.url.starts_with(&git("pullRequests/7/threads/41?")));
    assert_eq!(resolve.body, Some(json!({"status": "fixed"})));
    assert_eq!(writes.len(), 2, "resolution posts no comment");
}

#[test]
fn global_thread_and_comment_ids_round_trip() {
    let thread = ThreadRef {
        key: key(),
        number: 7,
        thread: 41,
    };
    assert_eq!(ThreadRef::parse(&thread.id()), Some(thread.clone()));
    assert_eq!(
        ThreadRef::parse_comment(&thread.comment_id(3)),
        Some((thread.clone(), 3))
    );
    for invalid in [
        "41".to_string(),
        format!("{}/pullRequests/07/threads/41", key().id()),
        format!("{}/pullRequests/7/threads/0", key().id()),
        format!("{}/comments/1", thread.id()).replace("comments", "replies"),
    ] {
        assert_eq!(ThreadRef::parse(&invalid), None, "{invalid}");
        assert_eq!(ThreadRef::parse_comment(&invalid), None, "{invalid}");
    }
}

fn readiness_wire(wire: &FakeWire, evaluations: Value, statuses: Value) {
    readiness_full(wire, evaluations, statuses, json!({"value": []}));
}

fn readiness_threads(wire: &FakeWire, threads: Value) {
    readiness_full(wire, json!({"value": []}), json!({"value": []}), threads);
}

fn readiness_full(wire: &FakeWire, evaluations: Value, statuses: Value, threads: Value) {
    let mut pull = json!({
        "pullRequestId": 7, "status": "active", "title": "Change 7",
        "description": "Body", "createdBy": author(BLAKE),
        "creationDate": "2026-10-20T10:00:00Z", "isDraft": false,
        "repository": {"id": REPO, "project": {"id": PROJECT}},
        "lastMergeSourceCommit": {"commitId": sha(2)},
        "lastMergeTargetCommit": {"commitId": sha(900)},
        "sourceRefName": "refs/heads/feature", "targetRefName": "refs/heads/main",
        "mergeStatus": "succeeded",
        "reviewers": [{"id": CASEY, "displayName": "Casey", "vote": 0}]
    });
    pull["labels"] = json!([]);
    wire.json(&git("pullrequests/7?"), pull);
    iterations(wire);
    wire.json(
        &format!("{ORG}/{PROJECT}/_apis/policy/evaluations?"),
        evaluations,
    )
    .json(&git("pullRequests/7/statuses?"), statuses)
    .json(&threads_url(), threads);
}

fn policy(kind: &str, status: &str, blocking: bool, settings: Value) -> Value {
    json!({"evaluationId": "e", "status": status, "configuration": {
        "isEnabled": true, "isBlocking": blocking, "type": {"id": kind}, "settings": settings}})
}

const BUILD: &str = "0609b952-1397-4640-95ec-e00a01b2c241";
const STATUS: &str = "cbdc66da-9728-4af8-aada-9a5a32e4a226";
const MINIMUM_REVIEWERS: &str = "fa4e907d-c16b-4a4c-9dfa-4906e5d171dd";

fn sign_off_denial(observation: &crate::provider::Observation) -> Option<String> {
    gates::lgtm_denial(&GateInput {
        tools: vec![],
        primary: true,
        grants: Grants {
            comment: true,
            reply: true,
            ..Grants::default()
        },
        review_drafts: false,
        capabilities: crate::provider::ProviderCapabilities::of(
            &crate::storage::ProviderId::AzureDevops,
        ),
        facts: Facts::from_observation(observation),
    })
}

#[test]
fn policy_evaluations_and_statuses_gate_sign_off_and_the_ready_notification() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    let build = |status| policy(BUILD, status, true, json!({"displayName": "CI"}));
    let reviewers = policy(MINIMUM_REVIEWERS, "rejected", true, json!({}));
    let statuses = json!({"value": [
        {"id": 1, "state": "failed", "context": {"genre": "lint", "name": "style"}, "iterationId": 2},
        {"id": 2, "state": "succeeded", "context": {"genre": "lint", "name": "style"}, "iterationId": 2},
        {"id": 3, "state": "failed", "context": {"genre": "old", "name": "check"}, "iterationId": 1},
    ]});
    let repository = RemoteRepository {
        id: key().id(),
        name: "contoso/Web/app".into(),
    };

    // Build running: CI pending, so no sign-off and no ready notification.
    readiness_wire(
        &wire,
        json!({"value": [build("running"), reviewers.clone()]}),
        statuses.clone(),
    );
    let observation = session.action_observation(&repository, 7).unwrap();
    assert_eq!(observation.checks.as_deref(), Some("PENDING"));
    assert_eq!(observation.account_id, AVERY);
    assert_eq!(observation.author_id, BLAKE);
    assert_eq!(observation.base_name, "main");
    assert_eq!(observation.mergeable, "MERGEABLE");
    assert_eq!(
        observation.review_decision.as_deref(),
        Some("REVIEW_REQUIRED")
    );
    assert_eq!(
        observation.check_contexts,
        [
            json!({"__typename": "StatusContext", "context": "Build/CI", "state": "PENDING", "isRequired": true}),
            json!({"__typename": "StatusContext", "context": "lint/style", "state": "SUCCESS", "isRequired": false}),
        ]
    );
    assert_eq!(
        sign_off_denial(&observation).as_deref(),
        Some("CI is pending. Do not sign off until it is green.")
    );
    let urls = wire.urls();
    let evaluations = urls
        .iter()
        .find(|url| url.contains("/_apis/policy/evaluations"))
        .unwrap();
    assert!(evaluations.contains(&format!(
        "artifactId=vstfs%3A%2F%2F%2FCodeReview%2FCodeReviewId%2F{PROJECT}%2F7"
    )));
    assert!(evaluations.contains("api-version=7.1-preview.1"));

    // Rejected build: CI red.
    readiness_wire(
        &wire,
        json!({"value": [build("rejected")]}),
        json!({"value": []}),
    );
    let observation = session.action_observation(&repository, 7).unwrap();
    assert_eq!(
        sign_off_denial(&observation).as_deref(),
        Some("CI is red. Do not sign off.")
    );

    // Nothing evaluated: CI unknown is never green.
    readiness_wire(&wire, json!({"value": []}), json!({"value": []}));
    let observation = session.action_observation(&repository, 7).unwrap();
    assert_eq!(observation.checks, None);
    assert_eq!(
        sign_off_denial(&observation).as_deref(),
        Some("CI not verified green. Do not sign off.")
    );

    // The newest status of a context decides, in any order: one that is not
    // a CI signal never lets an older red result back.
    readiness_wire(
        &wire,
        json!({"value": [build("approved")]}),
        json!({"value": [
            {"id": 9, "state": "notApplicable", "context": {"genre": "lint", "name": "style"}, "iterationId": 2},
            {"id": 4, "state": "failed", "context": {"genre": "lint", "name": "style"}, "iterationId": 2},
        ]}),
    );
    let observation = session.action_observation(&repository, 7).unwrap();
    assert_eq!(observation.check_contexts.len(), 1);
    assert_eq!(observation.checks.as_deref(), Some("SUCCESS"));

    // Approved build and status policy: the sign-off (and its ready
    // notification) is offered; approve stays a later delivery.
    let status_policy = policy(
        STATUS,
        "approved",
        false,
        json!({"statusGenre": "lint", "statusName": "style"}),
    );
    readiness_wire(
        &wire,
        json!({"value": [build("approved"), status_policy]}),
        statuses,
    );
    let observation = session.action_observation(&repository, 7).unwrap();
    assert_eq!(observation.checks.as_deref(), Some("SUCCESS"));
    assert_eq!(
        observation.check_contexts.len(),
        2,
        "status policy dedupes its status"
    );
    assert_eq!(sign_off_denial(&observation), None);

    // The heartbeat sees the same CI: settled green with the build required.
    readiness_wire(
        &wire,
        json!({"value": [build("running")]}),
        json!({"value": []}),
    );
    let snapshot = session.pr_snapshot(&repository, 7).unwrap();
    assert_eq!(snapshot.head_ref, "feature");
    assert_eq!(snapshot.required_checks, ["Build/CI"]);
    assert!(snapshot.normalize().ci_pending());
    readiness_wire(
        &wire,
        json!({"value": [build("approved")]}),
        json!({"value": []}),
    );
    let normalized = session.pr_snapshot(&repository, 7).unwrap().normalize();
    assert!(normalized.ci_green());
    assert!(!normalized.ci_pending());
}
