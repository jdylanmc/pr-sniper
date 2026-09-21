mod support;

use pr_sniper_lib::github::provider::{
    Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository, Response,
    Transport,
};
use pr_sniper_lib::github::{ConnectionError, Identity};
use pr_sniper_lib::monitoring::{Monitor, PollResult};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use support::Fixture;

const OPEN_FIRST: &str =
    "/repos/example/project/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1";
const OPEN_SECOND: &str =
    "/repos/example/project/pulls?state=open&sort=updated&direction=desc&per_page=100&page=2";

fn pull_request(number: u64) -> Value {
    json!({
        "id": 1000 + number,
        "number": number,
        "title": "A polling candidate",
        "user": {"id": 42, "login": "renamed-author"},
        "state": "open",
        "draft": false,
        "merged": false,
        "merged_at": null,
        "head": {
            "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "repo": {"id": 900}
        },
        "base": {
            "sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "repo": {"id": 900}
        },
        "updated_at": "2026-09-20T20:00:00Z",
        "changed_files": 1,
        "requested_reviewers": [{"id": 7, "login": "reviewer"}],
        "requested_teams": []
    })
}

struct PollTransport {
    requests: RefCell<Vec<String>>,
}

impl Transport for &PollTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.requests.borrow_mut().push(path.into());
        let (body, next) = match path {
            OPEN_FIRST => (json!([pull_request(31)]), Some(OPEN_SECOND)),
            OPEN_SECOND => (json!([pull_request(32)]), None),
            "/repos/example/project/pulls?state=all&sort=created&direction=asc&per_page=100&page=1" => {
                (json!([pull_request(31), pull_request(32)]), None)
            }
            "/repos/example/project/pulls/31" => (pull_request(31), None),
            "/repos/example/project/pulls/32" => (pull_request(32), None),
            "/repos/example/project/pulls/31/requested_reviewers"
            | "/repos/example/project/pulls/32/requested_reviewers" => (
                json!({"users": [{"id": 7, "login": "reviewer"}], "teams": []}),
                None,
            ),
            "/repos/example/project/pulls/31/files?per_page=100&page=1"
            | "/repos/example/project/pulls/32/files?per_page=100&page=1" => (
                json!([{
                    "filename": "src/main.rs",
                    "status": "modified",
                    "additions": 1,
                    "deletions": 1,
                    "changes": 2,
                    "sha": "cccccccccccccccccccccccccccccccccccccccc"
                }]),
                None,
            ),
            _ => panic!("unexpected provider read: {path}"),
        };
        let mut headers = BTreeMap::new();
        if let Some(next) = next {
            headers.insert(
                "link".into(),
                format!("<https://api.github.com{next}>; rel=\"next\""),
            );
        }
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

fn repository() -> RemoteRepository {
    RemoteRepository {
        id: "900".into(),
        name: "example/project".into(),
    }
}

struct AliasTransport {
    requests: RefCell<Vec<String>>,
    target: String,
}

impl Transport for &AliasTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.requests.borrow_mut().push(path.into());
        let (body, headers) = match path {
            OPEN_FIRST => (
                json!([pull_request(31)]),
                BTreeMap::from([(
                    "link".into(),
                    format!("<{}>; rel=\"next\", <{}>; rel=\"last\"", self.target, self.target),
                )]),
            ),
            OPEN_SECOND => (
                json!([pull_request(32)]),
                BTreeMap::from([(
                    "link".into(),
                    "<https://api.github.com/repositories/900/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1>; rel=\"prev\", <https://api.github.com/repositories/900/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1>; rel=\"first\"".into(),
                )]),
            ),
            _ => panic!("unexpected endpoint: {path}"),
        };
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn polling_accepts_pagination_alias_for_the_verified_repository_id() {
    let transport = AliasTransport {
        requests: RefCell::new(Vec::new()),
        target: "https://api.github.com/repositories/900/pulls?state=open&sort=updated&direction=desc&per_page=100&page=2".into(),
    };
    let pulls = GithubClient::new(&transport)
        .poll_pull_requests(&repository())
        .unwrap();
    assert_eq!(
        pulls.iter().map(|pull| pull.number).collect::<Vec<_>>(),
        [31, 32]
    );
    assert_eq!(
        transport.requests.borrow().as_slice(),
        [OPEN_FIRST, OPEN_SECOND, OPEN_SECOND, OPEN_FIRST]
    );
}

