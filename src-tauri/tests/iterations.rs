use pr_sniper_lib::{
    follow_up,
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{
            Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository, Response,
            Transport,
        },
        ConnectionError, Identity,
    },
    monitoring::{
        self, ActivationMode, JobOperation, Monitor, MonitoringActivation, OperationState,
        PollResult, QueueJob, WorkTrigger,
    },
    publication::{self, Publication, Receipt, RemoteState},
    queue,
    review::{self, ReviewRun, Selection},
    storage::{ResourceEdit, Settings, Store},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

mod support;
use support::Fixture;

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const NOW: i64 = 1_800_000_000;

fn add_agent(settings: &mut Settings, index: usize) {
    let id = format!("aaaaaaaa-aaaa-4aaa-8aaa-{index:012}");
    settings.agents.push(
        serde_json::from_value(json!({
            "id":id,"name":format!("Agent {index}"),"model":"explicit-model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Find actionable defects.","signature":"fixture"
        }))
        .unwrap(),
    );
    settings.repositories[0].assignments.push(
        serde_json::from_value(json!({
            "id":format!("cccccccc-cccc-4ccc-8ccc-{index:012}"),"agent_id":id,
            "schedule":{"kind":"interval","minutes":index + 1,"timezone":"UTC"},"comment":true
        }))
        .unwrap(),
    );
}

fn configured(count: usize) -> (Fixture, Store, Monitor) {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false, "doctrines":[],
        "repositories":[{
            "id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}]
        }]
    }))
    .unwrap();
    for index in 1..=count {
        add_agent(&mut settings, index);
    }
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "fixture-activation".into(),
            repository_id: REPO.into(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: 100,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW - 1,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    let monitor = Monitor::restore(&store).unwrap();
    (fixture, store, monitor)
}

fn pull(head: char) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 101,
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
        base_sha: "f".repeat(40),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-09-30T00:00:00Z".into(),
        files: vec![],
    }
}

fn result(pulls: Vec<PullRequest>) -> PollResult {
    PollResult {
        connection: Connection {
            identity: Identity {
                id: "22".into(),
                login: "acting-account".into(),
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
        pull_requests: pulls,
    }
}

fn explicit_configuration(count: usize) -> (Fixture, Store, Monitor) {
    let (fixture, store, _) = configured(count);
    let expected = store.load_settings().unwrap().repositories.remove(0);
    store
        .save_resource(ResourceEdit::Repository {
            id: expected.id.clone(),
            expected: Some(Box::new(expected.clone())),
            value: Some(Box::new(expected)),
        })
        .unwrap();
    let monitor = Monitor::restore(&store).unwrap();
    (fixture, store, monitor)
}

fn explicit(
    monitor: &mut Monitor,
    store: &Store,
    pull: PullRequest,
) -> Result<monitoring::ExplicitAdmission, String> {
    let expected = store.load_settings().unwrap().repositories.remove(0);
    let resolved = result(vec![]);
    monitor.admit_explicit_pull_request(
        store,
        &BTreeMap::from([(
            "22".into(),
            monitoring::AccountAvailability {
                login: "acting-account".into(),
                connected: true,
            },
        )]),
        &expected,
        pr_sniper_lib::github::intake::ResolvedTarget {
            connection: resolved.connection,
            pull_request: Some(pull),
        },
        NOW,
    )
}

#[test]
fn explicit_out_of_filter_pr_queues_immediately_and_reuses_completed_iteration() {
    let (_fixture, store, mut monitor) = explicit_configuration(1);
    let mut requested = pull('a');
    requested.author.as_mut().unwrap().id = "99".into();
    assert!(
        explicit(&mut monitor, &store, requested.clone())
            .unwrap()
            .queued
    );
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(!jobs[0].watched_author && !jobs[0].all_authors && !jobs[0].requested_reviewer);
    let settings = store.load_settings().unwrap();
    assert!(monitoring::review_policy(&settings, &jobs[0], Some(&requested)).is_ok());
    let review = completed(&settings, &jobs[0]);
    store.save_reviews(&[review]).unwrap();
    assert!(
        explicit(&mut monitor, &store, requested.clone())
            .unwrap()
            .queued
    );
    assert_eq!(store.load_queue().unwrap(), jobs);
    let mut restored = Monitor::restore(&store).unwrap();
    assert!(
        explicit(&mut restored, &store, requested.clone())
            .unwrap()
            .queued
    );
    assert_eq!(store.load_queue().unwrap(), jobs);
    scan(&mut restored, &store, vec![requested.clone()], NOW + 3);
    assert_eq!(store.load_queue().unwrap(), jobs);
    let batch = pr_sniper_lib::capacity::Coordinator::default()
        .dispatch(&store, NOW + 1)
        .unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert!(
        batch.dispatched.is_empty(),
        "completed iteration must not run twice"
    );
    requested.head_sha = "b".repeat(40);
    explicit(&mut restored, &store, requested).unwrap();
    assert_eq!(store.load_queue_state().unwrap().tracked[0].iteration, 2);
    assert_eq!(store.load_queue().unwrap().len(), 2);
}

#[test]
fn explicit_pr_queue_survives_pause_and_drains_without_another_poll() {
    let (_fixture, store, mut monitor) = explicit_configuration(2);
    let mut settings = store.load_settings().unwrap();
    settings.capacity = 1;
    store.save_settings(&settings).unwrap();
    store
        .save_automation(&pr_sniper_lib::capacity::Automation { paused: true })
        .unwrap();
    let admission = explicit(&mut monitor, &store, pull('a')).unwrap();
    assert!(admission.queued && admission.message.contains("paused"));
    assert_eq!(store.load_queue().unwrap().len(), 2);
    let coordinator = pr_sniper_lib::capacity::Coordinator::default();
    assert!(coordinator
        .dispatch(&store, NOW)
        .unwrap()
        .dispatched
        .is_empty());
    assert_eq!(store.load_queue().unwrap().len(), 2);
    store
        .save_automation(&pr_sniper_lib::capacity::Automation { paused: false })
        .unwrap();
    let batch = coordinator.dispatch(&store, NOW + 1).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 1);
    assert!(coordinator
        .dispatch(&store, NOW + 2)
        .unwrap()
        .dispatched
        .is_empty());
    assert!(store.load_publications().unwrap().is_empty());
    assert!(store.load_actions().unwrap().effects.is_empty());
}

#[test]
fn explicit_pr_rejects_stale_saved_configuration_without_queueing_work() {
    let (_fixture, store, mut monitor) = explicit_configuration(1);
    let mut expected = store.load_settings().unwrap().repositories.remove(0);
    expected.enabled = false;
    let resolved = result(vec![]);
    let error = monitor
        .admit_explicit_pull_request(
            &store,
            &BTreeMap::new(),
            &expected,
            pr_sniper_lib::github::intake::ResolvedTarget {
                connection: resolved.connection,
                pull_request: Some(pull('a')),
            },
            NOW,
        )
        .unwrap_err();
    assert!(error.contains("configuration changed"));
    assert!(store.load_queue().unwrap().is_empty());
}

#[test]
fn explicit_pr_requires_saved_repository_authorization_not_just_a_url() {
    let (_fixture, store, mut monitor) = explicit_configuration(1);
    let mut settings = store.load_settings().unwrap();
    settings.repository_authorizations.insert(REPO.into(), None);
    store.save_settings(&settings).unwrap();
    assert!(explicit(&mut monitor, &store, pull('a'))
        .unwrap_err()
        .contains("Save this repository"));
    assert!(store.load_queue().unwrap().is_empty());
}

#[test]
fn explicit_pr_cannot_bypass_disabled_account_agent_or_revision_gates() {
    let (_fixture, store, mut monitor) = explicit_configuration(1);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].enabled = false;
    store.save_settings(&settings).unwrap();
    let admission = explicit(&mut monitor, &store, pull('a')).unwrap();
    assert!(!admission.queued && admission.message.contains("disabled"));
    assert!(store.load_queue().unwrap().is_empty());
    settings.repositories[0].enabled = true;
    store.save_settings(&settings).unwrap();
    let expected = settings.repositories[0].clone();
    let resolved = result(vec![]);
    assert!(monitor
        .admit_explicit_pull_request(
            &store,
            &BTreeMap::new(),
            &expected,
            pr_sniper_lib::github::intake::ResolvedTarget {
                connection: resolved.connection,
                pull_request: Some(pull('a')),
            },
            NOW
        )
        .unwrap_err()
        .contains("unavailable"));
    let mut wrong = pull('a');
    wrong.base_repository_id = "999".into();
    assert!(explicit(&mut monitor, &store, wrong).is_err());
    assert!(store.load_queue().unwrap().is_empty());
    explicit(&mut monitor, &store, pull('a')).unwrap();
    let job = store.load_queue().unwrap().remove(0);
    let mut changed = pull('b');
    assert!(monitoring::review_policy(&settings, &job, Some(&changed)).is_err());
    changed = pull('a');
    changed.draft = true;
    assert!(explicit(&mut monitor, &store, changed).is_err());
    settings.agents[0].ai_account = None;
    store.save_settings(&settings).unwrap();
    let coordinator = pr_sniper_lib::capacity::Coordinator::default();
    assert!(coordinator
        .dispatch(&store, NOW)
        .unwrap()
        .dispatched
        .is_empty());
    assert_eq!(coordinator.snapshot(&store, NOW).unwrap().blocked, 1);
}

