use super::{
    provider::{decimal_id, parse_response, GithubClient, Response, Transport},
    ConnectionError,
};
use crate::{
    publication::{Mutation, Publication, Receipt, RemoteState, WriteFailure},
    review::Failure,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub enum Request {
    Post(Value),
    Delete,
}

// Deliberately separate from Transport: the Copilot read adapter cannot mutate.
pub trait MutationTransport: Transport {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError>;
}

impl<T: Transport> GithubClient<T> {
    pub fn reconcile_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        let path = review_path(run)?;
        let batch = run
            .batch
            .as_ref()
            .ok_or_else(|| Failure::permanent("Publication has no frozen batch."))?;
        let reviews = self.pages(&format!("{path}?per_page=100&page=1"))?;
        let mut matched = None;
        let mut ids = BTreeSet::new();
        let mut foreign_pending = false;
        for value in reviews {
            let id = decimal_id(&value["id"])?;
            if !ids.insert(id.clone()) {
                return Err(Failure::permanent(
                    "GitHub returned duplicate review identities.",
                ));
            }
            if decimal_id(&value["user"]["id"]).ok().as_ref() != Some(&run.review.job.account_id) {
                continue;
            }
            let body = value["body"]
                .as_str()
                .ok_or(ConnectionError::InvalidResponse)?;
            if body.contains(&run.marker()) || run.operation.pending_review_id.as_ref() == Some(&id)
            {
                if matched.is_some() {
                    return Err(Failure::permanent("Multiple GitHub reviews match this publication; manual reconciliation required."));
                }
                matched = Some(validate_review(run, &value)?);
            } else if value["state"] == "PENDING" {
                foreign_pending = true;
            }
        }
        let Some(mut receipt) = matched else {
            if foreign_pending {
                return Err(Failure::permanent("This GitHub account already has another pending review. Finish or discard it on GitHub; PR Sniper will not alter it."));
            }
            return Ok(None);
        };
        let comments = self.pages(&format!(
            "{path}/{}/comments?per_page=100&page=1",
            receipt.review_id
        ))?;
        if comments.len() != batch.comments.len() {
            return Err(Failure::permanent("The remote review contains different comments. No automatic submission or replacement is allowed."));
        }
        let mut unmatched = batch.comments.clone();
        let mut comment_ids = BTreeSet::new();
        for comment in comments {
            let id = decimal_id(&comment["id"])?;
            if !comment_ids.insert(id.clone())
                || decimal_id(&comment["user"]["id"])? != run.review.job.account_id
                || decimal_id(&comment["pull_request_review_id"])? != receipt.review_id
                || comment["original_commit_id"].as_str() != Some(&batch.commit_id)
            {
                return Err(Failure::permanent(
                    "The remote comment identity does not match the frozen batch.",
                ));
            }
            let line = comment["original_line"]
                .as_u64()
                .or_else(|| comment["line"].as_u64());
            let index = unmatched
                .iter()
                .position(|expected| {
                    comment["path"].as_str() == Some(&expected.path)
                        && comment["body"].as_str() == Some(&expected.body)
                        && comment["side"].as_str() == Some(&expected.side)
                        && line == Some(expected.line)
                })
                .ok_or_else(|| {
                    Failure::permanent("The remote inline comment differs from the frozen batch.")
                })?;
            unmatched.remove(index);
        }
        receipt.comment_ids = comment_ids.into_iter().collect();
        Ok(Some(receipt))
    }
}

