use base64::{engine::general_purpose::STANDARD, Engine};
use pr_sniper_lib::github::{
    provider::{GithubClient, RemoteRepository, Response, Transport},
    ConnectionError,
};
use pr_sniper_lib::provider::prelude::*;
use serde_json::json;
use std::{collections::BTreeMap, sync::Mutex};

struct Fixture {
    calls: Mutex<Vec<String>>,
    truncated: bool,
    changed_head: bool,
    missing_page: bool,
    changed_files: u64,
}

impl Transport for Fixture {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let mut calls = self.calls.lock().unwrap();
        let reread = calls.iter().any(|p| p == path);
        calls.push(path.into());
        let mut headers = BTreeMap::new();
        let body = if path == "/repos/example/repo/pulls/1" {
            json!({
                "id":9,"number":1,"title":"Hundreds of files","user":{"id":11,"login":"author"},
                "requested_reviewers":[],"requested_teams":[],"state":"open","merged":false,"draft":false,
                "head":{"sha":if reread && self.changed_head {"c".repeat(40)} else {"a".repeat(40)},"repo":{"id":100}},
                "base":{"sha":"b".repeat(40),"repo":{"id":100}},
                "updated_at":"2026-09-27T00:00:00Z","changed_files":self.changed_files
            })
        } else if path.starts_with("/repos/example/repo/pulls/1/files?") {
            let page: usize = path.rsplit('=').next().unwrap().parse().unwrap();
            if page < 4 && !(self.missing_page && page == 2) {
                headers.insert("link".into(),format!("<https://api.github.com/repos/example/repo/pulls/1/files?per_page=100&page={}>; rel=\"next\", <https://api.github.com/repos/example/repo/pulls/1/files?per_page=100&page=4>; rel=\"last\"",page+1));
            }
            json!(((page-1)*100..(page*100).min(301)).map(|i| {
                match i {
                    0 => json!({"filename":"renamed.rs","previous_filename":"old.rs","status":"renamed","patch":"@@ -1 +1 @@\n-old\n+new"}),
                    1 => json!({"filename":"removed.rs","status":"removed"}),
                    2 => json!({"filename":"binary.png","status":"modified"}),
                    3 => json!({"filename":"file-3.rs","status":"copied"}),
                    _ => json!({"filename":format!("file-{i}.rs"),"status":"modified"})
                }
            }).collect::<Vec<_>>())
        } else if path.contains("/compare/") {
            json!({"merge_base_commit":{"sha":"d".repeat(40)}})
        } else if path.contains("/git/trees/") {
            let base = path.contains(&"d".repeat(40));
            json!({"truncated":self.truncated,"tree":(0..301).filter_map(|i| {
                if base && i==3 { return None; }
                let path = match i {
                    0 => if base {"old.rs".into()} else {"renamed.rs".into()},
                    1 if !base => return None,
                    1 => "removed.rs".into(),
                    2 => "binary.png".into(),
                    _ => format!("file-{i}.rs")
                };
                Some(json!({"path":path,"mode":"100644","type":"blob","sha":format!("{i:040x}")}))
            }).collect::<Vec<_>>()})
        } else if path.contains("/git/blobs/") {
            let sha = path.rsplit('/').next().unwrap();
            let bytes: &[u8] = if sha == format!("{:040x}", 2) {
                &[0, 255, 128]
            } else {
                b"source\n"
            };
            json!({"sha":sha,"encoding":"base64","size":bytes.len(),"content":STANDARD.encode(bytes)})
        } else {
            panic!("Unexpected or mutating endpoint: {path}");
        };
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

fn fixture() -> Fixture {
    Fixture {
        calls: Mutex::new(vec![]),
        truncated: false,
        changed_head: false,
        missing_page: false,
        changed_files: 301,
    }
}
fn repository() -> RemoteRepository {
    RemoteRepository {
        id: "100".into(),
        name: "example/repo".into(),
    }
}

#[test]
fn complete_301_file_context_includes_renames_deletions_binary_and_all_pages() {
    let client = GithubClient::new(fixture());
    let context = client
        .review_context(&repository(), 1, &"a".repeat(40))
        .unwrap();
    assert_eq!(context.files.len(), 301);
    assert_eq!(context.base_revision, "d".repeat(40));
    assert_eq!(context.files[0].previous_path.as_deref(), Some("old.rs"));
    assert_eq!(context.files[1].status, "removed");
    assert_eq!(context.files[3].status, "copied");
    let source = client
        .file_content("example/repo", &context.head["file-300.rs"])
        .unwrap();
    assert_eq!(source["text"], "source\n");
    let binary = client
        .file_content("example/repo", &context.head["binary.png"])
        .unwrap();
    assert_eq!(binary["kind"], "binary");
    assert_eq!(binary["size"], 3);
}

#[test]
fn partial_pagination_truncated_tree_or_changed_head_never_returns_partial_success() {
    for mode in 0..3 {
        let mut f = fixture();
        match mode {
            0 => f.truncated = true,
            1 => f.changed_head = true,
            _ => f.missing_page = true,
        }
        assert!(GithubClient::new(f)
            .review_context(&repository(), 1, &"a".repeat(40))
            .is_err());
    }
}

#[test]
fn size_limits_are_permanent_but_a_missing_page_is_retryable() {
    let read = |f: Fixture| {
        GithubClient::new(f)
            .review_context(&repository(), 1, &"a".repeat(40))
            .unwrap_err()
    };
    // Past the listing cap, a listing short of the cap is a missing page.
    let mut over = fixture();
    over.changed_files = 3001;
    assert_eq!(read(over), ConnectionError::IncompleteRead);
    let mut truncated = fixture();
    truncated.truncated = true;
    assert_eq!(read(truncated), ConnectionError::LimitExceeded);
    let mut missing = fixture();
    missing.missing_page = true;
    assert_eq!(read(missing), ConnectionError::IncompleteRead);
}

#[test]
fn cancellation_fences_each_read_and_discards_an_inflight_response() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let client = GithubClient::new(fixture()).guarded(Arc::new(move || {
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err(ConnectionError::Configuration)
        }
    }));
    assert_eq!(
        client.pull_request(&repository(), 1),
        Err(ConnectionError::Configuration)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let client = GithubClient::new(fixture()).guarded(Arc::new(|| Err(ConnectionError::Timeout)));
    assert_eq!(
        client.pull_request(&repository(), 1),
        Err(ConnectionError::Timeout)
    );
}

