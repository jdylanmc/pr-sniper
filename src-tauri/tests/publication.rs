use pr_sniper_lib::{
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{GithubClient, Response, Transport},
        publication::{MutationTransport, Request},
        review::{ReviewContext, ReviewFile},
        ConnectionError,
    },
    monitoring::{JobOperation, OperationFailure, OperationState, QueueJob},
    publication::{
        self, Batch, Environment, Gate, Mutation, Phase, Publication, Receipt, RemoteState,
        WriteFailure,
    },
    review::{Failure, ReviewRun, Selection},
    storage::{Settings, Store},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn settings() -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":"repo","provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":"assignment","agent_id":"agent","schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":"agent","name":"Reviewer","model":"configured-model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"stored custom signature"}]
    })).unwrap()
}

fn job() -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":"assignment","provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":"repo","repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"all_authors":false,"requested_reviewer":false,
        "waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

fn pull() -> PullRequest {
    PullRequest {
        id: "9".into(),
        number: 1,
        title: "Review".into(),
        author: Some(pr_sniper_lib::github::Identity {
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
        updated_at: "2026-09-27T00:00:00Z".into(),
        files: vec![],
    }
}

fn review() -> ReviewRun {
    let job = job();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    ReviewRun {
        feedback_context: None,
        key: pr_sniper_lib::review::key(&job, "assignment"),
        selection: Selection::resolve(&settings(), &job, "assignment").unwrap(),
        job,
        assignment_id: "assignment".into(),
        operation,
        manual_start: true,
        trust_confirmed: true,
        phase: "Complete".into(),
        error: None,
        result: Some(serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),
            "output":{"synopsis":"The change needs review.","files":[{"path":"source.rs","explanation":"Implementation.","order":1}],
                "findings":[{"path":"source.rs","side":"head","line":1,"severity":"high","title":"A defect","explanation":"Evidence.","confidence":90}],
                "decision":"human_input_required"},
            "session_id":"session","model":"configured-model","runtime_version":"fixture",
            "input_tokens":10,"output_tokens":10,"tool_calls":1
        })).unwrap()),
    }
}

fn context(pull: PullRequest) -> ReviewContext {
    ReviewContext {
        pull,
        base_revision: "b".repeat(40),
        files: vec![ReviewFile {
            path: "source.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: Some("@@ -1 +1 @@\n-old\n+new".into()),
        }],
        head: BTreeMap::new(),
        base: BTreeMap::new(),
    }
}

#[derive(Default)]
struct Server {
    reviews: Vec<Value>,
    comments: BTreeMap<u64, Vec<Value>>,
    writes: Vec<(String, String, Value)>,
    fault: Option<(Mutation, Fault)>,
}

#[derive(Clone, Copy)]
enum Fault {
    Before,
    After,
    Reject,
    RateLimit,
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

impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let server = self.0.lock().unwrap();
        let (resource, query) = path.split_once('?').unwrap();
        let page = query
            .strip_prefix("per_page=100&page=")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let values = if resource.ends_with("/reviews") {
            server.reviews.clone()
        } else if resource.ends_with("/comments") {
            let id = resource.split('/').nth(7).unwrap().parse::<u64>().unwrap();
            server.comments.get(&id).cloned().unwrap_or_default()
        } else {
            panic!("Unexpected GET {path}");
        };
        let mut result = response(json!(values
            .iter()
            .skip((page - 1) * 100)
            .take(100)
            .collect::<Vec<_>>()));
        if page * 100 < values.len() {
            result.headers.insert(
                "link".into(),
                format!(
                    "<https://api.github.com{resource}?per_page=100&page={}>; rel=\"next\"",
                    page + 1
                ),
            );
        }
        Ok(result)
    }
}

