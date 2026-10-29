use super::*;
use crate::{
    capacity,
    monitoring::OperationState,
    storage::{ResourceEdit, Store},
};
use serde_json::json;
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
};

#[test]
fn duplicate_mutations_after_restart_return_receipts_not_reexecution() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    assert!(
        operations::begin_request(&store, "request", 1000, "same", 1000)
            .unwrap()
            .is_none()
    );
    let reopened = Store::new(root.path().into());
    assert_eq!(
        operations::begin_request(&reopened, "request", 1000, "same", 1001)
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        "outcome_unknown"
    );
    let outcome = json!({"state":"queued"});
    operations::finish_request(&reopened, "request", Ok(outcome.clone())).unwrap();
    assert_eq!(
        operations::begin_request(&store, "request", 1000, "same", 1002)
            .unwrap()
            .unwrap()
            .unwrap(),
        outcome
    );
    assert_eq!(
        operations::begin_request(&store, "request", 1000, "changed", 1002)
            .unwrap_err()
            .code,
        "request_id_conflict"
    );
    assert_eq!(
        operations::begin_request(&store, "request", 1000, "same", 1301)
            .unwrap_err()
            .code,
        "expired_request"
    );
}

#[test]
fn incomplete_watch_configuration_survives_restart_and_cannot_dispatch_defaults() {
    let (root, store) = capacity::tests::fixture(1, 1, true);
    let repo = store.load_settings().unwrap().repositories[0].id.clone();
    work::hold(&store, &repo, 1, true).unwrap();
    let reopened = Store::new(root.path().into());
    assert!(capacity::Coordinator::default()
        .dispatch(&reopened, 100)
        .unwrap()
        .dispatched
        .is_empty());
    assert_eq!(
        work::result(&reopened, &repo, 1, 0).unwrap().state,
        "waiting"
    );
    work::hold(&reopened, &repo, 1, false).unwrap();
    assert_eq!(
        capacity::Coordinator::default()
            .dispatch(&reopened, 100)
            .unwrap()
            .dispatched
            .len(),
        1
    );
}

#[test]
fn prioritization_and_explicit_rerun_do_not_bypass_wait() {
    let (_root, store) = capacity::tests::fixture(1, 1, true);
    let settings = store.load_settings().unwrap();
    let job = store.load_queue().unwrap().remove(0);
    let mut run = crate::review::host::request(&store, "normal-1", true, 100).unwrap();
    run.operation.state = OperationState::Completed;
    store.save_reviews(&[run]).unwrap();
    let binding = crate::retention::Binding::job(&job);
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "iteration-1",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 100,
        },
    );
    let snapshot = crate::github::state::PrSnapshot {
        state: "open".into(),
        required_checks: vec!["build".into()],
        ..Default::default()
    }
    .normalize();
    assert_eq!(
        states.wait(
            &binding,
            &snapshot.hash(),
            crate::pr_state::ExternalWait {
                reason: crate::exits::WaitReason::Ci,
                state_seq: 1,
                assignment_id: job.assignment_id.clone().unwrap(),
                agent_id: settings.agents[0].id.clone(),
                snapshot,
            },
        ),
        crate::pr_state::Consumed::Consumed
    );
    store.save_pr_states(&states).unwrap();
    assert_eq!(
        prioritize(&store, &settings.repositories[0].id, 1, 101).unwrap(),
        0
    );
    assert!(request_again(&store, &settings.repositories[0].id, 1, 101).is_err());
    assert_eq!(
        work::result(&store, &settings.repositories[0].id, 1, 0)
            .unwrap()
            .state,
        "waiting"
    );
    assert_eq!(store.load_queue().unwrap().len(), 1);
    assert_eq!(store.load_pr_states().unwrap(), states);
}

#[test]
fn shipped_preset_dispatches_teammates_before_maintainer() {
    let (_root, store) = capacity::tests::fixture(1, 4, true);
    let before = store.load_settings().unwrap();
    let result = preset::apply(&store, &revision(&before), "201", "fixture-model").unwrap();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments =
        serde_json::from_value(result["assignments"].clone()).unwrap();
    settings.repositories[0].primary_assignment_id =
        Some(result["primary_assignment_id"].as_str().unwrap().into());
    store.save_settings(&settings).unwrap();
    let template = store.load_queue().unwrap().remove(0);
    let jobs: Vec<_> = settings.repositories[0]
        .assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut job = template.clone();
            job.assignment_id = Some(a.id.clone());
            let work = job.work.as_mut().unwrap();
            work.agent_id = a.agent_id.clone();
            work.id = format!("preset-{i}");
            work.enqueue_order = i as u64 + 1;
            job
        })
        .collect();
    store.save_queue(&jobs).unwrap();
    let batch = capacity::Coordinator::default()
        .dispatch(&store, 100)
        .unwrap();
    assert_eq!(batch.dispatched.len(), 2);
    let runs = store.load_reviews().unwrap();
    assert_eq!(
        runs.iter()
            .map(|r| r.selection.agent.name.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["SOLID", "Security"])
    );
    assert!(!runs.iter().any(|r| r
        .selection
        .configuration
        .as_ref()
        .unwrap()
        .authority
        .primary));
}

#[test]
fn control_endpoint_uses_private_platform_storage() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("control");
    let _server = transport::Server::bind(&directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.join("endpoint.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(transport::Server::bind(&directory).is_err());
    }
    #[cfg(windows)]
    {
        crate::windows_permissions::assert_private(&directory, true);
        crate::windows_permissions::assert_private(&directory.join("endpoint.json"), false);
    }
}