fn scan(monitor: &mut Monitor, store: &Store, pulls: Vec<PullRequest>, at: i64) {
    let tickets = monitor.prepare_checks(store, at, true).unwrap();
    assert_eq!(tickets.len(), 1);
    assert!(tickets[0].assignment_id.is_none());
    monitor
        .finish(store, tickets[0].clone(), Ok(result(pulls)), at + 1)
        .unwrap();
}

fn completed(settings: &Settings, job: &QueueJob) -> ReviewRun {
    let assignment = job.assignment_id.as_ref().unwrap();
    let mut operation = JobOperation::review(job, NOW + 1);
    operation.begin_attempt(NOW + 1).unwrap();
    operation.state = OperationState::Completed;
    ReviewRun {
        feedback_context: None,
        key: review::key(job, assignment),
        assignment_id: assignment.clone(),
        job: job.clone(),
        selection: Selection::resolve(settings, job, assignment).unwrap(),
        operation,
        manual_start: true,
        trust_confirmed: false,
        phase: "Completed fixture".into(),
        error: None,
        result: Some(
            serde_json::from_value(json!({
                "reviewed_base_sha":"f".repeat(40),
                "output":{"synopsis":"No actionable defects were found.","files":[
                    {"path":"source.rs","explanation":"Reviewed source.","order":1}
                ],"findings":[],"decision":"machine_sign_off"},
                "session_id":"fixture-session","model":"explicit-model","runtime_version":"fixture",
                "input_tokens":10,"output_tokens":20,"tool_calls":1
            }))
            .unwrap(),
        ),
    }
}

fn running_primary_review() -> (Fixture, Store, ReviewRun) {
    let (fixture, store, mut monitor) = configured(2);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    settings.defaults.automatic_agent_start = true;
    settings.doctrines = serde_json::from_value(json!([
        {"title":"Correctness","body":"Trace state transitions."},
        {"title":"Boundaries","body":"Keep authority explicit."}
    ]))
    .unwrap();
    settings.agents[0].doctrines = Some(vec!["Correctness".into(), "Boundaries".into()]);
    settings.presets = serde_json::from_value(json!([{
        "id":"eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
        "name":"Review lens","body":"Check the repository contract."
    }]))
    .unwrap();
    settings.repositories[0].review_preset = Some(settings.presets[0].id.clone());
    store
        .save_preferences(settings, &store.load_settings().unwrap())
        .unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let settings = store.load_settings().unwrap();
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|job| {
            job.assignment_id.as_ref() == settings.repositories[0].primary_assignment_id.as_ref()
        })
        .unwrap();
    assert!(job.work.is_some());
    let mut run = completed(&settings, &job);
    run.operation.state = OperationState::Running;
    run.manual_start = false;
    run.phase = "Reviewing fixture".into();
    run.result = None;
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    review::validate_execution_selection(&store, &run).unwrap();
    (fixture, store, run)
}