impl MutationTransport for Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        let mut server = self.0.lock().unwrap();
        let (method, body, mutation) = match request {
            Request::Post(body) => (
                "POST",
                body,
                if path.ends_with("/events") {
                    Mutation::Submit
                } else {
                    Mutation::Create
                },
            ),
            Request::Delete => ("DELETE", Value::Null, Mutation::Discard),
        };
        let fault = if server.fault.is_some_and(|(target, _)| target == mutation) {
            server.fault.take().map(|(_, fault)| fault)
        } else {
            None
        };
        server
            .writes
            .push((method.into(), path.into(), body.clone()));
        if matches!(fault, Some(Fault::Before)) {
            return Err(ConnectionError::Timeout);
        }
        if matches!(fault, Some(Fault::Reject)) {
            return Ok(Response {
                status: 422,
                ..response(json!({"message":"private provider detail"}))
            });
        }
        if matches!(fault, Some(Fault::RateLimit)) {
            return Ok(Response {
                status: 429,
                headers: BTreeMap::from([("retry-after".into(), "17".into())]),
                ..response(json!({"message":"rate limited"}))
            });
        }
        let value = match mutation {
            Mutation::Create => {
                assert!(body.get("event").is_none(), "Create must remain pending");
                let id = server.reviews.len() as u64 + 1;
                let value = json!({"id":id,"user":{"id":22},"body":body["body"],"commit_id":body["commit_id"],
                    "state":"PENDING","pull_request_url":"https://api.github.com/repos/example/repo/pulls/1"});
                let comments = body["comments"].as_array().unwrap().iter().enumerate().map(|(i,c)| json!({
                    "id":id * 100 + i as u64,"user":{"id":22},"pull_request_review_id":id,
                    "path":c["path"],"body":c["body"],"side":c["side"],"line":c["line"],"original_line":c["line"],
                    "original_commit_id":body["commit_id"]
                })).collect();
                server.comments.insert(id, comments);
                server.reviews.push(value.clone());
                value
            }
            Mutation::Submit => {
                assert_eq!(body["event"], "COMMENT");
                let id = path.split('/').nth(7).unwrap().parse::<u64>().unwrap();
                let review = server.reviews.iter_mut().find(|r| r["id"] == id).unwrap();
                assert_eq!(review["state"], "PENDING");
                review["state"] = json!("COMMENTED");
                review["submitted_at"] = json!("2026-09-27T00:00:00Z");
                review.clone()
            }
            Mutation::Discard => {
                let id = path.rsplit('/').next().unwrap().parse::<u64>().unwrap();
                let index = server.reviews.iter().position(|r| r["id"] == id).unwrap();
                assert_eq!(server.reviews[index]["state"], "PENDING");
                server.reviews.remove(index)
            }
        };
        if matches!(fault, Some(Fault::After)) {
            return Err(ConnectionError::Timeout);
        }
        Ok(response(value))
    }
}

#[derive(Clone, Copy, Debug)]
enum Change {
    Head,
    Base,
    Draft,
    Closed,
    Author,
    Disabled,
    Comment,
    Gate,
    Cancel,
    Scope,
    Permission,
    HeadIneligible,
}

struct Fixture {
    _root: tempfile::TempDir,
    store: Store,
    wire: Wire,
    settings: Settings,
    pull: PullRequest,
    job: QueueJob,
    now: i64,
    inspections: usize,
    change: Option<(usize, Change)>,
    cancelled: bool,
    active: bool,
    can_comment: bool,
    requeues: usize,
    fail_save_after_writes: Option<usize>,
    move_head_during_prepare: bool,
    fail_inspection: Option<usize>,
    pause_after_mutation: Option<Mutation>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self {
            store: Store::new(root.path().into()),
            _root: root,
            wire: Wire(Arc::new(Mutex::new(Server::default()))),
            settings: settings(),
            pull: pull(),
            job: job(),
            now: 100,
            inspections: 0,
            change: None,
            cancelled: false,
            active: true,
            can_comment: true,
            requeues: 0,
            fail_save_after_writes: None,
            move_head_during_prepare: false,
            fail_inspection: None,
            pause_after_mutation: None,
        }
    }
    fn run(&mut self, run: &mut Publication) -> Result<(), Failure> {
        publication::execute(self, run)
    }
    fn visible(&self) -> usize {
        self.wire
            .0
            .lock()
            .unwrap()
            .reviews
            .iter()
            .filter(|r| r["state"] == "COMMENTED")
            .count()
    }
    fn writes(&self) -> usize {
        self.wire.0.lock().unwrap().writes.len()
    }
    fn resume(&mut self) -> Publication {
        publication::restore(&self.store).unwrap();
        let run = self.store.load_publications().unwrap().remove(0);
        self.now = run
            .operation
            .next_attempt_at
            .unwrap_or(self.now)
            .max(self.now);
        run
    }
}

