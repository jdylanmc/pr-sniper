use super::*;

#[test]
fn r1_crash_child() {
    let Ok(root) = std::env::var("PR_SNIPER_TEST_CRASH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/retention-remediation-r1/crash-fixtures");
    assert!(root.is_absolute());
    let root = std::fs::canonicalize(root).unwrap();
    let fixtures = std::fs::canonicalize(fixtures).unwrap();
    assert_eq!(root.parent(), Some(fixtures.as_path()));
    let request: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("crash-request.json")).unwrap()).unwrap();
    let name = request["name"].as_str().unwrap();
    assert!(!name.contains(['/', '\\']));
    crate::storage::private_fs::crash_test::arm(
        root.join("state").join(name),
        request["point"].as_str().unwrap().into(),
        request["occurrence"].as_u64().unwrap() as usize,
    );
    maintain(&Store::new(root), true).unwrap();
    panic!("crash boundary was not reached");
}

#[test]
fn r1_abrupt_replacement_interruption_recovers_terminal_cleanup() {
    use std::{
        fs,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/retention-remediation-r1/crash-fixtures");
    fs::create_dir_all(&parent).unwrap();
    for (name, occurrence) in [
        ("retention.json", 1),
        ("retention.json", 2),
        ("retention.json", 3),
        ("result-index-pending.json", 1),
        ("follow-ups.json", 1),
        ("publications.json", 1),
        ("reviews.json", 1),
        ("actions.json", 1),
        ("feedback.json", 1),
        ("notifications.json", 1),
        ("queue.json", 1),
        ("result-index.json", 1),
    ] {
        let mut points = vec!["created", "partial", "flushed", "pre-rename"];
        if name == "retention.json" || name == "result-index.json" {
            points.extend(["bootstrap-stage", "owner-created", "owner-partial"]);
        }
        for point in points {
            let (source, store, mut monitor) = fixture();
            evidence(&store);
            scan(
                &store,
                &mut monitor,
                vec![pull(1, Lifecycle::Closed), pull(2, Lifecycle::Open)],
                NOW + 5,
            );
            let active = store
                .load_queue()
                .unwrap()
                .into_iter()
                .find(|j| j.number == 2)
                .unwrap();
            let active_review = completed(&store, &active);
            let mut reviews = store.load_reviews().unwrap();
            reviews.push(active_review.clone());
            store.save_reviews(&reviews).unwrap();
            let root = tempfile::tempdir_in(&parent).unwrap();
            for directory in ["state", "config"] {
                fs::create_dir(root.path().join(directory)).unwrap();
                for entry in fs::read_dir(source.path().join(directory)).unwrap() {
                    let entry = entry.unwrap();
                    fs::copy(
                        entry.path(),
                        root.path().join(directory).join(entry.file_name()),
                    )
                    .unwrap();
                }
            }
            let settings = fs::read(root.path().join("config/settings.json")).unwrap();
            let target = root.path().join("state").join(name);
            fs::write(
                root.path().join("crash-request.json"),
                serde_json::to_vec(&json!({
                    "name": name, "occurrence": occurrence, "point": point,
                }))
                .unwrap(),
            )
            .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "retention::tests::remediation::r1_crash_child",
                    "--nocapture",
                ])
                .env("PR_SNIPER_TEST_CRASH_ROOT", root.path())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            let ready = root.path().join("crash-ready.json");
            while !ready.exists() && Instant::now() < deadline {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "child exited before {name}/{occurrence}/{point}"
                );
                thread::sleep(Duration::from_millis(10));
            }
            if !ready.exists() {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child did not reach {name}/{occurrence}/{point}");
            }
            // Kill only the exact test binary PID created above; no app or provider runs.
            child.kill().unwrap();
            assert!(!child.wait().unwrap().success());
            let ready: serde_json::Value =
                serde_json::from_slice(&fs::read(ready).unwrap()).unwrap();
            let stage = std::path::Path::new(ready["stage"].as_str().unwrap());
            assert_eq!(
                fs::canonicalize(stage.parent().unwrap()).unwrap(),
                fs::canonicalize(target.parent().unwrap()).unwrap()
            );
            let staged = fs::read(stage).unwrap();
            let bootstrap = matches!(point, "bootstrap-stage" | "owner-created" | "owner-partial");
            if point == "created" || bootstrap {
                assert!(staged.is_empty());
            }
            if point == "partial" {
                assert!(!staged.is_empty());
                assert!(serde_json::from_slice::<serde_json::Value>(&staged).is_err());
            }
            if matches!(point, "flushed" | "pre-rename") {
                assert!(serde_json::from_slice::<serde_json::Value>(&staged).is_ok());
            }
            let fresh = Store::new(root.path().into());
            assert!(load(&fresh).unwrap().receipts.is_empty());
            recover(&fresh).unwrap();
            maintain(&fresh, true).unwrap();
            assert_eq!(fresh.review_evidence().unwrap(), vec![active_review]);
            assert_eq!(fresh.load_queue().unwrap(), vec![active]);
            assert_eq!(
                fs::read(root.path().join("config/settings.json")).unwrap(),
                settings
            );
            assert_eq!(load(&fresh).unwrap().receipts.len(), 1);
            assert!(load(&fresh).unwrap().pending.is_none());
            if bootstrap {
                // Without a complete claim, an empty bootstrap artifact cannot
                // be distinguished from an unowned file. No detail was staged.
                assert_eq!(fs::metadata(stage).unwrap().len(), 0);
            } else {
                assert!(
                    !stage.exists(),
                    "abandoned stage survived {name}/{occurrence}/{point}"
                );
            }
            assert_eq!(
                page(
                    &fresh,
                    PageRequest {
                        limit: 10,
                        cursor: None,
                        ..Default::default()
                    }
                )
                .unwrap()
                .results
                .len(),
                1
            );
            assert_eq!(maintain(&fresh, true).unwrap(), 0);
            println!("recovered {name}/{occurrence}/{point}");
        }
    }
}

