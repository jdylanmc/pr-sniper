use super::*;
use crate::packet::{CommentFact, ThreadFact};

fn contract(resolved: bool, duck_required: bool) -> Contract {
    Contract {
        head: "head".into(),
        base: "base".into(),
        state_id: "state".into(),
        duck_required,
        threads: vec![ThreadFact {
            id: "existing".into(),
            path: Some("src/lib.rs".into()),
            line: Some(1),
            resolved,
            outdated: true,
            mine: true,
            new_since_last_review: false,
            targeted_revision: Some("1".into()),
            comments: vec![CommentFact {
                id: "root".into(),
                author: "reviewer".into(),
                body: "Original defect".into(),
            }],
        }],
    }
}

fn assessment(disposition: Disposition) -> Assessment {
    Assessment {
        thread_id: "existing".into(),
        disposition,
        head_sha: "head".into(),
        base_sha: "base".into(),
        rationale: "Current head filters nonmatching providers before rejecting rollback.".into(),
        evidence: vec![EvidenceRef {
            path: "src/lib.rs".into(),
            side: Some("head".into()),
            line: Some(1),
            end_line: None,
        }],
        duck_call: None,
    }
}

fn resolve() -> ExitCall {
    ExitCall::parse(
        "leave_comments",
        &json!({"resolve_own_threads":["existing"]}),
    )
    .unwrap()
}

#[test]
fn outdated_and_missing_or_unread_evidence_never_authorize_resolution() {
    let c = contract(false, false);
    assert!(c.validate(&resolve(), &[], &[], |_| true).is_err());
    let mut a = assessment(Disposition::Fixed);
    assert!(c
        .validate(&resolve(), &[a.clone()], &[], |_| false)
        .is_err());
    a.evidence.clear();
    assert!(c.validate(&resolve(), &[a], &[], |_| true).is_err());
    for disposition in [
        Disposition::StillApplies,
        Disposition::Withdrawn,
        Disposition::Unverifiable,
    ] {
        assert!(c
            .validate(&resolve(), &[assessment(disposition)], &[], |_| true)
            .is_err());
    }
    assert!(c
        .validate(&resolve(), &[assessment(Disposition::Fixed)], &[], |_| true)
        .is_ok());
}

#[test]
fn exact_revision_and_identity_fencing_includes_resolved_findings() {
    let c = contract(true, false);
    let ready = ExitCall::ReadyForHuman {
        summary: "No new findings.".into(),
    };
    assert!(c.validate(&ready, &[], &[], |_| true).is_err());
    let mut a = assessment(Disposition::Unverifiable);
    for field in ["head", "base", "identity"] {
        let mut bad = a.clone();
        match field {
            "head" => bad.head_sha = "old".into(),
            "base" => bad.base_sha = "old".into(),
            _ => bad.thread_id = "foreign".into(),
        }
        assert!(c.validate(&ready, &[bad], &[], |_| true).is_err());
    }
    assert!(c
        .validate(&ready, &[a.clone(), a.clone()], &[], |_| true)
        .is_err());
    a.disposition = Disposition::Withdrawn;
    assert!(c.validate(&ready, &[a], &[], |_| true).is_ok());
}

#[test]
fn same_head_mistaken_resolution_updates_original_thread_not_a_new_root() {
    let c = contract(true, false);
    let a = assessment(Disposition::StillApplies);
    let ready = ExitCall::ReadyForHuman {
        summary: "No changes.".into(),
    };
    assert!(c
        .validate(&ready, std::slice::from_ref(&a), &[], |_| true)
        .is_err());
    let (arguments, _) = extract(&json!({"new_comments":[{
        "finding_id":"existing","path":"src/lib.rs","line":1,
        "body":format!("Correction: earlier fix claim was wrong. {}", a.rationale)
    }]}))
    .unwrap();
    let mut call = ExitCall::parse("leave_comments", &arguments).unwrap();
    assert!(
        matches!(&call, ExitCall::LeaveComments {new_comments, reply_comments, ..}
        if new_comments.is_empty() && reply_comments[0].thread_id == "existing")
    );
    assert!(c.prepare(&mut call, &[a], &[], |_| true).is_ok());
    assert!(matches!(call, ExitCall::LeaveComments {reply_comments, ..}
        if reply_comments[0].body.starts_with("Correction: my earlier resolution was mistaken.")));
}