impl Environment for Fixture {
    fn paused(&self) -> Result<bool, Failure> {
        self.store
            .load_automation()
            .map(|s| s.paused)
            .map_err(Failure::permanent)
    }
    fn now(&self) -> Result<i64, Failure> {
        Ok(self.now)
    }
    fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
        if self
            .fail_save_after_writes
            .is_some_and(|count| self.writes() >= count)
        {
            return Err(Failure::permanent("Injected receipt persistence failure."));
        }
        self.store
            .save_publications(std::slice::from_ref(run))
            .map_err(Failure::permanent)
    }
    fn inspect(&mut self, run: &Publication) -> Result<Gate, Failure> {
        self.inspections += 1;
        if self.fail_inspection == Some(self.inspections) {
            return Err(ConnectionError::Timeout.into());
        }
        if let Some((at, change)) = self.change {
            if self.inspections == at {
                self.change = None;
                match change {
                    Change::Head => self.pull.head_sha = "c".repeat(40),
                    Change::Base => self.pull.base_sha = "c".repeat(40),
                    Change::Draft => self.pull.draft = true,
                    Change::Closed => self.pull.state = Lifecycle::Closed,
                    Change::Author => {
                        self.settings.repositories[0].watched_authors[0].id = "99".into()
                    }
                    Change::Disabled => self.settings.repositories[0].enabled = false,
                    Change::Comment => self.settings.repositories[0].assignments[0].comment = false,
                    Change::Gate => self.settings.defaults.automatic_comment_publication = true,
                    Change::Cancel => self.cancelled = true,
                    Change::Scope => self.active = false,
                    Change::Permission => self.can_comment = false,
                    Change::HeadIneligible => {
                        self.pull.head_sha = "c".repeat(40);
                        self.pull.draft = true;
                    }
                }
            }
        }
        let mut gate = publication::evaluate_gate(
            &self.settings,
            run,
            Some(&self.job),
            &self.pull,
            self.active,
            self.can_comment,
            self.cancelled,
        );
        if let Err(error) = pr_sniper_lib::capacity::publication_gate(&self.store) {
            gate.stop = Some(error);
        }
        Ok(gate)
    }
    fn prepare(&mut self, run: &Publication) -> Result<Batch, Failure> {
        if self.move_head_during_prepare {
            self.pull.head_sha = "c".repeat(40);
            return Err(ConnectionError::RevisionChanged.into());
        }
        Batch::prepare(run, &context(self.pull.clone()))
    }
    fn reconcile(&mut self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        GithubClient::new(self.wire.clone()).reconcile_publication(run)
    }
    fn mutate(&mut self, run: &Publication, mutation: Mutation) -> Result<Receipt, WriteFailure> {
        pr_sniper_lib::capacity::publication_gate(&self.store).map_err(|error| WriteFailure {
            failure: Failure::permanent(error),
            uncertain: false,
        })?;
        let gate = self.inspect(run).unwrap();
        if mutation != Mutation::Discard {
            if let Some(reason) = gate.stop {
                return Err(WriteFailure {
                    failure: Failure::permanent(reason),
                    uncertain: false,
                });
            }
        }
        let result = GithubClient::new(self.wire.clone()).mutate_publication(run, mutation);
        if self.pause_after_mutation == Some(mutation) {
            self.store
                .save_automation(&pr_sniper_lib::capacity::Automation { paused: true })
                .unwrap();
        }
        result
    }
    fn requeue(&mut self, _run: &Publication) -> Result<(), Failure> {
        self.requeues += 1;
        Ok(())
    }
}

