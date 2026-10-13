use super::*;

fn parse(name: &str, args: Value) -> Result<ExitCall, String> {
    ExitCall::parse(name, &args)
}

#[test]
fn every_exit_has_an_id_a_schema_and_round_trips() {
    for exit in Exit::ALL {
        assert_eq!(Exit::parse(exit.id()), Some(exit));
        assert_eq!(exit.schema()["additionalProperties"], false);
        assert!(!exit.description().is_empty());
    }
    assert!(Exit::parse("bash").is_none());
    assert!(Exit::LeaveComments.effect() && !Exit::ReadyForHuman.effect());
}

#[test]
fn leave_comments_validates_shape_and_bounds() {
    let ok = parse(
        "leave_comments",
        json!({"new_comments":[{"path":"a.rs","line":3,"side":"base","body":"x"}],
            "reply_comments":[{"thread_id":"T","body":"y"}],"resolve_own_threads":["T2"]}),
    )
    .unwrap();
    let (outcome, report) = ok.outcome().unwrap();
    assert!(report.is_none());
    assert!(matches!(outcome, ExitOutcome::Comments { .. }));
    for bad in [
        json!({}),
        json!({"new_comments":[{"path":"a.rs","line":0,"body":"x"}]}),
        json!({"new_comments":[{"path":"a.rs","line":1,"side":"left","body":"x"}]}),
        json!({"new_comments":[{"path":"a.rs","line":1,"body":" "}]}),
        json!({"new_comments":[{"path":"a.rs","line":1,"body":"x".repeat(MAX_COMMENT_BYTES + 1)}]}),
        json!({"new_comments":(0..=MAX_COMMENTS).map(|_| json!({"path":"a","line":1,"body":"x"})).collect::<Vec<_>>()}),
        json!({"resolve_own_threads":[" "]}),
        json!({"new_comments":[{"path":"a.rs","line":1,"body":"x","extra":1}]}),
        json!({"surprise":true}),
    ] {
        assert!(parse("leave_comments", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn small_exits_validate_text_and_reject_unknown_fields() {
    assert!(parse("ready_for_human", json!({"summary":"fine"})).is_ok());
    assert!(parse("ready_for_human", json!({"summary":""})).is_err());
    assert!(parse("ready_for_human", json!({})).is_err());
    assert!(parse("approve_pull_request", json!({"comment":"LGTM"})).is_ok());
    assert!(parse("approve_and_merge", json!({"comment":"LGTM"})).is_ok());
    assert!(parse("approve_and_merge", json!({"comment":"  "})).is_err());
    assert!(parse("approve_pull_request", json!({"comment":"x","merge":true})).is_err());
    assert!(parse("panic_exit", json!({"reason":"r","detail":"d"})).is_ok());
    assert!(parse(
        "panic_exit",
        json!({"reason":"r".repeat(MAX_PANIC_REASON_BYTES + 1),"detail":"d"})
    )
    .is_err());
    let panic = parse("panic_exit", json!({"reason":"r","detail":"d"})).unwrap();
    assert!(panic.outcome().is_none());
    assert_eq!(panic.exit(), Exit::PanicExit);
}

#[test]
fn reports_enforce_nano_words_and_full_fix_bytes_without_truncating() {
    let finding = |nano: String, full: String, fix: String| json!({"findings":[{"file":"a.rs","line":2,"nano":nano,"full":full,"fix":fix}]});
    let words = |n: usize| vec!["w"; n].join(" ");
    let ok = parse(
        "submit_report",
        finding(
            words(100),
            "f".repeat(MAX_FULL_BYTES),
            "x".repeat(MAX_FIX_BYTES),
        ),
    )
    .unwrap();
    let (outcome, report) = ok.outcome().unwrap();
    assert_eq!(outcome, ExitOutcome::SubmitReport);
    assert_eq!(report.unwrap().findings[0].nano.split(' ').count(), 100);
    for bad in [
        finding(words(101), "f".into(), "x".into()),
        finding(String::new(), "f".into(), "x".into()),
        finding("n".into(), "f".repeat(MAX_FULL_BYTES + 1), "x".into()),
        finding("n".into(), "f".into(), "x".repeat(MAX_FIX_BYTES + 1)),
        json!({"findings":[{"file":"a.rs","line":0,"nano":"n","full":"f","fix":"x"}]}),
        json!({"findings":(0..=MAX_FINDINGS).map(|_| json!({"file":"a","line":1,"nano":"n","full":"f","fix":"x"})).collect::<Vec<_>>()}),
        json!({"findings":[{"file":"a.rs","line":1,"nano":"n"}]}),
    ] {
        assert!(parse("submit_report", bad.clone()).is_err(), "{bad}");
    }
    assert!(parse("submit_report", json!({"findings":[]})).is_ok());
}

#[test]
fn outcomes_serialize_stably_and_old_output_still_loads() {
    let outcome = ExitOutcome::ReadyForHuman {
        summary: "s".into(),
    };
    assert_eq!(
        serde_json::to_value(&outcome).unwrap(),
        json!({"exit":"ready_for_human","summary":"s"})
    );
    // A legacy stored output has none of the new fields.
    let legacy: crate::review::ReviewOutput = serde_json::from_value(json!({
        "synopsis":"One.","files":[],"findings":[],"decision":"machine_sign_off"
    }))
    .unwrap();
    assert!(legacy.exit.is_none() && legacy.report.is_none());
    let value = serde_json::to_value(&legacy).unwrap();
    assert!(value.get("exit").is_none() && value.get("report").is_none());
    let exit = crate::review::ReviewOutput::from_exit(outcome, None);
    assert_eq!(exit.decision, crate::review::Decision::HumanInputRequired);
    let back: crate::review::ReviewOutput =
        serde_json::from_value(serde_json::to_value(&exit).unwrap()).unwrap();
    assert_eq!(back, exit);
}
