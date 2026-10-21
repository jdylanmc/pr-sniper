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
            crate::review::restore(&restarted).unwrap();
            let recovered = restarted.load_reviews().unwrap().remove(0);
            assert_eq!(
                recovered.operation.failure,
                Some(OperationFailure::Tossed),
                "{blocked}"
            );
            assert_eq!(recovered.operation.state, OperationState::Queued);
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
            let outcome = with_preparation_client(client, gate, |client| {
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
            let gate = Arc::new(move || {
                if read_finished.load(Ordering::SeqCst) {
                    Err(refusal.clone())
                } else {
                    Ok(())
                }
            });
            let repo = RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            };
            let actual = with_preparation_client(client, gate, |client| {
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
        let actual = with_preparation_client(client, gate, |client| {
            client
                .pr_snapshot(&repo, 1)
                .map(|_| ())
                .map_err(Failure::from)
        })
        .unwrap_err();
        assert_same_failure(&actual, &Failure::from(provider));
    }
}