#[test]
fn save_is_conflict_safe_and_grants_need_explicit_intent() {
    let (_root, store) = capacity::tests::fixture(1, 1, true);
    let before = store.load_settings().unwrap();
    let expected = before.repositories[0].clone();
    let mut changed = expected.clone();
    changed.assignments[0].actions = Some(crate::storage::ActionPermissions {
        reply: true,
        ..Default::default()
    });
    let edit = ResourceEdit::Repository {
        id: expected.id.clone(),
        expected: Some(Box::new(expected)),
        value: Some(Box::new(changed)),
    };
    let error =
        operations::save_checked(&store, &revision(&before), edit.clone(), false).unwrap_err();
    assert_eq!(error.code, "explicit_grant_required");
    assert_eq!(store.load_settings().unwrap(), before);
    let saved = operations::save_checked(&store, &revision(&before), edit.clone(), true).unwrap();
    assert!(
        saved.repositories[0].assignments[0]
            .actions
            .as_ref()
            .unwrap()
            .reply
    );
    assert_eq!(saved.agents, before.agents);
    let error = operations::save_checked(&store, &revision(&before), edit, true).unwrap_err();
    assert_eq!(error.code, "conflict");
}

#[test]
fn preset_is_additive_and_preserves_customized_components() {
    let (_root, store) = capacity::tests::fixture(1, 1, true);
    let before = store.load_settings().unwrap();
    let result = preset::apply(&store, &revision(&before), "201", "fixture-model").unwrap();
    let mut saved = store.load_settings().unwrap();
    assert_eq!(saved.agents.len(), before.agents.len() + 3);
    assert_eq!(saved.repositories, before.repositories);
    assert_eq!(saved.defaults, before.defaults);
    let primary = result["primary_assignment_id"].as_str().unwrap();
    let assignments: Vec<crate::storage::Assignment> =
        serde_json::from_value(result["assignments"].clone()).unwrap();
    assert_eq!(assignments[2].id, primary);
    assert!(assignments
        .iter()
        .all(|a| !a.comment && a.actions.is_none()));
    let maintained_id = assignments[2].agent_id.clone();
    saved
        .agents
        .iter_mut()
        .find(|a| a.id == maintained_id)
        .unwrap()
        .prompt = "My maintained prompt".into();
    store.save_settings(&saved).unwrap();
    preset::apply(&store, &revision(&saved), "999", "do-not-replace").unwrap();
    assert_eq!(store.load_settings().unwrap(), saved);
}