#[test]
fn normal_review_survives_sibling_resource_saves_without_replacing_snapshot() {
    let (fixture, store, run) = running_primary_review();
    let original = run.selection.clone();
    let bytes = std::fs::read(fixture.path().join("state/reviews.json")).unwrap();
    for change in ["comment", "actions", "schedule", "agent", "remove"] {
        let settings = store.load_settings().unwrap();
        if change == "agent" {
            let mut sibling = settings.agents[1].clone();
            sibling.prompt = "A different sibling lens.".into();
            sibling.model = "another-model".into();
            sibling.ai_account.as_mut().unwrap().account_id = "44".into();
            sibling.doctrines = Some(vec!["Boundaries".into()]);
            store
                .save_resource(ResourceEdit::Agent {
                    id: sibling.id.clone(),
                    expected: Some(settings.agents[1].clone()),
                    value: Some(sibling),
                })
                .unwrap();
        } else {
            let mut repository = settings.repositories[0].clone();
            match change {
                "comment" => repository.assignments[1].comment = false,
                "actions" => {
                    repository.assignments[1].actions = Some(
                        serde_json::from_value(
                            json!({"reply":false,"approve":false,"merge":false}),
                        )
                        .unwrap(),
                    );
                }
                "schedule" => {
                    repository.assignments[1].schedule = serde_json::from_value(json!({
                        "kind":"cron","expression":"0 9 * * MON-FRI","timezone":"America/New_York"
                    }))
                    .unwrap();
                }
                "remove" => {
                    repository.assignments.remove(1);
                }
                _ => unreachable!(),
            }
            store
                .save_resource(ResourceEdit::Repository {
                    id: REPO.into(),
                    expected: Some(Box::new(settings.repositories[0].clone())),
                    value: Some(Box::new(repository)),
                })
                .unwrap();
        }
        let current = Selection::resolve(
            &store.load_settings().unwrap(),
            &run.job,
            &run.assignment_id,
        )
        .unwrap();
        assert_eq!(current.agent, original.agent);
        assert_eq!(current.policy, original.policy);
        assert_eq!(current.doctrine, original.doctrine);
        assert_eq!(current.preset, original.preset);
        assert_eq!(
            current.configuration.as_ref().unwrap().authority,
            original.configuration.as_ref().unwrap().authority
        );
        assert_eq!(
            current.configuration.as_ref().unwrap().doctrines,
            original.configuration.as_ref().unwrap().doctrines
        );
        assert_ne!(current, original, "{change}: archive must remain exact");
        review::validate_execution_selection(&store, &run)
            .unwrap_or_else(|error| panic!("{change}: {}", error.message));
        assert_eq!(store.load_reviews().unwrap()[0], run);
        assert_eq!(
            std::fs::read(fixture.path().join("state/reviews.json")).unwrap(),
            bytes
        );
    }
    review::restore(&store).unwrap();
    let restored = fixture.store().load_reviews().unwrap().remove(0);
    assert_eq!(restored.operation.state, OperationState::Interrupted);
    assert_eq!(restored.operation.id, run.operation.id);
    assert_eq!(restored.key, review::key(&run.job, &run.assignment_id));
    assert_eq!(restored.selection, original);
    review::validate_execution_selection(&store, &restored).unwrap();
}

#[test]
fn normal_review_execution_invalidates_own_inputs_authority_and_repository_gates() {
    for change in [
        "agent prompt",
        "model",
        "AI account",
        "Agent replacement",
        "doctrine body",
        "doctrine order",
        "doctrine selection",
        "preset body",
        "repository prompt",
        "publication gate",
        "comment",
        "approve",
        "merge",
        "primary",
        "repository disabled",
        "repository account",
        "repository identity",
        "repository name",
        "repository provider",
        "watched authors",
        "reviewer trigger",
        "assignment removed",
    ] {
        let (_fixture, store, run) = running_primary_review();
        let mut settings = store.load_settings().unwrap();
        match change {
            "agent prompt" => settings.agents[0].prompt = "Changed own lens.".into(),
            "model" => settings.agents[0].model = "another-model".into(),
            "AI account" => {
                settings.agents[0].ai_account.as_mut().unwrap().account_id = "44".into()
            }
            "Agent replacement" => {
                settings.repositories[0].assignments[0].agent_id = settings.agents[1].id.clone();
            }
            "doctrine body" => settings.doctrines[0].body = "Changed own doctrine.".into(),
            "doctrine order" => settings.agents[0].doctrines.as_mut().unwrap().reverse(),
            "doctrine selection" => settings.agents[0].doctrines = Some(vec![]),
            "preset body" => settings.presets[0].body = "Changed preset instructions.".into(),
            "repository prompt" => {
                settings.repositories[0].review_preset = None;
                settings.repositories[0].overrides.prompt = Some("Changed repository lens.".into());
            }
            "publication gate" => settings.defaults.automatic_comment_publication = true,
            "comment" => settings.repositories[0].assignments[0].comment = false,
            "approve" | "merge" => {
                settings.repositories[0].assignments[0].actions = Some(
                    serde_json::from_value(json!({
                        "approve":true,"merge":change == "merge"
                    }))
                    .unwrap(),
                );
            }
            "primary" => {
                settings.repositories[0].primary_assignment_id =
                    Some(settings.repositories[0].assignments[1].id.clone());
            }
            "repository disabled" => settings.repositories[0].enabled = false,
            "repository account" => {
                settings.repositories[0].provider_account_id = Some("44".into());
            }
            "repository identity" => {
                settings.repositories[0].provider_repository_id = Some("200".into());
            }
            "repository name" => settings.repositories[0].name = "example/other".into(),
            "repository provider" => {
                settings.repositories[0].provider = pr_sniper_lib::storage::ProviderId::AzureDevops;
            }
            "watched authors" => settings.repositories[0].watched_authors.clear(),
            "reviewer trigger" => settings.defaults.reviewer_assignment = false,
            "assignment removed" => {
                settings.repositories[0].assignments.remove(0);
                settings.repositories[0].primary_assignment_id = None;
            }
            _ => unreachable!(),
        }
        store.save_settings(&settings).unwrap();
        let action_metadata_only = matches!(
            change,
            "publication gate" | "comment" | "approve" | "merge" | "primary"
        );
        assert_eq!(
            review::validate_execution_selection(&store, &run).is_err(),
            !action_metadata_only,
            "{change}: read-only execution and current provider authority are separate"
        );
        assert_eq!(store.load_reviews().unwrap()[0], run);
    }
}

