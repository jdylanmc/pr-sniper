use super::*;
use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::{Response, Transport},
    publication::{MutationTransport, Request},
    review::ReviewContext,
    ConnectionError, Identity,
};
use crate::review::{ReviewRun, Selection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn fixture() -> (tempfile::TempDir, Store, ReviewRun) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let repository_id = uuid::Uuid::new_v4().to_string();
    let assignment_id = uuid::Uuid::new_v4().to_string();
    let agent_id = uuid::Uuid::new_v4().to_string();
    let mut settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":repository_id,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":assignment_id,"agent_id":agent_id,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":true}]}],
        "agents":[{"id":agent_id,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"machine"}]
    })).unwrap();
    settings.defaults.automatic_comment_publication = true;
    store.save_settings(&settings).unwrap();
    let job: QueueJob = serde_json::from_value(json!({
        "assignment_id":assignment_id,"provider":"github","account_id":"22","account_login":"owner",
        "configuration_id":repository_id,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"9","number":1,"title":"Review","head_sha":"a".repeat(40),
        "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
        "watched_author":true,"requested_reviewer":false,"waiting":"human_start","detected_at":100
    }))
    .unwrap();
    store.save_queue(std::slice::from_ref(&job)).unwrap();
    let mut operation = JobOperation::review(&job, 100);
    operation.state = OperationState::Completed;
    let review = ReviewRun {
        feedback_context: None,
        key: crate::review::key(&job,&assignment_id),
        selection: Selection::resolve(&settings,&job,&assignment_id).unwrap(),
        job, assignment_id, operation, manual_start:true,
        trust_confirmed:true, phase:"Complete".into(), error:None,
        result:Some(serde_json::from_value(json!({
            "reviewed_base_sha":"b".repeat(40),
            "output":{"synopsis":"No changes need attention.","files":[],"findings":[],"decision":"machine_sign_off"},
            "session_id":"session","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":0
        })).unwrap()),
    };
    store.save_reviews(std::slice::from_ref(&review)).unwrap();
    (root, store, review)
}

#[test]
fn normal_publication_gate_refusals_remain_item_local() {
    for policy in [
        "pause",
        "human_closed_conflict",
        "human_closed_duplicate",
        "stale_feedback",
    ] {
        let (_root, store, review) = fixture();
        let mut run = Publication::new(review, true, false, 100).unwrap();
        let failures = FailureRouting::default();
        let context = crate::feedback::Context {
            id: "root".into(),
            publication_id: "prior-publication".into(),
            owner_agent_id: run.review.selection.agent.id.clone(),
            owner_assignment_id: run.review.assignment_id.clone(),
            original_head: run.review.job.head_sha.clone(),
            root_id: "100".into(),
            path: "source.rs".into(),
            title: "Defect".into(),
            body: "Evidence".into(),
            thread: None,
            closed: true,
            unavailable: None,
        };
        match policy {
            "pause" => store
                .save_automation(&crate::capacity::Automation { paused: true })
                .unwrap(),
            "human_closed_conflict" => {
                run.review.result.as_mut().unwrap().output.feedback_conflict = true
            }
            "human_closed_duplicate" => {
                run.review.result.as_mut().unwrap().output.findings.push(serde_json::from_value(json!({
                    "path":"source.rs","side":"head","line":1,"severity":"high","title":"Defect","explanation":"Evidence","confidence":90
                })).unwrap());
                let mut ledger = store.load_feedback().unwrap();
                ledger.records.push(crate::feedback::Record {
                    context,
                    job: run.review.job.clone(),
                    observed_head: run.review.job.head_sha.clone(),
                });
                store.save_feedback(&ledger).unwrap();
            }
            "stale_feedback" => run.review.feedback_context = Some(vec![context]),
            _ => unreachable!(),
        }
        let error = if policy == "pause" {
            failures.capacity_gate(&store)
        } else {
            failures.feedback_gate(&store, &run.review)
        }
        .unwrap_err();
        assert!(
            !failures.infrastructure_failed.load(Ordering::SeqCst),
            "{policy}"
        );
        run.error = Some(error.message.clone());
        save_publication(&store, &mut run, &failures).unwrap();
        assert_eq!(failures.host_warning(&store, &run), None, "{policy}");
        assert_eq!(
            store.load_publications().unwrap()[0].error,
            Some(error.message)
        );
    }
}