fn readonly_assignment_fixture() -> (
    tempfile::TempDir,
    Store,
    crate::storage::Settings,
    crate::storage::Repository,
) {
    let (root, store) = capacity::tests::fixture(1, 1, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments.clear();
    settings.repositories[0].primary_assignment_id = None;
    settings.repositories[0].enabled = false;
    store.save_settings(&settings).unwrap();
    let mut repository = settings.repositories[0].clone();
    repository.enabled = true;
    repository.assignments.push(crate::storage::Assignment {
        id: uuid::Uuid::new_v4().to_string(),
        agent_id: settings.agents[0].id.clone(),
        schedule: None,
        comment: false,
        approve: false,
        actions: None,
        tools: None,
        rubber_duck: None,
    });
    (root, store, settings, repository)
}

#[test]
fn new_readonly_assignment_tools_need_no_effect_grant_and_native_catalog_filters_effects() {
    use crate::exits::gates::{offered, Facts, GateInput, Grants};
    for requested in [
        vec![
            "read_changes",
            "read_source",
            "get_thread",
            "get_checks",
            "read_check_log",
            "list_tree",
            "search_code",
            "ready_for_human",
            "wait",
            "panic_exit",
        ],
        vec!["read_source", "ready_for_human", "panic_exit"],
        vec![
            "read_source",
            "ready_for_human",
            "panic_exit",
            "leave_comments",
            "approve_pull_request",
            "approve_and_merge",
            "submit_report",
            "rubber_duck",
        ],
    ] {
        let (_root, store, settings, mut repository) = readonly_assignment_fixture();
        repository.assignments[0].tools = Some(requested.iter().map(|s| s.to_string()).collect());
        let saved = operations::save_checked(
            &store,
            &revision(&settings),
            ResourceEdit::Repository {
                id: repository.id.clone(),
                expected: Some(Box::new(settings.repositories[0].clone())),
                value: Some(Box::new(repository)),
            },
            false,
        )
        .unwrap();
        let repo = &saved.repositories[0];
        let assignment = &repo.assignments[0];
        let authority = repo.assignment_authority(assignment);
        assert!(!authority.comment && !authority.reply && !authority.approve && !authority.merge);
        let tools = repo.assignment_tools(assignment);
        let input = GateInput {
            tools: tools.clone(),
            primary: authority.primary,
            grants: Grants::default(),
            review_drafts: false,
            capabilities: crate::provider::ProviderCapabilities::of(&repo.provider),
            facts: Facts::default(),
        };
        let exits: Vec<_> = offered(&input)
            .into_iter()
            .filter_map(|(exit, offer)| offer.is_offered().then_some(exit))
            .collect();
        let available = crate::review::tools::tool_set(
            &tools,
            repo.assignment_duck(assignment).is_some(),
            &exits,
        );
        assert!(available.iter().any(|t| t == "read_source"));
        for effect in [
            "leave_comments",
            "approve_pull_request",
            "approve_and_merge",
            "submit_report",
            "rubber_duck",
        ] {
            assert!(
                !available.iter().any(|t| t == effect),
                "{effect}: {available:?}"
            );
        }
        if requested.len() == 3 {
            // `lgtm` is the primary's own exit, never a configured tool; with no
            // grant it is a local hand-off.
            assert_eq!(
                available,
                ["read_source", "ready_for_human", "panic_exit", "lgtm"]
            );
        }
    }
}

#[test]
fn new_assignment_effect_flags_still_require_explicit_grant_changes() {
    for flag in ["comment", "reply", "approve", "merge", "include_self"] {
        let (_root, store, settings, mut repository) = readonly_assignment_fixture();
        let assignment = &mut repository.assignments[0];
        assignment.tools = Some(vec![
            "read_source".into(),
            "ready_for_human".into(),
            "panic_exit".into(),
        ]);
        if flag == "comment" {
            assignment.comment = true;
        } else {
            assignment.actions = Some(crate::storage::ActionPermissions {
                reply: flag == "reply",
                approve: matches!(flag, "approve" | "merge" | "include_self"),
                merge: matches!(flag, "merge" | "include_self"),
                include_self: flag == "include_self",
            });
        }
        let error = operations::save_checked(
            &store,
            &revision(&settings),
            ResourceEdit::Repository {
                id: repository.id.clone(),
                expected: Some(Box::new(settings.repositories[0].clone())),
                value: Some(Box::new(repository)),
            },
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, "explicit_grant_required", "{flag}");
        assert_eq!(store.load_settings().unwrap(), settings);
    }
}

#[test]
fn prioritization_persists_without_interrupting_or_bypassing_capacity() {
    let (root, store) = capacity::tests::fixture(3, 1, true);
    let coordinator = capacity::Coordinator::default();
    let batch = coordinator.dispatch(&store, 100).unwrap();
    assert_eq!(batch.dispatched.len(), 1);
    let running = store.load_reviews().unwrap();
    let repo = store.load_settings().unwrap().repositories[2].id.clone();
    assert_eq!(prioritize(&store, &repo, 3, 100).unwrap(), 1);
    assert_eq!(
        coordinator.dispatch(&store, 100).unwrap().dispatched.len(),
        0
    );
    assert_eq!(store.load_reviews().unwrap(), running);
    let reopened = Store::new(root.path().into());
    let candidates = capacity::candidates(&reopened, 100).unwrap();
    assert_eq!(candidates[0].key.id, "normal-3");
    assert!(!request_again(&reopened, &repo, 3, 100).unwrap());
    assert_eq!(reopened.load_queue().unwrap().len(), 3);
}

#[test]
fn request_override_changes_execution_not_saved_configuration_and_freezes_at_start() {
    let (_root, store) = capacity::tests::fixture(2, 1, true);
    let settings = store.load_settings().unwrap();
    let repo = &settings.repositories[0];
    let overrides = std::collections::BTreeMap::from([(
        repo.assignments[0].id.clone(),
        RequestOverride {
            agent_id: Some(settings.agents[1].id.clone()),
            doctrine_ids: Some(vec![]),
        },
    )]);
    work::apply_overrides(&store, &repo.id, 1, &overrides).unwrap();
    let coordinator = capacity::Coordinator::default();
    let batch = coordinator.dispatch(&store, 100).unwrap();
    assert_eq!(batch.dispatched.len(), 1);
    let run = store.load_reviews().unwrap().remove(0);
    assert_eq!(run.selection.agent.id, settings.agents[1].id);
    assert_eq!(
        run.selection.configuration.as_ref().unwrap().repository.id,
        repo.id
    );
    assert_eq!(store.load_settings().unwrap(), settings);
    assert!(work::apply_overrides(&store, &repo.id, 1, &overrides).is_err());
}

#[test]
fn pending_request_references_prevent_deleting_unassigned_agents_and_doctrines() {
    let (_root, store) = capacity::tests::fixture(2, 1, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories.truncate(1);
    let doctrine = crate::storage::Doctrine {
        id: uuid::Uuid::new_v4().to_string(),
        title: "Request only".into(),
        body: "Review failures.".into(),
    };
    settings.doctrines.push(doctrine.clone());
    store.save_settings(&settings).unwrap();
    let repo = &settings.repositories[0];
    work::apply_overrides(
        &store,
        &repo.id,
        1,
        &std::collections::BTreeMap::from([(
            repo.assignments[0].id.clone(),
            RequestOverride {
                agent_id: Some(settings.agents[1].id.clone()),
                doctrine_ids: Some(vec![doctrine.id.clone()]),
            },
        )]),
    )
    .unwrap();
    assert!(store
        .save_resource(ResourceEdit::Agent {
            id: settings.agents[1].id.clone(),
            expected: Some(settings.agents[1].clone()),
            value: None
        })
        .is_err());
    assert!(store
        .save_resource(ResourceEdit::Doctrine {
            id: doctrine.id.clone(),
            expected: Some(doctrine),
            value: None
        })
        .is_err());
    assert_eq!(store.load_settings().unwrap(), settings);
}

#[test]
fn review_again_keeps_completed_evidence_and_reuses_new_pending_pass() {
    let (_root, store) = capacity::tests::fixture(1, 1, true);
    let settings = store.load_settings().unwrap();
    let job = store.load_queue().unwrap().remove(0);
    let binding = crate::retention::Binding::job(&job);
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "iteration-1",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 100,
        },
    );
    states.consume(&binding, 1, "unchanged");
    store.save_pr_states(&states).unwrap();
    let mut run = crate::review::host::request(&store, "normal-1", true, 100).unwrap();
    run.operation.state = OperationState::Completed;
    store.save_reviews(&[run.clone()]).unwrap();
    assert!(request_again(&store, &settings.repositories[0].id, 1, 200).unwrap());
    assert!(!request_again(&store, &settings.repositories[0].id, 1, 201).unwrap());
    assert_eq!(store.load_reviews().unwrap(), vec![run]);
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_ne!(
        jobs[0].work.as_ref().unwrap().id,
        jobs[1].work.as_ref().unwrap().id
    );
    assert_eq!(jobs[1].work.as_ref().unwrap().pass_ordinal, 2);
    assert_eq!(jobs[1].work.as_ref().unwrap().state_seq, 2);
    // Only this explicit pass may publish anew on an already-published head.
    let record = store.load_pr_states().unwrap();
    assert_eq!(record.get(&binding).unwrap().review_again_seq, Some(2));
}