#[test]
fn seven_way_fanout_successful_pass_dedupe_restart_and_next_scan_assignment_reconciliation() {
    let (fixture, store, mut monitor) = configured(7);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 7);
    assert_eq!(monitor.snapshot().len(), 1);
    assert_eq!(
        jobs.iter()
            .map(|j| j.work.as_ref().unwrap().id.clone())
            .collect::<HashSet<_>>()
            .len(),
        7
    );
    assert_eq!(
        jobs.iter()
            .map(|j| j.work.as_ref().unwrap().enqueue_order)
            .collect::<Vec<_>>(),
        (1..=7).collect::<Vec<_>>()
    );
    assert!(jobs
        .iter()
        .all(|j| j.work.as_ref().unwrap().pass_ordinal == 1));
    let settings = store.load_settings().unwrap();
    let runs = jobs
        .iter()
        .map(|job| completed(&settings, job))
        .collect::<Vec<_>>();
    store.save_reviews(&runs).unwrap();
    let review_bytes = std::fs::read(fixture.path().join("state/reviews.json")).unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 10);
    assert_eq!(store.load_queue().unwrap(), jobs);
    monitor = Monitor::restore(&store).unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 20);
    assert_eq!(store.load_queue().unwrap(), jobs);
    let mut settings = store.load_settings().unwrap();
    add_agent(&mut settings, 8);
    store.save_settings(&settings).unwrap();
    assert!(monitor
        .prepare_checks(&store, NOW + 30, false)
        .unwrap()
        .is_empty());
    assert_eq!(store.load_queue().unwrap().len(), 7);
    let due = store
        .load_monitoring_state()
        .unwrap()
        .global_scan
        .unwrap()
        .next_run;
    let ticket = monitor
        .prepare_checks(&store, due, false)
        .unwrap()
        .remove(0);
    monitor
        .finish(&store, ticket, Ok(result(vec![pull('a')])), due + 1)
        .unwrap();
    let added = store.load_queue().unwrap();
    assert_eq!(added.len(), 8);
    assert_eq!(&added[..7], &jobs);
    assert_eq!(
        added[7].work.as_ref().unwrap().trigger,
        WorkTrigger::AssignmentAdded
    );
    assert_eq!(added[7].work.as_ref().unwrap().pass_ordinal, 1);
    assert_eq!(added[7].work.as_ref().unwrap().enqueue_order, 8);
    let snapshot =
        serde_json::to_value(queue::snapshot(&store, monitor.snapshot()).unwrap()).unwrap();
    assert_eq!(snapshot["reviews"].as_array().unwrap().len(), 8);
    assert_eq!(
        snapshot["reviews"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["run"]["operation"]["state"] == "completed")
            .count(),
        7
    );
    scan(&mut monitor, &store, vec![pull('b')], due + 10);
    let next = store.load_queue().unwrap();
    assert_eq!(next.len(), 16);
    assert!(next[..8]
        .iter()
        .all(|j| j.waiting == monitoring::WAITING_SUPERSEDED));
    assert!(next[8..]
        .iter()
        .all(|j| j.work.as_ref().unwrap().iteration == 2
            && j.work.as_ref().unwrap().pass_ordinal == 2));
    assert!(review::validate_execution_selection(&store, &runs[0]).is_err());
    assert_eq!(
        std::fs::read(fixture.path().join("state/reviews.json")).unwrap(),
        review_bytes
    );
}

#[test]
fn assignment_added_during_a_scan_waits_for_the_next_scan_snapshot() {
    let (_fixture, store, mut monitor) = configured(1);
    let ticket = monitor.prepare_checks(&store, NOW, true).unwrap().remove(0);
    let mut settings = store.load_settings().unwrap();
    add_agent(&mut settings, 2);
    store.save_settings(&settings).unwrap();
    monitor
        .finish(&store, ticket, Ok(result(vec![pull('a')])), NOW + 1)
        .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 1);
    assert!(monitor
        .prepare_checks(&store, NOW + 2, false)
        .unwrap()
        .is_empty());
    scan(&mut monitor, &store, vec![pull('a')], NOW + 3);
    assert_eq!(store.load_queue().unwrap().len(), 2);
}

#[test]
fn retries_do_not_allocate_another_normal_pass_or_change_fifo_metadata() {
    let (_fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let job = store.load_queue().unwrap().remove(0);
    let mut operation = JobOperation::review(&job, NOW + 2);
    operation.begin_attempt(NOW + 2).unwrap();
    let operation_id = operation.id.clone();
    operation.fail(&review::Failure::timeout().monitoring(), NOW + 3);
    operation
        .begin_attempt(operation.next_attempt_at.unwrap())
        .unwrap();
    assert_eq!(operation.id, operation_id);
    assert_eq!(operation.attempt_count, 2);
    let mut run = completed(&store.load_settings().unwrap(), &job);
    run.operation = operation;
    run.result = None;
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 20);
    assert_eq!(store.load_queue().unwrap(), vec![job]);
    assert_eq!(store.load_reviews().unwrap()[0].operation.attempt_count, 2);
    assert_eq!(
        store.load_queue().unwrap()[0]
            .work
            .as_ref()
            .unwrap()
            .pass_ordinal,
        1
    );
    assert_eq!(
        store.load_queue().unwrap()[0]
            .work
            .as_ref()
            .unwrap()
            .enqueue_order,
        1
    );
}

#[test]
fn a_filter_edit_discards_an_inflight_read_then_resumes_on_the_next_global_scan() {
    let (_fixture, store, mut monitor) = configured(1);
    let ticket = monitor.prepare_checks(&store, NOW, true).unwrap().remove(0);
    let mut settings = store.load_settings().unwrap();
    settings.defaults.reviewer_assignment = false;
    store.save_settings(&settings).unwrap();
    assert!(monitor
        .finish(&store, ticket, Ok(result(vec![pull('a')])), NOW + 1)
        .is_err());
    assert!(store.load_queue().unwrap().is_empty());
    let due = store
        .load_monitoring_state()
        .unwrap()
        .global_scan
        .unwrap()
        .next_run;
    let ticket = monitor
        .prepare_checks(&store, due, false)
        .unwrap()
        .remove(0);
    monitor
        .finish(&store, ticket, Ok(result(vec![pull('a')])), due + 1)
        .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 1);
}

