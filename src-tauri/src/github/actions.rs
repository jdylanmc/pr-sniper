use super::{
    provider::{parse_response, GithubClient, RemoteRepository, Transport},
    publication::{MutationTransport, Request},
    threads::QueryTransport,
    ConnectionError,
};
pub use crate::provider::{
    Action, HumanThread, MergeMethod, Observation, OutcomeReceipt as Receipt, ProviderReview,
    BEHIND_BASE,
};
use crate::provider::{CommentReader, ReadinessReader, ReviewerOutcome};
use crate::{publication::WriteFailure, review::Failure};
use serde_json::{json, Value};

fn text(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(ConnectionError::InvalidResponse)
}
fn id(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .ok_or(ConnectionError::InvalidResponse)
}
fn boolean(value: &Value) -> Result<bool, ConnectionError> {
    value.as_bool().ok_or(ConnectionError::InvalidResponse)
}
fn timestamp(value: &Value) -> Result<Option<String>, ConnectionError> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) if chrono::DateTime::parse_from_rfc3339(value).is_ok() => {
            Ok(Some(value.clone()))
        }
        _ => Err(ConnectionError::InvalidResponse),
    }
}
fn complete(value: &Value) -> Result<&Vec<Value>, ConnectionError> {
    let nodes = value["nodes"]
        .as_array()
        .ok_or(ConnectionError::IncompleteRead)?;
    // A single `first: 100` page is the cap; more is a limit, not a bad read.
    if value["pageInfo"]["hasNextPage"] == true {
        return Err(ConnectionError::LimitExceeded);
    }
    if value["totalCount"].as_u64() != Some(nodes.len() as u64)
        || value["pageInfo"]["hasNextPage"] != false
    {
        return Err(ConnectionError::IncompleteRead);
    }
    Ok(nodes)
}