#[test]
fn review_item_again_starts_a_prioritized_pass_for_the_queue_item_only() {
    let (_root, store) = capacity::tests::fixture(2, 1, true);
    let job = store.load_queue().unwrap().remove(0);
    let item = crate::queue::item_id(&job);
    let binding = crate::retention::Binding::job(&job);
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "iteration-1",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 100,
        },
    );
    states.consume(&binding, 1, "unchanged");
    store.save_pr_states(&states).unwrap();
    let mut run = crate::review::host::request(&store, "normal-1", true, 100).unwrap();
    run.operation.state = OperationState::Completed;
    store.save_reviews(&[run.clone()]).unwrap();
    assert!(work::review_item_again(&store, "missing-item", 200).is_err());
    assert!(work::review_item_again(&store, &item, 200).unwrap());
    // The new pass is pending, so a second press changes nothing.
    let pending = store.load_queue_state().unwrap();
    assert!(!work::review_item_again(&store, &item, 201).unwrap());
    assert_eq!(store.load_queue_state().unwrap(), pending);
    assert_eq!(store.load_reviews().unwrap(), vec![run]);
    let jobs = pending.jobs;
    assert_eq!(jobs.len(), 3);
    let again = jobs.last().unwrap();
    assert_eq!(again.configuration_id, job.configuration_id);
    assert_eq!(crate::queue::item_id(again), item);
    assert_eq!(again.work.as_ref().unwrap().pass_ordinal, 2);
    assert_eq!(again.work.as_ref().unwrap().enqueue_order, 0);
    let other = jobs
        .iter()
        .find(|j| j.configuration_id != job.configuration_id)
        .unwrap();
    assert!(other.work.as_ref().unwrap().enqueue_order > 0);
    assert_eq!(
        store
            .load_pr_states()
            .unwrap()
            .get(&binding)
            .unwrap()
            .review_again_seq,
        Some(2)
    );
}

#[test]
fn review_item_again_puts_every_new_job_first_even_when_not_runnable() {
    let (_root, store) = capacity::tests::fixture(2, 2, true);
    let mut settings = store.load_settings().unwrap();
    let mut jobs = store.load_queue().unwrap();
    let primary = jobs[0].clone();
    // A designated primary plus one teammate on the first PR.
    settings.repositories[0].primary_assignment_id = primary.assignment_id.clone();
    let mut agent = serde_json::to_value(&settings.agents[0]).unwrap();
    agent["id"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-900000000001");
    agent["name"] = json!("Teammate");
    settings.agents.push(serde_json::from_value(agent).unwrap());
    let mut assignment = serde_json::to_value(&settings.repositories[0].assignments[0]).unwrap();
    assignment["id"] = json!("cccccccc-cccc-4ccc-8ccc-900000000001");
    assignment["agent_id"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-900000000001");
    assignment["tools"] = json!(["read_changes", "submit_report"]);
    settings.repositories[0]
        .assignments
        .push(serde_json::from_value(assignment).unwrap());
    let mut teammate = primary.clone();
    teammate.assignment_id = Some("cccccccc-cccc-4ccc-8ccc-900000000001".into());
    let work = teammate.work.as_mut().unwrap();
    work.id = "teammate-1".into();
    work.agent_id = "aaaaaaaa-aaaa-4aaa-8aaa-900000000001".into();
    work.enqueue_order = 3;
    jobs.push(teammate);
    store.save_settings(&settings).unwrap();
    store.save_queue(&jobs).unwrap();
    let binding = crate::retention::Binding::job(&primary);
    let mut states = store.load_pr_states().unwrap();
    states.observe(
        &binding,
        crate::pr_state::Observation {
            iteration_id: "iteration-1",
            new_iteration: true,
            snapshot: None,
            already_reviewed: false,
            now: 100,
        },
    );
    states.consume(&binding, 1, "unchanged");
    store.save_pr_states(&states).unwrap();
    let runs: Vec<_> = ["normal-1", "teammate-1"]
        .into_iter()
        .map(|key| {
            let mut run = crate::review::host::request(&store, key, true, 100).unwrap();
            run.operation.state = OperationState::Completed;
            run
        })
        .collect();
    store.save_reviews(&runs).unwrap();
    // The first PR's repository account is offline: nothing of it is runnable.
    let mut connectivity = store.load_connectivity().unwrap();
    connectivity.services.push(crate::connectivity::Service {
        scope: crate::backpressure::Scope::github(&primary.account_id),
        offline_since: Some(150),
        last_success: None,
        next_check_at: 10_000,
    });
    store.save_connectivity(&connectivity).unwrap();
    assert!(work::review_item_again(&store, &crate::queue::item_id(&primary), 200).unwrap());
    let queue = store.load_queue_state().unwrap();
    let fresh: Vec<_> = queue
        .jobs
        .iter()
        .filter_map(|job| job.work.as_ref())
        .filter(|work| work.pass_ordinal == 2)
        .collect();
    assert_eq!(fresh.len(), 2);
    assert!(fresh.iter().all(|work| work.enqueue_order == 0));
    let waiting: Vec<_> = capacity::candidates(&store, 200)
        .unwrap()
        .into_iter()
        .filter(|w| fresh.iter().any(|f| f.id == w.key.id))
        .collect();
    assert_eq!(waiting.len(), 2);
    assert!(waiting
        .iter()
        .all(|w| w.offline.is_some() || w.reason.is_some()));
    let other = queue
        .jobs
        .iter()
        .find(|job| job.configuration_id != primary.configuration_id)
        .unwrap();
    assert!(other.work.as_ref().unwrap().enqueue_order > 0);
    assert!(queue.next_enqueue_order >= other.work.as_ref().unwrap().enqueue_order);
}

fn fixture_endpoint(port: u16) -> Endpoint {
    Endpoint {
        protocol: PROTOCOL,
        port,
        token: "a".repeat(64),
        app_version: "fixture".into(),
        pid: std::process::id(),
    }
}

fn request_frame(token: &str, client: &str, server: &str, raw: &[u8]) -> Value {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha2::{Digest, Sha256};
    let [_, request, _] = transport::PROOF_LABELS;
    let proof = transport::mac(
        token,
        request,
        &[client.as_bytes(), server.as_bytes(), &Sha256::digest(raw)],
    );
    json!({"protocol":PROTOCOL,"proof":proof,"request":STANDARD.encode(raw)})
}

fn send_frame(stream: &mut TcpStream, magic: &[u8; 8], value: &Value) {
    let payload = serde_json::to_vec(value).unwrap();
    stream.write_all(magic).unwrap();
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&payload).unwrap();
}

#[test]
fn unauthenticated_and_http_clients_never_reach_app_operations() {
    for valid_magic in [true, false] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let endpoint = fixture_endpoint(port);
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, &endpoint, &mut |_| panic!("Unauthorized operation"))
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        send_frame(
            &mut stream,
            if valid_magic {
                b"SNIPER1\n"
            } else {
                b"POST / H"
            },
            &json!({"protocol":PROTOCOL,"nonce":"b".repeat(64)}),
        );
        if valid_magic {
            let challenge = transport::client_read(&mut stream);
            assert_eq!(challenge["nonce"].as_str().unwrap().len(), 64);
            send_frame(
                &mut stream,
                b"SNIPER1\n",
                &request_frame(
                    &"c".repeat(64),
                    &"b".repeat(64),
                    challenge["nonce"].as_str().unwrap(),
                    br#"{"operation":"inspect"}"#,
                ),
            );
            assert_eq!(
                transport::client_read(&mut stream)["error"]["code"],
                "unauthorized"
            );
            server.join().unwrap().unwrap();
        } else {
            assert!(server.join().unwrap().is_err());
        }
    }
}

