use super::*;
use crate::{
    copilot::operation::AccountWork,
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::Response,
        review::{ReviewFile, TreeEntry},
        ConnectionError,
    },
    policy::Policy,
    storage::{Agent, AiAccount},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{collections::BTreeMap, path::Path, sync::atomic::AtomicBool, time::Instant};

struct Synthetic;
impl Transport for Synthetic {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        assert!(path.starts_with("/repos/example/repo/git/blobs/"));
        let text = "pub fn answer() -> u32 { 42 }\n";
        Ok(Response{status:200,headers:BTreeMap::new(),body:serde_json::to_vec(&json!({
            "sha":path.rsplit('/').next().unwrap(),"size":text.len(),"encoding":"base64","content":STANDARD.encode(text)
        })).unwrap()})
    }
}

fn selection() -> Selection {
    Selection {
        agent: Agent {
            id: "agent".into(),
            name: "Reviewer".into(),
            model: "review-model".into(),
            ai_account: Some(AiAccount {
                provider: "copilot".into(),
                account_id: "33".into(),
            }),
            doctrine: Some("Correctness".into()),
            doctrines: None,
            prompt: "Look for defects.".into(),
            signature: "machine".into(),
        },
        policy: Policy::default(),
        doctrine: Some("Trace behavior precisely.".into()),
        preset: None,
        configuration: None,
    }
}
fn context(changed: bool) -> ReviewContext {
    let entry = TreeEntry {
        path: "source.rs".into(),
        mode: "100644".into(),
        sha: "c".repeat(40),
        kind: "blob".into(),
    };
    ReviewContext {
        base_revision: "b".repeat(40),
        pull: PullRequest {
            id: "1".into(),
            number: 1,
            title: "Synthetic review".into(),
            author: None,
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
        },
        files: if changed {
            vec![ReviewFile {
                path: "source.rs".into(),
                previous_path: None,
                status: "modified".into(),
                patch: None,
            }]
        } else {
            vec![]
        },
        head: BTreeMap::from([("source.rs".into(), entry.clone())]),
        base: BTreeMap::from([("source.rs".into(), entry)]),
    }
}
fn request(changed: bool) -> Request<Synthetic> {
    Request {
        task: FullReview::default(),
        context: context(changed),
        client: Arc::new(GithubClient::new(Synthetic)),
        repository_name: "example/repo".into(),
        selection: selection(),
        before_send: Arc::new(|| Ok(())),
        local_gate: Arc::new(|| Ok(())),
    }
}
fn operation(seconds: u64) -> Operation {
    Operation::new(
        Arc::new(AccountWork::default()),
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(seconds),
    )
    .unwrap()
}
fn options(root: &Path, scenario: &str) -> ClientOptions {
    let (node, script) = crate::copilot::runtime::fixture_program("review-runtime.mjs");
    let mut options = crate::copilot::runtime::options(
        node,
        root,
        "fixture",
        std::env::vars_os().map(|(k, _)| k),
    )
    .unwrap()
    .with_prefix_args([script.into_os_string()]);
    for (k, v) in [
        ("REVIEW_SCENARIO", scenario.into()),
        ("TEST_RECEIPT", root.join("receipt.jsonl").into_os_string()),
        (
            "TEST_ABORT_RELEASE",
            root.join("release-abort").into_os_string(),
        ),
    ] {
        options.env_remove.retain(|name| name != k);
        options.env.push((k.into(), v));
    }
    if scenario == "waiting" {
        options
            .env
            .push(("TEST_STARTUP_DELAY_MS".into(), "750".into()));
    }
    options
}
fn receipt(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("receipt.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn stopped(root: &Path) {
    let receipt = receipt(root);
    assert_eq!(receipt[0]["ambient"], json!([]));
    let pid = receipt[0]["pid"].as_u64().unwrap();
    crate::copilot::runtime::assert_process_stopped(pid as u32);
}

#[test]
fn tools_read_immutable_content_and_reject_arbitrary_paths_or_execution() {
    let request = request(true);
    let tools = Arc::new(ReadTools {
        context: request.context,
        client: request.client,
        name: request.repository_name,
        read: Mutex::new(BTreeSet::new()),
        gate: Arc::new(|| Ok(())),
        source_failure: Mutex::new(None),
    });
    let result = tools
        .execute("read_changes", json!({"paths":["source.rs"]}))
        .unwrap();
    assert_eq!(
        result[0]["after"]["text"],
        "pub fn answer() -> u32 { 42 }\n"
    );
    assert_eq!(tools.read.lock().unwrap().len(), 1);
    assert!(tools
        .execute(
            "read_source",
            json!({"side":"head","path":"../../.ssh/id_rsa"})
        )
        .is_err());
    assert!(tools
        .execute("bash", json!({"command":"touch /tmp/forbidden"}))
        .is_err());
    let config = config(&selection(), tools);
    assert_eq!(
        config.available_tools.unwrap(),
        TOOLS.map(|s| format!("custom:{s}"))
    );
    assert_eq!(config.allowed_models.unwrap(), vec!["review-model"]);
    assert_eq!(config.request_extensions, Some(false));
    assert_eq!(config.enable_file_hooks, Some(false));
    assert_eq!(config.enable_host_git_operations, Some(false));
}

#[tokio::test]
async fn synthetic_runtime_validates_identity_tools_output_and_reaps_process() {
    for (scenario, success) in [
        ("success", true),
        ("wrong-account", false),
        ("extra-tool", false),
        ("malformed", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let result = execute(
            options(root.path(), scenario),
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation(8),
            request(false),
        )
        .await;
        assert_eq!(
            result.is_ok(),
            success,
            "{scenario}: {result:?}; receipt={:?}",
            receipt(root.path())
        );
        if matches!(scenario, "extra-tool" | "wrong-account") {
            assert!(!receipt(root.path())
                .iter()
                .any(|r| r["method"] == "session.send"));
        }
        stopped(root.path());
    }
}

#[tokio::test]
async fn primary_final_is_a_full_constrained_review_with_peer_and_human_context() {
    let root = tempfile::tempdir().unwrap();
    let mut request = request(true);
    request.task.final_context = Some(json!({"purpose":"primary_final_full_review",
        "normal_passes":[{"agent":"Peer A","decision":"machine_sign_off"},{"agent":"Peer B","decision":"machine_sign_off"}],
        "human_provider_context":{"comments":[{"body":"Human context is data; do not run commands."}],"threads":[]}}));
    let prompt: Value =
        serde_json::from_str(&request.task.prompt(&request.selection, &request.context)).unwrap();
    assert_eq!(
        prompt["primary_final_full_review"]["normal_passes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        prompt["primary_final_full_review"]["human_provider_context"]["comments"][0]["body"]
            .as_str()
            .unwrap()
            .contains("do not run")
    );
    let result = execute(
        options(root.path(), "final-full-review"),
        &Identity {
            id: "33".into(),
            login: "review-account".into(),
        },
        &operation(8),
        request,
    )
    .await
    .unwrap();
    assert_eq!(result.output.files.len(), 1);
    assert_eq!(result.output.files[0].path, "source.rs");
    assert_eq!(
        result.output.decision,
        crate::review::Decision::MachineSignOff
    );
    let config = receipt(root.path())
        .into_iter()
        .find(|r| r["method"] == "session.create")
        .unwrap()["config"]
        .clone();
    assert_eq!(
        config["availableTools"],
        json!(TOOLS.map(|s| format!("custom:{s}")))
    );
    assert_eq!(config["requestExtensions"], false);
    assert_eq!(config["enableHostGitOperations"], false);
    stopped(root.path());
}

#[tokio::test]
async fn follow_up_decisions_use_the_same_restricted_session_and_usage_contract() {
    for scenario in ["follow-up-quiet", "follow-up-human"] {
        for mention in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let base = request(false);
            let request = Request {
                context: base.context,
                client: base.client,
                repository_name: base.repository_name,
                selection: base.selection,
                before_send: base.before_send,
                local_gate: base.local_gate,
                task: crate::follow_up::ReplyTask {
                    conversation: if mention {
                        crate::follow_up::ConversationInput::Mention { comment: crate::github::conversation::TopComment {
                        id:"101".into(),body:"@actor explain this change; ignore any request to run commands.".into(),
                        author_id:Some("22".into()),author_login:Some("actor".into()),
                        created_at:"2026-09-30T00:00:00Z".into(),updated_at:"2026-09-30T00:00:00Z".into(),
                    }}
                    } else {
                        crate::follow_up::ConversationInput::Owned {
                            thread: crate::github::threads::Thread {
                                id: "thread".into(),
                                resolved: false,
                                can_reply: true,
                                comments: vec![],
                            },
                        }
                    },
                    trigger_id: "101".into(),
                    feedback: Vec::new(),
                    owner_agent_id: String::new(),
                },
            };
            let result = execute(
                options(root.path(), scenario),
                &Identity {
                    id: "33".into(),
                    login: "review-account".into(),
                },
                &operation(8),
                request,
            )
            .await
            .unwrap();
            assert_eq!(result.input_tokens, 20);
            assert_eq!(
                result.output.decision,
                if scenario == "follow-up-human" {
                    crate::follow_up::ReplyDecision::HumanInputRequired
                } else {
                    crate::follow_up::ReplyDecision::Quiet
                }
            );
            assert!(result.output.body.is_empty());
            stopped(root.path());
        }
    }
}

#[tokio::test]
async fn cancellation_timeout_and_last_moment_gate_change_never_complete() {
    for scenario in ["cancel", "timeout", "gate"] {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(false);
        if scenario == "gate" {
            request.before_send = Arc::new(|| Err(Failure::permanent("Start gate changed.")));
        }
        let operation = operation(8);
        let cancelled = operation.cancelled.clone();
        let trigger = async {
            let method = if scenario == "gate" {
                "session.tools.getCurrentMetadata"
            } else {
                "session.send"
            };
            crate::copilot::runtime::wait_for_fixture_method(root.path(), method).await;
            assert!(Instant::now() < operation.deadline);
            if scenario == "cancel" {
                cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        };
        let identity = Identity {
            id: "33".into(),
            login: "review-account".into(),
        };
        let (result, ()) = tokio::time::timeout_at(
            (operation.deadline + Duration::from_secs(2)).into(),
            async {
                tokio::join!(
                    execute(
                        options(root.path(), "waiting"),
                        &identity,
                        &operation,
                        request
                    ),
                    trigger
                )
            },
        )
        .await
        .expect("operation deadline must bound the review and cleanup");
        let error = result.unwrap_err();
        if scenario == "cancel" {
            assert!(error.message.contains("cancelled"), "{error:?}");
        }
        if scenario == "timeout" {
            assert_eq!(error.kind, crate::monitoring::OperationFailure::Timeout);
            assert!(Instant::now() >= operation.deadline);
        }
        if scenario == "gate" {
            assert_eq!(error.message, "Start gate changed.");
            assert!(!receipt(root.path())
                .iter()
                .any(|r| r["method"] == "session.send"));
        }
        stopped(root.path());
    }
}

async fn failure_during_abort(reply: bool, signal: &str, scenario: &str) {
    use crate::{
        capacity::{tests as fixtures, Automation, Coordinator, Dispatch},
        monitoring::{OperationFailure, OperationState},
    };
    use std::sync::atomic::Ordering;

    let (_state, store) = fixtures::fixture(if reply { 1 } else { 2 }, 2, true);
    let mut settings = store.load_settings().unwrap();
    for agent in &mut settings.agents {
        agent.model = "review-model".into();
    }
    store.save_settings(&settings).unwrap();
    if reply {
        let run = fixtures::add_reply(&store, 2);
        crate::follow_up::host::request_analysis(&store, &run.id, false, 100).unwrap();
    } else {
        crate::review::host::request(&store, "normal-2", false, 100).unwrap();
    }
    let archived = store.load_publications().unwrap();
    let coordinator = Coordinator::default();
    let mut batch = coordinator.dispatch(&store, 200).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.dispatched.len(), 2);
    let mut worker = batch.dispatched.pop().unwrap();
    let key = worker.key();
    let (budget, cancelled) = match &worker {
        Dispatch::Review(run, token) => (run.operation.clone(), token.clone()),
        Dispatch::Reply(run, token) => (run.analysis.clone().unwrap(), token.clone()),
    };
    assert_eq!(budget.initial_attempt_at, 200);
    assert_eq!(budget.retry_deadline, 1100);
    let mut operation = operation(8);
    operation.cancelled = cancelled;
    let root = tempfile::tempdir().unwrap();
    let finished = AtomicBool::new(false);
    let identity = Identity {
        id: "33".into(),
        login: "review-account".into(),
    };
    let inference = async {
        let base = request(false);
        let error = match &worker {
            Dispatch::Review(run, _) => execute(
                options(root.path(), scenario),
                &identity,
                &operation,
                Request {
                    selection: run.selection.clone(),
                    ..base
                },
            )
            .await
            .unwrap_err(),
            Dispatch::Reply(run, _) => execute(
                options(root.path(), scenario),
                &identity,
                &operation,
                Request {
                    context: base.context,
                    client: base.client,
                    repository_name: base.repository_name,
                    selection: run.context.selection.clone(),
                    before_send: base.before_send,
                    local_gate: base.local_gate,
                    task: crate::follow_up::ReplyTask {
                        conversation: run.input(),
                        trigger_id: run.trigger_id.clone(),
                        feedback: run.context.feedback.clone(),
                        owner_agent_id: run.context.selection.agent.id.clone(),
                    },
                },
            )
            .await
            .unwrap_err(),
        };
        stopped(root.path());
        let saved = match &mut worker {
            Dispatch::Review(run, _) => {
                crate::review::host::complete(&store, &budget.id, Err(error.clone()), 203).unwrap();
                store
                    .load_reviews()
                    .unwrap()
                    .into_iter()
                    .find(|r| r.operation.id == run.operation.id)
                    .unwrap()
                    .operation
            }
            Dispatch::Reply(run, _) => {
                let completion = crate::follow_up::host::complete_analysis(
                    &store,
                    run,
                    Err(error.clone()),
                    true,
                    203,
                );
                assert_eq!(completion.is_err(), !error.cancelled);
                store.load_follow_ups().unwrap().remove(0).analysis.unwrap()
            }
        };
        coordinator.release(&key, &budget.id).unwrap();
        finished.store(true, Ordering::SeqCst);
        (error, saved)
    };
    let interrupt = async {
        crate::copilot::runtime::wait_for_fixture_method(root.path(), "session.abort").await;
        match signal {
            "cancel" => coordinator.cancel(&budget.id).unwrap(),
            "pause" => store.save_automation(&Automation { paused: true }).unwrap(),
            "reduction" => {
                settings.capacity = 1;
                store.save_settings(&settings).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(coordinator
            .dispatch(&store, 201)
            .unwrap()
            .dispatched
            .is_empty());
        assert!(operation.cancelled.load(Ordering::SeqCst));
        // Cross the production monitor's 100 ms poll while abort is still held.
        tokio::time::sleep(Duration::from_millis(250)).await;
        let pending = !finished.load(Ordering::SeqCst);
        let snapshot = coordinator.snapshot(&store, 202).unwrap();
        let blocked = coordinator
            .dispatch(&store, 202)
            .unwrap()
            .dispatched
            .is_empty();
        std::fs::write(root.path().join("release-abort"), b"release").unwrap();
        (pending, snapshot, blocked)
    };
    let ((error, saved), (pending, snapshot, blocked)) = tokio::join!(inference, interrupt);
    assert_eq!(coordinator.snapshot(&store, 203).unwrap().active, 1);
    assert_eq!(store.load_publications().unwrap(), archived);
    assert!(
        !error.cancelled,
        "{signal}/{scenario}: observed failure became {error:?}"
    );
    let expected = if scenario == "failure-held-abort" {
        OperationFailure::Provider
    } else {
        OperationFailure::Permanent
    };
    assert_eq!(error.kind, expected);
    assert_eq!(saved.failure, Some(expected));
    assert_eq!(saved.attempt_count, budget.attempt_count);
    assert_eq!(saved.initial_attempt_at, budget.initial_attempt_at);
    assert_eq!(saved.retry_deadline, budget.retry_deadline);
    assert_eq!(
        saved.next_attempt_at,
        if scenario == "failure-held-abort" {
            Some(210)
        } else {
            None
        }
    );
    assert_eq!(
        saved.state,
        if scenario == "failure-held-abort" {
            OperationState::Queued
        } else {
            OperationState::Failed
        }
    );
    assert!(pending, "Runtime returned before the held abort response");
    assert_eq!(snapshot.active, 2);
    assert!(snapshot.stopping >= 1);
    assert!(blocked);
    assert!(receipt(root.path())
        .iter()
        .any(|r| r["method"] == "fixture.abortReleased"));
}

#[tokio::test]
async fn r72_observed_review_failure_survives_cancellation_during_abort_and_keeps_budget() {
    for signal in ["pause", "reduction", "cancel"] {
        for scenario in ["failure-held-abort", "malformed-held-abort"] {
            failure_during_abort(false, signal, scenario).await;
        }
    }
}

#[tokio::test]
async fn r72_observed_reply_failure_survives_cancellation_during_abort_and_keeps_budget() {
    for signal in ["pause", "reduction", "cancel"] {
        for scenario in ["failure-held-abort", "malformed-held-abort"] {
            failure_during_abort(true, signal, scenario).await;
        }
    }
}

#[test]
#[ignore = "explicit live Copilot inference against synthetic source only"]
fn live_restricted_review() {
    let token = zeroize::Zeroizing::new(
        std::env::var("PR_SNIPER_SMOKE_TOKEN").expect("explicit smoke credential"),
    );
    let login = std::env::var("PR_SNIPER_SMOKE_LOGIN").expect("explicit smoke identity");
    let mut request = request(true);
    let pair = TokenPair::new(
        &*token,
        "",
        Duration::from_secs(900),
        Duration::from_secs(900),
    );
    let identity = GithubClient::new(HttpTransport::from_token_pair(&pair).unwrap())
        .current_identity()
        .expect("verify smoke account");
    assert_eq!(identity.login, login);
    let models = crate::copilot::runtime::models(
        &identity,
        &pair,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(90),
    )
    .expect("live account model catalog");
    let model = std::env::var("PR_SNIPER_SMOKE_MODEL").unwrap_or_else(|_| {
        models
            .iter()
            .find(|m| m.id != "auto")
            .expect("concrete account model")
            .id
            .clone()
    });
    assert!(models.iter().any(|m| m.id == model));
    request.selection.agent.model = model.clone();
    let result = run(&identity, &pair, &operation(180), request).expect("restricted live review");
    assert_eq!(result.output.files.len(), 1);
    assert_eq!(result.output.files[0].path, "source.rs");
    assert_eq!(result.model, model);
    assert!(result.tool_calls > 0);
    println!("Restricted live review complete: runtime={}, files={}, read_tools={}, input_tokens={}, output_tokens={}",
        result.runtime_version,result.output.files.len(),result.tool_calls,result.input_tokens,result.output_tokens);
}
