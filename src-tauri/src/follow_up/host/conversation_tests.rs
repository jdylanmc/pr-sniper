use super::*;
use crate::{
    capacity::{Coordinator as Capacity, Dispatch, Kind},
    feedback::{Assessment, Disposition, MentionBinding},
    github::{
        conversation::TopComment,
        metadata::{Lifecycle, PullRequest},
        provider::{Capabilities, Connection, Response, Transport},
        publication::{MutationTransport, Request as HttpRequest},
        review::{ReviewContext, ReviewFile, TreeEntry},
        threads::Comment,
        Identity,
    },
    monitoring::{ActivationMode, Monitor, MonitoringActivation, PollResult},
    publication::{self, Batch, InlineComment, Receipt, RemoteState},
    review::{
        runtime::{FullReview, Task},
        ReviewRun,
    },
    storage::Settings,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const NOW: i64 = 1_800_000_000;

fn pull(head: char) -> PullRequest {
    PullRequest {
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
        head_sha: head.to_string().repeat(40),
        base_sha: "b".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-09-30T00:00:00Z".into(),
        files: vec![],
    }
}

fn poll(store: &Store, head: char, now: i64) -> PollTicket {
    let mut monitor = Monitor::restore(store).unwrap();
    let ticket = monitor.prepare_checks(store, now, true).unwrap().remove(0);
    monitor
        .finish(
            store,
            ticket.clone(),
            Ok(PollResult {
                connection: Connection {
                    identity: Identity {
                        id: "22".into(),
                        login: "actor".into(),
                    },
                    repository: RemoteRepository {
                        id: "100".into(),
                        name: "example/repo".into(),
                    },
                    capabilities: Capabilities {
                        read: true,
                        comment: CommentCapability::Available,
                    },
                },
                pull_requests: vec![pull(head)],
            }),
            now + 1,
        )
        .unwrap();
    ticket
}

fn fixture(count: usize) -> (tempfile::TempDir, Store, Publication, Thread) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut settings: Settings = serde_json::from_value(json!({"launch_at_login":false,"doctrines":[],
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}]}]})).unwrap();
    settings.defaults.automatic_agent_start = true;
    for i in 1..=count {
        let agent = format!("aaaaaaaa-aaaa-4aaa-8aaa-{i:012}");
        settings.agents.push(serde_json::from_value(json!({"id":agent,"name":format!("Agent {i}"),"model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"machine"})).unwrap());
        settings.repositories[0].assignments.push(serde_json::from_value(json!({
            "id":format!("cccccccc-cccc-4ccc-8ccc-{i:012}"),"agent_id":agent,"schedule":settings.defaults.schedule,"comment":true
        })).unwrap());
    }
    store.save_settings(&settings).unwrap();
    let activation = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "scope".into(),
            repository_id: REPO.into(),
            name: activation.name,
            account_id: activation.account_id,
            provider_repository_id: activation.provider_repository_id,
            trigger_policy: activation.trigger_policy,
            creation_watermark: 0,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW - 1,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    poll(&store, 'a', NOW);
    let jobs = store.load_queue().unwrap();
    let reviews=jobs.iter().enumerate().map(|(i,job)| {
        let mut operation=JobOperation::review(job,NOW+2);operation.state=OperationState::Completed;
        ReviewRun {
            feedback_context:None,key:crate::review::key(job,job.assignment_id.as_ref().unwrap()),
            assignment_id:job.assignment_id.clone().unwrap(),job:job.clone(),
            selection:Selection::resolve(&settings,job,job.assignment_id.as_ref().unwrap()).unwrap(),
            operation,manual_start:false,trust_confirmed:true,phase:"Completed fixture".into(),error:None,
            result:Some(serde_json::from_value(json!({"reviewed_base_sha":"b".repeat(40),
                "output":{"synopsis":"Review completed.","files":[{"path":"source.rs","explanation":"Source.","order":1}],
                    "findings":if i==0 {json!([{"path":"source.rs","side":"head","line":1,"severity":"high",
                        "title":"Wrong value","explanation":"Check the returned value.","confidence":90}])}else{json!([])},
                    "decision":if i==0 {"human_input_required"}else{"machine_sign_off"}},
                "session_id":"fixture","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":1
            })).unwrap()),
        }
    }).collect::<Vec<_>>();
    store.save_reviews(&reviews).unwrap();
    let mut origin = Publication::new(reviews[0].clone(), false, true, NOW + 3).unwrap();
    origin.batch = Some(Batch {
        commit_id: "a".repeat(40),
        body: "Summary".into(),
        unmappable: vec![],
        comments: vec![InlineComment {
            path: "source.rs".into(),
            line: 1,
            side: "RIGHT".into(),
            body: "Wrong value: check the returned value.".into(),
        }],
    });
    origin.phase = publication::Phase::Published;
    origin.operation.state = OperationState::Completed;
    origin.receipts = vec![Receipt {
        review_id: "42".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["100".into()],
    }];
    store
        .save_publications(std::slice::from_ref(&origin))
        .unwrap();
    let thread = Thread {
        id: "thread".into(),
        resolved: false,
        can_reply: true,
        comments: vec![Comment {
            id: "100".into(),
            body: origin.batch.as_ref().unwrap().comments[0].body.clone(),
            author_id: Some("22".into()),
            author_login: Some("actor".into()),
            reply_to: None,
            review_id: Some("42".into()),
            original_commit: Some("a".repeat(40)),
            created_at: "2026-09-30T00:00:00Z".into(),
            published_at: "2026-09-30T00:00:00Z".into(),
        }],
    };
    (root, store, origin, thread)
}

fn explanation(thread: &mut Thread, author: &str) {
    thread.comments.push(Comment {
        id: "101".into(),
        body: "This function intentionally returns 42; the caller expects 42.".into(),
        author_id: Some(author.into()),
        author_login: Some("author".into()),
        reply_to: Some("100".into()),
        review_id: Some("42".into()),
        original_commit: Some("a".repeat(40)),
        created_at: "2026-09-30T00:01:00Z".into(),
        published_at: "2026-09-30T00:01:00Z".into(),
    });
}

fn mention(id: &str, body: &str) -> TopComment {
    TopComment {
        id: id.into(),
        body: body.into(),
        author_id: Some("22".into()),
        author_login: Some("actor".into()),
        created_at: "2026-09-30T00:01:00Z".into(),
        updated_at: "2026-09-30T00:01:00Z".into(),
    }
}

fn observe(
    store: &Store,
    origin: &Publication,
    thread: &Thread,
    head: char,
    comments: Vec<TopComment>,
    now: i64,
) {
    let ticket = poll(store, head, now);
    admit_scan(
        store,
        &ticket,
        Scan {
            retained: vec![],
            feedback: vec![Observed {
                origin: origin.clone(),
                head: head.to_string().repeat(40),
                threads: vec![thread.clone()],
            }],
            mentions: vec![(
                MentionBinding {
                    configuration_id: REPO.into(),
                    account_id: "22".into(),
                    account_login: "actor".into(),
                    repository_id: "100".into(),
                    repository_name: "example/repo".into(),
                    pull_request_id: "9".into(),
                    number: 1,
                },
                comments,
            )],
        },
        now + 2,
    )
    .unwrap();
}

struct Source;
impl Transport for Source {
    fn get(&self, path: &str) -> Result<Response, github::ConnectionError> {
        assert!(path.contains("/git/blobs/"));
        Ok(Response{status:200,headers:BTreeMap::new(),body:serde_json::to_vec(&json!({
            "sha":"d".repeat(40),"size":11,"encoding":"base64","content":STANDARD.encode("return 42;\n")
        })).unwrap()})
    }
}
fn source_context(head: char) -> ReviewContext {
    let tree = BTreeMap::from([(
        "source.rs".into(),
        TreeEntry {
            path: "source.rs".into(),
            mode: "100644".into(),
            sha: "d".repeat(40),
            kind: "blob".into(),
        },
    )]);
    ReviewContext {
        pull: pull(head),
        base_revision: "b".repeat(40),
        files: vec![ReviewFile {
            path: "source.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: None,
        }],
        head: tree.clone(),
        base: tree,
    }
}
fn assessment(id: &str, disposition: Disposition) -> Assessment {
    Assessment {
        feedback_id: id.into(),
        disposition,
        reason: "The current source and author's explanation establish the expected value.".into(),
        evidence: vec![Evidence {
            path: "source.rs".into(),
            side: "head".into(),
            line: 1,
            quote: "return 42;".into(),
        }],
    }
}
fn output_for(
    run: &FollowUp,
    decision: ReplyDecision,
    assessments: Vec<Assessment>,
) -> crate::review::ReviewResult<ReplyOutput> {
    let value = json!({"decision":decision,"body":if decision==ReplyDecision::Reply {"The function returns the expected 42."}else{""},
        "new_information":if decision==ReplyDecision::Reply {"The function returns the expected 42."}else{""},
        "reason":"Verified source.","evidence":if decision==ReplyDecision::Reply {json!([{"path":"source.rs","side":"head","line":1,"quote":"return 42;"}])}else{json!([])},
        "feedback_assessments":assessments});
    let task = ReplyTask {
        conversation: run.input(),
        trigger_id: run.trigger_id.clone(),
        feedback: run.context.feedback.clone(),
        owner_agent_id: run.context.selection.agent.id.clone(),
    };
    let output = task
        .validate(
            &value.to_string(),
            &source_context(run.context.job.head_sha.chars().next().unwrap()),
            &GithubClient::new(Source),
            "example/repo",
        )
        .unwrap();
    crate::review::ReviewResult {
        reviewed_base_sha: Some("b".repeat(40)),
        output,
        session_id: "fixture".into(),
        model: "model".into(),
        runtime_version: "fixture".into(),
        input_tokens: 1,
        output_tokens: 1,
        tool_calls: 1,
    }
}

struct ReviewPublication<'a> {
    store: &'a Store,
    now: i64,
    remote: Option<Receipt>,
}

impl publication::Environment for ReviewPublication<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(self.now)
    }
    fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
        let mut publications = self.store.load_publications().unwrap();
        publications.retain(|p| p.id != run.id);
        publications.push(run.clone());
        self.store
            .save_publications(&publications)
            .map_err(Failure::permanent)
    }
    fn inspect(&mut self, run: &Publication) -> Result<publication::Gate, Failure> {
        crate::feedback::publication_gate(self.store, &run.review).map_err(Failure::permanent)?;
        let settings = self.store.load_settings().unwrap();
        let jobs = self.store.load_queue().unwrap();
        Ok(publication::evaluate_gate(
            &settings,
            run,
            jobs.iter().find(|j| run.review.matches_job(j)),
            &pull(run.review.job.head_sha.chars().next().unwrap()),
            true,
            true,
            false,
        ))
    }
    fn prepare(&mut self, run: &Publication) -> Result<Batch, Failure> {
        let mut context = source_context(run.review.job.head_sha.chars().next().unwrap());
        context.files[0].patch = Some("@@ -1 +1 @@\n-return 0;\n+return 42;".into());
        Batch::prepare(run, &context)
    }
    fn reconcile(&mut self, _: &Publication) -> Result<Option<Receipt>, Failure> {
        Ok(self.remote.clone())
    }
    fn mutate(
        &mut self,
        run: &Publication,
        mutation: publication::Mutation,
    ) -> Result<Receipt, WriteFailure> {
        assert!(self.inspect(run).unwrap().stop.is_none());
        let receipt = Receipt {
            review_id: format!("review-{}", run.id),
            state: match mutation {
                publication::Mutation::Create => RemoteState::Pending,
                publication::Mutation::Submit => RemoteState::Commented,
                publication::Mutation::Discard => panic!("Unexpected discard"),
            },
            comment_ids: run
                .batch
                .as_ref()
                .unwrap()
                .comments
                .iter()
                .enumerate()
                .map(|(i, _)| format!("{}-{i}", run.id))
                .collect(),
        };
        self.remote = Some(receipt.clone());
        Ok(receipt)
    }
    fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
        panic!("Unexpected revision change")
    }
}

