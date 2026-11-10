use super::*;
use crate::provider::{
    Lifecycle, ListingCursor, ListingScope, PullRequest, PullRequestReader, TreeEntry,
};
use std::collections::{BTreeMap, BTreeSet};

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
fn a_listing_that_moves_is_deduplicated_and_one_naming_another_repository_is_refused() {
    let (_, wire, client) = fakes();
    let page: Vec<Value> = (1..=100)
        .map(|n| raw_pull(n, BLAKE, n as u32, false))
        .collect();
    // A new pull request shifted the pages: one is seen twice, kept once.
    wire.json(&git("pullrequests?"), json!({"value": page}))
        .json(
            &git("pullrequests?"),
            json!({"value": [raw_pull(100, BLAKE, 100, false)]}),
        );
    let pulls = block(client.active_pull_requests(&key())).unwrap();
    assert_eq!(pulls.len(), 100);

    let (_, wire, client) = fakes();
    let mut moved = raw_pull(1, BLAKE, 1, false);
    moved["repository"]["id"] = json!(OTHER_REPO);
    wire.json(&git("pullrequests?"), json!({"value": [moved]}));
    assert_eq!(
        block(client.active_pull_requests(&key())),
        Err(ConnectionError::RepositoryChanged)
    );

    // One pull request that does not parse is skipped, not fatal.
    let (_, wire, client) = fakes();
    let mut short = raw_pull(1, BLAKE, 1, false);
    short["lastMergeSourceCommit"]["commitId"] = json!("abc");
    wire.json(
        &git("pullrequests?"),
        json!({"value": [short, raw_pull(2, BLAKE, 2, false)]}),
    );
    let pulls = block(client.active_pull_requests(&key())).unwrap();
    assert_eq!(pulls.iter().map(|p| p.number).collect::<Vec<_>>(), [2]);

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

fn pull_at(head: u32) -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "7".into(),
        number: 7,
        title: "Change 7".into(),
        author: None,
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: sha(head),
        base_sha: sha(900),
        head_repository_id: None,
        base_repository_id: key().id(),
        updated_at: "2026-10-20T10:00:00Z".into(),
        files: vec![],
    }
}

fn three_iterations(wire: &FakeWire) {
    wire.json(
        &git("pullRequests/7/iterations?"),
        json!({"value": [
            {"id": 3, "sourceRefCommit": {"commitId": sha(3)}},
            {"id": 1, "sourceRefCommit": {"commitId": sha(1)}},
            {"id": 2, "sourceRefCommit": {"commitId": sha(2)}}
        ]}),
    );
}

fn edits(paths: impl IntoIterator<Item = String>) -> Vec<Value> {
    paths
        .into_iter()
        .map(|path| json!({"item": {"path": format!("/{path}"), "objectId": sha(10)}, "changeType": "edit"}))
        .collect()
}

#[test]
fn path_listing_pages_iteration_changes_to_the_end_without_a_file_cap() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    three_iterations(&wire);
    let page = |from: usize, to: usize| edits((from..to).map(|n| format!("src/f{n}.rs")));
    wire.json(
        &git("pullRequests/7/iterations/3/changes?"),
        json!({"changeEntries": page(0, 1000), "nextSkip": 1000, "nextTop": 1000}),
    )
    .json(
        &git("pullRequests/7/iterations/3/changes?"),
        json!({"changeEntries": page(1000, 2000), "nextSkip": 2000, "nextTop": 1000}),
    )
    .json(
        &git("pullRequests/7/iterations/3/changes?"),
        json!({"changeEntries": page(2000, 3001), "nextSkip": 0, "nextTop": 0}),
    );
    let reader = reader(&wire, &runtime).unwrap();
    let pull = pull_at(3);
    // No total is reported: nothing is read until a glob needs the list.
    assert_eq!(
        reader.changed_file_listing(&remote(), &pull, false),
        Ok(crate::provider::Listing::Visible)
    );
    assert!(wire.urls().iter().all(|u| !u.contains("/iterations")));
    let Ok(crate::provider::Listing::Complete(files)) =
        reader.changed_file_listing(&remote(), &pull, true)
    else {
        panic!("complete listing")
    };
    assert_eq!(files.len(), 3001);
    let skips: Vec<_> = wire
        .urls()
        .into_iter()
        .filter(|u| u.contains("/changes?"))
        .collect();
    assert_eq!(skips.len(), 3);
    assert!(skips[2].contains("%24skip=2000") && skips[2].contains("%24compareTo=0"));
}

