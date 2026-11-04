//! GitHub changed-file listings for path globs (ADR 0027): complete lists
//! only, compare for incremental reads, retries never decide.

use pr_sniper_lib::github::provider::{GithubClient, RemoteRepository, Response, Transport};
use pr_sniper_lib::github::ConnectionError;
use pr_sniper_lib::provider::prelude::*;
use pr_sniper_lib::provider::{Lifecycle, Listing, PullRequest, Since, TreeDiff};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "/repos/octo-org/app";

/// (status, body, next link).
type Reply = (u16, Value, Option<String>);

#[derive(Default, Clone)]
struct Wire {
    /// Path → queued replies; the last reply repeats.
    replies: Arc<Mutex<BTreeMap<String, Vec<Reply>>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Wire {
    fn on(&self, path: &str, status: u16, body: Value, next: Option<String>) -> &Self {
        self.replies
            .lock()
            .unwrap()
            .entry(path.into())
            .or_default()
            .push((status, body, next));
        self
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.calls.lock().unwrap().push(path.into());
        let mut replies = self.replies.lock().unwrap();
        let queue = replies
            .get_mut(path)
            .unwrap_or_else(|| panic!("unexpected {path}"));
        let (status, body, next) = if queue.len() > 1 {
            queue.remove(0)
        } else {
            queue[0].clone()
        };
        let mut headers = BTreeMap::new();
        if let Some(next) = next {
            headers.insert(
                "link".into(),
                format!("<https://api.github.com{next}>; rel=\"next\""),
            );
        }
        if status == 429 {
            headers.insert("retry-after".into(), "30".into());
        }
        Ok(Response {
            status,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

fn detail(head: &str, changed: u64) -> Value {
    json!({"head": {"sha": head}, "changed_files": changed})
}

fn entry(name: &str) -> Value {
    json!({"filename": name, "status": "modified", "additions": 1, "deletions": 0,
        "changes": 1, "sha": "cccccccccccccccccccccccccccccccccccccccc"})
}

fn entries(from: usize, to: usize) -> Value {
    Value::Array((from..to).map(|n| entry(&format!("src/f{n}.rs"))).collect())
}

fn files_page(page: usize) -> String {
    format!("{BASE}/pulls/7/files?per_page=100&page={page}")
}

fn pull() -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "70".into(),
        number: 7,
        title: "t".into(),
        author: None,
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: HEAD.into(),
        base_sha: "d".repeat(40),
        head_repository_id: None,
        base_repository_id: "1".into(),
        updated_at: "2026-11-01T00:00:00Z".into(),
        files: vec![],
    }
}

fn repository() -> RemoteRepository {
    RemoteRepository {
        id: "1".into(),
        name: "octo-org/app".into(),
    }
}

fn listing(wire: &Wire, list: bool) -> Result<Listing, ConnectionError> {
    GithubClient::new(wire.clone()).changed_file_listing(&repository(), &pull(), list)
}

fn since(wire: &Wire, old: &str) -> Result<Since, ConnectionError> {
    GithubClient::new(wire.clone()).changed_files_since(&repository(), &pull(), old)
}

#[test]
fn whole_pull_request_files_page_to_the_end() {
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 200, detail(HEAD, 250), None)
        .on(&files_page(1), 200, entries(0, 100), Some(files_page(2)))
        .on(&files_page(2), 200, entries(100, 200), Some(files_page(3)))
        .on(&files_page(3), 200, entries(200, 250), None);
    let Ok(Listing::Complete(files)) = listing(&wire, true) else {
        panic!("complete")
    };
    assert_eq!(files.len(), 250);
    assert_eq!(files[249].path, "src/f249.rs");
    assert_eq!(wire.calls().len(), 4);
}

#[test]
fn visibility_alone_reads_only_the_count() {
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 200, detail(HEAD, 40), None);
    assert_eq!(listing(&wire, false), Ok(Listing::Visible));
    assert_eq!(wire.calls(), [format!("{BASE}/pulls/7")]);
}

#[test]
fn more_files_than_the_listing_reaches_is_incomplete_without_paging() {
    for list in [false, true] {
        let wire = Wire::default();
        wire.on(&format!("{BASE}/pulls/7"), 200, detail(HEAD, 3_001), None);
        assert_eq!(listing(&wire, list), Ok(Listing::Incomplete(3_001)));
        assert_eq!(wire.calls().len(), 1, "no pointless paging");
    }
}

#[test]
fn a_listing_short_of_the_count_on_the_same_head_is_incomplete() {
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 200, detail(HEAD, 120), None)
        .on(&files_page(1), 200, entries(0, 100), None);
    assert_eq!(listing(&wire, true), Ok(Listing::Incomplete(120)));
    // The same shortfall after a push raced the read: retried, not decided.
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 200, detail(HEAD, 120), None)
        .on(&format!("{BASE}/pulls/7"), 200, detail(OLD, 130), None)
        .on(&files_page(1), 200, entries(0, 100), None);
    assert_eq!(listing(&wire, true), Err(ConnectionError::RevisionChanged));
    // A moved head never lists.
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 200, detail(OLD, 1), None);
    assert_eq!(listing(&wire, false), Err(ConnectionError::RevisionChanged));
}