#[test]
fn r4_legacy_revalidation_keeps_all_existing_access_and_scope_gates() {
    for gate in [
        "disabled", "removed", "account", "binding", "scope", "paused",
    ] {
        let (_root, store, mut monitor) = fixture();
        let (review, _, _) = evidence(&store);
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Closed)],
            NOW + 5,
        );
        let mut baseline = serde_json::to_value(store.load_queue_state().unwrap()).unwrap();
        baseline["tracked"][0]
            .as_object_mut()
            .unwrap()
            .remove("terminal_observed");
        store.write_state("queue.json", &baseline).unwrap();
        let mut settings = store.load_settings().unwrap();
        match gate {
            "disabled" => settings.repositories[0].enabled = false,
            "removed" => settings.repositories.clear(),
            "binding" => settings.repositories[0].provider_account_id = Some("23".into()),
            "scope" => {
                let mut state = store.load_monitoring_state().unwrap();
                state.activations.clear();
                store.save_monitoring_state(&state).unwrap();
            }
            "paused" => {
                let mut state = store.load_automation().unwrap();
                state.paused = true;
                store.save_automation(&state).unwrap();
            }
            _ => {}
        }
        store.save_settings(&settings).unwrap();
        let mut monitor = Monitor::restore(&store).unwrap();
        let accounts = if gate == "account" {
            BTreeMap::new()
        } else {
            BTreeMap::from([(
                "22".into(),
                crate::monitoring::AccountAvailability {
                    connected: true,
                    login: "actor".into(),
                },
            )])
        };
        assert!(
            monitor
                .prepare_checks_with_accounts(&store, &accounts, NOW + 10, true)
                .unwrap()
                .is_empty(),
            "{gate}"
        );
        assert!(!store.load_queue_state().unwrap().tracked[0].terminal_observed);
        assert_eq!(maintain(&store, true).unwrap(), 0);
        assert_eq!(store.load_reviews().unwrap(), vec![review]);
    }
}