#[test]
fn a_change_listing_that_stops_advancing_is_incomplete() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    three_iterations(&wire);
    wire.json(
        &git("pullRequests/7/iterations/3/changes?"),
        json!({"changeEntries": edits(["a".to_string()]), "nextSkip": 5}),
    )
    .json(
        &git("pullRequests/7/iterations/3/changes?"),
        json!({"changeEntries": edits(["b".to_string()]), "nextSkip": 5}),
    );
    let reader = reader(&wire, &runtime).unwrap();
    assert_eq!(
        reader.changed_file_listing(&remote(), &pull_at(3), true),
        Err(ConnectionError::IncompleteRead)
    );
    // A head that is not the latest iteration moved during the read.
    three_iterations(&wire);
    assert_eq!(
        reader.changed_file_listing(&remote(), &pull_at(2), true),
        Err(ConnectionError::RevisionChanged)
    );
}

#[test]
fn incremental_changes_compare_to_the_last_reviewed_iteration() {
    use crate::provider::Since;
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    three_iterations(&wire);
    wire.json(
        &git("pullRequests/7/iterations/3/changes?"),
        json!({"changeEntries": edits(["docs/a.md".to_string()])}),
    );
    let reader = reader(&wire, &runtime).unwrap();
    let pull = pull_at(3);
    let Ok(Since::Files(files)) = reader.changed_files_since(&remote(), &pull, &sha(1)) else {
        panic!("incremental files")
    };
    assert_eq!(files[0].path, "docs/a.md");
    let changes: Vec<_> = wire
        .urls()
        .into_iter()
        .filter(|u| u.contains("/changes?"))
        .collect();
    assert!(changes[0].contains("%24compareTo=1"), "{}", changes[0]);
    // Unknown old head (rebase onto a new push history): whole PR.
    three_iterations(&wire);
    assert_eq!(
        reader.changed_files_since(&remote(), &pull, &sha(77)),
        Ok(Since::WholePullRequest)
    );
    // Reviewed the latest iteration: nothing changed since.
    three_iterations(&wire);
    assert_eq!(
        reader.changed_files_since(&remote(), &pull, &sha(3)),
        Ok(Since::Files(vec![]))
    );
    // A newer push than the polled head is a moved revision.
    let stale = pull_at(2);
    three_iterations(&wire);
    assert_eq!(
        reader.changed_files_since(&remote(), &stale, &sha(1)),
        Err(ConnectionError::RevisionChanged)
    );
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
fn review_context_lists_every_change_without_a_file_cap() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    let page = |from: usize, to: usize| edits((from..to).map(|n| format!("src/f{n}.rs")));
    let empty = json!({"value": [{"path": "/", "objectId": sha(500), "gitObjectType": "tree"}]});
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 2, false))
        .json(
            &git("pullRequests/7/iterations?"),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(2)}, "commonRefCommit": {"commitId": sha(800)}}]}),
        )
        .json(
            &git("pullRequests/7/iterations/1/changes?"),
            json!({"changeEntries": page(0, 1000), "nextSkip": 1000, "nextTop": 1000}),
        )
        .json(
            &git("pullRequests/7/iterations/1/changes?"),
            json!({"changeEntries": page(1000, 2000), "nextSkip": 2000, "nextTop": 1000}),
        )
        .json(
            &git("pullRequests/7/iterations/1/changes?"),
            json!({"changeEntries": page(2000, 3001), "nextSkip": 0, "nextTop": 0}),
        )
        .json(&git("items?"), empty.clone())
        .json(&git("items?"), empty);
    let reader = reader(&wire, &runtime).unwrap();
    let context = reader.review_context(&remote(), 7, &sha(2)).unwrap();
    assert_eq!(context.files.len(), 3001);
    assert_eq!(context.files[3000].path, "src/f3000.rs");
    assert!(context.files.iter().all(|f| f.patch.is_none()));
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
        // Diffed for the review context first, then read as content.
        .on(&git(&format!("blobs/{}?", sha(12))), blob(b"fn old() {}\n"))
        .on(&git(&format!("blobs/{}?", sha(11))), blob(b"fn main() {}\n"))
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
    assert_eq!(
        context.files[0].patch.as_deref(),
        Some("@@ -1 +1 @@\n-fn old() {}\n+fn main() {}")
    );
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

