use super::*;
use crate::{
    exits::{ExitOutcome, LgtmAction, NewComment},
    monitoring::JobOperation,
    review::{Decision, ReviewRun, Selection},
    storage::Store,
};
use serde_json::json;

const REPO: &str = "00000000-0000-4000-8000-000000000001";
const ASSIGNMENT: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "00000000-0000-4000-8000-000000000003";
const NOW: i64 = 1_000_000;

fn head() -> String {
    "abc1234".to_string() + &"f".repeat(33)
}

fn settings(sign_off: bool, comment: bool) -> Settings {
    serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{"id":REPO,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "sign_off_comments":sign_off,
            "assignments":[{"id":ASSIGNMENT,"agent_id":AGENT,
                "schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":comment}]}],
        "agents":[{"id":AGENT,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Apply the Example doctrine.","signature":"stored"}]
    }))
    .unwrap()
}

fn job(head: &str) -> QueueJob {
    serde_json::from_value(json!({
        "assignment_id":ASSIGNMENT,"provider":"github","account_id":"22","account_login":"operator",
        "configuration_id":REPO,"repository_id":"100","repository_name":"example/repo",
        "pull_request_id":"1","number":1,"title":"Change","head_sha":head,
        "observed_base_sha":"b".repeat(40),"trigger_policy":"[[\"11\"],true]",
        "author_id":"11","author_login":"author","watched_author":true,
        "requested_reviewer":false,"waiting":"human_start","detected_at":100
    }))
    .unwrap()
}

fn run_with(exit: ExitOutcome, head: &str, settings: &Settings) -> ReviewRun {
    let job = job(head);
    let mut operation = JobOperation::review(&job, NOW);
    operation.state = OperationState::Completed;
    operation.completed_at = Some(NOW);
    let mut result: crate::review::ReviewResult = serde_json::from_value(json!({
        "reviewed_base_sha":"b".repeat(40),"output":{"synopsis":"Fine.",
            "files":[],"findings":[],"decision":"machine_sign_off"},
        "session_id":"session","model":"model","runtime_version":"fixture",
        "input_tokens":1,"output_tokens":1,"tool_calls":1
    }))
    .unwrap();
    result.output.decision = Decision::HumanInputRequired;
    result.output.exit = Some(exit);
    ReviewRun {
        failed_duck: None,
        failed_model: None,
        attempt_ducks: Vec::new(),
        mode: crate::review::RunMode::Packet,
        packet: None,
        feedback_context: None,
        key: crate::review::key(&job, ASSIGNMENT),
        selection: Selection::resolve(settings, &job, ASSIGNMENT)
            .unwrap()
            .into(),
        job,
        assignment_id: ASSIGNMENT.into(),
        operation,
        manual_start: true,
        trust_confirmed: true,
        phase: "Completed".into(),
        error: None,
        result: Some(result),
    }
}

fn lgtm() -> ExitOutcome {
    ExitOutcome::Lgtm {
        comment: "Clean.".into(),
        resolve_own_threads: vec![],
        action: LgtmAction::Ready,
        dismissed_teammate_findings: vec![],
    }
}

fn ready() -> ExitOutcome {
    ExitOutcome::ReadyForHuman {
        summary: "Clean.".into(),
    }
}

/// The queue projection of one completed primary run, read from a real store.
fn project(settings: &Settings, runs: &[ReviewRun]) -> (tempfile::TempDir, Snapshot) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.save_settings(settings).unwrap();
    store
        .save_queue(&runs.iter().map(|r| r.job.clone()).collect::<Vec<_>>())
        .unwrap();
    store.save_reviews(runs).unwrap();
    let snapshot = queue::snapshot(&store, vec![]).unwrap();
    (root, snapshot)
}

fn planned(settings: &Settings, exit: ExitOutcome) -> Vec<SignOff> {
    let (_root, snapshot) = project(settings, &[run_with(exit, &head(), settings)]);
    plan(settings, &snapshot, &[], NOW)
}

#[test]
fn switch_off_plans_nothing() {
    assert!(planned(&settings(false, true), lgtm()).is_empty());
}

#[test]
fn publish_comment_off_plans_nothing() {
    let settings = settings(true, false);
    assert!(!settings.repositories[0].sign_off_comments_active());
    assert!(planned(&settings, lgtm()).is_empty());
}

#[test]
fn clean_lgtm_and_ready_for_human_plan_one_sign_off_for_the_revision() {
    for exit in [lgtm(), ready()] {
        let plan = planned(&settings(true, true), exit);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].head, head());
        assert!(!plan[0].merged);
    }
}

