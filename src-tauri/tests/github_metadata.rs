use pr_sniper_lib::github::metadata::{ChangedFile, Lifecycle, PullRequest, RequestedTeam};
use pr_sniper_lib::github::provider::{GithubClient, RemoteRepository, Response, Transport};
use pr_sniper_lib::github::{ConnectionError, Identity};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn open_pull_request() -> Value {
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
        "changed_files": 2
    })
}

fn merged_pull_request() -> Value {
    json!({
        "id": 1002,
        "number": 32,
        "title": "Read merged metadata",
        "user": null,
        "state": "closed",
        "draft": false,
        "merged": true,
        "merged_at": "2026-09-20T20:05:00Z",
        "head": {
            "sha": "cccccccccccccccccccccccccccccccccccccccc",
            "repo": null
        },
        "base": {
            "sha": "dddddddddddddddddddddddddddddddddddddddd",
            "repo": {"id": 1376547672}
        },
        "updated_at": "2026-09-20T20:05:00Z",
        "changed_files": 1
    })
}

struct MetadataTransport;

impl Transport for MetadataTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let (body, next) = match path {
            "/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=1" => (
                json!([open_pull_request()]),
                Some("<https://api.github.com/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\""),
            ),
            "/repos/jdylanmc/pr-sniper/pulls?state=all&sort=created&direction=asc&per_page=100&page=2" => (
                json!([merged_pull_request()]),
                None,
            ),
            "/repos/jdylanmc/pr-sniper/pulls/31" => (open_pull_request(), None),
            "/repos/jdylanmc/pr-sniper/pulls/32" => (merged_pull_request(), None),
            "/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=1" => (
                json!([{
                    "filename": "src/identity.rs",
                    "status": "added",
                    "additions": 12,
                    "deletions": 0,
                    "changes": 12,
                    "sha": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                }]),
                Some("<https://api.github.com/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=2>; rel=\"next\""),
            ),
            "/repos/jdylanmc/pr-sniper/pulls/31/files?per_page=100&page=2" => (
                json!([{
                    "filename": "src/provider.rs",
                    "previous_filename": "src/github.rs",
                    "status": "renamed",
                    "additions": 3,
                    "deletions": 1,
                    "changes": 4,
                    "sha": "ffffffffffffffffffffffffffffffffffffffff"
                }]),
                None,
            ),
            "/repos/jdylanmc/pr-sniper/pulls/32/files?per_page=100&page=1" => (
                json!([{
                    "filename": "README.md",
                    "status": "modified",
                    "additions": 2,
                    "deletions": 1,
                    "changes": 3,
                    "sha": "1111111111111111111111111111111111111111"
                }]),
                None,
            ),
            "/repos/jdylanmc/pr-sniper/pulls/31/requested_reviewers" => (
                json!({
                    "users": [{"id": 42, "login": "reviewer"}],
                    "teams": [{"id": 77, "slug": "maintainers"}]
                }),
                None,
            ),
            "/repos/jdylanmc/pr-sniper/pulls/32/requested_reviewers" => (
                json!({"users": [], "teams": []}),
                None,
            ),
            _ => panic!("unexpected GET path: {path}"),
        };
        let mut headers = BTreeMap::new();
        if let Some(link) = next {
            headers.insert("link".into(), link.into());
        }
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn metadata_exhausts_pull_request_and_file_pages_without_losing_verified_fields() {
    let client = GithubClient::new(MetadataTransport);
    let repository = RemoteRepository {
        id: "1376547672".into(),
        name: "jdylanmc/pr-sniper".into(),
    };

    let result = client.pull_requests(&repository);

    assert_eq!(
        result,
        Ok(vec![
            PullRequest {
                id: "1001".into(),
                number: 31,
                title: "Preserve stable identities".into(),
                author: Some(Identity {
                    id: "6954990".into(),
                    login: "author-renamed".into(),
                }),
                requested_reviewers: vec![Identity {
                    id: "42".into(),
                    login: "reviewer".into(),
                }],
                requested_teams: vec![RequestedTeam {
                    id: "77".into(),
                    slug: "maintainers".into(),
                }],
                state: Lifecycle::Open,
                draft: false,
                head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                base_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                head_repository_id: Some("1376547672".into()),
                base_repository_id: "1376547672".into(),
                updated_at: "2026-09-20T20:00:00Z".into(),
                files: vec![
                    ChangedFile {
                        path: "src/identity.rs".into(),
                        previous_path: None,
                        status: "added".into(),
                        additions: 12,
                        deletions: 0,
                        changes: 12,
                        sha: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into(),
                    },
                    ChangedFile {
                        path: "src/provider.rs".into(),
                        previous_path: Some("src/github.rs".into()),
                        status: "renamed".into(),
                        additions: 3,
                        deletions: 1,
                        changes: 4,
                        sha: "ffffffffffffffffffffffffffffffffffffffff".into(),
                    },
                ],
            },
            PullRequest {
                id: "1002".into(),
                number: 32,
                title: "Read merged metadata".into(),
                author: None,
                requested_reviewers: vec![],
                requested_teams: vec![],
                state: Lifecycle::Merged,
                draft: false,
                head_sha: "cccccccccccccccccccccccccccccccccccccccc".into(),
                base_sha: "dddddddddddddddddddddddddddddddddddddddd".into(),
                head_repository_id: None,
                base_repository_id: "1376547672".into(),
                updated_at: "2026-09-20T20:05:00Z".into(),
                files: vec![ChangedFile {
                    path: "README.md".into(),
                    previous_path: None,
                    status: "modified".into(),
                    additions: 2,
                    deletions: 1,
                    changes: 3,
                    sha: "1111111111111111111111111111111111111111".into(),
                }],
            },
        ])
    );
}
