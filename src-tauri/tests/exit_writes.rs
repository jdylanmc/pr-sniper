#[path = "support/fake_github.rs"]
mod fake_github;

use fake_github::{self as fake, foreign_thread, owned_thread, Fault, Wire, ASSIGNMENT, HEAD};
use pr_sniper_lib::{
    exit_writes::{self, Dependency, Environment, ExitWrite, Outcome, Phase, StepState},
    github::{
        provider::GithubClient,
        threads::{Ownership, Provenance, Thread},
    },
    publication::WriteFailure,
    review::Failure,
};
use serde_json::{json, Value};

struct Env {
    wire: Wire,
    proofs: Vec<Ownership>,
    dependency: Dependency,
    now: i64,
    /// Every saved snapshot, newest last: a restart resumes from one of these.
    saved: Vec<ExitWrite>,
    confirmed: usize,
    fail_confirm: bool,
    fail_resolve_save: bool,
    known: Vec<(String, u64)>,
    gate: Option<String>,
    pr_author: Option<String>,
    /// Applied right after a reply lands: a later human comment or author change.
    after_reply: Option<(
        Option<pr_sniper_lib::github::threads::Comment>,
        Option<String>,
    )>,
    /// Applied when a resolve intent is saved (before the final read) or as the
    /// resolve mutation lands (before the confirming read).
    on_attempted: Option<pr_sniper_lib::github::threads::Comment>,
    during_resolve: Option<pr_sniper_lib::github::threads::Comment>,
}

impl Env {
    fn new(wire: &Wire) -> Self {
        Self {
            wire: wire.clone(),
            proofs: vec![fake::origin().ownership().unwrap()],
            dependency: Dependency::Confirmed,
            now: 100,
            saved: vec![],
            confirmed: 0,
            fail_confirm: false,
            fail_resolve_save: false,
            known: vec![(HEAD.into(), 1)],
            gate: None,
            pr_author: None,
            after_reply: None,
            on_attempted: None,
            during_resolve: None,
        }
    }
    fn client(&self) -> GithubClient<Wire> {
        GithubClient::new(self.wire.clone())
    }
    fn last(&self) -> ExitWrite {
        self.saved.last().unwrap().clone()
    }
}

impl Environment for Env {
    fn now(&self) -> Result<i64, Failure> {
        Ok(self.now)
    }
    fn save(&mut self, write: &mut ExitWrite) -> Result<(), Failure> {
        if self.fail_resolve_save
            && write.steps.iter().any(|step| {
                step.kind == exit_writes::StepKind::Resolve && step.state == StepState::Done
            })
        {
            return Err(Failure::permanent(
                "Injected interruption before resolution receipt save.",
            ));
        }
        if write.steps.iter().any(|step| {
            step.kind == exit_writes::StepKind::Resolve && step.state == StepState::Attempted
        }) {
            if let Some(comment) = self.on_attempted.take() {
                self.wire.0.lock().unwrap().threads[0]
                    .comments
                    .push(comment);
            }
        }
        self.saved.push(write.clone());
        Ok(())
    }
    fn gate(&mut self, _: &ExitWrite) -> Result<(), Failure> {
        self.gate
            .as_ref()
            .map_or(Ok(()), |reason| Err(Failure::permanent(reason.clone())))
    }
    fn dependency(&mut self, _: &ExitWrite) -> Result<Dependency, Failure> {
        Ok(self.dependency.clone())
    }
    fn head(&mut self, _: &ExitWrite) -> Result<String, Failure> {
        Ok(self.wire.0.lock().unwrap().head.clone())
    }
    fn proofs(&mut self, _: &ExitWrite) -> Result<Vec<Ownership>, Failure> {
        Ok(self.proofs.clone())
    }
    fn thread(&mut self, _: &ExitWrite, id: &str) -> Result<Option<Thread>, Failure> {
        Ok(self.client().review_thread("100", "9", id)?)
    }
    fn pr_author(&mut self, _: &ExitWrite) -> Result<Option<String>, Failure> {
        Ok(self.pr_author.clone())
    }
    fn revision(&mut self, _: &ExitWrite, commit: Option<&str>) -> String {
        exit_writes::targeted_revision(commit, &self.known)
    }
    fn reply(&mut self, w: &ExitWrite, root: &str, body: &str) -> Result<String, WriteFailure> {
        let posted =
            self.client()
                .reply_to_review_thread("example/repo", w.job.number, "22", root, body);
        if let Some((comment, author)) = self.after_reply.take() {
            if let Some(comment) = comment {
                self.wire.0.lock().unwrap().threads[0]
                    .comments
                    .push(comment);
            }
            if author.is_some() {
                self.pr_author = author;
            }
        }
        posted
    }
    fn resolve(&mut self, _: &ExitWrite, thread: &str, request: &str) -> Result<(), WriteFailure> {
        let result = self.client().resolve_review_thread(thread, request);
        if let Some(comment) = self.during_resolve.take() {
            self.wire.0.lock().unwrap().threads[0]
                .comments
                .push(comment);
        }
        result
    }
    fn unresolve(
        &mut self,
        _: &ExitWrite,
        thread: &str,
        request: &str,
    ) -> Result<(), WriteFailure> {
        self.client().unresolve_review_thread(thread, request)
    }
    fn confirmed(&mut self, _: &ExitWrite) -> Result<(), Failure> {
        if self.fail_confirm {
            return Err(Failure::permanent("Injected consume failure."));
        }
        self.confirmed += 1;
        Ok(())
    }
}