#[test]
fn findings_and_open_threads_plan_nothing() {
    let settings = settings(true, true);
    let findings = ExitOutcome::Comments {
        new_comments: vec![NewComment {
            path: "a.rs".into(),
            line: 1,
            side: None,
            body: "Fix.".into(),
        }],
        body: None,
        reply_comments: vec![],
        resolve_own_threads: vec![],
    };
    assert!(planned(&settings, findings).is_empty());
    // A hand-off while own findings still apply is not clean.
    let mut run = run_with(ready(), &head(), &settings);
    run.result.as_mut().unwrap().finding_dispositions = vec![serde_json::from_value(json!({
        "thread_id":"T","disposition":"still_applies","rationale":"Unfixed.",
        "head_sha":head(),"base_sha":"b".repeat(40)
    }))
    .unwrap()];
    let (_root, snapshot) = project(&settings, &[run]);
    assert!(plan(&settings, &snapshot, &[], NOW).is_empty());
}

#[test]
fn outcomes_settled_before_the_fresh_window_plan_nothing() {
    let settings = settings(true, true);
    let (_root, snapshot) = project(&settings, &[run_with(lgtm(), &head(), &settings)]);
    assert!(plan(&settings, &snapshot, &[], NOW + FRESH_SECONDS + 1).is_empty());
}

fn approve_effect(action: &str, state: &str) -> crate::actions::Effect {
    serde_json::from_value(json!({
        "id":format!("final-{action}"),"final_id":"final","item_id":"item","action":action,
        "operation":{
            "id":format!("final-{action}-operation"),"provider":"github","account_id":"22",
            "configuration_id":REPO,"repository_id":"100","pull_request_id":"1",
            "head_sha":head(),"trigger_policy":"policy","operation_type":"github_approve",
            "state":"completed","attempt_count":1,"initial_attempt_at":NOW,
            "retry_deadline":NOW,"next_attempt_at":null,"failure":null,
            "attempted_mutation":null,"pending_review_id":null,"owned_thread_id":null,
            "triggering_external_comment_id":null,"confirmed_receipt":null
        },
        "observation":{
            "write_capability":true,"node_id":"node","repository_id":"100",
            "pull_request_id":"1","account_id":"22","author_id":"11",
            "head_repository_id":null,"head":head(),"base":"b".repeat(40),
            "base_name":"main","merge_rules":null,"merge_rules_error":null,
            "state":"OPEN","draft":false,"permission":"WRITE","mergeable":"MERGEABLE",
            "merge_state":"CLEAN","review_decision":null,"checks":null,
            "check_contexts":[],"in_merge_queue":false,"method":"SQUASH",
            "protection":null,"threads":[],"reviews":[],"comments":[],
            "merged_by":null,"merged_at":null,"merge_commit":null
        },
        "body":"","state":state,"error":null,"receipt":null
    }))
    .unwrap()
}

fn status(effects: Vec<crate::actions::Effect>) -> crate::actions::Status {
    crate::actions::Status {
        item_id: "item".into(),
        final_valid: true,
        provider_observed_at: Some(NOW),
        observation_retry_blocker: None,
        primary_assignment_id: Some(ASSIGNMENT.into()),
        machine_clear: true,
        personal_review: "",
        permissions: Default::default(),
        provider_approval: None,
        final_review: None,
        effects,
        blockers: vec![],
        settling: false,
    }
}

#[test]
fn approve_exits_sign_off_only_once_the_vote_or_merge_settles() {
    let settings = settings(true, true);
    for (exit, done, merged) in [
        (
            ExitOutcome::Approve {
                comment: "Clean.".into(),
            },
            ItemState::Approved,
            false,
        ),
        (
            ExitOutcome::ApproveAndMerge {
                comment: "Clean.".into(),
            },
            ItemState::Merged,
            true,
        ),
    ] {
        let (_root, mut snapshot) = project(&settings, &[run_with(exit, &head(), &settings)]);
        // Approving: the outcome is not settled yet.
        assert_eq!(snapshot.items[0].progress, Some(queue::Progress::Approving));
        assert!(plan(&settings, &snapshot, &[], NOW).is_empty());
        // Rejected: no sign-off.
        let item = &mut snapshot.items[0];
        item.progress = None;
        item.action_status = Some(status(vec![approve_effect("approve", "rejected")]));
        assert!(plan(&settings, &snapshot, &[], NOW).is_empty());
        // Confirmed.
        let item = &mut snapshot.items[0];
        item.state = done;
        item.action_status = Some(status(vec![approve_effect("approve", "confirmed")]));
        let plan = plan(&settings, &snapshot, &[], NOW);
        assert_eq!(plan.len(), 1, "{done:?}");
        assert_eq!(plan[0].merged, merged);
    }
    // A PR someone else merged after a Ready hand-off gets nothing new.
    let (_root, mut snapshot) = project(&settings, &[run_with(ready(), &head(), &settings)]);
    snapshot.items[0].state = ItemState::Merged;
    assert!(plan(&settings, &snapshot, &[], NOW).is_empty());
}