#[test]
fn control_proofs_are_domain_separated_hmac_sha256() {
    // RFC 4231 test case 2.
    assert_eq!(
        transport::hmac(b"Jefe", &[b"what do ya ", b"want for nothing?"]),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // Pinned with Node's crypto.createHmac in tests/control/cli.test.mjs.
    use sha2::{Digest, Sha256};
    let [challenge, request, response] = transport::PROOF_LABELS;
    let token = "a".repeat(64);
    let (a, b) = ("b".repeat(64), "c".repeat(64));
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let rq = Sha256::digest(br#"{"operation":"inspect"}"#);
    let rs = Sha256::digest(br#"{"protocol":2,"ok":true}"#);
    assert_eq!(
        transport::mac(&token, challenge, &[a, b]),
        "9be37531fcc3b4c3250e1bf788cf8a278d76e86925f29d220037a323b19dbf85"
    );
    assert_eq!(
        transport::mac(&token, request, &[a, b, &rq]),
        "840373773afcbfc2b5e109150d3aec8cc5c46d8683cdee8f21d5511a744006f5"
    );
    assert_eq!(
        transport::mac(&token, response, &[a, b, &rq, &rs]),
        "55cf3d2ee068f8ffa08852174a0ef68dd9f440b9865489def76c7468cbae1c1d"
    );
    // Length framing: moving a byte between fields changes the input.
    assert_ne!(
        transport::mac(&token, challenge, &[b"ab", b"c"]),
        transport::mac(&token, challenge, &[b"a", b"bc"])
    );
}

#[test]
fn server_proves_capability_before_client_reveals_anything() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let endpoint = fixture_endpoint(port);
        let (stream, _) = listener.accept().unwrap();
        serve_connection(stream, &endpoint, &mut |request| Ok(request)).unwrap();
    });
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let reply =
        transport::client_exchange(&mut stream, &"a".repeat(64), json!({"operation":"inspect"}));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["result"]["operation"], "inspect");
    server.join().unwrap();
}

#[test]
fn replayed_and_raw_token_client_proofs_are_rejected() {
    let [challenge_label, _, _] = transport::PROOF_LABELS;
    let token = "a".repeat(64);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let endpoint = fixture_endpoint(port);
        let mut calls = 0;
        for _ in 0..4 {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, &endpoint, &mut |request| {
                calls += 1;
                Ok(request)
            })
            .unwrap();
        }
        calls
    });
    // The same client nonce each time: only the server's fresh nonce defeats replay.
    let nonce = "d".repeat(64);
    let mut recorded: Option<(String, Value)> = None;
    for attempt in ["valid", "replay", "raw_token", "server_proof"] {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        send_frame(
            &mut stream,
            b"SNIPER1\n",
            &json!({"protocol":PROTOCOL,"nonce":nonce}),
        );
        let challenge = transport::client_read(&mut stream);
        let fresh = challenge["nonce"].as_str().unwrap().to_owned();
        assert_eq!(
            challenge["proof"],
            transport::mac(
                &token,
                challenge_label,
                &[nonce.as_bytes(), fresh.as_bytes()]
            )
        );
        let raw = br#"{"operation":"inspect"}"#;
        let frame = match attempt {
            "valid" => {
                let valid = request_frame(&token, &nonce, &fresh, raw);
                recorded = Some((fresh.clone(), valid.clone()));
                valid
            }
            "replay" => {
                let (old, valid) = recorded.clone().unwrap();
                assert_ne!(old, fresh, "Server nonce must be fresh per connection");
                valid
            }
            "raw_token" => {
                let mut frame = request_frame(&token, &nonce, &fresh, raw);
                frame["proof"] = json!(token);
                frame
            }
            _ => {
                let mut frame = request_frame(&token, &nonce, &fresh, raw);
                frame["proof"] = challenge["proof"].clone();
                frame
            }
        };
        send_frame(&mut stream, b"SNIPER1\n", &frame);
        let reply = transport::client_read(&mut stream);
        if attempt == "valid" {
            assert_eq!(reply["ok"], true);
        } else {
            assert_eq!(reply["error"]["code"], "unauthorized", "{attempt}");
        }
    }
    assert_eq!(server.join().unwrap(), 1);
}

#[test]
fn relayed_request_with_one_changed_byte_is_rejected_undispatched() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let endpoint = fixture_endpoint(port);
        let mut calls = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, &endpoint, &mut |request| {
                calls.push(request.clone());
                Ok(request)
            })
            .unwrap();
        }
        calls
    });
    let request = json!({"operation":"save","edit":{"value":"replacement"}});
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // A relay forwards the valid challenge and request proof but changes one
    // request byte ("replacement" -> "replacemenT").
    let rejected =
        transport::client_exchange_with(&mut stream, &"a".repeat(64), request.clone(), |raw| {
            let index = raw.iter().rposition(|b| *b == b't').unwrap();
            raw[index] = b'T';
        })
        .unwrap_err();
    assert_eq!(rejected["error"]["code"], "unauthorized");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let reply = transport::client_exchange(&mut stream, &"a".repeat(64), request.clone());
    assert_eq!(reply["result"], request);
    assert_eq!(server.join().unwrap(), vec![request]);
}