#[test]
fn pausing_during_a_lost_submit_keeps_the_receipt_and_never_sends_a_replacement() {
    let mut fixture = Fixture::new();
    fixture.pause_after_mutation = Some(Mutation::Submit);
    fixture.wire.0.lock().unwrap().fault = Some((Mutation::Submit, Fault::After));
    let mut run = publication();
    assert!(fixture.run(&mut run).is_err());
    assert!(fixture.store.load_automation().unwrap().paused);
    assert_eq!(fixture.visible(), 1);
    assert_eq!(fixture.writes(), 2);
    assert!(run.uncertain);
    let id = run.id.clone();
    fixture
        .store
        .save_automation(&pr_sniper_lib::capacity::Automation { paused: false })
        .unwrap();
    let mut restored = fixture.resume();
    restored.retry(false, fixture.now).unwrap();
    fixture.run(&mut restored).unwrap();
    assert_eq!(restored.id, id);
    assert_eq!(
        restored.receipts.last().unwrap().state,
        RemoteState::Commented
    );
    assert_eq!(fixture.writes(), 2);
    assert_eq!(fixture.visible(), 1);
}

#[test]
fn pause_after_creating_pending_comments_does_not_undo_or_replace_the_remote_batch() {
    let mut fixture = Fixture::new();
    fixture.pause_after_mutation = Some(Mutation::Create);
    let mut run = publication();
    fixture.run(&mut run).unwrap();
    assert_eq!(run.operation.state, OperationState::Interrupted);
    assert_eq!(fixture.writes(), 1);
    assert_eq!(run.receipts.last().unwrap().state, RemoteState::Pending);
    let receipt = run.receipts[0].review_id.clone();
    fixture
        .store
        .save_automation(&pr_sniper_lib::capacity::Automation { paused: false })
        .unwrap();
    run.retry(false, 101).unwrap();
    fixture.now = 101;
    fixture.run(&mut run).unwrap();
    assert_eq!(
        run.operation.pending_review_id.as_deref(),
        Some(receipt.as_str())
    );
    assert_eq!(
        fixture
            .wire
            .0
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|(method, path, _)| method == "POST" && !path.ends_with("/events"))
            .count(),
        1
    );
    assert_eq!(run.receipts.last().unwrap().state, RemoteState::Commented);
    assert_eq!(fixture.writes(), 2);
}

fn publication() -> Publication {
    Publication::new(review(), false, true, 100).unwrap()
}

#[test]
fn stages_one_revision_bound_batch_and_submits_comment_with_confirmed_receipts() {
    let mut fixture = Fixture::new();
    let mut run = publication();
    fixture.run(&mut run).unwrap();
    assert_eq!(fixture.visible(), 1);
    assert_eq!(fixture.writes(), 2);
    assert_eq!(run.phase, Phase::Published);
    assert_eq!(run.operation.state, OperationState::Completed);
    assert_eq!(run.receipts.last().unwrap().comment_ids, vec!["100"]);
    assert!(run
        .batch
        .as_ref()
        .unwrap()
        .body
        .ends_with("\u{f05b} PR Sniper"));
    assert!(run.retry(false, 200).is_err());
    assert!(fixture.run(&mut run).is_err());
    assert_eq!(fixture.writes(), 2);
}

#[test]
fn every_before_after_mutation_seam_revalidates_policy_revision_and_lifecycle() {
    for change in [
        Change::Head,
        Change::Base,
        Change::Draft,
        Change::Closed,
        Change::Author,
        Change::Disabled,
        Change::Comment,
        Change::Gate,
        Change::Cancel,
        Change::Scope,
        Change::Permission,
        Change::HeadIneligible,
    ] {
        for seam in 1..=7 {
            let mut fixture = Fixture::new();
            fixture.change = Some((seam, change));
            let mut run = publication();
            let _ = fixture.run(&mut run);
            assert!(
                fixture.change.is_none(),
                "unexercised seam {seam}: {change:?}"
            );
            assert_eq!(
                fixture.visible(),
                usize::from(seam == 7),
                "seam {seam}: {change:?}"
            );
            if seam == 7 {
                assert_eq!(run.phase, Phase::StaleAfterPublication);
            } else {
                assert!(
                    !fixture
                        .wire
                        .0
                        .lock()
                        .unwrap()
                        .reviews
                        .iter()
                        .any(|r| r["state"] == "PENDING"),
                    "pending output not discarded at seam {seam}: {change:?}"
                );
            }
            if matches!(change, Change::Head) {
                assert!(fixture.requeues > 0);
            } else {
                assert_eq!(
                    fixture.requeues, 0,
                    "ineligible work requeued at seam {seam}: {change:?}"
                );
            }
        }
    }
}

