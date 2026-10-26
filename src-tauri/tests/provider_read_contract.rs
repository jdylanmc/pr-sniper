//! Contract: a provider's read interfaces return the neutral account,
//! repository, pull request, changed-file and file-content shapes. The GitHub
//! implementation is driven here over a fake wire.

use base64::{engine::general_purpose::STANDARD, Engine};
use pr_sniper_lib::github::provider::{GithubClient, Response, Transport};
use pr_sniper_lib::provider::{
    CommentCapability, ConnectionError, Lifecycle, Person, ProviderReader, RemoteRepository,
    Revision,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE_BASE: &str = "dddddddddddddddddddddddddddddddddddddddd";
const BLOB: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

struct Wire;

fn pull() -> Value {
    json!({
        "id": 9, "number": 7, "title": "Neutral reads",
        "user": {"id": 11, "login": "author"},
        "requested_reviewers": [{"id": 12, "login": "reviewer"}],
        "requested_teams": [],
        "state": "open", "merged": false, "draft": false,
        "head": {"sha": HEAD, "repo": {"id": 100}},
        "base": {"sha": BASE, "repo": {"id": 100}},
        "updated_at": "2026-10-26T00:00:00Z", "changed_files": 1
    })
}

impl Transport for Wire {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let repository = json!({
            "id": 100, "full_name": "example/repo", "private": false,
            "archived": false, "disabled": false, "permissions": {"pull": true},
            "owner": {"login": "example", "type": "User"}
        });
        let body = match path {
            "/user" => json!({"id": 22, "login": "acting"}),
            "/users/reviewer" => json!({"id": 12, "login": "reviewer"}),
            "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1" => {
                json!([repository])
            }
            "/repos/example/repo" => repository,
            "/repos/example/repo/pulls?state=open&per_page=1"
            | "/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=1"
            | "/repos/example/repo/pulls?state=all&sort=created&direction=desc&per_page=1"
            | "/repos/example/repo/pulls?state=all&sort=created&direction=asc&per_page=100&page=1" => {
                json!([pull()])
            }
            "/repos/example/repo/pulls/7" => pull(),
            "/repos/example/repo/pulls/7/requested_reviewers" => {
                json!({"users": [{"id": 12, "login": "reviewer"}], "teams": []})
            }
            "/repos/example/repo/pulls/7/files?per_page=100&page=1" => json!([{
                "filename": "src/new.rs", "previous_filename": "src/old.rs",
                "status": "renamed", "additions": 2, "deletions": 1, "changes": 3,
                "sha": BLOB, "patch": "@@ -1 +1,2 @@\n-old\n+new\n+more"
            }]),
            p if p == format!("/repos/example/repo/compare/{BASE}...{HEAD}") => {
                json!({"merge_base_commit": {"sha": MERGE_BASE}})
            }
            p if p.starts_with("/repos/example/repo/git/trees/") => {
                let file = if p.contains(MERGE_BASE) { "src/old.rs" } else { "src/new.rs" };
                json!({"truncated": false, "tree": [
                    {"path": file, "mode": "100644", "type": "blob", "sha": BLOB}
                ]})
            }
            p if p == format!("/repos/example/repo/git/blobs/{BLOB}") => json!({
                "sha": BLOB, "encoding": "base64", "size": 7,
                "content": STANDARD.encode("source\n")
            }),
            _ => panic!("unexpected read: {path}"),
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

fn read_contract(reader: &dyn ProviderReader) {
    let acting = Person {
        id: "22".into(),
        login: "acting".into(),
    };
    let repository = RemoteRepository {
        id: "100".into(),
        name: "example/repo".into(),
    };

    assert_eq!(reader.current_identity().unwrap(), acting);
    assert_eq!(reader.resolve_person("reviewer").unwrap().id, "12");

    assert_eq!(
        reader.accessible_repositories().unwrap(),
        std::slice::from_ref(&repository)
    );
    let browser = reader.repository_browser(&acting).unwrap();
    assert_eq!(browser.repositories, std::slice::from_ref(&repository));
    assert!(browser.warnings.is_empty());
    let owned = reader.owner_repository_browser(&acting, "example").unwrap();
    assert_eq!(owned.repositories, std::slice::from_ref(&repository));

    let connection = reader.connect("example/repo", Some("22")).unwrap();
    assert_eq!(connection.identity, acting);
    assert_eq!(connection.repository, repository);
    assert!(connection.capabilities.read);
    assert_eq!(
        connection.capabilities.comment,
        CommentCapability::Available
    );
    assert_eq!(
        reader.connect("example/repo", Some("23")).unwrap_err(),
        ConnectionError::WrongIdentity
    );
    let target = reader
        .resolve_repository_target("https://github.com/example/repo/pull/7", "22")
        .unwrap();
    assert_eq!(target.connection.repository, repository);
    assert_eq!(target.pull_request.unwrap().number, 7);

    assert_eq!(reader.latest_pull_request_number(&repository).unwrap(), 7);
    let polled = reader.poll_pull_requests(&repository).unwrap();
    assert_eq!(polled.len(), 1);
    assert!(polled[0].files.is_empty());
    let tracked = reader
        .poll_tracked_pull_requests(&repository, &[("9".into(), 7)])
        .unwrap();
    assert_eq!(tracked, polled);

    let detail = reader.pull_request(&repository, 7).unwrap();
    assert_eq!(detail.id, "9");
    assert_eq!(detail.state, Lifecycle::Open);
    assert_eq!(
        detail.author.as_ref().map(|p| p.login.as_str()),
        Some("author")
    );
    assert_eq!(detail.requested_reviewers[0].id, "12");
    assert_eq!(
        detail.revision(),
        Revision {
            head: HEAD.into(),
            base: BASE.into(),
        }
    );

    let files = reader.pull_request_files(&repository, 7).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "src/new.rs");
    assert_eq!(files[0].previous_path.as_deref(), Some("src/old.rs"));
    assert_eq!((files[0].additions, files[0].deletions), (2, 1));
    let listed = reader.pull_requests(&repository).unwrap();
    assert_eq!(listed[0].files, files);

    let context = reader.review_context(&repository, 7, HEAD).unwrap();
    assert_eq!(context.pull, detail);
    assert_eq!(context.base_revision, MERGE_BASE);
    assert_eq!(context.files[0].status, "renamed");
    assert!(context.base.contains_key("src/old.rs"));
    let content = reader
        .file_content("example/repo", &context.head["src/new.rs"])
        .unwrap();
    assert_eq!(content["kind"], "text");
    assert_eq!(content["text"], "source\n");
    assert_eq!(
        reader.review_context(&repository, 7, BASE).unwrap_err(),
        ConnectionError::RevisionChanged
    );
}

#[test]
fn github_satisfies_the_read_interfaces() {
    read_contract(&GithubClient::new(Wire));
}