#[test]
fn explicit_connection_configuration_failure_marks_host_not_provider_refusal() {
    for (error, infrastructure) in [
        (crate::github::ConnectionError::Configuration, true),
        (crate::github::ConnectionError::SignedOut, false),
        (crate::github::ConnectionError::MissingScope, false),
    ] {
        let failures = FailureRouting::default();
        let _failure = failures.connection_error(error);
        assert_eq!(
            failures.infrastructure_failed.load(Ordering::SeqCst),
            infrastructure
        );
    }
}

#[test]
fn nested_gate_storage_failures_keep_host_warning_after_successful_item_save() {
    struct NestedFailure<'a> {
        store: &'a Store,
        path: std::path::PathBuf,
        failures: FailureRouting,
        feedback: bool,
        final_gate: bool,
        cause: Option<String>,
    }
    impl NestedFailure<'_> {
        fn fail_gate(&mut self, run: &Publication) -> Result<(), Failure> {
            let previous = self.path.with_extension("saved");
            let exists = self.path.exists();
            if exists {
                std::fs::rename(&self.path, &previous).unwrap();
            }
            std::fs::create_dir(&self.path).unwrap();
            let result = if self.feedback {
                self.failures.feedback_gate(self.store, &run.review)
            } else {
                self.failures.capacity_gate(self.store)
            };
            std::fs::remove_dir(&self.path).unwrap();
            if exists {
                std::fs::rename(previous, &self.path).unwrap();
            }
            self.cause = Some(result.as_ref().unwrap_err().message.clone());
            result
        }
    }
    impl Environment for NestedFailure<'_> {
        fn now(&self) -> Result<i64, Failure> {
            Ok(101)
        }
        fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
            save_publication(self.store, run, &self.failures)
        }
        fn inspect(&mut self, run: &Publication) -> Result<Gate, Failure> {
            let stop = if self.final_gate {
                None
            } else {
                self.fail_gate(run).err().map(|error| error.message)
            };
            Ok(Gate {
                stop,
                stale: false,
                requeue: false,
            })
        }
        fn prepare(&mut self, _: &Publication) -> Result<Batch, Failure> {
            unreachable!()
        }
        fn reconcile(&mut self, _: &Publication) -> Result<Option<Receipt>, Failure> {
            Ok(None)
        }
        fn mutate(&mut self, run: &Publication, _: Mutation) -> Result<Receipt, WriteFailure> {
            let failure = self.fail_gate(run).unwrap_err();
            Err(WriteFailure {
                failure,
                uncertain: false,
            })
        }
        fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
            panic!("No requeue")
        }
    }
    for (filename, feedback, final_gate) in [
        ("automation.json", false, false),
        ("feedback.json", true, false),
        ("publications.json", true, false),
        ("retention.json", true, false),
        ("automation.json", false, true),
    ] {
        let (root, store, review) = fixture();
        let mut run = Publication::new(review, true, false, 100).unwrap();
        run.review.feedback_context = Some(vec![]);
        run.batch = Some(
            Batch::prepare(
                &run,
                &ReviewContext {
                    pull: pull(&run.review),
                    base_revision: "b".repeat(40),
                    files: vec![],
                    head: BTreeMap::new(),
                    base: BTreeMap::new(),
                },
            )
            .unwrap(),
        );
        let mut environment = NestedFailure {
            store: &store,
            path: root.path().join("state").join(filename),
            failures: FailureRouting::default(),
            feedback,
            final_gate,
            cause: None,
        };
        let outcome = execute(&mut environment, &mut run);
        assert_eq!(outcome.is_ok(), !final_gate, "{filename}");
        assert_eq!(
            store.load_publications().unwrap()[0].error,
            environment.cause
        );
        assert_eq!(
            environment.failures.host_warning(&store, &run),
            Some(INFRASTRUCTURE_WARNING),
            "{filename}"
        );
        assert!(run.receipts.is_empty());
        assert!(!run.uncertain);
    }
}