/// A repository's active listing served by `$skip`/`$top`, newest first, as
/// Azure DevOps lists it. `open` may change between polls; `fail` answers
/// the listing request with that index instead.
#[derive(Default)]
struct Listing {
    /// Its own organization: a throttling hold never reaches other tests.
    organization: String,
    open: Mutex<Vec<u64>>,
    fail: Mutex<BTreeMap<usize, Response>>,
    skips: Mutex<Vec<u64>>,
}

impl Listing {
    fn new(open: impl IntoIterator<Item = u64>) -> Arc<Self> {
        Self::of("contoso", open)
    }

    fn of(organization: &str, open: impl IntoIterator<Item = u64>) -> Arc<Self> {
        let listing = Arc::new(Self {
            organization: organization.into(),
            ..Self::default()
        });
        listing.set(open);
        listing
    }

    fn key(&self) -> RepositoryKey {
        RepositoryKey {
            organization: self.organization.clone(),
            ..key()
        }
    }

    fn set(&self, open: impl IntoIterator<Item = u64>) {
        let mut open: Vec<u64> = open.into_iter().collect();
        open.sort_unstable_by(|a, b| b.cmp(a));
        *self.open.lock().unwrap() = open;
        self.skips.lock().unwrap().clear();
        self.fail.lock().unwrap().clear();
    }

    fn client(self: &Arc<Self>) -> Client {
        Client::new(
            Arc::new(FakeTokens::default()),
            Arc::clone(self) as Arc<dyn Transport>,
            TenantId::parse(TENANT).unwrap(),
            &self.organization,
        )
    }

    fn skips(&self) -> Vec<u64> {
        self.skips.lock().unwrap().clone()
    }
}

#[async_trait]
impl Transport for Listing {
    async fn send(&self, request: &Request, _: &AccessToken) -> Result<Response, NetworkError> {
        let url = url::Url::parse(&request.url).unwrap();
        assert!(url.path().ends_with("/pullrequests"), "{url}");
        let query = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.parse::<u64>().unwrap())
                .unwrap()
        };
        let (skip, top) = (query("$skip"), query("$top"));
        let index = {
            let mut skips = self.skips.lock().unwrap();
            skips.push(skip);
            skips.len() - 1
        };
        if let Some(response) = self.fail.lock().unwrap().remove(&index) {
            return Ok(response);
        }
        let open = self.open.lock().unwrap();
        let page: Vec<Value> = open
            .iter()
            .skip(skip as usize)
            .take(top as usize)
            .map(|&n| raw_pull(n, BLAKE, n as u32, false))
            .collect();
        Ok(ok(json!({ "value": page })))
    }
}

fn numbers(batch: &crate::provider::PolledPullRequests) -> BTreeSet<u64> {
    batch.pulls.iter().map(|p| p.number).collect()
}

const NOW: i64 = 1_800_000_000;
const ALL: ListingScope = ListingScope::All;

#[test]
fn a_listing_larger_than_one_batch_completes_across_polls_without_a_cap() {
    let listing = Listing::new(1..=5_150);
    let mut seen = BTreeSet::new();
    let mut cursor: Option<ListingCursor> = None;
    for poll in 1.. {
        let batch =
            block(
                listing
                    .client()
                    .poll_batch(&key(), &[], &[], &ALL, cursor.as_ref(), NOW),
            )
            .unwrap();
        assert!(listing.skips().len() <= 10, "one bounded batch per poll");
        listing.skips.lock().unwrap().clear();
        seen.extend(numbers(&batch));
        cursor = batch.cursor;
        if cursor.as_ref().is_some_and(|c| c.passes == 1) {
            assert_eq!(poll, 6);
            break;
        }
    }
    assert_eq!(seen, (1..=5_150).collect());
}

