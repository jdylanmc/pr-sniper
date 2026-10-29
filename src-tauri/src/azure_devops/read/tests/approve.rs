//! Approve (FR-37, AC-19) against a fake Azure DevOps: the bound identity's
//! reviewer vote of 10, read before it is set and never sent twice.
//! Approve-and-merge is never offered.

use super::publish::{author, git, iterations, review, runtime, session, sha, threads_url};
use super::*;
use crate::actions::{self, Effect, EffectState};
use crate::azure_devops::read::session::{VOTE_REFUSED, VOTE_STANDS};
use crate::exits::gates::{self, Facts, GateInput, Grants, Offer, UNSUPPORTED};
use crate::exits::{Exit, LgtmAction};
use crate::monitoring::{JobOperation, OperationFailure};
use crate::provider::{Action, ProviderCapabilities, ReadinessReader, ReviewerOutcome};
use crate::publication::WriteFailure;
use crate::storage::ProviderId;

fn repository() -> RemoteRepository {
    RemoteRepository {
        id: key().id(),
        name: "contoso/Web/app".into(),
    }
}

fn reviewers_url() -> String {
    git("pullRequests/7/reviewers?")
}

fn vote_url() -> String {
    git(&format!("pullRequests/7/reviewers/{AVERY}?"))
}

fn writes(wire: &FakeWire) -> Vec<Request> {
    wire.sent
        .lock()
        .unwrap()
        .iter()
        .map(|(request, _)| request.clone())
        .filter(|request| request.method != Method::Get)
        .collect()
}

fn reviewers(wire: &FakeWire, votes: &[(&str, i64)]) {
    let value: Vec<_> = votes
        .iter()
        .map(|(id, vote)| json!({"id": id, "displayName": "Someone", "vote": vote}))
        .collect();
    wire.json(&reviewers_url(), json!({ "value": value }));
}

fn approve(session: &Session) -> Result<crate::provider::OutcomeReceipt, WriteFailure> {
    session.approve_exact(&repository(), 7, AVERY, &sha(2), "")
}

fn confirmed(
    result: Result<crate::provider::OutcomeReceipt, WriteFailure>,
) -> crate::provider::OutcomeReceipt {
    result.unwrap_or_else(|e| panic!("{}", e.failure.message))
}

#[test]
fn approves_by_setting_the_bound_identity_vote_to_ten() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    iterations(&wire);
    reviewers(&wire, &[(CASEY, 0)]);
    wire.json(&vote_url(), json!({"id": AVERY, "vote": 10}));

    let receipt = confirmed(approve(&session));
    assert_eq!(receipt.id, format!("7:{AVERY}"));
    assert_eq!(receipt.actor_id, AVERY);
    assert_eq!(receipt.head, sha(2));
    assert_eq!(receipt.action, Action::Approve);
    assert_eq!(receipt.merge_commit, None);
    let writes = writes(&wire);
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].method, Method::Put);
    assert_eq!(writes[0].url, format!("{}api-version=7.1", vote_url()));
    assert_eq!(writes[0].body, Some(json!({"vote": 10})));
    // Read before it acts: head, then the identity's current vote.
    let urls = wire.urls();
    let position = |prefix: String| urls.iter().position(|u| u.starts_with(&prefix)).unwrap();
    assert!(position(git("pullRequests/7/iterations?")) < position(reviewers_url()));
    assert!(position(reviewers_url()) < position(vote_url()));
}

#[test]
fn an_approve_vote_already_held_is_the_receipt_and_nothing_is_sent() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    for vote in [10, 5] {
        iterations(&wire);
        reviewers(&wire, &[(CASEY, -10), (&AVERY.to_uppercase(), vote)]);
        let receipt = confirmed(approve(&session));
        assert_eq!(receipt.id, format!("7:{AVERY}"));
    }
    assert!(writes(&wire).is_empty());
}

#[test]
fn the_identity_vote_against_the_pr_is_never_replaced() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    for vote in [-5, -10] {
        iterations(&wire);
        reviewers(&wire, &[(AVERY, vote)]);
        let failure = approve(&session).unwrap_err();
        assert!(!failure.uncertain);
        assert_eq!(failure.failure.message, VOTE_STANDS);
    }
    assert!(writes(&wire).is_empty());
}

#[test]
fn a_policy_refusal_is_a_typed_permission_failure_with_no_effect() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    for refusal in [400, 403, 409] {
        iterations(&wire);
        reviewers(&wire, &[]);
        wire.on(&vote_url(), Ok(status(refusal, None)));
        let failure = approve(&session).unwrap_err();
        assert!(!failure.uncertain, "{refusal}");
        assert_eq!(failure.failure.kind, OperationFailure::Permission);
        assert_eq!(failure.failure.message, VOTE_REFUSED);
    }
}

#[test]
fn a_lost_or_unconfirmed_vote_is_uncertain() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    for reply in [
        Ok(status(500, None)),
        Err(NetworkError),
        Ok(ok(json!({"id": AVERY, "vote": 0}))),
        Ok(ok(json!({"id": CASEY, "vote": 10}))),
    ] {
        iterations(&wire);
        reviewers(&wire, &[]);
        wire.on(&vote_url(), reply);
        assert!(approve(&session).unwrap_err().uncertain);
    }
}