fn review_and_publish(store: &Store, output: Value, now: i64) -> Publication {
    let capacity = Capacity::default();
    let mut batch = capacity.dispatch(store, now).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let work = batch.dispatched.remove(0);
    let key = work.key();
    let Dispatch::Review(run, _) = work else {
        panic!("Normal review expected")
    };
    let task = FullReview {
        final_context: None,
        feedback: run.feedback_context.clone().unwrap(),
        owner_agent_id: run.selection.agent.id.clone(),
    };
    let output = task
        .validate(
            &output.to_string(),
            &source_context(run.job.head_sha.chars().next().unwrap()),
            &GithubClient::new(Source),
            "example/repo",
        )
        .unwrap();
    crate::review::validate_execution_selection(store, &run).unwrap();
    crate::review::host::complete(
        store,
        &run.operation.id,
        Ok(crate::review::ReviewResult {
            reviewed_base_sha: Some("b".repeat(40)),
            output,
            session_id: "fixture".into(),
            model: "model".into(),
            runtime_version: "fixture".into(),
            input_tokens: 1,
            output_tokens: 1,
            tool_calls: 1,
        }),
        now + 1,
    )
    .unwrap();
    capacity.release(&key, &run.operation.id).unwrap();
    let review = store
        .load_reviews()
        .unwrap()
        .into_iter()
        .find(|r| r.operation.id == run.operation.id)
        .unwrap();
    let candidate = publication::host::candidates(store)
        .unwrap()
        .into_iter()
        .find(|c| c.review_operation_id == review.operation.id)
        .unwrap();
    assert!(candidate.blocked.is_none(), "{:?}", candidate.blocked);
    let mut publication = Publication::new(review, false, true, now + 2).unwrap();
    publication::execute(
        &mut ReviewPublication {
            store,
            now: now + 3,
            remote: None,
        },
        &mut publication,
    )
    .unwrap();
    assert_eq!(publication.operation.state, OperationState::Completed);
    assert_eq!(publication.phase, publication::Phase::Published);
    assert!(publication.operation.confirmed_receipt.is_some());
    publication
}

fn two_published_iterations(
    new_concern: bool,
) -> (tempfile::TempDir, Store, Publication, Thread, Publication) {
    let (root, store, old, mut thread) = fixture(1);
    store.save_reviews(&[]).unwrap();
    store.save_publications(&[]).unwrap();
    let origin = review_and_publish(
        &store,
        serde_json::to_value(old.review.result.unwrap().output).unwrap(),
        NOW + 5,
    );
    thread.comments[0].id = origin.receipts.last().unwrap().comment_ids[0].clone();
    thread.comments[0].review_id = Some(origin.receipts.last().unwrap().review_id.clone());
    thread.comments[0].body = origin.batch.as_ref().unwrap().comments[0].body.clone();
    observe(&store, &origin, &thread, 'c', vec![], NOW + 10);
    let feedback = store.load_feedback().unwrap().records[0].context.clone();
    let next = review_and_publish(
        &store,
        json!({
            "synopsis":"Current revision reviewed.", "files":[{"path":"source.rs","explanation":"Source.","order":1}],
            "findings":if new_concern { json!([{"path":"source.rs","side":"head","line":1,"severity":"high",
                "title":"Another concern","explanation":"A distinct issue.","confidence":90}]) } else { json!([]) },
            "decision":if new_concern { "human_input_required" } else { "machine_sign_off" },
            "feedback_assessments":[assessment(&feedback.id, Disposition::Open)]
        }),
        NOW + 20,
    );
    assert_eq!(
        next.review.feedback_context.as_ref().unwrap(),
        &vec![feedback]
    );
    assert_ne!(
        origin.review.job.work.as_ref().unwrap().iteration_id,
        next.review.job.work.as_ref().unwrap().iteration_id
    );
    if !new_concern {
        assert_eq!(current_state(&store), crate::queue::State::WaitingForAuthor);
    }
    (root, store, origin, thread, next)
}

fn current_state(store: &Store) -> crate::queue::State {
    let snapshot =
        crate::queue::snapshot(store, Monitor::restore(store).unwrap().snapshot()).unwrap();
    snapshot
        .items
        .iter()
        .find(|i| {
            !matches!(
                i.job.waiting.as_str(),
                monitoring::WAITING_SUPERSEDED | monitoring::WAITING_NO_LONGER_CURRENT
            )
        })
        .unwrap()
        .state
}

#[test]
fn r73_completed_second_publication_allows_later_human_closure_without_rewriting_archive() {
    let (root, store, origin, mut thread, next) = two_published_iterations(false);
    let archive = std::fs::read(root.path().join("state/publications.json")).unwrap();
    let reviews = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    thread.resolved = true;
    observe(&store, &origin, &thread, 'c', vec![], NOW + 30);
    assert!(crate::feedback::publication_gate(&store, &next.review).is_err());
    assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        archive
    );
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        reviews
    );
}

