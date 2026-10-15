use super::*;
use crate::{
    capacity::Coordinator as Capacity,
    github::{
        actions::{HumanThread, MergeMethod, ProviderReview},
        metadata::{Lifecycle, PullRequest},
        provider::{
            Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository, Response,
            Transport,
        },
        publication::{MutationTransport, Request},
        threads::QueryTransport,
        ConnectionError, Identity,
    },
    monitoring::{ActivationMode, Monitor, MonitoringActivation, PollResult},
    review::ReviewResult,
    storage::Settings,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const NOW: i64 = 1_800_000_000;
mod approve_exits;
mod fixwave;
mod legacy_compat;

fn output() -> ReviewResult {
    serde_json::from_value(json!({"reviewed_base_sha":"b".repeat(40),"output":{"synopsis":"Review completed.",
        "files":[{"path":"source.rs","order":1,"explanation":"Complete source review."}],"findings":[],"decision":"machine_sign_off"},
        "session_id":"fixture","model":"model","runtime_version":"fixture","input_tokens":1,"output_tokens":1,"tool_calls":1})).unwrap()
}
fn fixture(count: usize, approve: bool, merge: bool) -> (tempfile::TempDir, Store, String) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut settings:Settings=serde_json::from_value(json!({"launch_at_login":false,"doctrines":[],
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,"provider_account_id":"22",
            "provider_repository_id":"100","watched_authors":[{"id":"11","login":"author"}]}]})).unwrap();
    settings.defaults.automatic_agent_start = false;
    settings.repositories[0].overrides.automatic_agent_start = Some(false);
    for i in 1..=count {
        let agent = format!("aaaaaaaa-aaaa-4aaa-8aaa-{i:012}");
        settings.agents.push(serde_json::from_value(json!({"id":agent,"name":format!("Agent {i}"),"model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},"prompt":"Review correctness.","signature":"machine"})).unwrap());
        settings.repositories[0].assignments.push(
            serde_json::from_value(
                json!({"id":format!("cccccccc-cccc-4ccc-8ccc-{i:012}"),"agent_id":agent,
            "schedule":settings.defaults.schedule,"comment":false,"approve":true,
            "actions":{"approve":i==1&&(approve||merge),"merge":i==1&&merge}}),
            )
            .unwrap(),
        );
    }
    settings.repositories[0].primary_assignment_id =
        Some(settings.repositories[0].assignments[0].id.clone());
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "scope".into(),
            repository_id: REPO.into(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: 0,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW - 1,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    let mut monitor = Monitor::restore(&store).unwrap();
    let ticket = monitor.prepare_checks(&store, NOW, true).unwrap().remove(0);
    monitor
        .finish(
            &store,
            ticket,
            Ok(PollResult {
                states: Default::default(),
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
                pull_requests: vec![PullRequest {
                    mentioned: false,
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
                    head_sha: "a".repeat(40),
                    base_sha: "b".repeat(40),
                    head_repository_id: Some("100".into()),
                    base_repository_id: "100".into(),
                    updated_at: "2026-09-30T00:00:00Z".into(),
                    files: vec![],
                }],
            }),
            NOW + 1,
        )
        .unwrap();
    let jobs = store.load_queue().unwrap();
    let item = queue::item_id(&jobs[0]);
    let peers = jobs
        .iter()
        .map(|job| {
            let assignment = job.assignment_id.as_ref().unwrap();
            let mut operation = JobOperation::review(job, NOW + 2);
            operation.state = OperationState::Completed;
            ReviewRun {
                mode: Default::default(),
                packet: None,
                failed_duck: None,
                attempt_ducks: Vec::new(),
                feedback_context: None,
                key: crate::review::key(job, assignment),
                assignment_id: assignment.clone(),
                job: job.clone(),
                selection: Selection::resolve(&settings, job, assignment).unwrap(),
                operation,
                manual_start: false,
                trust_confirmed: false,
                phase: "Complete".into(),
                error: None,
                result: Some(output()),
            }
        })
        .collect::<Vec<_>>();
    store.save_reviews(&peers).unwrap();
    (root, store, item)
}
#[test]
fn packet_preparation_reads_fresh_state_offers_exits_and_rechecks_them_at_exit_time() {
    use crate::{
        exits::{Exit, ExitCall},
        github::{
            metadata::{Lifecycle, PullRequest},
            review::{ReviewContext, ReviewFile},
        },
        review::{packet_run, runtime::Refusal, RunMode},
    };
    let (_root, store, _item) = fixture(1, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].tools = Some(
        [
            "read_changes",
            "leave_comments",
            "ready_for_human",
            "approve_pull_request",
            "approve_and_merge",
            "panic_exit",
        ]
        .map(String::from)
        .to_vec(),
    );
    settings.repositories[0].assignments[0].comment = true;
    settings.repositories[0].assignments[0]
        .actions
        .as_mut()
        .unwrap()
        .reply = true;
    store.save_settings(&settings).unwrap();
    let job = store.load_queue().unwrap().remove(0);
    let assignment = settings.repositories[0].assignments[0].id.clone();
    let selection = Selection::resolve(&settings, &job, &assignment).unwrap();
    let mode = RunMode::Packet;
    let run = ReviewRun {
        mode,
        packet: None,
        failed_duck: None,
        attempt_ducks: Vec::new(),
        feedback_context: None,
        key: crate::review::key(&job, &assignment),
        assignment_id: assignment,
        operation: JobOperation::review(&job, NOW),
        job: job.clone(),
        selection,
        manual_start: false,
        trust_confirmed: false,
        phase: String::new(),
        error: None,
        result: None,
    };
    let env = env(&store);
    let client = Arc::new(GithubClient::new(env.wire.clone()));
    let repo = RemoteRepository {
        id: "100".into(),
        name: "example/repo".into(),
    };
    let context = ReviewContext {
        pull: PullRequest {
            mentioned: false,
            id: "9".into(),
            number: 1,
            title: "Change".into(),
            author: None,
            requested_reviewers: vec![],
            requested_teams: vec![],
            state: Lifecycle::Open,
            draft: false,
            head_sha: "a".repeat(40),
            base_sha: "b".repeat(40),
            head_repository_id: Some("100".into()),
            base_repository_id: "100".into(),
            updated_at: "2026-09-30T00:00:00Z".into(),
            files: vec![],
        },
        base_revision: "b".repeat(40),
        files: vec![ReviewFile {
            path: "src/lib.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: Some("@@ -1 +1 @@\n-old\n+new".into()),
        }],
        head: BTreeMap::new(),
        base: BTreeMap::new(),
    };
    let snapshot = crate::github::state::PrSnapshot {
        number: 1,
        pull_request_id: "9".into(),
        title: "Change".into(),
        body: "Why this change.".into(),
        labels: vec!["bug".into()],
        head_ref: "feature/x".into(),
        head: "a".repeat(40),
        base: "b".repeat(40),
        state: "OPEN".into(),
        mergeable: "MERGEABLE".into(),
        ..Default::default()
    };
    let prepared = packet_run::prepare(
        &client,
        &repo,
        &run,
        &context,
        &snapshot,
        packet_run::Caller {
            acting_login: "actor",
            write_capability: true,
            teammates: vec![],
            iterations: &[],
            last_consumed_at: None,
        },
    )
    .unwrap();
    assert!(prepared.packet.text.contains("Why this change."));
    assert!(prepared.packet.text.contains("feature/x"));
    assert!(prepared.packet.text.contains("bug"));
    assert!(prepared.packet.text.contains("size: +1 -1"));
    assert_eq!(
        prepared.setup.offered,
        [
            Exit::LeaveComments,
            Exit::ReadyForHuman,
            Exit::ApprovePullRequest,
            Exit::ApproveAndMerge,
            Exit::PanicExit
        ],
        "{:?}",
        prepared.packet.record.exits_denied
    );
    assert_eq!(
        prepared.setup.tool_set,
        [
            "read_changes",
            "leave_comments",
            "ready_for_human",
            "approve_pull_request",
            "approve_and_merge",
            "panic_exit"
        ]
    );
    assert!(prepared.packet.text.contains("role: primary"));
    assert!(prepared
        .packet
        .text
        .contains("- src/lib.rs [modified] kind=text parts: patch=21"));
    let seq = job.work.as_ref().map_or(0, |w| w.state_seq);
    let state_hash = snapshot.normalize().hash();
    assert_eq!(
        prepared.packet.record.state_id,
        format!("{seq}:{}", &state_hash[..16])
    );
    assert_eq!(prepared.packet.record.state_hash, Some(state_hash));
    assert!(!serde_json::to_string(&prepared.packet.record)
        .unwrap()
        .contains("src/lib.rs"));
    {
        use crate::github::actions::HumanThread;
        // Location from the root (line, else originalLine when outdated); edits dated.
        let thread = |id: &str, commit: &str, line: Value, updated: &str| HumanThread {
            id: id.into(),
            resolved: false,
            outdated: false,
            resolved_by: None,
            comments: vec![
                json!({"fullDatabaseId":1,"body":"c","author":{"login":"x"},"originalCommit":{"oid":commit},
                    "path":"src/lib.rs","line":line,"originalLine":7,"updatedAt":updated}),
            ],
        };
        env.wire.0.lock().unwrap().observation.threads = vec![
            thread("mapped", &"a".repeat(40), json!(3), "2026-01-01T00:00:00Z"),
            thread(
                "elsewhere",
                &"c".repeat(40),
                Value::Null,
                "2026-06-01T00:00:00Z",
            ),
        ];
        let known = vec![("a".repeat(40), 2)];
        let stamped = packet_run::prepare(
            &client,
            &repo,
            &run,
            &context,
            &snapshot,
            packet_run::Caller {
                acting_login: "actor",
                write_capability: true,
                teammates: vec![],
                iterations: &known,
                last_consumed_at: Some(
                    chrono::DateTime::parse_from_rfc3339("2026-03-01T00:00:00Z")
                        .unwrap()
                        .timestamp(),
                ),
            },
        )
        .unwrap();
        let text = &stamped.packet.text;
        let line = |id: &str| {
            text.lines()
                .find(|l| l.starts_with(&format!("thread {id} ")))
                .unwrap()
                .to_string()
        };
        assert!(line("mapped").contains("targeted_revision=2"));
        assert!(line("elsewhere").contains("targeted_revision=unknown"));
        assert!(line("mapped").contains(" at src/lib.rs:3 "));
        assert!(line("elsewhere").contains(" at src/lib.rs:7 "));
        assert!(line("mapped").contains("new_since_last_review=false"));
        assert!(line("elsewhere").contains("new_since_last_review=true"));
        // Live: the model mangled one thread id; the gate now refuses it in the loop.
        let reply = |thread: &str| ExitCall::LeaveComments {
            new_comments: vec![],
            body: None,
            reply_comments: vec![crate::exits::ReplyComment {
                thread_id: thread.into(),
                body: "b".into(),
            }],
            resolve_own_threads: vec![],
        };
        assert_eq!((stamped.setup.gate)(&reply("mapped")), Ok(()));
        // Live: a comment on CHANGELOG.md:151 outside the diff parked the publication.
        // The gate refuses it in the loop; a body or a changed line is accepted.
        let leave = |comments: Vec<(&str, u64, Option<&str>)>, body: Option<&str>| {
            ExitCall::LeaveComments {
                new_comments: comments
                    .into_iter()
                    .map(|(path, line, side)| crate::exits::NewComment {
                        path: path.into(),
                        line,
                        side: side.map(String::from),
                        body: "b".into(),
                    })
                    .collect(),
                body: body.map(String::from),
                reply_comments: vec![],
                resolve_own_threads: vec![],
            }
        };
        assert_eq!(
            (stamped.setup.gate)(&leave(vec![("CHANGELOG.md", 151, Some("head"))], None)),
            Err(Refusal::Denied(
                "CHANGELOG.md:151 (head) not in diff. Comment on a changed line, or put it in body."
                    .into()
            ))
        );
        assert!(matches!(
            (stamped.setup.gate)(&leave(
                vec![("src/lib.rs", 1, None), ("src/lib.rs", 2, None), ("src/lib.rs", 1, Some("base"))],
                Some("x")
            )),
            Err(Refusal::Denied(reason)) if reason.starts_with("src/lib.rs:2 (head) not in diff.")
        ));
        assert_eq!(
            (stamped.setup.gate)(&leave(vec![("src/lib.rs", 1, None)], None)),
            Ok(())
        );
        assert_eq!(
            (stamped.setup.gate)(&leave(vec![("src/lib.rs", 1, Some("base"))], Some("x"))),
            Ok(())
        );
        assert_eq!(
            (stamped.setup.gate)(&leave(vec![], Some("Merge conflict in CHANGELOG.md."))),
            Ok(())
        );
        assert!(matches!(
            (stamped.setup.gate)(&reply("mapped-typo")),
            Err(Refusal::Denied(reason)) if reason.contains("unknown thread")
        ));
        env.wire.0.lock().unwrap().observation.threads.clear();
    }
    let approve = ExitCall::ApprovePullRequest {
        comment: "ok".into(),
    };
    assert_eq!((prepared.setup.gate)(&approve), Ok(()));
    // CI turns red between packet and exit: that exit is denied, the run continues.
    env.wire.0.lock().unwrap().observation.checks = Some("FAILURE".into());
    assert!(matches!(
        (prepared.setup.gate)(&approve),
        Err(Refusal::Denied(reason)) if reason.contains("CI is red")
    ));
    assert_eq!(
        (prepared.setup.gate)(&ExitCall::ReadyForHuman {
            summary: "s".into()
        }),
        Ok(())
    );
    // The head moves: the whole run is tossed.
    env.wire.0.lock().unwrap().observation.head = "d".repeat(40);
    assert!(matches!(
        (prepared.setup.gate)(&approve),
        Err(Refusal::Moved(_))
    ));
    assert_eq!(
        packet_run::prepare(
            &client,
            &repo,
            &run,
            &context,
            &snapshot,
            packet_run::Caller {
                acting_login: "actor",
                write_capability: true,
                teammates: vec![],
                iterations: &[],
                last_consumed_at: None,
            }
        )
        .err()
        .unwrap()
        .kind,
        monitoring::OperationFailure::Superseded
    );
}

fn observed() -> Observation {
    Observation {
        write_capability: true,
        node_id: "PR_node".into(),
        repository_id: "100".into(),
        pull_request_id: "9".into(),
        account_id: "22".into(),
        author_id: "11".into(),
        head_repository_id: Some("100".into()),
        head: "a".repeat(40),
        base: "b".repeat(40),
        base_name: "main".into(),
        merge_rules: Some(vec![]),
        merge_rules_error: None,
        state: "OPEN".into(),
        draft: false,
        permission: "WRITE".into(),
        mergeable: "MERGEABLE".into(),
        merge_state: "CLEAN".into(),
        review_decision: Some("APPROVED".into()),
        checks: Some("SUCCESS".into()),
        check_contexts: vec![],
        in_merge_queue: false,
        method: Some(MergeMethod::Squash),
        protection: Value::Null,
        threads: vec![],
        reviews: vec![],
        comments: vec![],
        merged_by: None,
        merged_at: None,
        merge_commit: None,
    }
}
/// A packet approve exit adopted as a final: the only final that authorizes actions.
pub(super) fn packet_final(store: &Store, item: &str, observation: &Observation) -> FinalReview {
    let merge = store.load_settings().unwrap().repositories[0].assignments[0]
        .actions
        .as_ref()
        .is_some_and(|a| a.merge);
    approve_exits::exit_run(store, approve_exits::approve_exit(merge), false);
    approve_exits::adopted(store, item, observation.clone())
}

fn completed_final(store: &Store, item: &str) -> FinalReview {
    packet_final(store, item, &observed())
}

#[test]
fn retention_waits_for_uncertain_actions_then_discards_all_final_detail_copies() {
    let (root, store, item) = fixture(1, true, false);
    let run = completed_final(&store, &item);
    let effect = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    mark_intent(&store, &effect.id, NOW + 21).unwrap();
    finish_effect(
        &store,
        &effect.id,
        &effect.operation.id,
        Err(crate::publication::WriteFailure {
            failure: Failure::permanent("Synthetic lost response"),
            uncertain: true,
        }),
    )
    .unwrap();
    let mut closed = observed();
    closed.state = "CLOSED".into();
    synchronize(&store, &item, Ok(closed.clone()), NOW + 22).unwrap();
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 0);
    assert!(!store.load_actions().unwrap().finals.is_empty());
    let receipt = Receipt {
        id: "confirmed-fixture-vote".into(),
        actor_id: "22".into(),
        head: observed().head,
        action: Action::Approve,
        merge_commit: None,
    };
    finish_effect(
        &store,
        &effect.id,
        &effect.operation.id,
        Ok(receipt.clone()),
    )
    .unwrap();
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0]
        .execution
        .result
        .as_mut()
        .unwrap()
        .output
        .synopsis = "RETENTION-BULK-ONLY".into();
    ledger.finals[0].basis.peers[0]
        .result
        .as_mut()
        .unwrap()
        .output
        .synopsis = "RETENTION-BULK-ONLY".into();
    ledger.effects[0].body = "RETENTION-BULK-ONLY".into();
    store.save_actions(&ledger).unwrap();
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 1);
    let actions = store.load_actions().unwrap();
    assert!(
        actions.finals.is_empty() && actions.effects.is_empty() && actions.observations.is_empty()
    );
    let retained = crate::retention::load(&store).unwrap();
    assert_eq!(
        retained.receipts[0].effects[0].receipt.as_ref(),
        Some(&receipt)
    );
    for file in std::fs::read_dir(root.path().join("state")).unwrap() {
        assert!(
            !String::from_utf8(std::fs::read(file.unwrap().path()).unwrap())
                .unwrap()
                .contains("RETENTION-BULK-ONLY")
        );
    }
    assert!(finish_effect(&store, &effect.id, &effect.operation.id, Ok(receipt)).is_err());
    assert!(crate::actions::prepare_effect(
        &store,
        &run.id,
        Action::Approve,
        &observed(),
        NOW + 25
    )
    .is_err());
}

#[test]
fn retention_does_not_treat_a_running_final_as_settled() {
    let (_root, store, item) = fixture(1, true, false);
    let run = completed_final(&store, &item);
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0].execution.operation.state = OperationState::Running;
    store.save_actions(&ledger).unwrap();
    let mut closed = observed();
    closed.state = "CLOSED".into();
    synchronize(&store, &item, Ok(closed), NOW + 20).unwrap();
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 0);
    assert_eq!(store.load_actions().unwrap().finals[0].id, run.id);
}

