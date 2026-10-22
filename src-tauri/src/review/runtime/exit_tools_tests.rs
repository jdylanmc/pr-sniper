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