fn write(replies: Value, resolutions: Value, new_comments: Value) -> ExitWrite {
    let mut review = fake::review(
        new_comments,
        replies,
        resolutions.clone(),
        &fake::settings(),
    );
    review.result.as_mut().unwrap().finding_dispositions = resolutions
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            serde_json::from_value(json!({
                "thread_id":id,"disposition":"fixed","head_sha":HEAD,"base_sha":"b".repeat(40),
                "rationale":"Current head removes the invalid branch.",
                "evidence":[{"path":"src/lib.rs","side":"head","line":1}]
            }))
            .unwrap()
        })
        .collect();
    exit_writes::plan(&review, 100).expect("replies or resolutions planned")
}

#[test]
fn resolve_without_current_code_proof_is_denied_before_any_effect() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let review = fake::review(json!([]), json!([]), json!(["T1"]), &fake::settings());
    let mut w = exit_writes::plan(&review, 100).unwrap();
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
    assert!(!wire.thread("T1").resolved);
}

#[test]
fn resumed_verified_write_cannot_resolve_without_proof_or_on_a_different_head() {
    for moved in [false, true] {
        let wire = wire();
        let mut env = Env::new(&wire);
        let mut w = write(json!([]), json!(["T1"]), json!([]));
        w.phase = Phase::Verified;
        w.steps[0].root_id = Some("100".into());
        if moved {
            wire.0.lock().unwrap().head = "other".into();
        } else {
            w.steps[0].resolution_evidence = None;
        }

        drive(&mut env, &mut w);
        assert_eq!(w.phase, Phase::Tossed);
        assert!(wire.log().is_empty());
    }
}

#[test]
fn another_assignments_publication_and_a_different_evidence_base_cannot_resolve() {
    for foreign in [true, false] {
        let wire = wire();
        let mut env = Env::new(&wire);
        let mut w = write(json!([]), json!(["T1"]), json!([]));
        if foreign {
            env.proofs[0].assignment_id = "other-assignment".into();
        } else {
            w.steps[0].resolution_evidence.as_mut().unwrap().base_sha = "old-base".into();
        }
        drive(&mut env, &mut w);
        assert_eq!(w.phase, Phase::Tossed);
        assert!(wire.log().is_empty());
    }
}

fn wire() -> Wire {
    Wire::new(vec![
        owned_thread("T1", "100"),
        foreign_thread("T2", "300"),
        owned_thread("T3", "200"),
    ])
}

fn reply(thread: &str, body: &str) -> Value {
    json!({"thread_id":thread,"body":body})
}

/// Drive `run` until nothing more is due, advancing the clock past each retry time.
fn drive(env: &mut Env, write: &mut ExitWrite) {
    for _ in 0..12 {
        let _ = exit_writes::run(env, write);
        if write.terminal() {
            return;
        }
        env.now = write
            .operation
            .next_attempt_at
            .unwrap_or(env.now)
            .max(env.now)
            + 1;
    }
}

#[test]
fn replies_then_resolutions_run_in_order_and_confirm_once() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = write(
        json!([reply("T1", "Fixed."), reply("T2", "Disagree.")]),
        json!(["T1"]),
        json!([]),
    );
    assert_eq!(
        exit_writes::run(&mut env, &mut w).unwrap(),
        Outcome::Progressed
    );
    assert_eq!(w.phase, Phase::Completed);
    assert!(w.consumed);
    assert_eq!(env.confirmed, 1);
    assert_eq!(wire.log(), ["reply:100", "reply:300", "resolve:T1"]);
    assert!(wire.thread("T1").resolved);
    assert!(!wire.thread("T2").resolved);
    let thread = wire.thread("T1");
    let posted = &thread.comments.last().unwrap().body;
    assert!(posted.starts_with("Fixed."));
    assert!(posted.contains(&format!("<!-- pr-sniper:reply:exit-{}-0 -->", w.id)));
    assert!(posted.contains("<!-- pr-sniper:revision:1 -->"));
    assert!(posted.ends_with("PR Sniper"));
    assert!(w.steps.iter().all(|s| s.state == StepState::Done));
}

#[test]
fn replies_go_to_any_thread_but_resolution_only_to_owned() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = write(json!([reply("T2", "Because.")]), json!(["T2"]), json!([]));
    let _ = exit_writes::run(&mut env, &mut w);
    // Ownership refusal tosses the whole exit: the reply to T2 is not posted either.
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
    assert!(w.error.as_deref().unwrap().contains("own comments"));
}

#[test]
fn tampered_or_unverifiable_roots_are_not_owned() {
    let wire = wire();
    wire.0.lock().unwrap().threads[0].comments[0].body = "Edited root".into();
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());

    // A thread whose root belongs to the account but a different review is not owned.
    let wire = Wire::new(vec![{
        let mut t = owned_thread("T1", "100");
        t.comments[0].review_id = Some("999".into());
        t
    }]);
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);

    // Without any saved proof nothing is owned.
    let wire = self::wire();
    let mut env = Env::new(&wire);
    env.proofs.clear();
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
}