#[test]
fn retention_stale_same_head_action_observation_cannot_retire_reopened_iteration() {
    let (_root, store, item) = fixture(1, true, false);
    completed_final(&store, &item);
    let mut queue = store.load_queue_state().unwrap();
    queue.tracked[0].iteration = 2;
    queue.tracked[0].iteration_id = "reopened-iteration".into();
    queue.tracked[0].item_id = "reopened-item".into();
    store.save_queue_state(&queue).unwrap();
    let mut closed = observed();
    closed.state = "CLOSED".into();
    assert!(synchronize(&store, &item, Ok(closed), NOW + 20).is_err());
    let queue = store.load_queue_state().unwrap();
    assert_eq!(queue.tracked[0].lifecycle, Lifecycle::Open);
    assert!(!queue.tracked[0].terminal_observed);
    assert_eq!(crate::retention::maintain(&store, true).unwrap(), 0);
}

#[test]
fn correction_action_read_error_opt_out_restores_personal_handoff_after_restart() {
    let (root, store, item) = fixture(1, true, true);
    synchronize(
        &store,
        &item,
        Err(Failure::permanent("Optional action evidence unavailable.")),
        NOW + 10,
    )
    .unwrap();
    let before = store.load_actions().unwrap();
    assert!(before.finals.is_empty());
    assert!(before.effects.is_empty());
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::Failed
    );
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
        include_self: false,
        reply: false,
        approve: false,
        merge: false,
    });
    store.save_settings(&settings).unwrap();
    drop(store);
    let store = Store::new(root.path().into());
    restore(&store).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].state, State::MachineSignedOff);
    let status = snapshot.items[0].action_status.as_ref().unwrap();
    assert!(status.machine_clear);
    assert!(status.personal_review.starts_with("Required:"));
    assert!(status
        .blockers
        .iter()
        .any(|b| b == "Optional action evidence unavailable."));
    assert!(status
        .observation_retry_blocker
        .as_ref()
        .unwrap()
        .contains("No enabled provider action"));
    assert!(request_observation_retry(&store, &item, NOW + 19)
        .unwrap_err()
        .contains("No enabled provider action"));
    assert_eq!(store.load_actions().unwrap(), before);
    synchronize(&store, &item, Ok(observed()), NOW + 20).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert!(store.load_actions().unwrap().effects.is_empty());
    assert!(Capacity::default()
        .dispatch(&store, NOW + 21)
        .unwrap()
        .dispatched
        .is_empty());
}