#[test]
fn recovered_storage_write_failure_still_requires_host_warning() {
    struct StorageFailure<'a> {
        store: &'a Store,
        obstruction: std::path::PathBuf,
        saves: usize,
        fail_at: usize,
        persistent: bool,
        failures: FailureRouting,
    }
    impl Environment for StorageFailure<'_> {
        fn now(&self) -> Result<i64, Failure> {
            Ok(101)
        }
        fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
            self.saves += 1;
            if self.saves == self.fail_at || (self.persistent && self.saves > self.fail_at) {
                let previous = self.obstruction.with_extension("saved");
                let exists = self.obstruction.exists();
                if exists {
                    std::fs::rename(&self.obstruction, &previous).unwrap();
                }
                std::fs::create_dir(&self.obstruction).unwrap();
                let result = self.failures.persist(self.store, std::slice::from_ref(run));
                std::fs::remove_dir(&self.obstruction).unwrap();
                if exists {
                    std::fs::rename(previous, &self.obstruction).unwrap();
                }
                assert!(result.is_err());
                return result;
            }
            self.failures.persist(self.store, std::slice::from_ref(run))
        }
        fn inspect(&mut self, _: &Publication) -> Result<Gate, Failure> {
            Ok(Gate {
                stop: None,
                stale: false,
                requeue: false,
            })
        }
        fn prepare(&mut self, run: &Publication) -> Result<Batch, Failure> {
            Batch::prepare(
                run,
                &ReviewContext {
                    pull: pull(&run.review),
                    base_revision: "b".repeat(40),
                    files: vec![],
                    head: BTreeMap::new(),
                    base: BTreeMap::new(),
                },
            )
        }
        fn reconcile(&mut self, _: &Publication) -> Result<Option<Receipt>, Failure> {
            unreachable!()
        }
        fn mutate(&mut self, _: &Publication, _: Mutation) -> Result<Receipt, WriteFailure> {
            panic!("No provider writes")
        }
        fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
            unreachable!()
        }
    }
    for (fail_at, persistent) in [(1, false), (2, false), (1, true), (2, true)] {
        let (root, store, review) = fixture();
        let mut run = Publication::new(review, true, false, 100).unwrap();
        let mut environment = StorageFailure {
            store: &store,
            obstruction: root.path().join("state/publications.json"),
            saves: 0,
            fail_at,
            persistent,
            failures: FailureRouting::default(),
        };
        let error = execute(&mut environment, &mut run).unwrap_err();
        assert_eq!(environment.saves, fail_at + 1);
        if !persistent {
            assert_eq!(
                store.load_publications().unwrap()[0].error,
                Some(error.message.clone())
            );
        }
        assert_eq!(
            environment.failures.host_warning(&store, &run),
            Some(INFRASTRUCTURE_WARNING)
        );
        assert!(run.receipts.is_empty());
        assert!(!run.uncertain);
    }
}

