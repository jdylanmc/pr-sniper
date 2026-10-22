use super::*;
use crate::{
    feedback::{Context, Record},
    github::threads::{Comment, Thread},
    monitoring::OperationFailure,
    publication::{Publication, Receipt, RemoteState},
};

const CHANGED: &str = "Owned feedback changed during analysis; no stale assessment accepted.";

fn feedback_fixture(teammate: bool) -> (tempfile::TempDir, Store, ReviewRun) {
    let (root, store, mut template) = packet_work_fixture();
    if teammate {
        let mut settings = store.load_settings().unwrap();
        let mut primary = settings.repositories[0].assignments[0].clone();
        primary.id = uuid::Uuid::new_v4().to_string();
        let mut agent = settings.agents[0].clone();
        agent.id = uuid::Uuid::new_v4().to_string();
        agent.name = "Primary".into();
        primary.agent_id = agent.id.clone();
        settings.repositories[0].primary_assignment_id = Some(primary.id.clone());
        settings.repositories[0].assignments.push(primary);
        settings.agents.push(agent);
        store.save_settings(&settings).unwrap();
        template.selection =
            Selection::resolve(&settings, &template.job, &template.assignment_id).unwrap();
    }
    let mut origin = template.clone();
    origin.job.work.as_mut().unwrap().iteration_id = "earlier".into();
    origin.job.work.as_mut().unwrap().id = "earlier-work".into();
    origin.job.head_sha = "b".repeat(40);
    origin.key = key(&origin.job, &origin.assignment_id);
    origin.operation = monitoring::JobOperation::review(&origin.job, 90);
    origin.operation.state = OperationState::Completed;
    origin.result = Some(packet_result(
        json!({"exit":"ready_for_human","summary":"Prior."}),
    ));
    let mut publication = Publication::new(origin, false, true, 90).unwrap();
    publication.receipts.push(Receipt {
        review_id: "review".into(),
        state: RemoteState::Commented,
        comment_ids: vec!["root".into()],
    });
    let context = Context {
        id: crate::feedback::root_key(&template.job, "root"),
        publication_id: publication.id.clone(),
        owner_agent_id: template.selection.agent.id.clone(),
        owner_assignment_id: template.assignment_id.clone(),
        original_head: "b".repeat(40),
        root_id: "root".into(),
        path: "src/example.rs".into(),
        title: "An existing concern".into(),
        body: "Check this boundary.".into(),
        thread: Some(Thread {
            id: "thread".into(),
            resolved: false,
            can_reply: true,
            comments: vec![Comment {
                id: "root".into(),
                body: "Check this boundary.".into(),
                author_id: Some("22".into()),
                author_login: Some("owner".into()),
                reply_to: None,
                review_id: Some("review".into()),
                original_commit: Some("b".repeat(40)),
                created_at: "2026-10-20T18:00:00Z".into(),
                published_at: "2026-10-20T18:00:00Z".into(),
            }],
        }),
        closed: false,
        unavailable: None,
    };
    let mut ledger = store.load_feedback().unwrap();
    ledger.records.push(Record {
        context,
        job: publication.review.job.clone(),
        observed_head: template.job.head_sha.clone(),
    });
    store.save_feedback(&ledger).unwrap();
    store.save_publications(&[publication]).unwrap();
    (root, store, template)
}

fn change_feedback(store: &Store) {
    let mut ledger = store.load_feedback().unwrap();
    let thread = ledger.records[0].context.thread.as_mut().unwrap();
    let mut reply = thread.comments[0].clone();
    reply.id = format!("reply-{}", thread.comments.len());
    reply.reply_to = Some("root".into());
    reply.body = "Author supplied new evidence.".into();
    thread.comments.push(reply);
    store.save_feedback(&ledger).unwrap();
}

#[test]
fn owned_feedback_movement_retries_both_roles_and_rejects_stale_results() {
    for teammate in [false, true] {
        let (root, store, template) = feedback_fixture(teammate);
        let run = running_packet(&store, &template, 100);
        assert_eq!(run.feedback_context.as_ref().unwrap().len(), 1);
        crate::review::validate_execution_selection(&store, &run).unwrap();
        change_feedback(&store);

        let failure = crate::review::validate_execution_selection(&store, &run).unwrap_err();
        complete(&store, &run.operation.id, Err(failure), 101).unwrap();

        let restarted = Store::new(root.path().into());
        crate::review::restore(&restarted).unwrap();
        let saved = restarted.load_reviews().unwrap().remove(0);
        assert_eq!(saved.operation.failure, Some(OperationFailure::Tossed));
        assert_eq!(saved.operation.state, OperationState::Queued);
        assert_eq!(saved.operation.attempt_count, 1);
        assert!(saved.result.is_none());
        let states = restarted.load_pr_states().unwrap();
        let state = states
            .get(&crate::retention::Binding::job(&run.job))
            .unwrap();
        assert_eq!(state.error_count, 1);
        assert!(state.baseline_hash.is_none() && state.exit.is_none());
        assert!(restarted.load_feedback().unwrap().exit_writes.is_empty());
        assert_eq!(restarted.load_publications().unwrap().len(), 1);

        let next = running_packet(
            &restarted,
            &template,
            saved.operation.next_attempt_at.unwrap(),
        );
        assert_eq!(next.operation.id, run.operation.id);
        assert_eq!(next.operation.attempt_count, 2);
        assert_ne!(next.feedback_context, run.feedback_context);
        crate::review::validate_execution_selection(&restarted, &next).unwrap();
        let exit = if teammate {
            json!({"exit":"submit_report","findings":[]})
        } else {
            json!({"exit":"ready_for_human","summary":"Fresh assessment."})
        };
        complete(
            &restarted,
            &next.operation.id,
            Ok(packet_result(exit)),
            next.operation.initial_attempt_at + 60,
        )
        .unwrap();
        assert_eq!(
            restarted.load_reviews().unwrap()[0].operation.state,
            OperationState::Completed
        );
    }
}