#[test]
fn long_operation_keeps_handshakes_prompt_and_execution_single_writer() {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory).unwrap();
    let endpoint: Endpoint =
        serde_json::from_slice(&std::fs::read(directory.join("endpoint.json")).unwrap()).unwrap();
    let quit = Arc::new(AtomicBool::new(false));
    let (started, running) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let (current, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let host = {
        let (quit, current, peak) = (quit.clone(), current.clone(), peak.clone());
        std::thread::spawn(move || {
            server.run(
                || quit.load(Ordering::SeqCst),
                |request, _: &transport::Early| {
                    let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    if request["operation"] == "slow" {
                        started.send(()).unwrap();
                        released.recv().unwrap();
                    }
                    current.fetch_sub(1, Ordering::SeqCst);
                    Ok(request)
                },
                not_fatal,
            )
        })
    };
    let call = |operation: &'static str| {
        let (port, token) = (endpoint.port, endpoint.token.clone());
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            transport::client_exchange(&mut stream, &token, json!({ "operation": operation }))
        })
    };
    let slow = call("slow");
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    // The native operation is busy; a new client still gets its challenge
    // well inside the client's 5-second pre-challenge window.
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let nonce = "b".repeat(64);
    let asked = Instant::now();
    send_frame(
        &mut stream,
        b"SNIPER1\n",
        &json!({"protocol":PROTOCOL,"nonce":nonce}),
    );
    let challenge = transport::client_read(&mut stream);
    assert!(asked.elapsed() < Duration::from_secs(1));
    stream.set_read_timeout(None).unwrap();
    let raw = br#"{"operation":"second"}"#;
    send_frame(
        &mut stream,
        b"SNIPER1\n",
        &request_frame(
            &endpoint.token,
            &nonce,
            challenge["nonce"].as_str().unwrap(),
            raw,
        ),
    );
    let second = std::thread::spawn(move || transport::client_read(&mut stream));
    let queued: Vec<_> = (1..transport::QUEUE).map(|_| call("queued")).collect();
    std::thread::sleep(Duration::from_millis(1500));
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    let busy =
        transport::client_exchange(&mut stream, &endpoint.token, json!({"operation":"over"}));
    assert_eq!(busy["error"]["code"], "control_busy");
    // Held longer than the client's pre-challenge timeout.
    std::thread::sleep(Duration::from_millis(4000));
    release.send(()).unwrap();
    assert_eq!(slow.join().unwrap()["result"]["operation"], "slow");
    assert_eq!(second.join().unwrap()["result"]["operation"], "second");
    for call in queued {
        assert_eq!(call.join().unwrap()["result"]["operation"], "queued");
    }
    assert_eq!(
        peak.load(Ordering::SeqCst),
        1,
        "Native execution stays serialized"
    );
    quit.store(true, Ordering::SeqCst);
    host.join().unwrap();
    assert!(!directory.join("endpoint.json").exists());
}

fn read_endpoint(directory: &std::path::Path) -> Endpoint {
    serde_json::from_slice(&std::fs::read(directory.join("endpoint.json")).unwrap()).unwrap()
}

fn wait_closed(port: u16) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(std::time::Instant::now() < deadline, "Listener must close");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn echo(request: Value, _: &transport::Early) -> Result<Value, Error> {
    Ok(request)
}

fn not_fatal() -> fn() {
    panic!("No operation panics in this test")
}

#[test]
fn panicking_operation_is_outcome_unknown_and_fatal() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory).unwrap();
    let endpoint = read_endpoint(&directory);
    // (endpoint still published, when) at fail-stop, and when exit ran.
    let began = Arc::new(Mutex::new(None));
    let exited = Arc::new(Mutex::new(None));
    let host = {
        let (began, exited, directory) = (began.clone(), exited.clone(), directory.clone());
        std::thread::spawn(move || {
            server.run(
                || false,
                |request, _: &transport::Early| {
                    if request["operation"] == "boom" {
                        panic!("synthetic native panic");
                    }
                    Ok(request)
                },
                // Injectable fail-stop and application exit.
                move || {
                    *began.lock().unwrap() =
                        Some((directory.join("endpoint.json").exists(), Instant::now()));
                    move || *exited.lock().unwrap() = Some(Instant::now())
                },
            )
        })
    };
    // An idle client holds a connection slot until its 2 s handshake deadline,
    // so the bounded drain is still running when fail-stop must already hold.
    let _idle = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    let failed =
        transport::client_exchange(&mut stream, &endpoint.token, json!({"operation":"boom"}));
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"]["code"], "outcome_unknown");
    host.join().unwrap();
    let (published, began) = began.lock().unwrap().expect("Fail-stop began");
    let exited = exited.lock().unwrap().expect("App exit requested");
    assert!(published, "Fail-stop precedes drain and withdrawal");
    assert!(
        exited.duration_since(began) >= Duration::from_millis(500),
        "Exit follows the bounded drain"
    );
    assert!(
        !directory.join("endpoint.json").exists(),
        "Endpoint withdrawn"
    );
    wait_closed(endpoint.port);
    let diagnostics = std::fs::read_to_string(directory.join("transport-errors.json")).unwrap();
    assert!(diagnostics.contains("\"stage\":\"execute\""));
    assert!(!diagnostics.contains("synthetic") && !diagnostics.contains("boom"));

    // The relaunched process serves from a fresh descriptor.
    let fresh = transport::Server::bind(&directory).unwrap();
    let next = read_endpoint(&directory);
    assert_ne!(next.token, endpoint.token);
    let quit = Arc::new(AtomicBool::new(false));
    let host = {
        let quit = quit.clone();
        std::thread::spawn(move || fresh.run(|| quit.load(Ordering::SeqCst), echo, not_fatal))
    };
    let mut stream = TcpStream::connect(("127.0.0.1", next.port)).unwrap();
    let reply = transport::client_exchange(&mut stream, &next.token, json!({"operation":"after"}));
    assert_eq!(reply["result"]["operation"], "after");
    quit.store(true, Ordering::SeqCst);
    host.join().unwrap();
}

