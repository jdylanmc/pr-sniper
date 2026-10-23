use super::*;
use crate::{
    capacity,
    monitoring::OperationState,
    storage::{ResourceEdit, Store},
};
use serde_json::json;
use std::{
    io::{Read, Write},
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
}

#[test]
fn unauthenticated_and_http_clients_never_reach_app_operations() {
    for valid_magic in [true, false] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let endpoint = Endpoint {
                protocol: 1,
                port,
                token: "a".repeat(64),
                app_version: "fixture".into(),
            };
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, &endpoint, &mut |_| panic!("Unauthorized operation"))
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let payload = serde_json::to_vec(
            &json!({"protocol":1,"token":"b".repeat(64),"request":{"operation":"inspect"}}),
        )
        .unwrap();
        stream
            .write_all(if valid_magic {
                b"SNIPER1\n"
            } else {
                b"POST / H"
            })
            .unwrap();
        stream
            .write_all(&(payload.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&payload).unwrap();
        if valid_magic {
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
            stream.read_exact(&mut bytes).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["error"]["code"],
                "unauthorized"
            );
            server.join().unwrap().unwrap();
        } else {
            assert!(server.join().unwrap().is_err());
        }
    }
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
        let endpoint = Endpoint {
            protocol: 1,
            port,
            token: "a".repeat(64),
            app_version: "fixture".into(),
        };
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
    let payload = serde_json::to_vec(
        &json!({"protocol":1,"token":"a".repeat(64),"request":{"operation":"login"}}),
    )
    .unwrap();
    stream.write_all(b"SNIPER1\n").unwrap();
    std::thread::sleep(Duration::from_millis(40));
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .unwrap();
    std::thread::sleep(Duration::from_millis(40));
    stream.write_all(&payload).unwrap();
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["error"]["code"],
        "invalid_request"
    );
    server.join().unwrap();
}