#[test]
fn owned_feedback_churn_uses_three_attempts_not_unlimited_supersession() {
    let (_root, store, template) = feedback_fixture(false);
    let mut now = 100;
    for count in 1..=3 {
        let run = running_packet(&store, &template, now);
        change_feedback(&store);
        let failure = crate::review::validate_execution_selection(&store, &run).unwrap_err();
        complete(&store, &run.operation.id, Err(failure), now + 1).unwrap();
        let saved = store.load_reviews().unwrap().remove(0);
        assert_eq!(saved.operation.attempt_count, count);
        assert_eq!(saved.operation.failure, Some(OperationFailure::Tossed));
        assert!(saved.result.is_none());
        if count < 3 {
            assert_eq!(saved.operation.state, OperationState::Queued);
            now = saved.operation.next_attempt_at.unwrap();
        } else {
            assert_eq!(saved.operation.state, OperationState::ManualRetry);
            assert!(saved.operation.resume_at.is_some());
        }
    }
}

#[test]
fn owned_feedback_comparison_ignores_unrelated_records_and_publication_bookkeeping() {
    for teammate in [false, true] {
        let (_root, store, template) = feedback_fixture(teammate);
        let run = running_packet(&store, &template, 100);
        let mut ledger = store.load_feedback().unwrap();
        ledger.records[0].job.waiting = "different bookkeeping".into();
        ledger.records[0].job.detected_at += 10;
        let mut unrelated = ledger.records[0].clone();
        unrelated.job.pull_request_id = "other-pr".into();
        unrelated.context.id = crate::feedback::root_key(&unrelated.job, "other-root");
        unrelated.context.closed = true;
        ledger.records.insert(0, unrelated);
        store.save_feedback(&ledger).unwrap();
        let mut publications = store.load_publications().unwrap();
        publications[0].operation.retry_deadline += 10;
        publications[0].error = Some("Unrelated publication bookkeeping".into());
        // Another assignment's open feedback on the current iteration is not
        // part of this legacy owned-context gate (the packet sees all threads).
        let mut peer = publications[0].clone();
        peer.id = "peer-publication".into();
        peer.review.job.work = run.job.work.clone();
        peer.review.selection.agent.id = "other-agent".into();
        peer.receipts[0].comment_ids = vec!["peer-root".into()];
        publications.push(peer);
        store.save_publications(&publications).unwrap();

        crate::review::validate_execution_selection(&store, &run).unwrap();
    }
}

#[test]
fn owned_feedback_closure_and_ownership_changes_still_reject_both_roles() {
    for teammate in [false, true] {
        for closure in [false, true] {
            let (_root, store, template) = feedback_fixture(teammate);
            let run = running_packet(&store, &template, 100);
            let mut ledger = store.load_feedback().unwrap();
            if closure {
                ledger.records[0].context.closed = true;
                ledger.records[0].context.thread.as_mut().unwrap().resolved = true;
            } else {
                ledger.records[0].context.owner_assignment_id = "different-owner".into();
            }
            store.save_feedback(&ledger).unwrap();
            let failure = crate::review::validate_execution_selection(&store, &run).unwrap_err();
            assert_eq!(failure.kind, OperationFailure::Tossed);
        }
    }
}