#[test]
fn panic_on_the_blocking_dispatch_pool_is_fatal() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let (root, store) = capacity::tests::fixture(1, 1, true);
    let store = std::sync::Mutex::new(store);
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory).unwrap();
    let endpoint = read_endpoint(&directory);
    let exited = Arc::new(AtomicBool::new(false));
    let host = {
        let exited = exited.clone();
        std::thread::spawn(move || {
            server.run(
                || false,
                // The production request path; only the AppHandle-bound
                // dispatcher body is replaced by a panicking one.
                |request, _: &transport::Early| {
                    tauri::async_runtime::block_on(operations::execute_request(
                        &store,
                        request,
                        |_| operations::blocking(|| panic!("synthetic dispatcher panic")),
                    ))
                },
                move || move || exited.store(true, Ordering::SeqCst),
            )
        })
    };
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    let reply = transport::client_exchange(
        &mut stream,
        &endpoint.token,
        json!({"operation":"inspect","request_id":uuid::Uuid::new_v4().to_string(),
            "issued_at":crate::now_seconds().unwrap()}),
    );
    assert_eq!(reply["error"]["code"], "outcome_unknown");
    host.join().unwrap();
    assert!(exited.load(Ordering::SeqCst), "Fail-stop and exit ran");
    assert!(
        !directory.join("endpoint.json").exists(),
        "Endpoint withdrawn"
    );
    wait_closed(endpoint.port);
}

#[test]
fn queued_requests_past_their_start_deadline_never_execute() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    };
    use std::time::Duration;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory)
        .unwrap()
        .with_queue_wait(Duration::from_millis(400));
    let endpoint = read_endpoint(&directory);
    let quit = Arc::new(AtomicBool::new(false));
    let executed = Arc::new(Mutex::new(Vec::new()));
    let (started, running) = mpsc::channel();
    let host = {
        let (quit, executed) = (quit.clone(), executed.clone());
        std::thread::spawn(move || {
            server.run(
                || quit.load(Ordering::SeqCst),
                |request, _: &transport::Early| {
                    let operation = request["operation"].as_str().unwrap().to_owned();
                    executed.lock().unwrap().push(operation.clone());
                    match operation.as_str() {
                        "long" => {
                            started.send(()).unwrap();
                            std::thread::sleep(Duration::from_millis(1200));
                        }
                        "short" => {
                            started.send(()).unwrap();
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        _ => {}
                    }
                    Ok(request)
                },
                not_fatal,
            )
        })
    };
    let call = |operation: &'static str| {
        let (port, token) = (endpoint.port, endpoint.token.clone());
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            transport::client_exchange(&mut stream, &token, json!({ "operation": operation }))
        })
    };
    // Queued behind an operation that outlasts its start deadline.
    let long = call("long");
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    let late = call("late").join().unwrap();
    assert_eq!(late["error"]["code"], "control_busy");
    assert_eq!(late["error"]["detail"]["reason"], "expired");
    assert_eq!(long.join().unwrap()["result"]["operation"], "long");
    // Queued behind a short operation: starts in time and executes.
    let short = call("short");
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    let timely = call("timely").join().unwrap();
    assert_eq!(timely["result"]["operation"], "timely");
    short.join().unwrap();
    assert_eq!(*executed.lock().unwrap(), ["long", "short", "timely"]);
    quit.store(true, Ordering::SeqCst);
    host.join().unwrap();
}

fn inspect_request() -> Value {
    json!({"operation":"inspect","request_id":uuid::Uuid::new_v4().to_string(),
        "issued_at":crate::now_seconds().unwrap()})
}

#[test]
fn timed_out_operation_keeps_the_executor_until_it_finishes() {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};
    let (root, store) = capacity::tests::fixture(1, 1, true);
    let store = std::sync::Mutex::new(store);
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory)
        .unwrap()
        .with_queue_wait(Duration::from_millis(300));
    let endpoint = read_endpoint(&directory);
    let limits = operations::Limits {
        reply: Duration::from_millis(200),
        hard: Duration::from_secs(20),
    };
    let quit = Arc::new(AtomicBool::new(false));
    let (calls, current, peak) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let host = {
        let (quit, calls, current, peak) =
            (quit.clone(), calls.clone(), current.clone(), peak.clone());
        std::thread::spawn(move || {
            server.run(
                || quit.load(Ordering::SeqCst),
                |request, early| {
                    let (calls, current, peak) = (calls.clone(), current.clone(), peak.clone());
                    tauri::async_runtime::block_on(operations::execute_request_with(
                        &store,
                        request,
                        |reply| early.reply(reply),
                        limits,
                        move |_| {
                            operations::blocking(move || {
                                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                                peak.fetch_max(now, Ordering::SeqCst);
                                // The first operation outlives the reply limit.
                                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                                    std::thread::sleep(Duration::from_millis(1200));
                                }
                                current.fetch_sub(1, Ordering::SeqCst);
                                Ok(json!({"done":true}))
                            })
                        },
                    ))
                },
                not_fatal,
            )
        })
    };
    let call = || {
        let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
        transport::client_exchange(&mut stream, &endpoint.token, inspect_request())
    };
    let asked = Instant::now();
    let first = call();
    assert_eq!(first["error"]["code"], "outcome_unknown");
    assert!(
        asked.elapsed() < Duration::from_millis(1000),
        "Released at the reply limit"
    );
    // The operation is still running: nothing else may start meanwhile.
    let queued = call();
    assert_eq!(queued["error"]["code"], "control_busy");
    assert_eq!(queued["error"]["detail"]["reason"], "expired");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let after = call();
    assert_eq!(after["result"]["done"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(peak.load(Ordering::SeqCst), 1, "Never concurrent");
    quit.store(true, Ordering::SeqCst);
    host.join().unwrap();
}

#[test]
fn operation_past_the_hard_limit_is_fatal() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;
    let (root, store) = capacity::tests::fixture(1, 1, true);
    let store = std::sync::Mutex::new(store);
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory).unwrap();
    let endpoint = read_endpoint(&directory);
    let limits = operations::Limits {
        reply: Duration::from_millis(100),
        hard: Duration::from_millis(400),
    };
    let exited = Arc::new(AtomicBool::new(false));
    let host = {
        let exited = exited.clone();
        std::thread::spawn(move || {
            server.run(
                || false,
                |request, early| {
                    tauri::async_runtime::block_on(operations::execute_request_with(
                        &store,
                        request,
                        |reply| early.reply(reply),
                        limits,
                        |_| {
                            operations::blocking(|| {
                                std::thread::sleep(Duration::from_secs(3));
                                Ok(Value::Null)
                            })
                        },
                    ))
                },
                move || move || exited.store(true, Ordering::SeqCst),
            )
        })
    };
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    let reply = transport::client_exchange(&mut stream, &endpoint.token, inspect_request());
    assert_eq!(reply["error"]["code"], "outcome_unknown");
    host.join().unwrap();
    assert!(
        exited.load(Ordering::SeqCst),
        "Hung operation escalates to fail-stop"
    );
    assert!(!directory.join("endpoint.json").exists());
}

