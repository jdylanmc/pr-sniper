use super::*;
use crate::admission::AdmissionConfig;
use crate::github::provider::Response;
use crate::provider::ReviewContext;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

const ME: &str = "22";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE_BASE: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn repo() -> RemoteRepository {
    RemoteRepository {
        id: "100".into(),
        name: "example/repo".into(),
    }
}

fn sha(n: u64) -> String {
    format!("{n:040x}")
}

fn login(id: u64) -> String {
    if id == 22 {
        "me".into()
    } else {
        format!("user{id}")
    }
}

fn pr(number: u64, author: u64, reviewers: &[u64]) -> Value {
    json!({
        "id": 100_000 + number, "number": number, "title": format!("Change {number}"),
        "user": {"id": author, "login": login(author)},
        "requested_reviewers": reviewers.iter()
            .map(|id| json!({"id": id, "login": login(*id)})).collect::<Vec<_>>(),
        "requested_teams": [],
        "state": "open", "draft": false,
        "head": {"sha": HEAD, "repo": {"id": 100}},
        "base": {"sha": BASE, "repo": {"id": 100}},
        "updated_at": "2026-11-01T00:00:00Z", "changed_files": 1
    })
}

fn response(status: u16, value: Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}

/// A fault served once to the first read whose path contains `at`.
struct Fault {
    at: String,
    response: Response,
}

#[derive(Default)]
struct Hub {
    open: BTreeMap<u64, Value>,
    closed: BTreeMap<u64, Value>,
    users: BTreeMap<u64, String>,
    /// Search reports this many results, whatever matched.
    total: Option<u64>,
    faults: VecDeque<Fault>,
    /// After serving a listing page, these pull requests open (newest).
    opened_after: BTreeMap<u64, Vec<Value>>,
    /// After serving a listing page, these pull requests close.
    closed_after: BTreeMap<u64, Vec<u64>>,
    trees: BTreeMap<String, Value>,
    files: Vec<Value>,
    changed_files: u64,
    log: Vec<String>,
}

struct Fake(Mutex<Hub>);

impl Fake {
    fn new(open: impl IntoIterator<Item = Value>) -> Self {
        let mut hub = Hub::default();
        for value in open {
            hub.open.insert(value["number"].as_u64().unwrap(), value);
        }
        for id in [22, 30, 31, 32, 33, 34, 35, 36] {
            hub.users.insert(id, login(id));
        }
        Self(Mutex::new(hub))
    }

    fn hub(&self) -> std::sync::MutexGuard<'_, Hub> {
        self.0.lock().unwrap()
    }

    fn log(&self) -> Vec<String> {
        std::mem::take(&mut self.hub().log)
    }

    fn fault(&self, at: &str, response: Response) {
        self.hub().faults.push_back(Fault {
            at: at.into(),
            response,
        });
    }
}

fn page_of(path: &str) -> u64 {
    path.rsplit("page=").next().unwrap().parse().unwrap()
}

/// GitHub's issue search over open pull requests: any qualifier matches.
fn search(hub: &Hub, path: &str) -> Value {
    let query = path.split_once('?').unwrap().1;
    let pairs: BTreeMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let q = &pairs["q"];
    assert!(q.starts_with("repo:example/repo is:pr is:open "), "{q}");
    let terms: Vec<&str> = q
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|term| term.contains(':') && !term.starts_with("repo:") && !term.starts_with("is:"))
        .collect();
    assert_eq!(
        pairs.contains_key("advanced_search"),
        terms.len() > 1,
        "OR needs advanced search"
    );
    let id_of = |name: &str| {
        hub.users
            .iter()
            .find(|(_, login)| login.as_str() == name)
            .map(|(id, _)| *id)
    };
    let matches = |pull: &Value| {
        terms
            .iter()
            .any(|term| match term.split_once(':').unwrap() {
                ("author", name) => id_of(name) == pull["user"]["id"].as_u64(),
                ("review-requested", name) => pull["requested_reviewers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reviewer| id_of(name) == reviewer["id"].as_u64()),
                (other, _) => panic!("unexpected qualifier {other}"),
            })
    };
    let hits: Vec<u64> = hub
        .open
        .values()
        .rev()
        .filter(|pull| matches(pull))
        .map(|pull| pull["number"].as_u64().unwrap())
        .collect();
    let page: usize = pairs["page"].parse().unwrap();
    let items: Vec<Value> = hits
        .iter()
        .skip((page - 1) * 100)
        .take(100)
        .map(|number| json!({"number": number, "pull_request": {}}))
        .collect();
    json!({
        "total_count": hub.total.unwrap_or(hits.len() as u64),
        "incomplete_results": false,
        "items": items
    })
}