#[test]
fn correction_optional_action_diagnostic_does_not_clear_normal_feedback_or_revision_blockers() {
    for case in ["normal", "feedback", "revision", "human"] {
        let (_root, store, item) = fixture(1, false, false);
        synchronize(
            &store,
            &item,
            Err(Failure::permanent("Optional action evidence unavailable.")),
            NOW + 10,
        )
        .unwrap();
        match case {
            "normal" => {
                let mut reviews = store.load_reviews().unwrap();
                reviews[0].operation.state = OperationState::Running;
                reviews[0].result = None;
                store.save_reviews(&reviews).unwrap();
            }
            "feedback" => {
                let mut monitoring = store.load_monitoring_state().unwrap();
                monitoring
                    .health
                    .get_mut(REPO)
                    .unwrap()
                    .conversation_admission_pending = true;
                store.save_monitoring_state(&monitoring).unwrap();
            }
            "revision" => {
                let mut jobs = store.load_queue().unwrap();
                jobs[0].waiting = monitoring::WAITING_SUPERSEDED.into();
                store.save_queue(&jobs).unwrap();
            }
            "human" => {
                let mut reviews = store.load_reviews().unwrap();
                reviews[0].result.as_mut().unwrap().output.decision =
                    crate::review::Decision::HumanInputRequired;
                store.save_reviews(&reviews).unwrap();
            }
            _ => unreachable!(),
        }
        let normal = queue::normal_snapshot(&store, vec![]).unwrap();
        assert_ne!(normal.items[0].state, State::MachineSignedOff, "{case}");
        let projected = queue::snapshot(&store, vec![]).unwrap();
        assert_eq!(projected.items[0].state, normal.items[0].state, "{case}");
        assert!(
            !projected.items[0]
                .action_status
                .as_ref()
                .unwrap()
                .machine_clear
        );
        assert!(request_observation_retry(&store, &item, NOW + 11).is_err());
        assert!(store.load_actions().unwrap().effects.is_empty());
    }
}