#[test]
fn older_reviewer_admission_is_sticky_and_automatic_without_granting_publication() {
    let (_fixture, store, mut monitor) = configured(1);
    let mut observed = pull('a');
    observed.number = 5;
    observed.author.as_mut().unwrap().id = "44".into();
    observed.requested_reviewers.push(Identity {
        id: "22".into(),
        login: "acting-account".into(),
    });
    scan(&mut monitor, &store, vec![observed.clone()], NOW);
    let original = store.load_queue().unwrap().remove(0);
    assert_eq!(original.waiting, monitoring::WAITING_AI_CAPACITY);
    let mut settings = store.load_settings().unwrap();
    assert!(
        monitoring::review_policy(&settings, &original, Some(&observed))
            .unwrap()
            .automatic_agent_start
    );
    assert!(monitoring::review_policy(&settings, &original, Some(&observed)).is_ok());
    let run = completed(&settings, &original);
    let gate = publication::evaluate_review_gate(
        &settings,
        &run,
        Some(&original),
        &observed,
        publication::GatePermissions {
            active: true,
            can_comment: true,
            cancelled: false,
            automatic: false,
            confirmed: true,
        },
    );
    assert!(gate.stop.is_none());
    settings.defaults.reviewer_assignment = false;
    settings.repositories[0].watched_authors[0].id = "55".into();
    store.save_settings(&settings).unwrap();
    observed.requested_reviewers.clear();
    scan(&mut monitor, &store, vec![observed.clone()], NOW + 10);
    let current = store.load_queue().unwrap().remove(0);
    assert_eq!(current.work, original.work);
    assert!(!current.requested_reviewer);
    assert!(current.work.as_ref().unwrap().admission.requested_reviewer);
    assert!(monitoring::review_policy(&settings, &current, Some(&observed)).is_ok());
    assert!(!current.watched_author);
    let accounts = BTreeMap::from([(
        "22".into(),
        monitoring::AccountAvailability {
            login: "acting-account".into(),
            connected: false,
        },
    )]);
    monitor
        .synchronize_configuration(&store, &accounts, NOW + 20)
        .unwrap();
    assert!(monitoring::review_policy(&settings, &store.load_queue().unwrap()[0], None).is_err());
    assert_eq!(store.load_queue_state().unwrap().tracked.len(), 1);
}

#[test]
fn missing_observation_is_not_terminal_and_same_head_reopen_has_a_fresh_iteration() {
    for lifecycle in [Lifecycle::Closed, Lifecycle::Merged] {
        let (_fixture, store, mut monitor) = configured(1);
        scan(&mut monitor, &store, vec![pull('a')], NOW);
        let original = store.load_queue().unwrap().remove(0);
        scan(&mut monitor, &store, vec![], NOW + 10);
        assert_eq!(store.load_queue().unwrap()[0], original);
        assert_eq!(
            store.load_queue_state().unwrap().tracked[0].lifecycle,
            Lifecycle::Open
        );
        let mut terminal = pull('a');
        terminal.state = lifecycle.clone();
        scan(&mut monitor, &store, vec![terminal], NOW + 20);
        let snapshot = queue::snapshot(&store, monitor.snapshot()).unwrap();
        assert_eq!(
            snapshot.items[0].state,
            if lifecycle == Lifecycle::Merged {
                queue::State::Merged
            } else {
                queue::State::Closed
            }
        );
        assert!(monitoring::review_policy(
            &store.load_settings().unwrap(),
            &store.load_queue().unwrap()[0],
            None
        )
        .is_err());
        monitor = Monitor::restore(&store).unwrap();
        let ticket = monitor
            .prepare_checks(&store, NOW + 30, true)
            .unwrap()
            .remove(0);
        assert!(
            ticket.tracked.is_empty(),
            "Verified terminal PRs need no repeated detail reads."
        );
        monitor
            .finish(&store, ticket, Ok(result(vec![pull('a')])), NOW + 31)
            .unwrap();
        let jobs = store.load_queue().unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].head_sha, jobs[1].head_sha);
        assert_ne!(
            jobs[0].work.as_ref().unwrap().iteration_id,
            jobs[1].work.as_ref().unwrap().iteration_id
        );
        assert_eq!(
            jobs[1].work.as_ref().unwrap().trigger,
            WorkTrigger::Reopened
        );
        assert_eq!(jobs[1].work.as_ref().unwrap().pass_ordinal, 2);
        assert_eq!(
            queue::snapshot(&store, monitor.snapshot())
                .unwrap()
                .items
                .len(),
            2
        );
    }
}

#[test]
fn closed_iteration_stays_closed_after_same_head_reopen_and_merge() {
    let (_fixture, store, mut monitor) = configured(2);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let original = store.load_queue().unwrap();
    let mut closed = pull('a');
    closed.state = Lifecycle::Closed;
    scan(&mut monitor, &store, vec![closed], NOW + 10);
    let closed_jobs = store.load_queue().unwrap();
    assert!(closed_jobs
        .iter()
        .all(|job| job.waiting == monitoring::WAITING_CLOSED));
    scan(&mut monitor, &store, vec![pull('a')], NOW + 20);
    let reopened = store.load_queue().unwrap();
    assert_eq!(reopened.len(), 4);
    assert_eq!(&reopened[..2], closed_jobs.as_slice());
    for (old, new) in original.iter().zip(&reopened[2..]) {
        let old_work = old.work.as_ref().unwrap();
        let new_work = new.work.as_ref().unwrap();
        assert_eq!(old.head_sha, new.head_sha);
        assert_ne!(old_work.id, new_work.id);
        assert_ne!(old_work.iteration_id, new_work.iteration_id);
        assert_ne!(old_work.item_id, new_work.item_id);
        assert_eq!(new_work.iteration, 2);
        assert_eq!(new_work.pass_ordinal, 2);
        assert_eq!(new_work.trigger, WorkTrigger::Reopened);
    }
    let mut merged = pull('a');
    merged.state = Lifecycle::Merged;
    scan(&mut monitor, &store, vec![merged], NOW + 30);
    monitor = Monitor::restore(&store).unwrap();
    let terminal = store.load_queue().unwrap();
    assert_eq!(&terminal[..2], closed_jobs.as_slice());
    for (before, after) in reopened[2..].iter().zip(&terminal[2..]) {
        assert_eq!(after.waiting, monitoring::WAITING_MERGED);
        assert_eq!(after.work, before.work);
        assert!(monitoring::review_policy(&store.load_settings().unwrap(), after, None).is_err());
    }
    let snapshot = queue::snapshot(&store, monitor.snapshot()).unwrap();
    assert_eq!(snapshot.items.len(), 2);
    for (job, state) in [
        (&closed_jobs[0], queue::State::Closed),
        (&terminal[2], queue::State::Merged),
    ] {
        let item = snapshot
            .items
            .iter()
            .find(|item| item.id == queue::item_id(job))
            .unwrap();
        assert_eq!(item.state, state);
    }
}