#[test]
fn preflight_failures_toss_before_any_effect() {
    for (label, mutate) in [
        (
            "unknown thread",
            Box::new(|_: &Wire| ()) as Box<dyn Fn(&Wire)>,
        ),
        (
            "cannot reply",
            Box::new(|w: &Wire| w.0.lock().unwrap().threads[0].can_reply = false),
        ),
        (
            "head moved",
            Box::new(|w: &Wire| w.0.lock().unwrap().head = "c".repeat(40)),
        ),
    ] {
        let wire = wire();
        mutate(&wire);
        let mut env = Env::new(&wire);
        let target = if label == "unknown thread" {
            "missing"
        } else {
            "T1"
        };
        let mut w = write(
            json!([reply("T2", "ok"), reply(target, "second")]),
            json!([]),
            json!([]),
        );
        let _ = exit_writes::run(&mut env, &mut w);
        assert_eq!(w.phase, Phase::Tossed, "{label}");
        assert!(wire.log().is_empty(), "{label}: nothing partial");
        assert!(!w.effects(), "{label}");
    }
}

#[test]
fn oversized_reply_is_refused_not_truncated() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = write(
        json!([reply("T2", &"x".repeat(65_530))]),
        json!([]),
        json!([]),
    );
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
}

#[test]
fn authorization_loss_refuses_without_writing() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.gate = Some("Reply Comment permission is not granted.".into());
    let mut w = write(json!([reply("T2", "ok")]), json!([]), json!([]));
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
}

#[test]
fn publication_goes_first_and_preflight_precedes_it() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.dependency = Dependency::Pending;
    let mut w = write(
        json!([reply("T2", "ok")]),
        json!(["T1"]),
        json!([{"path":"a.rs","line":1,"body":"new"}]),
    );
    assert!(w.comments_first);
    assert_eq!(
        exit_writes::run(&mut env, &mut w).unwrap(),
        Outcome::Waiting
    );
    assert_eq!(
        w.phase,
        Phase::Verified,
        "verified before publication may start"
    );
    assert!(
        wire.log().is_empty(),
        "no reply or resolution before the batch"
    );
    env.dependency = Dependency::Confirmed;
    env.now = w.operation.next_attempt_at.unwrap();
    exit_writes::run(&mut env, &mut w).unwrap();
    assert_eq!(w.phase, Phase::Completed);
    assert!(w.published);
    assert_eq!(wire.log(), ["reply:300", "resolve:T1"]);
}

#[test]
fn a_failed_publication_tosses_the_whole_exit() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.dependency = Dependency::Failed("batch discarded".into());
    let mut w = write(
        json!([reply("T2", "ok")]),
        json!([]),
        json!([{"path":"a.rs","line":1,"body":"n"}]),
    );
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
}

#[test]
fn a_reply_that_landed_but_errored_reconciles_without_a_duplicate() {
    let wire = wire();
    wire.fault("reply", Fault::After);
    let mut env = Env::new(&wire);
    let mut w = write(json!([reply("T2", "ok")]), json!([]), json!([]));
    exit_writes::run(&mut env, &mut w).unwrap();
    assert_eq!(
        w.phase,
        Phase::Completed,
        "landed reply is found by its marker"
    );
    assert_eq!(wire.log(), ["reply:300"]);
    assert_eq!(wire.thread("T2").comments.len(), 2);
    assert!(w.steps[0].receipt.is_some());
}

#[test]
fn an_unconfirmed_reply_is_never_replaced() {
    let wire = wire();
    wire.fault("reply", Fault::Before);
    let mut env = Env::new(&wire);
    let mut w = write(json!([reply("T2", "ok")]), json!([]), json!([]));
    drive(&mut env, &mut w);
    assert_eq!(
        wire.log(),
        ["reply:300"],
        "one POST only, however many retries"
    );
    assert_eq!(w.phase, Phase::Failed);
    assert!(w.uncertain);
    assert_eq!(wire.thread("T2").comments.len(), 1);
}

#[test]
fn restart_mid_sequence_resumes_without_duplicates() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = write(
        json!([reply("T1", "one"), reply("T2", "two")]),
        json!(["T3"]),
        json!([]),
    );
    // Crash right after the first reply landed: resume from that saved snapshot.
    wire.fault("reply", Fault::After);
    let _ = exit_writes::run(&mut env, &mut w);
    let snapshot = env
        .saved
        .iter()
        .find(|s| s.steps[0].state == StepState::Done && s.steps[1].state == StepState::Planned)
        .expect("first reply saved as done")
        .clone();
    let mut resumed = Env::new(&wire);
    resumed.now = 200;
    let mut again = snapshot;
    again.operation.next_attempt_at = Some(0);
    drive(&mut resumed, &mut again);
    assert_eq!(again.phase, Phase::Completed);
    let replies: Vec<_> = wire
        .log()
        .into_iter()
        .filter(|l| l.starts_with("reply"))
        .collect();
    assert_eq!(
        replies,
        ["reply:100", "reply:300"],
        "each reply posted exactly once"
    );
    assert_eq!(
        wire.log()
            .iter()
            .filter(|l| l.starts_with("resolve"))
            .count(),
        1
    );

    // A restart after completion changes nothing.
    let before = wire.log();
    let mut done = resumed.last();
    let mut third = Env::new(&wire);
    exit_writes::run(&mut third, &mut done).unwrap();
    assert_eq!(wire.log(), before);
}

