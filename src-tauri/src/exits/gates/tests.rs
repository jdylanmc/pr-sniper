use super::*;
use crate::exits::ReplyComment;
use crate::github::actions::{HumanThread, ProviderReview};
use serde_json::json;

const ALL_TOOLS: [&str; 6] = [
    "leave_comments",
    "ready_for_human",
    "approve_pull_request",
    "approve_and_merge",
    "panic_exit",
    "submit_report",
];

fn base() -> GateInput {
    GateInput {
        tools: ALL_TOOLS.map(String::from).to_vec(),
        primary: true,
        grants: Grants {
            comment: true,
            reply: true,
            approve: true,
            merge: true,
            include_self: false,
        },
        review_drafts: false,
        facts: Facts::default(),
    }
}

fn offer(input: &GateInput, exit: Exit) -> Option<Offer> {
    offered(input)
        .into_iter()
        .find(|(e, _)| *e == exit)
        .map(|(_, o)| o)
}

fn is_offered(input: &GateInput, exit: Exit) -> bool {
    offer(input, exit).is_some_and(|o| o.is_offered())
}

#[test]
fn wait_requires_a_concrete_prerequisite_to_an_enabled_action() {
    let mut input = base();
    input.tools.push("wait".into());
    input.facts.ci = Ci::Pending;
    assert_eq!(wait_reasons(&input), [WaitReason::Ci]);
    assert!(is_offered(&input, Exit::Wait));
    type Edit = fn(&mut GateInput);
    let denied: [Edit; 9] = [
        |i| i.primary = false,
        |i| i.grants.merge = false,
        |i| i.tools.retain(|t| t != "approve_and_merge"),
        |i| i.facts.ci = Ci::Red,
        |i| i.facts.ci = Ci::Unknown,
        |i| i.facts.unresolved_threads = true,
        |i| i.facts.changes_requested = true,
        |i| i.facts.wait_merge_blocker = Some("Permission unavailable".into()),
        |i| i.facts.author_is_acting = true,
    ];
    for edit in denied {
        let mut blocked = input.clone();
        edit(&mut blocked);
        assert!(!is_offered(&blocked, Exit::Wait), "{blocked:?}");
    }
    input.facts.ci = Ci::Green;
    input.facts.required_reviews_unmet = true;
    assert!(
        !is_offered(&input, Exit::Wait),
        "supply the permitted own vote before waiting"
    );
    assert!(is_offered(&input, Exit::ApprovePullRequest));
    input.facts.already_approved = true;
    assert_eq!(wait_reasons(&input), [WaitReason::RequiredReviews]);
    input.grants.merge = false;
    assert!(
        !is_offered(&input, Exit::Wait),
        "approve-only must not wait for reviews"
    );
    input.facts.draft = true;
    assert_eq!(wait_reasons(&input), [WaitReason::Draft]);
    input.grants.approve = false;
    assert!(
        !is_offered(&input, Exit::Wait),
        "comment-only must not wait for merge"
    );
}