#[test]
fn polling_rejects_unverified_or_endpoint_changing_numeric_aliases() {
    let valid = "https://api.github.com/repositories/900/pulls?state=open&sort=updated&direction=desc&per_page=100&page=2";
    for target in [
        valid.replace("/900/", "/901/"),
        valid.replace("api.github.com", "example.com"),
        valid.replace("https:", "http:"),
        valid.replace("/pulls?", "/issues?"),
        valid.replace("/pulls?", "/pulls/31/files?"),
        valid.replace("state=open", "state=all"),
        valid.replace("sort=updated", "sort=created"),
        valid.replace("direction=desc", "direction=asc"),
        valid.replace("per_page=100", "per_page=50"),
        valid.replace("page=2", "page=3"),
        valid.replace("https://", "https://user:password@"),
        format!("{valid}#fragment"),
        format!("{valid}&state=open"),
    ] {
        let transport = AliasTransport {
            requests: RefCell::new(Vec::new()),
            target: target.clone(),
        };
        assert_eq!(
            GithubClient::new(&transport).poll_pull_requests(&repository()),
            Err(ConnectionError::IncompleteRead),
            "{target}"
        );
        assert_eq!(transport.requests.borrow().as_slice(), [OPEN_FIRST]);
    }
}

struct UnorderedTransport {
    pages: Vec<Vec<Value>>,
    requests: RefCell<Vec<String>>,
}

impl Transport for &UnorderedTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.requests.borrow_mut().push(path.into());
        let prefix = OPEN_FIRST.strip_suffix('1').unwrap();
        let page: usize = path.strip_prefix(prefix).unwrap().parse().unwrap();
        let mut headers = BTreeMap::new();
        if page < self.pages.len() {
            headers.insert(
                "link".into(),
                format!("<https://api.github.com{prefix}{}>; rel=\"next\"", page + 1),
            );
        }
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&self.pages[page - 1]).unwrap(),
        })
    }
}

fn updated_pull(number: u64, updated_at: &str) -> Value {
    let mut pull = pull_request(number);
    pull["updated_at"] = json!(updated_at);
    pull
}

#[test]
fn fresh_poll_accepts_stable_unordered_timestamps_within_a_page() {
    let transport = UnorderedTransport {
        pages: vec![vec![
            updated_pull(31, "2026-09-20T19:00:00Z"),
            updated_pull(32, "2026-09-20T21:00:00Z"),
        ]],
        requests: RefCell::new(Vec::new()),
    };
    let pulls = GithubClient::new(&transport)
        .poll_pull_requests(&repository())
        .unwrap();
    assert_eq!(
        pulls.iter().map(|pull| pull.number).collect::<Vec<_>>(),
        [31, 32]
    );
}

#[test]
fn incremental_poll_finds_newer_and_equal_items_after_an_entire_older_page() {
    let transport = UnorderedTransport {
        pages: vec![
            vec![updated_pull(31, "2026-09-20T19:00:00Z")],
            vec![
                updated_pull(32, "2026-09-20T21:00:00Z"),
                updated_pull(33, "2026-09-20T20:00:00Z"),
            ],
        ],
        requests: RefCell::new(Vec::new()),
    };
    let pulls = GithubClient::new(&transport)
        .poll_pull_requests_since(&repository(), Some("2026-09-20T20:00:00Z"))
        .unwrap();
    assert_eq!(
        pulls.iter().map(|pull| pull.number).collect::<Vec<_>>(),
        [32, 33]
    );
    assert_eq!(
        transport.requests.borrow().as_slice(),
        [OPEN_FIRST, OPEN_SECOND, OPEN_SECOND, OPEN_FIRST]
    );
}