/// GitHub lists the first 3,000 changed files (and advertises more pages);
/// the merge-base tree diff has every one. Listed first: `new.rs` renamed from
/// `old.rs` and `a.rs` copied from `z.rs`, which itself changed past the cap.
struct PastCap {
    calls: Mutex<Vec<String>>,
}

/// `f-2.rs` to `f-3200.rs`, the rename, the copy, `z.rs` and `gone.rs`.
const PAST_CAP: usize = 3_203;

fn past_cap_name(i: usize) -> String {
    match i {
        0 => "new.rs".into(),
        1 => "a.rs".into(),
        _ => format!("f-{i}.rs"),
    }
}

impl Transport for PastCap {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.calls.lock().unwrap().push(path.into());
        let mut headers = BTreeMap::new();
        let body = if path == "/repos/example/repo/pulls/1" {
            json!({
                "id":9,"number":1,"title":"Thousands of files","user":{"id":11,"login":"author"},
                "requested_reviewers":[],"requested_teams":[],"state":"open","merged":false,"draft":false,
                "head":{"sha":"a".repeat(40),"repo":{"id":100}},
                "base":{"sha":"b".repeat(40),"repo":{"id":100}},
                "updated_at":"2026-09-27T00:00:00Z","changed_files":PAST_CAP
            })
        } else if path.starts_with("/repos/example/repo/pulls/1/files?") {
            let page: usize = path.rsplit('=').next().unwrap().parse().unwrap();
            assert!(page <= 30, "read past the listing cap: {path}");
            headers.insert("link".into(), format!("<https://api.github.com/repos/example/repo/pulls/1/files?per_page=100&page={}>; rel=\"next\", <https://api.github.com/repos/example/repo/pulls/1/files?per_page=100&page=33>; rel=\"last\"", page + 1));
            json!(((page - 1) * 100..page * 100)
                .map(|i| match i {
                    0 => json!({"filename":"new.rs","previous_filename":"old.rs","status":"renamed"}),
                    1 => json!({"filename":"a.rs","previous_filename":"z.rs","status":"copied","patch":"@@ -1 +1 @@\n-a\n+b"}),
                    _ => json!({"filename":past_cap_name(i),"status":"modified","patch":"@@ -1 +1 @@\n-a\n+b"}),
                })
                .collect::<Vec<_>>())
        } else if path.contains("/compare/") {
            json!({"merge_base_commit":{"sha":"d".repeat(40)}})
        } else if path.contains("/git/trees/") {
            let base = path.contains(&"d".repeat(40));
            let blob = |path: &str, sha: String| json!({"path":path,"mode":"100644","type":"blob","sha":sha});
            let mut tree: Vec<_> = (2..3_201)
                .map(|i| {
                    blob(
                        &past_cap_name(i),
                        format!("{:040x}", if base { i } else { i + 10_000 }),
                    )
                })
                .collect();
            tree.push(blob("same.rs", "e".repeat(40)));
            tree.push(blob("z.rs", if base { "1" } else { "2" }.repeat(40)));
            if base {
                tree.push(blob("old.rs", "3".repeat(40)));
                tree.push(blob("gone.rs", "f".repeat(40)));
            } else {
                tree.push(blob("new.rs", "3".repeat(40)));
                tree.push(blob("a.rs", "1".repeat(40)));
            }
            json!({"truncated":false,"tree":tree})
        } else {
            panic!("Unexpected or mutating endpoint: {path}");
        };
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn past_the_listing_cap_the_tree_diff_completes_the_review_files() {
    let client = GithubClient::new(PastCap {
        calls: Mutex::new(vec![]),
    });
    let context = client
        .review_context(&repository(), 1, &"a".repeat(40))
        .unwrap();
    assert_eq!(context.files.len(), PAST_CAP);
    let paths: std::collections::BTreeSet<_> = context.files.iter().map(|f| &f.path).collect();
    assert_eq!(paths.len(), PAST_CAP);
    assert!(!paths.contains(&"same.rs".to_string()));
    // Listed files keep GitHub's patch; the rest have no addressable line.
    assert!(context.files[1..3_000].iter().all(|f| f.patch.is_some()));
    assert!(context.files[3_000..].iter().all(|f| f.patch.is_none()));
    let gone = context.files.iter().find(|f| f.path == "gone.rs").unwrap();
    assert_eq!(gone.status, "removed");
    let last = context
        .files
        .iter()
        .find(|f| f.path == "f-3200.rs")
        .unwrap();
    assert_eq!(last.status, "modified");
    // A copy source that changed past the listing is its own change; a
    // renamed file's deleted source is not listed twice.
    let source = context.files.iter().find(|f| f.path == "z.rs").unwrap();
    assert_eq!(source.status, "modified");
    assert!(source.patch.is_none());
    assert!(!paths.contains(&"old.rs".to_string()));
    // The packet manifest lists every changed file.
    let manifest = pr_sniper_lib::packet::reads::files(&context);
    assert_eq!(manifest.len(), PAST_CAP);
    assert!(manifest.iter().any(|f| f.path == "z.rs"));
}

#[test]
fn past_the_listing_cap_a_truncated_tree_never_returns_a_partial_list() {
    struct Truncated(PastCap);
    impl Transport for Truncated {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            let mut response = self.0.get(path)?;
            if path.contains("/git/trees/") {
                let mut body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
                body["truncated"] = json!(true);
                response.body = serde_json::to_vec(&body).unwrap();
            }
            Ok(response)
        }
    }
    let client = GithubClient::new(Truncated(PastCap {
        calls: Mutex::new(vec![]),
    }));
    assert_eq!(
        client
            .review_context(&repository(), 1, &"a".repeat(40))
            .unwrap_err(),
        ConnectionError::LimitExceeded
    );
}
