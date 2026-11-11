//! Path globs end to end (ADR 0027): provider reads → path facts → queue.
//! Gate, secondary skips, incremental matching, rebase fallback, the
//! strategy ladder and parking, over a fake GitHub transport.

use super::*;
use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::{Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository},
    Identity,
};
use crate::monitoring::{
    ActivationMode, Monitor, MonitoringActivation, PollResult, WAITING_AI_CAPACITY,
    WAITING_INCOMPLETE_FILES, WAITING_SKIPPED_NO_MATCHING_FILES, WAITING_SUPERSEDED,
};
use crate::queue::State;
use crate::review::packet_run;
use crate::storage::{Settings, Store};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const REPO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const PRIMARY: &str = "cccccccc-cccc-4ccc-8ccc-000000000001";
const TEAMMATE: &str = "cccccccc-cccc-4ccc-8ccc-000000000002";
const NOW: i64 = 1_800_000_000;
const API: &str = "/repos/octo-org/app";

fn sha(c: char) -> String {
    c.to_string().repeat(40)
}

fn configured(gate: &[&str], teammate: &[&str]) -> (tempfile::TempDir, Store, Monitor) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let agent = |index: u8, name: &str| {
        json!({
            "id":format!("aaaaaaaa-aaaa-4aaa-8aaa-00000000000{index}"),"name":name,
            "model":"explicit-model","ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Find actionable defects.","signature":"fixture"
        })
    };
    let settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,"doctrines":[],
        "repositories":[{
            "id":REPO,"provider":"github","name":"octo-org/app","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "admission":{"mode":"all","rules":[]},
            "paths":gate,
            "primary_assignment_id":PRIMARY,
            "assignments":[
                {"id":PRIMARY,"agent_id":"aaaaaaaa-aaaa-4aaa-8aaa-000000000001","comment":true},
                {"id":TEAMMATE,"agent_id":"aaaaaaaa-aaaa-4aaa-8aaa-000000000002","comment":true,
                 "paths":teammate}
            ]
        }],
        "agents":[agent(1, "Primary"), agent(2, "Docs Reviewer")]
    }))
    .unwrap();
    settings.validate().unwrap();
    store.save_settings(&settings).unwrap();
    let context = Monitor::activation_context(&settings, REPO).unwrap();
    let mut state = store.load_monitoring_state().unwrap();
    state.activations.insert(
        REPO.into(),
        MonitoringActivation {
            version: "fixture-activation".into(),
            repository_id: REPO.into(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: 0,
            mode: ActivationMode::NewOnly,
            selected_existing: 0,
            baseline: BTreeMap::new(),
            confirmed_at: NOW - 1,
        },
    );
    store.save_monitoring_state(&state).unwrap();
    let monitor = Monitor::restore(&store).unwrap();
    (root, store, monitor)
}