#[test]
fn lost_create_and_submit_responses_reconcile_without_duplicate_batches() {
    for mutation in [Mutation::Create, Mutation::Submit] {
        let mut fixture = Fixture::new();
        fixture.wire.0.lock().unwrap().fault = Some((mutation, Fault::After));
        let mut run = publication();
        assert!(fixture.run(&mut run).is_err());
        assert!(run.uncertain);
        let mut restored = fixture.resume();
        fixture.run(&mut restored).unwrap();
        assert_eq!(fixture.visible(), 1);
        assert_eq!(fixture.writes(), 2);
        assert_eq!(restored.operation.attempt_count, 2);
        assert_eq!(restored.operation.retry_deadline, 1000);
    }
}

#[test]
fn missing_uncertain_create_never_reposts_even_with_manual_new_budget() {
    let mut fixture = Fixture::new();
    fixture.wire.0.lock().unwrap().fault = Some((Mutation::Create, Fault::Before));
    let mut run = publication();
    assert!(fixture.run(&mut run).is_err());
    for _ in 0..3 {
        run = fixture.resume();
        assert!(fixture.run(&mut run).is_err());
    }
    assert_eq!(run.operation.state, OperationState::ManualRetry);
    assert_eq!(fixture.writes(), 1);
    let id = run.id.clone();
    run.retry(false, 1200).unwrap();
    fixture.now = 1200;
    assert!(fixture.run(&mut run).is_err());
    assert_eq!(run.id, id);
    assert_eq!(run.history.len(), 1);
    assert_eq!(fixture.writes(), 1);
}

#[test]
fn crash_after_server_acceptance_before_receipt_persistence_preserves_intent() {
    for submit in [false, true] {
        let mut fixture = Fixture::new();
        let mut run = publication();
        fixture.fail_save_after_writes = Some(if submit { 2 } else { 1 });
        assert!(fixture.run(&mut run).is_err());
        fixture.fail_save_after_writes = None;
        let mut restored = fixture.resume();
        assert!(restored.uncertain);
        fixture.run(&mut restored).unwrap();
        assert_eq!(fixture.visible(), 1);
        assert_eq!(fixture.writes(), 2);
    }
}

#[test]
fn uncertain_discard_is_reconciled_and_cannot_resurrect_a_batch() {
    let mut fixture = Fixture::new();
    fixture.change = Some((4, Change::Head));
    fixture.wire.0.lock().unwrap().fault = Some((Mutation::Discard, Fault::After));
    let mut run = publication();
    assert!(fixture.run(&mut run).is_err());
    let mut restored = fixture.resume();
    fixture.run(&mut restored).unwrap();
    assert_eq!(restored.phase, Phase::Stale);
    assert_eq!(fixture.visible(), 0);
    assert_eq!(fixture.writes(), 2);
    assert_eq!(
        restored.receipts.last().unwrap().state,
        RemoteState::Deleted
    );
}

#[test]
fn explicit_provider_rejection_is_permanent_and_does_not_leak_response() {
    let mut fixture = Fixture::new();
    fixture.wire.0.lock().unwrap().fault = Some((Mutation::Create, Fault::Reject));
    let mut run = publication();
    fixture.run(&mut run).unwrap_err();
    assert_eq!(run.operation.state, OperationState::Failed);
    assert_eq!(run.operation.failure, Some(OperationFailure::Permanent));
    assert!(!run.uncertain);
    assert!(!run.error.unwrap().contains("private provider detail"));
    assert_eq!(fixture.visible(), 0);
}

#[test]
fn unmappable_findings_remain_local_and_no_findings_produces_machine_signoff() {
    let mut run = publication();
    run.review.result.as_mut().unwrap().output.findings[0].line = 200;
    let batch = Batch::prepare(&run, &context(pull())).unwrap();
    assert!(batch.comments.is_empty());
    assert_eq!(batch.unmappable, vec![0]);
    assert_eq!(run.review.result.as_ref().unwrap().output.findings.len(), 1);
    assert!(batch.body.contains("1 finding(s) could not be mapped"));
    let result = run.review.result.as_mut().unwrap();
    result.output.findings.clear();
    result.output.decision = pr_sniper_lib::review::Decision::MachineSignOff;
    let batch = Batch::prepare(&run, &context(pull())).unwrap();
    assert!(batch.body.contains("Machine sign-off"));
    assert!(batch.body.ends_with("\u{f05b} PR Sniper"));
    assert!(!batch.body.contains("stored custom signature"));
}

