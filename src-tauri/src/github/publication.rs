use super::{
    provider::{decimal_id, parse_response, GithubClient, Response, Transport},
    ConnectionError,
};
use crate::{
    publication::{
        diff_positions, InlineComment, Mutation, Publication, Receipt, RemoteState, WriteFailure,
    },
    review::Failure,
    storage::diagnostics::{Event, Outcome, PublicationStage, VerificationField},
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub enum Request {
    Post(Value),
    Delete,
}

// Deliberately separate from Transport: the Copilot read adapter cannot mutate.
pub trait MutationTransport: Transport {
    fn mutate(&self, path: &str, request: Request) -> Result<Response, ConnectionError>;
}

impl<T: Transport> GithubClient<T> {
    fn publication_event(
        &self,
        stage: PublicationStage,
        mismatch: Option<VerificationField>,
        ownership: Outcome,
        failure: Option<&Failure>,
    ) {
        self.publication_observation(stage, mismatch, ownership, failure, None, None);
    }

    fn publication_observation(
        &self,
        stage: PublicationStage,
        mismatch: Option<VerificationField>,
        ownership: Outcome,
        failure: Option<&Failure>,
        duration_ms: Option<u64>,
        uncertain: Option<bool>,
    ) {
        if let Some(sink) = &self.publication_diagnostics {
            sink(Event::Publication {
                stage,
                mismatch,
                ownership,
                failure: failure.map(crate::review::diagnostics::failure_kind),
                duration_ms,
                uncertain,
            });
        }
    }

    pub fn reconcile_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        let started = std::time::Instant::now();
        let found = self.find_review(run);
        self.publication_observation(
            PublicationStage::Ownership,
            None,
            match &found {
                Ok(Some(_)) => Outcome::Success,
                Ok(None) => Outcome::NotRequired,
                Err(_) => Outcome::Unavailable,
            },
            found.as_ref().err(),
            Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
            None,
        );
        let Some(receipt) = found? else {
            return Ok(None);
        };
        let stage = verification_stage(&receipt.state);
        let started = std::time::Instant::now();
        let result = self.verify_comments(run, receipt);
        self.publication_observation(
            stage,
            None,
            Outcome::Success,
            match &result {
                Err(Verify::Mismatch(_, f) | Verify::Failed(f)) => Some(f),
                _ => None,
            },
            Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
            None,
        );
        match result {
            Ok(receipt) => Ok(Some(receipt)),
            Err(Verify::Mismatch(_, failure) | Verify::Failed(failure)) => Err(failure),
        }
    }

    /// The owned review whose inline comments differ from the frozen batch, if
    /// that is the only reason it cannot be reconciled. Typed, never message-based.
    pub fn divergent_publication(&self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        let Some(receipt) = self.find_review(run)? else {
            return Ok(None);
        };
        match self.verify_comments(run, receipt) {
            Err(Verify::Mismatch(receipt, _)) => Ok(Some(receipt)),
            Ok(_) => Ok(None),
            Err(Verify::Failed(failure)) => Err(failure),
        }
    }

    fn find_review(&self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        let path = review_path(run)?;
        let reviews = self.pages(&format!("{path}?per_page=100&page=1"))?;
        let mut matched = None;
        let mut ids = BTreeSet::new();
        let mut foreign_pending = false;
        for value in reviews {
            let id = decimal_id(&value["id"])?;
            if !ids.insert(id.clone()) {
                self.publication_event(
                    PublicationStage::Ownership,
                    Some(VerificationField::ReviewId),
                    Outcome::Failed,
                    None,
                );
                return Err(Failure::permanent(
                    "GitHub returned duplicate review identities.",
                ));
            }
            if decimal_id(&value["user"]["id"]).ok().as_ref() != Some(&run.review.job.account_id) {
                if run.operation.pending_review_id.as_ref() == Some(&id)
                    || value["body"]
                        .as_str()
                        .is_some_and(|body| body.contains(&run.marker()))
                {
                    self.publication_event(
                        PublicationStage::Ownership,
                        Some(VerificationField::Author),
                        Outcome::Failed,
                        None,
                    );
                }
                continue;
            }
            let body = value["body"]
                .as_str()
                .ok_or(ConnectionError::InvalidResponse)?;
            if body.contains(&run.marker()) || run.operation.pending_review_id.as_ref() == Some(&id)
            {
                if matched.is_some() {
                    self.publication_event(
                        PublicationStage::Ownership,
                        Some(VerificationField::Marker),
                        Outcome::Failed,
                        None,
                    );
                    return Err(Failure::permanent("Multiple GitHub reviews match this publication; manual reconciliation required."));
                }
                matched = Some(validate_review(run, &value, &|field| {
                    self.publication_event(
                        PublicationStage::Ownership,
                        Some(field),
                        Outcome::Failed,
                        None,
                    );
                })?);
            } else if value["state"] == "PENDING" {
                foreign_pending = true;
            }
        }
        if matched.is_none() && foreign_pending {
            self.publication_event(
                PublicationStage::Ownership,
                Some(VerificationField::Marker),
                Outcome::Failed,
                None,
            );
            return Err(Failure::permanent("This GitHub account already has another pending review. Finish or discard it on GitHub; PR Sniper will not alter it."));
        }
        Ok(matched)
    }

    fn verify_comments(&self, run: &Publication, mut receipt: Receipt) -> Result<Receipt, Verify> {
        let path = review_path(run)?;
        let batch = run
            .batch
            .as_ref()
            .ok_or_else(|| Failure::permanent("Publication has no frozen batch."))?;
        let comments = self.pages(&format!(
            "{path}/{}/comments?per_page=100&page=1",
            receipt.review_id
        ))?;
        let mismatch = |receipt: &Receipt, field| {
            self.publication_event(
                verification_stage(&receipt.state),
                Some(field),
                Outcome::Success,
                None,
            );
            Verify::Mismatch(receipt.clone(), comment_mismatch(&receipt.state, field))
        };
        if comments.len() != batch.comments.len() {
            return Err(mismatch(&receipt, VerificationField::CommentCount));
        }
        let positions = if receipt.state == RemoteState::Pending
            && batch.comments.iter().any(|c| c.diff_position.is_none())
        {
            self.pending_positions(run)?
        } else {
            BTreeMap::new()
        };
        let mut unmatched = batch.comments.clone();
        let mut comment_ids = BTreeSet::new();
        for comment in comments {
            let id = decimal_id(&comment["id"])?;
            for (matches, field) in [
                (comment_ids.insert(id.clone()), VerificationField::ReviewId),
                (
                    decimal_id(&comment["user"]["id"])? == run.review.job.account_id,
                    VerificationField::Author,
                ),
                (
                    decimal_id(&comment["pull_request_review_id"])? == receipt.review_id,
                    VerificationField::ReviewId,
                ),
                (
                    comment["original_commit_id"].as_str() == Some(&batch.commit_id),
                    VerificationField::Commit,
                ),
            ] {
                if !matches {
                    return Err(mismatch(&receipt, field));
                }
            }
            // Finding markers make bodies unique even at the same diff location.
            let index = unmatched
                .iter()
                .position(|expected| comment["body"].as_str() == Some(&expected.body))
                .ok_or_else(|| mismatch(&receipt, VerificationField::Body))?;
            let expected = &unmatched[index];
            if comment["path"].as_str() != Some(&expected.path) {
                return Err(mismatch(&receipt, VerificationField::Path));
            }
            verify_location(&comment, expected, &receipt.state, &positions)
                .map_err(|field| mismatch(&receipt, field))?;
            unmatched.remove(index);
        }
        receipt.comment_ids = comment_ids.into_iter().collect();
        Ok(receipt)
    }

    fn pending_positions(
        &self,
        run: &Publication,
    ) -> Result<BTreeMap<(String, String, u64), u64>, Failure> {
        let mismatch = |field| {
            self.publication_event(
                PublicationStage::VerifyPending,
                Some(field),
                Outcome::Success,
                None,
            );
            comment_mismatch(&RemoteState::Pending, field)
        };
        let batch = run
            .batch
            .as_ref()
            .ok_or_else(|| mismatch(VerificationField::Batch))?;
        let base = run
            .review
            .result
            .as_ref()
            .and_then(|r| r.reviewed_base_sha.as_deref())
            .ok_or_else(|| mismatch(VerificationField::Commit))?;
        if [base, batch.commit_id.as_str()]
            .iter()
            .any(|sha| sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(mismatch(VerificationField::Commit));
        }
        // Compare the frozen revisions, not the mutable PR head. This also supports
        // already-persisted batches without a reset. Missing/truncated comparison
        // patches fail closed; newly frozen batches retain their own positions.
        let name = crate::storage::canonical_repository(&run.review.job.repository_name)
            .map_err(Failure::permanent)?;
        let (comparison, _) = self.read(&format!(
            "/repos/{name}/compare/{base}...{}?per_page=1&page=1",
            batch.commit_id
        ))?;
        let files = comparison["files"]
            .as_array()
            .ok_or_else(|| mismatch(VerificationField::Diff))?;
        let mut positions = BTreeMap::new();
        let mut paths = BTreeSet::new();
        for file in files {
            let path = file["filename"]
                .as_str()
                .ok_or_else(|| mismatch(VerificationField::Path))?;
            if !paths.insert(path) {
                return Err(mismatch(VerificationField::Path));
            }
            if !batch.comments.iter().any(|c| c.path == path) {
                continue;
            }
            let lines = file["patch"]
                .as_str()
                .and_then(diff_positions)
                .ok_or_else(|| mismatch(VerificationField::Diff))?;
            for ((side, line), position) in lines {
                positions.insert(
                    (
                        path.to_string(),
                        if side == "base" { "LEFT" } else { "RIGHT" }.into(),
                        line,
                    ),
                    position,
                );
            }
        }
        Ok(positions)
    }
}