#[test]
fn persisted_publication_failure_is_item_local_not_a_settings_banner() {
    let (_root, store, review) = fixture();
    let mut run = Publication::new(review, true, false, 100).unwrap();
    let error = Failure::permanent("Publication verify_pending: remote comment body differs. Inspect the PR on GitHub before retrying.");
    run.error = Some(error.message.clone());
    run.operation.state = OperationState::Failed;
    let failures = FailureRouting::default();
    assert_eq!(
        failures.host_warning(&store, &run),
        Some(INFRASTRUCTURE_WARNING)
    );
    store.save_publications(std::slice::from_ref(&run)).unwrap();
    assert_eq!(failures.host_warning(&store, &run), None);
    let candidate = candidates(&store).unwrap().remove(0);
    assert_eq!(candidate.publication.as_ref().unwrap().error, run.error);
    assert_eq!(candidate.publication.unwrap().review.job.number, 1);
    failures.infrastructure_error("Storage unavailable.");
    assert_eq!(
        failures.host_warning(&store, &run),
        Some(INFRASTRUCTURE_WARNING)
    );
    let failures = FailureRouting::default();
    for different_operation in [false, true] {
        let mut changed = run.clone();
        if different_operation {
            changed.operation.id = "different-operation".into();
        } else {
            changed.operation.attempt_count += 1;
        }
        assert_eq!(
            failures.host_warning(&store, &changed),
            Some(INFRASTRUCTURE_WARNING)
        );
    }
}

#[test]
fn executed_provider_mismatch_remains_item_local_without_infrastructure_warning() {
    struct ProviderMismatch<'a> {
        store: &'a Store,
        failures: FailureRouting,
    }
    impl Environment for ProviderMismatch<'_> {
        fn now(&self) -> Result<i64, Failure> {
            Ok(101)
        }
        fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
            save_publication(self.store, run, &self.failures)
        }
        fn inspect(&mut self, _: &Publication) -> Result<Gate, Failure> {
            Ok(Gate {
                stop: None,
                stale: false,
                requeue: false,
            })
        }
        fn prepare(&mut self, _: &Publication) -> Result<Batch, Failure> {
            unreachable!()
        }
        fn reconcile(&mut self, _: &Publication) -> Result<Option<Receipt>, Failure> {
            Err(Failure::permanent("Publication verify_pending: remote comment body does not match the frozen batch. Inspect the PR before retrying."))
        }
        fn mutate(&mut self, _: &Publication, _: Mutation) -> Result<Receipt, WriteFailure> {
            panic!("No provider writes for a mismatch")
        }
        fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
            unreachable!()
        }
    }
    let (_root, store, review) = fixture();
    let mut run = retained_publication(review, "42");
    let mut environment = ProviderMismatch {
        store: &store,
        failures: FailureRouting::default(),
    };
    let error = execute(&mut environment, &mut run).unwrap_err();
    assert_eq!(environment.failures.host_warning(&store, &run), None);
    assert_eq!(
        store.load_publications().unwrap()[0].error,
        Some(error.message)
    );
    assert_eq!(run.receipts[0].review_id, "42");
    assert_eq!(run.receipts[0].state, RemoteState::Pending);
    assert!(run.uncertain);
}

#[test]
fn unreadable_publication_evidence_requires_host_warning() {
    let (root, store, review) = fixture();
    let mut run = Publication::new(review, true, false, 100).unwrap();
    run.error = Some("Publication verification failed.".into());
    let path = root.path().join("state/publications.json");
    std::fs::create_dir(&path).unwrap();
    let failures = FailureRouting::default();
    assert_eq!(
        failures.host_warning(&store, &run),
        Some(INFRASTRUCTURE_WARNING)
    );
    assert!(save_publication(&store, &mut run, &failures).is_err());
    std::fs::remove_dir(path).unwrap();
    save_publication(&store, &mut run, &failures).unwrap();
    assert_eq!(
        failures.host_warning(&store, &run),
        Some(INFRASTRUCTURE_WARNING)
    );
    assert_eq!(store.load_publications().unwrap()[0].error, run.error);
}

#[test]
fn newer_local_attempt_blocks_automatic_publication_of_the_older_result() {
    let (_root, store, review) = fixture();
    assert!(candidates(&store).unwrap()[0].automatic);
    let mut newer = review.clone();
    newer.operation.id = "new-attempt".into();
    newer.operation.state = OperationState::Running;
    newer.result = None;
    store.save_reviews(&[review, newer]).unwrap();
    let candidates = candidates(&store).unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(!candidates[0].automatic);
    assert!(candidates[0]
        .blocked
        .as_ref()
        .unwrap()
        .contains("newer local review"));
}