#[test]
fn confirmed_submission_can_retry_only_its_failed_post_publication_verification() {
    let mut fixture = Fixture::new();
    fixture.fail_inspection = Some(7);
    let mut run = publication();
    fixture.run(&mut run).unwrap_err();
    assert_eq!(run.receipts.last().unwrap().state, RemoteState::Commented);
    assert_eq!(run.phase, Phase::Published);
    assert_ne!(run.operation.state, OperationState::Completed);
    run.retry(false, 2000).unwrap();
    fixture.now = 2000;
    fixture.run(&mut run).unwrap();
    assert_eq!(run.operation.state, OperationState::Completed);
    assert_eq!(fixture.visible(), 1);
    assert_eq!(fixture.writes(), 2);
}

#[test]
fn crash_after_durable_intent_before_request_remains_honestly_unresolved() {
    let mut fixture = Fixture::new();
    let mut run = publication();
    run.batch = Some(Batch::prepare(&run, &context(pull())).unwrap());
    run.operation.begin_attempt(100).unwrap();
    run.mutation = Some(Mutation::Create);
    run.operation.attempted_mutation = Some("\"create\"".into());
    run.uncertain = true;
    fixture
        .store
        .save_publications(std::slice::from_ref(&run))
        .unwrap();
    run = fixture.resume();
    fixture.run(&mut run).unwrap_err();
    assert_eq!(run.phase, Phase::Unresolved);
    assert_eq!(fixture.writes(), 0);
    assert_eq!(fixture.visible(), 0);
}

#[test]
fn head_movement_during_context_preparation_queues_only_an_eligible_new_head() {
    let mut fixture = Fixture::new();
    fixture.move_head_during_prepare = true;
    let mut run = publication();
    fixture.run(&mut run).unwrap();
    assert_eq!(run.phase, Phase::Stale);
    assert_eq!(fixture.requeues, 1);
    assert_eq!(fixture.writes(), 0);
}

#[test]
fn legacy_results_need_current_evidence_but_forks_need_no_revision_consent() {
    let mut old = review();
    old.result.as_mut().unwrap().reviewed_base_sha = None;
    assert!(Publication::new(old, false, true, 100).is_err());
    assert!(Publication::new(review(), false, false, 100).is_err());
    let mut fixture = Fixture::new();
    fixture.pull.head_repository_id = Some("99".into());
    let mut run = publication();
    run.review.trust_confirmed = false;
    fixture.run(&mut run).unwrap();
    assert!(fixture.writes() > 0);
    assert_eq!(run.phase, Phase::Published);
}

#[test]
fn remote_batch_tampering_or_foreign_pending_review_never_gets_submitted() {
    for tamper in ["body", "comment", "commit", "account", "foreign"] {
        let mut fixture = Fixture::new();
        fixture.wire.0.lock().unwrap().fault = Some((Mutation::Create, Fault::After));
        let mut run = publication();
        fixture.run(&mut run).unwrap_err();
        {
            let mut server = fixture.wire.0.lock().unwrap();
            match tamper {
                "body" => server.reviews[0]["body"] = json!("Different body"),
                "comment" => {
                    server.comments.get_mut(&1).unwrap()[0]["body"] = json!("Human added content")
                }
                "commit" => server.reviews[0]["commit_id"] = json!("c".repeat(40)),
                "account" => server.reviews[0]["user"]["id"] = json!(99),
                "foreign" => server.reviews[0]["body"] = json!("Human pending review"),
                _ => unreachable!(),
            }
        }
        run = fixture.resume();
        assert!(fixture.run(&mut run).is_err());
        assert_eq!(fixture.visible(), 0);
        assert_eq!(fixture.writes(), 1);
    }
}