#[test]
fn completed_review_stays_ready_after_repository_disabled_or_agent_edited() {
    let (_root, store, _item) = fixture(1, false, false);
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::MachineSignedOff
    );
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].enabled = false;
    settings.agents[0].prompt = "Changed after the review.".into();
    store.save_settings(&settings).unwrap();
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0].state,
        State::MachineSignedOff
    );
}

#[test]
fn draft_self_approval_quorum_ci_outdated_threads_and_provider_method_have_distinct_gates() {
    let (_root, store, item) = fixture(1, true, true);
    let final_review = completed_final(&store, &item);
    let mut current = observed();
    current.review_decision = Some("REVIEW_REQUIRED".into());
    current.merge_state = "BLOCKED".into();
    current.checks = Some("PENDING".into());
    assert!(ready(&store, &final_review, &current, Action::Approve).is_ok());
    assert!(ready(&store, &final_review, &current, Action::Merge).is_err());
    current = observed();
    current.author_id = "22".into();
    assert!(current.blocker(Action::Approve).unwrap().contains("author"));
    assert!(current.blocker(Action::Merge).is_none());
    for kind in [
        "draft",
        "head",
        "base",
        "review",
        "thread",
        "permission",
        "method",
        "queue",
        "checks",
        "hooks",
    ] {
        let mut current = observed();
        match kind {
            "draft" => current.draft = true,
            "head" => current.head = "c".repeat(40),
            "base" => current.base = "c".repeat(40),
            "review" => current.reviews.push(ProviderReview {
                id: "44".into(),
                actor_id: "55".into(),
                head: current.head.clone(),
                state: "CHANGES_REQUESTED".into(),
                body: "Concern".into(),
                submitted_at: Some("2026-09-30T00:00:00Z".into()),
            }),
            "thread" => current.threads.push(HumanThread {
                id: "human-thread".into(),
                resolved: false,
                outdated: true,
                resolved_by: None,
                comments: vec![json!({"body":"Still blocking"})],
            }),
            "permission" => current.permission = "READ".into(),
            "method" => current.method = None,
            "queue" => current.in_merge_queue = true,
            "checks" => current.checks = Some("FAILURE".into()),
            _ => current.merge_state = "HAS_HOOKS".into(),
        }
        assert!(
            ready(&store, &final_review, &current, Action::Merge).is_err(),
            "{kind}"
        );
    }
}