#[test]
fn unordered_sweeps_apply_cursors_only_after_complete_enumeration() {
    let old = updated_pull(31, "2026-09-20T19:00:00Z");
    let equal = updated_pull(32, "2026-09-20T20:00:00Z");
    let newer = updated_pull(33, "2026-09-20T21:00:00Z");
    for pages in [
        vec![vec![old.clone(), newer.clone(), equal.clone()]],
        vec![vec![old.clone()], vec![newer.clone(), equal.clone()]],
        vec![vec![old.clone(), equal.clone()], vec![newer.clone()]],
        vec![vec![equal.clone()], vec![old, newer]],
    ] {
        for cursor in [None, Some("2026-09-20T20:00:00Z")] {
            let transport = UnorderedTransport {
                pages: pages.clone(),
                requests: RefCell::new(Vec::new()),
            };
            let pulls = GithubClient::new(&transport)
                .poll_pull_requests_since(&repository(), cursor)
                .unwrap();
            let mut numbers = pulls.iter().map(|pull| pull.number).collect::<Vec<_>>();
            numbers.sort();
            assert_eq!(
                numbers,
                if cursor.is_some() {
                    vec![32, 33]
                } else {
                    vec![31, 32, 33]
                }
            );
            assert_eq!(
                transport.requests.borrow().len(),
                if pages.len() == 1 { 1 } else { 4 }
            );
        }
    }
}

#[test]
fn cursor_filter_cannot_hide_duplicate_or_invalid_older_items() {
    let old = updated_pull(31, "2026-09-20T19:00:00Z");
    let mut wrong_repository = old.clone();
    wrong_repository["base"]["repo"]["id"] = json!(901);
    let mut wrong_timestamp = old.clone();
    wrong_timestamp["updated_at"] = json!("invalid");
    for (pages, error) in [
        (
            vec![vec![old.clone()], vec![old]],
            ConnectionError::IncompleteRead,
        ),
        (
            vec![vec![wrong_repository]],
            ConnectionError::RepositoryChanged,
        ),
        (
            vec![vec![wrong_timestamp]],
            ConnectionError::InvalidResponse,
        ),
    ] {
        let transport = UnorderedTransport {
            pages,
            requests: RefCell::new(Vec::new()),
        };
        assert_eq!(
            GithubClient::new(&transport)
                .poll_pull_requests_since(&repository(), Some("2026-09-20T20:00:00Z")),
            Err(error)
        );
    }
}

#[test]
fn polling_enumerates_all_open_pages_in_recently_updated_order() {
    let transport = PollTransport {
        requests: RefCell::new(Vec::new()),
    };
    let client = GithubClient::new(&transport);

    let result = client.poll_pull_requests(&repository()).unwrap();

    assert_eq!(
        result.iter().map(|pr| pr.number).collect::<Vec<_>>(),
        vec![31, 32]
    );
    let requests = transport.requests.borrow();
    assert_eq!(requests.first().map(String::as_str), Some(OPEN_FIRST));
    assert!(
        requests.iter().any(|path| path == OPEN_SECOND),
        "Polling must exhaust the second open-PR page"
    );
}

#[test]
fn polling_does_not_hydrate_changed_files_before_eligibility() {
    let transport = PollTransport {
        requests: RefCell::new(Vec::new()),
    };
    let client = GithubClient::new(&transport);

    let result = client.poll_pull_requests(&repository()).unwrap();

    assert_eq!(result.len(), 2);
    assert!(
        transport
            .requests
            .borrow()
            .iter()
            .all(|path| !path.contains("/files?")),
        "Polling must not spend changed-file reads on unfiltered candidates"
    );
    assert_eq!(result[0].author.as_ref().unwrap().id, "42");
    assert_eq!(result[0].requested_reviewers[0].id, "7");
}

struct CursorTransport {
    requests: RefCell<Vec<String>>,
    fail_second: bool,
}