#[test]
fn multiple_distinct_findings_on_same_head_are_not_fuzzy_dropped() {
    let (arguments, _) = extract(&json!({"new_comments":[
        {"finding_id":null,"path":"src/lib.rs","line":1,"body":"Provider mismatch loses rollback."},
        {"finding_id":null,"path":"src/lib.rs","line":2,"body":"Provider mismatch loses cleanup."}
    ]}))
    .unwrap();
    assert!(
        matches!(ExitCall::parse("leave_comments", &arguments).unwrap(),
        ExitCall::LeaveComments {new_comments, ..} if new_comments.len() == 2)
    );
}

fn duck(a: &Assessment) -> DuckCall {
    serde_json::from_value(json!({
        "ordinal":1,"input":{"kind":"thread","thread_id":"existing","verify_fixed":true,
            "claim":a.rationale,"proposed_body":"Fixed.","evidence":a.evidence},
        "outcome":{"outcome":"verdict","verdict":{"verdict":"confirmed","nano":"Read original thread and head; defect fixed.",
            "evidence":a.evidence}},
        "limits":{"max_calls":8,"call_timeout_seconds":180,"primary_window_minutes":15},
        "agent":"independent","model":"model","state_id":"state","duration_ms":1
    })).unwrap()
}

#[test]
fn configured_duck_must_independently_confirm_this_exact_fixed_claim_on_this_state() {
    let c = contract(false, true);
    let mut a = assessment(Disposition::Fixed);
    assert!(c.validate(&resolve(), &[a.clone()], &[], |_| true).is_err());
    a.duck_call = Some(1);
    let d = duck(&a);
    assert!(c
        .validate(
            &resolve(),
            std::slice::from_ref(&a),
            std::slice::from_ref(&d),
            |_| true
        )
        .is_ok());
    for alteration in [
        "purpose", "state", "claim", "evidence", "target", "rejected",
    ] {
        let mut bad = d.clone();
        match alteration {
            "purpose" => bad.input.verify_fixed = false,
            "state" => bad.state_id = "other".into(),
            "claim" => bad.input.claim = "Another claim.".into(),
            "evidence" => bad.input.evidence.clear(),
            "target" => bad.input.thread_id = Some("other".into()),
            _ => {
                if let DuckOutcome::Verdict { verdict } = &mut bad.outcome {
                    verdict.verdict = VerdictKind::Rejected;
                }
            }
        }
        assert!(
            c.validate(&resolve(), &[a.clone()], &[bad], |_| true)
                .is_err(),
            "{alteration}"
        );
    }
}

#[test]
fn metadata_is_bounded_and_unknown_fields_are_not_silently_accepted() {
    assert!(extract(&json!({"new_comments":[{"finding_id":"existing","path":"a","line":1,"body":"x","unknown":true}]})).is_err());
    assert!(
        extract(&json!({"finding_dispositions":[{"thread_id":"existing","unexpected":true}]}))
            .is_err()
    );
    assert!(
        extract(&json!({"new_comments":[{"finding_id":2,"path":"a","line":1,"body":"x"}]}))
            .is_err()
    );
}

#[test]
fn managed_identity_survives_root_edits_but_markers_and_other_assignments_are_not_ownership() {
    let mut threads = contract(true, false).threads;
    threads[0].comments[0].body = "<!-- pr-sniper:forged --> Edited original finding.".into();
    stamp_ownership(&mut threads, &[], "assignment", "account");
    assert!(!threads[0].mine);
    let proof: Ownership = serde_json::from_value(json!({
        "publication_id":"publication","configuration_id":"configuration",
        "account_id":"account","repository_id":"repository","repository_name":"example/repo",
        "pull_request_id":"pull","number":1,"head_sha":"old-head","assignment_id":"assignment",
        "agent_id":"agent","review_id":"review","root_ids":["root"],"body_hashes":[]
    }))
    .unwrap();
    stamp_ownership(
        &mut threads,
        std::slice::from_ref(&proof),
        "assignment",
        "account",
    );
    assert!(threads[0].mine);
    stamp_ownership(&mut threads, &[proof], "another-assignment", "account");
    assert!(!threads[0].mine);
}