/// GitHub answers by exact path; anything else is a test bug.
#[derive(Clone, Default)]
struct Wire {
    replies: Arc<Mutex<BTreeMap<String, Value>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Wire {
    fn on(&self, path: String, body: Value) -> &Self {
        self.replies.lock().unwrap().insert(path, body);
        self
    }
    /// A PR head with these files, listed in full.
    fn head(&self, head: char, files: &[&str]) -> &Self {
        self.on(
            format!("{API}/pulls/1"),
            json!({"head":{"sha":sha(head)},"changed_files":files.len()}),
        )
        .on(
            format!("{API}/pulls/1/files?per_page=100&page=1"),
            Value::Array(files.iter().map(|f| entry(f)).collect()),
        )
    }
    fn compare(&self, old: char, new: char, status: &str, files: &[&str]) -> &Self {
        self.on(
            format!("{API}/compare/{}...{}", sha(old), sha(new)),
            json!({"status":status,"files":files.iter().map(|f| entry(f)).collect::<Vec<_>>()}),
        )
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl crate::github::provider::Transport for Wire {
    fn get(
        &self,
        path: &str,
    ) -> Result<crate::github::provider::Response, crate::provider::ConnectionError> {
        self.calls.lock().unwrap().push(path.into());
        let body = self
            .replies
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_else(|| panic!("unexpected read {path}"));
        Ok(crate::github::provider::Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

fn entry(name: &str) -> Value {
    json!({"filename":name,"status":"modified","additions":1,"deletions":0,"changes":1,"sha":sha('c')})
}

fn pull(head: char) -> PullRequest {
    on_base(head, 'f')
}

fn on_base(head: char, base: char) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "9".into(),
        number: 1,
        title: "Change".into(),
        author: Some(Identity {
            id: "11".into(),
            login: "author".into(),
        }),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: sha(head),
        base_sha: sha(base),
        head_repository_id: Some("100".into()),
        base_repository_id: "100".into(),
        updated_at: "2026-11-01T00:00:00Z".into(),
        files: vec![],
    }
}

/// One poll: listing, path facts through the provider, then the commit.
fn scan(monitor: &mut Monitor, store: &Store, wire: &Wire, head: char, at: i64) {
    let (ticket, result) = read(monitor, store, wire, pull(head), at);
    monitor.finish(store, ticket, result, at + 1).unwrap();
}

/// A poll's reads, before its commit.
fn read(
    monitor: &mut Monitor,
    store: &Store,
    wire: &Wire,
    pull: PullRequest,
    at: i64,
) -> (
    crate::monitoring::PollTicket,
    Result<PollResult, crate::provider::ConnectionError>,
) {
    let ticket = monitor.prepare_checks(store, at, true).unwrap().remove(0);
    let connection = Connection {
        identity: Identity {
            id: "22".into(),
            login: "acting-account".into(),
        },
        repository: RemoteRepository {
            id: "100".into(),
            name: "octo-org/app".into(),
        },
        capabilities: Capabilities {
            read: true,
            comment: CommentCapability::Available,
        },
    };
    let pulls = vec![pull];
    let client = GithubClient::new(wire.clone());
    let result = crate::path_facts(&client, &connection, &pulls, &ticket).map(|facts| PollResult {
        connection,
        pull_requests: pulls,
        states: Default::default(),
        facts,
        listing: None,
    });
    (ticket, result)
}

/// Each assignment's entry for `head`.
fn waiting(store: &Store, head: char) -> BTreeMap<String, String> {
    store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.head_sha == sha(head) && j.waiting != WAITING_SUPERSEDED)
        .map(|j| (j.assignment_id.unwrap(), j.waiting))
        .collect()
}

fn item(store: &Store, monitor: &Monitor) -> crate::queue::Item {
    let snapshot = crate::queue::snapshot(store, monitor.snapshot()).unwrap();
    assert_eq!(snapshot.items.len(), 1);
    snapshot.items.into_iter().next().unwrap()
}

fn primary_blocked(store: &Store, now: i64) -> bool {
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.assignment_id.as_deref() == Some(PRIMARY))
        .find(|j| j.waiting == WAITING_AI_CAPACITY)
        .unwrap();
    let primary = key(&job, PRIMARY);
    let work = crate::capacity::candidates(store, now).unwrap();
    let primary = work.iter().find(|w| w.key.id == primary).unwrap();
    primary.blocked_by.is_some()
}

fn report() -> crate::review::ReviewResult {
    let mut result: crate::review::ReviewResult = serde_json::from_value(json!({
        "reviewed_base_sha":sha('f'),"output":{},
        "session_id":"fixture","model":"fixture-model","runtime_version":"fixture",
        "input_tokens":1,"output_tokens":1,"tool_calls":1
    }))
    .unwrap();
    result.output = crate::review::ReviewOutput::from_exit(
        crate::exits::ExitOutcome::SubmitReport,
        Some(crate::exits::TeammateReport { findings: vec![] }),
    );
    result
}

/// Runs the teammate's entry for the current head to a completed report.
fn complete_teammate(store: &Store, now: i64) {
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.assignment_id.as_deref() == Some(TEAMMATE))
        .filter(|j| j.waiting == WAITING_AI_CAPACITY)
        .max_by_key(|j| j.work.as_ref().map(|w| w.state_seq))
        .unwrap();
    let key = key(&job, TEAMMATE);
    request(store, &key, true, now).unwrap();
    let run = prepare_dispatch(store, &key, now).unwrap().unwrap();
    complete(store, &run.operation.id, Ok(report()), now + 1).unwrap();
}

#[test]
fn empty_globs_queue_every_assignment_with_one_count_read() {
    let (_root, store, mut monitor) = configured(&[], &[]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(
        waiting(&store, 'a'),
        BTreeMap::from([
            (PRIMARY.into(), WAITING_AI_CAPACITY.into()),
            (TEAMMATE.into(), WAITING_AI_CAPACITY.into()),
        ])
    );
    assert_eq!(wire.calls(), [format!("{API}/pulls/1")], "no file listing");
    // The same head is never read again.
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    assert_eq!(wire.calls().len(), 1);
    let item = item(&store, &monitor);
    assert!(item.skipped.is_empty() && item.incomplete_files.is_none());
}

#[test]
fn the_gate_keeps_out_non_matching_and_excluded_prs() {
    for (files, queued) in [
        (&["docs/a.md"][..], false),
        (&["src/gen/a.rs"][..], false),
        (&["docs/a.md", "src/gen/a.rs"][..], false),
        (&["src/a.rs"][..], true),
    ] {
        let (_root, store, mut monitor) = configured(&["src/", "!src/gen/"], &[]);
        let wire = Wire::default();
        wire.head('a', files);
        scan(&mut monitor, &store, &wire, 'a', NOW);
        assert_eq!(!waiting(&store, 'a').is_empty(), queued, "{files:?}");
    }
}

#[test]
fn a_tracked_pr_whose_new_revision_leaves_the_gate_gets_no_new_reviews() {
    let (_root, store, mut monitor) = configured(&["src/"], &[]);
    let wire = Wire::default();
    wire.head('a', &["src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(waiting(&store, 'a').len(), 2);
    wire.head('b', &["docs/a.md"]);
    scan(&mut monitor, &store, &wire, 'b', NOW + 10);
    assert!(waiting(&store, 'b').is_empty());
    assert!(
        waiting(&store, 'a').is_empty(),
        "the old revision is superseded"
    );
}

#[test]
fn a_skipped_secondary_is_shown_and_never_holds_the_primary() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(
        waiting(&store, 'a'),
        BTreeMap::from([
            (PRIMARY.into(), WAITING_AI_CAPACITY.into()),
            (TEAMMATE.into(), WAITING_SKIPPED_NO_MATCHING_FILES.into()),
        ])
    );
    // Stable across polls of the same head.
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    assert_eq!(
        waiting(&store, 'a')[TEAMMATE],
        WAITING_SKIPPED_NO_MATCHING_FILES
    );
    assert!(!primary_blocked(&store, NOW + 20));
    let item = item(&store, &monitor);
    assert_eq!(item.skipped.len(), 1);
    assert_eq!(item.skipped[0].assignment_id, TEAMMATE);
    assert_eq!(item.skipped[0].agent_name, "Docs Reviewer");
    assert_eq!(
        item.skipped[0].reason,
        crate::queue::SkipReason::NoMatchingFiles
    );
    assert_ne!(item.state, State::Failed);
}

#[test]
fn a_stale_revision_keeps_its_skipped_secondary() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    wire.head('b', &["docs/a.md", "src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'b', NOW + 10);
    assert_eq!(waiting(&store, 'b')[TEAMMATE], WAITING_AI_CAPACITY);
    // Startup and maintenance compaction keep the skip history.
    crate::monitoring::compact_saved_queue(&store).unwrap();
    let snapshot = crate::queue::snapshot(&store, monitor.snapshot()).unwrap();
    let stale = snapshot
        .items
        .iter()
        .find(|item| item.job.head_sha == sha('a'))
        .unwrap();
    assert_eq!(stale.state, State::Stale);
    assert_eq!(stale.skipped.len(), 1);
    assert_eq!(stale.skipped[0].assignment_id, TEAMMATE);
    assert_eq!(
        stale.skipped[0].reason,
        crate::queue::SkipReason::NoMatchingFiles
    );
    let current = snapshot
        .items
        .iter()
        .find(|item| item.job.head_sha == sha('b'))
        .unwrap();
    assert!(current.skipped.is_empty());
}

#[test]
fn a_same_head_state_move_keeps_the_secondary_report_on_the_item() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md", "src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    complete_teammate(&store, NOW + 2);
    let reported = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|j| j.assignment_id.as_deref() == Some(TEAMMATE))
        .map(|j| key(&j, TEAMMATE))
        .unwrap();
    // Discussion moves the PR state; the head stays.
    let mut states = store.load_pr_states().unwrap();
    let binding = states.records[0].binding.clone();
    states.review_again(&binding, NOW + 5).unwrap();
    store.save_pr_states(&states).unwrap();
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    assert_eq!(
        waiting(&store, 'a')[TEAMMATE],
        WAITING_SKIPPED_NO_MATCHING_FILES,
        "no file changed since its review of this head"
    );
    // The item shows the report it gave for this head, not a skip.
    let item = item(&store, &monitor);
    assert!(item.skipped.is_empty(), "{:?}", item.skipped);
    assert!(item.blocked_by.is_none(), "{:?}", item.blocked_by);
    assert!(
        item.review_keys.contains(&reported),
        "{:?}",
        item.review_keys
    );
}

#[test]
fn a_replacement_agent_skip_shows_despite_the_old_agent_report() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    complete_teammate(&store, NOW + 2);
    let mut settings = store.load_settings().unwrap();
    let mut replacement = settings.agents[1].clone();
    replacement.id = "aaaaaaaa-aaaa-4aaa-8aaa-000000000003".into();
    replacement.name = "Code Reviewer".into();
    settings.agents.push(replacement);
    let assignment = &mut settings.repositories[0].assignments[1];
    assignment.agent_id = "aaaaaaaa-aaaa-4aaa-8aaa-000000000003".into();
    assignment.paths = vec!["src/".into()];
    settings.validate().unwrap();
    store.save_settings(&settings).unwrap();
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    assert_eq!(
        waiting(&store, 'a')[TEAMMATE],
        WAITING_SKIPPED_NO_MATCHING_FILES
    );
    let item = item(&store, &monitor);
    assert_eq!(item.skipped.len(), 1, "{:?}", item.skipped);
    assert_eq!(item.skipped[0].assignment_id, TEAMMATE);
}

#[test]
fn a_hidden_same_head_skip_never_leaves_the_item_stale_after_a_draft_round_trip() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    complete_teammate(&store, NOW + 2);
    let reported = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|j| j.assignment_id.as_deref() == Some(TEAMMATE))
        .map(|j| key(&j, TEAMMATE))
        .unwrap();
    let mut draft = pull('a');
    draft.draft = true;
    let (ticket, result) = read(&mut monitor, &store, &wire, draft, NOW + 5);
    monitor.finish(&store, ticket, result, NOW + 6).unwrap();
    let mut states = store.load_pr_states().unwrap();
    let binding = states.records[0].binding.clone();
    states.review_again(&binding, NOW + 7).unwrap();
    store.save_pr_states(&states).unwrap();
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    let item = item(&store, &monitor);
    assert_ne!(item.state, State::Stale);
    assert!(item.skipped.is_empty(), "{:?}", item.skipped);
    assert!(
        item.review_keys.contains(&reported),
        "{:?}",
        item.review_keys
    );
}