impl Transport for &CursorTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.requests.borrow_mut().push(path.into());
        let mut older = pull_request(33);
        older["updated_at"] = json!("2026-09-20T19:59:59Z");
        let (body, next) = match path {
            OPEN_FIRST => {
                let mut newer = pull_request(31);
                newer["updated_at"] = json!("2026-09-20T20:01:00Z");
                (json!([newer, pull_request(32), older]), Some(OPEN_SECOND))
            }
            OPEN_SECOND if self.fail_second => return Err(ConnectionError::RateLimited),
            OPEN_SECOND => {
                let mut oldest = pull_request(34);
                oldest["updated_at"] = json!("2026-09-19T20:00:00Z");
                (json!([oldest]), None)
            }
            _ => panic!("incremental polling issued an unexpected GET: {path}"),
        };
        let mut headers = BTreeMap::new();
        if let Some(next) = next {
            headers.insert(
                "link".into(),
                format!("<https://api.github.com{next}>; rel=\"next\""),
            );
        }
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn incremental_poll_keeps_equal_timestamp_updates_but_excludes_older_revisions() {
    let transport = CursorTransport {
        requests: RefCell::new(Vec::new()),
        fail_second: false,
    };

    let result = GithubClient::new(&transport)
        .poll_pull_requests_since(&repository(), Some("2026-09-20T20:00:00Z"))
        .unwrap();

    assert_eq!(
        result.iter().map(|pr| pr.number).collect::<Vec<_>>(),
        vec![31, 32]
    );
    assert_eq!(
        transport.requests.borrow().as_slice(),
        &[OPEN_FIRST, OPEN_SECOND, OPEN_SECOND, OPEN_FIRST]
    );
}

#[test]
fn incremental_poll_cannot_ignore_a_failed_page_after_crossing_the_cursor() {
    let transport = CursorTransport {
        requests: RefCell::new(Vec::new()),
        fail_second: true,
    };

    let result = GithubClient::new(&transport)
        .poll_pull_requests_since(&repository(), Some("2026-09-20T20:00:00Z"));

    assert_eq!(result, Err(ConnectionError::RateLimited));
    assert_eq!(
        transport.requests.borrow().as_slice(),
        &[OPEN_FIRST, OPEN_SECOND]
    );
}

#[test]
fn initial_poll_never_returns_a_partial_success_when_a_later_page_fails() {
    let transport = CursorTransport {
        requests: RefCell::new(Vec::new()),
        fail_second: true,
    };

    let result = GithubClient::new(&transport).poll_pull_requests_since(&repository(), None);

    assert_eq!(result, Err(ConnectionError::RateLimited));
    assert_eq!(
        transport.requests.borrow().as_slice(),
        &[OPEN_FIRST, OPEN_SECOND]
    );
}

struct MovingOpenList {
    pulls: RefCell<Vec<Value>>,
    changed: std::cell::Cell<bool>,
    close_first: bool,
}

impl MovingOpenList {
    fn new(close_first: bool) -> Self {
        Self {
            pulls: RefCell::new(
                (1..=250)
                    .map(|number| {
                        let mut pull = pull_request(number);
                        let minute = 20 * 60 + 59 - number;
                        pull["updated_at"] = json!(format!(
                            "2026-09-20T{:02}:{:02}:00Z",
                            minute / 60,
                            minute % 60
                        ));
                        pull
                    })
                    .collect(),
            ),
            changed: std::cell::Cell::new(false),
            close_first,
        }
    }
}

impl Transport for &MovingOpenList {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let prefix =
            "/repos/example/project/pulls?state=open&sort=updated&direction=desc&per_page=100&page=";
        let page: usize = path.strip_prefix(prefix).unwrap().parse().unwrap();
        if page == 2 && !self.changed.replace(true) {
            let mut pulls = self.pulls.borrow_mut();
            if self.close_first {
                pulls.remove(0);
            } else {
                let mut updated = pulls.remove(100);
                updated["updated_at"] = json!("2026-09-20T21:01:00Z");
                pulls.insert(0, updated);
            }
        }
        let pulls = self.pulls.borrow();
        let start = (page - 1) * 100;
        let body = &pulls[start.min(pulls.len())..(start + 100).min(pulls.len())];
        let mut headers = BTreeMap::new();
        if page < 3 {
            headers.insert(
                "link".into(),
                format!(
                    "<https://api.github.com{prefix}{}>; rel=\"next\", <https://api.github.com{prefix}3>; rel=\"last\"",
                    page + 1
                ),
            );
        }
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(body).unwrap(),
        })
    }
}