#[test]
fn r4_missing_and_failed_legacy_reads_never_authorize_cleanup() {
    use crate::github::{
        provider::{GithubClient, Response, Transport},
        ConnectionError,
    };
    struct Remote(u16);
    impl Transport for Remote {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            let status = if path.contains("?state=open") {
                200
            } else {
                assert_eq!(path, "/repos/example/repo/pulls/1");
                self.0
            };
            Ok(Response {
                status,
                headers: BTreeMap::new(),
                body: if path.contains("?state=open") {
                    b"[]".to_vec()
                } else {
                    b"{}".to_vec()
                },
            })
        }
    }
    for status in [0, 401, 403, 404, 500, 200] {
        let (_root, store, mut monitor) = fixture();
        let (review, _, _) = evidence(&store);
        scan(
            &store,
            &mut monitor,
            vec![pull(1, Lifecycle::Merged)],
            NOW + 5,
        );
        let mut baseline = serde_json::to_value(store.load_queue_state().unwrap()).unwrap();
        baseline["tracked"][0]
            .as_object_mut()
            .unwrap()
            .remove("terminal_observed");
        store.write_state("queue.json", &baseline).unwrap();
        let ticket = monitor
            .prepare_checks(&store, NOW + 10, true)
            .unwrap()
            .remove(0);
        assert_eq!(ticket.tracked, vec![("1".into(), 1)]);
        let repository = RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        };
        let pulls = if status == 0 {
            Ok(vec![])
        } else {
            let result = GithubClient::new(Remote(status))
                .poll_tracked_pull_requests(&repository, &ticket.tracked);
            assert!(result.is_err(), "{status}");
            result
        };
        let result = monitor.finish(
            &store,
            ticket,
            pulls.map(|pull_requests| PollResult {
                connection: Connection {
                    identity: Identity {
                        id: "22".into(),
                        login: "actor".into(),
                    },
                    repository,
                    capabilities: Capabilities {
                        read: true,
                        comment: CommentCapability::Available,
                    },
                },
                pull_requests,
            }),
            NOW + 11,
        );
        if status != 0 {
            assert!(result.is_err());
        }
        assert!(!store.load_queue_state().unwrap().tracked[0].terminal_observed);
        assert_eq!(maintain(&store, true).unwrap(), 0);
        assert_eq!(store.load_reviews().unwrap(), vec![review]);
    }
}

#[test]
fn r2_missing_closed_compact_root_blocks_unreferenced_republication() {
    let (root, store, mut monitor) = fixture();
    let (_, publication, _) = evidence(&store);
    let mut closed = thread();
    closed.resolved = true;
    let mut feedback = store.load_feedback().unwrap();
    feedback
        .observe(&publication, &"a".repeat(40), &[closed])
        .unwrap();
    store.save_feedback(&feedback).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Closed)],
        NOW + 5,
    );
    maintain(&store, true).unwrap();
    scan(
        &store,
        &mut monitor,
        vec![pull(1, Lifecycle::Open)],
        NOW + 10,
    );
    let job = store.load_queue().unwrap().last().unwrap().clone();
    let origin = load(&store).unwrap().receipts[0].owned[0].clone();
    assert!(origin.closed_roots.contains("100"));
    for (offset, path, title) in [
        (11, "source.rs", "Concern"),
        (12, "source.rs", "Same defect, different wording"),
        (13, "renamed.rs", "Concern"),
        (14, "renamed.rs", "Same defect, different wording"),
    ] {
        let fresh = Store::new(root.path().into());
        admit_observations(
            &fresh,
            vec![Observed {
                origin: origin.clone(),
                head: job.head_sha.clone(),
                threads: vec![],
            }],
            NOW + offset,
        )
        .unwrap();
        let record = fresh.load_feedback().unwrap().records.remove(0);
        assert!(record.context.closed);
        assert!(
            record.context.unavailable.is_some(),
            "missing root must fail closed"
        );
        assert!(crate::feedback::contexts(&fresh, &job, AGENT).is_err());
        let mut attempted = completed(&fresh, &job);
        attempted.feedback_context = Some(vec![]);
        let finding = &mut attempted.result.as_mut().unwrap().output.findings[0];
        finding.path = path.into();
        finding.title = title.into();
        assert!(finding.feedback_id.is_none());
        assert!(crate::feedback::publication_gate(&fresh, &attempted).is_err());
        assert!(fresh.load_publications().unwrap().is_empty());
        assert!(fresh.load_follow_ups().unwrap().is_empty());
    }
}

