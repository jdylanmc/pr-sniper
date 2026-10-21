use super::*;
use crate::{
    capacity::{tests as fixtures, Automation, Coordinator, Dispatch},
    monitoring::{OperationFailure, OperationState},
    review::host::AttemptGuard,
};
use std::sync::{Condvar, Mutex};

struct HeldRead {
    entered: tokio::sync::Notify,
    released: Mutex<bool>,
    wake: Condvar,
    returned_ok: AtomicBool,
}

impl HeldRead {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}

struct Wire(Arc<HeldRead>);

impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.0.entered.notify_one();
        let released = self.0.released.lock().unwrap();
        let (released, _) = self
            .0
            .wake
            .wait_timeout_while(released, Duration::from_secs(20), |released| !*released)
            .unwrap();
        assert!(*released, "the test must release its held provider read");
        let response = Synthetic.get(path)?;
        self.0.returned_ok.store(true, Ordering::SeqCst);
        Ok(response)
    }
}

impl crate::github::threads::QueryTransport for Wire {
    fn query(&self, _: &str, _: Value) -> Result<Response, ConnectionError> {
        unreachable!()
    }
}

async fn ordering_case(inactivity: bool, earlier_feedback: bool) {
    let (_state, store) = fixtures::fixture(1, 1, true);
    let mut settings = store.load_settings().unwrap();
    settings.agents[0].model = "review-model".into();
    store.save_settings(&settings).unwrap();
    let coordinator = Coordinator::default();
    let dispatch = coordinator
        .dispatch(&store, 200)
        .unwrap()
        .dispatched
        .remove(0);
    let key = dispatch.key();
    let Dispatch::Review(run, token) = dispatch;
    let budget = run.operation.clone();
    let publications = store.load_publications().unwrap();
    let effects = store.load_feedback().unwrap().exit_writes;
    let feedback = Arc::new(AtomicBool::new(false));
    let gate_feedback = feedback.clone();
    let gate_token = token.clone();
    let attempt = AttemptGuard::new(Arc::new(move || {
        if gate_feedback.load(Ordering::SeqCst) {
            Err(Failure::tossed("Owned feedback changed."))
        } else if gate_token.load(Ordering::SeqCst) {
            Err(Failure::cancelled())
        } else {
            Ok(())
        }
    }));
    let held = Arc::new(HeldRead {
        entered: tokio::sync::Notify::new(),
        released: Mutex::new(false),
        wake: Condvar::new(),
        returned_ok: AtomicBool::new(false),
    });
    let client = attempt
        .prepare(GithubClient::new(Wire(held.clone())), |client| {
            Ok(Arc::new(client))
        })
        .unwrap();
    let reads = ReadTools::new(
        None,
        context(true),
        client.clone(),
        "example/repo".into(),
        attempt.gate(),
    );
    let read = tokio::task::spawn_blocking(move || {
        reads.execute("read_source", json!({"side":"head","path":"source.rs"}))
    });
    held.entered.notified().await;
    let root = tempfile::tempdir().unwrap();
    let scenario = if inactivity {
        "guarded-inactivity-held-abort"
    } else {
        "guarded-provider-held-abort"
    };
    let mut options = options(root.path(), scenario);
    options.env.push((
        "TEST_TERMINAL_RELEASE".into(),
        root.path().join("release-terminal").into_os_string(),
    ));
    let mut operation = operation(15);
    operation.cancelled = token.clone();
    let base = request(false);
    let runtime_returned = AtomicBool::new(false);
    let inference = async {
        let outcome = execute(
            options,
            &Identity {
                id: "33".into(),
                login: "review-account".into(),
            },
            &operation,
            Request {
                inactivity: if inactivity {
                    Duration::from_secs(2)
                } else {
                    base.inactivity
                },
                diagnostics: base.diagnostics,
                context: base.context,
                client,
                repository_name: base.repository_name,
                selection: run.selection.clone(),
                before_send: base.before_send,
                local_gate: attempt.gate(),
                failure_commit: Some(attempt.committer()),
                task: base.task,
                exits: base.exits,
                duck: base.duck,
            },
        )
        .await;
        runtime_returned.store(true, Ordering::SeqCst);
        let error = attempt.finish(outcome).unwrap_err();
        crate::review::host::complete(&store, &budget.id, Err(error.clone()), 203).unwrap();
        error
    };
    let interrupt = async {
        crate::copilot::runtime::wait_for_fixture_method(root.path(), "session.send").await;
        if earlier_feedback {
            feedback.store(true, Ordering::SeqCst);
            held.release();
            assert!(read.await.unwrap().is_err());
            feedback.store(false, Ordering::SeqCst);
            std::fs::write(root.path().join("release-terminal"), b"release").unwrap();
            crate::copilot::runtime::wait_for_fixture_method(root.path(), "fixture.primaryFailure")
                .await;
            crate::copilot::runtime::wait_for_fixture_method(root.path(), "session.abort").await;
            store.save_automation(&Automation { paused: true }).unwrap();
            assert!(coordinator
                .dispatch(&store, 201)
                .unwrap()
                .dispatched
                .is_empty());
            assert!(token.load(Ordering::SeqCst));
        } else {
            if !inactivity {
                std::fs::write(root.path().join("release-terminal"), b"release").unwrap();
            }
            // Abort begins strictly after the primary runtime chose its error.
            crate::copilot::runtime::wait_for_fixture_method(root.path(), "session.abort").await;
            store.save_automation(&Automation { paused: true }).unwrap();
            assert!(coordinator
                .dispatch(&store, 201)
                .unwrap()
                .dispatched
                .is_empty());
            assert!(token.load(Ordering::SeqCst));
            assert!(
                !runtime_returned.load(Ordering::SeqCst),
                "runtime cleanup must still be held"
            );
            held.release();
            assert!(
                read.await.unwrap().is_err(),
                "late work must still be rejected"
            );
            assert!(
                !runtime_returned.load(Ordering::SeqCst),
                "the guard must refuse before runtime cleanup returns"
            );
        }
        assert!(held.returned_ok.load(Ordering::SeqCst));
        std::fs::write(root.path().join("release-abort"), b"release").unwrap();
    };
    let (error, ()) = tokio::join!(inference, interrupt);
    stopped(root.path());
    let expected = if earlier_feedback {
        OperationFailure::Tossed
    } else if inactivity {
        OperationFailure::Timeout
    } else {
        OperationFailure::Provider
    };
    assert!(
        !error.cancelled,
        "inactivity={inactivity} earlier_feedback={earlier_feedback}: {error:?}"
    );
    assert_eq!(error.kind, expected);
    assert_eq!(
        error.origin,
        if earlier_feedback {
            crate::backpressure::Origin::Github
        } else {
            crate::backpressure::Origin::Copilot
        }
    );
    assert_eq!(error.resume, None);
    assert_eq!(
        error.retry_after_seconds,
        if !inactivity && !earlier_feedback {
            Some(7)
        } else {
            None
        }
    );
    let saved = store
        .load_reviews()
        .unwrap()
        .into_iter()
        .find(|r| r.operation.id == budget.id)
        .unwrap();
    assert_eq!(saved.operation.failure, Some(expected));
    assert_eq!(saved.operation.state, OperationState::Queued);
    assert_eq!(saved.operation.attempt_count, budget.attempt_count);
    assert_eq!(
        saved.operation.initial_attempt_at,
        budget.initial_attempt_at
    );
    assert_eq!(saved.operation.retry_deadline, budget.retry_deadline);
    assert!(saved.operation.next_attempt_at.unwrap() > 203);
    if !inactivity && !earlier_feedback {
        assert_eq!(saved.operation.next_attempt_at, Some(210));
    }
    assert!(saved.result.is_none());
    assert_eq!(store.load_publications().unwrap(), publications);
    assert_eq!(store.load_feedback().unwrap().exit_writes, effects);
    coordinator.release(&key, &budget.id).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_failure_ordering_keeps_provider_failure_during_paused_teardown() {
    ordering_case(false, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_failure_ordering_keeps_inactivity_during_paused_teardown() {
    ordering_case(true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_failure_ordering_keeps_earlier_feedback_refusal() {
    ordering_case(false, true).await;
}