#[test]
fn owned_feedback_old_permanent_failure_recovers_once_without_resetting_budget() {
    for teammate in [false, true] {
        let (root, store, template) = feedback_fixture(teammate);
        let run = running_packet(&store, &template, 100);
        change_feedback(&store);
        complete(
            &store,
            &run.operation.id,
            Err(Failure::permanent(CHANGED)),
            101,
        )
        .unwrap();
        let restarted = Store::new(root.path().into());
        crate::review::restore(&restarted).unwrap();
        let recovered = restarted.load_reviews().unwrap().remove(0);
        assert_eq!(recovered.operation.id, run.operation.id);
        assert_eq!(recovered.operation.attempt_count, 1);
        assert_eq!(
            recovered.operation.retry_deadline,
            run.operation.retry_deadline
        );
        assert_eq!(recovered.operation.failure, Some(OperationFailure::Tossed));
        // The old deadline expired: honour cooldown instead of granting an
        // unbounded fresh window on every restart.
        assert_eq!(recovered.operation.state, OperationState::ManualRetry);
        assert!(recovered.operation.resume_at.is_some());
        assert!(recovered.result.is_none());
        crate::review::restore(&restarted).unwrap();
        assert_eq!(restarted.load_reviews().unwrap()[0], recovered);

        let mut next = recovered;
        assert!(!next
            .operation
            .auto_resume(false, next.operation.resume_at.unwrap() - 1));
        assert!(next
            .operation
            .auto_resume(false, next.operation.resume_at.unwrap()));
        assert_eq!(next.operation.resume_cycle, 1);
        restarted.save_reviews(&[next.clone()]).unwrap();
        let fresh = running_packet(
            &restarted,
            &template,
            next.operation.next_attempt_at.unwrap(),
        );
        assert_eq!(fresh.operation.id, run.operation.id);
        crate::review::validate_execution_selection(&restarted, &fresh).unwrap();
    }
}

#[test]
fn owned_feedback_recovery_does_not_revive_retired_or_unrelated_failures() {
    for retired in [false, true] {
        let (_root, store, template) = feedback_fixture(false);
        let run = running_packet(&store, &template, 100);
        let message = if retired {
            CHANGED
        } else {
            "An unrelated permanent failure."
        };
        complete(
            &store,
            &run.operation.id,
            Err(Failure::permanent(message)),
            101,
        )
        .unwrap();
        if retired {
            let mut jobs = store.load_queue().unwrap();
            jobs[0].waiting = monitoring::WAITING_SUPERSEDED.into();
            store.save_queue(&jobs).unwrap();
        }
        crate::review::restore(&store).unwrap();
        let saved = store.load_reviews().unwrap().remove(0);
        if retired {
            assert!(saved.operation.ended_superseded());
        } else {
            assert_eq!(saved.operation.failure, Some(OperationFailure::Permanent));
            assert_eq!(saved.error.as_deref(), Some(message));
        }
        assert!(saved.operation.next_attempt_at.is_none());
        assert!(saved.operation.resume_at.is_none());
    }
}

#[test]
fn owned_feedback_recovery_requires_an_unfinished_packet_review_without_effects() {
    for excluded in ["legacy", "missing_context", "result", "mutation", "receipt"] {
        let (_root, store, template) = feedback_fixture(false);
        let run = running_packet(&store, &template, 100);
        complete(
            &store,
            &run.operation.id,
            Err(Failure::permanent(CHANGED)),
            101,
        )
        .unwrap();
        let mut saved = store.load_reviews().unwrap().remove(0);
        match excluded {
            "legacy" => saved.mode = crate::review::RunMode::Legacy,
            "missing_context" => saved.feedback_context = None,
            "result" => {
                saved.result = Some(packet_result(
                    json!({"exit":"ready_for_human","summary":"Existing result."}),
                ))
            }
            "mutation" => saved.operation.attempted_mutation = Some("sent".into()),
            "receipt" => saved.operation.confirmed_receipt = Some("receipt".into()),
            _ => unreachable!(),
        }
        store.save_reviews(&[saved.clone()]).unwrap();
        crate::review::restore(&store).unwrap();
        assert_eq!(store.load_reviews().unwrap()[0], saved, "{excluded}");
    }
}