#[test]
fn r73_completed_receipt_does_not_relax_pending_new_mutation_or_current_configuration_gates() {
    let (_root, store, origin, mut thread, next) = two_published_iterations(false);
    thread.resolved = true;
    observe(&store, &origin, &thread, 'c', vec![], NOW + 30);
    let saved = store.load_publications().unwrap();
    for state in [
        OperationState::Queued,
        OperationState::Running,
        OperationState::Interrupted,
        OperationState::ManualRetry,
    ] {
        let mut publications = saved.clone();
        let current = publications.iter_mut().find(|p| p.id == next.id).unwrap();
        current.operation.state = state;
        current.phase = publication::Phase::Pending;
        current.receipts.last_mut().unwrap().state = RemoteState::Pending;
        store.save_publications(&publications).unwrap();
        let candidate = publication::host::candidates(&store)
            .unwrap()
            .into_iter()
            .find(|c| c.review_operation_id == next.review.operation.id)
            .unwrap();
        assert!(candidate
            .blocked
            .as_deref()
            .unwrap()
            .contains("Prior feedback changed"));
        assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
    }
    let mut uncertain = saved.clone();
    uncertain
        .iter_mut()
        .find(|p| p.id == next.id)
        .unwrap()
        .uncertain = true;
    store.save_publications(&uncertain).unwrap();
    assert_eq!(current_state(&store), crate::queue::State::Failed);

    store.save_publications(&[origin]).unwrap();
    let candidate = publication::host::candidates(&store)
        .unwrap()
        .into_iter()
        .find(|c| c.review_operation_id == next.review.operation.id)
        .unwrap();
    assert!(candidate.publication.is_none());
    assert!(candidate
        .blocked
        .as_deref()
        .unwrap()
        .contains("Prior feedback changed"));
    assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);

    store.save_publications(&saved).unwrap();
    let settings = store.load_settings().unwrap();
    for change in ["assignment", "account", "lens", "disabled"] {
        let mut changed = settings.clone();
        match change {
            "assignment" => changed.repositories[0].assignments.clear(),
            "account" => changed.repositories[0].provider_account_id = Some("44".into()),
            "lens" => changed.agents[0].prompt.push_str(" Different lens."),
            "disabled" => changed.repositories[0].enabled = false,
            _ => unreachable!(),
        }
        store.save_settings(&changed).unwrap();
        assert_ne!(
            current_state(&store),
            crate::queue::State::MachineSignedOff,
            "{change}"
        );
    }
    store.save_settings(&settings).unwrap();
    let mut jobs = store.load_queue().unwrap();
    for job in jobs
        .iter_mut()
        .filter(|j| j.head_sha == next.review.job.head_sha)
    {
        job.observed_base_sha = Some("d".repeat(40));
    }
    store.save_queue(&jobs).unwrap();
    assert_eq!(
        current_state(&store),
        crate::queue::State::StaleAfterPublication
    );
}

#[test]
fn r73_closing_earlier_root_does_not_clear_new_second_iteration_concern() {
    let (_root, store, origin, mut thread, next) = two_published_iterations(true);
    let mut current_thread = thread.clone();
    current_thread.id = "second-thread".into();
    current_thread.comments[0].id = next.receipts.last().unwrap().comment_ids[0].clone();
    current_thread.comments[0].review_id = Some(next.receipts.last().unwrap().review_id.clone());
    current_thread.comments[0].original_commit = Some(next.review.job.head_sha.clone());
    current_thread.comments[0].body = next.batch.as_ref().unwrap().comments[0].body.clone();
    observe(&store, &next, &current_thread, 'c', vec![], NOW + 30);
    thread.resolved = true;
    observe(&store, &origin, &thread, 'c', vec![], NOW + 40);
    assert_eq!(current_state(&store), crate::queue::State::WaitingForAuthor);
    current_thread.resolved = true;
    observe(&store, &next, &current_thread, 'c', vec![], NOW + 50);
    assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
}

#[test]
fn r73_completed_second_publication_allows_validated_same_head_reply_clearance() {
    for disposition in [
        Disposition::Open,
        Disposition::Cleared,
        Disposition::HumanInputRequired,
    ] {
        let (root, store, origin, mut thread, next) = two_published_iterations(false);
        let archive = std::fs::read(root.path().join("state/publications.json")).unwrap();
        explanation(&mut thread, "22");
        thread.comments[1].reply_to = Some(thread.comments[0].id.clone());
        thread.comments[1].review_id = thread.comments[0].review_id.clone();
        observe(&store, &origin, &thread, 'c', vec![], NOW + 30);
        assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
        let capacity = Capacity::default();
        let Dispatch::Reply(mut run, _) = capacity
            .dispatch(&store, NOW + 40)
            .unwrap()
            .dispatched
            .remove(0)
        else {
            panic!("Owner reply expected");
        };
        let result = output_for(
            &run,
            ReplyDecision::Quiet,
            vec![assessment(&run.context.feedback[0].id, disposition.clone())],
        );
        complete_analysis(&store, &mut run, Ok(result), true, NOW + 41).unwrap();
        assert!(crate::feedback::publication_gate(&store, &next.review).is_err());
        assert_eq!(
            current_state(&store),
            match disposition {
                Disposition::Open => crate::queue::State::WaitingForAuthor,
                Disposition::Cleared => crate::queue::State::MachineSignedOff,
                Disposition::HumanInputRequired => crate::queue::State::WaitingForHuman,
            }
        );
        let ledger = store.load_feedback().unwrap();
        assert!(!ledger.records[0].context.closed);
        assert!(!ledger.records[0].context.thread.as_ref().unwrap().resolved);
        assert_eq!(
            std::fs::read(root.path().join("state/publications.json")).unwrap(),
            archive
        );
    }
}

fn mention_scan(comments: Vec<TopComment>) -> Scan {
    Scan {
        retained: vec![],
        feedback: vec![],
        mentions: vec![(
            MentionBinding {
                configuration_id: REPO.into(),
                account_id: "22".into(),
                account_login: "actor".into(),
                repository_id: "100".into(),
                repository_name: "example/repo".into(),
                pull_request_id: "9".into(),
                number: 1,
            },
            comments,
        )],
    }
}

#[test]
fn retention_mixed_old_and_new_publications_observe_all_feedback_before_admitting_replies() {
    let (_root, store, origin, mut old_thread) = fixture(1);
    let mut queue = store.load_queue_state().unwrap();
    queue.tracked[0].lifecycle = Lifecycle::Closed;
    queue.tracked[0].terminal_observed = true;
    queue.jobs[0].waiting = monitoring::WAITING_CLOSED.into();
    store.save_queue_state(&queue).unwrap();
    crate::retention::maintain(&store, true).unwrap();
    let retained = crate::retention::load(&store).unwrap().receipts[0].owned[0].clone();
    let ticket = poll(&store, 'a', NOW + 10);
    let job = store.load_queue().unwrap().pop().unwrap();
    let mut current = origin;
    current.id = "new-iteration-publication".into();
    current.review.job = job.clone();
    current.review.key = crate::review::key(&job, &current.review.assignment_id);
    current.review.operation = JobOperation::review(&job, NOW + 11);
    current.review.operation.state = OperationState::Completed;
    current.operation = JobOperation::review(&job, NOW + 11);
    current.operation.state = OperationState::Completed;
    current.receipts[0].review_id = "43".into();
    current.receipts[0].comment_ids = vec!["200".into()];
    current.batch.as_mut().unwrap().comments[0].body = "New iteration finding".into();
    store
        .save_reviews(std::slice::from_ref(&current.review))
        .unwrap();
    store
        .save_publications(std::slice::from_ref(&current))
        .unwrap();
    let mut new_thread = old_thread.clone();
    new_thread.id = "new-thread".into();
    new_thread.comments[0].id = "200".into();
    new_thread.comments[0].review_id = Some("43".into());
    new_thread.comments[0].body = current.batch.as_ref().unwrap().comments[0].body.clone();
    explanation(&mut old_thread, "11");
    let observations = || Scan {
        retained: vec![crate::retention::Observed {
            origin: retained.clone(),
            head: "a".repeat(40),
            threads: vec![old_thread.clone()],
        }],
        feedback: vec![Observed {
            origin: current.clone(),
            head: "a".repeat(40),
            threads: vec![new_thread.clone()],
        }],
        mentions: vec![],
    };
    admit_scan(&store, &ticket, observations(), NOW + 12).unwrap();
    admit_scan(&store, &ticket, observations(), NOW + 13).unwrap();
    let contexts =
        crate::feedback::contexts(&store, &job, &current.review.selection.agent.id).unwrap();
    assert_eq!(contexts.len(), 2);
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert!(matches!(runs[0].target, ConversationTarget::Retained(_)));
    assert_eq!(runs[0].context.feedback.len(), 2);
}