// Inline comments that provably differ, apart from reads that merely failed.
enum Verify {
    Mismatch(Receipt, Failure),
    Failed(Failure),
}

impl<E: Into<Failure>> From<E> for Verify {
    fn from(error: E) -> Self {
        Self::Failed(error.into())
    }
}

fn verification_stage(state: &RemoteState) -> PublicationStage {
    if *state == RemoteState::Pending {
        PublicationStage::VerifyPending
    } else {
        PublicationStage::VerifySubmitted
    }
}

fn comment_mismatch(state: &RemoteState, field: VerificationField) -> Failure {
    let stage = if *state == RemoteState::Pending {
        "verify_pending"
    } else {
        "verify_submitted"
    };
    let field = match field {
        VerificationField::Position => "diff_position".into(),
        _ => format!("{field:?}").to_ascii_lowercase(),
    };
    Failure::permanent(format!(
        "Publication {stage}: remote comment {field} does not match the frozen batch. Open this PR on GitHub and inspect the review before retrying; no automatic submission or replacement is allowed."
    ))
}

fn verify_location(
    comment: &Value,
    expected: &InlineComment,
    state: &RemoteState,
    positions: &BTreeMap<(String, String, u64), u64>,
) -> Result<(), VerificationField> {
    if *state == RemoteState::Pending {
        let position = expected
            .diff_position
            .or_else(|| {
                positions
                    .get(&(expected.path.clone(), expected.side.clone(), expected.line))
                    .copied()
            })
            .filter(|position| *position > 0)
            .ok_or(VerificationField::Position)?;
        if comment["position"].as_u64() != Some(position) {
            return Err(VerificationField::Position);
        }
        for field in ["original_line", "line"] {
            if !comment[field].is_null() && comment[field].as_u64() != Some(expected.line) {
                return Err(VerificationField::Line);
            }
        }
        if !comment["side"].is_null() && comment["side"].as_str() != Some(&expected.side) {
            return Err(VerificationField::Side);
        }
    } else {
        let line = comment["original_line"]
            .as_u64()
            .or_else(|| comment["line"].as_u64());
        if line != Some(expected.line) {
            return Err(VerificationField::Line);
        }
        if comment["side"].as_str() != Some(&expected.side) {
            return Err(VerificationField::Side);
        }
    }
    if !comment["start_line"].is_null() || !comment["original_start_line"].is_null() {
        return Err(VerificationField::Line);
    }
    Ok(())
}