#[test]
fn owned_feedback_blocked_migration_keeps_the_operation_until_unblocked() {
    for teammate in [false, true] {
        for blocked in [
            monitoring::WAITING_ACCOUNT_DISCONNECTED,
            monitoring::WAITING_REPOSITORY_DISABLED,
            monitoring::WAITING_BINDING_CHANGED,
            monitoring::WAITING_POLICY_CHANGED,
        ] {
            let (root, store, template) = feedback_fixture(teammate);
            let now = crate::now_seconds().unwrap();
            let run = running_packet(&store, &template, now - 20);
            complete(
                &store,
                &run.operation.id,
                Err(Failure::permanent(CHANGED)),
                now - 19,
            )
            .unwrap();
            let mut reviews = store.load_reviews().unwrap();
            reviews[0].operation.resume_cycle = 2;
            store.save_reviews(&reviews).unwrap();
            let mut jobs = store.load_queue().unwrap();
            jobs[0].waiting = blocked.into();
            store.save_queue(&jobs).unwrap();

            let restarted = Store::new(root.path().into());
            let before_restore = crate::now_seconds().unwrap();
            crate::review::restore(&restarted).unwrap();
            let after_restore = crate::now_seconds().unwrap();
            let recovered = restarted.load_reviews().unwrap().remove(0);
            assert_eq!(
                recovered.operation.failure,
                Some(OperationFailure::Tossed),
                "{blocked}"
            );
            assert_eq!(
                recovered.operation.state,
                OperationState::Queued,
                "role_teammate={teammate} blocked={blocked} fixture_now={now} \
                 restore_clock=[{before_restore},{after_restore}] \
                 initial={} deadline={} count={} cycle={} next={:?} resume={:?}",
                recovered.operation.initial_attempt_at,
                recovered.operation.retry_deadline,
                recovered.operation.attempt_count,
                recovered.operation.resume_cycle,
                recovered.operation.next_attempt_at,
                recovered.operation.resume_at
            );
            assert_eq!(recovered.operation.id, run.operation.id);
            assert_eq!(recovered.operation.attempt_count, 1);
            assert_eq!(
                recovered.operation.retry_deadline,
                run.operation.retry_deadline
            );
            assert_eq!(recovered.operation.resume_cycle, 2);
            let next = recovered.operation.next_attempt_at.unwrap();
            assert!(prepare_dispatch(&restarted, &run.key, next).is_err());
            crate::review::restore(&restarted).unwrap();
            assert_eq!(restarted.load_reviews().unwrap()[0], recovered);

            jobs[0].waiting = monitoring::WAITING_HUMAN_START.into();
            restarted.save_queue(&jobs).unwrap();
            let fresh = prepare_dispatch(&restarted, &run.key, next).unwrap();
            assert_eq!(fresh.operation.id, run.operation.id);
            assert_eq!(fresh.operation.attempt_count, 2);
            assert_eq!(fresh.operation.retry_deadline, run.operation.retry_deadline);
            assert_eq!(fresh.operation.resume_cycle, 2);
            assert_eq!(restarted.load_reviews().unwrap().len(), 1);
        }
    }
}

struct PreparationWire {
    during_read: Box<dyn Fn()>,
    error: Option<crate::github::ConnectionError>,
}

impl crate::github::provider::Transport for PreparationWire {
    fn get(
        &self,
        _: &str,
    ) -> Result<crate::github::provider::Response, crate::github::ConnectionError> {
        (self.during_read)();
        if let Some(error) = &self.error {
            return Err(*error);
        }
        Ok(crate::github::provider::Response {
            status: 200,
            headers: Default::default(),
            body: b"{}".to_vec(),
        })
    }
}

impl crate::github::threads::QueryTransport for PreparationWire {
    fn query(
        &self,
        _: &str,
        _: serde_json::Value,
    ) -> Result<crate::github::provider::Response, crate::github::ConnectionError> {
        crate::github::provider::Transport::get(self, "graphql")
    }
}

#[test]
fn owned_feedback_guarded_preparation_preserves_in_flight_tosses_for_both_roles() {
    use crate::github::provider::GithubClient;
    use std::sync::atomic::AtomicUsize;

    for teammate in [false, true] {
        for snapshot in [false, true] {
            let (root, store, template) = feedback_fixture(teammate);
            let run = running_packet(&store, &template, 100);
            let calls = Arc::new(AtomicUsize::new(0));
            let read_calls = calls.clone();
            let path = root.path().to_owned();
            let client = GithubClient::new(PreparationWire {
                during_read: Box::new(move || {
                    read_calls.fetch_add(1, Ordering::SeqCst);
                    change_feedback(&Store::new(path.clone()));
                }),
                error: None,
            });
            let gate_path = root.path().to_owned();
            let gate_run = run.clone();
            let gate = Arc::new(move || {
                crate::review::validate_execution_selection(
                    &Store::new(gate_path.clone()),
                    &gate_run,
                )
            });
            let repo = RemoteRepository {
                id: run.job.repository_id.clone(),
                name: run.job.repository_name.clone(),
            };
            let outcome = AttemptGuard::new(gate).prepare(client, |client| {
                if snapshot {
                    client
                        .pr_snapshot(&repo, run.job.number)
                        .map(|_| ())
                        .map_err(Failure::from)
                } else {
                    client
                        .review_context(&repo, run.job.number, &run.job.head_sha)
                        .map(|_| ())
                        .map_err(Failure::from)
                }
            });
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "the provider read must actually start"
            );
            let failure = outcome.unwrap_err();
            complete(&store, &run.operation.id, Err(failure), 101).unwrap();
            let saved = store.load_reviews().unwrap().remove(0);
            assert_eq!(saved.operation.failure, Some(OperationFailure::Tossed));
            assert_eq!(saved.operation.state, OperationState::Queued);
            assert_eq!(saved.operation.attempt_count, 1);
            assert!(saved.operation.next_attempt_at.unwrap() > 101);
            assert!(saved.result.is_none());
            assert_eq!(
                store
                    .load_pr_states()
                    .unwrap()
                    .get(&crate::retention::Binding::job(&run.job))
                    .unwrap()
                    .error_count,
                1
            );
            assert!(store.load_feedback().unwrap().exit_writes.is_empty());
        }
    }
}

