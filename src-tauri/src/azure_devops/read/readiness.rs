//! Azure DevOps merge readiness and check status (FR-34): policy evaluations
//! and pull request statuses mapped onto the provider-neutral check
//! vocabulary that gates sign-off and the ready-for-review notification.
//! Build and status policies, and PR statuses, are checks; reviewer policies
//! and votes are reviews. Nothing here can complete a pull request.

use super::pulls::RawPullRequest;
use super::threads::AdoThread;
use super::{Client, List, Reader, RepositoryKey, Request};
use crate::azure_devops::tenant::TenantId;
use crate::provider::{
    ConnectionError, HumanThread, Lifecycle, Observation, PrSnapshot, ProviderReview, PullRequest,
    SnapshotCheck, SnapshotComment, SnapshotReview, SnapshotThread,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const POLICY_API_VERSION: &str = "7.1-preview.1";
const BUILD_POLICY: &str = "0609b952-1397-4640-95ec-e00a01b2c241";
const STATUS_POLICY: &str = "cbdc66da-9728-4af8-aada-9a5a32e4a226";
const MINIMUM_REVIEWERS_POLICY: &str = "fa4e907d-c16b-4a4c-9dfa-4906e5d171dd";
const REQUIRED_REVIEWERS_POLICY: &str = "fd2167ab-b0be-447a-8ec8-39368250530e";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CheckState {
    Success,
    Failure,
    Error,
    Pending,
}

impl CheckState {
    fn policy(status: &str) -> Option<Self> {
        Some(match status {
            "approved" => Self::Success,
            "rejected" => Self::Failure,
            "broken" => Self::Error,
            "queued" | "running" => Self::Pending,
            _ => return None,
        })
    }

    fn status(state: &str) -> Option<Self> {
        Some(match state {
            "succeeded" => Self::Success,
            "failed" => Self::Failure,
            "error" => Self::Error,
            "pending" | "notSet" => Self::Pending,
            _ => return None,
        })
    }

    fn context(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Failure => "FAILURE",
            Self::Error => "ERROR",
            Self::Pending => "PENDING",
        }
    }
}

/// One CI signal: a build or status policy evaluation, or a PR status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Check {
    pub name: String,
    pub state: CheckState,
    /// A blocking policy: completion needs it.
    pub required: bool,
}