impl<T: QueryTransport> ReadinessReader for GithubClient<T> {
    fn action_observation(
        &self,
        repo: &RemoteRepository,
        number: u64,
    ) -> Result<Observation, ConnectionError> {
        let name = crate::storage::canonical_repository(&repo.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (owner, name_part) = name
            .split_once('/')
            .ok_or(ConnectionError::InvalidRepository)?;
        let query = r#"query($owner:String!,$name:String!,$number:Int!){
          viewer { databaseId }
          repository(owner:$owner,name:$name) {
            databaseId viewerPermission viewerDefaultMergeMethod mergeCommitAllowed rebaseMergeAllowed squashMergeAllowed
            pullRequest(number:$number) {
              id fullDatabaseId baseRefOid baseRefName headRefOid isDraft state mergeStateStatus mergeable reviewDecision isInMergeQueue
              author { ... on User { databaseId } ... on Bot { databaseId } }
              headRepository { databaseId }
              mergedBy { ... on User { databaseId } ... on Bot { databaseId } } mergedAt mergeCommit { oid }
              baseRef { branchProtectionRule {
                requiresApprovingReviews requiredApprovingReviewCount requiresCodeOwnerReviews
                requiresConversationResolution requiresStatusChecks requiresStrictStatusChecks requiredStatusCheckContexts
              }}
              commits(last:1) { nodes { commit { oid statusCheckRollup { state contexts(first:100) {
                totalCount pageInfo { hasNextPage } nodes { __typename
                  ... on CheckRun { name status conclusion databaseId checkSuite { app { slug } } }
                  ... on StatusContext { context state }
                }
              }}}}}
              reviewThreads(first:100) { totalCount pageInfo { hasNextPage } nodes {
                id isResolved isOutdated resolvedBy { login }
                comments(first:100) { totalCount pageInfo { hasNextPage } nodes { fullDatabaseId body updatedAt path line originalLine author { login ... on User { databaseId } ... on Bot { databaseId } } originalCommit { oid } } }
              }}
            }
          }
        }"#;
        let value = self.graph(
            query,
            json!({"owner":owner,"name":name_part,"number":number}),
        )?;
        let repository = &value["repository"];
        if id(&repository["databaseId"])? != repo.id {
            return Err(ConnectionError::RepositoryChanged);
        }
        let pull = &repository["pullRequest"];
        let head = super::metadata::sha(&pull["headRefOid"])?;
        let commits = pull["commits"]["nodes"]
            .as_array()
            .ok_or(ConnectionError::IncompleteRead)?;
        if commits.len() != 1 || commits[0]["commit"]["oid"].as_str() != Some(&head) {
            return Err(ConnectionError::RevisionChanged);
        }
        let rollup = &commits[0]["commit"]["statusCheckRollup"];
        let mut checks = if rollup.is_null() {
            None
        } else {
            let contexts = complete(&rollup["contexts"])?;
            let valid = contexts.iter().all(|c| match c["__typename"].as_str() {
                Some("CheckRun") => {
                    c["status"] == "COMPLETED"
                        && matches!(
                            c["conclusion"].as_str(),
                            Some("SUCCESS" | "NEUTRAL" | "SKIPPED")
                        )
                }
                Some("StatusContext") => c["state"] == "SUCCESS",
                _ => false,
            });
            if contexts.is_empty() {
                None
            } else {
                Some(if valid {
                    text(&rollup["state"])?
                } else {
                    "NONPASSING".into()
                })
            }
        };
        let protection = pull
            .get("baseRef")
            .and_then(|r| r.get("branchProtectionRule"))
            .ok_or(ConnectionError::IncompleteRead)?;
        let check_contexts = if rollup.is_null() {
            Vec::new()
        } else {
            complete(&rollup["contexts"])?.clone()
        };
        if !protection.is_null() {
            for field in [
                "requiresApprovingReviews",
                "requiresCodeOwnerReviews",
                "requiresConversationResolution",
                "requiresStatusChecks",
                "requiresStrictStatusChecks",
            ] {
                boolean(&protection[field])?;
            }
            if protection["requiresStatusChecks"] == true
                && protection["requiredStatusCheckContexts"]
                    .as_array()
                    .is_some_and(|required| {
                        required.iter().any(|name| {
                            !check_contexts
                                .iter()
                                .any(|c| c["name"] == *name || c["context"] == *name)
                        })
                    })
            {
                checks = Some("MISSING_REQUIRED".into());
            }
            let review_count_complete = match protection.get("requiredApprovingReviewCount") {
                Some(Value::Null) => protection["requiresApprovingReviews"] == false,
                Some(count) => count.as_u64().is_some(),
                None => false,
            };
            if !review_count_complete
                || protection["requiredStatusCheckContexts"]
                    .as_array()
                    .is_none_or(|values| values.iter().any(|v| v.as_str().is_none()))
            {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        if !matches!(pull.get("reviewDecision"), Some(Value::Null))
            && !matches!(
                pull["reviewDecision"].as_str(),
                Some("APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED")
            )
        {
            return Err(ConnectionError::IncompleteRead);
        }
        let mut threads = Vec::new();
        for thread in complete(&pull["reviewThreads"])? {
            threads.push(HumanThread {
                id: text(&thread["id"])?,
                resolved: boolean(&thread["isResolved"])?,
                outdated: boolean(&thread["isOutdated"])?,
                resolved_by: thread["resolvedBy"]["login"].as_str().map(str::to_string),
                comments: complete(&thread["comments"])?.clone(),
            });
        }
        let method = match repository["viewerDefaultMergeMethod"].as_str() {
            Some("MERGE") if repository["mergeCommitAllowed"] == true => Some(MergeMethod::Merge),
            Some("REBASE") if repository["rebaseMergeAllowed"] == true => Some(MergeMethod::Rebase),
            Some("SQUASH") if repository["squashMergeAllowed"] == true => Some(MergeMethod::Squash),
            _ => None,
        };
        let reviews = self.action_reviews(repo, number)?;
        let comments = self.top_comments(repo, number)?;
        let base_name = text(&pull["baseRefName"])?;
        let mut rules_url = url::Url::parse(&format!(
            "https://api.github.com/repos/{name}/rules/branches/"
        ))
        .map_err(|_| ConnectionError::InvalidRepository)?;
        rules_url
            .path_segments_mut()
            .map_err(|_| ConnectionError::InvalidRepository)?
            .pop_if_empty()
            .push(&base_name);
        let (merge_rules,merge_rules_error)=match self.read(rules_url.path()){
            Ok((rules,response)) if !response.headers.contains_key("link") && rules.as_array()
                .is_some_and(|r|r.len()<=1000&&r.iter().all(|rule|rule["type"].as_str().is_some()))=>(rules.as_array().cloned(),None),
            Ok(_)=>(None,Some("Active repository merge rules were incomplete or unsupported; direct merge is unavailable.".into())),
            Err(error @ (ConnectionError::RateLimited|ConnectionError::RateLimitedAfter(_)|ConnectionError::RateLimitedWithContext { .. }|ConnectionError::Network
                |ConnectionError::Timeout|ConnectionError::ProviderFailure|ConnectionError::ProviderFailureAfter(_)))=>return Err(error),
            Err(error)=>(None,Some(format!("Active repository merge rules are unavailable ({error:?}); approval eligibility remains separate."))),
        };
        let observation = Observation {
            write_capability: false,
            node_id: text(&pull["id"])?,
            repository_id: repo.id.clone(),
            pull_request_id: id(&pull["fullDatabaseId"])?,
            account_id: id(&value["viewer"]["databaseId"])?,
            author_id: id(&pull["author"]["databaseId"])?,
            head_repository_id: match pull.get("headRepository") {
                Some(Value::Null) => None,
                Some(repo) => Some(id(&repo["databaseId"])?),
                None => return Err(ConnectionError::IncompleteRead),
            },
            head,
            base: super::metadata::sha(&pull["baseRefOid"])?,
            base_name,
            merge_rules,
            merge_rules_error,
            state: text(&pull["state"])?,
            draft: boolean(&pull["isDraft"])?,
            permission: text(&repository["viewerPermission"])?,
            mergeable: text(&pull["mergeable"])?,
            merge_state: text(&pull["mergeStateStatus"])?,
            review_decision: pull["reviewDecision"].as_str().map(str::to_string),
            in_merge_queue: boolean(&pull["isInMergeQueue"])?,
            method,
            checks,
            check_contexts,
            protection: protection.clone(),
            threads,
            reviews,
            comments,
            merged_by: pull["mergedBy"]["databaseId"]
                .as_u64()
                .map(|n| n.to_string()),
            merged_at: timestamp(&pull["mergedAt"])?,
            merge_commit: if pull["mergeCommit"].is_null() {
                None
            } else {
                Some(super::metadata::sha(&pull["mergeCommit"]["oid"])?)
            },
        };
        if serde_json::to_vec(&observation)
            .map_err(|_| ConnectionError::InvalidResponse)?
            .len()
            > 1024 * 1024
        {
            return Err(ConnectionError::LimitExceeded);
        }
        Ok(observation)
    }

    fn action_reviews(
        &self,
        repo: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ProviderReview>, ConnectionError> {
        self.reviews(repo, number)
    }
}

impl<T: Transport> GithubClient<T> {
    fn reviews(
        &self,
        repo: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ProviderReview>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repo.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        self.pages_limited(
            &format!("/repos/{name}/pulls/{number}/reviews?per_page=100&page=1"),
            1000,
        )?
        .iter()
        .map(|v| {
            Ok(ProviderReview {
                id: id(&v["id"])?,
                actor_id: id(&v["user"]["id"])?,
                head: super::metadata::sha(&v["commit_id"])?,
                state: text(&v["state"])?,
                body: v["body"]
                    .as_str()
                    .ok_or(ConnectionError::InvalidResponse)?
                    .into(),
                submitted_at: timestamp(&v["submitted_at"])?,
            })
        })
        .collect()
    }
}

impl<T: MutationTransport> ReviewerOutcome for GithubClient<T> {
    fn approve_exact(
        &self,
        repo: &RemoteRepository,
        number: u64,
        account: &str,
        head: &str,
        body: &str,
    ) -> Result<Receipt, WriteFailure> {
        let name = crate::storage::canonical_repository(&repo.name).map_err(|e| WriteFailure {
            failure: Failure::permanent(e),
            uncertain: false,
        })?;
        // Idempotent against the provider: an approval this account already
        // gave on this head (a lost response, a reopened PR whose local
        // evidence is gone) is the receipt; nothing is posted twice.
        let existing = self.reviews(repo, number).map_err(|e| WriteFailure {
            failure: e.into(),
            uncertain: false,
        })?;
        if let Some(review) = existing
            .iter()
            .find(|r| r.actor_id == account && r.head == head && r.state == "APPROVED")
        {
            return Ok(Receipt {
                id: review.id.clone(),
                actor_id: account.into(),
                head: head.into(),
                action: Action::Approve,
                merge_commit: None,
            });
        }
        let response = self
            .transport
            .mutate(
                &format!("/repos/{name}/pulls/{number}/reviews"),
                Request::Post(json!({"event":"APPROVE","commit_id":head,"body":body})),
            )
            .map_err(|e| WriteFailure {
                failure: e.into(),
                uncertain: true,
            })?;
        if response.status != 200 && response.status != 201 {
            let uncertain = !(400..500).contains(&response.status) || response.status == 408;
            return Err(WriteFailure {
                failure: parse_response(response)
                    .err()
                    .map(Failure::from)
                    .unwrap_or_else(|| Failure::permanent("GitHub did not confirm approval.")),
                uncertain,
            });
        }
        let parsed = (|| {
            let v: Value = serde_json::from_slice(&response.body)
                .map_err(|_| ConnectionError::InvalidResponse)?;
            if v["state"] != "APPROVED"
                || v["body"].as_str() != Some(body)
                || v["commit_id"].as_str() != Some(head)
                || id(&v["user"]["id"])? != account
                || timestamp(&v["submitted_at"])?.is_none()
            {
                return Err(ConnectionError::InvalidResponse);
            }
            Ok(Receipt {
                id: id(&v["id"])?,
                actor_id: account.into(),
                head: head.into(),
                action: Action::Approve,
                merge_commit: None,
            })
        })();
        parsed.map_err(|e: ConnectionError| WriteFailure {
            failure: e.into(),
            uncertain: true,
        })
    }

    fn merge_exact(
        &self,
        observation: &Observation,
        request_id: &str,
    ) -> Result<Receipt, WriteFailure> {
        let method = observation.method.as_ref().ok_or_else(|| WriteFailure {
            failure: Failure::permanent("Provider merge method unavailable."),
            uncertain: false,
        })?;
        let query="mutation($input:MergePullRequestInput!){mergePullRequest(input:$input){clientMutationId pullRequest{id headRefOid merged mergedAt mergedBy{... on User{databaseId} ... on Bot{databaseId}} mergeCommit{oid}}}}";
        let response=self.transport.mutate("/graphql",Request::Post(json!({"query":query,"variables":{"input":{
            "pullRequestId":observation.node_id,"expectedHeadOid":observation.head,"mergeMethod":method,"clientMutationId":request_id
        }}}))).map_err(|e|WriteFailure{failure:e.into(),uncertain:true})?;
        let (value, _) = parse_response(response).map_err(|e| WriteFailure {
            failure: e.into(),
            uncertain: true,
        })?;
        if value
            .get("errors")
            .is_some_and(|e| !e.is_null() && e.as_array().is_none_or(|a| !a.is_empty()))
        {
            return Err(WriteFailure{failure:Failure::permanent("GitHub rejected or could not confirm the exact-head merge; reconciliation is required."),uncertain:true});
        }
        let parsed = (|| {
            let data = &value["data"]["mergePullRequest"];
            let p = &data["pullRequest"];
            if data["clientMutationId"].as_str() != Some(request_id)
                || p["id"].as_str() != Some(&observation.node_id)
                || p["merged"] != true
                || p["headRefOid"].as_str() != Some(&observation.head)
                || id(&p["mergedBy"]["databaseId"])? != observation.account_id
                || timestamp(&p["mergedAt"])?.is_none()
            {
                return Err(ConnectionError::InvalidResponse);
            }
            Ok(Receipt {
                id: observation.node_id.clone(),
                actor_id: observation.account_id.clone(),
                head: observation.head.clone(),
                action: Action::Merge,
                merge_commit: Some(super::metadata::sha(&p["mergeCommit"]["oid"])?),
            })
        })();
        parsed.map_err(|e: ConnectionError| WriteFailure {
            failure: e.into(),
            uncertain: true,
        })
    }
}

#[cfg(test)]
mod complete_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_further_page_is_a_limit_but_a_count_mismatch_is_incomplete() {
        let more = json!({"totalCount": 101, "pageInfo": {"hasNextPage": true}, "nodes": [{}]});
        assert_eq!(complete(&more), Err(ConnectionError::LimitExceeded));
        let short = json!({"totalCount": 2, "pageInfo": {"hasNextPage": false}, "nodes": [{}]});
        assert_eq!(complete(&short), Err(ConnectionError::IncompleteRead));
        let whole = json!({"totalCount": 1, "pageInfo": {"hasNextPage": false}, "nodes": [{}]});
        assert_eq!(complete(&whole).map(Vec::len), Ok(1));
    }
}