/// Moves the PR state on the same head; the next poll queues a new pass.
fn move_state(store: &Store, at: i64) {
    let mut states = store.load_pr_states().unwrap();
    // The primary's exit would consume the previous state; none runs here.
    states.records[0].pending = None;
    let binding = states.records[0].binding.clone();
    states.review_again(&binding, at).unwrap();
    store.save_pr_states(&states).unwrap();
}

fn teammate_keys(store: &Store) -> Vec<String> {
    let mut jobs: Vec<_> = store
        .load_queue()
        .unwrap()
        .into_iter()
        .filter(|j| j.assignment_id.as_deref() == Some(TEAMMATE))
        .collect();
    jobs.sort_by_key(|j| j.work.as_ref().map(|w| w.state_seq));
    jobs.iter().map(|j| key(j, TEAMMATE)).collect()
}

#[test]
fn a_hidden_same_head_skip_shows_the_newest_report() {
    let (_root, store, mut monitor) = configured(&[], &[]);
    let wire = Wire::default();
    wire.head('a', &["src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    complete_teammate(&store, NOW + 2);
    move_state(&store, NOW + 5);
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    complete_teammate(&store, NOW + 12);
    let keys = teammate_keys(&store);
    let (first, second) = (keys[0].clone(), keys[1].clone());
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[1].paths = vec!["src/".into()];
    store.save_settings(&settings).unwrap();
    move_state(&store, NOW + 15);
    scan(&mut monitor, &store, &wire, 'a', NOW + 20);
    assert_eq!(
        waiting(&store, 'a')[TEAMMATE],
        WAITING_SKIPPED_NO_MATCHING_FILES
    );
    let item = item(&store, &monitor);
    assert!(item.skipped.is_empty(), "{:?}", item.skipped);
    assert!(item.review_keys.contains(&second), "{:?}", item.review_keys);
    assert!(!item.review_keys.contains(&first), "{:?}", item.review_keys);
}

#[test]
fn a_skip_saved_before_the_flag_existed_survives_a_new_revision() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    let mut queue = store.load_queue().unwrap();
    for job in &mut queue {
        job.work.as_mut().unwrap().skipped = false;
    }
    store.save_queue(&queue).unwrap();
    wire.head('b', &["docs/a.md", "src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'b', NOW + 10);
    crate::monitoring::compact_saved_queue(&store).unwrap();
    let snapshot = crate::queue::snapshot(&store, monitor.snapshot()).unwrap();
    let stale = snapshot
        .items
        .iter()
        .find(|item| item.job.head_sha == sha('a'))
        .unwrap();
    assert_eq!(stale.skipped.len(), 1);
    assert_eq!(stale.skipped[0].assignment_id, TEAMMATE);
}

#[test]
fn a_secondary_matches_incrementally_and_falls_back_to_the_whole_pr_on_rebase() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md", "src/a.rs"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(waiting(&store, 'a')[TEAMMATE], WAITING_AI_CAPACITY);
    complete_teammate(&store, NOW + 2);

    // Push b only touches src/ since the teammate's review of a: skipped.
    wire.head('b', &["docs/a.md", "src/a.rs", "src/b.rs"])
        .compare('a', 'b', "ahead", &["src/b.rs"]);
    scan(&mut monitor, &store, &wire, 'b', NOW + 10);
    assert_eq!(
        waiting(&store, 'b'),
        BTreeMap::from([
            (PRIMARY.into(), WAITING_AI_CAPACITY.into()),
            (TEAMMATE.into(), WAITING_SKIPPED_NO_MATCHING_FILES.into()),
        ])
    );

    // Rebased onto new history: a is no ancestor, so the whole PR decides.
    wire.head('c', &["docs/a.md", "src/b.rs"])
        .compare('a', 'c', "diverged", &[]);
    scan(&mut monitor, &store, &wire, 'c', NOW + 20);
    assert_eq!(waiting(&store, 'c')[TEAMMATE], WAITING_AI_CAPACITY);
    // Compared from the last completed review, not the skipped revision.
    assert!(!wire
        .calls()
        .iter()
        .any(|c| c.contains(&format!("{}...", sha('b')))));
}

#[test]
fn an_incomplete_listing_climbs_the_ladder_before_it_parks() {
    let (_root, store, mut monitor) = configured(&["src/"], &[]);
    let wire = Wire::default();
    let trees = |wire: &Wire, truncated: bool| {
        for commit in ['9', 'a'] {
            wire.on(
                format!("{API}/git/trees/{}?recursive=1", sha(commit)),
                json!({"sha":sha('e'),"truncated":truncated,"tree":[]}),
            )
            .on(
                format!("{API}/git/trees/{}", sha(commit)),
                json!({"sha":sha('e'),"truncated":true,"tree":[]}),
            );
        }
    };
    wire.on(
        format!("{API}/pulls/1"),
        json!({"head":{"sha":sha('a')},"changed_files":3_001}),
    )
    .on(
        format!("{API}/compare/{}...{}?per_page=1", sha('f'), sha('a')),
        json!({"status":"ahead","merge_base_commit":{"sha":sha('9')}}),
    );
    trees(&wire, true);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    // Listing, tree diff and scoped tree diff all failed: parked.
    assert_eq!(
        waiting(&store, 'a'),
        BTreeMap::from([
            (PRIMARY.into(), WAITING_INCOMPLETE_FILES.into()),
            (TEAMMATE.into(), WAITING_INCOMPLETE_FILES.into()),
        ])
    );
    let parked = item(&store, &monitor);
    assert_eq!(parked.state, State::Failed);
    assert_eq!(
        parked.failure_cause,
        Some(crate::queue::CAUSE_INCOMPLETE_FILES)
    );
    let incomplete = parked.incomplete_files.unwrap();
    assert_eq!(incomplete.count, 3_001);
    use crate::paths::Strategy::*;
    assert_eq!(incomplete.tried, [Listing, TreeDiff, ScopedTreeDiff]);
    assert!(parked.remedy.is_none() && !parked.retry_all);
    assert!(dispatch(&store, NOW + 5).is_empty(), "no review runs");

    // The next revision is evaluated afresh: a complete tree diff decides.
    let (_root, store, mut monitor) = configured(&["src/"], &[]);
    let wire = Wire::default();
    wire.on(
        format!("{API}/pulls/1"),
        json!({"head":{"sha":sha('a')},"changed_files":3_001}),
    )
    .on(
        format!("{API}/compare/{}...{}?per_page=1", sha('f'), sha('a')),
        json!({"status":"ahead","merge_base_commit":{"sha":sha('9')}}),
    );
    trees(&wire, false);
    wire.on(
        format!("{API}/git/trees/{}?recursive=1", sha('a')),
        json!({"sha":sha('e'),"truncated":false,"tree":[
            {"path":"src/a.rs","mode":"100644","type":"blob","sha":sha('1')}
        ]}),
    );
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(waiting(&store, 'a')[PRIMARY], WAITING_AI_CAPACITY);
}

#[test]
fn without_globs_a_count_past_the_listing_cap_still_climbs_before_parking() {
    let (_root, store, mut monitor) = configured(&[], &[]);
    let wire = Wire::default();
    wire.on(
        format!("{API}/pulls/1"),
        json!({"head":{"sha":sha('a')},"changed_files":5_000}),
    )
    .on(
        format!("{API}/compare/{}...{}?per_page=1", sha('f'), sha('a')),
        json!({"status":"ahead","merge_base_commit":{"sha":sha('9')}}),
    );
    for commit in ['9', 'a'] {
        wire.on(
            format!("{API}/git/trees/{}?recursive=1", sha(commit)),
            json!({"sha":sha('e'),"truncated":false,"tree":[]}),
        );
    }
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(waiting(&store, 'a')[PRIMARY], WAITING_AI_CAPACITY);
    assert!(!wire.calls().iter().any(|c| c.contains("/files")));
}

/// Keys the scheduler would start now.
fn dispatch(store: &Store, now: i64) -> Vec<String> {
    crate::capacity::candidates(store, now)
        .unwrap()
        .into_iter()
        .filter(|w| w.blocked_by.is_none() && w.reason.is_none())
        .map(|w| w.key.id)
        .collect()
}

#[test]
fn a_retarget_on_the_same_head_re_reads_the_gate() {
    let (_root, store, mut monitor) = configured(&["src/"], &[]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert!(waiting(&store, 'a').is_empty());
    // Retargeted to another base: same head, now touching src/.
    wire.head('a', &["src/auth.rs"]);
    let (ticket, result) = read(&mut monitor, &store, &wire, on_base('a', 'e'), NOW + 10);
    monitor.finish(&store, ticket, result, NOW + 11).unwrap();
    assert_eq!(waiting(&store, 'a').len(), 2);
}

#[test]
fn a_verdict_read_with_old_secondary_paths_never_skips_under_new_ones() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["src/a.rs"]);
    let (ticket, result) = read(&mut monitor, &store, &wire, pull('a'), NOW);
    // Saved while the poll was in flight: the teammate now wants src/.
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[1].paths = vec!["src/".into()];
    store.save_settings(&settings).unwrap();
    monitor.finish(&store, ticket, result, NOW + 1).unwrap();
    assert_eq!(waiting(&store, 'a')[TEAMMATE], WAITING_AI_CAPACITY);
    // The next poll reads the new patterns and keeps it.
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    assert_eq!(waiting(&store, 'a')[TEAMMATE], WAITING_AI_CAPACITY);
}

#[test]
fn a_push_racing_the_path_read_requeues_the_poll_without_a_failure() {
    let (_root, store, mut monitor) = configured(&["src/"], &[]);
    let wire = Wire::default();
    // Listed at a, but the detail read already sees b.
    wire.head('b', &["src/a.rs"]);
    let (ticket, result) = read(&mut monitor, &store, &wire, pull('a'), NOW);
    assert!(matches!(
        result,
        Err(crate::provider::ConnectionError::RevisionChanged)
    ));
    monitor.finish(&store, ticket, result, NOW + 1).unwrap();
    let health = monitor
        .snapshot()
        .into_iter()
        .find(|h| h.repository_id == REPO)
        .unwrap();
    let operation = health.operation.unwrap();
    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.failure, None);
    assert_eq!(operation.attempt_count, 0);
    assert!(operation.next_attempt_at.is_some());
    assert!(waiting(&store, 'a').is_empty() && waiting(&store, 'b').is_empty());
}