#[test]
fn paused_host_neither_launches_new_batches_nor_resets_pending_reconciliation() {
    let (_root, store, review) = fixture();
    let mut publication = Publication::new(review.clone(), true, false, 100).unwrap();
    publication.uncertain = true;
    publication.operation.attempt_count = 1;
    publication.operation.next_attempt_at = Some(120);
    store
        .save_publications(std::slice::from_ref(&publication))
        .unwrap();
    store
        .save_automation(&crate::capacity::Automation { paused: true })
        .unwrap();
    assert!(next_candidate(&store, 121).unwrap().is_none());
    assert!(prepare_launch(&store, &review.operation.id, true, 121).is_err());
    assert_eq!(
        store.load_publications().unwrap(),
        vec![publication.clone()]
    );
    store
        .save_automation(&crate::capacity::Automation { paused: false })
        .unwrap();
    assert!(next_candidate(&store, 121).unwrap().is_some());
    assert_eq!(
        prepare_launch(&store, &review.operation.id, false, 121).unwrap(),
        publication
    );
}

#[test]
fn uncertain_publication_reserves_revision_even_after_another_result_appears() {
    let (_root, store, review) = fixture();
    let mut publication = Publication::new(review.clone(), true, false, 100).unwrap();
    publication.uncertain = true;
    publication.operation.state = OperationState::ManualRetry;
    store
        .save_publications(std::slice::from_ref(&publication))
        .unwrap();
    let mut newer = review.clone();
    newer.operation.id = "new-attempt".into();
    store.save_reviews(&[review, newer]).unwrap();
    let candidates = candidates(&store).unwrap();
    assert!(candidates[1]
        .blocked
        .as_ref()
        .unwrap()
        .contains("owns this revision"));
    assert!(publication.reserves_revision());
    assert_eq!(
        candidates[0].publication.as_ref().unwrap().id,
        publication.id
    );
    assert!(prepare_launch(&store, "new-attempt", false, 101)
        .unwrap_err()
        .contains("owns this revision"));
    assert_eq!(store.load_publications().unwrap(), vec![publication]);
}

#[test]
fn retained_origin_controls_launch_conflicts_even_if_ordinary_evidence_differs() {
    for conflict in [false, true] {
        let (_root, store, review) = fixture();
        let retained = Publication::new(review.clone(), true, false, 100).unwrap();
        let mut other = review.clone();
        other.operation.id = "another-review".into();
        if !conflict {
            other.key = "another-revision".into();
        }
        let other = Publication::new(other, true, false, 100).unwrap();
        let mut ordinary = review;
        ordinary.key = if conflict {
            "another-revision".into()
        } else {
            other.review.key.clone()
        };
        store.save_reviews(&[ordinary]).unwrap();
        let publications = vec![retained.clone(), other];
        store.save_publications(&publications).unwrap();
        let launch = prepare_launch(&store, &retained.review.operation.id, false, 101);
        if conflict {
            assert!(launch.unwrap_err().contains("owns this revision"));
        } else {
            assert_eq!(launch.unwrap(), retained);
        }
        assert_eq!(store.load_publications().unwrap(), publications);
    }
}