const LISTING: &str =
    "/repos/example/repo/pulls?state=open&sort=created&direction=desc&per_page=100&page=";

impl Transport for Fake {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let mut hub = self.hub();
        hub.log.push(path.to_owned());
        if let Some(index) = hub.faults.iter().position(|fault| path.contains(&fault.at)) {
            return Ok(hub.faults.remove(index).unwrap().response);
        }
        if let Some(id) = path.strip_prefix("/user/") {
            let id: u64 = id.parse().unwrap();
            return Ok(match hub.users.get(&id) {
                Some(login) => response(200, json!({"id": id, "login": login})),
                None => response(404, json!({"message": "Not Found"})),
            });
        }
        if path.starts_with(LISTING) {
            let page = page_of(path);
            let values: Vec<Value> = hub
                .open
                .values()
                .rev()
                .skip(((page - 1) * 100) as usize)
                .take(100)
                .cloned()
                .collect();
            for value in hub.opened_after.remove(&page).unwrap_or_default() {
                hub.open.insert(value["number"].as_u64().unwrap(), value);
            }
            for number in hub.closed_after.remove(&page).unwrap_or_default() {
                hub.open.remove(&number);
            }
            return Ok(response(200, Value::Array(values)));
        }
        if path.starts_with("/search/issues?") {
            return Ok(response(200, search(&hub, path)));
        }
        if let Some(rest) = path.strip_prefix("/repos/example/repo/pulls/") {
            if let Ok(number) = rest.parse::<u64>() {
                let value = hub.open.get(&number).cloned().or_else(|| {
                    hub.closed.get(&number).map(|value| {
                        let mut value = value.clone();
                        value["state"] = json!("closed");
                        value["merged"] = json!(false);
                        value
                    })
                });
                return Ok(match value {
                    Some(mut value) => {
                        value["changed_files"] = json!(hub.changed_files.max(1));
                        response(200, value)
                    }
                    None => response(404, json!({"message": "Not Found"})),
                });
            }
            if rest.starts_with("7/files?per_page=100&page=") {
                let page = page_of(path);
                let files = hub
                    .files
                    .iter()
                    .skip(((page - 1) * 100) as usize)
                    .take(100)
                    .cloned()
                    .collect();
                let mut out = response(200, Value::Array(files));
                if page * 100 < hub.files.len() as u64 {
                    out.headers.insert(
                        "link".into(),
                        format!(
                            "<https://api.github.com/repos/example/repo/pulls/7/files?per_page=100&page={}>; rel=\"next\"",
                            page + 1
                        ),
                    );
                }
                return Ok(out);
            }
        }
        if path == format!("/repos/example/repo/compare/{BASE}...{HEAD}") {
            return Ok(response(
                200,
                json!({"merge_base_commit": {"sha": MERGE_BASE}}),
            ));
        }
        if let Some(rest) = path.strip_prefix("/repos/example/repo/git/trees/") {
            // GitHub truncates a very large recursive tree.
            if rest.ends_with("?recursive=1") {
                return Ok(response(200, json!({"truncated": true, "tree": []})));
            }
            if let Some(tree) = hub.trees.get(rest) {
                return Ok(response(200, tree.clone()));
            }
        }
        panic!("unexpected read: {path}");
    }
}