#[test]
fn a_throttled_page_ends_the_batch_and_the_next_poll_resumes_there() {
    let listing = Listing::of("throttled", 1..=3_000);
    let key = listing.key();
    let resume = ListingCursor {
        skip: 1_000,
        ..ListingCursor::default()
    };
    // Head page, 980, 1,080, then 1,180 is throttled.
    listing
        .fail
        .lock()
        .unwrap()
        .insert(3, status(429, Some("1")));
    // The tracked pull request is not read while the organization waits:
    // the listing transport refuses any other request.
    let tracked = [("7".to_string(), 7)];
    let batch = block(
        listing
            .client()
            .poll_batch(&key, &tracked, &[], &ALL, Some(&resume), NOW),
    )
    .unwrap();
    assert!(batch.throttled);
    let cursor = batch.cursor.unwrap();
    assert_eq!(
        (cursor.query, cursor.skip, cursor.passes),
        (0, 1_000, 0),
        "no progress past pages the poll does not commit"
    );
    assert_eq!(cursor.retry_at, Some(NOW + 1), "the stated wait holds");
    assert_eq!(cursor.stalls, 1);

    // Within the wait: no listing read and no failure.
    listing.skips.lock().unwrap().clear();
    let held = block(
        listing
            .client()
            .poll_batch(&key, &[], &[], &ALL, Some(&cursor), NOW),
    )
    .unwrap();
    assert!(held.pulls.is_empty() && listing.skips().is_empty());
    assert_eq!(held.cursor.as_ref(), Some(&cursor));

    // After it (the organization's hold paused inline): the same batch
    // again, never from 0.
    let batch = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(
            listing
                .client()
                .poll_batch(&key, &[], &[], &ALL, Some(&cursor), NOW + 1),
        )
        .unwrap();
    assert!(!batch.throttled);
    assert_eq!(listing.skips()[..4], [0, 980, 1_080, 1_180]);
    assert!(numbers(&batch).contains(&1_850));
    let next = batch.cursor.unwrap();
    assert_eq!((next.retry_at, next.stalls), (None, 0));
}

#[test]
fn a_listing_that_fails_before_any_page_backs_off_and_keeps_its_place() {
    let listing = Listing::new(1..=5_000);
    let cursor = ListingCursor {
        skip: 4_000,
        ..ListingCursor::default()
    };
    for index in 0..2 {
        listing
            .fail
            .lock()
            .unwrap()
            .insert(index, status(500, None));
    }
    let batch = block(
        listing
            .client()
            .poll_batch(&key(), &[], &[], &ALL, Some(&cursor), NOW),
    )
    .unwrap();
    let next = batch.cursor.unwrap();
    assert_eq!((next.skip, next.stalls), (3_980, 1));
    assert!(next.retry_at.is_some_and(|at| at > NOW));
}

#[test]
fn a_listing_that_shifts_between_polls_never_fails_or_misses_a_pull_request() {
    let listing = Listing::new(1..=1_500);
    let batch = block(
        listing
            .client()
            .poll_batch(&key(), &[], &[], &ALL, None, NOW),
    )
    .unwrap();
    let mut seen = numbers(&batch);
    let cursor = batch.cursor.unwrap();
    assert_eq!(cursor.skip, 1_000);
    // Before poll 2: five close (two not read yet) and three open.
    let closed = [1_490, 1_200, 900, 400, 350];
    let open: Vec<u64> = (1..=1_503).filter(|n| !closed.contains(n)).collect();
    listing.set(open.clone());
    let batch = block(
        listing
            .client()
            .poll_batch(&key(), &[], &[], &ALL, Some(&cursor), NOW),
    )
    .unwrap();
    assert_eq!(
        batch.cursor.as_ref().unwrap().passes,
        1,
        "the pass completes"
    );
    seen.extend(numbers(&batch));
    for number in open {
        assert!(seen.contains(&number), "{number} missed");
    }
}