#[test]
fn the_decided_scenario_table() {
    use Exit::{
        ApproveAndMerge as Merge, ApprovePullRequest as Approve, LeaveComments as Comment,
        ReadyForHuman as Ready,
    };
    type Row = (&'static str, fn(&mut GateInput), [bool; 4]);
    // Columns: comment, ready, approve, approve_and_merge.
    let rows: [Row; 17] = [
        ("green, clean", |_| {}, [true, true, true, true]),
        (
            "CI red",
            |i| i.facts.ci = Ci::Red,
            [true, true, false, false],
        ),
        (
            "merge conflicts",
            |i| i.facts.conflicts = true,
            [true, true, false, false],
        ),
        (
            "CI pending",
            |i| i.facts.ci = Ci::Pending,
            [true, true, true, false],
        ),
        (
            "CI unknown",
            |i| i.facts.ci = Ci::Unknown,
            [true, true, true, false],
        ),
        (
            "required reviews unmet",
            |i| i.facts.required_reviews_unmet = true,
            [true, true, true, false],
        ),
        (
            "changes requested by others",
            |i| i.facts.changes_requested = true,
            [true, true, true, false],
        ),
        (
            "others' unresolved threads",
            |i| i.facts.unresolved_threads = true,
            [true, true, true, false],
        ),
        (
            "draft, review_drafts off",
            |i| i.facts.draft = true,
            [false, true, false, false],
        ),
        (
            "draft, review_drafts on",
            |i| {
                i.facts.draft = true;
                i.review_drafts = true
            },
            [true, true, false, false],
        ),
        (
            "closed",
            |i| i.facts.open = false,
            [false, true, false, false],
        ),
        (
            "self-authored",
            |i| i.facts.author_is_acting = true,
            [true, true, false, false],
        ),
        (
            "self-authored, include_self",
            |i| {
                i.facts.author_is_acting = true;
                i.grants.include_self = true
            },
            [true, true, false, true],
        ),
        (
            "already approved",
            |i| i.facts.already_approved = true,
            [true, true, true, true],
        ),
        (
            "no comment grant",
            |i| i.grants.comment = false,
            [false, true, true, true],
        ),
        (
            "approve only grant",
            |i| i.grants.merge = false,
            [true, true, true, false],
        ),
        (
            "no approve grants",
            |i| {
                i.grants.approve = false;
                i.grants.merge = false
            },
            [true, true, false, false],
        ),
    ];
    for (name, change, expected) in rows {
        let mut input = base();
        change(&mut input);
        let got = [Comment, Ready, Approve, Merge].map(|e| is_offered(&input, e));
        assert_eq!(got, expected, "{name}: {:?}", offered(&input));
    }
}

#[test]
fn denials_carry_a_short_directive_reason() {
    let mut input = base();
    input.facts.ci = Ci::Red;
    match offer(&input, Exit::ApprovePullRequest).unwrap() {
        Offer::Denied(reason) => assert!(reason.contains("CI is red") && reason.len() < 100),
        Offer::Offered => panic!("offered"),
    }
    input.facts.ci = Ci::Pending;
    assert_eq!(
        offer(&input, Exit::ApproveAndMerge),
        Some(Offer::Denied("CI is pending. Cannot merge yet.".into()))
    );
    input.facts = Facts {
        author_is_acting: true,
        ..Facts::default()
    };
    let reason = match offer(&input, Exit::ApprovePullRequest).unwrap() {
        Offer::Denied(reason) => reason,
        Offer::Offered => panic!("offered"),
    };
    assert!(reason.contains("self-approval"));
}

#[test]
fn self_authorship_never_masks_an_actionable_pr_state_reason() {
    // Live on the sandbox: a self-authored red PR showed only the self-approval reason,
    // so the agent never learned CI was red and handed off instead of commenting.
    for (fact, approve, merge) in [
        (
            (|f: &mut Facts| f.ci = Ci::Red) as fn(&mut Facts),
            "CI is red",
            "CI is red",
        ),
        (|f| f.conflicts = true, "Merge conflicts", "Merge conflicts"),
        (|_| {}, "self-approval", "Include self is off"),
    ] {
        let mut input = base();
        input.facts.author_is_acting = true;
        fact(&mut input.facts);
        for (exit, expected) in [
            (Exit::ApprovePullRequest, approve),
            (Exit::ApproveAndMerge, merge),
        ] {
            match offer(&input, exit).unwrap() {
                Offer::Denied(reason) => assert!(reason.contains(expected), "{reason}"),
                Offer::Offered => panic!("{exit:?} offered"),
            }
        }
    }
}

#[test]
fn provider_blockers_outside_the_table_still_deny() {
    let mut input = base();
    input.facts.merge_blocker = Some("Provider merge queue.".into());
    assert!(is_offered(&input, Exit::ApprovePullRequest));
    assert!(!is_offered(&input, Exit::ApproveAndMerge));
    input.facts.approve_blocker = Some("No write access.".into());
    assert!(!is_offered(&input, Exit::ApprovePullRequest));
}

#[test]
fn roles_and_assignment_tools_intersect_with_the_gates() {
    let mut teammate = base();
    teammate.primary = false;
    let ids: Vec<_> = offered(&teammate).iter().map(|(e, _)| e.id()).collect();
    assert_eq!(ids, ["panic_exit", "submit_report"]);
    let primary: Vec<_> = offered(&base()).iter().map(|(e, _)| e.id()).collect();
    assert_eq!(
        primary,
        [
            "leave_comments",
            "ready_for_human",
            "approve_pull_request",
            "approve_and_merge",
            "panic_exit"
        ]
    );
    let mut narrow = base();
    narrow.tools = vec!["ready_for_human".into(), "read_changes".into()];
    let ids: Vec<_> = offered(&narrow).iter().map(|(e, _)| e.id()).collect();
    assert_eq!(ids, ["ready_for_human"]);
}

#[test]
fn exit_time_check_rechecks_gates_and_payload_grants() {
    let comments = ExitCall::LeaveComments {
        new_comments: vec![],
        body: None,
        reply_comments: vec![ReplyComment {
            thread_id: "T".into(),
            body: "b".into(),
        }],
        resolve_own_threads: vec![],
    };
    assert!(check_call(&base(), &comments).is_ok());
    let mut no_reply = base();
    no_reply.grants.reply = false;
    assert!(check_call(&no_reply, &comments).is_err());
    let mut red = base();
    red.facts.ci = Ci::Red;
    let approve = ExitCall::ApprovePullRequest {
        comment: "ok".into(),
    };
    assert!(check_call(&red, &approve)
        .unwrap_err()
        .contains("CI is red"));
    let mut none = base();
    none.tools = vec!["panic_exit".into()];
    assert_eq!(
        check_call(&none, &approve).unwrap_err(),
        "Exit not available. Pick another."
    );
}

#[test]
fn idempotent_approve_is_a_fact_of_the_gate_input() {
    let mut input = base();
    assert!(!approve_is_idempotent(&input));
    input.facts.already_approved = true;
    assert!(approve_is_idempotent(&input));
    assert!(is_offered(&input, Exit::ApprovePullRequest));
}

fn observation() -> Observation {
    serde_json::from_value(json!({
        "write_capability": true, "node_id": "n", "repository_id": "1", "pull_request_id": "2",
        "account_id": "7", "author_id": "8", "head_repository_id": "1",
        "head": "a".repeat(40), "base": "b".repeat(40), "base_name": "main",
        "merge_rules": [], "merge_rules_error": null, "state": "OPEN", "draft": false,
        "permission": "WRITE", "mergeable": "MERGEABLE", "merge_state": "CLEAN",
        "review_decision": "APPROVED", "checks": "SUCCESS", "check_contexts": [],
        "in_merge_queue": false, "method": "SQUASH",
        "protection": {"requiresApprovingReviews": false},
        "threads": [], "reviews": [], "comments": [],
        "merged_by": null, "merged_at": null, "merge_commit": null
    }))
    .unwrap()
}

#[test]
fn provider_observations_normalize_into_facts() {
    let green = Facts::from_observation(&observation());
    assert_eq!(green.ci, Ci::Green);
    assert!(green.open && !green.draft && !green.conflicts);
    assert!(green.approve_blocker.is_none() && green.merge_blocker.is_none());

    let mut o = observation();
    o.checks = Some("PENDING".into());
    assert_eq!(Facts::from_observation(&o).ci, Ci::Pending);
    o.checks = Some("NONPASSING".into());
    o.check_contexts = vec![
        json!({"__typename":"CheckRun","name":"t","status":"COMPLETED","conclusion":"FAILURE"}),
    ];
    assert_eq!(Facts::from_observation(&o).ci, Ci::Red);
    o.check_contexts =
        vec![json!({"__typename":"CheckRun","name":"t","status":"IN_PROGRESS","conclusion":null})];
    assert_eq!(Facts::from_observation(&o).ci, Ci::Pending);
    o.checks = None;
    assert_eq!(Facts::from_observation(&o).ci, Ci::Unknown);

    let mut o = observation();
    o.mergeable = "CONFLICTING".into();
    o.merge_state = "DIRTY".into();
    assert!(Facts::from_observation(&o).conflicts);

    // Others' unresolved threads and changes requested: approve stays open, merge closes.
    let mut o = observation();
    o.threads = vec![HumanThread {
        id: "T".into(),
        resolved: false,
        outdated: false,
        resolved_by: None,
        comments: vec![],
    }];
    o.reviews = vec![ProviderReview {
        id: "r".into(),
        actor_id: "99".into(),
        head: o.head.clone(),
        state: "CHANGES_REQUESTED".into(),
        body: String::new(),
        submitted_at: None,
    }];
    o.review_decision = Some("CHANGES_REQUESTED".into());
    let facts = Facts::from_observation(&o);
    assert!(facts.unresolved_threads && facts.changes_requested);
    assert!(facts.approve_blocker.is_none());
    assert!(facts.merge_blocker.is_some());

    let mut o = observation();
    o.review_decision = Some("REVIEW_REQUIRED".into());
    assert!(Facts::from_observation(&o).required_reviews_unmet);

    let mut o = observation();
    o.author_id = o.account_id.clone();
    assert!(Facts::from_observation(&o).author_is_acting);

    let mut o = observation();
    o.reviews = vec![ProviderReview {
        id: "r".into(),
        actor_id: o.account_id.clone(),
        head: o.head.clone(),
        state: "APPROVED".into(),
        body: String::new(),
        submitted_at: None,
    }];
    assert!(Facts::from_observation(&o).already_approved);
    o.reviews[0].head = "c".repeat(40);
    assert!(!Facts::from_observation(&o).already_approved);
}

#[test]
fn approve_ignores_discussion_that_only_blocks_merge() {
    let mut o = observation();
    o.threads = vec![HumanThread {
        id: "T".into(),
        resolved: false,
        outdated: true,
        resolved_by: None,
        comments: vec![],
    }];
    assert!(o.blocker(Action::Approve).is_none());
    assert!(o.blocker(Action::Merge).unwrap().contains("discussions"));
    assert!(o.discussion_blocker().is_some());
    o.reviews = vec![ProviderReview {
        id: "p".into(),
        actor_id: "7".into(),
        head: o.head.clone(),
        state: "PENDING".into(),
        body: String::new(),
        submitted_at: None,
    }];
    assert!(o.blocker(Action::Approve).unwrap().contains("pending"));
}
