use base64::{engine::general_purpose::STANDARD, Engine};
use pr_sniper_lib::github::{
    provider::{GithubClient, RemoteRepository, Response, Transport},
    ConnectionError,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Mutex};

struct Fixture {
    calls: Mutex<Vec<String>>,
    truncated: bool,
    changed_head: bool,
    missing_page: bool,
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
                "updated_at":"2026-09-27T00:00:00Z","changed_files":301
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
        .review_source("example/repo", &context.head["file-300.rs"])
        .unwrap();
    assert_eq!(source["text"], "source\n");
    let binary = client
        .review_source("example/repo", &context.head["binary.png"])
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
        client.review_pull(&repository(), 1),
        Err(ConnectionError::Configuration)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let client = GithubClient::new(fixture()).guarded(Arc::new(|| Err(ConnectionError::Timeout)));
    assert_eq!(
        client.review_pull(&repository(), 1),
        Err(ConnectionError::Timeout)
    );
}