fn finish_scan(store: Store, observations: Scan, now: i64) -> (Store, Result<(), String>) {
    let mut monitor = Monitor::restore(&store).unwrap();
    if let Some(operation) = monitor.snapshot()[0].operation.as_ref().filter(|o| {
        matches!(
            o.state,
            OperationState::Failed | OperationState::ManualRetry
        )
    }) {
        let id = operation.id.clone();
        monitor.manual_retry_operation(&store, &id, now).unwrap();
        assert!(monitor.snapshot()[0].conversation_admission_pending);
        assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
    }
    let ticket = monitor.prepare_checks(&store, now, true).unwrap().remove(0);
    let mut auth = crate::GithubAuth::new();
    auth.accounts.insert(
        "22".into(),
        crate::GithubAccountState::Connected(Identity {
            id: "22".into(),
            login: "actor".into(),
        }),
    );
    let store = Mutex::new(store);
    let result = crate::finish_monitored_ticket(
        &Mutex::new(BTreeMap::new()),
        &Mutex::new(auth),
        &store,
        &Mutex::new(monitor),
        ticket,
        Ok((
            PollResult {
                connection: Connection {
                    identity: Identity {
                        id: "22".into(),
                        login: "actor".into(),
                    },
                    repository: RemoteRepository {
                        id: "100".into(),
                        name: "example/repo".into(),
                    },
                    capabilities: Capabilities {
                        read: true,
                        comment: CommentCapability::Available,
                    },
                },
                pull_requests: vec![pull('a')],
            },
            observations,
        )),
        now + 1,
    )
    .map_err(|e| e.message().to_string());
    let store = store.into_inner().unwrap();
    (store, result)
}

fn cleared_fixture() -> (tempfile::TempDir, Store, Publication, Thread) {
    let (root, store, origin, mut thread) = fixture(1);
    thread.resolved = true;
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
    (root, store, origin, thread)
}

#[test]
fn r73_follow_up_write_failure_retains_mention_intent_and_recovers_without_rediscovery() {
    let (root, store, _, _) = cleared_fixture();
    let order = store.load_queue_state().unwrap().next_enqueue_order + 1;
    store.fail_state_write("follow-ups.json", 1);
    let (store, result) = finish_scan(
        store,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 20,
    );
    assert!(result.is_err());
    drop(store);
    let store = Store::new(root.path().into());
    let ledger = store.load_feedback().unwrap();
    assert_eq!(
        ledger.mentions.len(),
        1,
        "Observed mention was lost at follow-up persistence"
    );
    let intent = &ledger.mentions[0];
    assert_eq!(intent.enqueue_order, order);
    assert_eq!(intent.enqueued_at, NOW + 21);
    assert!(uuid::Uuid::parse_str(&intent.work_id).is_ok());
    assert!(intent.follow_up_id.is_none());
    assert!(store.load_follow_ups().unwrap().is_empty());
    let health = Monitor::restore(&store).unwrap().snapshot().remove(0);
    assert_eq!(health.last_success, Some(NOW + 11));
    assert_eq!(
        health.last_failure.as_deref(),
        Some("conversation_admission")
    );
    assert!(health.conversation_admission_pending);
    assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
    assert!(Capacity::default()
        .snapshot(&store, NOW + 22)
        .unwrap()
        .work
        .iter()
        .any(|w| w.key.id == intent.work_id && w.enqueue_order == order && w.state == "blocked"));
    let (store, result) = finish_scan(store, Scan::default(), NOW + 30);
    result.unwrap();
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, intent.work_id);
    assert_eq!(runs[0].enqueue_order, Some(order));
    assert_eq!(runs[0].enqueued_at, Some(intent.enqueued_at));
    assert_eq!(
        store.load_feedback().unwrap().mentions[0]
            .follow_up_id
            .as_deref(),
        Some(intent.work_id.as_str())
    );
    let capacity = Capacity::default();
    let mut dispatched = capacity.dispatch(&store, NOW + 32).unwrap();
    assert!(dispatched.errors.is_empty(), "{:?}", dispatched.errors);
    assert_eq!(dispatched.dispatched.len(), 1);
    let work = dispatched.dispatched.remove(0);
    let key = work.key();
    assert_eq!(key.kind, Kind::Mention);
    assert_eq!(key.id, intent.work_id);
    let Dispatch::Reply(mut run, _) = work else {
        panic!("Recovered mention expected");
    };
    let result = output_for(&run, ReplyDecision::Quiet, vec![]);
    complete_analysis(&store, &mut run, Ok(result), true, NOW + 33).unwrap();
    capacity
        .release(&key, &run.analysis.as_ref().unwrap().id)
        .unwrap();
    assert_eq!(current_state(&store), crate::queue::State::MachineSignedOff);
    let runs = store.load_follow_ups().unwrap();
    let (store, result) = finish_scan(
        Store::new(root.path().into()),
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 40,
    );
    result.unwrap();
    assert_eq!(store.load_follow_ups().unwrap(), runs);
    assert!(Capacity::default()
        .dispatch(&store, NOW + 42)
        .unwrap()
        .dispatched
        .is_empty());
}

#[test]
fn r73_final_ledger_link_failure_recovers_exact_committed_execution() {
    let (root, store, _, _) = cleared_fixture();
    store.fail_state_write("feedback.json", 3);
    let (store, result) = finish_scan(
        store,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 20,
    );
    assert!(result.is_err());
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    let ledger = store.load_feedback().unwrap();
    assert_eq!(ledger.mentions.len(), 1);
    assert!(ledger.mentions[0].follow_up_id.is_none());
    assert_eq!(ledger.mentions[0].work_id, runs[0].id);
    let health = Monitor::restore(&store).unwrap().snapshot().remove(0);
    assert_eq!(health.last_success, Some(NOW + 11));
    assert_eq!(
        health.last_failure.as_deref(),
        Some("conversation_admission")
    );
    assert!(health.conversation_admission_pending);
    assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
    assert_eq!(
        Capacity::default()
            .snapshot(&store, NOW + 22)
            .unwrap()
            .work
            .iter()
            .filter(|w| w.key.id == runs[0].id)
            .count(),
        1,
        "Unlinked intent is not a second capacity job"
    );
    let (store, result) = finish_scan(Store::new(root.path().into()), Scan::default(), NOW + 30);
    result.unwrap();
    assert_eq!(store.load_follow_ups().unwrap(), runs);
    assert_eq!(
        store.load_feedback().unwrap().mentions[0]
            .follow_up_id
            .as_deref(),
        Some(runs[0].id.as_str())
    );
}

#[test]
fn r73_mention_recovery_never_replaces_linked_or_uncertain_execution() {
    let (_root, store, _, _) = cleared_fixture();
    store.fail_state_write("feedback.json", 3);
    let (store, result) = finish_scan(
        store,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 20,
    );
    assert!(result.is_err());
    let mut runs = store.load_follow_ups().unwrap();
    runs[0].publication = Some(runs[0].operation("mention_reply", NOW + 22));
    runs[0].publication.as_mut().unwrap().attempted_mutation = Some("mention_reply".into());
    runs[0].uncertain = true;
    runs[0].phase = Phase::Unresolved;
    runs[0].body = Some("Exact signed body awaiting reconciliation.".into());
    store.save_follow_ups(&runs).unwrap();
    let (store, result) = finish_scan(store, Scan::default(), NOW + 30);
    result.unwrap();
    assert_eq!(store.load_follow_ups().unwrap(), runs);
    assert_eq!(current_state(&store), crate::queue::State::Failed);
    store.save_follow_ups(&[]).unwrap();
    let (store, result) = finish_scan(
        store,
        mention_scan(vec![mention("501", "@actor explain")]),
        NOW + 40,
    );
    result.unwrap();
    assert!(store.load_follow_ups().unwrap().is_empty());
    let mention = store.load_feedback().unwrap().mentions.remove(0);
    assert_eq!(mention.follow_up_id.as_deref(), Some(runs[0].id.as_str()));
    assert!(mention.blocked.unwrap().contains("no replacement"));
    assert_eq!(current_state(&store), crate::queue::State::Blocked);
}

