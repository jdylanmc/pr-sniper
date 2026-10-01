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