impl<T: MutationTransport> GithubClient<T> {
    pub fn mutate_publication(
        &self,
        run: &Publication,
        mutation: Mutation,
    ) -> Result<Receipt, WriteFailure> {
        let request = (|| {
            let path = review_path(run)?;
            let batch = run
                .batch
                .as_ref()
                .ok_or_else(|| Failure::permanent("Publication has no frozen batch."))?;
            if mutation == Mutation::Create {
                return Ok((
                    path,
                    Request::Post(json!({
                        "commit_id":batch.commit_id, "body":batch.body, "comments":batch.comments
                    })),
                ));
            }
            let id = run
                .operation
                .pending_review_id
                .as_ref()
                .filter(|id| id.parse::<u64>().is_ok_and(|id| id > 0))
                .ok_or_else(|| {
                    Failure::permanent("A confirmed pending-review identity is required.")
                })?;
            Ok(match mutation {
                Mutation::Submit => (
                    format!("{path}/{id}/events"),
                    Request::Post(json!({
                        "event":"COMMENT", "body":batch.body
                    })),
                ),
                Mutation::Discard => (format!("{path}/{id}"), Request::Delete),
                Mutation::Create => unreachable!(),
            })
        })()
        .map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        let response = self
            .transport
            .mutate(&request.0, request.1)
            .map_err(|error| WriteFailure {
                failure: error.into(),
                uncertain: true,
            })?;
        let status = response.status;
        let uncertain = !(400..500).contains(&status) || status == 408;
        let (value, _) = parse_response(response).map_err(|error| WriteFailure {
            failure: if status == 422 {
                Failure::permanent("GitHub rejected this review batch. Correct the publication issue before retrying.")
            } else { error.into() },
            uncertain,
        })?;
        let mut receipt = validate_review(run, &value).map_err(|failure| WriteFailure {
            failure,
            uncertain: true,
        })?;
        let expected = if mutation == Mutation::Submit {
            RemoteState::Commented
        } else {
            RemoteState::Pending
        };
        if receipt.state != expected {
            return Err(WriteFailure {
                failure: Failure::permanent("GitHub did not confirm the requested review state."),
                uncertain: true,
            });
        }
        if mutation == Mutation::Discard {
            receipt.state = RemoteState::Deleted;
        }
        if let Some(previous) = run.receipts.last() {
            receipt.comment_ids = previous.comment_ids.clone();
        }
        Ok(receipt)
    }
}

fn review_path(run: &Publication) -> Result<String, Failure> {
    let name = crate::storage::canonical_repository(&run.review.job.repository_name)
        .map_err(Failure::permanent)?;
    if run.review.job.number == 0 {
        return Err(Failure::permanent("Invalid pull request number."));
    }
    Ok(format!(
        "/repos/{name}/pulls/{}/reviews",
        run.review.job.number
    ))
}

fn validate_review(run: &Publication, value: &Value) -> Result<Receipt, Failure> {
    let batch = run
        .batch
        .as_ref()
        .ok_or_else(|| Failure::permanent("Publication has no frozen batch."))?;
    let id = decimal_id(&value["id"])?;
    if decimal_id(&value["user"]["id"])? != run.review.job.account_id
        || value["body"].as_str() != Some(&batch.body)
        || value["commit_id"].as_str() != Some(&batch.commit_id)
        || value["pull_request_url"].as_str()
            != Some(&format!(
                "https://api.github.com/repos/{}/pulls/{}",
                run.review.job.repository_name, run.review.job.number
            ))
        || run
            .operation
            .pending_review_id
            .as_ref()
            .is_some_and(|expected| expected != &id)
    {
        return Err(Failure::permanent(
            "GitHub review does not match the acting account, revision and frozen output.",
        ));
    }
    let state = match value["state"].as_str() {
        Some("PENDING") if value.get("submitted_at").is_none_or(Value::is_null) => {
            RemoteState::Pending
        }
        Some("COMMENTED")
            if value["submitted_at"]
                .as_str()
                .is_some_and(|s| !s.is_empty()) =>
        {
            RemoteState::Commented
        }
        _ => return Err(Failure::permanent(
            "GitHub review has an unexpected state. No approval is accepted as a COMMENT receipt.",
        )),
    };
    Ok(Receipt {
        review_id: id,
        state,
        comment_ids: vec![],
    })
}