#[test]
fn monitoring_state_write_failure_cannot_duplicate_an_already_committed_reopen() {
    let (fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let mut closed = pull('a');
    closed.state = Lifecycle::Closed;
    scan(&mut monitor, &store, vec![closed], NOW + 10);
    let ticket = monitor
        .prepare_checks(&store, NOW + 20, true)
        .unwrap()
        .remove(0);
    let blocked = fixture.path().join("state/monitoring.json.tmp");
    std::fs::create_dir(&blocked).unwrap();
    assert!(monitor
        .finish(&store, ticket, Ok(result(vec![pull('a')])), NOW + 21)
        .is_err());
    assert_eq!(store.load_queue().unwrap().len(), 2);
    let work = store.load_queue().unwrap()[1].work.clone();
    std::fs::remove_dir(blocked).unwrap();
    monitor = Monitor::restore(&store).unwrap();
    let ticket = monitor
        .prepare_checks(&store, NOW + 22, false)
        .unwrap()
        .remove(0);
    monitor
        .finish(&store, ticket, Ok(result(vec![pull('a')])), NOW + 23)
        .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 2);
    assert_eq!(store.load_queue().unwrap()[1].work, work);
}

#[test]
fn legacy_success_and_publication_receipts_keep_their_keys_and_old_destinations() {
    let (fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let mut legacy = store.load_queue().unwrap().remove(0);
    legacy.work = None;
    let settings = store.load_settings().unwrap();
    let run = completed(&settings, &legacy);
    let legacy_id = queue::item_id(&legacy);
    store.save_queue_selection(Some(&legacy_id)).unwrap();
    std::fs::write(
        fixture.path().join("state/queue.json"),
        serde_json::to_vec(&vec![legacy]).unwrap(),
    )
    .unwrap();
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    let mut publication = Publication::new(run.clone(), false, true, NOW + 1).unwrap();
    publication.receipts.push(Receipt {
        review_id: "123".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["456".into()],
    });
    store
        .save_publications(std::slice::from_ref(&publication))
        .unwrap();
    let bytes = std::fs::read(fixture.path().join("state/reviews.json")).unwrap();
    let mut settings = settings;
    settings.repositories[0].watched_authors[0].id = "55".into();
    settings.defaults.reviewer_assignment = false;
    store.save_settings(&settings).unwrap();
    monitor = Monitor::restore(&store).unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 10);
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].work.as_ref().unwrap().id, run.key);
    assert!(run.matches_job(&jobs[0]));
    assert_eq!(store.load_publications().unwrap()[0], publication);
    assert_eq!(
        std::fs::read(fixture.path().join("state/reviews.json")).unwrap(),
        bytes
    );
    assert_eq!(
        queue::destination(&store, &legacy_id, None)
            .unwrap()
            .as_str(),
        "https://github.com/example/repo/pull/101"
    );
    assert_eq!(
        store.load_queue_selection().unwrap().as_deref(),
        Some(legacy_id.as_str())
    );
    let mut retry = run;
    retry.operation = JobOperation::review(&jobs[0], NOW + 30);
    assert!(publication.conflicts_with(&retry));
}

#[test]
fn legacy_filter_key_duplicates_adopt_the_existing_publication_owner_not_a_replacement_batch() {
    let (fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let mut old = store.load_queue().unwrap().remove(0);
    old.work = None;
    let mut settings = store.load_settings().unwrap();
    let old_review = completed(&settings, &old);
    let pending = Publication::new(old_review.clone(), false, true, NOW + 1).unwrap();
    let mut recent = old.clone();
    recent.trigger_policy = "[[\"11\",\"55\"],true]".into();
    recent.detected_at = NOW + 10;
    settings.repositories[0]
        .watched_authors
        .push(pr_sniper_lib::policy::WatchedIdentity {
            id: "55".into(),
            login: "another-author".into(),
        });
    store.save_settings(&settings).unwrap();
    let recent_review = completed(&settings, &recent);
    store
        .save_reviews(&[old_review.clone(), recent_review.clone()])
        .unwrap();
    store
        .save_publications(std::slice::from_ref(&pending))
        .unwrap();
    std::fs::write(
        fixture.path().join("state/queue.json"),
        serde_json::to_vec(&vec![old, recent]).unwrap(),
    )
    .unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 20);
    let jobs = store.load_queue().unwrap();
    let active = jobs
        .iter()
        .filter(|job| job.work.is_some())
        .collect::<Vec<_>>();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].work.as_ref().unwrap().id, old_review.key);
    assert_eq!(jobs[1].waiting, monitoring::WAITING_SUPERSEDED);
    let snapshot =
        serde_json::to_value(queue::snapshot(&store, monitor.snapshot()).unwrap()).unwrap();
    let replacement = snapshot["publications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["review_operation_id"] == recent_review.operation.id)
        .unwrap();
    assert!(replacement["blocked"].as_str().is_some());
    assert_eq!(store.load_publications().unwrap(), vec![pending]);
}

#[test]
fn a_repository_ticket_keeps_all_existing_owned_thread_polling_origins() {
    let (_fixture, store, mut monitor) = configured(7);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let settings = store.load_settings().unwrap();
    let publications = store
        .load_queue()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let mut publication =
                Publication::new(completed(&settings, job), false, true, NOW + 1).unwrap();
            publication.receipts.push(Receipt {
                review_id: (100 + i).to_string(),
                state: RemoteState::Commented,
                comment_ids: vec![(200 + i).to_string()],
            });
            publication
        })
        .collect::<Vec<_>>();
    store.save_publications(&publications).unwrap();
    let ticket = monitor
        .prepare_checks(&store, NOW + 20, true)
        .unwrap()
        .remove(0);
    assert!(ticket.assignment_id.is_none());
    assert_eq!(
        follow_up::polling_origins(&store, &ticket, &[pull('a')])
            .unwrap()
            .len(),
        7
    );
    let mut settings = settings;
    settings.repositories[0].assignments.remove(0);
    store.save_settings(&settings).unwrap();
    assert_eq!(
        follow_up::polling_origins(&store, &ticket, &[pull('a')])
            .unwrap()
            .len(),
        6
    );
}