#[test]
fn a_narrowed_scope_lists_one_query_per_author_and_the_reviewer_only() {
    let (_, wire, client) = fakes();
    let creator = |id: &str| git(&format!("pullrequests?searchCriteria.creatorId={id}"));
    wire.json(
        &creator(BLAKE),
        json!({"value": [raw_pull(7, BLAKE, 7, false)]}),
    )
    .json(
        &creator(CASEY),
        json!({"value": [raw_pull(8, CASEY, 8, false)]}),
    )
    .json(
        &git(&format!("pullrequests?searchCriteria.reviewerId={AVERY}")),
        json!({"value": [raw_pull(7, BLAKE, 7, false), raw_pull(9, CASEY, 9, false)]}),
    );
    let scope = ListingScope::Narrowed {
        authors: vec![CASEY.into(), BLAKE.into(), BLAKE.into(), "12345".into()],
        reviewer: Some(AVERY.into()),
        // Azure DevOps detects no mentions: never a reason to list everything.
        mentions: true,
    };
    let batch = block(client.poll_batch(&key(), &[], &[], &scope, None, NOW)).unwrap();
    assert_eq!(
        batch.pulls.iter().map(|p| p.number).collect::<Vec<_>>(),
        [7, 8, 9]
    );
    assert_eq!(batch.cursor.unwrap().passes, 1);
    let urls = wire.urls();
    assert_eq!(urls.len(), 3, "{urls:?}");
    assert!(urls
        .iter()
        .all(|url| url.contains("searchCriteria.creatorId=")
            || url.contains("searchCriteria.reviewerId=")));
    assert!(urls
        .iter()
        .all(|url| url.contains("searchCriteria.status=active")));
}

#[test]
fn explicit_and_tracked_reads_never_list_the_repository() {
    let (_, wire, client) = fakes();
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 7, false));
    let nothing = ListingScope::Narrowed {
        authors: Vec::new(),
        reviewer: None,
        mentions: false,
    };
    let batch =
        block(client.poll_batch(&key(), &[("7".into(), 7)], &[], &nothing, None, NOW)).unwrap();
    assert_eq!(batch.pulls.len(), 1);
    assert_eq!(batch.cursor.unwrap().passes, 1);
    assert_eq!(wire.urls().len(), 1);
    assert!(wire.urls()[0].contains("/pullrequests/7?"));
}

#[test]
fn a_tracked_pull_request_is_read_even_when_the_listing_fails() {
    let (_, wire, client) = fakes();
    wire.on(&git("pullrequests?"), Ok(status(500, None)))
        .json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 7, false));
    let batch = block(client.poll_batch(&key(), &[("7".into(), 7)], &[], &ALL, None, NOW)).unwrap();
    assert_eq!(
        batch.pulls.iter().map(|p| p.number).collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(batch.cursor.unwrap().stalls, 1);
}

#[test]
fn access_failures_on_the_listing_still_surface() {
    for (response, expected) in [
        (status(401, None), ConnectionError::SignedOut),
        (status(403, None), ConnectionError::MissingReadPermission),
    ] {
        let (_, wire, client) = fakes();
        wire.on(&git("pullrequests?"), Ok(response.clone()))
            .on(&git("pullrequests?"), Ok(response));
        assert_eq!(
            block(client.poll_batch(&key(), &[], &[], &ALL, None, NOW)),
            Err(expected)
        );
    }
}