impl<T: MutationTransport> GithubClient<T> {
    pub fn mutate_publication(
        &self,
        run: &Publication,
        mutation: Mutation,
    ) -> Result<Receipt, WriteFailure> {
        let started = std::time::Instant::now();
        let result = self.mutate_publication_inner(run, mutation);
        self.publication_observation(
            match mutation {
                Mutation::Create => PublicationStage::Create,
                Mutation::Submit => PublicationStage::Submit,
                Mutation::Discard => PublicationStage::Discard,
            },
            None,
            if result.is_ok() {
                Outcome::Success
            } else {
                Outcome::Unavailable
            },
            result.as_ref().err().map(|error| &error.failure),
            Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
            Some(result.as_ref().err().is_some_and(|error| error.uncertain)),
        );
        result
    }

    fn mutate_publication_inner(
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
                let comments: Vec<_> = batch
                    .comments
                    .iter()
                    .map(|comment| {
                        json!({
                            "path": comment.path, "body": comment.body,
                            "line": comment.line, "side": comment.side
                        })
                    })
                    .collect();
                return Ok((
                    path,
                    Request::Post(json!({
                        "commit_id":batch.commit_id, "body":batch.body, "comments":comments
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
            .inspect_err(|error| self.publication_provider_error(*error))
            .map_err(|error| WriteFailure {
                failure: error.into(),
                uncertain: true,
            })?;
        let status = response.status;
        let uncertain = !(400..500).contains(&status) || status == 408;
        let (value, _) = parse_response(response).inspect_err(|error| self.publication_provider_error(*error)).map_err(|error| WriteFailure {
            // A rejected create payload cannot succeed on retry: a typed content toss.
            failure: match (status, mutation) {
                (422, Mutation::Create) => Failure::tossed("GitHub rejected the review payload; the review was tossed."),
                (422, _) => Failure::permanent("GitHub rejected this review batch. Correct the publication issue before retrying."),
                _ => error.into(),
            },
            uncertain,
        })?;
        let mut receipt = validate_review(run, &value, &|field| {
            self.publication_event(
                PublicationStage::Ownership,
                Some(field),
                Outcome::Failed,
                None,
            );
        })
        .map_err(|failure| WriteFailure {
            failure,
            uncertain: true,
        })?;
        let expected = if mutation == Mutation::Submit {
            RemoteState::Commented
        } else {
            RemoteState::Pending
        };
        if receipt.state != expected {
            self.publication_event(
                PublicationStage::Ownership,
                Some(VerificationField::State),
                Outcome::Failed,
                None,
            );
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

fn validate_review(
    run: &Publication,
    value: &Value,
    mismatch: &dyn Fn(VerificationField),
) -> Result<Receipt, Failure> {
    let batch = run
        .batch
        .as_ref()
        .ok_or_else(|| Failure::permanent("Publication has no frozen batch."))?;
    let id = decimal_id(&value["id"])?;
    for (matches, field) in [
        (
            decimal_id(&value["user"]["id"])? == run.review.job.account_id,
            VerificationField::Author,
        ),
        (
            value["body"].as_str() == Some(&batch.body),
            VerificationField::Body,
        ),
        (
            value["commit_id"].as_str() == Some(&batch.commit_id),
            VerificationField::Commit,
        ),
        (
            value["pull_request_url"].as_str()
                == Some(&format!(
                    "https://api.github.com/repos/{}/pulls/{}",
                    run.review.job.repository_name, run.review.job.number
                )),
            VerificationField::Repository,
        ),
        (
            !run.operation
                .pending_review_id
                .as_ref()
                .is_some_and(|expected| expected != &id),
            VerificationField::ReviewId,
        ),
    ] {
        if !matches {
            mismatch(field);
            return Err(Failure::permanent(
                "GitHub review does not match the acting account, revision and frozen output.",
            ));
        }
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
        _ => {
            mismatch(VerificationField::State);
            return Err(Failure::permanent(
            "GitHub review has an unexpected state. No approval is accepted as a COMMENT receipt.",
        ));
        }
    };
    Ok(Receipt {
        review_id: id,
        state,
        comment_ids: vec![],
    })
}