#[test]
fn r73_initial_intent_and_prior_feedback_failures_cannot_commit_monitor_success() {
    for failure in ["intent_write", "feedback_write", "feedback_observation"] {
        let (root, store, origin, mut thread) = cleared_fixture();
        let success = Monitor::restore(&store).unwrap().snapshot()[0].last_success;
        match failure {
            "intent_write" => store.fail_state_write("feedback.json", 2),
            "feedback_write" => store.fail_state_write("feedback.json", 1),
            "feedback_observation" => thread.comments[0].body = "Unverified altered root.".into(),
            _ => unreachable!(),
        }
        let mut scan = mention_scan(vec![mention("501", "@actor explain")]);
        if failure != "intent_write" {
            scan.mentions.clear();
            scan.feedback.push(Observed {
                origin,
                head: "a".repeat(40),
                threads: vec![thread],
            });
        }
        let (store, result) = finish_scan(store, scan, NOW + 20);
        assert!(result.is_err());
        assert!(store.load_follow_ups().unwrap().is_empty());
        drop(store);
        let store = Store::new(root.path().into());
        let health = Monitor::restore(&store).unwrap().snapshot().remove(0);
        assert_eq!(
            health.last_success, success,
            "Admission failure was hidden by monitor success"
        );
        assert!(health.last_failure.is_some());
        assert!(health.conversation_admission_pending);
        assert_ne!(health.operation.unwrap().state, OperationState::Completed);
        assert_ne!(current_state(&store), crate::queue::State::MachineSignedOff);
        assert_ne!(
            crate::queue::snapshot(&store, vec![]).unwrap().items[0].state,
            crate::queue::State::MachineSignedOff,
            "Projection cannot omit the durable admission blocker"
        );
    }
}

#[test]
fn current_iteration_owner_reply_keeps_original_review_root_and_receipts() {
    let (root, store, origin, mut thread) = fixture(2);
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let reviews = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    let publications = std::fs::read(root.path().join("state/publications.json")).unwrap();
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'c', vec![], NOW + 20);
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].context.job.head_sha, "c".repeat(40));
    assert_eq!(runs[0].owned().unwrap().review.job.head_sha, "a".repeat(40));
    assert_eq!(
        runs[0]
            .owned()
            .unwrap()
            .thread
            .root()
            .unwrap()
            .original_commit
            .as_deref(),
        Some("a".repeat(40).as_str())
    );
    assert_eq!(
        runs[0].context.selection.agent.id,
        origin.review.selection.agent.id
    );
    assert!(!runs[0].context.trust_confirmed);
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        reviews
    );
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        publications
    );
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|j| {
            j.head_sha == "c".repeat(40)
                && j.assignment_id.as_deref() == Some(&origin.review.assignment_id)
        })
        .unwrap();
    let context =
        crate::feedback::contexts(&store, &job, &origin.review.selection.agent.id).unwrap();
    assert_eq!(context.len(), 1);
    let task = FullReview {
        final_context: None,
        feedback: context.clone(),
        owner_agent_id: origin.review.selection.agent.id.clone(),
    };
    let output = json!({"synopsis":"Review completed.","files":[{"path":"source.rs","order":1,"explanation":"Source."}],"findings":[],"decision":"machine_sign_off"});
    assert!(task
        .validate(
            &output.to_string(),
            &source_context('c'),
            &GithubClient::new(Source),
            "example/repo"
        )
        .is_err());
    let mut complete = output;
    complete["feedback_assessments"] = json!([assessment(&context[0].id, Disposition::Open)]);
    assert!(task
        .validate(
            &complete.to_string(),
            &source_context('c'),
            &GithubClient::new(Source),
            "example/repo"
        )
        .is_ok());
}

#[test]
fn same_head_explanation_clears_only_with_an_explicit_validated_assessment() {
    for clear in [false, true] {
        let (root, store, origin, mut thread) = fixture(1);
        explanation(&mut thread, "22");
        observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
        let archive = std::fs::read(root.path().join("state/reviews.json")).unwrap();
        let jobs = store.load_queue().unwrap();
        let capacity = Capacity::default();
        let mut batch = capacity.dispatch(&store, NOW + 20).unwrap();
        assert!(batch.errors.is_empty(), "{:?}", batch.errors);
        assert_eq!(batch.dispatched.len(), 1);
        let Dispatch::Reply(mut run, _) = batch.dispatched.remove(0) else {
            panic!("Owner reply expected");
        };
        let assessments = if clear {
            vec![assessment(
                &run.context.feedback[0].id,
                Disposition::Cleared,
            )]
        } else {
            vec![]
        };
        let result = output_for(&run, ReplyDecision::Quiet, assessments);
        complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
        assert_eq!(
            crate::feedback::views(&store, &run.context.job).unwrap()[0].state,
            if clear { "cleared" } else { "open" }
        );
        let snapshot = crate::queue::snapshot(&store, vec![]).unwrap();
        assert_eq!(
            snapshot.items[0].state,
            if clear {
                crate::queue::State::MachineSignedOff
            } else {
                crate::queue::State::WaitingForAuthor
            }
        );
        assert_eq!(store.load_queue().unwrap(), jobs);
        assert_eq!(
            std::fs::read(root.path().join("state/reviews.json")).unwrap(),
            archive
        );
        assert_eq!(store.load_publications().unwrap(), vec![origin.clone()]);
        observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
        assert_eq!(store.load_follow_ups().unwrap().len(), 1);
    }
}

#[test]
fn closure_does_not_hide_an_unresolved_reply_mutation_or_erase_its_intent() {
    let (_root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let mut run =
        prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
    run.publication = Some(run.operation("thread_reply", NOW + 21));
    run.publication.as_mut().unwrap().state = OperationState::ManualRetry;
    run.publication.as_mut().unwrap().attempted_mutation = Some("thread_reply".into());
    run.phase = Phase::Unresolved;
    run.uncertain = true;
    run.body = Some("Frozen reply.".into());
    save_to_store(&store, &run).unwrap();
    thread.resolved = true;
    observe(&store, &origin, &thread, 'a', vec![], NOW + 30);
    assert_eq!(
        crate::queue::snapshot(&store, vec![]).unwrap().items[0].state,
        crate::queue::State::Failed
    );
    assert_eq!(store.load_follow_ups().unwrap()[0], run);
}
#[test]
fn closed_tombstones_cannot_be_resurrected_by_new_heads_or_renamed_findings() {
    let (_root, store, origin, mut thread) = fixture(1);
    thread.resolved = true;
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    assert_eq!(
        crate::queue::snapshot(&store, vec![]).unwrap().items[0].state,
        crate::queue::State::MachineSignedOff
    );
    thread.resolved = false;
    observe(&store, &origin, &thread, 'c', vec![], NOW + 20);
    let job = store.load_queue().unwrap().pop().unwrap();
    let context =
        crate::feedback::contexts(&store, &job, &origin.review.selection.agent.id).unwrap();
    assert!(context[0].closed);
    let mut output:crate::review::ReviewOutput=serde_json::from_value(json!({"synopsis":"A problem remains.","files":[],"findings":[
        {"feedback_id":context[0].id,"path":"renamed.rs","side":"head","line":99,"severity":"high","title":"Paraphrased concern","explanation":"Same concern.","confidence":90}
    ],"decision":"human_input_required"})).unwrap();
    assert!(
        crate::feedback::validate_review(&output, &context, &origin.review.selection.agent.id)
            .is_err()
    );
    output.findings[0].feedback_id = None;
    output.findings[0].path = "source.rs".into();
    output.findings[0].title = "Wrong value".into();
    assert!(
        crate::feedback::validate_review(&output, &context, &origin.review.selection.agent.id)
            .is_err()
    );
    output.findings[0].title = "A different wording".into();
    output.files = vec![crate::review::FileGuide {
        path: "source.rs".into(),
        order: Some(1),
        explanation: "Source.".into(),
    }];
    let task = FullReview {
        final_context: None,
        feedback: context.clone(),
        owner_agent_id: origin.review.selection.agent.id.clone(),
    };
    let held = task
        .validate(
            &serde_json::to_string(&output).unwrap(),
            &source_context('c'),
            &GithubClient::new(Source),
            "example/repo",
        )
        .unwrap();
    assert!(held.feedback_conflict);
    assert_eq!(held.held_findings, output.findings);
    assert!(held.findings.is_empty());
    assert_eq!(held.decision, crate::review::Decision::HumanInputRequired);
    assert!(store.load_follow_ups().unwrap().is_empty());
    let mut ledger = store.load_feedback().unwrap();
    ledger.observe(&origin, &job.head_sha, &[]).unwrap();
    store.save_feedback(&ledger).unwrap();
    assert!(ledger.records[0].context.closed);
    assert_eq!(
        crate::feedback::views(&store, &job).unwrap()[0].state,
        "unavailable"
    );
}

#[test]
fn unresolved_feedback_change_fences_publication_without_blocking_its_own_new_root_receipt() {
    let (_root, store, mut origin, thread) = fixture(1);
    origin.review.feedback_context = Some(vec![]);
    store
        .save_publications(std::slice::from_ref(&origin))
        .unwrap();
    assert!(crate::feedback::publication_gate(&store, &origin.review).is_ok());
    observe(&store, &origin, &thread, 'c', vec![], NOW + 10);
    let current_job = store.load_queue().unwrap().pop().unwrap();
    let mut next = origin.review.clone();
    next.operation.id = "later-pass".into();
    next.job = current_job.clone();
    next.selection = Selection::resolve(
        &store.load_settings().unwrap(),
        &current_job,
        &next.assignment_id,
    )
    .unwrap();
    next.feedback_context =
        Some(crate::feedback::contexts(&store, &current_job, &next.selection.agent.id).unwrap());
    assert!(crate::feedback::publication_gate(&store, &next).is_ok());
    let mut changed = thread;
    explanation(&mut changed, "11");
    observe(&store, &origin, &changed, 'c', vec![], NOW + 20);
    assert!(crate::feedback::publication_gate(&store, &next).is_err());
}

#[test]
fn observing_removed_owners_does_not_transfer_or_erase_feedback() {
    let (_root, store, origin, mut thread) = fixture(2);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments.remove(0);
    store.save_settings(&settings).unwrap();
    explanation(&mut thread, "11");
    let ticket = poll(&store, 'c', NOW + 10);
    assert_eq!(
        super::super::observation_origins(&store, &ticket, &[pull('c')])
            .unwrap()
            .len(),
        1
    );
    admit_scan(
        &store,
        &ticket,
        Scan {
            retained: vec![],
            feedback: vec![Observed {
                origin: origin.clone(),
                head: "c".repeat(40),
                threads: vec![thread],
            }],
            mentions: vec![],
        },
        NOW + 12,
    )
    .unwrap();
    assert!(store.load_follow_ups().unwrap().is_empty());
    let job = store.load_queue().unwrap().pop().unwrap();
    assert_eq!(
        crate::feedback::views(&store, &job).unwrap()[0].state,
        "owner_unavailable"
    );
    assert_eq!(store.load_publications().unwrap(), vec![origin]);
}

#[test]
fn one_mention_routes_to_primary_with_stable_order_and_no_role_change_replay() {
    let (_root, store, origin, thread) = fixture(7);
    let comment = mention("501", "@actor please explain the contract.");
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![comment.clone()],
        NOW + 10,
    );
    assert!(store.load_follow_ups().unwrap().is_empty());
    let waiting = store.load_feedback().unwrap().mentions.remove(0);
    assert!(waiting.blocked.unwrap().contains("No primary"));
    let capacity = Capacity::default();
    assert!(capacity
        .snapshot(&store, NOW + 12)
        .unwrap()
        .work
        .iter()
        .any(|w| w.key.kind == Kind::Mention && w.state == "blocked"));
    let mut settings = store.load_settings().unwrap();
    let selected = settings.repositories[0].assignments[1].clone();
    settings.repositories[0].primary_assignment_id = Some(selected.id.clone());
    store.save_settings(&settings).unwrap();
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![comment.clone()],
        NOW + 20,
    );
    let runs = store.load_follow_ups().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, waiting.work_id);
    assert_eq!(runs[0].enqueue_order, Some(waiting.enqueue_order));
    assert_eq!(runs[0].context.selection.agent.id, selected.agent_id);
    assert_eq!(runs[0].kind(), Kind::Mention);
    let batch = capacity.dispatch(&store, NOW + 23).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    assert_eq!(batch.dispatched[0].key().kind, Kind::Mention);
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[2].id.clone());
    store.save_settings(&settings).unwrap();
    observe(&store, &origin, &thread, 'a', vec![comment], NOW + 30);
    assert_eq!(store.load_follow_ups().unwrap().len(), 1);
    assert!(local_gate(&store, &store.load_follow_ups().unwrap()[0]).is_err());
    assert_eq!(store.load_reviews().unwrap().len(), 7);
    assert_eq!(store.load_publications().unwrap(), vec![origin]);
}