#[derive(Clone)]
struct Wire(Arc<Mutex<Server>>);
struct Server {
    observation: Observation,
    writes: Vec<(String, Value)>,
    lost: bool,
    reject: bool,
}
impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let server = self.0.lock().unwrap();
        let value = if path == "/user" {
            json!({"id":22,"login":"actor"})
        } else if path == "/repos/example/repo" {
            json!({"id":100,"full_name":"example/repo","private":false,"archived":false,"disabled":false,"permissions":{"pull":true}})
        } else if path == "/repos/example/repo/pulls?state=open&per_page=1" {
            json!([])
        } else if path.contains("/reviews?") {
            json!(server.observation.reviews.iter().map(|r|json!({
            "id":r.id.parse::<u64>().unwrap(),"user":{"id":r.actor_id.parse::<u64>().unwrap()},"commit_id":r.head,"state":r.state,"body":r.body,"submitted_at":r.submitted_at
        })).collect::<Vec<_>>())
        } else if path.contains("/comments?") || path == "/repos/example/repo/rules/branches/main" {
            json!([])
        } else {
            panic!("Unexpected read {path}")
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}
impl QueryTransport for Wire {
    fn query(&self, query: &str, _: Value) -> Result<Response, ConnectionError> {
        assert!(!query.contains("viewerCanMergeAsAdmin"));
        assert!(!query.starts_with("mutation"));
        let o = &self.0.lock().unwrap().observation;
        let mut pull = json!({"id":o.node_id,"fullDatabaseId":9,"baseRefOid":o.base,"headRefOid":o.head,"isDraft":o.draft,"state":o.state,
                "mergeStateStatus":o.merge_state,"mergeable":o.mergeable,"reviewDecision":o.review_decision,"isInMergeQueue":o.in_merge_queue,
                "author":{"databaseId":o.author_id.parse::<u64>().unwrap()},"mergedBy":o.merged_by.as_ref().map(|id|json!({"databaseId":id.parse::<u64>().unwrap()})),
                "mergedAt":o.merged_at,"mergeCommit":o.merge_commit.as_ref().map(|id|json!({"oid":id})),
                "baseRef":{"branchProtectionRule":o.protection}});
        pull["headRepository"] = json!(o
            .head_repository_id
            .as_ref()
            .map(|id| json!({"databaseId":id.parse::<u64>().unwrap()})));
        pull["baseRefName"] = json!(o.base_name);
        pull["commits"] = json!({"nodes":[{"commit":{"oid":o.head,"statusCheckRollup":{"state":o.checks,"contexts":{
                    "totalCount":1,"pageInfo":{"hasNextPage":false},"nodes":[{"__typename":"CheckRun","name":"CI","status":"COMPLETED","conclusion":"SUCCESS"}]}}}}]});
        pull["reviewThreads"] = json!({"totalCount":o.threads.len(),"pageInfo":{"hasNextPage":false},"nodes":o.threads.iter().map(|t|json!({
                    "id":t.id,"isResolved":t.resolved,"isOutdated":t.outdated,"resolvedBy":t.resolved_by.as_ref().map(|login|json!({"login":login})),
                    "comments":{"totalCount":t.comments.len(),"pageInfo":{"hasNextPage":false},"nodes":t.comments}
                })).collect::<Vec<_>>()});
        let value = json!({"data":{"viewer":{"databaseId":22},"repository":{"databaseId":100,"viewerPermission":o.permission,
            "viewerDefaultMergeMethod":o.method,"mergeCommitAllowed":true,"rebaseMergeAllowed":true,"squashMergeAllowed":true,"pullRequest":pull}}});
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}
impl MutationTransport for Wire {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError> {
        let Request::Post(body) = request else {
            panic!("No deletion or bypass allowed")
        };
        let mut server = self.0.lock().unwrap();
        server.writes.push((path.into(), body.clone()));
        if server.reject {
            return Ok(Response {
                status: 403,
                headers: BTreeMap::new(),
                body: b"{}".to_vec(),
            });
        }
        let value = if path == "/graphql" {
            let input = &body["variables"]["input"];
            assert_eq!(input["expectedHeadOid"], server.observation.head);
            assert_eq!(input["mergeMethod"], "SQUASH");
            assert!(input.get("admin").is_none());
            server.observation.state = "MERGED".into();
            server.observation.merged_by = Some("22".into());
            server.observation.merged_at = Some("2026-09-30T00:02:00Z".into());
            server.observation.merge_commit = Some("c".repeat(40));
            json!({"data":{"mergePullRequest":{"clientMutationId":input["clientMutationId"],"pullRequest":{"id":"PR_node","headRefOid":server.observation.head,
                "merged":true,"mergedAt":"2026-09-30T00:02:00Z","mergedBy":{"databaseId":22},"mergeCommit":{"oid":"c".repeat(40)}}}}})
        } else {
            assert_eq!(path, "/repos/example/repo/pulls/1/reviews");
            assert_eq!(body["event"], "APPROVE");
            assert_eq!(body["commit_id"], server.observation.head);
            let r = ProviderReview {
                id: "101".into(),
                actor_id: "22".into(),
                head: server.observation.head.clone(),
                state: "APPROVED".into(),
                body: body["body"].as_str().unwrap().into(),
                submitted_at: Some("2026-09-30T00:02:00Z".into()),
            };
            server.observation.reviews.push(r.clone());
            json!({"id":101,"user":{"id":22},"commit_id":r.head,"state":"APPROVED","body":r.body,"submitted_at":r.submitted_at})
        };
        if server.lost {
            server.lost = false;
            return Err(ConnectionError::Timeout);
        }
        Ok(Response {
            status: if path == "/graphql" { 200 } else { 201 },
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
}

struct Env<'a> {
    store: &'a Store,
    wire: Wire,
    now: i64,
    cancel_after_intent: bool,
    pause_after_intent: bool,
    fail_save: bool,
}
impl host::ActionEnvironment for Env<'_> {
    fn observe(&mut self, _: &Effect) -> Result<Observation, Failure> {
        let mut o = GithubClient::new(self.wire.clone()).action_observation(
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            1,
        )?;
        o.write_capability = true;
        Ok(o)
    }
    fn authorize(&mut self, effect: &Effect, current: &Observation) -> Result<(), Failure> {
        crate::capacity::publication_gate(self.store).map_err(Failure::permanent)?;
        let ledger = self.store.load_actions().map_err(Failure::permanent)?;
        if ledger
            .effects
            .iter()
            .any(|e| e.id == effect.id && e.cancelled)
        {
            return Err(Failure::permanent("Cancelled."));
        }
        ready(
            self.store,
            ledger
                .finals
                .iter()
                .find(|f| f.id == effect.final_id)
                .unwrap(),
            current,
            effect.action,
        )
        .map_err(Failure::permanent)
    }
    fn intent(&mut self, id: &str) -> Result<Effect, Failure> {
        let effect = mark_intent(self.store, id, self.now).map_err(Failure::permanent)?;
        if self.cancel_after_intent {
            cancel_effect(self.store, id).unwrap();
        }
        if self.pause_after_intent {
            self.store
                .save_automation(&crate::capacity::Automation { paused: true })
                .unwrap();
        }
        Ok(effect)
    }
    fn mutate(&mut self, effect: &Effect) -> Result<Receipt, crate::publication::WriteFailure> {
        assert_eq!(
            self.store
                .load_actions()
                .unwrap()
                .effects
                .last()
                .unwrap()
                .state,
            EffectState::Uncertain
        );
        let client = GithubClient::new(self.wire.clone());
        match effect.action {
            Action::Approve => client.approve_exact(
                &RemoteRepository {
                    id: "100".into(),
                    name: "example/repo".into(),
                },
                1,
                "22",
                &effect.observation.head,
                &effect.body,
            ),
            Action::Merge => client.merge_exact(&effect.observation, &effect.id),
        }
    }
    fn finish(
        &mut self,
        effect: &Effect,
        result: Result<Receipt, crate::publication::WriteFailure>,
    ) -> Result<(), Failure> {
        if self.fail_save {
            return Err(Failure::permanent("Injected receipt save failure."));
        }
        finish_effect(self.store, &effect.id, &effect.operation.id, result)
            .map_err(Failure::permanent)
    }
    fn post_check(&mut self, effect: &Effect) -> Result<(), Failure> {
        let current = self.observe(effect);
        verify_after_effect(self.store, effect, current)
    }
}
fn env(store: &Store) -> Env<'_> {
    Env {
        store,
        wire: Wire(Arc::new(Mutex::new(Server {
            observation: observed(),
            writes: vec![],
            lost: false,
            reject: false,
        }))),
        now: NOW + 20,
        cancel_after_intent: false,
        pause_after_intent: false,
        fail_save: false,
    }
}

#[test]
fn lost_responses_crash_before_receipt_and_external_merge_never_blindly_repeat() {
    for action in [Action::Approve, Action::Merge] {
        for crash in [false, true] {
            let (_root, store, item) = fixture(1, true, true);
            let run = completed_final(&store, &item);
            let mut env = env(&store);
            if action == Action::Merge {
                let approval =
                    prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 19)
                        .unwrap();
                host::execute_action(&mut env, &approval).unwrap();
            }
            let before = env.wire.0.lock().unwrap().writes.len();
            let current = env.wire.0.lock().unwrap().observation.clone();
            let effect = prepare_effect(&store, &run.id, action, &current, NOW + 20).unwrap();
            env.wire.0.lock().unwrap().lost = !crash;
            env.fail_save = crash;
            let result = host::execute_action(&mut env, &effect);
            assert_eq!(result.is_err(), crash);
            assert_eq!(
                store
                    .load_actions()
                    .unwrap()
                    .effects
                    .iter()
                    .find(|saved| saved.id == effect.id)
                    .unwrap()
                    .state,
                EffectState::Uncertain
            );
            restore(&store).unwrap();
            let current = env.wire.0.lock().unwrap().observation.clone();
            synchronize(&store, &item, Ok(current), NOW + 30).unwrap();
            let saved = store
                .load_actions()
                .unwrap()
                .effects
                .into_iter()
                .find(|saved| saved.id == effect.id)
                .unwrap();
            assert_eq!(saved.id, effect.id);
            if action == Action::Approve {
                assert_eq!(saved.state, EffectState::Confirmed);
                assert!(saved.receipt.is_some());
            } else {
                assert_eq!(saved.state, EffectState::ExternalMerge);
                assert!(saved.receipt.is_none());
            }
            assert_eq!(env.wire.0.lock().unwrap().writes.len(), before + 1);
            assert!(host::execute_action(&mut env, &saved).is_err());
        }
    }
}

