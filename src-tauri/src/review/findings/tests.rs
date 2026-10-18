use super::*;
use crate::packet::{CommentFact, ThreadFact};

fn contract(resolved: bool, duck_required: bool) -> Contract {
    Contract {
        head: "head".into(),
        base: "base".into(),
        state_id: "state".into(),
        duck_required,
        corrections: Vec::new(),
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
fn confirmed_correction_survives_another_assessment_without_another_reply() {
    let mut c = contract(true, false);
    let a = assessment(Disposition::StillApplies);
    let write = correction_write();
    add_correction(&mut c, &write);
    c.corrections = corrections(std::slice::from_ref(&write), &write.job, "assignment");
    c.state_id = "same-head-base-new-ci-and-conversation-state".into();
    let mut call = ExitCall::ReadyForHuman {
        summary: "Existing correction still applies; nothing new to post.".into(),
    };
    assert!(
        c.prepare(&mut call, &[a], &[], |_| true).is_ok(),
        "A confirmed correction on the original thread must not demand another reply"
    );
}

fn correction_write() -> crate::exit_writes::ExitWrite {
    let a = assessment(Disposition::StillApplies);
    let mut write: crate::exit_writes::ExitWrite = serde_json::from_value(json!({
        "id":"correction","review_operation_id":"review",
        "job":{"provider":"github","account_id":"account","account_login":"reviewer",
            "configuration_id":"configuration","repository_id":"repo","repository_name":"example/repo",
            "pull_request_id":"pull","number":1,"title":"Fix","head_sha":"head",
            "trigger_policy":"all","watched_author":true,"requested_reviewer":false,
            "waiting":"ready","detected_at":1},
        "assignment_id":"assignment","comments_first":false,"published":false,
        "steps":[{"kind":"reply","thread_id":"existing","body":"","root_id":"root",
            "revision":"1","state":"done","receipt":"correction"}],
        "phase":"completed","operation":{"id":"operation","provider":"github",
            "account_id":"account","configuration_id":"configuration","repository_id":"repo",
            "trigger_policy":"all","operation_type":"reply","state":"completed",
            "attempt_count":1,"initial_attempt_at":1,"retry_deadline":100},
        "uncertain":false,"consumed":true,"reviewed_base_sha":"base"
    })).unwrap();
    write.steps[0].body = write.reply_body(0, &correction_text(&a.rationale), "1");
    // Use the exact persisted shape consumed after restart.
    serde_json::from_value(serde_json::to_value(write).unwrap()).unwrap()
}

fn add_correction(c: &mut Contract, write: &crate::exit_writes::ExitWrite) {
    c.threads[0].comments.push(CommentFact {
        id: write.steps[0].receipt.clone().unwrap(),
        author: "reviewer".into(),
        body: write.steps[0].body.clone(),
    });
}

fn correction_call(body: &str) -> ExitCall {
    ExitCall::parse(
        "leave_comments",
        &json!({"reply_comments":[{"thread_id":"existing","body":body}]}),
    )
    .unwrap()
}

#[test]
fn confirmed_corrections_deny_exact_reposts_but_allow_new_information() {
    let mut c = contract(true, false);
    let a = assessment(Disposition::StillApplies);
    let write = correction_write();
    add_correction(&mut c, &write);
    c.corrections = corrections(std::slice::from_ref(&write), &write.job, "assignment");
    for body in [a.rationale.clone(), correction_text(&a.rationale)] {
        let mut call = correction_call(&body);
        assert!(c
            .prepare(&mut call, std::slice::from_ref(&a), &[], |_| true)
            .unwrap_err()
            .contains("already confirmed"));
    }
    let mut revised = a.clone();
    revised.rationale = "New evidence: the second provider is also affected.".into();
    let mut call = correction_call(&revised.rationale);
    assert!(c.prepare(&mut call, &[revised], &[], |_| true).is_ok());
    // An attempted duplicate is not silently replaced with another exit.
    assert!(matches!(call, ExitCall::LeaveComments { .. }));
}

#[test]
fn uncertain_or_unreadable_corrections_never_create_replacement_replies() {
    use crate::exit_writes::StepState;
    for outcome in ["attempted", "missing", "edited", "root", "receipt"] {
        let mut c = contract(true, false);
        let mut write = correction_write();
        add_correction(&mut c, &write);
        match outcome {
            "attempted" => {
                write.steps[0].state = StepState::Attempted;
                write.steps[0].receipt = None;
            }
            "missing" => {
                c.threads[0].comments.pop();
            }
            "edited" => c.threads[0].comments[1].body.push_str(" edited"),
            "root" => c.threads[0].comments[0].id = "other-root".into(),
            _ => write.steps[0].receipt = Some("wrong-receipt".into()),
        }
        c.corrections = corrections(&[write.clone()], &write.job, "assignment");
        let a = assessment(Disposition::StillApplies);
        let mut call = correction_call(&format!("New rationale. {}", a.rationale));
        assert!(
            c.prepare(&mut call, std::slice::from_ref(&a), &[], |_| true)
                .unwrap_err()
                .contains("no replacement"),
            "{outcome}"
        );
        let mut ready = ExitCall::ReadyForHuman {
            summary: "Await correction reconciliation.".into(),
        };
        assert!(
            c.prepare(&mut ready, &[a], &[], |_| true).is_ok(),
            "{outcome}"
        );
        c.threads[0].resolved = false;
        let a = assessment(Disposition::Unverifiable);
        assert!(
            c.prepare(&mut call, &[a], &[], |_| true).is_err(),
            "Changing disposition or reopening cannot authorize a replacement: {outcome}"
        );
    }
}

#[test]
fn correction_receipts_are_assignment_account_and_pr_scoped_not_agent_scoped() {
    let write = correction_write();
    for field in ["account", "provider", "repository", "pull"] {
        let mut other = write.job.clone();
        match field {
            "account" => other.account_id = "other".into(),
            "provider" => other.provider = "other".into(),
            "repository" => other.repository_id = "other".into(),
            _ => other.pull_request_id = "other".into(),
        }
        assert!(
            corrections(std::slice::from_ref(&write), &other, "assignment").is_empty(),
            "{field}"
        );
    }
    assert!(corrections(
        std::slice::from_ref(&write),
        &write.job,
        "replacement-assignment"
    )
    .is_empty());
    assert_eq!(
        corrections(std::slice::from_ref(&write), &write.job, "assignment").len(),
        1
    );
}

#[test]
fn later_resolution_starts_a_new_correction_episode_but_uncertain_resolution_blocks_reposting() {
    use crate::exit_writes::{StepKind, StepState};
    let write = correction_write();
    for state in [StepState::Planned, StepState::Attempted, StepState::Done] {
        let mut later = write.clone();
        later.id = "new-resolution".into();
        later.operation.initial_attempt_at = 2;
        later.steps[0].kind = StepKind::Resolve;
        later.steps[0].body.clear();
        later.steps[0].state = state;
        let mut c = contract(true, false);
        add_correction(&mut c, &write);
        // Sorting follows write attempts rather than the serialized array order.
        c.corrections = corrections(&[later, write.clone()], &write.job, "assignment");
        let a = assessment(Disposition::StillApplies);
        let mut ready = ExitCall::ReadyForHuman {
            summary: "Nothing new.".into(),
        };
        assert_eq!(
            c.prepare(&mut ready, std::slice::from_ref(&a), &[], |_| true)
                .is_err(),
            state == StepState::Done
        );
        let mut call = correction_call(&a.rationale);
        assert_eq!(
            c.prepare(&mut call, &[a], &[], |_| true).is_ok(),
            state == StepState::Done
        );
    }
}

#[test]
fn historical_correction_reposts_are_denied_after_a_genuine_update_and_noop_resolution() {
    let first = correction_write();
    let mut update = first.clone();
    update.id = "updated-correction".into();
    update.operation.initial_attempt_at = 2;
    update.steps[0].receipt = Some("updated".into());
    update.steps[0].body = update.reply_body(0, &correction_text("New evidence."), "1");
    let mut noop = first.clone();
    noop.id = "noop-resolution".into();
    noop.operation.initial_attempt_at = 3;
    noop.steps[0].kind = crate::exit_writes::StepKind::Resolve;
    noop.steps[0].receipt = Some("already".into());
    let mut c = contract(true, false);
    add_correction(&mut c, &first);
    add_correction(&mut c, &update);
    c.corrections = corrections(&[first.clone(), update, noop], &first.job, "assignment");
    let a = assessment(Disposition::StillApplies);
    let mut call = correction_call(&a.rationale);
    assert!(c
        .prepare(&mut call, std::slice::from_ref(&a), &[], |_| true)
        .unwrap_err()
        .contains("already confirmed"));
    let mut ready = ExitCall::ReadyForHuman {
        summary: "Already corrected.".into(),
    };
    assert!(c.prepare(&mut ready, &[a], &[], |_| true).is_ok());
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
