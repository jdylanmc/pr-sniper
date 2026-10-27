use super::*;
use crate::provider::{Lifecycle, PullRequestReader, TreeEntry};

const TEAM: &str = "00000000-0000-0000-0000-0000000000d1";

fn sha(n: u32) -> String {
    format!("{n:040x}")
}

fn git(path: &str) -> String {
    format!("{ORG}/{PROJECT}/_apis/git/repositories/{REPO}/{path}")
}

fn raw_pull(number: u64, author: &str, source: u32, draft: bool) -> Value {
    json!({
        "pullRequestId": number,
        "status": "active",
        "title": format!("Change {number}"),
        "createdBy": {"id": author, "displayName": "Blake Example"},
        "creationDate": "2026-10-20T10:00:00.1234567Z",
        "isDraft": draft,
        "repository": {"id": REPO, "project": {"id": PROJECT}},
        "lastMergeSourceCommit": {"commitId": sha(source)},
        "lastMergeTargetCommit": {"commitId": sha(900)},
        "reviewers": [
            {"id": AVERY, "displayName": "Avery Example", "vote": 0},
            {"id": TEAM, "displayName": "Web Team", "isContainer": true}
        ]
    })
}

fn remote() -> RemoteRepository {
    RemoteRepository {
        id: key().id(),
        name: "contoso/Web/app".into(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

fn signed_in(wire: &FakeWire) {
    wire.json(
        &format!("{ORG}/_apis/connectionData"),
        identity_body(AVERY, "Avery Example"),
    );
}

#[test]
fn polls_active_pull_requests_across_pages_with_the_source_commit_as_revision() {
    let (_, wire, client) = fakes();
    let first: Vec<Value> = (1..=100)
        .map(|n| raw_pull(n, BLAKE, n as u32, false))
        .collect();
    wire.json(&git("pullrequests?"), json!({"value": first}))
        .json(
            &git("pullrequests?"),
            json!({"value": [raw_pull(101, CASEY, 101, true)]}),
        );
    let pulls = block(client.active_pull_requests(&key())).unwrap();
    assert_eq!(pulls.len(), 101);
    let urls = wire.urls();
    assert_eq!(urls.len(), 2);
    for (url, skip) in urls.iter().zip(["0", "100"]) {
        assert!(url.contains("searchCriteria.status=active"), "{url}");
        assert!(url.contains("api-version=7.1"), "{url}");
        assert!(url.contains(&format!("%24skip={skip}")), "{url}");
    }
    let pull = &pulls[0];
    assert_eq!((pull.id.as_str(), pull.number), ("1", 1));
    assert_eq!(pull.head_sha, sha(1));
    assert_eq!(pull.base_sha, sha(900));
    assert_eq!(pull.state, Lifecycle::Open);
    assert_eq!(pull.base_repository_id, key().id());
    assert_eq!(
        pull.head_repository_id.as_deref(),
        Some(key().id().as_str())
    );
    assert_eq!(pull.author.as_ref().unwrap().id, BLAKE);
    assert_eq!(pull.requested_reviewers[0].id, AVERY);
    assert_eq!(pull.requested_teams[0].id, TEAM);
    assert!(chrono::DateTime::parse_from_rfc3339(&pull.updated_at).is_ok());
    assert!(!pull.draft);
    assert!(pulls[100].draft);
    assert_eq!(pulls[100].author.as_ref().unwrap().id, CASEY);
}

#[test]
fn a_listing_that_moves_or_names_another_repository_is_refused() {
    let (_, wire, client) = fakes();
    let page: Vec<Value> = (1..=100)
        .map(|n| raw_pull(n, BLAKE, n as u32, false))
        .collect();
    // A new pull request shifted the pages: one is seen twice.
    wire.json(&git("pullrequests?"), json!({"value": page}))
        .json(
            &git("pullrequests?"),
            json!({"value": [raw_pull(100, BLAKE, 100, false)]}),
        );
    assert_eq!(
        block(client.active_pull_requests(&key())),
        Err(ConnectionError::IncompleteRead)
    );

    let (_, wire, client) = fakes();
    let mut moved = raw_pull(1, BLAKE, 1, false);
    moved["repository"]["id"] = json!(OTHER_REPO);
    wire.json(&git("pullrequests?"), json!({"value": [moved]}));
    assert_eq!(
        block(client.active_pull_requests(&key())),
        Err(ConnectionError::RepositoryChanged)
    );

    let (_, wire, client) = fakes();
    let mut short = raw_pull(1, BLAKE, 1, false);
    short["lastMergeSourceCommit"]["commitId"] = json!("abc");
    wire.json(&git("pullrequests?"), json!({"value": [short]}));
    assert_eq!(
        block(client.active_pull_requests(&key())),
        Err(ConnectionError::InvalidResponse)
    );

    let (_, wire, client) = fakes();
    let other = RepositoryKey {
        organization: "fabrikam".into(),
        ..key()
    };
    assert_eq!(
        block(client.active_pull_requests(&other)),
        Err(ConnectionError::RepositoryUnavailable)
    );
    assert!(wire.urls().is_empty());
}

#[test]
fn a_new_iteration_is_a_new_revision() {
    let (_, wire, client) = fakes();
    wire.json(
        &git("pullrequests?"),
        json!({"value": [raw_pull(7, BLAKE, 1, false)]}),
    )
    .json(
        &git("pullrequests?"),
        json!({"value": [raw_pull(7, BLAKE, 2, false)]}),
    );
    let first = block(client.active_pull_requests(&key())).unwrap();
    let second = block(client.active_pull_requests(&key())).unwrap();
    assert_eq!(first[0].id, second[0].id);
    assert_ne!(first[0].revision(), second[0].revision());
    assert_eq!(second[0].head_sha, sha(2));
}

#[test]
fn reads_changed_files_of_the_latest_iteration() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    let change = |path: &str, kind: &str, object: u32| json!({"item": {"path": path, "objectId": sha(object), "originalObjectId": sha(object + 1000)}, "changeType": kind});
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 2, false))
        .json(
            &git("pullRequests/7/iterations?"),
            json!({"value": [
                {"id": 2, "sourceRefCommit": {"commitId": sha(2)}, "commonRefCommit": {"commitId": sha(800)}},
                {"id": 1, "sourceRefCommit": {"commitId": sha(1)}}
            ]}),
        )
        .json(
            &git("pullRequests/7/iterations/2/changes?"),
            json!({"changeEntries": [
                change("/src/new.rs", "add", 10),
                change("/src/main.rs", "edit", 11),
                {"item": {"path": "/src", "isFolder": true}, "changeType": "edit"},
            ], "nextSkip": 3, "nextTop": 1000}),
        )
        .json(
            &git("pullRequests/7/iterations/2/changes?"),
            json!({"changeEntries": [
                change("/docs/old.md", "delete", 12),
                {"item": {"path": "/docs/moved.md", "objectId": sha(13)}, "changeType": "edit, rename", "originalPath": "/docs/before.md"},
            ]}),
        );
    let reader = reader(&wire, &runtime).unwrap();
    let files = reader.pull_request_files(&remote(), 7).unwrap();
    let summary: Vec<_> = files
        .iter()
        .map(|file| {
            (
                file.path.as_str(),
                file.status.as_str(),
                file.previous_path.as_deref(),
                file.sha.clone(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("src/new.rs", "added", None, sha(10)),
            ("src/main.rs", "modified", None, sha(11)),
            ("docs/old.md", "removed", None, sha(1012)),
            ("docs/moved.md", "renamed", Some("docs/before.md"), sha(13)),
        ]
    );
    let changes: Vec<_> = wire
        .urls()
        .into_iter()
        .filter(|url| url.contains("/changes?"))
        .collect();
    assert!(changes[0].contains("%24compareTo=0") && changes[0].contains("%24skip=0"));
    assert!(changes[1].contains("%24skip=3"));
}

#[test]
fn a_head_that_is_not_yet_the_latest_iteration_has_changed() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 3, false))
        .json(
            &git("pullRequests/7/iterations?"),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(1)}}]}),
        );
    let reader = reader(&wire, &runtime).unwrap();
    assert_eq!(
        reader.pull_request_files(&remote(), 7),
        Err(ConnectionError::RevisionChanged)
    );
}

