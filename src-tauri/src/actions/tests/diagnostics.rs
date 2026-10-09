use super::*;
use crate::storage::diagnostics::{Event, FailureKind, Record, RetryDecision};

fn start_evidence(store: &Store, run: &crate::review::ReviewRun, name: &str) {
    let mut attempt =
        crate::review::diagnostics::attempt(&run.job, &run.operation, "diagnostic-test");
    attempt.id = format!("{name}-attempt");
    attempt.session_id = Some(format!("{name}-session"));
    store
        .record_attempt(Record {
            attempt,
            sequence: Some(1),
            elapsed_ms: Some(0),
            event: Event::Started,
        })
        .unwrap();
}

#[test]
fn diagnostics_stale_final_records_only_same_operation_decisions() {
    let (root, store, item) = fixture(1, true, false);
    let mut settings = store.load_settings().unwrap();
    settings.capacity = 1;
    store.save_settings(&settings).unwrap();
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let capacity = Capacity::default();
    let Dispatch::Review(old, token) = capacity
        .dispatch(&store, NOW + 11)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Final worker expected");
    };
    start_evidence(&store, &old, "old");
    host::cancel_final_in_store(&store, &capacity, &old.key, NOW + 12).unwrap();
    assert!(token.load(std::sync::atomic::Ordering::SeqCst));
    request_final(&store, &old.key, NOW + 13).unwrap();
    let replacement = store.load_actions().unwrap().finals.remove(0);
    assert_ne!(replacement.execution.operation.id, old.operation.id);
    assert_eq!(
        replacement.execution.operation.state,
        OperationState::Queued
    );
    assert!(capacity
        .dispatch(&store, NOW + 14)
        .unwrap()
        .dispatched
        .is_empty());
    host::finish_final_worker(&store, &capacity, &old, Err(Failure::cancelled()), NOW + 15)
        .unwrap();
    host::record_final_worker_decision(&store, &old).unwrap();
    let fresh = Store::new(root.path().into());
    assert_eq!(fresh.load_actions().unwrap().finals[0], replacement);
    assert!(capacity.finished());
    let records: Vec<_> = fresh
        .diagnostics()
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.attempt)
        .filter(|record| matches!(record.event, Event::Retry { .. }))
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attempt.operation_id, old.operation.id);
    assert_eq!(records[0].attempt.id, "old-attempt");
    assert_eq!(
        records[0].attempt.session_id.as_deref(),
        Some("old-session")
    );
    assert!(
        matches!(
            records[0].event,
            Event::Retry {
                decision: RetryDecision::Terminal,
                failure: Some(FailureKind::EligibilityOrConfiguration),
                next_attempt_at: None,
                attempt_count: 1
            }
        ),
        "Old cancelled attempt must not borrow queued replacement fields"
    );
    let Dispatch::Review(new, _) = capacity
        .dispatch(&store, NOW + 16)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Replacement final worker expected");
    };
    assert_eq!(new.operation.id, replacement.execution.operation.id);
    start_evidence(&store, &new, "new");
    host::finish_final_worker(&store, &capacity, &new, Ok(output()), NOW + 18).unwrap();
    host::record_final_worker_decision(&store, &new).unwrap();
    let records: Vec<_> = Store::new(root.path().into())
        .diagnostics()
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.attempt)
        .filter(|record| matches!(record.event, Event::Retry { .. }))
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].attempt.operation_id, new.operation.id);
    assert_eq!(records[1].attempt.id, "new-attempt");
    assert_eq!(
        records[1].attempt.session_id.as_deref(),
        Some("new-session")
    );
    assert!(matches!(
        records[1].event,
        Event::Retry {
            decision: RetryDecision::Completed,
            failure: None,
            next_attempt_at: None,
            attempt_count: 1
        }
    ));
    assert!(capacity.finished());
    assert!(capacity
        .dispatch(&store, NOW + 19)
        .unwrap()
        .dispatched
        .is_empty());
}

#[test]
fn diagnostics_missing_final_attempt_is_explicit_without_holding_released_capacity() {
    let (root, store, item) = fixture(1, true, false);
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let capacity = Capacity::default();
    let Dispatch::Review(run, _) = capacity
        .dispatch(&store, NOW + 11)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Final worker expected");
    };
    host::finish_final_worker(&store, &capacity, &run, Ok(output()), NOW + 12).unwrap();
    assert_eq!(
        host::record_final_worker_decision(&store, &run).unwrap_err(),
        "Attempt diagnostics missing; decision could not be correlated."
    );
    let fresh = Store::new(root.path().into());
    assert!(fresh.diagnostics().unwrap().is_empty());
    assert_eq!(
        fresh.load_actions().unwrap().finals[0]
            .execution
            .operation
            .state,
        OperationState::Completed
    );
    assert!(capacity.finished());
}

#[test]
fn diagnostics_stale_final_missing_history_never_uses_the_replacement_operation() {
    let (root, store, item) = fixture(1, true, false);
    synchronize(&store, &item, Ok(observed()), NOW + 10).unwrap();
    let capacity = Capacity::default();
    let Dispatch::Review(old, _) = capacity
        .dispatch(&store, NOW + 11)
        .unwrap()
        .dispatched
        .remove(0)
    else {
        panic!("Final worker expected");
    };
    start_evidence(&store, &old, "old");
    host::cancel_final_in_store(&store, &capacity, &old.key, NOW + 12).unwrap();
    request_final(&store, &old.key, NOW + 13).unwrap();
    let mut ledger = store.load_actions().unwrap();
    ledger.finals[0].attempts.clear();
    store.save_actions(&ledger).unwrap();
    let replacement = store.load_actions().unwrap();
    host::finish_final_worker(&store, &capacity, &old, Err(Failure::cancelled()), NOW + 15)
        .unwrap();
    assert_eq!(
        host::record_final_worker_decision(&store, &old).unwrap_err(),
        "Final review diagnostic operation is no longer retained."
    );
    let fresh = Store::new(root.path().into());
    assert_eq!(fresh.load_actions().unwrap(), replacement);
    assert!(fresh
        .diagnostics()
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.attempt)
        .all(|record| !matches!(record.event, Event::Retry { .. })));
    assert!(capacity.finished());
}
