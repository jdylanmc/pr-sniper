use super::{
    conversation::TopComment,
    provider::{parse_response, GithubClient, RemoteRepository, Transport},
    publication::{MutationTransport, Request},
    threads::QueryTransport,
    ConnectionError,
};
use crate::{publication::WriteFailure, review::Failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Approve,
    Merge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeMethod {
    Merge,
    Rebase,
    Squash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReview {
    pub id: String,
    pub actor_id: String,
    pub head: String,
    pub state: String,
    pub body: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanThread {
    pub id: String,
    pub resolved: bool,
    pub outdated: bool,
    pub resolved_by: Option<String>,
    pub comments: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub write_capability: bool,
    pub node_id: String,
    pub repository_id: String,
    pub pull_request_id: String,
    pub account_id: String,
    pub author_id: String,
    pub head_repository_id: Option<String>,
    pub head: String,
    pub base: String,
    pub base_name: String,
    pub merge_rules: Option<Vec<Value>>,
    pub merge_rules_error: Option<String>,
    pub state: String,
    pub draft: bool,
    pub permission: String,
    pub mergeable: String,
    pub merge_state: String,
    pub review_decision: Option<String>,
    pub checks: Option<String>,
    pub check_contexts: Vec<Value>,
    pub in_merge_queue: bool,
    pub method: Option<MergeMethod>,
    pub protection: Value,
    pub threads: Vec<HumanThread>,
    pub reviews: Vec<ProviderReview>,
    pub comments: Vec<TopComment>,
    pub merged_by: Option<String>,
    pub merged_at: Option<String>,
    pub merge_commit: Option<String>,
}

impl Observation {
    pub fn common_blocker(&self) -> Option<String> {
        if self.state != "OPEN" {
            return Some(format!("GitHub reports {}.", self.state));
        }
        if self.draft {
            return Some("Draft pull requests cannot be approved or merged.".into());
        }
        if !self.write_capability
            || !matches!(self.permission.as_str(), "WRITE" | "MAINTAIN" | "ADMIN")
        {
            return Some(
                "The acting account lacks verified repository write/review authority.".into(),
            );
        }
        self.discussion_blocker()
    }
    pub fn discussion_blocker(&self) -> Option<String> {
        if self.reviews.iter().any(|r| r.state == "PENDING") {
            return Some("An account review is still pending; it will not be submitted or replaced by automation.".into());
        }
        if self.threads.iter().any(|t| !t.resolved) {
            return Some(
                "Unresolved provider discussions remain, including outdated threads.".into(),
            );
        }
        let mut latest = std::collections::BTreeMap::new();
        for review in &self.reviews {
            if review.state == "APPROVED"
                || review.state == "CHANGES_REQUESTED"
                || review.state == "DISMISSED"
            {
                latest.insert(&review.actor_id, review);
            }
        }
        if latest.values().any(|r| r.state == "CHANGES_REQUESTED") {
            return Some("A provider reviewer is still requesting changes.".into());
        }
        None
    }
    pub fn blocker(&self, action: Action) -> Option<String> {
        if let Some(reason) = self.common_blocker() {
            return Some(reason);
        }
        match action {
            Action::Approve if self.author_id == self.account_id => {
                Some("GitHub does not allow the PR author to approve their own PR.".into())
            }
            Action::Approve => None,
            Action::Merge if self.in_merge_queue => {
                Some("This PR is in a provider merge queue; direct merge is unavailable.".into())
            }
            Action::Merge if self.merge_rules_error.is_some() => self.merge_rules_error.clone(),
            Action::Merge if self.merge_rules.is_none() => {
                Some("Active repository rules could not be verified for direct merge.".into())
            }
            Action::Merge
                if self
                    .merge_rules
                    .as_ref()
                    .is_some_and(|rules| rules.iter().any(|r| r["type"] == "merge_queue")) =>
            {
                Some(
                    "Repository rules require a merge queue; direct merge is not permitted.".into(),
                )
            }
            Action::Merge if self.method.is_none() => {
                Some("No provider-selected supported merge method is available.".into())
            }
            Action::Merge if self.checks.as_deref() != Some("SUCCESS") => {
                Some("Current-head CI has not been verified green.".into())
            }
            Action::Merge if self.mergeable != "MERGEABLE" || self.merge_state != "CLEAN" => {
                Some(format!(
                    "Provider merge readiness is {}/{}; no bypass is permitted.",
                    self.mergeable, self.merge_state
                ))
            }
            Action::Merge
                if self.review_decision.as_deref() == Some("REVIEW_REQUIRED")
                    || self.review_decision.as_deref() == Some("CHANGES_REQUESTED")
                    || (self.protection["requiresApprovingReviews"] == true
                        && self.review_decision.as_deref() != Some("APPROVED")) =>
            {
                Some("Provider-required reviews are not satisfied.".into())
            }
            Action::Merge => None,
        }
    }
}

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
    if value["totalCount"].as_u64() != Some(nodes.len() as u64)
        || value["pageInfo"]["hasNextPage"] != false
    {
        return Err(ConnectionError::IncompleteRead);
    }
    Ok(nodes)
}

impl<T: QueryTransport> GithubClient<T> {
    pub fn action_observation(
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
                  ... on CheckRun { name status conclusion }
                  ... on StatusContext { context state }
                }
              }}}}}
              reviewThreads(first:100) { totalCount pageInfo { hasNextPage } nodes {
                id isResolved isOutdated resolvedBy { login }
                comments(first:100) { totalCount pageInfo { hasNextPage } nodes { fullDatabaseId body updatedAt author { login } } }
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
            if protection["requiredApprovingReviewCount"]
                .as_u64()
                .is_none()
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
            Err(error @ (ConnectionError::RateLimited|ConnectionError::RateLimitedAfter(_)|ConnectionError::Network
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
            return Err(ConnectionError::IncompleteRead);
        }
        Ok(observation)
    }
}

impl<T: Transport> GithubClient<T> {
    pub fn action_reviews(
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub id: String,
    pub actor_id: String,
    pub head: String,
    pub action: Action,
    pub merge_commit: Option<String>,
}

impl<T: MutationTransport> GithubClient<T> {
    pub fn approve_exact(
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
    pub fn merge_exact(
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
