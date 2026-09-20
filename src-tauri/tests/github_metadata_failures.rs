use pr_sniper_lib::github::metadata::PullRequest;
use pr_sniper_lib::github::provider::{GithubClient, RemoteRepository, Response, Transport};
use pr_sniper_lib::github::ConnectionError;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};

const LIST: &str =
    "/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=1";
const LIST_NEXT: &str =
    "/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2";
const DETAIL: &str = "/repos/jdylanmc/pr-sniper/pulls/31";
const FILES: &str = "/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=1";
const FILES_NEXT: &str = "/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=2";
const REVIEWERS: &str = "/repos/jdylanmc/pr-sniper/pulls/31/requested_reviewers";

fn detail() -> Value {
    json!({
        "id": 1001,
        "number": 31,
        "title": "Preserve stable identities",
        "user": {"id": 6954990, "login": "author-renamed"},
        "state": "open",
        "draft": false,
        "merged": false,
        "merged_at": null,
        "head": {
            "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "repo": {"id": 1376547672}
        },
        "base": {
            "sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "repo": {"id": 1376547672}
        },
        "updated_at": "2026-09-20T20:00:00Z",
        "changed_files": 1
    })
}

fn file() -> Value {
    json!({
        "filename": "src/identity.rs",
        "status": "added",
        "additions": 12,
        "deletions": 0,
        "changes": 12,
        "sha": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    })
}

fn response(status: u16, body: Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&body).unwrap(),
    }
}

fn linked(body: Value, link: &str) -> Response {
    let mut response = response(200, body);
    response.headers.insert("link".into(), link.into());
    response
}

struct ScriptedTransport {
    replies: RefCell<BTreeMap<&'static str, VecDeque<Response>>>,
}

impl ScriptedTransport {
    fn reply(&mut self, path: &'static str, response: Response) {
        self.replies
            .get_mut()
            .insert(path, VecDeque::from([response]));
    }

    fn sequence(&mut self, path: &'static str, responses: Vec<Response>) {
        self.replies.get_mut().insert(path, responses.into());
    }
}

impl Transport for ScriptedTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let mut replies = self.replies.borrow_mut();
        let Some(sequence) = replies.get_mut(path) else {
            return Ok(response(404, json!({"message": "Missing fixture page"})));
        };
        if sequence.len() > 1 {
            return Ok(sequence.pop_front().unwrap());
        }
        let response = sequence.front().expect("empty test response script");
        Ok(Response {
            status: response.status,
            headers: response.headers.clone(),
            body: response.body.clone(),
        })
    }
}

fn ready_transport() -> ScriptedTransport {
    ScriptedTransport {
        replies: RefCell::new(BTreeMap::from([
            (LIST, VecDeque::from([response(200, json!([detail()]))])),
            (DETAIL, VecDeque::from([response(200, detail())])),
            (FILES, VecDeque::from([response(200, json!([file()]))])),
            (
                REVIEWERS,
                VecDeque::from([response(200, json!({"users": [], "teams": []}))]),
            ),
        ])),
    }
}

fn read(transport: ScriptedTransport) -> Result<Vec<PullRequest>, ConnectionError> {
    GithubClient::new(transport).pull_requests(&RemoteRepository {
        id: "1376547672".into(),
        name: "jdylanmc/pr-sniper".into(),
    })
}