#[test]
fn review_context_reads_both_trees_and_content_at_commits() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    let tree = |file: u32| {
        json!({"value": [
            {"path": "/", "objectId": sha(500), "gitObjectType": "tree"},
            {"path": "/src", "objectId": sha(501), "gitObjectType": "tree"},
            {"path": "/src/main.rs", "objectId": sha(file), "gitObjectType": "blob"},
        ]})
    };
    let blob = |body: &[u8]| {
        Ok(Response {
            status: 200,
            body: body.to_vec(),
            ..Response::default()
        })
    };
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 2, false))
        .json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 2, false))
        .json(
            &git("pullRequests/7/iterations?"),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(2)}, "commonRefCommit": {"commitId": sha(800)}}]}),
        )
        .json(
            &git("pullRequests/7/iterations/1/changes?"),
            json!({"changeEntries": [{"item": {"path": "/src/main.rs", "objectId": sha(11)}, "changeType": "edit"}]}),
        )
        .json(&git("items?"), tree(11))
        .json(&git("items?"), tree(12))
        .on(&git(&format!("blobs/{}?", sha(11))), blob(b"fn main() {}\n"))
        .on(&git(&format!("blobs/{}?", sha(12))), blob(&[0, 159, 146, 150]))
        .on(
            &git(&format!("blobs/{}?", sha(13))),
            Ok(Response {
                status: 200,
                oversize: true,
                ..Response::default()
            }),
        );
    let reader = reader(&wire, &runtime).unwrap();
    assert_eq!(
        reader.review_context(&remote(), 7, &sha(1)).err(),
        Some(ConnectionError::RevisionChanged)
    );
    let context = reader.review_context(&remote(), 7, &sha(2)).unwrap();
    assert_eq!(context.base_revision, sha(800));
    assert_eq!(context.pull.files.len(), 1);
    assert_eq!(context.files[0].path, "src/main.rs");
    assert!(context.files[0].patch.is_none());
    assert_eq!(
        context.head.keys().collect::<Vec<_>>(),
        ["src", "src/main.rs"]
    );
    assert_eq!(context.head["src/main.rs"].sha, sha(11));
    assert_eq!(context.base["src/main.rs"].sha, sha(12));
    let items: Vec<_> = wire
        .urls()
        .into_iter()
        .filter(|url| url.contains("/items?"))
        .collect();
    assert!(items[0].contains(&format!("versionDescriptor.version={}", sha(2))));
    assert!(items[0].contains("versionDescriptor.versionType=commit"));
    assert!(items[0].contains("recursionLevel=Full"));
    assert!(items[1].contains(&format!("versionDescriptor.version={}", sha(800))));

    let text = reader
        .file_content(&key().id(), &context.head["src/main.rs"])
        .unwrap();
    assert_eq!(text["kind"], "text");
    assert_eq!(text["text"], "fn main() {}\n");
    let binary = reader
        .file_content(&key().id(), &context.base["src/main.rs"])
        .unwrap();
    assert_eq!(binary["kind"], "binary");
    assert_eq!(binary["size"], 4);
    assert!(wire
        .urls()
        .iter()
        .any(|url| url.contains("/blobs/") && url.contains("%24format=octetstream")));
    let folder = reader
        .file_content(&key().id(), &context.head["src"])
        .unwrap();
    assert_eq!(folder["kind"], "tree");
    let large = TreeEntry {
        path: "big.bin".into(),
        mode: "100644".into(),
        sha: sha(13),
        kind: "blob".into(),
    };
    assert_eq!(
        reader.file_content(&key().id(), &large),
        Err(ConnectionError::LimitExceeded)
    );
}

