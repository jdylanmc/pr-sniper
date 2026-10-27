//! GitHub reads of PR state ([`PrSnapshot`]) for the heartbeat diff.

use super::{
    provider::{GithubClient, RemoteRepository},
    threads::QueryTransport,
    ConnectionError,
};
use crate::provider::PullRequestStateReader;
pub use crate::provider::{
    NormalizedComment, NormalizedSnapshot, OwnWrites, PrSnapshot, SnapshotCheck, SnapshotComment,
    SnapshotReview, SnapshotThread, StateMarks,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// PRs per GraphQL request. Each PR costs about one rate-limit point.
const BATCH: usize = 10;

const FRAGMENT: &str = r#"fragment S on PullRequest {
  fullDatabaseId number title body state isDraft mergeable reviewDecision baseRefOid headRefOid headRefName
  baseRef { branchProtectionRule { requiresStatusChecks requiredStatusCheckContexts } }
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
    let protection = pull
        .get("baseRef")
        .and_then(|r| r.get("branchProtectionRule"))
        .ok_or(ConnectionError::IncompleteRead)?;
    let required_checks = if protection.is_null() {
        Vec::new()
    } else {
        let required = protection["requiresStatusChecks"]
            .as_bool()
            .ok_or(ConnectionError::IncompleteRead)?;
        let names = protection["requiredStatusCheckContexts"]
            .as_array()
            .ok_or(ConnectionError::IncompleteRead)?
            .iter()
            .map(string)
            .collect::<Result<Vec<_>, _>>()?;
        if required {
            names
        } else {
            Vec::new()
        }
    };
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
        if check_total != checks.len() as u64 {
            return Err(ConnectionError::IncompleteRead);
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
        head_ref: pull["headRefName"].as_str().unwrap_or("").to_string(),
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
        required_checks,
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

impl<T: QueryTransport> PullRequestStateReader for GithubClient<T> {
    /// One PR, read fresh. Used at consume time and after own writes.
    fn pr_snapshot(
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
    fn pr_snapshots(
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
            required_checks: vec![],
            number: 1,
            pull_request_id: "9".into(),
            title: "t".into(),
            body: "b".into(),
            labels: vec!["b".into(), "a".into()],
            head_ref: "feature".into(),
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
        moved(|s| s.comments[0].body = "edited".into());
        moved(|s| s.threads[0].resolved = true);
        moved(|s| s.body = "new".into());
        moved(|s| s.head = "other".into());
        moved(|s| s.base = "other".into());
        moved(|s| s.draft = true);
    }

    #[test]
    fn ci_and_metadata_do_not_move_the_hash() {
        let hash = base().normalize().hash();
        let kept = |edit: fn(&mut PrSnapshot)| {
            let mut s = base();
            edit(&mut s);
            assert_eq!(s.normalize().hash(), hash);
            assert_ne!(
                s.normalize().legacy_hash(),
                base().normalize().legacy_hash()
            );
        };
        kept(|s| s.checks[0].conclusion = "FAILURE".into());
        kept(|s| s.checks[0].status = "IN_PROGRESS".into());
        kept(|s| s.check_total += 1);
        kept(|s| s.required_checks.push("macos".into()));
        kept(|s| s.mergeable = "CONFLICTING".into());
        kept(|s| s.labels.push("c".into()));
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
                "baseRef":{"branchProtectionRule":null},
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