#[test]
fn provider_reviews_and_comment_receipts_exhaust_all_pages() {
    let mut fixture = Fixture::new();
    fixture
        .wire
        .0
        .lock()
        .unwrap()
        .reviews
        .extend((1000..1101).map(
        |id| json!({"id":id,"user":{"id":33},"body":"Someone else's review","state":"COMMENTED"}),
    ));
    let mut run = publication();
    let output = &mut run.review.result.as_mut().unwrap().output;
    let finding = output.findings[0].clone();
    output.findings = (0..101)
        .map(|index| {
            let mut finding = finding.clone();
            finding.title = format!("Finding {index}");
            finding
        })
        .collect();
    fixture.run(&mut run).unwrap();
    assert_eq!(fixture.visible(), 102);
    assert_eq!(run.receipts.last().unwrap().comment_ids.len(), 101);
    assert_eq!(fixture.writes(), 2);
}

#[test]
fn deadlines_rate_limits_and_retry_identity_survive_restart() {
    let mut fixture = Fixture::new();
    fixture.wire.0.lock().unwrap().fault = Some((Mutation::Create, Fault::RateLimit));
    let mut run = publication();
    fixture.run(&mut run).unwrap_err();
    let id = run.operation.id.clone();
    assert!(!run.uncertain);
    assert_eq!(run.operation.next_attempt_at, Some(117));
    run = fixture.resume();
    fixture.run(&mut run).unwrap();
    assert_eq!(run.operation.id, id);
    assert_eq!(run.operation.attempt_count, 2);
    assert_eq!(fixture.visible(), 1);

    let mut expired = publication();
    fixture.now = expired.operation.retry_deadline;
    assert!(fixture.run(&mut expired).is_err());
    assert_eq!(expired.operation.state, OperationState::ManualRetry);
    assert_eq!(expired.operation.attempt_count, 0);
    assert_eq!(fixture.writes(), 3);
}

#[test]
fn another_local_review_attempt_cannot_duplicate_or_bypass_uncertain_publication() {
    let original = publication();
    let mut newer_review = original.review.clone();
    newer_review.operation.id = "explicit-rereview".into();
    assert!(original.conflicts_with(&newer_review));
    let mut run = original.clone();
    run.operation.state = OperationState::Failed;
    assert!(!run.conflicts_with(&newer_review));
    run.uncertain = true;
    assert!(run.conflicts_with(&newer_review));
    run.uncertain = false;
    for state in [
        RemoteState::Pending,
        RemoteState::Commented,
        RemoteState::Deleted,
    ] {
        run.receipts = vec![Receipt {
            review_id: "42".into(),
            state: state.clone(),
            comment_ids: vec![],
        }];
        assert_eq!(
            run.conflicts_with(&newer_review),
            state != RemoteState::Deleted
        );
    }
    newer_review.key.push_str("new-head");
    assert!(!original.conflicts_with(&newer_review));
}

#[test]
fn reviewer_only_eligibility_and_binding_loss_stop_publication_without_requeue() {
    for change in ["reviewer", "account", "repository", "prompt", "scope"] {
        let mut fixture = Fixture::new();
        fixture.settings.repositories[0].watched_authors[0].id = "99".into();
        fixture.job.trigger_policy = "[[\"99\"],true]".into();
        fixture.job.watched_author = false;
        fixture.job.requested_reviewer = true;
        fixture.pull.requested_reviewers = vec![pr_sniper_lib::github::Identity {
            id: "22".into(),
            login: "owner".into(),
        }];
        let mut review = review();
        review.job = fixture.job.clone();
        review.key = pr_sniper_lib::review::key(&review.job, "assignment");
        review.selection =
            Selection::resolve(&fixture.settings, &review.job, "assignment").unwrap();
        let mut run = Publication::new(review, false, true, 100).unwrap();
        assert!(fixture.inspect(&run).unwrap().stop.is_none());
        match change {
            "reviewer" => fixture.pull.requested_reviewers.clear(),
            "account" => fixture.settings.repositories[0].provider_account_id = Some("33".into()),
            "repository" => {
                fixture.settings.repositories[0].provider_repository_id = Some("999".into())
            }
            "prompt" => fixture.settings.agents[0].prompt = "Different lens.".into(),
            "scope" => fixture.active = false,
            _ => unreachable!(),
        }
        fixture.run(&mut run).unwrap();
        assert_eq!(fixture.writes(), 0, "{change}");
        assert_eq!(fixture.requeues, 0, "{change}");
        assert_eq!(run.phase, Phase::Stopped, "{change}");
    }
}