#[test]
fn tracked_pull_requests_missing_from_the_active_listing_are_read_on_their_own() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    let mut completed = raw_pull(8, BLAKE, 8, false);
    completed["status"] = json!("completed");
    completed["closedDate"] = json!("2026-10-21T09:00:00Z");
    let mut abandoned = raw_pull(9, BLAKE, 9, false);
    abandoned["status"] = json!("abandoned");
    wire.json(
        &git("pullrequests?"),
        json!({"value": [raw_pull(7, BLAKE, 7, false)]}),
    )
    .json(&git("pullrequests/8?"), completed)
    .json(&git("pullrequests/9?"), abandoned);
    let reader = reader(&wire, &runtime).unwrap();
    let pulls = reader
        .poll_tracked_pull_requests(
            &remote(),
            &[("7".into(), 7), ("8".into(), 8), ("9".into(), 9)],
        )
        .unwrap();
    let states: Vec<_> = pulls.iter().map(|p| (p.number, p.state.clone())).collect();
    assert_eq!(
        states,
        [
            (7, Lifecycle::Open),
            (8, Lifecycle::Merged),
            (9, Lifecycle::Closed)
        ]
    );
    assert_eq!(pulls[1].updated_at, "2026-10-21T09:00:00+00:00");
    assert!(!wire
        .urls()
        .iter()
        .any(|url| url.contains("pullrequests/7?")));
}

#[test]
fn pull_request_reads_keep_the_shared_failure_types() {
    let runtime = runtime();
    for (response, expected) in [
        (status(401, None), ConnectionError::SignedOut),
        (status(403, None), ConnectionError::MissingReadPermission),
        (
            status(429, Some("12")),
            ConnectionError::RateLimitedAfter(12),
        ),
        (status(503, None), ConnectionError::ProviderFailure),
    ] {
        let wire = Arc::new(FakeWire::default());
        signed_in(&wire);
        wire.on(&git("pullrequests?"), Ok(response.clone()))
            .on(&git("pullrequests?"), Ok(response));
        let reader = reader(&wire, &runtime).unwrap();
        assert_eq!(
            reader.poll_pull_requests(&remote()),
            Err(expected),
            "{expected:?}"
        );
    }
}