fn assert_same_failure(actual: &Failure, expected: &Failure) {
    assert_eq!(actual.kind, expected.kind);
    assert_eq!(actual.cancelled, expected.cancelled);
    assert_eq!(actual.origin, expected.origin);
    assert_eq!(actual.resume, expected.resume);
    assert_eq!(actual.retry_after_seconds, expected.retry_after_seconds);
    assert_eq!(actual.message, expected.message);
}

#[test]
fn preparation_guard_preserves_local_provenance_and_rejects_ignored_refusals() {
    use crate::github::{provider::GithubClient, ConnectionError};
    for expected in [
        Failure::cancelled(),
        Failure::timeout(),
        Failure::policy(monitoring::PolicyRejection::RevisionChanged),
        Failure::superseded("Base moved").after(crate::review::Resume::NewBase("old-base".into())),
        Failure::from(ConnectionError::SignedOut),
        Failure::from(ConnectionError::MissingReadPermission),
    ] {
        for ignore_refusal in [false, true] {
            let read_finished = Arc::new(AtomicBool::new(false));
            let on_read = read_finished.clone();
            let client = GithubClient::new(PreparationWire {
                during_read: Box::new(move || {
                    on_read.store(true, Ordering::SeqCst);
                }),
                error: None,
            });
            let refusal = expected.clone();
            let gate_finished = read_finished.clone();
            let gate = Arc::new(move || {
                if gate_finished.load(Ordering::SeqCst) {
                    Err(refusal.clone())
                } else {
                    Ok(())
                }
            });
            let repo = RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            };
            let attempt = AttemptGuard::new(gate);
            let actual = attempt
                .prepare(client, |client| {
                    let read = client
                        .pr_snapshot(&repo, 1)
                        .map(|_| ())
                        .map_err(Failure::from);
                    if ignore_refusal {
                        Ok(())
                    } else {
                        read
                    }
                })
                .unwrap_err();
            assert_same_failure(&actual, &expected);
            assert_same_failure(
                &attempt.commit(Failure::provider("Primary runtime terminal error")),
                &expected,
            );
            read_finished.store(false, Ordering::SeqCst);
            assert_same_failure(&attempt.gate()().unwrap_err(), &expected);
            assert_same_failure(
                &attempt
                    .finish::<()>(Err(Failure::provider("Later runtime failure")))
                    .unwrap_err(),
                &expected,
            );
        }
    }
}

#[test]
fn preparation_guard_keeps_actual_provider_errors_when_no_guard_refused() {
    use crate::github::{provider::GithubClient, ConnectionError};
    for provider in [
        ConnectionError::Network,
        ConnectionError::Timeout,
        ConnectionError::SignedOut,
        ConnectionError::MissingReadPermission,
        ConnectionError::RateLimitedAfter(47),
        ConnectionError::Configuration,
    ] {
        let read_finished = Arc::new(AtomicBool::new(false));
        let on_read = read_finished.clone();
        let client = GithubClient::new(PreparationWire {
            during_read: Box::new(move || {
                on_read.store(true, Ordering::SeqCst);
            }),
            error: Some(provider),
        });
        let gate = Arc::new(move || {
            if read_finished.load(Ordering::SeqCst) {
                Err(Failure::tossed(
                    "A later local change must not overwrite a provider error.",
                ))
            } else {
                Ok(())
            }
        });
        let repo = RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        };
        let actual = AttemptGuard::new(gate)
            .prepare(client, |client| {
                client
                    .pr_snapshot(&repo, 1)
                    .map(|_| ())
                    .map_err(Failure::from)
            })
            .unwrap_err();
        assert_same_failure(&actual, &Failure::from(provider));
    }
}

struct FinalSnapshotWire {
    during_read: Box<dyn Fn()>,
}

impl crate::github::provider::Transport for FinalSnapshotWire {
    fn get(
        &self,
        _: &str,
    ) -> Result<crate::github::provider::Response, crate::github::ConnectionError> {
        unreachable!("snapshot-only fixture")
    }
}