fn admission(value: Value) -> AdmissionConfig {
    serde_json::from_value(value).unwrap()
}

fn identity(id: u64) -> Value {
    json!({"id": id.to_string(), "login": login(id)})
}

fn batch(
    client: &GithubClient<Fake>,
    scope: &ListingScope,
    cursor: Option<&ListingCursor>,
    tracked: &[(String, u64)],
) -> Result<PolledPullRequests, ConnectionError> {
    client.listing_batch(&repo(), tracked, &[], scope, cursor, 1_000)
}

/// Polls until the pass `cursor` is in ends: what it listed, the cursor
/// after it, and how many polls it took.
fn whole_pass(
    client: &GithubClient<Fake>,
    scope: &ListingScope,
    mut cursor: Option<ListingCursor>,
) -> (BTreeMap<u64, PullRequest>, Option<ListingCursor>, usize) {
    let passes = cursor.as_ref().map_or(0, |cursor| cursor.passes);
    let mut listed = BTreeMap::new();
    for polls in 1..100 {
        let polled = batch(client, scope, cursor.as_ref(), &[]).unwrap();
        for pull in polled.pulls {
            listed.insert(pull.number, pull);
        }
        cursor = polled.cursor;
        if cursor.as_ref().is_none_or(|cursor| cursor.passes > passes) {
            return (listed, cursor, polls);
        }
    }
    panic!("the pass never ended");
}

fn listing_reads(log: &[String]) -> usize {
    log.iter().filter(|path| path.starts_with(LISTING)).count()
}

fn search_reads(log: &[String]) -> usize {
    log.iter()
        .filter(|path| path.starts_with("/search/issues?"))
        .count()
}

fn numbers(pulls: &[PullRequest]) -> Vec<u64> {
    pulls.iter().map(|pull| pull.number).collect()
}

#[test]
fn include_users_and_assigned_search_only_candidates_and_admit_exactly_the_same() {
    let wire = Fake::new((1..=1_500).map(|n| {
        let reviewers: &[u64] = if n % 10 == 0 { &[22] } else { &[] };
        pr(n, 30 + n % 7, reviewers)
    }));
    let config = admission(json!({"mode": "filtered", "rules": [
        {"kind": "exclude_users", "enabled": true, "users": [identity(31)]},
        {"kind": "include_users", "enabled": true, "users": [identity(31), identity(32)]},
        {"kind": "assigned", "enabled": true},
        {"kind": "mentioned", "enabled": false},
    ]}));
    let scope = config.listing_scope(ME);
    let client = GithubClient::new(wire);
    let (listed, cursor, polls) = whole_pass(&client, &scope, None);
    let log = client.transport.log();
    assert_eq!(listing_reads(&log), 0, "no open listing");
    assert!(polls > 1, "a search pass is batched across polls");
    assert!(cursor.is_some_and(|cursor| cursor.passes == 1));
    let everything: Vec<PullRequest> = client
        .transport
        .hub()
        .open
        .values()
        .map(|value| polling_pull_request(value).unwrap())
        .collect();
    let admitted = |pulls: &mut dyn Iterator<Item = &PullRequest>| -> BTreeSet<u64> {
        pulls
            .filter(|pull| config.admits(ME, pull))
            .map(|pull| pull.number)
            .collect()
    };
    // `decide` over the narrowed listing admits exactly what it admits over
    // the whole repository.
    let expected = admitted(&mut everything.iter());
    assert!(!expected.is_empty());
    assert_eq!(admitted(&mut listed.values()), expected);
    assert!(listed.len() < everything.len() / 2);
}

#[test]
fn mentioning_me_lists_every_open_pull_request() {
    let client = GithubClient::new(Fake::new((1..=50).map(|n| pr(n, 30, &[]))));
    let scope = ListingScope::Narrowed {
        authors: vec!["31".into()],
        reviewer: None,
        mentions: true,
    };
    let polled = batch(&client, &scope, None, &[]).unwrap();
    assert_eq!(polled.pulls.len(), 50);
    let log = client.transport.log();
    assert_eq!((search_reads(&log), listing_reads(&log)), (0, 1));
}

