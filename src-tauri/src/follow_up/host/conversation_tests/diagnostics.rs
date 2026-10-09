use super::*;
use crate::storage::diagnostics::{Event, FailureKind, RetryDecision};

fn analysis_trace(
    root: &std::path::Path,
    run: &FollowUp,
) -> Arc<crate::review::diagnostics::Trace> {
    let sink = Store::new(root.into());
    let operation = run.analysis.as_ref().unwrap();
    let mut attempt =
        crate::review::diagnostics::attempt(&run.context.job, operation, "diagnostic-test");
    attempt.id = "conversation-attempt".into();
    attempt.session_id = Some("conversation-session".into());
    crate::review::diagnostics::Trace::new(
        attempt,
        Arc::new(move |record| sink.record_attempt(record)),
    )
}

fn running_analysis() -> (tempfile::TempDir, Store, FollowUp, Capacity) {
    let (root, store, origin, mut thread) = fixture(1);
    explanation(&mut thread, "11");
    observe(&store, &origin, &thread, 'a', vec![], NOW + 10);
    let capacity = Capacity::default();
    let Dispatch::Reply(run, _) = capacity
        .dispatch(&store, NOW + 20)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Conversation worker expected");
    };
    (root, store, *run, capacity)
}

fn retry_records(root: &std::path::Path) -> Vec<crate::storage::diagnostics::Record> {
    Store::new(root.into())
        .diagnostics()
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.attempt)
        .filter(|record| matches!(record.event, Event::Retry { .. }))
        .collect()
}

#[test]
fn diagnostics_failed_analysis_preserves_committed_backoff_and_original_error() {
    let (root, store, mut run, _) = running_analysis();
    let trace = analysis_trace(root.path(), &run);
    let failure = Failure {
        cancelled: false,
        kind: crate::monitoring::OperationFailure::Network,
        message: "Synthetic network analysis failure".into(),
        retry_after_seconds: Some(60),
    };
    let saved = analysis_completion(&store, &mut run, Err(failure), false, NOW + 21);
    let returned = record_analysis_completion(saved, &trace).unwrap_err();
    assert_eq!(returned.message, "Synthetic network analysis failure");
    let fresh = Store::new(root.path().into());
    let persisted = fresh.load_follow_ups().unwrap().remove(0);
    assert_eq!(persisted.error.as_deref(), Some(returned.message.as_str()));
    let operation = persisted.analysis.unwrap();
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.next_attempt_at, Some(NOW + 81));
    let events = retry_records(root.path());
    assert_eq!(
        events.len(),
        1,
        "Durably committed failure needs a decision"
    );
    assert_eq!(events[0].attempt.operation_id, operation.id);
    assert_eq!(events[0].attempt.id, "conversation-attempt");
    assert_eq!(
        events[0].attempt.session_id.as_deref(),
        Some("conversation-session")
    );
    assert!(matches!(
        events[0].event,
        Event::Retry {
            decision: RetryDecision::Scheduled,
            failure: Some(FailureKind::Network),
            next_attempt_at: Some(at),
            attempt_count: 1,
        } if at == NOW + 81
    ));
}

#[test]
fn diagnostics_failed_analysis_records_permanent_and_exhausted_manual_decisions() {
    for exhausted in [false, true] {
        let (root, store, mut run, _) = running_analysis();
        if exhausted {
            let mut runs = store.load_follow_ups().unwrap();
            runs[0].analysis.as_mut().unwrap().retry_deadline = NOW + 21;
            store.save_follow_ups(&runs).unwrap();
        }
        let trace = analysis_trace(root.path(), &run);
        let failure = if exhausted {
            Failure::timeout()
        } else {
            Failure::permanent("Synthetic permanent analysis failure")
        };
        let original = failure.message.clone();
        let completion = analysis_completion(&store, &mut run, Err(failure), false, NOW + 21);
        assert_eq!(
            record_analysis_completion(completion, &trace)
                .unwrap_err()
                .message,
            original
        );
        let saved = Store::new(root.path().into())
            .load_follow_ups()
            .unwrap()
            .remove(0);
        let operation = saved.analysis.unwrap();
        assert_eq!(
            operation.state,
            if exhausted {
                OperationState::ManualRetry
            } else {
                OperationState::Failed
            }
        );
        assert_eq!(operation.next_attempt_at, None);
        assert_eq!(saved.error.as_deref(), Some(original.as_str()));
        let events = retry_records(root.path());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].attempt.operation_id, operation.id);
        assert!(matches!(
            events[0].event,
            Event::Retry { decision, failure: Some(kind), next_attempt_at: None, attempt_count: 1 }
                if matches!((exhausted, decision), (true, RetryDecision::Manual) | (false, RetryDecision::Terminal))
                    && kind == if exhausted { FailureKind::Timeout } else { FailureKind::EligibilityOrConfiguration }
        ));
    }
}