#[test]
fn executor_failure_withdraws_the_endpoint_so_clients_relaunch() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("control");
    let server = transport::Server::bind(&directory).unwrap();
    let endpoint = read_endpoint(&directory);
    // A failure outside an operation (here: the quit probe) ends the executor.
    let host = std::thread::spawn(move || {
        server.run(|| panic!("synthetic executor failure"), echo, not_fatal)
    });
    assert!(host.join().is_err());
    assert!(!directory.join("endpoint.json").exists());
    wait_closed(endpoint.port);
}

#[test]
fn teardown_never_removes_a_successor_descriptor() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("control");
    let old = transport::Server::bind(&directory).unwrap();
    // A successor publishes before the old server tears down.
    let successor = transport::Server::bind(&directory).unwrap();
    let published = std::fs::read(directory.join("endpoint.json")).unwrap();
    std::thread::spawn(move || old.run(|| true, echo, not_fatal))
        .join()
        .unwrap();
    assert_eq!(
        std::fs::read(directory.join("endpoint.json")).unwrap(),
        published,
        "Successor descriptor survives"
    );
    let names: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.contains("retired"))
        .collect();
    assert!(names.is_empty(), "{names:?}");
    // The successor still withdraws its own.
    std::thread::spawn(move || successor.run(|| true, echo, not_fatal))
        .join()
        .unwrap();
    assert!(!directory.join("endpoint.json").exists());
}

#[test]
fn previous_protocol_envelopes_fail_before_dispatch() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        serve_connection(stream, &fixture_endpoint(port), &mut |_| {
            panic!("Unauthorized operation")
        })
        .unwrap();
    });
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    send_frame(
        &mut stream,
        b"SNIPER1\n",
        &json!({"protocol":1,"token":"a".repeat(64),"request":{"operation":"inspect"}}),
    );
    let reply = transport::client_read(&mut stream);
    assert_eq!(reply["error"]["code"], "incompatible_protocol");
    assert_eq!(reply["error"]["detail"]["supported"], PROTOCOL);
    server.join().unwrap();
}

#[test]
fn bundled_cli_crosses_framed_transport_to_real_validated_store_save() {
    let (root, store) = capacity::tests::fixture(1, 1, true);
    let before = store.load_settings().unwrap();
    let mut agent = before.agents[0].clone();
    agent.prompt = "Exact CLI-edited prompt".into();
    let input = json!({"operation":"save","revision":revision(&before), "edit":{
        "kind":"agent","id":agent.id,"expected":before.agents[0],"value":agent
    }});
    let server = transport::Server::bind(&root.path().join("control")).unwrap();
    let endpoint: Endpoint =
        serde_json::from_slice(&std::fs::read(root.path().join("control/endpoint.json")).unwrap())
            .unwrap();
    // Serve one connection with the same transport and native save boundary
    // used by the app, without constructing a desktop or opening credentials.
    drop(server);
    let listener = TcpListener::bind(("127.0.0.1", endpoint.port)).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        serve_connection(stream, &endpoint, &mut |input| {
            let edit: ResourceEdit = serde_json::from_value(input["edit"].clone()).unwrap();
            let saved =
                operations::save_checked(&store, input["revision"].as_str().unwrap(), edit, false)?;
            Ok(json!({"state":"saved","revision":revision(&saved)}))
        })
        .unwrap();
    });
    let mut cli = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src-control/pr-sniper.mjs"
        ))
        .env("PR_SNIPER_DATA_DIR", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    cli.stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = cli.wait_with_output().unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"]["state"],
        "saved"
    );
    let reopened = Store::new(root.path().into()).load_settings().unwrap();
    assert_eq!(reopened.agents[0].prompt, "Exact CLI-edited prompt");
    assert_eq!(reopened.repositories, before.repositories);
}

#[test]
fn accepted_nonblocking_listener_handles_fragmented_request_without_losing_rejection() {
    use std::time::Duration;
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("{error}"),
            }
        };
        let endpoint = fixture_endpoint(port);
        let mut calls = 0;
        serve_connection(stream, &endpoint, &mut |request| {
            calls += 1;
            assert_eq!(request["operation"], "login");
            Err(Error::new("invalid_request", ()))
        })
        .unwrap();
        assert_eq!(calls, 1);
    });
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let nonce = "b".repeat(64);
    send_frame(
        &mut stream,
        b"SNIPER1\n",
        &json!({"protocol":PROTOCOL,"nonce":nonce}),
    );
    let challenge = transport::client_read(&mut stream);
    let payload = serde_json::to_vec(&request_frame(
        &"a".repeat(64),
        &nonce,
        challenge["nonce"].as_str().unwrap(),
        br#"{"operation":"login"}"#,
    ))
    .unwrap();
    stream.write_all(b"SNIPER1\n").unwrap();
    std::thread::sleep(Duration::from_millis(40));
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .unwrap();
    std::thread::sleep(Duration::from_millis(40));
    stream.write_all(&payload).unwrap();
    assert_eq!(
        transport::client_read(&mut stream)["error"]["code"],
        "invalid_request"
    );
    server.join().unwrap();
}