#[test]
fn a_search_past_its_cap_falls_back_to_the_batched_listing_for_the_pass() {
    let client = GithubClient::new(Fake::new((1..=1_200).map(|n| pr(n, 31, &[]))));
    client.transport.hub().total = Some(1_200);
    let scope = ListingScope::Narrowed {
        authors: vec!["31".into()],
        reviewer: None,
        mentions: false,
    };
    let first = batch(&client, &scope, None, &[]).unwrap();
    let cursor = first.cursor.clone().unwrap();
    assert!(cursor.wide);
    let log = client.transport.log();
    assert_eq!(search_reads(&log), 1);
    assert_eq!(listing_reads(&log), 10);
    let (rest, cursor, _) = whole_pass(&client, &scope, Some(cursor));
    let all: BTreeSet<u64> = numbers(&first.pulls)
        .into_iter()
        .chain(rest.keys().copied())
        .collect();
    assert_eq!(all, (1..=1_200).collect());
    // The next pass tries the search again.
    let cursor = cursor.unwrap();
    assert!(!cursor.wide);
    {
        let mut hub = client.transport.hub();
        hub.total = None;
        hub.open.retain(|number, _| *number > 1_150);
        hub.log.clear();
    }
    let next = batch(&client, &scope, Some(&cursor), &[]).unwrap();
    assert!(next.cursor.is_some_and(|cursor| !cursor.wide));
    let log = client.transport.log();
    assert!(search_reads(&log) > 0);
    assert_eq!(listing_reads(&log), 0);
}

#[test]
fn a_user_the_search_cannot_name_falls_back_to_the_listing() {
    let client = GithubClient::new(Fake::new((1..=5).map(|n| pr(n, 31, &[]))));
    let scope = ListingScope::Narrowed {
        authors: vec!["99".into()],
        reviewer: None,
        mentions: false,
    };
    let polled = batch(&client, &scope, None, &[]).unwrap();
    assert_eq!(polled.pulls.len(), 5);
    assert_eq!(search_reads(&client.transport.log()), 0);
}

#[test]
fn all_mode_admits_by_batch_across_polls() {
    let client = GithubClient::new(Fake::new((1..=2_500).map(|n| pr(n, 30, &[]))));
    let first = batch(&client, &ListingScope::All, None, &[]).unwrap();
    assert_eq!(first.pulls.len(), 1_000, "ten pages a poll");
    assert_eq!(listing_reads(&client.transport.log()), 10);
    let cursor = first.cursor.clone().unwrap();
    assert_eq!((cursor.skip, cursor.passes), (1_000, 0));
    let (rest, cursor, polls) = whole_pass(&client, &ListingScope::All, Some(cursor));
    assert!(polls >= 2);
    let all: BTreeSet<u64> = numbers(&first.pulls)
        .into_iter()
        .chain(rest.keys().copied())
        .collect();
    assert_eq!(all, (1..=2_500).collect());
    assert_eq!(cursor.unwrap().passes, 1);
}

#[test]
fn a_throttled_page_is_the_polls_rate_limit_and_the_pass_resumes_from_the_committed_cursor() {
    let client = GithubClient::new(Fake::new((1..=3_000).map(|n| pr(n, 30, &[]))));
    let committed = batch(&client, &ListingScope::All, None, &[])
        .unwrap()
        .cursor
        .unwrap();
    client.transport.log();
    let mut limited = response(403, json!({"message": "API rate limit exceeded"}));
    limited
        .headers
        .insert("x-ratelimit-remaining".into(), "0".into());
    client.transport.fault("&page=14", limited);
    let error = batch(&client, &ListingScope::All, Some(&committed), &[]).unwrap_err();
    assert!(listing::throttled(error), "{error:?}");
    // Nothing committed: the next poll resumes where the last one ended.
    let resumed = batch(&client, &ListingScope::All, Some(&committed), &[]).unwrap();
    assert!(numbers(&resumed.pulls).contains(&(3_000 - 1_350)));
    let next = resumed.cursor.unwrap();
    assert!(next.skip > committed.skip);
    assert_eq!(next.stalls, 0);
}