#[test]
fn closing_a_first_page_pr_cannot_silently_skip_the_open_page_boundary() {
    let transport = MovingOpenList::new(true);

    let result = GithubClient::new(&transport).poll_pull_requests(&repository());

    match result {
        Ok(pulls) => assert!(
            pulls.iter().any(|pull| pull.number == 101),
            "PR 101 stayed open but shifted behind the offset after PR 1 closed"
        ),
        Err(error) => assert_eq!(error, ConnectionError::IncompleteRead),
    }
}

#[test]
fn a_pr_reordered_across_pages_is_complete_or_explicitly_incomplete() {
    let transport = MovingOpenList::new(false);

    let result = GithubClient::new(&transport).poll_pull_requests(&repository());

    match result {
        Ok(pulls) => assert!(
            pulls.iter().any(|pull| pull.number == 101),
            "Updated PR 101 moved into a page already read"
        ),
        Err(error) => assert_eq!(error, ConnectionError::IncompleteRead),
    }
}

#[test]
fn unstable_pages_preserve_the_durable_cursor_and_reopen_recovers_the_boundary_pr() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.add_repository("example/project").unwrap();
    let connected = |pull_requests| PollResult {
        connection: Connection {
            identity: Identity {
                id: "7".into(),
                login: "reviewer".into(),
            },
            repository: repository(),
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Unknown,
            },
        },
        pull_requests,
    };
    let mut monitor = Monitor::default();
    let ticket = monitor
        .prepare_checks(&store, 1000, true)
        .unwrap()
        .pop()
        .unwrap();
    let previous = GithubClient::new(&CursorTransport {
        requests: RefCell::new(Vec::new()),
        fail_second: false,
    })
    .poll_pull_requests_since(&repository(), Some("2026-09-20T20:01:00Z"))
    .unwrap();
    let mut previous = previous[0].clone();
    previous.id = "1900".into();
    previous.number = 900;
    previous.updated_at = "2026-09-19T00:00:00Z".into();
    monitor
        .finish(&store, ticket, Ok(connected(vec![previous])), 1001)
        .unwrap();
    let transport = MovingOpenList::new(true);
    let client = GithubClient::new(&transport);
    let ticket = monitor
        .prepare_checks(&store, 1100, true)
        .unwrap()
        .pop()
        .unwrap();

    let incomplete =
        client.poll_pull_requests_since(&repository(), ticket.updated_after.as_deref());

    assert_eq!(incomplete, Err(ConnectionError::IncompleteRead));
    monitor
        .finish(&store, ticket, incomplete.map(&connected), 1101)
        .unwrap();
    assert_eq!(monitor.snapshot()[0].last_success, Some(1001));
    assert_eq!(
        monitor.snapshot()[0].last_failure,
        Some(ConnectionError::IncompleteRead)
    );
    assert_eq!(fixture.store().load_queue().unwrap().len(), 1);
    let saved_health: Value =
        serde_json::from_slice(&std::fs::read(fixture.path().join("state/polling.json")).unwrap())
            .unwrap();
    assert_eq!(saved_health[0]["last_success"], 1001);
    drop(monitor);
    drop(store);
    let reopened = fixture.store();
    let mut restarted = Monitor::restore(&reopened).unwrap();
    let ticket = restarted
        .prepare_checks(&reopened, 1200, true)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        ticket.updated_after.as_deref(),
        Some("2026-09-19T00:00:00Z")
    );
    let stable = client.poll_pull_requests_since(&repository(), ticket.updated_after.as_deref());
    assert!(stable
        .as_ref()
        .unwrap()
        .iter()
        .any(|pull| pull.number == 101));
    restarted
        .finish(&reopened, ticket, stable.map(&connected), 1201)
        .unwrap();
    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 250);
    assert_eq!(jobs.iter().filter(|job| job.number == 101).count(), 1);
    assert!(!jobs.iter().any(|job| job.number == 1));
    let mut restored_again = Monitor::restore(&fixture.store()).unwrap();
    let ticket = restored_again
        .prepare_checks(&reopened, 1300, true)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        ticket.updated_after.as_deref(),
        Some("2026-09-20T20:57:00Z")
    );
    let repeated = client.poll_pull_requests_since(&repository(), ticket.updated_after.as_deref());
    restored_again
        .finish(&reopened, ticket, repeated.map(connected), 1301)
        .unwrap();
    assert_eq!(fixture.store().load_queue().unwrap().len(), 250);
}