#[test]
fn mention_matching_excludes_ai_identity_emails_and_signed_machine_output_but_not_same_account_humans(
) {
    for (body, expected) in [
        ("@ACTOR explain this", true),
        ("email@actor.example", false),
        ("@actor-extra", false),
        ("@ai-account help", false),
        ("@actor help <!-- pr-sniper:reply:already-sent -->", false),
        ("(@actor), another human question", true),
    ] {
        assert_eq!(
            mention("1", body).mentions("actor", "22"),
            expected,
            "{body}"
        );
    }
}

#[test]
fn handled_mentions_and_signed_output_do_not_retrigger_across_restart() {
    let (_root, store, origin, thread) = fixture(1);
    let comment = mention("501", "@actor explain the expected value.");
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![
            comment.clone(),
            mention("502", "@ai-account not the repository actor"),
        ],
        NOW + 10,
    );
    let capacity = Capacity::default();
    let Dispatch::Reply(mut run, _) = capacity
        .dispatch(&store, NOW + 20)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Mention expected")
    };
    let result = output_for(&run, ReplyDecision::Quiet, vec![]);
    complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
    let order = store.load_queue_state().unwrap().next_enqueue_order;
    super::super::restore(&store).unwrap();
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![
            comment,
            mention("503", "@actor response <!-- pr-sniper:reply:machine -->"),
        ],
        NOW + 30,
    );
    assert_eq!(store.load_follow_ups().unwrap().len(), 1);
    assert_eq!(store.load_feedback().unwrap().mentions.len(), 1);
    assert_eq!(store.load_queue_state().unwrap().next_enqueue_order, order);
    assert!(Capacity::default()
        .dispatch(&store, NOW + 40)
        .unwrap()
        .dispatched
        .is_empty());
}

#[test]
fn legacy_decoding_separates_actual_analysis_selection_from_the_retained_original() {
    let (_root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "11");
    let mut legacy_review = origin.review.clone();
    legacy_review.selection.agent.prompt = "Actual later analysis prompt.".into();
    let raw = json!([{"id":"legacy","key":"legacy-key","publication_id":origin.id,"review":legacy_review,"thread":thread,
        "trigger_id":"101","phase":"quiet","analysis":null,"publication":null,"history":[],"manual_start":true,
        "confirmed":false,"automatic_publication":false,"cancelled":false,"error":null,"result":null,"body":null,"uncertain":false,"receipt":null}]);
    let runs = super::super::decode_with_origins(
        &serde_json::to_vec(&raw).unwrap(),
        std::slice::from_ref(&origin),
    )
    .unwrap();
    assert_eq!(runs[0].owned().unwrap().review, origin.review);
    assert_eq!(
        runs[0].context.selection.agent.prompt,
        "Actual later analysis prompt."
    );
    assert_eq!(store.load_publications().unwrap(), vec![origin]);
}

#[test]
fn legacy_missing_local_configuration_does_not_orphan_verified_owned_roots() {
    let (_root, store, mut origin, thread) = fixture(1);
    origin.review.job.configuration_id.clear();
    store
        .save_publications(std::slice::from_ref(&origin))
        .unwrap();
    let ticket = poll(&store, 'c', NOW + 10);
    assert_eq!(
        super::super::observation_origins(&store, &ticket, &[pull('c')])
            .unwrap()
            .len(),
        1
    );
    admit_scan(
        &store,
        &ticket,
        Scan {
            retained: vec![],
            feedback: vec![Observed {
                origin: origin.clone(),
                head: "c".repeat(40),
                threads: vec![thread],
            }],
            mentions: vec![],
        },
        NOW + 12,
    )
    .unwrap();
    let job = store.load_queue().unwrap().pop().unwrap();
    assert_eq!(
        crate::feedback::contexts(&store, &job, &origin.review.selection.agent.id)
            .unwrap()
            .len(),
        1
    );
    assert!(store.load_publications().unwrap()[0]
        .review
        .job
        .configuration_id
        .is_empty());
}