#[test]
fn pause_cancellation_and_permission_rejection_preserve_original_effect_without_writes_or_bypass() {
    for case in ["pause", "cancel", "reject"] {
        let (_root, store, item) = fixture(1, true, false);
        let run = completed_final(&store, &item);
        let effect =
            prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
        let mut env = env(&store);
        env.pause_after_intent = case == "pause";
        env.cancel_after_intent = case == "cancel";
        env.wire.0.lock().unwrap().reject = case == "reject";
        host::execute_action(&mut env, &effect).unwrap();
        let saved = store.load_actions().unwrap().effects.remove(0);
        assert_eq!(saved.state, EffectState::Rejected);
        assert!(saved.receipt.is_none());
        assert!(saved.error.is_some());
        assert_eq!(
            env.wire.0.lock().unwrap().writes.len(),
            usize::from(case == "reject")
        );
        assert!(prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 30).is_err());
    }
}

#[test]
fn mutation_owner_serializes_actions_with_review_and_reply_publications() {
    let owner = host::MutationOwner::default();
    assert!(owner.acquire("comment").unwrap());
    assert!(!owner.acquire("approve").unwrap());
    assert!(!owner.acquire("reply").unwrap());
    assert!(owner.release("foreign").is_err());
    owner.release("comment").unwrap();
    assert!(owner.acquire("merge").unwrap());
    owner.release("merge").unwrap();
}

#[test]
fn confirmed_existing_account_approval_allows_merge_without_another_or_fabricated_vote() {
    let (_root, store, item) = fixture(1, true, true);
    let mut observation = observed();
    observation.reviews.push(ProviderReview {
        id: "100".into(),
        actor_id: "22".into(),
        head: observation.head.clone(),
        state: "APPROVED".into(),
        body: "Existing account vote.".into(),
        submitted_at: Some("2026-09-30T00:00:00Z".into()),
    });
    let run = packet_final(&store, &item, &observation);
    assert!(ready(&store, &run, &observation, Action::Approve)
        .unwrap_err()
        .contains("already has an approval"));
    assert!(ready(&store, &run, &observation, Action::Merge).is_ok());
    assert!(
        store.load_actions().unwrap().effects.is_empty(),
        "External approval is not a fabricated PR Sniper receipt."
    );
    observation.reviews[0].state = "DISMISSED".into();
    assert!(ready(&store, &run, &observation, Action::Merge).is_err());
}

struct RulesReadWire {
    wire: Wire,
    status: u16,
    headers: BTreeMap<String, String>,
    body: Value,
}

impl Transport for RulesReadWire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        if path == "/repos/example/repo/rules/branches/main" {
            return Ok(Response {
                status: self.status,
                headers: self.headers.clone(),
                body: serde_json::to_vec(&self.body).unwrap(),
            });
        }
        self.wire.get(path)
    }
}

impl QueryTransport for RulesReadWire {
    fn query(&self, query: &str, variables: Value) -> Result<Response, ConnectionError> {
        self.wire.query(query, variables)
    }
}

#[test]
fn provider_rules_rate_limits_preserve_action_observation_failure_and_backoff() {
    for status in [403, 429] {
        for (extra_headers, expected, delay) in [
            (vec![], ConnectionError::RateLimited, None),
            (
                vec![("retry-after", "60")],
                ConnectionError::RateLimitedAfter(60),
                Some(60),
            ),
            (
                vec![("retry-after", "60"), ("x-ratelimit-reset", "1800000120")],
                ConnectionError::RateLimitedWithContext {
                    retry_after_seconds: Some(60),
                    reset_at: Some(1_800_000_120),
                    organization_access_incomplete: false,
                    missing_repo_scope: false,
                },
                Some(60),
            ),
            (
                vec![
                    ("retry-after", "60"),
                    ("x-ratelimit-reset", "1800000120"),
                    ("x-github-sso", "partial-results; organizations=123"),
                ],
                ConnectionError::RateLimitedWithContext {
                    retry_after_seconds: Some(60),
                    reset_at: Some(1_800_000_120),
                    organization_access_incomplete: true,
                    missing_repo_scope: false,
                },
                Some(60),
            ),
        ] {
            let (_root, store, item) = fixture(1, true, true);
            let wire = env(&store).wire;
            wire.0
                .lock()
                .unwrap()
                .observation
                .reviews
                .push(fixwave::peer_approval());
            let repository = RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            };
            let mut baseline = GithubClient::new(wire.clone())
                .action_observation(&repository, 1)
                .unwrap();
            // The host sets this from its separately verified connection.
            baseline.write_capability = true;
            let final_review = fixwave::prepare_final(&store, &item, &baseline);
            let settings = store.load_settings().unwrap();
            assert!(ready(&store, &final_review, &baseline, Action::Approve).is_ok());
            assert!(ready(&store, &final_review, &baseline, Action::Merge).is_ok());
            let mut headers = BTreeMap::from([("x-ratelimit-remaining".into(), "0".into())]);
            headers.extend(extra_headers.into_iter().map(|(k, v)| (k.into(), v.into())));
            let error = GithubClient::new(RulesReadWire {
                wire: wire.clone(),
                status,
                headers,
                body: json!({"message":"API rate limit exceeded; fixture-private-body"}),
            })
            .action_observation(&repository, 1)
            .unwrap_err();
            assert_eq!(error, expected);
            let failure = Failure::from(error);
            assert_eq!(failure.kind, monitoring::OperationFailure::RateLimited);
            assert_eq!(failure.retry_after_seconds, delay);
            assert!(!failure.message.contains("fixture-private-body"));
            synchronize(&store, &item, Err(failure), NOW + 21).unwrap();
            let ledger = store.load_actions().unwrap();
            assert!(ledger.observations[0].observation.is_none());
            // A rate limit is a wait: no stored error, no failure counted.
            assert!(ledger.observations[0].error.is_none());
            assert_eq!(ledger.observations[0].failures, 0);
            let retry_at = ledger.observations[0].retry_at.unwrap();
            match delay {
                Some(delay) => assert_eq!(retry_at, NOW + 21 + delay),
                None => assert!((NOW + 21 + 30..=NOW + 21 + 33).contains(&retry_at)),
            }
            let gates = store.load_backpressure().unwrap();
            assert_eq!(gates.gates.len(), 1);
            assert_eq!(gates.gates[0].until, retry_at);
            assert_eq!(ledger.finals, vec![final_review.clone()]);
            assert!(ledger.effects.is_empty());
            assert_eq!(store.load_settings().unwrap(), settings);
            validate_local(&store, &final_review).unwrap();
            assert!(wire.0.lock().unwrap().writes.is_empty());
        }
    }
}