#[test]
fn a_replaced_secondary_agent_reviews_the_head_its_predecessor_reviewed() {
    let (_root, store, mut monitor) = configured(&[], &["docs/"]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    complete_teammate(&store, NOW + 2);
    // The teammate assignment now runs another Agent.
    let replacement = "aaaaaaaa-aaaa-4aaa-8aaa-000000000003";
    let mut settings = store.load_settings().unwrap();
    let mut agent = serde_json::to_value(&settings.agents[1]).unwrap();
    agent["id"] = json!(replacement);
    agent["name"] = json!("Docs Reviewer 2");
    settings.agents.push(serde_json::from_value(agent).unwrap());
    settings.repositories[0].assignments[1].agent_id = replacement.into();
    store.save_settings(&settings).unwrap();
    monitor
        .synchronize_configuration(&store, &Default::default(), NOW + 5)
        .unwrap();
    scan(&mut monitor, &store, &wire, 'a', NOW + 10);
    let replaced = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|j| j.work.as_ref().is_some_and(|w| w.agent_id == replacement))
        .expect("the new Agent has work");
    assert_eq!(replaced.waiting, WAITING_AI_CAPACITY);
    assert!(primary_blocked(&store, NOW + 20));
}

#[test]
fn turning_on_review_drafts_mid_poll_never_bypasses_the_gate() {
    let (_root, store, mut monitor) = configured(&["src/"], &[]);
    let wire = Wire::default();
    wire.head('a', &["docs/a.md"]);
    let mut draft = pull('a');
    draft.draft = true;
    let (ticket, result) = read(&mut monitor, &store, &wire, draft, NOW);
    assert!(result.as_ref().unwrap().facts.is_empty(), "drafts unread");
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].review_drafts = true;
    store.save_settings(&settings).unwrap();
    let _ = monitor.finish(&store, ticket, result, NOW + 1);
    assert!(waiting(&store, 'a').is_empty());
}