#[test]
fn mixed_normal_owner_reply_and_mention_use_real_fifo_dispatch_and_pause_accounting() {
    let (_root, store, origin, mut thread) = fixture(2);
    let mut settings = store.load_settings().unwrap();
    settings.capacity = 2;
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    store.save_settings(&settings).unwrap();
    explanation(&mut thread, "11");
    observe(
        &store,
        &origin,
        &thread,
        'c',
        vec![mention("501", "@actor explain this revision")],
        NOW + 10,
    );
    let capacity = Capacity::default();
    let mut normal = capacity.dispatch(&store, NOW + 20).unwrap();
    assert!(normal.errors.is_empty(), "{:?}", normal.errors);
    assert_eq!(normal.dispatched.len(), 2);
    for work in normal.dispatched.drain(..) {
        let key = work.key();
        let Dispatch::Review(run, _) = work else {
            panic!("Normal passes arrived first")
        };
        crate::review::validate_execution_selection(&store, &run).unwrap();
        let mut result = origin.review.result.clone().unwrap();
        result.output.findings.clear();
        result.output.decision = crate::review::Decision::MachineSignOff;
        result.output.feedback_assessments = run
            .feedback_context
            .as_ref()
            .unwrap()
            .iter()
            .filter(|c| c.owner_agent_id == run.selection.agent.id && !c.closed)
            .map(|c| assessment(&c.id, Disposition::Open))
            .collect();
        crate::review::host::complete(&store, &run.operation.id, Ok(result), NOW + 21).unwrap();
        capacity.release(&key, &run.operation.id).unwrap();
    }
    let mut replies = capacity.dispatch(&store, NOW + 22).unwrap();
    assert!(replies.errors.is_empty(), "{:?}", replies.errors);
    assert_eq!(replies.dispatched.len(), 2);
    assert_eq!(replies.dispatched[0].key().kind, Kind::Reply);
    assert_eq!(replies.dispatched[1].key().kind, Kind::Mention);
    store
        .save_automation(&crate::capacity::Automation { paused: true })
        .unwrap();
    assert!(capacity
        .dispatch(&store, NOW + 23)
        .unwrap()
        .dispatched
        .is_empty());
    assert_eq!(capacity.snapshot(&store, NOW + 23).unwrap().stopping, 2);
    for work in replies.dispatched.drain(..) {
        let key = work.key();
        let Dispatch::Reply(mut run, token) = work else {
            panic!("Conversation worker expected")
        };
        assert!(token.load(Ordering::SeqCst));
        let order = run.enqueue_order;
        complete_analysis(&store, &mut run, Err(Failure::cancelled()), true, NOW + 24).unwrap();
        assert_eq!(run.analysis.as_ref().unwrap().attempt_count, 0);
        assert_eq!(run.enqueue_order, order);
        capacity
            .release(&key, &run.analysis.as_ref().unwrap().id)
            .unwrap();
    }
    store
        .save_automation(&crate::capacity::Automation { paused: false })
        .unwrap();
    assert_eq!(
        capacity
            .dispatch(&store, NOW + 25)
            .unwrap()
            .dispatched
            .len(),
        2
    );
    assert_eq!(store.load_publications().unwrap(), vec![origin]);
}

fn retry_while_phase_save_is_held(kind: Kind) {
    use std::{sync::mpsc, time::Duration};

    let (root, store, origin, mut thread) = fixture(1);
    let comments = if kind == Kind::Mention {
        vec![mention("501", "@actor explain this revision")]
    } else {
        explanation(&mut thread, "11");
        vec![]
    };
    observe(&store, &origin, &thread, 'a', comments, NOW + 10);
    let reviews = std::fs::read(root.path().join("state/reviews.json")).unwrap();
    let publications = std::fs::read(root.path().join("state/publications.json")).unwrap();
    let capacity = Capacity::default();
    let mut batch = capacity.dispatch(&store, NOW + 20).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let work = batch.dispatched.remove(0);
    let key = work.key();
    assert_eq!(key.kind, kind);
    let Dispatch::Reply(mut old, token) = work else {
        panic!("Conversation worker expected")
    };
    let old_id = old.analysis.as_ref().unwrap().id.clone();
    let store = Mutex::new(store);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let accepted = std::thread::scope(|scope| {
        let store = &store;
        let capacity = &capacity;
        let key = &key;
        let old_id = &old_id;
        let worker = scope.spawn(move || {
            old.phase = Phase::Analyzing;
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let store = store.lock().unwrap();
            // The same locked save called by Native at both analysis phase boundaries.
            let saved = save_progress(&store, &mut old, NOW + 24);
            let failure = saved.clone().err().unwrap_or_else(Failure::cancelled);
            let completed = complete_analysis(&store, &mut old, Err(failure), false, NOW + 24);
            let after = store.load_follow_ups().unwrap().remove(0);
            let released = finish_analysis_worker(&store, capacity, key, old_id);
            (saved, completed, after, released)
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let accepted = {
            let store = store.lock().unwrap();
            cancel_in_store(&store, capacity, None, &key.id, NOW + 21).unwrap();
            assert!(token.load(Ordering::SeqCst));
            let cancelled = store.load_follow_ups().unwrap().remove(0);
            assert_eq!(
                cancelled.analysis.as_ref().unwrap().state,
                OperationState::Failed
            );
            let accepted = request_analysis(&store, &key.id, true, NOW + 22).unwrap();
            assert_ne!(&accepted.analysis.as_ref().unwrap().id, old_id);
            assert_eq!(
                accepted.analysis.as_ref().unwrap().state,
                OperationState::Queued
            );
            assert_eq!(accepted.history, vec![cancelled.analysis.unwrap()]);
            assert!(accepted.manual_start);
            assert!(!accepted.cancelled);
            assert!(capacity
                .dispatch(&store, NOW + 23)
                .unwrap()
                .dispatched
                .is_empty());
            let snapshot = capacity.snapshot(&store, NOW + 23).unwrap();
            assert_eq!((snapshot.active, snapshot.stopping), (1, 1));
            accepted
        };
        release_tx.send(()).unwrap();
        let (saved, completed, after, released) = worker.join().unwrap();
        assert_eq!(
            after, accepted,
            "Stale phase save/completion replaced accepted retry intent"
        );
        assert!(
            saved.is_err(),
            "Superseded worker must stop at the phase save"
        );
        assert!(completed.is_err());
        released.unwrap();
        accepted
    });
    let store = store.lock().unwrap();
    assert!(capacity.finished());
    let mut batch = capacity.dispatch(&store, NOW + 25).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    let Dispatch::Reply(mut retry, _) = batch.dispatched.remove(0) else {
        panic!("Retry expected")
    };
    let retry_id = retry.analysis.as_ref().unwrap().id.clone();
    assert_eq!(retry_id, accepted.analysis.as_ref().unwrap().id);
    assert_eq!(retry.analysis.as_ref().unwrap().attempt_count, 1);
    assert_eq!(retry.history, accepted.history);
    assert_eq!(retry.target, accepted.target);
    assert_eq!(retry.context, accepted.context);
    assert!(retry.manual_start);
    assert!(capacity
        .dispatch(&store, NOW + 26)
        .unwrap()
        .dispatched
        .is_empty());
    // A late A callback cannot release B's reservation or change its durable operation.
    assert!(finish_analysis_worker(&store, &capacity, &key, &old_id).is_err());
    let snapshot = capacity.snapshot(&store, NOW + 26).unwrap();
    assert_eq!((snapshot.active, snapshot.stopping), (1, 0));
    assert!(snapshot
        .work
        .iter()
        .find(|w| w.key == key)
        .unwrap()
        .reason
        .is_none());
    assert_eq!(store.load_follow_ups().unwrap(), vec![*retry.clone()]);
    let assessments = retry
        .context
        .feedback
        .iter()
        .filter(|c| {
            kind == Kind::Reply && !c.closed && c.owner_agent_id == retry.context.selection.agent.id
        })
        .map(|c| assessment(&c.id, Disposition::Open))
        .collect();
    let result = output_for(&retry, ReplyDecision::Quiet, assessments);
    complete_analysis(&store, &mut retry, Ok(result), true, NOW + 27).unwrap();
    finish_analysis_worker(&store, &capacity, &key, &retry_id).unwrap();
    assert!(capacity.finished());
    assert!(capacity
        .dispatch(&store, NOW + 28)
        .unwrap()
        .dispatched
        .is_empty());
    let saved = store.load_follow_ups().unwrap().remove(0);
    assert_eq!(saved.analysis.as_ref().unwrap().id, retry_id);
    assert_eq!(saved.analysis.as_ref().unwrap().attempt_count, 1);
    assert_eq!(
        saved.analysis.as_ref().unwrap().state,
        OperationState::Completed
    );
    assert_eq!(saved.history, accepted.history);
    assert!(saved.manual_start);
    assert_eq!(
        std::fs::read(root.path().join("state/reviews.json")).unwrap(),
        reviews
    );
    assert_eq!(
        std::fs::read(root.path().join("state/publications.json")).unwrap(),
        publications
    );
}

#[test]
fn r72_owner_reply_retry_survives_stale_production_phase_save_and_completion() {
    retry_while_phase_save_is_held(Kind::Reply);
}

#[test]
fn r72_primary_mention_retry_survives_stale_production_phase_save_and_completion() {
    retry_while_phase_save_is_held(Kind::Mention);
}

#[test]
fn unavailable_owner_and_fork_trust_do_not_gain_authority_from_comments() {
    let (_root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "11");
    let mut settings = store.load_settings().unwrap();
    settings.agents[0].ai_account = None;
    store.save_settings(&settings).unwrap();
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![mention("501", "@actor investigate")],
        NOW + 10,
    );
    assert!(store.load_follow_ups().unwrap().is_empty());
    let ledger = store.load_feedback().unwrap();
    assert!(ledger.mentions[0].blocked.is_some());
    assert_eq!(
        crate::feedback::views(&store, &store.load_queue().unwrap()[0]).unwrap()[0].state,
        "owner_unavailable"
    );
    settings.agents[0].ai_account = origin.review.selection.agent.ai_account.clone();
    store.save_settings(&settings).unwrap();
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![mention("501", "@actor investigate")],
        NOW + 20,
    );
    let mut run = store
        .load_follow_ups()
        .unwrap()
        .into_iter()
        .find(|r| r.kind() == Kind::Mention)
        .unwrap();
    let mut fork = pull('a');
    fork.head_repository_id = Some("200".into());
    assert!(run
        .validate_current(&settings, &run.context.job, &fork, true, true)
        .unwrap_err()
        .message
        .contains("Trust"));
    run.context.trust_confirmed = true;
    assert!(run
        .validate_current(&settings, &run.context.job, &fork, true, true)
        .is_ok());
    assert!(run
        .validate_current(&settings, &run.context.job, &fork, false, true)
        .is_err());
    assert!(run
        .validate_current(&settings, &run.context.job, &fork, true, false)
        .is_err());
    settings.repositories[0].assignments[0].comment = false;
    assert!(run.authority(&settings, &run.context.job).is_err());
}