#[test]
fn a_page_lost_mid_batch_keeps_the_pages_read_and_resumes_there() {
    let client = GithubClient::new(Fake::new((1..=3_000).map(|n| pr(n, 30, &[]))));
    client
        .transport
        .fault("&page=4", response(502, json!({"message": "Bad Gateway"})));
    let polled = batch(&client, &ListingScope::All, None, &[]).unwrap();
    assert_eq!(polled.pulls.len(), 300);
    let cursor = polled.cursor.unwrap();
    assert_eq!(
        (cursor.skip, cursor.stalls, cursor.retry_at),
        (300, 0, None)
    );
}

#[test]
fn a_shifting_listing_is_de_duplicated_and_what_it_skipped_is_read_next_pass() {
    let client = GithubClient::new(Fake::new((1..=350).map(|n| pr(n, 30, &[]))));
    {
        let mut hub = client.transport.hub();
        // Two open after page 1, so page 2 repeats two of page 1's; then
        // three of page 2's close after it, so page 3 skips one.
        hub.opened_after
            .insert(1, vec![pr(351, 30, &[]), pr(352, 30, &[])]);
        hub.closed_after.insert(2, vec![200, 201, 202]);
    }
    let polled = batch(&client, &ListingScope::All, None, &[]).unwrap();
    let listed = numbers(&polled.pulls);
    let unique: BTreeSet<u64> = listed.iter().copied().collect();
    assert_eq!(unique.len(), listed.len(), "no duplicate");
    let open: BTreeSet<u64> = client.transport.hub().open.keys().copied().collect();
    assert!(!open.is_subset(&unique), "this pass missed one");
    assert!(polled
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.passes == 1));
    let (next, _, _) = whole_pass(&client, &ListingScope::All, polled.cursor);
    let next: BTreeSet<u64> = next.keys().copied().collect();
    assert!(open.is_subset(&next), "the next pass lists it");
}

#[test]
fn explicit_lists_nothing_and_reads_tracked_pull_requests_on_their_own() {
    let client = GithubClient::new(Fake::new((1..=20).map(|n| pr(n, 30, &[]))));
    client.transport.hub().closed.insert(30, pr(30, 30, &[]));
    let explicit = ListingScope::Narrowed {
        authors: Vec::new(),
        reviewer: None,
        mentions: false,
    };
    let polled = client
        .listing_batch(
            &repo(),
            &[("100005".into(), 5)],
            &[("100030".into(), 30)],
            &explicit,
            None,
            1_000,
        )
        .unwrap();
    assert_eq!(numbers(&polled.pulls), [5]);
    assert_eq!(
        client.transport.log(),
        [
            "/repos/example/repo/pulls/5",
            "/repos/example/repo/pulls/30"
        ]
    );
}

#[test]
fn a_failing_listing_never_fails_a_watched_pull_request() {
    let client = GithubClient::new(Fake::new((1..=20).map(|n| pr(n, 30, &[]))));
    client
        .transport
        .fault("&page=1", response(503, json!({"message": "unavailable"})));
    let tracked = [("100005".to_string(), 5)];
    let polled = batch(&client, &ListingScope::All, None, &tracked).unwrap();
    assert_eq!(numbers(&polled.pulls), [5]);
    let cursor = polled.cursor.unwrap();
    assert_eq!(cursor.stalls, 1);
    assert!(cursor.retry_at.is_some_and(|at| at > 1_000));
    client.transport.log();
    // Backing off: the listing waits, the tracked pull request is read.
    let waiting = batch(&client, &ListingScope::All, Some(&cursor), &tracked).unwrap();
    assert_eq!(numbers(&waiting.pulls), [5]);
    assert_eq!(client.transport.log(), ["/repos/example/repo/pulls/5"]);
}