#[test]
fn crash_after_intent_before_effect_does_not_double_post() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = write(json!([reply("T2", "ok")]), json!([]), json!([]));
    wire.fault("reply", Fault::Before);
    let _ = exit_writes::run(&mut env, &mut w);
    let attempted = env
        .saved
        .iter()
        .find(|s| s.steps[0].state == StepState::Attempted)
        .expect("intent saved before the effect")
        .clone();
    assert!(attempted.uncertain);
    // The process died; the restart sees an attempted step with no marker on the thread.
    let mut resumed = Env::new(&wire);
    resumed.now = 10_000;
    let mut again = attempted;
    again.operation.next_attempt_at = Some(0);
    let _ = exit_writes::run(&mut resumed, &mut again);
    assert_eq!(
        wire.log(),
        ["reply:300"],
        "unresolved intent never re-posts"
    );
}

#[test]
fn a_permanent_failure_after_an_effect_stops_without_more_writes() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = write(
        json!([reply("T1", "one"), reply("T2", "two")]),
        json!([]),
        json!([]),
    );
    // The second reply is refused with a definite 4xx after the first landed.
    let _ = exit_writes::run(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Completed);
    let wire = self::wire();
    let mut env = Env::new(&wire);
    let mut w = write(json!([reply("T1", "one")]), json!(["T3"]), json!([]));
    wire.fault("resolve", Fault::Reject);
    drive(&mut env, &mut w);
    assert_eq!(
        w.phase,
        Phase::Failed,
        "reply already posted, so not a clean toss"
    );
    assert!(w.effects());
    assert_eq!(
        wire.log().iter().filter(|l| l.starts_with("reply")).count(),
        1,
        "the posted reply is never repeated"
    );
    assert_eq!(w.steps[0].state, StepState::Done);
    assert_ne!(w.steps[1].state, StepState::Done);
}

#[test]
fn a_definite_reply_rejection_before_any_effect_is_a_clean_toss() {
    let wire = wire();
    wire.fault("reply", Fault::Reject);
    let mut env = Env::new(&wire);
    let mut w = write(json!([reply("T2", "ok")]), json!([]), json!([]));
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert_eq!(w.steps[0].state, StepState::Planned);
    assert_eq!(wire.thread("T2").comments.len(), 1);
}

#[test]
fn resolution_that_landed_but_errored_is_confirmed_by_reading_the_thread() {
    let wire = wire();
    wire.fault("resolve", Fault::After);
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    exit_writes::run(&mut env, &mut w).unwrap();
    assert_eq!(w.phase, Phase::Completed);
    assert_eq!(wire.log(), ["resolve:T1"]);
}

#[test]
fn an_unconfirmed_resolution_retries_the_idempotent_mutation() {
    let wire = wire();
    wire.fault("resolve", Fault::Before);
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Completed);
    assert_eq!(wire.log(), ["resolve:T1", "resolve:T1"]);
    assert!(wire.thread("T1").resolved);
}

#[test]
fn an_already_resolved_own_thread_is_done_without_a_mutation() {
    let wire = wire();
    wire.0.lock().unwrap().threads[0].resolved = true;
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1", "T1"]), json!([]));
    assert_eq!(w.steps.len(), 1, "duplicate resolutions collapse");
    exit_writes::run(&mut env, &mut w).unwrap();
    assert_eq!(w.phase, Phase::Completed);
    assert!(wire.log().is_empty());
    assert_eq!(w.steps[0].receipt.as_deref(), Some("already"));
}

#[test]
fn reconciled_landed_resolution_preserves_attempt_provenance_after_store_restart() {
    let wire = wire();
    wire.0.lock().unwrap().threads[0].resolved = true;
    let mut env = Env::new(&wire);
    let mut prior = write(
        json!([reply(
            "T1",
            "Correction: my earlier resolution was mistaken.\n\nThe bug still applies."
        )]),
        json!([]),
        json!([]),
    );
    prior.id = "prior-correction".into();
    drive(&mut env, &mut prior);
    assert_eq!(prior.phase, Phase::Completed);
    wire.0.lock().unwrap().threads[0].resolved = false;
    let mut later = write(json!([]), json!(["T1"]), json!([]));
    later.id = "later-fixed-resolution".into();
    env.now = 200;
    env.fail_resolve_save = true;
    assert!(exit_writes::run(&mut env, &mut later).is_err());
    assert!(
        wire.thread("T1").resolved,
        "Provider accepted before the interruption"
    );
    let saved = env.last();
    assert_eq!(saved.steps[0].state, StepState::Attempted);
    let root = tempfile::tempdir().unwrap();
    let store = pr_sniper_lib::storage::Store::new(root.path().into());
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes = vec![prior, saved];
    store.save_feedback(&ledger).unwrap();
    drop(store);
    let restored = pr_sniper_lib::storage::Store::new(root.path().into());
    let mut ledger = restored.load_feedback().unwrap();
    let resumed = &mut ledger.exit_writes[1];
    let mut after = Env::new(&wire);
    after.now = 300;
    drive(&mut after, resumed);
    assert_eq!(resumed.phase, Phase::Completed);
    assert_ne!(
        resumed.steps[0].receipt.as_deref(),
        Some("already"),
        "A landed attempted mutation starts a new correction episode, not a no-op"
    );
    assert!(resumed.steps[0].receipt.is_some());
    restored.save_feedback(&ledger).unwrap();
    assert_eq!(wire.log(), ["reply:100", "resolve:T1"]);
}