/// Rollup in the shared vocabulary: any failure fails, any pending is pending.
pub(super) fn rollup(checks: &[Check]) -> Option<&'static str> {
    if checks.is_empty() {
        None
    } else if checks
        .iter()
        .any(|c| matches!(c.state, CheckState::Failure | CheckState::Error))
    {
        Some("FAILURE")
    } else if checks.iter().any(|c| c.state == CheckState::Pending) {
        Some("PENDING")
    } else {
        Some("SUCCESS")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReviewer {
    id: Option<String>,
    #[serde(default)]
    vote: i64,
    #[serde(default)]
    is_container: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLabel {
    name: Option<String>,
    #[serde(default = "active")]
    active: bool,
}

fn active() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDetail {
    description: Option<String>,
    source_ref_name: Option<String>,
    target_ref_name: Option<String>,
    merge_status: Option<String>,
    #[serde(default)]
    reviewers: Vec<RawReviewer>,
    #[serde(default)]
    labels: Vec<RawLabel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPolicyType {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPolicyConfiguration {
    #[serde(default = "active")]
    is_enabled: bool,
    #[serde(default)]
    is_blocking: bool,
    #[serde(rename = "type")]
    kind: RawPolicyType,
    #[serde(default)]
    settings: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEvaluation {
    evaluation_id: Option<String>,
    status: String,
    configuration: RawPolicyConfiguration,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStatusContext {
    name: Option<String>,
    genre: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStatus {
    id: u64,
    state: String,
    context: Option<RawStatusContext>,
    iteration_id: Option<u64>,
}

fn status_name(genre: Option<&str>, name: Option<&str>) -> Option<String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty())?;
    Some(match genre.map(str::trim).filter(|g| !g.is_empty()) {
        Some(genre) => format!("{genre}/{name}"),
        None => name.to_owned(),
    })
}

/// Everything readiness reads for one pull request.
pub(super) struct Readiness {
    pub pull: PullRequest,
    pub description: String,
    pub head_ref: String,
    pub base_ref: String,
    pub merge_status: Option<String>,
    pub labels: Vec<String>,
    /// Reviewer ID -> vote (10 approved, 5 with suggestions, -5 waiting, -10 rejected).
    pub votes: BTreeMap<String, i64>,
    pub checks: Vec<Check>,
    /// A blocking reviewer policy is not approved.
    pub reviews_required: bool,
    pub threads: Vec<AdoThread>,
    pub commits: BTreeMap<u64, String>,
}

impl Client {
    /// Policy evaluations of one pull request.
    async fn policy_evaluations(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<Vec<RawEvaluation>, ConnectionError> {
        self.owns(key)?;
        let artifact = format!(
            "vstfs:///CodeReview/CodeReviewId/{}/{number}",
            key.project_id.as_str()
        );
        let url = self.url(
            &[key.project_id.as_str(), "_apis", "policy", "evaluations"],
            &[
                ("artifactId", &artifact),
                ("api-version", POLICY_API_VERSION),
            ],
        );
        let (list, _): (List<RawEvaluation>, _) = self.get(url).await?;
        Ok(list.value)
    }

    async fn statuses(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<Vec<RawStatus>, ConnectionError> {
        self.owns(key)?;
        let url = self.git_url(key, &["pullRequests", &number.to_string(), "statuses"], &[]);
        let (list, _): (List<RawStatus>, _) = self.get(url).await?;
        Ok(list.value)
    }

    pub(super) async fn readiness(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<Readiness, ConnectionError> {
        self.owns(key)?;
        let url = self.git_url(key, &["pullrequests", &number.to_string()], &[]);
        let response = self.call(Request::get(url)).await?;
        let raw: Value =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        let pull = serde_json::from_value::<RawPullRequest>(raw.clone())
            .map_err(|_| ConnectionError::InvalidResponse)?
            .into_pull_request(key)?;
        if pull.number != number {
            return Err(ConnectionError::RepositoryChanged);
        }
        let detail: RawDetail =
            serde_json::from_value(raw).map_err(|_| ConnectionError::InvalidResponse)?;
        let iterations = self.iterations(key, number).await?;
        let latest = iterations.last().map(|i| i.id);
        let mut checks = Vec::new();
        let mut reviews_required = false;
        for evaluation in self.policy_evaluations(key, number).await? {
            let configuration = &evaluation.configuration;
            if !configuration.is_enabled {
                continue;
            }
            let kind = configuration.kind.id.to_ascii_lowercase();
            let Some(state) = CheckState::policy(&evaluation.status) else {
                continue;
            };
            if kind == MINIMUM_REVIEWERS_POLICY || kind == REQUIRED_REVIEWERS_POLICY {
                reviews_required |= configuration.is_blocking && state != CheckState::Success;
                continue;
            }
            let settings = &configuration.settings;
            let name = if kind == BUILD_POLICY {
                settings["displayName"]
                    .as_str()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(|n| format!("Build/{n}"))
                    .or_else(|| {
                        settings["buildDefinitionId"]
                            .as_u64()
                            .map(|id| format!("Build/{id}"))
                    })
            } else if kind == STATUS_POLICY {
                status_name(
                    settings["statusGenre"].as_str(),
                    settings["statusName"].as_str(),
                )
            } else {
                continue;
            };
            let name = name
                .or_else(|| evaluation.evaluation_id.clone())
                .ok_or(ConnectionError::InvalidResponse)?;
            checks.push(Check {
                name: super::bounded(Some(name)).ok_or(ConnectionError::InvalidResponse)?,
                state,
                required: configuration.is_blocking,
            });
        }
        // The newest status per context on the latest iteration; one a status
        // policy already evaluates is that policy's check.
        // Context -> newest status ID and its state (`None`: not a CI signal).
        let mut statuses: BTreeMap<String, (u64, Option<CheckState>)> = BTreeMap::new();
        for status in self.statuses(key, number).await? {
            if status.iteration_id.is_some_and(|i| Some(i) != latest) {
                continue;
            }
            let Some(name) = status
                .context
                .as_ref()
                .and_then(|c| status_name(c.genre.as_deref(), c.name.as_deref()))
                .and_then(|name| super::bounded(Some(name)))
            else {
                continue;
            };
            if statuses.get(&name).is_none_or(|(id, _)| status.id > *id) {
                statuses.insert(name, (status.id, CheckState::status(&status.state)));
            }
        }
        for (name, (_, state)) in statuses {
            let Some(state) = state else {
                continue;
            };
            if !checks.iter().any(|c| c.name == name) {
                checks.push(Check {
                    name,
                    state,
                    required: false,
                });
            }
        }
        let threads = self.threads(key, number).await?;
        let votes = detail
            .reviewers
            .iter()
            .filter(|r| !r.is_container)
            .filter_map(|r| {
                let id = TenantId::parse(r.id.as_deref()?)?;
                Some((id.as_str().to_owned(), r.vote))
            })
            .collect();
        let branch = |name: Option<String>| {
            name.map(|n| n.strip_prefix("refs/heads/").unwrap_or(&n).to_owned())
                .unwrap_or_default()
        };
        Ok(Readiness {
            pull,
            description: detail.description.unwrap_or_default(),
            head_ref: branch(detail.source_ref_name),
            base_ref: branch(detail.target_ref_name),
            merge_status: detail.merge_status,
            labels: detail
                .labels
                .into_iter()
                .filter(|l| l.active)
                .filter_map(|l| super::bounded(l.name))
                .collect(),
            votes,
            checks,
            reviews_required,
            threads,
            commits: iterations.into_iter().map(|i| (i.id, i.source)).collect(),
        })
    }
}

fn vote_state(vote: i64) -> Option<&'static str> {
    match vote {
        v if v > 0 => Some("APPROVED"),
        v if v < 0 => Some("CHANGES_REQUESTED"),
        _ => None,
    }
}

impl Readiness {
    fn lifecycle(&self) -> &'static str {
        match self.pull.state {
            Lifecycle::Open => "OPEN",
            Lifecycle::Merged => "MERGED",
            Lifecycle::Closed => "CLOSED",
        }
    }

    /// `succeeded` merges cleanly, `conflicts` conflicts; anything else is not
    /// computed yet.
    fn mergeable(&self) -> (&'static str, &'static str) {
        match self.merge_status.as_deref() {
            Some("succeeded") => ("MERGEABLE", "CLEAN"),
            Some("conflicts") => ("CONFLICTING", "DIRTY"),
            _ => ("UNKNOWN", "UNKNOWN"),
        }
    }

    fn review_decision(&self) -> Option<String> {
        if self.votes.values().any(|v| *v < 0) {
            Some("CHANGES_REQUESTED".into())
        } else if self.reviews_required {
            Some("REVIEW_REQUIRED".into())
        } else {
            None
        }
    }

    fn reviews(&self) -> impl Iterator<Item = (&String, &'static str)> {
        self.votes
            .iter()
            .filter_map(|(id, vote)| Some((id, vote_state(*vote)?)))
    }

    /// A thread in the shape the packet reads: global comment IDs, author
    /// identity, the root's place and commit, and edit times.
    fn human_thread(&self, thread: &AdoThread) -> HumanThread {
        let commit = thread.iteration.and_then(|i| self.commits.get(&i));
        let (path, line) = (thread.path.as_deref(), thread.line.map(|(_, line)| line));
        HumanThread {
            id: thread.reference.id(),
            resolved: thread.resolved,
            outdated: false,
            resolved_by: None,
            comments: thread
                .comments
                .iter()
                .map(|c| {
                    json!({
                        "fullDatabaseId": thread.reference.comment_id(c.id),
                        "author": {"login": c.author_login, "databaseId": c.author_id},
                        "body": c.content,
                        "path": path,
                        "line": line,
                        "originalCommit": {"oid": commit},
                        "createdAt": c.published,
                        "updatedAt": c.updated,
                    })
                })
                .collect(),
        }
    }

    /// Everything a sign-off and its Approve vote are gated on. Azure DevOps
    /// exposes no cheap per-repository vote check: a contributor may vote, and
    /// a refused vote reports its permission failure. Completion is never
    /// offered.
    pub fn observation(&self, account_id: &str) -> Observation {
        let (mergeable, merge_state) = self.mergeable();
        Observation {
            write_capability: false,
            node_id: self.pull.id.clone(),
            repository_id: self.pull.base_repository_id.clone(),
            pull_request_id: self.pull.id.clone(),
            account_id: account_id.to_owned(),
            author_id: self
                .pull
                .author
                .as_ref()
                .map(|a| a.id.clone())
                .unwrap_or_default(),
            head_repository_id: self.pull.head_repository_id.clone(),
            head: self.pull.head_sha.clone(),
            base: self.pull.base_sha.clone(),
            base_name: self.base_ref.clone(),
            merge_rules: None,
            merge_rules_error: None,
            state: self.lifecycle().into(),
            draft: self.pull.draft,
            permission: "WRITE".into(),
            mergeable: mergeable.into(),
            merge_state: merge_state.into(),
            review_decision: self.review_decision(),
            checks: rollup(&self.checks).map(str::to_owned),
            check_contexts: self
                .checks
                .iter()
                .map(|c| {
                    json!({
                        "__typename": "StatusContext",
                        "context": c.name,
                        "state": c.state.context(),
                        "isRequired": c.required,
                    })
                })
                .collect(),
            in_merge_queue: false,
            method: None,
            protection: json!({}),
            threads: self.threads.iter().map(|t| self.human_thread(t)).collect(),
            reviews: self
                .reviews()
                .map(|(id, state)| ProviderReview {
                    id: format!("{}:{id}", self.pull.id),
                    actor_id: id.clone(),
                    head: self.pull.head_sha.clone(),
                    state: state.into(),
                    body: String::new(),
                    submitted_at: None,
                })
                .collect(),
            comments: Vec::new(),
            merged_by: None,
            merged_at: None,
            merge_commit: None,
        }
    }

    /// The heartbeat's view of the pull request.
    pub fn snapshot(&self) -> PrSnapshot {
        let (mergeable, _) = self.mergeable();
        let threads: Vec<_> = self
            .threads
            .iter()
            .map(|t| {
                let thread = t.to_thread(&self.commits);
                SnapshotThread {
                    id: thread.id,
                    resolved: thread.resolved,
                    outdated: false,
                    comment_total: thread.comments.len() as u64,
                    comments: thread
                        .comments
                        .into_iter()
                        .map(|c| SnapshotComment {
                            id: c.id,
                            body: c.body,
                            commit: c.original_commit,
                        })
                        .collect(),
                }
            })
            .collect();
        let mut required: Vec<_> = self
            .checks
            .iter()
            .filter(|c| c.required)
            .map(|c| c.name.clone())
            .collect();
        required.sort();
        PrSnapshot {
            number: self.pull.number,
            pull_request_id: self.pull.id.clone(),
            title: self.pull.title.clone(),
            body: self.description.clone(),
            labels: self.labels.clone(),
            head_ref: self.head_ref.clone(),
            head: self.pull.head_sha.clone(),
            base: self.pull.base_sha.clone(),
            state: self.lifecycle().into(),
            draft: self.pull.draft,
            mergeable: mergeable.into(),
            review_decision: self.review_decision(),
            reviews: self
                .reviews()
                .map(|(id, state)| SnapshotReview {
                    actor: id.clone(),
                    state: state.into(),
                    head: Some(self.pull.head_sha.clone()),
                })
                .collect(),
            check_total: self.checks.len() as u64,
            checks: self
                .checks
                .iter()
                .map(|c| SnapshotCheck {
                    name: c.name.clone(),
                    status: if c.state == CheckState::Pending {
                        "IN_PROGRESS"
                    } else {
                        "COMPLETED"
                    }
                    .into(),
                    conclusion: match c.state {
                        CheckState::Success => "success",
                        CheckState::Failure => "failure",
                        CheckState::Error => "error",
                        CheckState::Pending => "",
                    }
                    .into(),
                })
                .collect(),
            required_checks: required,
            thread_total: threads.len() as u64,
            threads,
            comments: Vec::new(),
            comment_total: 0,
        }
    }
}

impl Reader {
    pub(super) fn readiness(
        &self,
        key: &RepositoryKey,
        number: u64,
    ) -> Result<Readiness, ConnectionError> {
        self.runtime.block_on(self.client.readiness(key, number))
    }
}