#[test]
fn a_same_head_re_review_never_plans_a_second_sign_off() {
    let settings = settings(true, true);
    let first = run_with(lgtm(), &head(), &settings);
    let (_root, snapshot) = project(&settings, std::slice::from_ref(&first));
    let existing = plan(&settings, &snapshot, &[], NOW);
    assert_eq!(existing.len(), 1);
    // The state_changed pass of the same head settles clean again.
    let mut second = run_with(ready(), &head(), &settings);
    second.operation.id = "second-pass".into();
    second.operation.completed_at = Some(NOW + 60);
    let (_root, again) = project(&settings, &[second]);
    assert!(plan(&settings, &again, &existing, NOW + 60).is_empty());
    // A new head is a new revision.
    let next = run_with(lgtm(), &"d".repeat(40), &settings);
    let (_root, moved) = project(&settings, &[next]);
    assert_eq!(plan(&settings, &moved, &existing, NOW + 60).len(), 1);
}

#[derive(Default)]
struct Fake {
    gate: Option<bool>,
    pull_head: String,
    draft: bool,
    lifecycle: Option<Lifecycle>,
    comments: Vec<TopComment>,
    posts: Vec<String>,
    post_fails: Option<bool>,
    saved: Vec<SignOff>,
}

impl Environment for Fake {
    fn now(&self) -> Result<i64, Failure> {
        Ok(NOW)
    }
    fn save(&mut self, record: &SignOff) -> Result<(), Failure> {
        self.saved.push(record.clone());
        Ok(())
    }
    fn gate(&mut self, _record: &SignOff) -> Result<Gate, Failure> {
        Ok(match self.gate {
            None => Gate::Refused,
            Some(review_drafts) => Gate::Open { review_drafts },
        })
    }
    fn pull(&mut self, _record: &SignOff) -> Result<PullRequest, Failure> {
        Ok(PullRequest {
            mentioned: false,
            id: "1".into(),
            number: 1,
            title: "Change".into(),
            author: None,
            requested_reviewers: vec![],
            requested_teams: vec![],
            state: self.lifecycle.clone().unwrap_or(Lifecycle::Open),
            draft: self.draft,
            head_sha: self.pull_head.clone(),
            base_sha: "b".repeat(40),
            head_repository_id: None,
            base_repository_id: "100".into(),
            updated_at: String::new(),
            files: vec![],
        })
    }
    fn comments(&mut self, _record: &SignOff) -> Result<Vec<TopComment>, Failure> {
        Ok(self.comments.clone())
    }
    fn post(&mut self, _record: &SignOff, body: &str) -> Result<String, WriteFailure> {
        self.posts.push(body.into());
        match self.post_fails {
            None => {
                self.comments.push(comment("900", body));
                Ok("900".into())
            }
            Some(uncertain) => Err(WriteFailure {
                failure: Failure::permanent("refused"),
                uncertain,
            }),
        }
    }
}