#[test]
fn provider_rules_reset_only_rate_limit_preserves_reported_retry_interval() {
    let (_root, store, item) = fixture(1, true, true);
    let final_review = completed_final(&store, &item);
    let wire = env(&store).wire;
    let reset = crate::now_seconds().unwrap() + 120;
    let error = GithubClient::new(RulesReadWire {
        wire: wire.clone(),
        status: 403,
        headers: BTreeMap::from([
            ("x-ratelimit-remaining".into(), "0".into()),
            ("x-ratelimit-reset".into(), reset.to_string()),
        ]),
        body: json!({"message":"API rate limit exceeded; fixture-private-body"}),
    })
    .action_observation(
        &RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        },
        1,
    )
    .unwrap_err();
    assert_eq!(
        error,
        ConnectionError::RateLimitedWithContext {
            retry_after_seconds: None,
            reset_at: Some(reset),
            organization_access_incomplete: false,
            missing_repo_scope: false,
        }
    );
    let before = crate::now_seconds().unwrap();
    let failure = Failure::from(error);
    let after = crate::now_seconds().unwrap();
    assert_eq!(failure.kind, monitoring::OperationFailure::RateLimited);
    let delay = failure.retry_after_seconds.unwrap();
    assert!(delay >= reset.saturating_sub(after).max(0));
    assert!(delay <= reset.saturating_sub(before).max(0));
    assert!(!failure.message.contains("fixture-private-body"));
    synchronize(&store, &item, Err(failure), NOW + 21).unwrap();
    let ledger = store.load_actions().unwrap();
    assert_eq!(ledger.observations[0].retry_at, Some(NOW + 21 + delay));
    assert_eq!(ledger.observations[0].failures, 0);
    assert!(ledger.observations[0].error.is_none());
    assert!(ledger.observations[0].observation.is_none());
    assert_eq!(ledger.finals, vec![final_review.clone()]);
    validate_local(&store, &final_review).unwrap();
    assert!(ledger.effects.is_empty());
    assert!(wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn provider_rules_unsupported_or_denied_remain_approval_independent_and_merge_blocking() {
    for (status, body) in [
        (200, json!([{"unsupported_rule_shape":true}])),
        (
            403,
            json!({"message":"Rules access denied; fixture-private-body"}),
        ),
    ] {
        let (_root, store, _) = fixture(1, true, true);
        let wire = env(&store).wire;
        let mut observation = GithubClient::new(RulesReadWire {
            wire: wire.clone(),
            status,
            headers: BTreeMap::new(),
            body,
        })
        .action_observation(
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            1,
        )
        .unwrap();
        observation.write_capability = true;
        assert!(observation.merge_rules.is_none());
        assert!(observation.merge_rules_error.is_some());
        assert!(observation.blocker(Action::Approve).is_none());
        assert!(observation.blocker(Action::Merge).is_some());
        assert!(!serde_json::to_string(&observation)
            .unwrap()
            .contains("fixture-private-body"));
        assert!(wire.0.lock().unwrap().writes.is_empty());
    }
}

struct ShapeWire {
    wire: Wire,
    case: &'static str,
}
impl Transport for ShapeWire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        if path.contains("/rules/branches/") {
            if self.case == "rules_unavailable" {
                return Err(ConnectionError::MissingReadPermission);
            }
            if self.case == "queue_rule" {
                return Ok(Response {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: serde_json::to_vec(&json!([{"type":"merge_queue"}])).unwrap(),
                });
            }
        }
        self.wire.get(path)
    }
}
impl QueryTransport for ShapeWire {
    fn query(&self, query: &str, variables: Value) -> Result<Response, ConnectionError> {
        let response = self.wire.query(query, variables)?;
        let mut value: Value = serde_json::from_slice(&response.body).unwrap();
        let pull = &mut value["data"]["repository"]["pullRequest"];
        match self.case {
            "partial_threads" => pull["reviewThreads"]["pageInfo"]["hasNextPage"] = json!(true),
            "partial_checks" => {
                pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]
                    ["totalCount"] = json!(2)
            }
            "wrong_check_head" => {
                pull["commits"]["nodes"][0]["commit"]["oid"] = json!("d".repeat(40))
            }
            "missing_policy" => {
                pull["baseRef"]
                    .as_object_mut()
                    .unwrap()
                    .remove("branchProtectionRule");
            }
            "nonpassing" => {
                pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"][0]
                    ["conclusion"] = json!("FAILURE")
            }
            "no_checks" => {
                pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"] = Value::Null;
            }
            "errors" => value["errors"] = json!([{"message":"unsupported provider capability"}]),
            "queue_rule" | "rules_unavailable" => {}
            _ => panic!("Unknown fixture"),
        }
        Ok(Response {
            body: serde_json::to_vec(&value).unwrap(),
            ..response
        })
    }
}
#[test]
fn provider_gate_reads_fail_closed_on_partial_policy_and_ci_evidence() {
    let (_root, store, _) = fixture(1, true, true);
    let wire = env(&store).wire;
    for case in [
        "partial_threads",
        "partial_checks",
        "wrong_check_head",
        "missing_policy",
        "errors",
        "nonpassing",
        "no_checks",
        "queue_rule",
        "rules_unavailable",
    ] {
        let result = GithubClient::new(ShapeWire {
            wire: wire.clone(),
            case,
        })
        .action_observation(
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            1,
        );
        if matches!(
            case,
            "nonpassing" | "no_checks" | "queue_rule" | "rules_unavailable"
        ) {
            let mut observation = result.unwrap();
            observation.write_capability = true;
            assert!(observation.blocker(Action::Merge).is_some());
            assert!(observation.blocker(Action::Approve).is_none());
        } else {
            assert!(result.is_err(), "{case}");
        }
    }
    assert!(wire.0.lock().unwrap().writes.is_empty());
}