#[test]
fn human_judgment_and_tampered_ownership_never_become_clearance() {
    let (_root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let mut run =
        prepare_dispatch(&store, &store.load_follow_ups().unwrap()[0].id, NOW + 20).unwrap();
    let result = output_for(&run, ReplyDecision::HumanInputRequired, vec![]);
    complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
    assert_eq!(
        crate::queue::snapshot(&store, vec![]).unwrap().items[0].state,
        crate::queue::State::WaitingForHuman
    );
    let mut tampered = thread.clone();
    tampered.comments[0].body = "Changed root.".into();
    let mut ledger = store.load_feedback().unwrap();
    assert!(ledger
        .observe(&origin, &"a".repeat(40), &[tampered])
        .is_err());
    assert_eq!(ledger, store.load_feedback().unwrap());
    let mut second = thread;
    second.comments.last_mut().unwrap().id = "102".into();
    observe(&store, &origin, &second, 'a', vec![], NOW + 30);
    let candidates = candidates(&store).unwrap();
    assert!(candidates.iter().all(|c| !c.automatic_start));
}

#[derive(Default)]
struct CommentServer {
    comments: Vec<Value>,
    writes: usize,
    lost: bool,
}
#[derive(Clone)]
struct CommentWire(Arc<Mutex<CommentServer>>);
impl Transport for CommentWire {
    fn get(&self, path: &str) -> Result<Response, github::ConnectionError> {
        assert_eq!(
            path,
            "/repos/example/repo/issues/1/comments?per_page=100&page=1"
        );
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&self.0.lock().unwrap().comments).unwrap(),
        })
    }
}
impl MutationTransport for CommentWire {
    fn mutate(
        &self,
        path: &str,
        request: HttpRequest,
    ) -> Result<Response, github::ConnectionError> {
        assert_eq!(path, "/repos/example/repo/issues/1/comments");
        let HttpRequest::Post(body) = request else {
            panic!("No resolve/edit/delete operation permitted")
        };
        let mut server = self.0.lock().unwrap();
        server.writes += 1;
        let comment = json!({"id":9001,"body":body["body"],"user":{"id":22,"login":"actor"},
            "created_at":"2026-09-30T00:02:00Z","updated_at":"2026-09-30T00:02:00Z",
            "issue_url":"https://api.github.com/repos/example/repo/issues/1"});
        server.comments.push(comment.clone());
        if server.lost {
            server.lost = false;
            return Err(github::ConnectionError::Timeout);
        }
        Ok(Response {
            status: 201,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&comment).unwrap(),
        })
    }
}

struct MentionEnvironment<'a> {
    store: &'a Store,
    wire: CommentWire,
    now: i64,
    change_primary: bool,
}
impl Environment for MentionEnvironment<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(self.now)
    }
    fn save(&mut self, run: &mut FollowUp) -> Result<(), Failure> {
        save_to_store(self.store, run).map_err(Failure::permanent)
    }
    fn observe(&mut self, run: &FollowUp) -> Result<Observation, Failure> {
        let replies = GithubClient::new(self.wire.clone()).top_comments(
            &RemoteRepository {
                id: run.context.job.repository_id.clone(),
                name: run.context.job.repository_name.clone(),
            },
            run.context.job.number,
        )?;
        let comment = replies.iter().find(|c| c.id == run.trigger_id).cloned();
        Ok(Observation::Mention { comment, replies })
    }
    fn gate(
        &mut self,
        run: &FollowUp,
        observation: &Observation,
    ) -> Result<Option<String>, Failure> {
        if !run.fresh_observation(observation) {
            return Ok(Some("Trigger changed.".into()));
        }
        crate::capacity::publication_gate(self.store).map_err(Failure::permanent)?;
        let settings = self.store.load_settings().map_err(Failure::permanent)?;
        Ok(run
            .validate_current(&settings, &run.context.job, &pull('a'), true, true)
            .err()
            .map(|e| e.message))
    }
    fn reply(&mut self, run: &FollowUp) -> Result<String, WriteFailure> {
        let observation = self.observe(run).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        if let Some(reason) = self
            .gate(run, &observation)
            .map_err(|failure| WriteFailure {
                failure,
                uncertain: false,
            })?
        {
            return Err(WriteFailure {
                failure: Failure::permanent(reason),
                uncertain: false,
            });
        }
        let result = GithubClient::new(self.wire.clone()).reply_to_mention(
            &RemoteRepository {
                id: run.context.job.repository_id.clone(),
                name: run.context.job.repository_name.clone(),
            },
            run.context.job.number,
            &run.context.job.account_id,
            run.body.as_deref().unwrap(),
        );
        if self.change_primary {
            let mut settings = self.store.load_settings().unwrap();
            settings.repositories[0].primary_assignment_id =
                Some(settings.repositories[0].assignments[1].id.clone());
            self.store.save_settings(&settings).unwrap();
        }
        result
    }
}

#[test]
fn mention_lost_response_reconciles_original_body_after_primary_change_without_replay() {
    let (_root, store, origin, thread) = fixture(2);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    store.save_settings(&settings).unwrap();
    let comment = mention("501", "@actor explain the expected result.");
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![comment.clone()],
        NOW + 10,
    );
    let capacity = Capacity::default();
    let Dispatch::Reply(mut run, _) = capacity
        .dispatch(&store, NOW + 20)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Mention adapter expected")
    };
    assert_eq!(run.kind(), Kind::Mention);
    let result = output_for(&run, ReplyDecision::Reply, vec![]);
    complete_analysis(&store, &mut run, Ok(result), true, NOW + 21).unwrap();
    run.publication = Some(run.operation("mention_reply", NOW + 22));
    run.confirmed = true;
    run.automatic_publication = false;
    run.body = Some(run.reply_body().unwrap());
    save_to_store(&store, &run).unwrap();
    let original_body = run.body.clone();
    let original_agent = run.context.selection.agent.id.clone();
    let wire = CommentWire(Arc::new(Mutex::new(CommentServer {
        comments: vec![json!({
            "id":501,"body":comment.body,"user":{"id":22,"login":"actor"},"created_at":comment.created_at,
            "updated_at":comment.updated_at,"issue_url":"https://api.github.com/repos/example/repo/issues/1"
        })],
        writes: 0,
        lost: true,
    })));
    let mut env = MentionEnvironment {
        store: &store,
        wire: wire.clone(),
        now: NOW + 22,
        change_primary: true,
    };
    assert!(super::super::publish(&mut env, &mut run).is_err());
    assert!(run.uncertain);
    assert_eq!(wire.0.lock().unwrap().writes, 1);
    wire.0.lock().unwrap().comments.retain(|c| c["id"] != 501);
    super::super::restore(&store).unwrap();
    let mut restored = store.load_follow_ups().unwrap().remove(0);
    env.now = restored
        .publication
        .as_ref()
        .unwrap()
        .next_attempt_at
        .unwrap();
    super::super::publish(&mut env, &mut restored).unwrap();
    assert_eq!(restored.receipt.as_deref(), Some("9001"));
    assert_eq!(wire.0.lock().unwrap().writes, 1);
    assert_eq!(restored.body, original_body);
    assert_eq!(restored.context.selection.agent.id, original_agent);
    assert_eq!(restored.phase, Phase::StaleAfterPublication);
    observe(&store, &origin, &thread, 'a', vec![comment], NOW + 40);
    assert_eq!(store.load_follow_ups().unwrap().len(), 1);
    assert_eq!(store.load_publications().unwrap(), vec![origin]);
}