#[test]
fn unordered_recovery_preserves_failed_cursor_and_deduplicates_after_restart() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.add_repository("example/project").unwrap();
    let connected = |pull_requests| PollResult {
        connection: Connection {
            identity: Identity {
                id: "7".into(),
                login: "reviewer".into(),
            },
            repository: repository(),
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Unknown,
            },
        },
        pull_requests,
    };
    let seed = UnorderedTransport {
        pages: vec![vec![pull_request(31)]],
        requests: RefCell::new(Vec::new()),
    };
    let mut monitor = Monitor::default();
    let ticket = monitor
        .prepare_checks(&store, 1000, true)
        .unwrap()
        .pop()
        .unwrap();
    monitor
        .finish(
            &store,
            ticket,
            GithubClient::new(&seed)
                .poll_pull_requests(&repository())
                .map(&connected),
            1001,
        )
        .unwrap();
    let before_cursor = std::fs::read(fixture.path().join("state/poll-cursors.json")).unwrap();
    let before_queue = std::fs::read(fixture.path().join("state/queue.json")).unwrap();
    let old = updated_pull(32, "2026-09-20T19:00:00Z");
    let incomplete = UnorderedTransport {
        pages: vec![vec![old.clone()], vec![old.clone()]],
        requests: RefCell::new(Vec::new()),
    };
    let ticket = monitor
        .prepare_checks(&store, 1100, true)
        .unwrap()
        .pop()
        .unwrap();
    let result = GithubClient::new(&incomplete)
        .poll_pull_requests_since(&repository(), ticket.updated_after.as_deref());
    assert_eq!(result, Err(ConnectionError::IncompleteRead));
    monitor
        .finish(&store, ticket, result.map(&connected), 1101)
        .unwrap();
    assert_eq!(monitor.snapshot()[0].last_success, Some(1001));
    assert_eq!(
        monitor.snapshot()[0].last_failure,
        Some(ConnectionError::IncompleteRead)
    );
    assert_eq!(
        std::fs::read(fixture.path().join("state/poll-cursors.json")).unwrap(),
        before_cursor
    );
    assert_eq!(
        std::fs::read(fixture.path().join("state/queue.json")).unwrap(),
        before_queue
    );

    let stable = UnorderedTransport {
        pages: vec![
            vec![old],
            vec![updated_pull(34, "2026-09-20T21:00:00Z"), pull_request(33)],
        ],
        requests: RefCell::new(Vec::new()),
    };
    for (now, cursor) in [
        (1200, "2026-09-20T20:00:00Z"),
        (1300, "2026-09-20T21:00:00Z"),
    ] {
        let reopened = fixture.store();
        let mut restarted = Monitor::restore(&reopened).unwrap();
        let ticket = restarted
            .prepare_checks(&reopened, now, true)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(ticket.updated_after.as_deref(), Some(cursor));
        let pulls = GithubClient::new(&stable)
            .poll_pull_requests_since(&repository(), ticket.updated_after.as_deref());
        restarted
            .finish(&reopened, ticket, pulls.map(&connected), now + 1)
            .unwrap();
        let jobs = reopened.load_queue().unwrap();
        let mut numbers = jobs.iter().map(|job| job.number).collect::<Vec<_>>();
        numbers.sort();
        assert_eq!(numbers, [31, 33, 34]);
        assert_eq!(restarted.snapshot()[0].last_success, Some(now + 1));
        assert_eq!(restarted.snapshot()[0].last_failure, None);
    }
}