#[test]
fn malformed_entries_and_pages_skip_or_back_off_never_fail_the_poll() {
    let mut broken = pr(10, 30, &[]);
    broken["head"]["sha"] = json!("not a sha");
    let client = GithubClient::new(Fake::new((1..=9).map(|n| pr(n, 30, &[])).chain([broken])));
    let polled = batch(&client, &ListingScope::All, None, &[]).unwrap();
    assert_eq!(numbers(&polled.pulls), (1..=9).collect::<Vec<_>>());
    client
        .transport
        .fault("&page=1", response(200, json!({"message": "not a page"})));
    let polled = batch(&client, &ListingScope::All, None, &[]).unwrap();
    assert!(polled.pulls.is_empty());
    assert_eq!(polled.cursor.unwrap().stalls, 1);
}

#[test]
fn an_access_failure_on_the_listing_still_surfaces() {
    let client = GithubClient::new(Fake::new((1..=3).map(|n| pr(n, 30, &[]))));
    client.transport.fault("&page=1", response(401, json!({})));
    assert_eq!(
        batch(&client, &ListingScope::All, None, &[]),
        Err(ConnectionError::SignedOut)
    );
}

fn tree(entries: Vec<(String, &str, String)>) -> Value {
    json!({"truncated": false, "tree": entries.into_iter().map(|(path, kind, sha)| json!({
        "path": path, "mode": if kind == "tree" { "040000" } else { "100644" },
        "type": kind, "sha": sha
    })).collect::<Vec<_>>()})
}

#[test]
fn a_review_past_the_whole_tree_and_files_caps_reads_only_its_changed_paths() {
    let client = GithubClient::new(Fake::new([pr(7, 30, &[])]));
    {
        let mut hub = client.transport.hub();
        let count = 3_000;
        hub.changed_files = count + 1;
        hub.files = (0..count)
            .map(|i| {
                json!({"filename": format!("gen/f{i}"), "status": "modified",
                "sha": sha(10_000 + i), "patch": "@@ -1 +1 @@\n-a\n+b"})
            })
            .collect();
        let gen = |offset: u64| {
            tree(
                (0..count)
                    .map(|i| (format!("f{i}"), "blob", sha(offset + i)))
                    .collect(),
            )
        };
        let file = |name: &str, n: u64| (name.to_string(), "blob", sha(n));
        let dir = |name: &str, n: u64| (name.to_string(), "tree", sha(n));
        hub.trees.insert(sha(1), gen(20_000));
        hub.trees.insert(sha(2), gen(10_000));
        hub.trees.insert(sha(3), tree(vec![file("keep.rs", 5)]));
        hub.trees
            .insert(sha(4), tree(vec![file("keep.rs", 5), file("x.rs", 6)]));
        hub.trees.insert(
            MERGE_BASE.into(),
            tree(vec![dir("gen", 1), dir("same", 9), dir("src", 3)]),
        );
        hub.trees.insert(
            HEAD.into(),
            tree(vec![dir("gen", 2), dir("same", 9), dir("src", 4)]),
        );
    }
    let context: ReviewContext = client.review_context(&repo(), 7, HEAD).unwrap();
    assert_eq!(context.files.len(), 3_001);
    let unlisted = context.files.last().unwrap();
    assert_eq!(
        (unlisted.path.as_str(), unlisted.status.as_str()),
        ("src/x.rs", "added")
    );
    assert!(unlisted.patch.is_none());
    assert_eq!(context.head["src/x.rs"].sha, sha(6));
    assert_eq!(context.base["gen/f0"].sha, sha(20_000));
    assert!(
        !context.head.contains_key("src/keep.rs"),
        "unchanged: unread"
    );
    let log = client.transport.log();
    let unchanged = format!("/trees/{}", sha(9));
    assert!(log.iter().all(|path| !path.ends_with(&unchanged)));
    assert!(log
        .iter()
        .all(|path| path.contains("/pulls/7") || !path.contains("/pulls/")));
}