#[test]
fn scripted_control_returns_complete_metadata_before_failure_injection() {
    let pulls = read(ready_transport()).unwrap();

    assert_eq!(pulls.len(), 1);
    assert_eq!(pulls[0].id, "1001");
    assert_eq!(pulls[0].files.len(), 1);
    assert_eq!(pulls[0].files[0].path, "src/identity.rs");
    assert_eq!(
        pulls[0].head_sha,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
}

#[test]
fn missing_or_failed_later_pull_page_never_returns_partial_metadata() {
    for (status, expected) in [
        (None, ConnectionError::MissingReadPermission),
        (Some(500), ConnectionError::ProviderFailure),
    ] {
        let mut transport = ready_transport();
        transport.reply(LIST, linked(
            json!([detail()]),
            "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\"",
        ));
        if let Some(status) = status {
            transport.reply(
                LIST_NEXT,
                response(status, json!({"message": "Page failed"})),
            );
        }

        assert_eq!(read(transport), Err(expected));
    }
}

#[test]
fn missing_or_failed_later_file_page_never_returns_partial_metadata() {
    for (status, expected) in [
        (None, ConnectionError::MissingReadPermission),
        (Some(500), ConnectionError::ProviderFailure),
    ] {
        let mut transport = ready_transport();
        let mut pull = detail();
        pull["changed_files"] = json!(2);
        transport.reply(DETAIL, response(200, pull));
        transport.reply(FILES, linked(
            json!([file()]),
            "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=2>; rel=\"next\"",
        ));
        if let Some(status) = status {
            transport.reply(
                FILES_NEXT,
                response(status, json!({"message": "Page failed"})),
            );
        }

        assert_eq!(read(transport), Err(expected));
    }
}

#[test]
fn file_count_mismatch_and_provider_file_limit_are_explicitly_incomplete() {
    for count in [0, 2, 3001] {
        let mut transport = ready_transport();
        let mut pull = detail();
        pull["changed_files"] = json!(count);
        transport.reply(DETAIL, response(200, pull));

        assert_eq!(
            read(transport),
            Err(ConnectionError::IncompleteRead),
            "{count}"
        );
    }
}

#[test]
fn duplicate_pull_ids_on_later_pages_are_not_silently_deduplicated() {
    let mut transport = ready_transport();
    transport.reply(LIST, linked(
        json!([detail()]),
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\"",
    ));
    let mut duplicate = detail();
    duplicate["number"] = json!(32);
    transport.reply(LIST_NEXT, response(200, json!([duplicate])));

    assert_eq!(read(transport), Err(ConnectionError::IncompleteRead));
}

#[test]
fn duplicate_file_paths_across_pages_are_explicitly_incomplete() {
    let mut transport = ready_transport();
    let mut pull = detail();
    pull["changed_files"] = json!(2);
    transport.reply(DETAIL, response(200, pull));
    transport.reply(FILES, linked(
        json!([file()]),
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=2>; rel=\"next\"",
    ));
    let mut duplicate = file();
    duplicate["sha"] = json!("ffffffffffffffffffffffffffffffffffffffff");
    transport.reply(FILES_NEXT, response(200, json!([duplicate])));

    assert_eq!(read(transport), Err(ConnectionError::IncompleteRead));
}

#[test]
fn malformed_json_at_any_metadata_endpoint_is_an_explicit_error() {
    for path in [LIST, DETAIL, FILES, REVIEWERS] {
        let mut transport = ready_transport();
        let mut malformed = response(200, Value::Null);
        malformed.body = b"{not-json".to_vec();
        transport.reply(path, malformed);

        assert_eq!(
            read(transport),
            Err(ConnectionError::InvalidResponse),
            "{path}"
        );
    }
}

#[test]
fn malformed_metadata_schema_never_becomes_partial_success() {
    let mut invalid_detail = detail();
    invalid_detail["draft"] = json!("false");
    let mut invalid_file = file();
    invalid_file["additions"] = json!(-1);
    for (path, body) in [
        (LIST, json!({"pulls": []})),
        (DETAIL, invalid_detail),
        (FILES, json!([invalid_file])),
        (REVIEWERS, json!({"users": {}, "teams": []})),
    ] {
        let mut transport = ready_transport();
        transport.reply(path, response(200, body));

        assert_eq!(
            read(transport),
            Err(ConnectionError::InvalidResponse),
            "{path}"
        );
    }
}

#[test]
fn untrusted_looping_skipped_or_filter_changing_links_are_rejected() {
    for link in [
        "<https://example.invalid/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\"",
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=1>; rel=\"next\"",
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3>; rel=\"next\"",
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=open&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\"",
        "not-a-valid-link",
    ] {
        let mut transport = ready_transport();
        transport.reply(LIST, linked(json!([detail()]), link));

        assert_eq!(read(transport), Err(ConnectionError::IncompleteRead), "{link}");
    }
}

#[test]
fn duplicate_query_parameters_cannot_hide_a_dropped_pull_filter() {
    let mut transport = ready_transport();
    transport.reply(LIST, linked(
        json!([detail()]),
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?sort=created&direction=asc&per_page=100&page=2&page=2>; rel=\"next\"",
    ));
    transport.reply(
        "/repos/jdylanmc/pr-sniper/pulls?sort=created&direction=asc&per_page=100&page=2&page=2",
        response(200, json!([])),
    );

    assert_eq!(read(transport), Err(ConnectionError::IncompleteRead));
}

#[test]
fn contradictory_last_links_cannot_hide_unread_pages() {
    let mut transport = ready_transport();
    transport.reply(LIST, linked(
        json!([detail()]),
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3>; rel=\"last\", <https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=1>; rel=\"last\"",
    ));

    assert_eq!(read(transport), Err(ConnectionError::IncompleteRead));
}

#[test]
fn head_change_after_file_reads_invalidates_the_metadata_snapshot() {
    let mut transport = ready_transport();
    let mut changed = detail();
    changed["head"]["sha"] = json!("cccccccccccccccccccccccccccccccccccccccc");
    transport.sequence(
        DETAIL,
        vec![response(200, detail()), response(200, changed)],
    );

    assert_eq!(read(transport), Err(ConnectionError::RevisionChanged));
}

#[test]
fn base_repository_must_match_the_verified_remote_identity() {
    let mut transport = ready_transport();
    let mut pull = detail();
    pull["base"]["repo"]["id"] = json!(42);
    transport.reply(DETAIL, response(200, pull));

    assert_eq!(read(transport), Err(ConnectionError::RepositoryChanged));
}

#[test]
fn advertised_last_page_cannot_disappear_on_a_later_page() {
    for terminal in [None, Some("<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"last\"")] {
        let mut transport = ready_transport();
        transport.reply(LIST, linked(
            json!([detail()]),
            "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\", <https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3>; rel=\"last\"",
        ));
        let mut second = detail();
        second["id"] = json!(1002);
        second["number"] = json!(32);
        transport.reply(LIST_NEXT, match terminal {
            Some(link) => linked(json!([second.clone()]), link),
            None => response(200, json!([second.clone()])),
        });
        transport.reply("/repos/jdylanmc/pr-sniper/pulls/32", response(200, second));
        transport.reply("/repos/jdylanmc/pr-sniper/pulls/32/files?per_page=100&page=1", response(200, json!([file()])));
        transport.reply("/repos/jdylanmc/pr-sniper/pulls/32/requested_reviewers", response(200, json!({"users": [], "teams": []})));
        assert_eq!(read(transport), Err(ConnectionError::IncompleteRead));
    }
}

#[test]
fn advertised_next_page_cannot_be_an_empty_terminal_tail() {
    let mut transport = ready_transport();
    transport.reply(LIST, linked(json!([detail()]),
        "<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\""));
    transport.reply(LIST_NEXT, response(200, json!([])));
    assert_eq!(read(transport), Err(ConnectionError::IncompleteRead));
}

#[test]
fn advertised_three_page_read_returns_all_distinct_pull_requests() {
    let mut transport = ready_transport();
    for (number, id, list_path, detail_path, files_path, reviewers_path, link) in [
        (31, 1001, LIST, DETAIL, FILES, REVIEWERS,
         Some("<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\", <https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3>; rel=\"last\"")),
        (32, 1002, LIST_NEXT, "/repos/jdylanmc/pr-sniper/pulls/32", "/repos/jdylanmc/pr-sniper/pulls/32/files?per_page=100&page=1", "/repos/jdylanmc/pr-sniper/pulls/32/requested_reviewers",
         Some("<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3>; rel=\"next\", <https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3>; rel=\"last\"")),
        (33, 1003, "/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=3", "/repos/jdylanmc/pr-sniper/pulls/33", "/repos/jdylanmc/pr-sniper/pulls/33/files?per_page=100&page=1", "/repos/jdylanmc/pr-sniper/pulls/33/requested_reviewers", None),
    ] {
        let mut pull = detail();
        pull["id"] = json!(id);
        pull["number"] = json!(number);
        pull["changed_files"] = json!(0);
        transport.reply(list_path, match link {
            Some(link) => linked(json!([pull.clone()]), link),
            None => response(200, json!([pull.clone()])),
        });
        transport.reply(detail_path, response(200, pull));
        transport.reply(files_path, response(200, json!([])));
        transport.reply(reviewers_path, response(200, json!({"users": [], "teams": []})));
    }
    let pulls = read(transport).unwrap();
    assert_eq!(
        pulls
            .iter()
            .map(|pull| pull.id.as_str())
            .collect::<Vec<_>>(),
        ["1001", "1002", "1003"]
    );
}