impl crate::github::threads::QueryTransport for FinalSnapshotWire {
    fn query(
        &self,
        _: &str,
        _: serde_json::Value,
    ) -> Result<crate::github::provider::Response, crate::github::ConnectionError> {
        (self.during_read)();
        let body = json!({"data":{"repository":{"databaseId":100,"p1":{
            "fullDatabaseId":"9","number":1,"title":"Review","body":"",
            "state":"OPEN","isDraft":false,"mergeable":"UNKNOWN","reviewDecision":null,
            "baseRefOid":"b".repeat(40),"headRefOid":"a".repeat(40),
            "baseRef":{"branchProtectionRule":null},"labels":{"nodes":[]},
            "latestOpinionatedReviews":{"nodes":[]},"comments":{"totalCount":0,"nodes":[]},
            "commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{
                "totalCount":1,"nodes":[{"__typename":"CheckRun","name":"ci","status":"IN_PROGRESS","conclusion":null}]
            }}}}]},"reviewThreads":{"totalCount":0,"nodes":[]}
        }}}});
        Ok(crate::github::provider::Response {
            status: 200,
            headers: Default::default(),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn feedback_change_after_preparation_is_tossed_at_ready_and_wait_finalization() {
    use crate::github::provider::GithubClient;
    use std::sync::atomic::AtomicUsize;

    for wait in [false, true] {
        let (root, store, template) = feedback_fixture(false);
        let run = running_packet(&store, &template, 100);
        let original_feedback = store.load_feedback().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let wire_count = count.clone();
        let path = root.path().to_owned();
        let client = GithubClient::new(FinalSnapshotWire {
            during_read: Box::new(move || {
                if wire_count.fetch_add(1, Ordering::SeqCst) == 1 {
                    change_feedback(&Store::new(path.clone()));
                }
            }),
        });
        let gate_path = root.path().to_owned();
        let gate_run = run.clone();
        let gate = Arc::new(move || {
            crate::review::validate_execution_selection(&Store::new(gate_path.clone()), &gate_run)
        });
        let repo = RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        };
        let attempt = AttemptGuard::new(gate);
        let (client, snapshot) = attempt
            .prepare(client, |client| {
                let snapshot = client.pr_snapshot(&repo, 1).map_err(Failure::from)?;
                Ok((client, snapshot.normalize()))
            })
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut packet = run.packet.clone().unwrap();
        packet.state_hash = Some(snapshot.hash());
        packet.wait_snapshot = Some(snapshot);
        let mut result = packet_result(if wait {
            json!({"exit":"wait","reason":"ci"})
        } else {
            json!({"exit":"ready_for_human","summary":"Reviewed."})
        });

        let failure = attempt
            .finish(finalize_local_exit(&client, &repo, 1, &packet, &mut result))
            .unwrap_err();
        // Even if observation rolls back, the first refusal invalidated this
        // attempt: neither its client nor a late successful result can recover it.
        store.save_feedback(&original_feedback).unwrap();
        crate::review::validate_execution_selection(&store, &run).unwrap();
        assert_eq!(attempt.gate()().unwrap_err().kind, OperationFailure::Tossed);
        assert!(client.action_observation(&repo, 1).is_err());
        assert_eq!(
            attempt.finish(Ok(result)).unwrap_err().kind,
            OperationFailure::Tossed
        );
        complete(&store, &run.operation.id, Err(failure), 101).unwrap();
        let saved = store.load_reviews().unwrap().remove(0);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(saved.operation.failure, Some(OperationFailure::Tossed));
        assert_eq!(saved.operation.state, OperationState::Queued);
        assert_eq!(saved.operation.attempt_count, 1);
        assert!(saved.result.is_none());
        assert!(store.load_feedback().unwrap().exit_writes.is_empty());
    }
}

struct UserWire(Box<dyn Fn() + Send + Sync>);

impl crate::github::provider::Transport for UserWire {
    fn get(
        &self,
        _: &str,
    ) -> Result<crate::github::provider::Response, crate::github::ConnectionError> {
        (self.0)();
        Ok(crate::github::provider::Response {
            status: 200,
            headers: Default::default(),
            body: br#"{"id":22,"login":"owner"}"#.to_vec(),
        })
    }
}

impl crate::github::threads::QueryTransport for UserWire {
    fn query(
        &self,
        _: &str,
        _: serde_json::Value,
    ) -> Result<crate::github::provider::Response, crate::github::ConnectionError> {
        crate::github::provider::Transport::get(self, "graphql")
    }
}

const READS: usize = 500;

type Change = Box<dyn Fn(&Store, &AtomicBool) + Send + Sync>;

/// A guarded preparation of `READS` provider reads under the native gate shape
/// (cancellation, then cached validation of saved state on the shared Store).
/// `change` runs once before read `at`; returns completed reads and outcome.
fn guarded_reads(
    store: &Arc<std::sync::Mutex<Store>>,
    run: &ReviewRun,
    at: usize,
    change: impl Fn(&Store, &AtomicBool) + Send + Sync + 'static,
) -> (usize, Result<(), Failure>) {
    use std::sync::atomic::AtomicUsize;
    let cancelled = Arc::new(AtomicBool::new(false));
    let validated = Arc::new(crate::review::ValidatedSelection::default());
    let (gate_store, gate_run, gate_cancelled) = (store.clone(), run.clone(), cancelled.clone());
    let gate: runtime::Gate = Arc::new(move || {
        if gate_cancelled.load(Ordering::SeqCst) {
            return Err(Failure::cancelled());
        }
        let store = gate_store.lock().unwrap();
        crate::review::validate_execution_selection_cached(&store, &gate_run, &validated)
    });
    let reads = Arc::new(AtomicUsize::new(0));
    let (wire_reads, wire_store) = (reads.clone(), store.clone());
    let client = crate::github::provider::GithubClient::new(UserWire(Box::new(move || {
        if wire_reads.fetch_add(1, Ordering::SeqCst) + 1 == at {
            change(&wire_store.lock().unwrap(), &cancelled);
        }
    })));
    let outcome = AttemptGuard::new(gate).prepare(client, |client| {
        for _ in 0..READS {
            client.current_identity().map_err(Failure::from)?;
        }
        Ok(())
    });
    (reads.load(Ordering::SeqCst), outcome)
}

/// Live-size Queue: the running PR plus 1,400 retired passes (~2 MB).
fn live_size_queue(store: &Store) {
    let mut jobs = store.load_queue().unwrap();
    let template = jobs[0].clone();
    for n in 0..1_400 {
        let mut job = template.clone();
        job.pull_request_id = format!("retired-{n}");
        job.number = 10_000 + n;
        job.title = "x".repeat(1_200);
        job.waiting = monitoring::WAITING_MERGED.into();
        jobs.push(job);
    }
    store.save_queue(&jobs).unwrap();
}

#[test]
fn guarded_reads_revalidate_saved_state_only_after_an_input_changes() {
    let (_root, store, template) = feedback_fixture(false);
    live_size_queue(&store);
    let run = running_packet(&store, &template, 100);
    let store = Arc::new(std::sync::Mutex::new(store));

    // Every file the full validation reads is a cache input.
    store.lock().unwrap().take_state_reads();
    crate::review::validate_execution_selection(&store.lock().unwrap(), &run).unwrap();
    let full = store.lock().unwrap().take_state_reads();
    for name in full.keys() {
        assert!(
            crate::review::EXECUTION_INPUTS.contains(&name.as_str()),
            "{name} is read but does not invalidate the cache"
        );
    }

    crate::storage::file_identity::settle();
    let started = std::time::Instant::now();
    let (reads, outcome) = guarded_reads(&store, &run, 0, |_, _| {});
    let cached = started.elapsed();
    outcome.unwrap();
    assert_eq!(reads, READS);
    // One validation for the whole preparation, not one per guarded read.
    let loads = store.lock().unwrap().take_state_reads();
    for (name, count) in &full {
        assert_eq!(loads.get(name), Some(count), "{name}: {loads:?}");
    }
    // The former gate validated on both sides of every read.
    let started = std::time::Instant::now();
    for _ in 0..10 {
        crate::review::validate_execution_selection(&store.lock().unwrap(), &run).unwrap();
    }
    eprintln!(
        "{READS} guarded reads: cached gate {cached:?}; former gate ~{:?}",
        started.elapsed() / 10 * (2 * READS as u32)
    );

    let refuse = |change: Change, expected: &str| {
        let (reads, outcome) = guarded_reads(&store, &run, READS / 2, change);
        assert_eq!(reads, READS / 2, "{expected}: refused at the next gate");
        let failure = outcome.unwrap_err();
        assert!(failure.message.contains(expected), "{}", failure.message);
    };
    let restore =
        |store: &Arc<std::sync::Mutex<Store>>,
         snapshot: &(Vec<QueueJob>, Settings, crate::feedback::Ledger)| {
            let store = store.lock().unwrap();
            store.save_queue(&snapshot.0).unwrap();
            store.save_settings(&snapshot.1).unwrap();
            store.save_feedback(&snapshot.2).unwrap();
            drop(store);
            crate::storage::file_identity::settle();
        };
    let saved = {
        let store = store.lock().unwrap();
        (
            store.load_queue().unwrap(),
            store.load_settings().unwrap(),
            store.load_feedback().unwrap(),
        )
    };
    refuse(
        Box::new(|store, _| {
            let mut jobs = store.load_queue().unwrap();
            jobs[0].waiting = monitoring::WAITING_MERGED.into();
            store.save_queue(&jobs).unwrap();
        }),
        "",
    );
    restore(&store, &saved);
    refuse(
        Box::new(|store, _| {
            let mut settings = store.load_settings().unwrap();
            settings.agents[0].prompt = "Review security.".into();
            store.save_settings(&settings).unwrap();
        }),
        "Agent configuration changed",
    );
    restore(&store, &saved);
    refuse(Box::new(|store, _| change_feedback(store)), CHANGED);
    restore(&store, &saved);
    // A change written outside this Store is still seen at the next read.
    let elsewhere = _root.path().to_path_buf();
    refuse(
        Box::new(move |_, _| {
            let other = Store::new(elsewhere.clone());
            let mut jobs = other.load_queue().unwrap();
            jobs[0].waiting = monitoring::WAITING_MERGED.into();
            other.save_queue(&jobs).unwrap();
        }),
        "",
    );
    restore(&store, &saved);
    // Equal-length foreign rewrites that restore the modification time, by
    // atomic replace and in place, still refuse at the next read.
    for atomic in [true, false] {
        let path = _root.path().join("state/queue.json");
        let sha = run.job.head_sha.clone();
        refuse(
            Box::new(move |_, _| {
                crate::storage::file_identity::rewrite_foreign(&path, atomic, |bytes| {
                    let text = String::from_utf8(bytes.clone()).unwrap();
                    let last = if sha.ends_with('0') { "1" } else { "0" };
                    let other = format!("{}{last}", &sha[..sha.len() - 1]);
                    *bytes = text.replace(&sha, &other).into_bytes();
                })
            }),
            "no longer available",
        );
        restore(&store, &saved);
    }
    // Pause and human cancel signal the attempt; no saved input is consulted.
    let (reads, outcome) = guarded_reads(&store, &run, READS / 2, |_, cancelled| {
        cancelled.store(true, Ordering::SeqCst)
    });
    assert_eq!(reads, READS / 2);
    assert!(outcome.unwrap_err().cancelled);
}

fn set_observation(store: &Store, job: &QueueJob, failure: Option<&str>) {
    let mut monitoring = store.load_monitoring_state().unwrap();
    monitoring.health.insert(
        "repository-health".into(),
        serde_json::from_value(json!({
            "repository_id": job.configuration_id, "name": "example/repo",
            "schedule_key": "schedule", "enabled": true, "last_attempt": 100,
            "last_success": null, "next_run": 0, "schedule_available": true,
            "last_failure": failure, "in_flight": false
        }))
        .unwrap(),
    );
    store.save_monitoring_state(&monitoring).unwrap();
}

fn waiting_reason(store: &Store, key: &str, now: i64) -> Option<String> {
    crate::capacity::candidates(store, now)
        .unwrap()
        .into_iter()
        .find(|work| work.key.id == key)
        .and_then(|work| work.reason)
}

#[test]
fn failed_feedback_observation_is_a_bounded_toss_and_holds_dispatch_until_a_clean_poll() {
    let (_root, store, template) = feedback_fixture(false);
    let run = running_packet(&store, &template, 100);
    set_observation(&store, &run.job, Some("network"));
    let failure = crate::review::validate_execution_selection(&store, &run).unwrap_err();
    assert_eq!(failure.kind, OperationFailure::Tossed);
    assert_eq!(failure.message, crate::feedback::OBSERVATION_FAILED);
    complete(&store, &run.operation.id, Err(failure), 101).unwrap();
    let saved = store.load_reviews().unwrap().remove(0);
    assert_eq!(saved.operation.state, OperationState::Queued);
    assert_eq!(saved.operation.attempt_count, 1);
    assert!(saved.result.is_none());

    // While observation is unhealthy no new AI attempt starts or spends budget.
    let next = saved.operation.next_attempt_at.unwrap();
    assert_eq!(
        waiting_reason(&store, &run.key, next).as_deref(),
        Some(crate::feedback::WAITING_FOR_OBSERVATION)
    );
    assert_eq!(store.load_reviews().unwrap()[0].operation.attempt_count, 1);

    set_observation(&store, &run.job, None);
    assert_eq!(waiting_reason(&store, &run.key, next), None);
    let retried = running_packet(&store, &template, next);
    assert_eq!(retried.operation.id, run.operation.id);
    assert_eq!(retried.operation.attempt_count, 2);
    crate::review::validate_execution_selection(&store, &retried).unwrap();
    complete(
        &store,
        &retried.operation.id,
        Ok(packet_result(
            json!({"exit":"ready_for_human","summary":"Fresh."}),
        )),
        next + 60,
    )
    .unwrap();
    assert_eq!(
        store.load_reviews().unwrap()[0].operation.state,
        OperationState::Completed
    );
}

#[test]
fn old_permanent_observation_failures_recover_through_the_toss_budget() {
    let (root, store, template) = feedback_fixture(false);
    let run = running_packet(&store, &template, 100);
    complete(
        &store,
        &run.operation.id,
        Err(Failure::permanent(crate::feedback::OBSERVATION_FAILED)),
        101,
    )
    .unwrap();
    assert_eq!(
        store.load_reviews().unwrap()[0].operation.failure,
        Some(OperationFailure::Permanent)
    );
    let restarted = Store::new(root.path().into());
    crate::review::restore(&restarted).unwrap();
    let recovered = restarted.load_reviews().unwrap().remove(0);
    assert_eq!(recovered.operation.id, run.operation.id);
    assert_eq!(recovered.operation.attempt_count, 1);
    assert_eq!(recovered.operation.failure, Some(OperationFailure::Tossed));
    assert_eq!(
        recovered.operation.retry_deadline,
        run.operation.retry_deadline
    );
}