/// Dispatches `assignment`'s queued entry for `head` as a run.
fn started(store: &Store, assignment: &str, head: char, now: i64) -> ReviewRun {
    let job = store
        .load_queue()
        .unwrap()
        .into_iter()
        .find(|j| {
            j.assignment_id.as_deref() == Some(assignment)
                && j.head_sha == sha(head)
                && j.waiting == WAITING_AI_CAPACITY
        })
        .unwrap();
    let key = key(&job, assignment);
    request(store, &key, true, now).unwrap();
    prepare_dispatch(store, &key, now).unwrap().unwrap()
}

/// The review read of `head` changing `files`, each with a one-line patch.
fn review_context(head: char, files: &[&str]) -> crate::provider::ReviewContext {
    let tree: BTreeMap<String, crate::provider::TreeEntry> = files
        .iter()
        .map(|path| {
            (
                path.to_string(),
                crate::provider::TreeEntry {
                    path: path.to_string(),
                    mode: "100644".into(),
                    sha: sha('c'),
                    kind: "blob".into(),
                },
            )
        })
        .collect();
    crate::provider::ReviewContext {
        base_revision: sha('f'),
        pull: PullRequest {
            files: files
                .iter()
                .map(|path| crate::provider::ChangedFile {
                    path: path.to_string(),
                    previous_path: None,
                    status: "modified".into(),
                    additions: 1,
                    deletions: 1,
                    changes: 2,
                    sha: sha('c'),
                })
                .collect(),
            ..pull(head)
        },
        files: files
            .iter()
            .map(|path| crate::provider::ReviewFile {
                path: path.to_string(),
                previous_path: None,
                status: "modified".into(),
                patch: Some("@@ -1 +1 @@\n-old\n+new".into()),
            })
            .collect(),
        head: tree.clone(),
        base: tree,
    }
}