#[test]
fn uncertain_resolution_keeps_attempt_provenance_across_a_retry() {
    let wire = wire();
    wire.fault("resolve", Fault::Before);
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    assert!(exit_writes::run(&mut env, &mut w).is_err());
    assert!(w.uncertain);
    assert_eq!(
        w.steps[0].state,
        StepState::Attempted,
        "An uncertain mutation cannot become an unattempted no-op on a later retry"
    );
    let mut restored: ExitWrite =
        serde_json::from_value(serde_json::to_value(&w).unwrap()).unwrap();
    // The earlier request can land after the first reconciliation read.
    wire.0.lock().unwrap().threads[0].resolved = true;
    env.now = restored.operation.next_attempt_at.unwrap_or(300).max(300);
    drive(&mut env, &mut restored);
    assert_eq!(restored.phase, Phase::Completed);
    assert_ne!(restored.steps[0].receipt.as_deref(), Some("already"));
    assert_eq!(wire.log(), ["resolve:T1"]);
}

#[test]
fn a_definite_retry_rejection_cannot_erase_an_earlier_uncertain_resolution() {
    let wire = wire();
    wire.fault("resolve", Fault::Before);
    let mut env = Env::new(&wire);
    let mut w = write(json!([]), json!(["T1"]), json!([]));
    assert!(exit_writes::run(&mut env, &mut w).is_err());
    let mut restored: ExitWrite =
        serde_json::from_value(serde_json::to_value(&w).unwrap()).unwrap();
    wire.fault("resolve", Fault::Reject);
    env.now = restored.operation.next_attempt_at.unwrap_or(300).max(300);
    assert!(exit_writes::run(&mut env, &mut restored).is_err());
    assert!(restored.uncertain, "The earlier attempt might still land");
    assert_eq!(restored.steps[0].state, StepState::Attempted);
    assert_eq!(wire.log(), ["resolve:T1", "resolve:T1"]);
}

#[test]
fn the_consume_hook_runs_once_and_retries_without_rewriting() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.fail_confirm = true;
    let mut w = write(json!([reply("T2", "ok")]), json!([]), json!([]));
    assert!(exit_writes::run(&mut env, &mut w).is_err());
    assert_eq!(w.phase, Phase::Completed);
    assert!(!w.consumed);
    env.fail_confirm = false;
    exit_writes::run(&mut env, &mut w).unwrap();
    assert!(w.consumed);
    assert_eq!(env.confirmed, 1);
    assert_eq!(wire.log(), ["reply:300"]);
    exit_writes::run(&mut env, &mut w).unwrap();
    assert_eq!(env.confirmed, 1);
}

#[test]
fn resolve_mutation_confirms_the_exact_thread_and_classifies_failures() {
    let wire = wire();
    let client = GithubClient::new(wire.clone());
    assert!(client.resolve_review_thread("T1", "req-1").is_ok());
    assert!(wire.thread("T1").resolved);

    // The provider echoing another thread or an unresolved one is not confirmation.
    wire.0.lock().unwrap().resolve_override = Some(json!({"data":{"resolveReviewThread":
        {"clientMutationId":"req-2","thread":{"id":"OTHER","isResolved":true}}}}));
    let error = client.resolve_review_thread("T3", "req-2").unwrap_err();
    assert!(error.uncertain);
    wire.0.lock().unwrap().resolve_override = Some(json!({"data":{"resolveReviewThread":
        {"clientMutationId":"req-3","thread":{"id":"T3","isResolved":false}}}}));
    assert!(
        client
            .resolve_review_thread("T3", "req-3")
            .unwrap_err()
            .uncertain
    );

    wire.fault("resolve", Fault::Reject);
    assert!(
        !client
            .resolve_review_thread("T3", "req-4")
            .unwrap_err()
            .uncertain
    );
    wire.fault("resolve", Fault::ServerError);
    assert!(
        client
            .resolve_review_thread("T3", "req-5")
            .unwrap_err()
            .uncertain
    );
    wire.fault("resolve", Fault::Before);
    assert!(
        client
            .resolve_review_thread("T3", "req-6")
            .unwrap_err()
            .uncertain
    );
    assert!(
        !client
            .resolve_review_thread("", "req-7")
            .unwrap_err()
            .uncertain
    );
}

#[test]
fn plan_ignores_exits_without_replies_or_resolutions() {
    let none = fake::review(
        json!([{"path":"a.rs","line":1,"body":"n"}]),
        json!([]),
        json!([]),
        &fake::settings(),
    );
    assert!(exit_writes::plan(&none, 100).is_none());
    let mut running = fake::review(
        json!([]),
        json!([reply("T1", "x")]),
        json!([]),
        &fake::settings(),
    );
    running.operation.state = pr_sniper_lib::monitoring::OperationState::Running;
    assert!(exit_writes::plan(&running, 100).is_none());
}