struct LifecycleProvider {
    mode: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Transport for LifecycleProvider {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.calls.lock().unwrap().push(path.into());
        if path.contains("?state=open") {
            if self.mode == "partial_list" && path.ends_with("&page=1") {
                return Ok(Response {
                    status: 200,
                    headers: BTreeMap::from([("link".into(),
                        "<https://api.github.com/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\"".into())]),
                    body: b"[{}]".to_vec(),
                });
            }
            return Ok(Response {
                status: 200,
                headers: BTreeMap::new(),
                body: b"[]".to_vec(),
            });
        }
        assert_eq!(path, "/repos/example/repo/pulls/101");
        if self.mode == "unavailable" {
            return Err(ConnectionError::Network);
        }
        if self.mode == "404" {
            return Ok(Response {
                status: 404,
                headers: BTreeMap::new(),
                body: b"{}".to_vec(),
            });
        }
        let mut value = json!({
            "id":9,"number":101,"title":"Review","user":{"id":11,"login":"author"},
            "requested_reviewers":[],"requested_teams":[],
            "state":if self.mode == "open" { "open" } else { "closed" },
            "merged":self.mode == "merged","draft":false,
            "head":{"sha":"a".repeat(40),"repo":{"id":100}},
            "base":{"sha":"f".repeat(40),"repo":{"id":100}},
            "updated_at":"2026-09-30T00:00:00Z"
        });
        if self.mode == "incomplete" || self.mode == "invalid_date" {
            value.as_object_mut().unwrap().remove("merged");
        }
        if self.mode == "invalid_date" {
            value["merged_at"] = json!("not a timestamp");
        }
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}

#[test]
fn provider_lifecycle_requires_explicit_fetch_and_missing_or_failed_reads_never_close_work() {
    for mode in [
        "open",
        "closed",
        "merged",
        "404",
        "unavailable",
        "incomplete",
        "invalid_date",
        "partial_list",
    ] {
        let (fixture, store, mut monitor) = configured(1);
        scan(&mut monitor, &store, vec![pull('a')], NOW);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = GithubClient::new(LifecycleProvider {
            mode,
            calls: calls.clone(),
        });
        let ticket = monitor
            .prepare_checks(&store, NOW + 10, true)
            .unwrap()
            .remove(0);
        assert_eq!(ticket.tracked, vec![("9".into(), 101)]);
        let before = std::fs::read(fixture.path().join("state/queue.json")).unwrap();
        let observed = client
            .poll_tracked_pull_requests(&result(vec![]).connection.repository, &ticket.tracked);
        assert_eq!(calls.lock().unwrap().len(), 2);
        if [
            "404",
            "unavailable",
            "incomplete",
            "invalid_date",
            "partial_list",
        ]
        .contains(&mode)
        {
            assert!(observed.is_err());
            assert!(monitor
                .finish(&store, ticket, observed.map(result), NOW + 11)
                .is_err());
            assert_eq!(
                std::fs::read(fixture.path().join("state/queue.json")).unwrap(),
                before
            );
            assert_eq!(
                store.load_queue_state().unwrap().tracked[0].lifecycle,
                Lifecycle::Open
            );
        } else {
            let observed = observed.unwrap();
            assert_eq!(
                observed[0].state,
                match mode {
                    "closed" => Lifecycle::Closed,
                    "merged" => Lifecycle::Merged,
                    _ => Lifecycle::Open,
                }
            );
            monitor
                .finish(&store, ticket, Ok(result(observed)), NOW + 11)
                .unwrap();
        }
    }
}

#[test]
fn legacy_assignment_retry_health_collapses_without_resetting_the_operation_or_backoff() {
    let (_fixture, store, mut monitor) = configured(1);
    let ticket = monitor.prepare_checks(&store, NOW, true).unwrap().remove(0);
    assert!(monitor
        .finish(
            &store,
            ticket,
            Err(ConnectionError::RateLimitedAfter(60)),
            NOW + 1
        )
        .is_err());
    let mut state = store.load_monitoring_state().unwrap();
    let mut health = state.health.remove(REPO).unwrap();
    let operation = health.operation.clone().unwrap();
    health.assignment_id = Some("cccccccc-cccc-4ccc-8ccc-000000000001".into());
    health.schedule_key = "interval:2:UTC".into();
    health.scan_assignments.clear();
    state.global_scan = None;
    state.health.insert(
        format!("{REPO}:assignment:cccccccc-cccc-4ccc-8ccc-000000000001"),
        health,
    );
    store.save_monitoring_state(&state).unwrap();
    monitor = Monitor::restore(&store).unwrap();
    assert!(monitor
        .prepare_checks(&store, NOW + 2, true)
        .unwrap()
        .is_empty());
    let health = monitor.snapshot();
    assert_eq!(health.len(), 1);
    assert_eq!(health[0].schedule_key, "cron:*/15 * * * *:UTC");
    assert_eq!(health[0].operation.as_ref().unwrap(), &operation);
    assert!(!store
        .load_monitoring_state()
        .unwrap()
        .operations
        .contains_key(&operation.id));
    let retry = monitor
        .prepare_checks(&store, operation.next_attempt_at.unwrap(), false)
        .unwrap();
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].assignments.len(), 1);
    let active = monitor.snapshot()[0].operation.clone().unwrap();
    assert_eq!(active.id, operation.id);
    assert_eq!(active.retry_deadline, operation.retry_deadline);
    assert_eq!(active.attempt_count, 2);
}

#[test]
fn tracking_with_zero_assignments_survives_restart_and_creates_work_only_at_a_later_scan() {
    let (_fixture, store, mut monitor) = configured(0);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    assert!(store.load_queue().unwrap().is_empty());
    assert_eq!(store.load_queue_state().unwrap().tracked.len(), 1);
    let mut settings = store.load_settings().unwrap();
    add_agent(&mut settings, 1);
    settings.repositories[0].watched_authors[0].id = "55".into();
    store.save_settings(&settings).unwrap();
    monitor = Monitor::restore(&store).unwrap();
    assert!(monitor
        .prepare_checks(&store, NOW + 10, false)
        .unwrap()
        .is_empty());
    scan(&mut monitor, &store, vec![pull('a')], NOW + 20);
    assert_eq!(store.load_queue().unwrap().len(), 1);
    assert_eq!(
        store.load_queue().unwrap()[0].waiting,
        monitoring::WAITING_AI_CAPACITY
    );
}