fn changed(context: &crate::provider::ReviewContext) -> Vec<String> {
    let files: Vec<_> = context.files.iter().map(|f| f.path.clone()).collect();
    let listed: Vec<_> = context.pull.files.iter().map(|f| f.path.clone()).collect();
    let packet: Vec<_> = crate::packet::reads::files(context)
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(files, listed);
    assert_eq!(files, packet);
    files
}

fn comment_on(path: &str) -> crate::exits::ExitCall {
    crate::exits::ExitCall::LeaveComments {
        new_comments: vec![crate::exits::NewComment {
            path: path.into(),
            line: 1,
            side: None,
            body: "Defect.".into(),
        }],
        reply_comments: vec![],
        resolve_own_threads: vec![],
    }
}

#[test]
fn an_exclude_only_secondary_reviews_and_comments_only_inside_its_paths() {
    let (_root, store, mut monitor) = configured(&[], &["!**/*.md"]);
    let wire = Wire::default();
    let files = ["README.md", "docs/b.md", "src/a.ts"];
    wire.head('a', &files);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(waiting(&store, 'a')[TEAMMATE], WAITING_AI_CAPACITY);

    let teammate = started(&store, TEAMMATE, 'a', NOW + 2);
    let scoped = packet_run::scoped(&teammate, review_context('a', &files));
    assert_eq!(changed(&scoped), ["src/a.ts"]);
    // Read-only context: read_source still reaches out-of-scope files.
    assert!(scoped.head.contains_key("docs/b.md"));
    let scope = packet_run::scope(&teammate);
    assert_eq!(scope, ["!**/*.md"]);
    let refusal = packet_run::out_of_scope(scope, &scoped.files, &comment_on("docs/b.md"));
    assert!(refusal.unwrap().starts_with("docs/b.md outside"));
    assert_eq!(
        packet_run::out_of_scope(scope, &scoped.files, &comment_on("src/a.ts")),
        None
    );
    let finding = crate::exits::ExitCall::SubmitReport {
        report: crate::exits::TeammateReport {
            findings: vec![crate::exits::ReportFinding {
                file: "README.md".into(),
                line: 1,
                nano: "Typo.".into(),
                full: "Typo.".into(),
                fix: "Fix it.".into(),
            }],
        },
    };
    assert!(packet_run::out_of_scope(scope, &scoped.files, &finding).is_some());
    complete(&store, &teammate.operation.id, Ok(report()), NOW + 3).unwrap();

    // The primary is unaffected: every changed file, no scope.
    let primary = started(&store, PRIMARY, 'a', NOW + 4);
    assert!(packet_run::scope(&primary).is_empty());
    let all = packet_run::scoped(&primary, review_context('a', &files));
    assert_eq!(changed(&all), files);
    assert_eq!(
        packet_run::out_of_scope(
            packet_run::scope(&primary),
            &all.files,
            &comment_on("docs/b.md")
        ),
        None
    );
}