#[test]
fn r3_full_and_compact_settlement_parity_preserves_unsettled_replies() {
    for retained in [false, true] {
        for closure in [false, true] {
            for unsettled in ["none", "uncertain", "incomplete"] {
                let (_root, store, _monitor) = fixture();
                let (mut review, publication, mut follow) = evidence(&store);
                // Isolate the conversation's contribution to readiness.
                review.result.as_mut().unwrap().output.findings.clear();
                review.result.as_mut().unwrap().output.decision =
                    crate::review::Decision::MachineSignOff;
                store.save_reviews(&[review]).unwrap();
                follow.phase = FollowPhase::Stopped;
                follow.error = Some("Original reply stopped.".into());
                follow.context.feedback_checked = true;
                if retained {
                    follow.target = crate::follow_up::ConversationTarget::Retained(Box::new(
                        crate::follow_up::RetainedTarget {
                            proof: publication.ownership().unwrap(),
                            thread: thread(),
                        },
                    ));
                }
                let context = store.load_feedback().unwrap().records.remove(0).context;
                assert!(follow.owns_feedback(&context));
                for field in ["publication", "agent", "assignment", "head", "root"] {
                    let mut foreign = context.clone();
                    match field {
                        "publication" => foreign.publication_id = "foreign".into(),
                        "agent" => foreign.owner_agent_id = "foreign".into(),
                        "assignment" => foreign.owner_assignment_id = "foreign".into(),
                        "head" => foreign.original_head = "f".repeat(40),
                        _ => foreign.root_id = "foreign".into(),
                    }
                    assert!(!follow.owns_feedback(&foreign));
                }
                if unsettled != "none" {
                    let mut op = follow.operation("reply_publish", NOW + 2);
                    op.state = OperationState::Failed;
                    op.attempted_mutation = Some("reply".into());
                    follow.publication = Some(op);
                    follow.uncertain = unsettled == "uncertain";
                }
                store.save_follow_ups(&[follow.clone()]).unwrap();
                let mut observed = thread();
                if closure {
                    observed.resolved = true;
                } else {
                    observed.comments[1].id = "newer-trigger".into();
                }
                let mut feedback = store.load_feedback().unwrap();
                feedback
                    .observe(&publication, &"a".repeat(40), &[observed])
                    .unwrap();
                store.save_feedback(&feedback).unwrap();
                let snapshot = crate::queue::normal_snapshot(&store, vec![]).unwrap();
                let state = snapshot.items[0].state;
                if unsettled == "none" {
                    assert_eq!(
                        state,
                        if closure {
                            crate::queue::State::MachineSignedOff
                        } else {
                            crate::queue::State::WaitingForAuthor
                        },
                        "retained={retained}, closure={closure}"
                    );
                } else {
                    assert_eq!(state, crate::queue::State::Failed);
                    assert_eq!(store.load_follow_ups().unwrap(), vec![follow]);
                }
            }
        }
    }
}

#[test]
fn r4_legacy_terminal_records_require_explicit_revalidation_before_cleanup() {
    for lifecycle in [Lifecycle::Closed, Lifecycle::Merged] {
        let (root, store, mut monitor) = fixture();
        let (review, _, _) = evidence(&store);
        scan(
            &store,
            &mut monitor,
            vec![pull(1, lifecycle.clone())],
            NOW + 5,
        );
        let mut baseline = serde_json::to_value(store.load_queue_state().unwrap()).unwrap();
        baseline["tracked"][0]
            .as_object_mut()
            .unwrap()
            .remove("terminal_observed");
        store.write_state("queue.json", &baseline).unwrap();
        let store = Store::new(root.path().into());
        let mut monitor = Monitor::restore(&store).unwrap();
        assert!(!store.load_queue_state().unwrap().tracked[0].terminal_observed);
        assert_eq!(maintain(&store, true).unwrap(), 0);
        assert_eq!(store.load_reviews().unwrap(), vec![review]);
        let ticket = monitor
            .prepare_checks(&store, NOW + 10, true)
            .unwrap()
            .remove(0);
        assert_eq!(
            ticket.tracked,
            vec![("1".into(), 1)],
            "legacy {lifecycle:?}"
        );
        monitor
            .finish(
                &store,
                ticket,
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
                    pull_requests: vec![pull(1, lifecycle)],
                }),
                NOW + 11,
            )
            .unwrap();
        assert!(store.load_queue_state().unwrap().tracked[0].terminal_observed);
        assert_eq!(maintain(&store, true).unwrap(), 1);
        assert!(store.review_evidence().unwrap().is_empty());
        assert!(monitor.prepare_checks(&store, NOW + 20, true).unwrap()[0]
            .tracked
            .is_empty());
    }
}