fn comment(id: &str, body: &str) -> TopComment {
    TopComment {
        id: id.into(),
        body: body.into(),
        author_id: Some("22".into()),
        author_login: Some("operator".into()),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn fresh() -> SignOff {
    planned(&settings(true, true), lgtm()).remove(0)
}

fn open() -> Fake {
    Fake {
        gate: Some(false),
        pull_head: head(),
        ..Fake::default()
    }
}

#[test]
fn posts_one_fixed_host_comment_naming_the_short_revision() {
    let mut env = open();
    let mut record = fresh();
    run(&mut env, &mut record).unwrap();
    assert_eq!(record.phase, Phase::Done);
    assert_eq!(record.receipt.as_deref(), Some("900"));
    assert_eq!(
        env.posts,
        [format!(
            "Revision `abc1234` looks good. No issues found.\n\n<!-- pr-sniper:signoff:{} -->\n\n\u{1F52B} PR Sniper automated review",
            head()
        )]
    );
    // No agent prose, doctrine or recap: the Agent's prompt and exit text never appear.
    assert!(!env.posts[0].contains("doctrine") && !env.posts[0].contains("Clean."));
    // Intent was saved before the post.
    assert_eq!(env.saved[0].phase, Phase::Attempted);
    // Done is terminal: a retry posts nothing more.
    run(&mut env, &mut record).unwrap();
    assert_eq!(env.posts.len(), 1);
}

#[test]
fn restart_finds_the_marker_and_never_posts_twice() {
    // State lost after a landed post: the remote marker is the receipt.
    let mut env = open();
    env.comments = vec![comment("77", &body(&head()))];
    let mut record = fresh();
    run(&mut env, &mut record).unwrap();
    assert_eq!(record.phase, Phase::Done);
    assert_eq!(record.receipt.as_deref(), Some("77"));
    assert!(env.posts.is_empty());

    // Crash after the intent, post not visible: only read back, then give up.
    let mut env = open();
    let mut record = fresh();
    record.phase = Phase::Attempted;
    for _ in 0..ATTEMPTS {
        record.next_at = NOW;
        run(&mut env, &mut record).unwrap();
    }
    assert!(env.posts.is_empty());
    assert_eq!(record.phase, Phase::Failed);
}

#[test]
fn an_uncertain_post_is_read_back_never_reposted() {
    let mut env = open();
    env.post_fails = Some(true);
    let mut record = fresh();
    run(&mut env, &mut record).unwrap();
    assert_eq!(record.phase, Phase::Attempted);
    env.post_fails = None;
    run(&mut env, &mut record).unwrap();
    assert_eq!(env.posts.len(), 1);
    // A refused post may be retried; attempts are bounded.
    let mut env = open();
    env.post_fails = Some(false);
    let mut record = fresh();
    for _ in 0..ATTEMPTS {
        run(&mut env, &mut record).unwrap();
    }
    assert_eq!(env.posts.len(), usize::from(ATTEMPTS));
    assert_eq!(record.phase, Phase::Failed);
}

#[test]
fn a_moved_head_closed_pr_draft_or_refused_gate_posts_nothing() {
    let cases: [fn(&mut Fake); 4] = [
        |env| env.pull_head = "d".repeat(40),
        |env| env.lifecycle = Some(Lifecycle::Closed),
        |env| env.draft = true,
        |env| env.gate = None,
    ];
    for change in cases {
        let mut env = open();
        change(&mut env);
        let mut record = fresh();
        run(&mut env, &mut record).unwrap();
        assert_eq!(record.phase, Phase::Skipped);
        assert!(env.posts.is_empty());
    }
    // Review drafts on: a reviewed draft gets its sign-off.
    let mut env = open();
    env.draft = true;
    env.gate = Some(true);
    let mut record = fresh();
    run(&mut env, &mut record).unwrap();
    assert_eq!(env.posts.len(), 1);
}

#[test]
fn own_sign_offs_never_change_the_pr_state_hash() {
    use crate::provider::{PrSnapshot, SnapshotComment, SnapshotThread};
    let base = PrSnapshot {
        head: head(),
        comments: vec![SnapshotComment {
            id: "1".into(),
            body: "Question?".into(),
            commit: None,
        }],
        comment_total: 1,
        ..PrSnapshot::default()
    };
    let sign_off = || SnapshotComment {
        id: "2".into(),
        body: body(&head()),
        commit: None,
    };
    // GitHub: a conversation comment.
    let mut github = base.clone();
    github.comments.push(sign_off());
    github.comment_total += 1;
    assert_eq!(github.normalize().hash(), base.normalize().hash());
    // Azure DevOps: a closed general thread.
    let mut ado = base.clone();
    ado.threads.push(SnapshotThread {
        id: "t".into(),
        resolved: true,
        outdated: false,
        comments: vec![sign_off()],
        comment_total: 1,
    });
    ado.thread_total += 1;
    assert_eq!(ado.normalize().hash(), base.normalize().hash());
    // A human comment quoting the marker is still a change.
    let mut human = base.clone();
    human.comments.push(SnapshotComment {
        body: format!("Why? {}", body(&head())),
        ..sign_off()
    });
    human.comment_total += 1;
    assert_ne!(human.normalize().hash(), base.normalize().hash());
    assert!(is_sign_off(&body(&head()).replace('\n', "\r\n")));
}
