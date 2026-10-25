use super::*;
use crate::review::findings::{Assessment, Disposition};
use serde_json::json;

#[test]
fn existing_identity_reaches_grant_gate_as_reply_not_root_and_keeps_assessment() {
    let observed = Arc::new(Mutex::new(None));
    let sink = observed.clone();
    let mut tools = ExitTools::new(
        vec![Exit::LeaveComments],
        Arc::new(move |call| {
            *sink.lock().unwrap() = Some(call.clone());
            Err(Refusal::Denied("Reply grant absent.".into()))
        }),
        None,
    );
    tools.require_finding_identity = true;
    let a = Assessment {
        author_comment_id: None,
        anchor: None,
        thread_id: "original".into(),
        disposition: Disposition::StillApplies,
        head_sha: "same-head".into(),
        base_sha: "same-base".into(),
        rationale: "Earlier resolution was wrong.".into(),
        evidence: vec![],
        duck_call: None,
    };
    let args = json!({"new_comments":[{"finding_id":"original","path":"a.rs","line":1,"body":a.rationale}],
        "finding_dispositions":[a]});
    assert!(tools.handle("leave_comments", &args).1);
    assert!(
        matches!(&*observed.lock().unwrap(), Some(ExitCall::LeaveComments {new_comments, reply_comments, ..})
        if new_comments.is_empty() && reply_comments[0].thread_id == "original")
    );
    assert!(!tools.state.taken());
    tools.gate = Arc::new(|_| Ok(()));
    assert!(!tools.handle("leave_comments", &args).1);
    assert_eq!(*tools.state.findings.lock().unwrap(), vec![a]);
    assert!(tools.handle("leave_comments", &args).1);
}

#[test]
fn missing_identity_or_resolution_evidence_does_not_win_the_terminal_slot() {
    let mut tools = ExitTools::new(
        vec![Exit::LeaveComments, Exit::ReadyForHuman],
        Arc::new(|_| Ok(())),
        None,
    );
    tools.require_finding_identity = true;
    tools.finding_gate = Some(Arc::new(|call, _| match call {
        ExitCall::LeaveComments {
            resolve_own_threads,
            ..
        } if !resolve_own_threads.is_empty() => Err("Resolution evidence missing.".into()),
        _ => Ok(()),
    }));
    assert!(
        tools
            .handle(
                "leave_comments",
                &json!({"new_comments":[{"path":"a.rs","line":1,"body":"Bug."}]})
            )
            .1
    );
    assert!(
        tools
            .handle(
                "leave_comments",
                &json!({"resolve_own_threads":["original"]})
            )
            .1
    );
    assert!(!tools.state.taken());
    assert!(
        !tools
            .handle(
                "ready_for_human",
                &json!({"summary":"Insufficient evidence."})
            )
            .1
    );
}

fn recorded_trace() -> (Arc<Trace>, Arc<Mutex<Vec<serde_json::Value>>>) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let sink = records.clone();
    let trace = Trace::new(
        crate::storage::diagnostics::Attempt {
            id: "attempt-1".into(),
            operation_id: "operation".into(),
            work_id: "work".into(),
            number: 220,
            head: "a".repeat(40),
            model: "review-model".into(),
            started_at_ms: Some(1),
            session_id: None,
            runtime_version: None,
        },
        Arc::new(move |record| {
            sink.lock()
                .unwrap()
                .push(serde_json::to_value(record).unwrap());
            Ok(())
        }),
    );
    (trace, records)
}

fn invocation(name: &str, arguments: serde_json::Value) -> ToolInvocation {
    serde_json::from_value(
        json!({"sessionId":"s","toolCallId":"c","toolName":name,"arguments":arguments}),
    )
    .unwrap()
}

fn events(records: &Mutex<Vec<serde_json::Value>>, kind: &str) -> Vec<serde_json::Value> {
    records
        .lock()
        .unwrap()
        .iter()
        .map(|r| r["event"].clone())
        .filter(|e| e["kind"] == kind)
        .collect()
}

// Live #220: the exit-time re-read failed and the agent saw only "try again".
#[tokio::test]
async fn an_unreadable_exit_gate_ends_the_run_with_a_recorded_typed_cause() {
    let (trace, records) = recorded_trace();
    let tools = ExitTools::new(
        vec![Exit::LeaveComments, Exit::PanicExit],
        Arc::new(|_| Err(Refusal::Unreadable(crate::github::ConnectionError::Network))),
        Some(trace),
    );
    let result = tools
        .call(invocation("leave_comments", json!({"body":"Looks off."})))
        .await
        .unwrap();
    assert!(matches!(result, ToolResult::Expanded(_)));
    assert!(!tools.state.taken());
    let failure = tools.state.take_failure().unwrap();
    assert_eq!(failure.kind, crate::monitoring::OperationFailure::Network);
    assert_eq!(
        events(&records, "exit_refused"),
        vec![
            json!({"kind":"exit_refused","call":1,"refusal":"provider_read","provider_error":"network"})
        ]
    );
}

#[tokio::test]
async fn a_host_panic_in_an_exit_call_is_a_typed_failure_with_its_location() {
    let (trace, records) = recorded_trace();
    let line = line!() + 5;
    let tools = ExitTools::new(
        vec![Exit::ReadyForHuman],
        Arc::new(|_| {
            let empty: Vec<u8> = Vec::new();
            let _ = empty[3];
            Ok(())
        }),
        Some(trace),
    );
    let result = tools
        .call(invocation("ready_for_human", json!({"summary":"Done."})))
        .await
        .unwrap();
    assert!(matches!(result, ToolResult::Expanded(_)));
    let failure = tools.state.take_failure().unwrap();
    assert_eq!(failure.kind, crate::monitoring::OperationFailure::Permanent);
    assert!(failure
        .message
        .ends_with(&format!("exit_tools_tests.rs:{line}.")));
    let panic = &events(&records, "host_panic")[0]["panic"];
    assert_eq!(panic["line"], line);
    assert!(panic.get("message").is_none(), "{panic}");
    assert_eq!(events(&records, "exit_refused")[0]["refusal"], "host_panic");
    assert_eq!(events(&records, "finished").len(), 0);
}