#[test]
fn a_moved_head_or_another_identity_never_votes() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    iterations(&wire);
    let moved = session
        .approve_exact(&repository(), 7, AVERY, &sha(1), "")
        .unwrap_err();
    assert!(!moved.uncertain);
    let other = session
        .approve_exact(&repository(), 7, BLAKE, &sha(2), "")
        .unwrap_err();
    assert!(!other.uncertain);
    assert!(writes(&wire).is_empty());
    assert!(!wire.urls().iter().any(|u| u.contains("/reviewers")));
}

#[test]
fn approve_and_merge_is_never_offered_and_completion_is_refused() {
    let capabilities = ProviderCapabilities::of(&ProviderId::AzureDevops);
    assert!(capabilities.approve.is_available());
    assert!(!capabilities.approve_and_merge.is_available());
    let input = GateInput {
        tools: vec![
            Exit::ApprovePullRequest.id().into(),
            Exit::ApproveAndMerge.id().into(),
        ],
        primary: true,
        grants: Grants {
            approve: true,
            merge: true,
            ..Grants::default()
        },
        review_drafts: false,
        capabilities,
        facts: Facts::default(),
    };
    let offered = gates::offered(&input);
    let offer = |exit| offered.iter().find(|(e, _)| *e == exit).unwrap().1.clone();
    assert_eq!(
        offer(Exit::ApproveAndMerge),
        Offer::Denied(UNSUPPORTED.into())
    );
    assert_eq!(offer(Exit::ApprovePullRequest), Offer::Offered);
    // A sign-off with the merge grant approves only.
    assert_eq!(gates::lgtm_action(&input), LgtmAction::Approve);
    // Self-authored: denied before any vote, with this provider's reason.
    let mut own = input.clone();
    own.facts.author_is_acting = true;
    assert!(matches!(
        gates::offered(&own).iter().find(|(e, _)| *e == Exit::ApprovePullRequest),
        Some((_, Offer::Denied(reason))) if reason.contains("Azure DevOps")
    ));

    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    pull(&wire, &[]);
    let observation = session.action_observation(&repository(), 7).unwrap();
    let refused = session.merge_exact(&observation, "request").unwrap_err();
    assert!(!refused.uncertain);
    assert_eq!(refused.failure.message, UNSUPPORTED);
    assert!(writes(&wire).is_empty());
}

/// Queues one readiness read whose reviewers hold `votes`.
fn pull(wire: &FakeWire, votes: &[(&str, i64)]) {
    let reviewers: Vec<_> = votes
        .iter()
        .map(|(id, vote)| json!({"id": id, "displayName": "Someone", "vote": vote}))
        .collect();
    wire.json(
        &git("pullrequests/7?"),
        json!({
            "pullRequestId": 7, "status": "active", "title": "Change 7",
            "createdBy": author(BLAKE), "creationDate": "2026-10-20T10:00:00Z",
            "isDraft": false, "repository": {"id": REPO, "project": {"id": PROJECT}},
            "lastMergeSourceCommit": {"commitId": sha(2)},
            "lastMergeTargetCommit": {"commitId": sha(900)},
            "mergeStatus": "succeeded", "reviewers": reviewers
        }),
    );
    iterations(wire);
    wire.json(
        &format!("{ORG}/{PROJECT}/_apis/policy/evaluations?"),
        json!({"value": []}),
    )
    .json(&git("pullRequests/7/statuses?"), json!({"value": []}))
    .json(&threads_url(), json!({"value": []}));
}

#[test]
fn an_uncertain_vote_reconciles_from_the_observed_reviewers_once() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    let session = session(&wire, &runtime);
    pull(&wire, &[]);
    let before = session.action_observation(&repository(), 7).unwrap();
    assert_eq!(before.permission, "WRITE");
    let job = review().job;
    let mut effect = Effect {
        cancelled: false,
        reconcile_attempts: 0,
        reconcile_requested: false,
        id: "effect".into(),
        final_id: "final".into(),
        item_id: "item".into(),
        action: Action::Approve,
        operation: JobOperation::review(&job, 100),
        observation: before.clone(),
        body: actions::packet::approval_body("effect", &ProviderId::AzureDevops),
        state: EffectState::Uncertain,
        error: None,
        receipt: None,
    };
    assert!(effect.body.is_empty());

    // Not visible yet: stays uncertain, and nothing is sent again.
    actions::reconcile(&mut effect, &before).unwrap();
    assert_eq!(effect.state, EffectState::Uncertain);

    pull(&wire, &[(CASEY, 0), (AVERY, 10)]);
    let after = session.action_observation(&repository(), 7).unwrap();
    assert!(Facts::from_observation(&after).already_approved);
    actions::reconcile(&mut effect, &after).unwrap();
    assert_eq!(effect.state, EffectState::Confirmed);
    let receipt = effect.receipt.as_ref().unwrap();
    assert_eq!(receipt.id, format!("7:{AVERY}"));
    assert_eq!(receipt.actor_id, AVERY);
    assert!(writes(&wire).is_empty());
}