fn pull(review: &ReviewRun) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: review.job.pull_request_id.clone(),
        number: review.job.number,
        title: review.job.title.clone(),
        author: Some(Identity {
            id: review.job.author_id.clone().unwrap(),
            login: review.job.author_login.clone().unwrap(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: review.job.head_sha.clone(),
        base_sha: "b".repeat(40),
        head_repository_id: Some(review.job.repository_id.clone()),
        base_repository_id: review.job.repository_id.clone(),
        updated_at: "2026-09-30T00:00:00Z".into(),
        files: vec![],
    }
}

fn retained_publication(review: ReviewRun, remote_id: &str) -> Publication {
    let context = ReviewContext {
        pull: pull(&review),
        base_revision: "b".repeat(40),
        files: vec![],
        head: BTreeMap::new(),
        base: BTreeMap::new(),
    };
    let mut run = Publication::new(review, true, false, 100).unwrap();
    run.batch = Some(Batch::prepare(&run, &context).unwrap());
    run.accept(Receipt {
        review_id: remote_id.into(),
        state: RemoteState::Pending,
        comment_ids: vec![],
    })
    .unwrap();
    run.operation.attempt_count = 1;
    run.operation.attempted_mutation = Some("\"submit\"".into());
    run.operation.next_attempt_at = Some(101);
    run.mutation = Some(Mutation::Submit);
    run.uncertain = true;
    run
}

struct Wire {
    remote: Value,
    submissions: std::sync::atomic::AtomicUsize,
}

fn response(value: Value) -> Response {
    Response {
        status: 200,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}

impl Transport for &Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let base = self.remote["pull_request_url"]
            .as_str()
            .unwrap()
            .strip_prefix("https://api.github.com")
            .unwrap();
        if path == format!("{base}/reviews?per_page=100&page=1") {
            Ok(response(json!([self.remote])))
        } else {
            assert_eq!(
                path,
                format!(
                    "{base}/reviews/{}/comments?per_page=100&page=1",
                    self.remote["id"]
                )
            );
            Ok(response(json!([])))
        }
    }
}

impl MutationTransport for &Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        let base = self.remote["pull_request_url"]
            .as_str()
            .unwrap()
            .strip_prefix("https://api.github.com")
            .unwrap();
        assert_eq!(path, format!("{base}/reviews/{}/events", self.remote["id"]));
        let Request::Post(body) = request else {
            panic!("Recovery must not discard the original batch.");
        };
        assert_eq!(body, json!({"event":"COMMENT","body":self.remote["body"]}));
        self.submissions.fetch_add(1, Ordering::SeqCst);
        let mut remote = self.remote.clone();
        remote["state"] = json!("COMMENTED");
        remote["submitted_at"] = json!("2026-09-30T00:00:00Z");
        Ok(response(remote))
    }
}

struct Recovery<'a> {
    store: &'a Store,
    wire: Wire,
    reconciliations: usize,
}

impl Environment for Recovery<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(101)
    }

    fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
        let mut runs = self.store.load_publications().map_err(Failure::permanent)?;
        replace(&mut runs, run);
        self.store
            .save_publications(&runs)
            .map_err(Failure::permanent)
    }

    fn inspect(&mut self, run: &Publication) -> Result<Gate, Failure> {
        let settings = self.store.load_settings().map_err(Failure::permanent)?;
        let jobs = self.store.load_queue().map_err(Failure::permanent)?;
        Ok(evaluate_gate(
            &settings,
            run,
            jobs.iter().find(|job| run.review.matches_job(job)),
            &pull(&run.review),
            true,
            true,
            false,
        ))
    }

    fn prepare(&mut self, _: &Publication) -> Result<Batch, Failure> {
        panic!("Recovery must reuse the frozen batch.");
    }

    fn reconcile(&mut self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        self.reconciliations += 1;
        GithubClient::new(&self.wire).reconcile_publication(run)
    }

    fn mutate(&mut self, run: &Publication, mutation: Mutation) -> Result<Receipt, WriteFailure> {
        assert_eq!(mutation, Mutation::Submit);
        GithubClient::new(&self.wire).mutate_publication(run, mutation)
    }

    fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
        panic!("Recovery must not replace the original review.");
    }
}

