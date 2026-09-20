use pr_sniper_lib::github::provider::{GithubClient, RemoteRepository, Response, Transport};
use pr_sniper_lib::github::ConnectionError;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

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