#[test]
fn targeted_revision_maps_the_comment_commit_to_a_local_iteration() {
    let known = vec![("aaa".to_string(), 1), ("bbb".to_string(), 2)];
    assert_eq!(exit_writes::targeted_revision(Some("bbb"), &known), "2");
    assert_eq!(
        exit_writes::targeted_revision(Some("ccc"), &known),
        "unknown"
    );
    assert_eq!(exit_writes::targeted_revision(None, &known), "unknown");
    let clash = vec![("aaa".to_string(), 1), ("aaa".to_string(), 3)];
    assert_eq!(
        exit_writes::targeted_revision(Some("aaa"), &clash),
        "unknown"
    );
    let same = vec![("aaa".to_string(), 2), ("aaa".to_string(), 2)];
    assert_eq!(exit_writes::targeted_revision(Some("aaa"), &same), "2");
}

#[test]
fn ledger_admits_once_holds_publication_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = pr_sniper_lib::storage::Store::new(dir.path().into());
    let review = fake::review(
        json!([{"path":"a.rs","line":1,"body":"n"}]),
        json!([reply("T2", "ok")]),
        json!([]),
        &fake::settings(),
    );
    store.save_reviews(std::slice::from_ref(&review)).unwrap();
    assert!(
        exit_writes::holds_publication(&store, &review)
            .unwrap()
            .is_some(),
        "held before the ledger has the exit"
    );
    assert_eq!(exit_writes::admit(&store, 100).unwrap(), 1);
    assert_eq!(exit_writes::admit(&store, 100).unwrap(), 0, "idempotent");
    let saved = exit_writes::for_review(&store, &review.operation.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.phase, Phase::Planned);
    assert!(exit_writes::holds_publication(&store, &review)
        .unwrap()
        .is_some());
    assert_eq!(
        exit_writes::next_due(&store, 100).unwrap(),
        Some(saved.id.clone())
    );

    // Restart: a fresh store handle sees the same single record.
    let again = pr_sniper_lib::storage::Store::new(dir.path().into());
    assert_eq!(exit_writes::admit(&again, 200).unwrap(), 0);

    let mut ledger = again.load_feedback().unwrap();
    ledger.exit_writes[0].phase = Phase::Verified;
    again.save_feedback(&ledger).unwrap();
    assert!(exit_writes::holds_publication(&again, &review)
        .unwrap()
        .is_none());
    ledger.exit_writes[0].phase = Phase::Tossed;
    again.save_feedback(&ledger).unwrap();
    assert!(exit_writes::holds_publication(&again, &review)
        .unwrap()
        .is_some());

    // Reply-only exits have no batch to hold.
    let reply_only = fake::review(
        json!([]),
        json!([reply("T2", "ok")]),
        json!([]),
        &fake::settings(),
    );
    assert!(exit_writes::holds_publication(&again, &reply_only)
        .unwrap()
        .is_none());
    assert_eq!(ASSIGNMENT.len(), 36);
}

fn batch_write() -> ExitWrite {
    write(
        json!([reply("T2", "ok")]),
        json!([]),
        json!([{"path":"a.rs","line":1,"body":"n"}]),
    )
}

#[test]
fn a_parked_or_terminal_publication_fails_the_dependency() {
    use pr_sniper_lib::{
        monitoring::OperationState,
        publication::{Phase as P, Publication},
    };
    let review = fake::review(
        json!([{"path":"a.rs","line":1,"body":"n"}]),
        json!([reply("T2", "ok")]),
        json!([]),
        &fake::settings(),
    );
    let id = review.operation.id.clone();
    let publication = Publication::new(review, true, true, 100).unwrap();
    let with = |phase: P, state: OperationState| {
        let mut p = publication.clone();
        p.phase = phase;
        p.operation.state = state;
        exit_writes::publication_dependency(&[p], &id)
    };
    assert_eq!(
        with(P::Pending, OperationState::Queued),
        Dependency::Pending
    );
    for parked in [OperationState::ManualRetry, OperationState::Failed] {
        for phase in [P::Preparing, P::Pending, P::Unresolved] {
            let label = format!("{phase:?} parked as {parked:?}");
            assert!(
                matches!(with(phase, parked.clone()), Dependency::Failed(_)),
                "{label}"
            );
        }
    }
    for phase in [P::Stopped, P::StaleAfterPublication] {
        assert!(matches!(
            with(phase, OperationState::Completed),
            Dependency::Failed(_)
        ));
    }
}

#[test]
fn waiting_on_the_batch_backs_off_and_tosses_when_the_budget_runs_out() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.dependency = Dependency::Pending;
    let mut w = batch_write();
    assert_eq!(
        exit_writes::run(&mut env, &mut w).unwrap(),
        Outcome::Waiting
    );
    assert!(
        w.operation.next_attempt_at.unwrap() > env.now,
        "a waiting write is not due again at once"
    );
    for _ in 0..200 {
        if w.terminal() {
            break;
        }
        env.now = w.operation.next_attempt_at.unwrap_or(env.now).max(env.now) + 1;
        let _ = exit_writes::run(&mut env, &mut w);
    }
    assert_eq!(
        w.phase,
        Phase::Tossed,
        "a batch that never lands tosses the exit"
    );
    assert!(wire.log().is_empty());
}