#[test]
fn a_review_of_a_repository_too_large_for_whole_trees_reads_only_its_changes() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    let blob = |body: &[u8]| {
        Ok(Response {
            status: 200,
            body: body.to_vec(),
            ..Response::default()
        })
    };
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 2, false))
        .json(
            &git("pullRequests/7/iterations?"),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(2)}, "commonRefCommit": {"commitId": sha(800)}}]}),
        )
        .json(
            &git("pullRequests/7/iterations/1/changes?"),
            json!({"changeEntries": [
                {"item": {"path": "/src/main.rs", "objectId": sha(11), "originalObjectId": sha(12)}, "changeType": "edit"},
                {"item": {"path": "/src/new.rs", "objectId": sha(13)}, "changeType": "add"}
            ]}),
        )
        // The whole tree times out.
        .on(&git("items?"), Err(NetworkError))
        .on(&git(&format!("blobs/{}?", sha(12))), blob(b"fn old() {}\n"))
        .on(&git(&format!("blobs/{}?", sha(11))), blob(b"fn main() {}\n"))
        .on(&git(&format!("blobs/{}?", sha(13))), blob(b"fn new() {}\n"));
    let reader = reader(&wire, &runtime).unwrap();
    let context = reader.review_context(&remote(), 7, &sha(2)).unwrap();
    assert_eq!(
        context.files[0].patch.as_deref(),
        Some("@@ -1 +1 @@\n-fn old() {}\n+fn main() {}")
    );
    assert_eq!(
        context.files[1].patch.as_deref(),
        Some("@@ -0,0 +1 @@\n+fn new() {}")
    );
    assert_eq!(
        context.head.keys().collect::<Vec<_>>(),
        ["src/main.rs", "src/new.rs"]
    );
    assert_eq!(context.base.keys().collect::<Vec<_>>(), ["src/main.rs"]);
    assert!(
        !wire.urls().iter().any(|url| url.contains("/pullrequests?")),
        "a review never lists other pull requests"
    );
}

#[test]
fn changed_file_trees_read_unnamed_earlier_blobs_and_keep_deletions_and_submodules() {
    let runtime = runtime();
    let wire = Arc::new(FakeWire::default());
    signed_in(&wire);
    let blob = |body: &[u8]| {
        Ok(Response {
            status: 200,
            body: body.to_vec(),
            ..Response::default()
        })
    };
    wire.json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 2, false))
        .json(
            &git("pullRequests/7/iterations?"),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(2)}, "commonRefCommit": {"commitId": sha(800)}}]}),
        )
        .json(
            &git("pullRequests/7/iterations/1/changes?"),
            json!({"changeEntries": [
                {"item": {"path": "/src/main.rs", "objectId": sha(11)}, "changeType": "edit"},
                {"item": {"path": "/src/old.rs", "objectId": sha(14)}, "changeType": "delete"},
                {"item": {"path": "/vendor/lib", "objectId": sha(15), "originalObjectId": sha(16), "gitObjectType": "commit"}, "changeType": "edit"}
            ]}),
        )
        .on(&git("items?scopePath"), Ok(Response {
            status: 200,
            oversize: true,
            ..Response::default()
        }))
        .json(
            &git("items?path=%2Fsrc%2Fmain.rs"),
            json!({"objectId": sha(12), "gitObjectType": "blob", "path": "/src/main.rs"}),
        )
        .on(&git(&format!("blobs/{}?", sha(12))), blob(b"fn old() {}\n"))
        .on(&git(&format!("blobs/{}?", sha(11))), blob(b"fn main() {}\n"))
        .on(&git(&format!("blobs/{}?", sha(14))), blob(b"gone\n"));
    let reader = reader(&wire, &runtime).unwrap();
    let context = reader.review_context(&remote(), 7, &sha(2)).unwrap();
    let patch = |path: &str| {
        context
            .files
            .iter()
            .find(|f| f.path == path)
            .unwrap()
            .patch
            .clone()
    };
    assert_eq!(
        patch("src/main.rs").as_deref(),
        Some("@@ -1 +1 @@\n-fn old() {}\n+fn main() {}")
    );
    assert_eq!(patch("src/old.rs").as_deref(), Some("@@ -1 +0,0 @@\n-gone"));
    assert_eq!(patch("vendor/lib"), None);
    assert_eq!(context.base["src/old.rs"].sha, sha(14));
    let gitlink = &context.head["vendor/lib"];
    assert_eq!(
        (gitlink.kind.as_str(), gitlink.mode.as_str()),
        ("commit", "160000")
    );
    assert_eq!(context.base["vendor/lib"].sha, sha(16));
    assert!(!wire
        .urls()
        .iter()
        .any(|url| url.contains(&format!("blobs/{}", sha(15)))
            || url.contains(&format!("blobs/{}", sha(16)))));
}
