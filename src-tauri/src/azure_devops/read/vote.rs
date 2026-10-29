//! Azure DevOps Approve (FR-37): the bound identity's reviewer vote of 10.
//! A vote has no text and one per reviewer per pull request, so the
//! reviewer's current vote is the receipt. The vote is read before it is set
//! and never replaced. Completion and auto-complete are never performed.

use super::{Client, List, RepositoryKey, Request};
use crate::azure_devops::tenant::TenantId;
use crate::provider::ConnectionError;
use serde::Deserialize;
use serde_json::json;

/// Approved.
pub(super) const APPROVE: i64 = 10;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVote {
    id: Option<String>,
    #[serde(default)]
    vote: i64,
}

impl RawVote {
    fn is(&self, reviewer: &TenantId) -> bool {
        self.id.as_deref().and_then(TenantId::parse).as_ref() == Some(reviewer)
    }
}

impl Client {
    fn reviewers_url(&self, key: &RepositoryKey, number: u64, reviewer: Option<&str>) -> String {
        let number = number.to_string();
        let mut path = vec!["pullRequests", &number, "reviewers"];
        path.extend(reviewer);
        self.git_url(key, &path, &[])
    }

    /// `reviewer`'s vote on the pull request; `None` while not a reviewer.
    pub(super) async fn reviewer_vote(
        &self,
        key: &RepositoryKey,
        number: u64,
        reviewer: &TenantId,
    ) -> Result<Option<i64>, ConnectionError> {
        self.owns(key)?;
        let (list, _): (List<RawVote>, _) = self.get(self.reviewers_url(key, number, None)).await?;
        Ok(list.value.iter().find(|r| r.is(reviewer)).map(|r| r.vote))
    }

    /// Sets `reviewer`'s vote, adding them as a reviewer if needed. Returns
    /// the vote the response reports.
    pub(super) async fn set_vote(
        &self,
        key: &RepositoryKey,
        number: u64,
        reviewer: &TenantId,
        vote: i64,
    ) -> Result<i64, ConnectionError> {
        self.owns(key)?;
        let url = self.reviewers_url(key, number, Some(reviewer.as_str()));
        let response = self
            .call(Request::put(url, json!({ "vote": vote })))
            .await?;
        let raw: RawVote =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        if !raw.is(reviewer) {
            return Err(ConnectionError::InvalidResponse);
        }
        Ok(raw.vote)
    }
}