#[test]
fn recovered_publications_resume_original_operations_and_release_the_next_candidate() {
    for interrupted in [false, true] {
        let (root, store, mut review) = fixture();
        review.job.work = Some(
            serde_json::from_value(json!({
                "id":"normal-pass", "item_id":"iteration-item", "iteration_id":"iteration",
                "iteration":1, "agent_id":review.selection.agent.id,
                "enqueue_order":7, "pass_ordinal":1, "trigger":"admission",
                "admission":{"watched_author":true,"all_authors":false,"requested_reviewer":false}
            }))
            .unwrap(),
        );
        review.key = crate::review::key(&review.job, &review.assignment_id);
        let mut first = retained_publication(review.clone(), "41");
        if interrupted {
            first.operation.state = OperationState::Running;
            first.operation.next_attempt_at = None;
        }
        let mut later = review;
        later.job.pull_request_id = "10".into();
        later.job.number = 2;
        later.job.work = None;
        later.key = crate::review::key(&later.job, &later.assignment_id);
        later.operation = JobOperation::review(&later.job, 100);
        later.operation.state = OperationState::Completed;
        let second = retained_publication(later, "42");
        store
            .save_publications(&[first.clone(), second.clone()])
            .unwrap();
        std::fs::remove_file(root.path().join("state/reviews.json")).unwrap();
        std::fs::remove_file(root.path().join("state/queue.json")).unwrap();
        restore(&store).unwrap();
        let mut queue = store.load_queue_state().unwrap();
        queue.recover_evidence(&store).unwrap();
        store.save_queue_state(&queue).unwrap();
        assert_eq!(
            queue.jobs,
            vec![first.review.job.clone(), second.review.job.clone()]
        );
        assert!(store.load_reviews().unwrap().is_empty());
        if interrupted {
            first.operation.state = OperationState::Interrupted;
            first.operation.next_attempt_at = Some(0);
        }
        for expected in [&first, &second] {
            let selected = next_candidate(&store, 101).unwrap().unwrap();
            assert_eq!(selected.review_operation_id, expected.review.operation.id);
            let mut run =
                prepare_launch(&store, &selected.review_operation_id, false, 101).unwrap();
            assert_eq!(
                &run, expected,
                "Launch must preserve all retained evidence."
            );
            let batch = run.batch.as_ref().unwrap();
            let mut recovery = Recovery {
                store: &store,
                wire: Wire {
                    remote: json!({
                        "id":run.operation.pending_review_id.as_ref().unwrap().parse::<u64>().unwrap(),
                        "user":{"id":22}, "body":batch.body, "commit_id":batch.commit_id,
                        "state":if interrupted { "COMMENTED" } else { "PENDING" },
                        "submitted_at":if interrupted { Some("2026-09-30T00:00:00Z") } else { None },
                        "pull_request_url":format!("https://api.github.com/repos/example/repo/pulls/{}", run.review.job.number)
                    }),
                    submissions: Default::default(),
                },
                reconciliations: 0,
            };
            execute(&mut recovery, &mut run).unwrap();
            assert!(recovery.reconciliations > 0);
            assert_eq!(
                recovery.wire.submissions.load(Ordering::SeqCst),
                usize::from(!interrupted)
            );
            assert_eq!(run.operation.state, OperationState::Completed);
            assert_eq!(run.phase, Phase::Published);
            assert_eq!(run.id, expected.id);
            assert_eq!(run.review, expected.review);
            assert_eq!(run.operation.id, expected.operation.id);
            assert_eq!(
                run.operation.initial_attempt_at,
                expected.operation.initial_attempt_at
            );
            assert_eq!(
                run.operation.retry_deadline,
                expected.operation.retry_deadline
            );
            assert_eq!(
                run.operation.attempt_count,
                expected.operation.attempt_count + 1
            );
            assert_eq!(
                run.operation.pending_review_id,
                expected.operation.pending_review_id
            );
            assert_eq!(run.batch, expected.batch);
            assert_eq!(run.history, expected.history);
            assert!(run.receipts.starts_with(&expected.receipts));
            assert_eq!(run.receipts.last().unwrap().state, RemoteState::Commented);
            let saved = store.load_publications().unwrap();
            assert_eq!(saved.len(), 2);
            assert_eq!(saved.iter().find(|saved| saved.id == run.id), Some(&run));
            assert!(!root.path().join("state/reviews.json").exists());
            assert_eq!(store.load_queue_state().unwrap(), queue);
        }
        assert!(next_candidate(&store, 101).unwrap().is_none());
    }
}