#[test]
fn a_write_waiting_on_its_batch_does_not_block_other_writes() {
    use pr_sniper_lib::storage::Store;
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let wire = wire();
    let mut env = Env::new(&wire);
    env.dependency = Dependency::Pending;
    let mut waiting = batch_write();
    exit_writes::run(&mut env, &mut waiting).unwrap();
    let mut other = write(json!([reply("T1", "Fixed.")]), json!([]), json!([]));
    other.review_operation_id = "other".into();
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes = vec![waiting.clone(), other.clone()];
    store.save_feedback(&ledger).unwrap();
    assert_eq!(
        exit_writes::next_due(&store, env.now).unwrap().as_deref(),
        Some(other.id.as_str())
    );
    // Among due writes the oldest retry time goes first.
    let mut ledger = store.load_feedback().unwrap();
    ledger.exit_writes[0].operation.next_attempt_at = Some(50);
    ledger.exit_writes[1].operation.next_attempt_at = Some(40);
    store.save_feedback(&ledger).unwrap();
    assert_eq!(
        exit_writes::next_due(&store, 100).unwrap().as_deref(),
        Some(other.id.as_str())
    );
}

fn hash(body: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(body.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// T1's author reply "101" (account 11, the PR author) declines; the primary accepted it.
fn won_t_fix_write(cited: &str, anchored_author: &str) -> ExitWrite {
    let mut review = fake::review(
        json!([]),
        json!([reply("T1", "Accepted: intended product behavior.")]),
        json!(["T1"]),
        &fake::settings(),
    );
    review.result.as_mut().unwrap().finding_dispositions = vec![serde_json::from_value(json!({
        "thread_id":"T1","disposition":"author_wont_fix","head_sha":HEAD,"base_sha":"b".repeat(40),
        "rationale":"Accepted: intended product behavior.","evidence":[],
        "author_comment_id":cited,
        "anchor":{"base_sha":"b".repeat(40),"blobs":{},"manifests":"m",
            "comments":{"100":hash("Root finding"),"101":hash("Why?")},"author_id":anchored_author}
    }))
    .unwrap()];
    exit_writes::plan(&review, 100).unwrap()
}

#[test]
fn an_accepted_author_won_t_fix_replies_then_resolves_after_rechecking_the_author() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.pr_author = Some("11".into());
    let mut w = won_t_fix_write("101", "11");
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Completed);
    assert_eq!(wire.log(), ["reply:100", "resolve:T1"]);
    assert!(wire.thread("T1").resolved);
}

#[test]
fn an_author_won_t_fix_is_refused_unless_the_provider_still_shows_the_authors_latest_word() {
    for case in [
        "unknown",
        "changed-author",
        "root",
        "later-human",
        "later-author",
    ] {
        let wire = wire();
        let mut env = Env::new(&wire);
        env.pr_author = (case != "unknown").then(|| "11".into());
        let (cited, anchored) = match case {
            "changed-author" => ("101", "99"),
            "root" => ("100", "11"),
            _ => ("101", "11"),
        };
        if case.starts_with("later") {
            wire.0.lock().unwrap().threads[0]
                .comments
                .push(fake::comment(
                    "150",
                    "I still think this loses data.",
                    if case == "later-human" { "33" } else { "11" },
                    Some("100"),
                    Some("43"),
                ));
        }
        let mut w = won_t_fix_write(cited, anchored);
        drive(&mut env, &mut w);
        assert_eq!(w.phase, Phase::Tossed, "{case}");
        assert!(wire.log().is_empty(), "{case}");
        assert!(!wire.thread("T1").resolved, "{case}");
    }
}

#[test]
fn an_author_won_t_fix_is_rechecked_after_the_audit_reply_right_before_resolving() {
    let edited = {
        let mut c = fake::comment("101", "Why?", "11", Some("100"), Some("43"));
        c.id = "160".into();
        c.body = "Actually this loses data; please fix.".into();
        c.author_id = Some("33".into());
        c
    };
    for (case, comment, author) in [
        ("later human comment", Some(edited), None),
        ("PR author changed", None, Some("44".to_string())),
    ] {
        let wire = wire();
        let mut env = Env::new(&wire);
        env.pr_author = Some("11".into());
        env.after_reply = Some((comment, author));
        let mut w = won_t_fix_write("101", "11");
        drive(&mut env, &mut w);
        assert_eq!(wire.log(), ["reply:100"], "{case}");
        assert!(!wire.thread("T1").resolved, "{case}");
        assert_ne!(w.phase, Phase::Completed, "{case}");
    }
}

#[test]
fn an_edited_author_reply_is_not_resolved() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.pr_author = Some("11".into());
    wire.0.lock().unwrap().threads[0].comments[1].body = "Fine, I will fix it.".into();
    let mut w = won_t_fix_write("101", "11");
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
}

fn human_concern() -> pr_sniper_lib::github::threads::Comment {
    fake::comment(
        "170",
        "This still loses data.",
        "33",
        Some("100"),
        Some("43"),
    )
}

