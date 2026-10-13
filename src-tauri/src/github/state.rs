//! Provider-side PR state for the heartbeat diff.
//!
//! `PrSnapshot` is the raw read. `NormalizedSnapshot` keeps only fields that
//! change when a human-visible PR state changes: sorted, no timestamps, no
//! transient provider values. Its hash is what the heartbeat compares.

use super::{
    provider::{GithubClient, RemoteRepository},
    threads::QueryTransport,
    ConnectionError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// PRs per GraphQL request. Each PR costs about one rate-limit point.
const BATCH: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCheck {
    pub name: String,
    pub status: String,
    pub conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotComment {
    pub id: String,
    pub body: String,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotThread {
    pub id: String,
    pub resolved: bool,
    pub outdated: bool,
    pub comments: Vec<SnapshotComment>,
    pub comment_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotReview {
    pub actor: String,
    pub state: String,
    pub head: Option<String>,
}

/// Raw provider read of one PR. Public so the packet builder and exits can
/// re-read fresh state at consume time and after their own writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSnapshot {
    pub number: u64,
    pub pull_request_id: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub head: String,
    pub base: String,
    pub state: String,
    pub draft: bool,
    pub mergeable: String,
    pub review_decision: Option<String>,
    pub reviews: Vec<SnapshotReview>,
    pub checks: Vec<SnapshotCheck>,
    pub check_total: u64,
    pub threads: Vec<SnapshotThread>,
    pub thread_total: u64,
    pub comments: Vec<SnapshotComment>,
    pub comment_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedComment {
    pub id: String,
    pub body_hash: String,
    pub resolved: bool,
    pub outdated: bool,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedSnapshot {
    pub head: String,
    pub base: String,
    pub text_hash: String,
    pub lifecycle: String,
    pub mergeable: String,
    pub review_decision: Option<String>,
    pub approvals: Vec<(String, String)>,
    pub labels: Vec<String>,
    pub checks: Vec<(String, String, String)>,
    pub check_total: u64,
    pub comments: Vec<NormalizedComment>,
    pub thread_total: u64,
    pub comment_total: u64,
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

impl PrSnapshot {
    /// Stable view: sorted, deduplicated, no timestamps. A re-read of an
    /// unchanged PR yields an identical value whatever order the provider used.
    pub fn normalize(&self) -> NormalizedSnapshot {
        let lifecycle = match self.state.as_str() {
            "MERGED" => "merged",
            "CLOSED" => "closed",
            _ if self.draft => "draft",
            _ => "open",
        };
        // UNKNOWN is a transient provider computation, not a PR change.
        let mergeable = if self.mergeable == "CONFLICTING" {
            "conflicting"
        } else {
            "ok"
        };
        let mut labels = self.labels.clone();
        labels.sort();
        labels.dedup();
        let mut approvals: Vec<_> = self
            .reviews
            .iter()
            .filter(|r| r.state == "APPROVED")
            .map(|r| (r.actor.clone(), r.head.clone().unwrap_or_default()))
            .collect();
        approvals.sort();
        approvals.dedup();
        let mut checks: Vec<_> = self
            .checks
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    check_status(&c.status).to_string(),
                    c.conclusion.to_ascii_lowercase(),
                )
            })
            .collect();
        checks.sort();
        checks.dedup();
        let mut comments: Vec<_> = self
            .comments
            .iter()
            .map(|c| NormalizedComment {
                id: c.id.clone(),
                body_hash: digest(&c.body),
                resolved: false,
                outdated: false,
                commit: c.commit.clone(),
            })
            .collect();
        for thread in &self.threads {
            for c in &thread.comments {
                comments.push(NormalizedComment {
                    id: c.id.clone(),
                    body_hash: digest(&c.body),
                    resolved: thread.resolved,
                    outdated: thread.outdated,
                    commit: c.commit.clone(),
                });
            }
        }
        comments.sort_by(|a, b| a.id.cmp(&b.id));
        comments.dedup_by(|a, b| a.id == b.id);
        NormalizedSnapshot {
            head: self.head.clone(),
            base: self.base.clone(),
            text_hash: digest(&format!("{}\n{}", self.title, self.body)),
            lifecycle: lifecycle.into(),
            mergeable: mergeable.into(),
            review_decision: self.review_decision.clone(),
            approvals,
            labels,
            checks,
            check_total: self.check_total,
            comments,
            thread_total: self.thread_total,
            comment_total: self.comment_total
                + self.threads.iter().map(|t| t.comment_total).sum::<u64>(),
        }
    }
}

fn check_status(status: &str) -> &'static str {
    match status {
        "COMPLETED" | "SUCCESS" | "FAILURE" | "ERROR" => "done",
        _ => "pending",
    }
}

impl NormalizedSnapshot {
    pub fn hash(&self) -> String {
        digest(&serde_json::to_string(self).expect("normalized snapshot serializes"))
    }
}

const FRAGMENT: &str = r#"fragment S on PullRequest {
  fullDatabaseId number title body state isDraft mergeable reviewDecision baseRefOid headRefOid
  labels(first:100) { nodes { name } }
  latestOpinionatedReviews(first:100) { nodes { state author { login } commit { oid } } }
  comments(last:100) { totalCount nodes { fullDatabaseId body } }
  commits(last:1) { nodes { commit { statusCheckRollup { contexts(first:100) {
    totalCount nodes { __typename
      ... on CheckRun { name status conclusion }
      ... on StatusContext { context state }
    }
  }}}}}
  reviewThreads(last:100) { totalCount nodes {
    id isResolved isOutdated
    comments(first:20) { totalCount nodes { fullDatabaseId body originalCommit { oid } } }
  }}
}"#;

fn string(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or(ConnectionError::InvalidResponse)
}

fn count(value: &Value) -> Result<u64, ConnectionError> {
    value.as_u64().ok_or(ConnectionError::InvalidResponse)
}

fn nodes(value: &Value) -> Result<&Vec<Value>, ConnectionError> {
    value["nodes"]
        .as_array()
        .ok_or(ConnectionError::IncompleteRead)
}

fn comment(value: &Value) -> Result<SnapshotComment, ConnectionError> {
    let id = match &value["fullDatabaseId"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return Err(ConnectionError::InvalidResponse),
    };
    Ok(SnapshotComment {
        id,
        body: string(&value["body"])?,
        commit: value["originalCommit"]["oid"].as_str().map(str::to_string),
    })
}

fn parse(pull: &Value) -> Result<PrSnapshot, ConnectionError> {
    let pull_request_id = match &pull["fullDatabaseId"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return Err(ConnectionError::InvalidResponse),
    };
    let rollup = &pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"];
    let mut checks = Vec::new();
    let mut check_total = 0;
    if !rollup.is_null() {
        check_total = count(&rollup["contexts"]["totalCount"])?;
        for context in nodes(&rollup["contexts"])? {
            checks.push(match context["__typename"].as_str() {
                Some("CheckRun") => SnapshotCheck {
                    name: string(&context["name"])?,
                    status: string(&context["status"])?,
                    conclusion: context["conclusion"].as_str().unwrap_or("").to_string(),
                },
                Some("StatusContext") => SnapshotCheck {
                    name: string(&context["context"])?,
                    status: string(&context["state"])?,
                    conclusion: string(&context["state"])?,
                },
                _ => return Err(ConnectionError::InvalidResponse),
            });
        }
    }
    let mut threads = Vec::new();
    for thread in nodes(&pull["reviewThreads"])? {
        threads.push(SnapshotThread {
            id: string(&thread["id"])?,
            resolved: thread["isResolved"]
                .as_bool()
                .ok_or(ConnectionError::InvalidResponse)?,
            outdated: thread["isOutdated"]
                .as_bool()
                .ok_or(ConnectionError::InvalidResponse)?,
            comments: nodes(&thread["comments"])?
                .iter()
                .map(comment)
                .collect::<Result<_, _>>()?,
            comment_total: count(&thread["comments"]["totalCount"])?,
        });
    }
    Ok(PrSnapshot {
        number: count(&pull["number"])?,
        pull_request_id,
        title: string(&pull["title"])?,
        body: pull["body"].as_str().unwrap_or("").to_string(),
        labels: nodes(&pull["labels"])?
            .iter()
            .map(|l| string(&l["name"]))
            .collect::<Result<_, _>>()?,
        head: string(&pull["headRefOid"])?,
        base: string(&pull["baseRefOid"])?,
        state: string(&pull["state"])?,
        draft: pull["isDraft"]
            .as_bool()
            .ok_or(ConnectionError::InvalidResponse)?,
        mergeable: string(&pull["mergeable"])?,
        review_decision: pull["reviewDecision"].as_str().map(str::to_string),
        reviews: nodes(&pull["latestOpinionatedReviews"])?
            .iter()
            .map(|r| {
                Ok(SnapshotReview {
                    actor: r["author"]["login"].as_str().unwrap_or("").to_string(),
                    state: string(&r["state"])?,
                    head: r["commit"]["oid"].as_str().map(str::to_string),
                })
            })
            .collect::<Result<_, ConnectionError>>()?,
        checks,
        check_total,
        thread_total: count(&pull["reviewThreads"]["totalCount"])?,
        threads,
        comment_total: count(&pull["comments"]["totalCount"])?,
        comments: nodes(&pull["comments"])?
            .iter()
            .map(comment)
            .collect::<Result<_, _>>()?,
    })
}

impl<T: QueryTransport> GithubClient<T> {
    /// One PR, read fresh. Used at consume time and after own writes.
    pub fn pr_snapshot(
        &self,
        repo: &RemoteRepository,
        number: u64,
    ) -> Result<PrSnapshot, ConnectionError> {
        self.pr_snapshots(repo, &[number])?
            .remove(&number)
            .ok_or(ConnectionError::IncompleteRead)
    }

    /// Heartbeat read: up to `BATCH` PRs per GraphQL request via aliases.
    /// A PR the provider no longer returns is absent from the map.
    pub fn pr_snapshots(
        &self,
        repo: &RemoteRepository,
        numbers: &[u64],
    ) -> Result<BTreeMap<u64, PrSnapshot>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repo.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (owner, name_part) = name
            .split_once('/')
            .ok_or(ConnectionError::InvalidRepository)?;
        let mut result = BTreeMap::new();
        for chunk in numbers.chunks(BATCH) {
            let fields: String = chunk
                .iter()
                .map(|n| format!("p{n}:pullRequest(number:{n}){{...S}} "))
                .collect();
            let query = format!(
                "query($owner:String!,$name:String!){{repository(owner:$owner,name:$name){{databaseId {fields}}}}} {FRAGMENT}"
            );
            let value = self.graph(&query, json!({"owner":owner,"name":name_part}))?;
            let repository = &value["repository"];
            if repository["databaseId"].as_u64().map(|id| id.to_string()) != Some(repo.id.clone())
                && repository["databaseId"].as_str() != Some(repo.id.as_str())
            {
                return Err(ConnectionError::RepositoryChanged);
            }
            for number in chunk {
                let pull = &repository[format!("p{number}")];
                if pull.is_null() {
                    continue;
                }
                let snapshot = parse(pull)?;
                if snapshot.number != *number {
                    return Err(ConnectionError::InvalidResponse);
                }
                result.insert(*number, snapshot);
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PrSnapshot {
        PrSnapshot {
            number: 1,
            pull_request_id: "9".into(),
            title: "t".into(),
            body: "b".into(),
            labels: vec!["b".into(), "a".into()],
            head: "h".into(),
            base: "s".into(),
            state: "OPEN".into(),
            draft: false,
            mergeable: "MERGEABLE".into(),
            review_decision: None,
            reviews: vec![
                SnapshotReview {
                    actor: "z".into(),
                    state: "APPROVED".into(),
                    head: Some("h".into()),
                },
                SnapshotReview {
                    actor: "a".into(),
                    state: "COMMENTED".into(),
                    head: None,
                },
            ],
            checks: vec![
                SnapshotCheck {
                    name: "b".into(),
                    status: "COMPLETED".into(),
                    conclusion: "SUCCESS".into(),
                },
                SnapshotCheck {
                    name: "a".into(),
                    status: "IN_PROGRESS".into(),
                    conclusion: "".into(),
                },
            ],
            check_total: 2,
            threads: vec![SnapshotThread {
                id: "t1".into(),
                resolved: false,
                outdated: false,
                comments: vec![SnapshotComment {
                    id: "20".into(),
                    body: "x".into(),
                    commit: Some("h".into()),
                }],
                comment_total: 1,
            }],
            thread_total: 1,
            comments: vec![SnapshotComment {
                id: "10".into(),
                body: "y".into(),
                commit: None,
            }],
            comment_total: 1,
        }
    }

    #[test]
    fn normalization_ignores_order_and_transient_provider_values() {
        let first = base().normalize();
        let mut shuffled = base();
        shuffled.labels.reverse();
        shuffled.checks.reverse();
        shuffled.reviews.reverse();
        shuffled.comments.push(shuffled.comments[0].clone());
        shuffled.mergeable = "UNKNOWN".into();
        let second = shuffled.normalize();
        assert_eq!(first, second);
        assert_eq!(first.hash(), second.hash());
        assert_eq!(first.labels, ["a", "b"]);
        assert_eq!(first.checks[0].0, "a");
        assert_eq!(first.approvals, [("z".to_string(), "h".to_string())]);
    }

    #[test]
    fn meaningful_changes_move_the_hash() {
        let hash = base().normalize().hash();
        let moved = |edit: fn(&mut PrSnapshot)| {
            let mut s = base();
            edit(&mut s);
            assert_ne!(s.normalize().hash(), hash);
        };
        moved(|s| s.checks[0].conclusion = "FAILURE".into());
        moved(|s| s.comments[0].body = "edited".into());
        moved(|s| s.threads[0].resolved = true);
        moved(|s| s.body = "new".into());
        moved(|s| s.head = "other".into());
        moved(|s| s.draft = true);
        moved(|s| s.mergeable = "CONFLICTING".into());
        moved(|s| s.labels.push("c".into()));
    }

    struct Wire;
    impl super::super::provider::Transport for Wire {
        fn get(&self, _: &str) -> Result<super::super::provider::Response, ConnectionError> {
            unreachable!()
        }
    }
    impl QueryTransport for Wire {
        fn query(
            &self,
            query: &str,
            _: Value,
        ) -> Result<super::super::provider::Response, ConnectionError> {
            // Two PRs ride one request through aliases.
            assert!(query.contains("p1:pullRequest(number:1)"));
            assert!(query.contains("p2:pullRequest(number:2)"));
            let pull = |n: u64| {
                json!({"fullDatabaseId":"9","number":n,"title":"t","body":null,"state":"OPEN",
                "isDraft":false,"mergeable":"UNKNOWN","reviewDecision":null,
                "baseRefOid":"s","headRefOid":"h",
                "labels":{"nodes":[{"name":"x"}]},
                "latestOpinionatedReviews":{"nodes":[{"state":"APPROVED","author":{"login":"a"},"commit":{"oid":"h"}}]},
                "comments":{"totalCount":1,"nodes":[{"fullDatabaseId":7,"body":"c"}]},
                "commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{"totalCount":2,"nodes":[
                    {"__typename":"CheckRun","name":"ci","status":"IN_PROGRESS","conclusion":null},
                    {"__typename":"StatusContext","context":"legacy","state":"SUCCESS"}]}}}}]},
                "reviewThreads":{"totalCount":1,"nodes":[{"id":"t","isResolved":false,"isOutdated":true,
                    "comments":{"totalCount":1,"nodes":[{"fullDatabaseId":8,"body":"r","originalCommit":{"oid":"h"}}]}}]}})
            };
            let body = json!({"data":{"repository":{"databaseId":100,"p1":pull(1),"p2":pull(2)}}});
            Ok(super::super::provider::Response {
                status: 200,
                headers: BTreeMap::new(),
                body: serde_json::to_vec(&body).unwrap(),
            })
        }
    }

    #[test]
    fn batched_read_parses_every_pr_and_ignores_transient_values() {
        let client = GithubClient::new(Wire);
        let repo = RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        };
        let all = client.pr_snapshots(&repo, &[1, 2]).unwrap();
        assert_eq!(all.len(), 2);
        let one = &all[&1];
        assert_eq!(one.body, "");
        assert_eq!(one.checks.len(), 2);
        assert_eq!(one.threads[0].comments[0].commit.as_deref(), Some("h"));
        let normalized = one.normalize();
        assert_eq!(normalized.mergeable, "ok");
        assert_eq!(normalized.checks[0].1, "pending");
        assert!(normalized.comments.iter().any(|c| c.outdated));
        let other = GithubClient::new(Wire)
            .pr_snapshots(
                &RemoteRepository {
                    id: "5".into(),
                    name: "example/repo".into(),
                },
                &[1, 2],
            )
            .unwrap_err();
        assert_eq!(other, ConnectionError::RepositoryChanged);
    }
}