#[test]
fn rate_limits_and_failures_retry_instead_of_deciding() {
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 429, json!({}), None);
    assert!(matches!(
        listing(&wire, true),
        Err(ConnectionError::RateLimitedAfter(30))
    ));
    let wire = Wire::default();
    wire.on(&format!("{BASE}/pulls/7"), 200, detail(HEAD, 2), None)
        .on(&files_page(1), 502, json!({}), None);
    assert_eq!(listing(&wire, true), Err(ConnectionError::ProviderFailure));
    let wire = Wire::default();
    wire.on(
        &format!("{BASE}/compare/{OLD}...{HEAD}"),
        429,
        json!({}),
        None,
    );
    assert!(matches!(
        since(&wire, OLD),
        Err(ConnectionError::RateLimitedAfter(30))
    ));
}

#[test]
fn compare_is_incremental_only_when_the_old_head_is_an_ancestor() {
    let path = format!("{BASE}/compare/{OLD}...{HEAD}");
    let compared = |status: &str, files: Value| json!({"status": status, "files": files});
    let wire = Wire::default();
    wire.on(&path, 200, compared("ahead", entries(0, 2)), None);
    let Ok(Since::Files(files)) = since(&wire, OLD) else {
        panic!("incremental")
    };
    assert_eq!(files.len(), 2);
    // At the compare file cap the list may be cut: climb to a tree diff.
    let wire = Wire::default();
    wire.on(&path, 200, compared("ahead", entries(0, 300)), None);
    assert_eq!(since(&wire, OLD), Ok(Since::Capped));
    for (status, code, body) in [
        // Rebased or force-pushed: the old head is no ancestor.
        (200, "diverged", entries(0, 1)),
        (200, "behind", entries(0, 1)),
    ] {
        let wire = Wire::default();
        wire.on(&path, status, compared(code, body), None);
        assert_eq!(since(&wire, OLD), Ok(Since::WholePullRequest), "{code}");
    }
    for missing in [404, 422] {
        let wire = Wire::default();
        wire.on(
            &path,
            missing,
            json!({"message": "No common ancestor"}),
            None,
        );
        assert_eq!(since(&wire, OLD), Ok(Since::WholePullRequest));
    }
    let wire = Wire::default();
    wire.on(&path, 200, compared("identical", json!([])), None);
    assert_eq!(since(&wire, OLD), Ok(Since::Files(vec![])));
    // Never a path built from something that is not a commit.
    let wire = Wire::default();
    assert_eq!(since(&wire, "../main"), Ok(Since::WholePullRequest));
    assert!(wire.calls().is_empty());
}

const MERGE: &str = "9999999999999999999999999999999999999999";

fn blob(path: &str, sha: char) -> Value {
    json!({"path": path, "mode": "100644", "type": "blob", "sha": sha.to_string().repeat(40)})
}

fn dir(path: &str, sha: char) -> Value {
    json!({"path": path, "mode": "040000", "type": "tree", "sha": sha.to_string().repeat(40)})
}

fn tree(entries: Vec<Value>, truncated: bool) -> Value {
    json!({"sha": "f".repeat(40), "tree": entries, "truncated": truncated})
}

fn diff(wire: &Wire, from: &str, scope: &[&str]) -> Result<TreeDiff, ConnectionError> {
    let scope: Vec<String> = scope.iter().map(|s| s.to_string()).collect();
    GithubClient::new(wire.clone()).tree_diff(&repository(), from, HEAD, &scope)
}