#[test]
fn diagnostics_intentional_pause_records_restored_retry_budget_not_a_failure() {
    let (root, store, mut run, capacity) = running_analysis();
    let trace = analysis_trace(root.path(), &run);
    store
        .save_automation(&crate::capacity::Automation { paused: true })
        .unwrap();
    assert!(capacity
        .dispatch(&store, NOW + 21)
        .unwrap()
        .dispatched
        .is_empty());
    let completion =
        analysis_completion(&store, &mut run, Err(Failure::cancelled()), false, NOW + 24);
    record_analysis_completion(completion, &trace).unwrap();
    let saved = Store::new(root.path().into())
        .load_follow_ups()
        .unwrap()
        .remove(0);
    let operation = saved.analysis.unwrap();
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(saved.error, None);
    let records = retry_records(root.path());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attempt.operation_id, operation.id);
    assert!(matches!(
        records[0].event,
        Event::Retry { decision: RetryDecision::Scheduled, failure: None, next_attempt_at: Some(at), attempt_count: 0 }
            if at == NOW + 24
    ));
}

#[test]
fn diagnostics_failed_analysis_save_never_claims_a_committed_retry() {
    let (root, store, mut run, _) = running_analysis();
    let trace = analysis_trace(root.path(), &run);
    let original = store.load_follow_ups().unwrap();
    store.fail_state_write("follow-ups.json", 1);
    let completion =
        analysis_completion(&store, &mut run, Err(Failure::timeout()), false, NOW + 21);
    let error = record_analysis_completion(completion, &trace).unwrap_err();
    assert_ne!(error.message, Failure::timeout().message);
    assert_eq!(
        Store::new(root.path().into()).load_follow_ups().unwrap(),
        original
    );
    assert!(retry_records(root.path()).is_empty());
}

#[test]
fn diagnostics_lost_analysis_ownership_never_claims_the_replacement_decision() {
    let (root, store, mut run, capacity) = running_analysis();
    let trace = analysis_trace(root.path(), &run);
    cancel_in_store(&store, &capacity, None, &run.id, NOW + 21).unwrap();
    let replacement = request_analysis(&store, &run.id, true, NOW + 22).unwrap();
    assert_ne!(
        replacement.analysis.as_ref().unwrap().id,
        run.analysis.as_ref().unwrap().id
    );
    let original = store.load_follow_ups().unwrap();
    let completion =
        analysis_completion(&store, &mut run, Err(Failure::timeout()), false, NOW + 24);
    let error = record_analysis_completion(completion, &trace).unwrap_err();
    assert_eq!(
        error.message,
        "A newer analysis attempt owns this follow-up."
    );
    assert_eq!(
        Store::new(root.path().into()).load_follow_ups().unwrap(),
        original
    );
    assert!(retry_records(root.path()).is_empty());
}

#[test]
fn diagnostics_final_analysis_gate_failure_preserves_committed_failure() {
    let (root, store, mut run, _) = running_analysis();
    let trace = analysis_trace(root.path(), &run);
    let result = output_for(&run, ReplyDecision::Quiet, vec![]);
    let completion = analysis_completion(&store, &mut run, Ok(result), false, NOW + 21);
    let error = record_analysis_completion(completion, &trace).unwrap_err();
    let saved = Store::new(root.path().into())
        .load_follow_ups()
        .unwrap()
        .remove(0);
    assert_eq!(saved.error.as_deref(), Some(error.message.as_str()));
    assert!(saved.result.is_none());
    let operation = saved.analysis.unwrap();
    assert_eq!(operation.state, OperationState::Failed);
    let records = retry_records(root.path());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attempt.operation_id, operation.id);
    assert!(matches!(
        records[0].event,
        Event::Retry {
            decision: RetryDecision::Terminal,
            failure: Some(FailureKind::EligibilityOrConfiguration),
            next_attempt_at: None,
            attempt_count: 1
        }
    ));
}

#[test]
fn diagnostics_explicit_analysis_cancellation_records_terminal_not_pause_retry() {
    let (root, store, mut run, capacity) = running_analysis();
    let trace = analysis_trace(root.path(), &run);
    cancel_in_store(&store, &capacity, None, &run.id, NOW + 21).unwrap();
    let completion =
        analysis_completion(&store, &mut run, Err(Failure::cancelled()), false, NOW + 24);
    let error = record_analysis_completion(completion, &trace).unwrap_err();
    assert!(error.cancelled);
    let saved = Store::new(root.path().into())
        .load_follow_ups()
        .unwrap()
        .remove(0);
    assert!(saved.cancelled);
    assert_eq!(saved.error.as_deref(), Some(error.message.as_str()));
    let operation = saved.analysis.unwrap();
    assert_eq!(operation.state, OperationState::Failed);
    let records = retry_records(root.path());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attempt.operation_id, operation.id);
    assert!(matches!(
        records[0].event,
        Event::Retry {
            decision: RetryDecision::Terminal,
            failure: Some(FailureKind::EligibilityOrConfiguration),
            next_attempt_at: None,
            attempt_count: 1
        }
    ));
}