#[test]
fn a_change_after_the_resolve_intent_is_saved_stops_before_the_mutation() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.pr_author = Some("11".into());
    env.on_attempted = Some(human_concern());
    let mut w = won_t_fix_write("101", "11");
    drive(&mut env, &mut w);
    // The final read follows the persisted intent: no resolve request was sent.
    assert_eq!(wire.log(), ["reply:100"]);
    assert!(!wire.thread("T1").resolved);
    assert_ne!(w.phase, Phase::Completed);
    let resolve = w
        .steps
        .iter()
        .find(|s| s.kind == exit_writes::StepKind::Resolve)
        .unwrap();
    assert_ne!(resolve.state, StepState::Done);
}

#[test]
fn a_change_while_the_resolve_lands_is_undone_and_not_completed() {
    let wire = wire();
    let mut env = Env::new(&wire);
    env.pr_author = Some("11".into());
    env.during_resolve = Some(human_concern());
    let mut w = won_t_fix_write("101", "11");
    drive(&mut env, &mut w);
    assert_eq!(wire.log(), ["reply:100", "resolve:T1", "unresolve:T1"]);
    assert!(!wire.thread("T1").resolved);
    assert_ne!(w.phase, Phase::Completed);
    let resolve = w
        .steps
        .iter()
        .find(|s| s.kind == exit_writes::StepKind::Resolve)
        .unwrap();
    assert_eq!(resolve.state, StepState::Planned);
    assert!(w.error.as_deref().unwrap().contains("undone"));
}

/// The primary's `lgtm` on `threads`, each anchored to its current snapshot.
fn sign_off_write(threads: &[&str]) -> ExitWrite {
    sign_off_as(threads, "signed_off")
}

fn sign_off_as(threads: &[&str], disposition: &str) -> ExitWrite {
    let mut review = fake::review(json!([]), json!([]), json!([]), &fake::settings());
    let result = review.result.as_mut().unwrap();
    result.output.exit = Some(pr_sniper_lib::exits::ExitOutcome::Lgtm {
        comment: "Signed off: every finding is settled.".into(),
        resolve_own_threads: threads.iter().map(|t| t.to_string()).collect(),
        action: pr_sniper_lib::exits::LgtmAction::Merge,
        dismissed_teammate_findings: vec![],
    });
    result.finding_dispositions = threads
        .iter()
        .map(|id| {
            let comments = match *id {
                "T2" => json!({"300":hash("Please rename")}),
                _ => json!({"100":hash("Root finding"),"101":hash("Why?")}),
            };
            serde_json::from_value(json!({
                "thread_id":id,"disposition":disposition,"head_sha":HEAD,"base_sha":"b".repeat(40),
                "rationale":"Signed off: every finding is settled.","evidence":[],
                "anchor":{"base_sha":"b".repeat(40),"blobs":{},"manifests":"m","comments":comments}
            }))
            .unwrap()
        })
        .collect();
    exit_writes::plan(&review, 100).unwrap()
}

#[test]
fn a_sign_off_resolves_only_its_own_threads_without_fixed_evidence() {
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = sign_off_write(&["T1"]);
    assert_eq!(w.sign_off, Some(pr_sniper_lib::exits::LgtmAction::Merge));
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Completed);
    assert_eq!(wire.log(), ["resolve:T1"]);
    assert!(wire.thread("T1").resolved);

    // Another reviewer's thread is never the sign-off's to resolve.
    let wire = self::wire();
    let mut env = Env::new(&wire);
    let mut w = sign_off_write(&["T1", "T2"]);
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
    assert!(!wire.thread("T1").resolved);
}

#[test]
fn a_sign_off_resolves_silently_keeping_each_disposition_as_local_evidence() {
    for disposition in ["withdrawn", "no_longer_applies"] {
        let wire = wire();
        let mut env = Env::new(&wire);
        let mut w = sign_off_as(&["T1"], disposition);
        // No status chatter: no audit reply, the disposition stays local evidence.
        assert_eq!(w.steps.len(), 1);
        assert_eq!(
            w.steps[0]
                .resolution_evidence
                .as_ref()
                .unwrap()
                .disposition
                .id(),
            disposition
        );
        drive(&mut env, &mut w);
        assert_eq!(w.phase, Phase::Completed, "{disposition}");
        assert_eq!(wire.log(), ["resolve:T1"]);
    }
    // Not a disposition a sign-off accounts for: nothing is resolved.
    let wire = wire();
    let mut env = Env::new(&wire);
    let mut w = sign_off_as(&["T1"], "still_applies");
    drive(&mut env, &mut w);
    assert_ne!(w.phase, Phase::Completed);
    assert!(!wire.thread("T1").resolved);
}

#[test]
fn a_sign_off_is_refused_when_its_thread_changed_since_the_review() {
    let wire = wire();
    let mut env = Env::new(&wire);
    wire.0.lock().unwrap().threads[0]
        .comments
        .push(human_concern());
    let mut w = sign_off_write(&["T1"]);
    drive(&mut env, &mut w);
    assert_eq!(w.phase, Phase::Tossed);
    assert!(wire.log().is_empty());
    assert!(!wire.thread("T1").resolved);
}