#[test]
fn a_rename_into_scope_keeps_its_old_name_addressable() {
    let (_root, store, mut monitor) = configured(&[], &["src/"]);
    let wire = Wire::default();
    wire.head('a', &["src/a.ts"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    let teammate = started(&store, TEAMMATE, 'a', NOW + 2);
    let mut context = review_context('a', &["src/a.ts", "docs/b.md"]);
    context.files[0].previous_path = Some("lib/a.ts".into());
    let scoped = packet_run::scoped(&teammate, context);
    assert_eq!(
        scoped
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["src/a.ts"]
    );
    let scope = packet_run::scope(&teammate);
    assert_eq!(
        packet_run::out_of_scope(scope, &scoped.files, &comment_on("lib/a.ts")),
        None
    );
}

#[test]
fn an_exclude_only_secondary_skips_a_markdown_only_pr() {
    let (_root, store, mut monitor) = configured(&[], &["!**/*.md"]);
    let wire = Wire::default();
    wire.head('a', &["README.md", "docs/b.md"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    assert_eq!(
        waiting(&store, 'a'),
        BTreeMap::from([
            (PRIMARY.into(), WAITING_AI_CAPACITY.into()),
            (TEAMMATE.into(), WAITING_SKIPPED_NO_MATCHING_FILES.into()),
        ])
    );
}

#[test]
fn an_incremental_secondary_review_is_scoped_too() {
    let (_root, store, mut monitor) = configured(&[], &["!**/*.md"]);
    let wire = Wire::default();
    wire.head('a', &["docs/b.md", "src/a.ts"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    complete_teammate(&store, NOW + 2);

    // Only Markdown since the teammate's review of a: skipped.
    wire.head('b', &["docs/b.md", "docs/c.md", "src/a.ts"])
        .compare('a', 'b', "ahead", &["docs/c.md"]);
    scan(&mut monitor, &store, &wire, 'b', NOW + 10);
    assert_eq!(
        waiting(&store, 'b')[TEAMMATE],
        WAITING_SKIPPED_NO_MATCHING_FILES
    );

    // Code since then: it reviews, and still sees no Markdown.
    let files = ["docs/b.md", "docs/c.md", "src/a.ts", "src/d.ts"];
    wire.head('c', &files)
        .compare('a', 'c', "ahead", &["docs/c.md", "src/d.ts"]);
    scan(&mut monitor, &store, &wire, 'c', NOW + 20);
    assert_eq!(waiting(&store, 'c')[TEAMMATE], WAITING_AI_CAPACITY);
    let teammate = started(&store, TEAMMATE, 'c', NOW + 22);
    let scoped = packet_run::scoped(&teammate, review_context('c', &files));
    assert_eq!(changed(&scoped), ["src/a.ts", "src/d.ts"]);
}

#[test]
fn changing_a_secondarys_paths_makes_its_run_stale_but_not_the_primarys() {
    let (_root, store, mut monitor) = configured(&[], &["!**/*.md"]);
    let wire = Wire::default();
    wire.head('a', &["docs/b.md", "src/a.ts"]);
    scan(&mut monitor, &store, &wire, 'a', NOW);
    let teammate = started(&store, TEAMMATE, 'a', NOW + 2);
    crate::review::validate_execution_selection(&store, &teammate).unwrap();
    let save = |paths: &[&str]| {
        let mut settings = store.load_settings().unwrap();
        let assignment = settings.repositories[0]
            .assignments
            .iter_mut()
            .find(|a| a.id == TEAMMATE)
            .unwrap();
        assignment.paths = paths.iter().map(|p| p.to_string()).collect();
        store.save_settings(&settings).unwrap();
    };
    save(&["!**/*.md", "!docs/"]);
    let stale = crate::review::validate_execution_selection(&store, &teammate).unwrap_err();
    assert_eq!(
        stale.message,
        "Agent configuration changed; explicitly retry."
    );
    save(&["!**/*.md"]);
    crate::review::validate_execution_selection(&store, &teammate).unwrap();
    complete(&store, &teammate.operation.id, Ok(report()), NOW + 3).unwrap();

    // A sibling's Paths never touch the primary's run.
    let primary = started(&store, PRIMARY, 'a', NOW + 4);
    save(&["src/"]);
    crate::review::validate_execution_selection(&store, &primary).unwrap();
}