#[test]
fn persisted_review_and_publication_evidence_prevents_replay_when_the_queue_file_is_missing() {
    for legacy in [false, true] {
        let (fixture, store, mut monitor) = configured(1);
        scan(&mut monitor, &store, vec![pull('a')], NOW);
        let mut job = store.load_queue().unwrap().remove(0);
        if legacy {
            job.work = None;
        }
        let run = completed(&store.load_settings().unwrap(), &job);
        let publication = Publication::new(run.clone(), false, true, NOW + 1).unwrap();
        store
            .save_publications(std::slice::from_ref(&publication))
            .unwrap();
        std::fs::remove_file(fixture.path().join("state/queue.json")).unwrap();
        let ticket = monitor
            .prepare_checks(&store, NOW + 10, true)
            .unwrap()
            .remove(0);
        assert_eq!(ticket.tracked, vec![("9".into(), 101)]);
        monitor
            .finish(&store, ticket, Ok(result(vec![pull('a')])), NOW + 11)
            .unwrap();
        let restored = store.load_queue().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].work.as_ref().unwrap().id, run.key);
        let snapshot =
            serde_json::to_value(queue::snapshot(&store, monitor.snapshot()).unwrap()).unwrap();
        assert_eq!(
            snapshot["reviews"][0]["run"]["operation"]["state"],
            "completed"
        );
        assert_eq!(snapshot["publications"].as_array().unwrap().len(), 1);
        assert_eq!(store.load_publications().unwrap(), vec![publication]);
    }
}

#[test]
fn a_legacy_missing_local_configuration_id_keeps_its_review_identity_on_adoption() {
    let (fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let mut job = store.load_queue().unwrap().remove(0);
    job.work = None;
    let mut run = completed(&store.load_settings().unwrap(), &job);
    job.configuration_id.clear();
    run.job = job.clone();
    run.key = review::key(&job, &run.assignment_id);
    store.save_reviews(std::slice::from_ref(&run)).unwrap();
    std::fs::write(
        fixture.path().join("state/queue.json"),
        serde_json::to_vec(&vec![job]).unwrap(),
    )
    .unwrap();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors[0].id = "55".into();
    store.save_settings(&settings).unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 10);
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(run.matches_job(&jobs[0]));
    assert_eq!(jobs[0].work.as_ref().unwrap().id, run.key);
    assert!(store.load_reviews().unwrap()[0]
        .job
        .configuration_id
        .is_empty());
}

#[test]
fn closure_after_an_unseen_push_still_marks_saved_work_terminal() {
    let (_fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let mut closed = pull('b');
    closed.state = Lifecycle::Closed;
    scan(&mut monitor, &store, vec![closed], NOW + 10);
    let snapshot = queue::snapshot(&store, monitor.snapshot()).unwrap();
    assert_eq!(snapshot.items[0].state, queue::State::Closed);
    assert_eq!(snapshot.jobs[0].head_sha, "a".repeat(40));
    assert_eq!(snapshot.tracked[0].head_sha, "b".repeat(40));
}

#[test]
fn closure_after_an_unseen_push_also_terminates_superseded_heads() {
    let (_fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    scan(&mut monitor, &store, vec![pull('b')], NOW + 10);
    assert_eq!(
        store.load_queue().unwrap()[0].waiting,
        monitoring::WAITING_SUPERSEDED
    );
    let mut closed = pull('c');
    closed.state = Lifecycle::Closed;
    scan(&mut monitor, &store, vec![closed], NOW + 20);
    let snapshot = queue::snapshot(&store, monitor.snapshot()).unwrap();
    assert_eq!(snapshot.items.len(), 2);
    assert!(snapshot
        .items
        .iter()
        .all(|item| item.state == queue::State::Closed));
    assert!(snapshot
        .jobs
        .iter()
        .all(|job| job.waiting == monitoring::WAITING_CLOSED));
    assert_eq!(snapshot.tracked[0].head_sha, "c".repeat(40));
}

#[test]
fn legacy_global_interval_is_preserved_and_reported_instead_of_silently_retimed() {
    let (_fixture, store, mut monitor) = configured(1);
    let mut settings = store.load_settings().unwrap();
    settings.defaults.schedule = monitoring_schedule();
    store.save_settings(&settings).unwrap();
    assert!(monitor
        .prepare_checks(&store, NOW, true)
        .unwrap()
        .is_empty());
    assert_eq!(
        monitor.snapshot()[0].last_failure.as_deref(),
        Some("invalid_global_cron")
    );
    assert_eq!(store.load_settings().unwrap(), settings);
}

fn monitoring_schedule() -> pr_sniper_lib::policy::Schedule {
    pr_sniper_lib::policy::Schedule::Interval {
        minutes: 7,
        timezone: "America/New_York".into(),
    }
}

#[test]
fn a_replaced_assignment_gets_new_work_without_reusing_the_previous_agents_pass() {
    let (_fixture, store, mut monitor) = configured(1);
    scan(&mut monitor, &store, vec![pull('a')], NOW);
    let old = store.load_queue().unwrap().remove(0);
    let mut settings = store.load_settings().unwrap();
    add_agent(&mut settings, 2);
    settings.repositories[0].assignments.pop();
    settings.repositories[0].assignments[0].agent_id = settings.agents[1].id.clone();
    store.save_settings(&settings).unwrap();
    scan(&mut monitor, &store, vec![pull('a')], NOW + 10);
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].waiting, monitoring::WAITING_ASSIGNMENT_REMOVED);
    assert_ne!(
        old.work.as_ref().unwrap().id,
        jobs[1].work.as_ref().unwrap().id
    );
    assert_eq!(jobs[1].work.as_ref().unwrap().pass_ordinal, 1);
    assert_eq!(
        queue::snapshot(&store, monitor.snapshot()).unwrap().items[0].state,
        queue::State::Queued
    );
}