fn paths(result: Result<TreeDiff, ConnectionError>) -> Vec<(String, String)> {
    match result {
        Ok(TreeDiff::Complete(files)) => files.into_iter().map(|f| (f.path, f.status)).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn merge_base_comes_from_comparing_base_and_head() {
    let wire = Wire::default();
    wire.on(
        &format!("{BASE}/compare/{}...{HEAD}?per_page=1", "d".repeat(40)),
        200,
        json!({"status": "diverged", "merge_base_commit": {"sha": MERGE}}),
        None,
    );
    assert_eq!(
        GithubClient::new(wire.clone()).merge_base(&repository(), &pull()),
        Ok(MERGE.to_string())
    );
}

#[test]
fn tree_diff_lists_every_changed_blob_or_mode() {
    let wire = Wire::default();
    wire.on(
        &format!("{BASE}/git/trees/{MERGE}?recursive=1"),
        200,
        tree(
            vec![
                dir("src", '1'),
                blob("src/a.rs", '2'),
                blob("gone.md", '3'),
                blob("same", '4'),
            ],
            false,
        ),
        None,
    )
    .on(
        &format!("{BASE}/git/trees/{HEAD}?recursive=1"),
        200,
        tree(
            vec![
                dir("src", '5'),
                blob("src/a.rs", '6'),
                json!({"path": "same", "mode": "100755", "type": "blob", "sha": "4".repeat(40)}),
                blob("new.rs", '7'),
            ],
            false,
        ),
        None,
    );
    assert_eq!(
        paths(diff(&wire, MERGE, &[])),
        [
            ("gone.md".into(), "removed".into()),
            ("new.rs".into(), "added".into()),
            ("same".into(), "modified".into()),
            ("src/a.rs".into(), "modified".into()),
        ]
    );
    // A truncated recursive tree is never a complete diff.
    let wire = Wire::default();
    wire.on(
        &format!("{BASE}/git/trees/{MERGE}?recursive=1"),
        200,
        tree(vec![], true),
        None,
    )
    .on(
        &format!("{BASE}/git/trees/{HEAD}?recursive=1"),
        200,
        tree(vec![], false),
        None,
    );
    assert_eq!(diff(&wire, MERGE, &[]), Ok(TreeDiff::Truncated));
}

#[test]
fn scoped_tree_diff_walks_only_differing_subtrees_under_the_prefixes() {
    let level = |sha: char| format!("{BASE}/git/trees/{}", sha.to_string().repeat(40));
    let wire = Wire::default();
    wire.on(
        &format!("{BASE}/git/trees/{MERGE}"),
        200,
        tree(
            vec![dir("src", '1'), dir("vendor", '2'), dir("docs", '3')],
            false,
        ),
        None,
    )
    .on(
        &format!("{BASE}/git/trees/{HEAD}"),
        200,
        // vendor/ changed but is out of scope; docs/ is unchanged.
        tree(
            vec![dir("src", '5'), dir("vendor", '6'), dir("docs", '3')],
            false,
        ),
        None,
    )
    .on(
        &level('1'),
        200,
        tree(vec![dir("gen", '7'), blob("a.rs", '8')], false),
        None,
    )
    .on(
        &level('5'),
        200,
        tree(
            vec![dir("gen", '7'), blob("a.rs", '9'), blob("b.rs", 'a')],
            false,
        ),
        None,
    );
    assert_eq!(
        paths(diff(&wire, MERGE, &["src/"])),
        [
            ("src/a.rs".into(), "modified".into()),
            ("src/b.rs".into(), "added".into()),
        ]
    );
    let read: Vec<_> = wire.calls();
    assert!(
        !read.contains(&level('2')) && !read.contains(&level('7')),
        "{read:?}"
    );
    // A cut-off level read is never complete.
    let wire = Wire::default();
    wire.on(
        &format!("{BASE}/git/trees/{MERGE}"),
        200,
        tree(vec![], true),
        None,
    )
    .on(
        &format!("{BASE}/git/trees/{HEAD}"),
        200,
        tree(vec![], false),
        None,
    );
    assert_eq!(diff(&wire, MERGE, &[""]), Ok(TreeDiff::Truncated));
}