#[test]
fn provider_receipts_produce_distinct_approval_and_merge_notifications_without_human_review_claims()
{
    let (_root, store, item) = fixture(1, true, true);
    let run = completed_final(&store, &item);
    let approval = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &approval).unwrap();
    let frames = crate::notifications::frames(&queue::snapshot(&store, vec![]).unwrap());
    assert!(frames.iter().any(|f| f
        .event
        .as_ref()
        .is_some_and(|e| e.category == crate::notifications::Category::Approved)));
    let mut external = env.wire.0.lock().unwrap().observation.clone();
    external.state = "MERGED".into();
    external.merged_by = Some("77".into());
    external.merged_at = Some("2026-09-30T00:04:00Z".into());
    external.merge_commit = Some("c".repeat(40));
    synchronize(&store, &item, Ok(external), NOW + 30).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    assert_eq!(snapshot.items[0].state, State::Merged);
    assert!(snapshot.items[0]
        .action_status
        .as_ref()
        .unwrap()
        .effects
        .iter()
        .all(|e| e.action != Action::Merge));
    assert!(crate::notifications::frames(&snapshot).iter().any(|f| f
        .event
        .as_ref()
        .is_some_and(|e| e.category == crate::notifications::Category::Merged)));
}

#[test]
fn wrong_provider_identity_cannot_create_a_final_or_mark_another_pr_merged() {
    let (_root, store, item) = fixture(1, true, true);
    for wrong in ["account", "repository", "pull"] {
        let mut current = observed();
        current.state = "MERGED".into();
        match wrong {
            "account" => current.account_id = "44".into(),
            "repository" => current.repository_id = "200".into(),
            _ => current.pull_request_id = "99".into(),
        }
        synchronize(&store, &item, Ok(current), NOW + 10).unwrap();
        let ledger = store.load_actions().unwrap();
        assert!(ledger.observations[0].error.is_some());
        assert!(ledger.finals.is_empty());
        assert!(ledger.effects.is_empty());
        assert_eq!(
            store.load_queue_state().unwrap().tracked[0].lifecycle,
            Lifecycle::Open
        );
        assert_ne!(
            queue::snapshot(&store, vec![]).unwrap().items[0].state,
            State::Merged
        );
    }
}

#[test]
fn observation_retry_after_and_original_mutation_ownership_survive_retry_requests() {
    let (_root, store, item) = fixture(1, true, false);
    let run = completed_final(&store, &item);
    let effect = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    let intent = mark_intent(&store, &effect.id, NOW + 20).unwrap();
    let error = Failure {
        cancelled: false,
        kind: monitoring::OperationFailure::RateLimited,
        message: "Provider read rate limited.".into(),
        retry_after_seconds: Some(60),
        origin: crate::backpressure::Origin::Github,
        duck_setup: false,
    };
    let network = Failure {
        kind: monitoring::OperationFailure::Network,
        message: "Provider read unreachable.".into(),
        ..error.clone()
    };
    synchronize(&store, &item, Err(error.clone()), NOW + 21).unwrap();
    assert_eq!(
        store.load_actions().unwrap().observations[0].retry_at,
        Some(NOW + 81)
    );
    assert!(request_observation_retry(&store, &item, NOW + 22).is_err());
    // Rate limits never spend the budget; real failures still do.
    for i in 1..=4 {
        synchronize(&store, &item, Err(network.clone()), NOW + 21 + i * 60).unwrap();
    }
    assert_eq!(store.load_actions().unwrap().observations[0].retry_at, None);
    request_observation_retry(&store, &item, NOW + 300).unwrap();
    let saved = store.load_actions().unwrap().effects.remove(0);
    assert_eq!(saved, intent);
    assert!(finish_effect(
        &store,
        &effect.id,
        "obsolete-operation",
        Ok(Receipt {
            id: "101".into(),
            actor_id: "22".into(),
            head: "a".repeat(40),
            action: Action::Approve,
            merge_commit: None
        })
    )
    .is_err());
    assert_eq!(store.load_actions().unwrap().effects[0], intent);
}

#[test]
fn a_post_approval_revision_change_retains_the_vote_but_blocks_merge() {
    let (_root, store, item) = fixture(1, true, true);
    let run = completed_final(&store, &item);
    let effect = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    let mut env = env(&store);
    host::execute_action(&mut env, &effect).unwrap();
    let receipt = store.load_actions().unwrap().effects[0].receipt.clone();
    let mut current = env.wire.0.lock().unwrap().observation.clone();
    current.base = "d".repeat(40);
    assert!(verify_after_effect(&store, &effect, Ok(current.clone())).is_err());
    let saved = store.load_actions().unwrap().effects.remove(0);
    assert_eq!(saved.state, EffectState::Confirmed);
    assert_eq!(saved.receipt, receipt);
    assert!(saved.error.unwrap().contains("post-action"));
    assert!(ready(&store, &run, &current, Action::Merge).is_err());
    assert_eq!(env.wire.0.lock().unwrap().writes.len(), 1);
}

#[test]
fn legacy_inert_flags_and_expired_unattempted_intents_never_become_provider_grants() {
    let (_root, store, item) = fixture(1, true, true);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].actions = None;
    assert!(settings.repositories[0].assignments[0].approve);
    store.save_settings(&settings).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    assert!(store.load_actions().unwrap().finals.is_empty());
    assert_eq!(
        queue::snapshot(&store, vec![]).unwrap().items[0]
            .action_status
            .as_ref()
            .unwrap()
            .permissions,
        ActionPermissions {
            include_self: false,
            reply: false,
            approve: false,
            merge: false
        }
    );
    settings.repositories[0].assignments[0].actions = Some(ActionPermissions {
        include_self: false,
        reply: false,
        approve: true,
        merge: false,
    });
    store.save_settings(&settings).unwrap();
    let run = completed_final(&store, &item);
    let intent = prepare_effect(&store, &run.id, Action::Approve, &observed(), NOW + 20).unwrap();
    expire_prepared(&store, intent.operation.retry_deadline).unwrap();
    let expired = store.load_actions().unwrap().effects.remove(0);
    assert_eq!(expired.state, EffectState::Stale);
    assert!(expired.operation.attempted_mutation.is_none());
    assert!(mark_intent(&store, &expired.id, NOW + 1000).is_err());
}
