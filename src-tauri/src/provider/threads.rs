//! Provider-neutral review threads and comments. Each provider maps its
//! inline threads and top-level comments onto these shapes.

use super::ConnectionError;
use crate::publication::{Publication, RemoteState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Enough to authenticate the original roots without retaining their text or
/// pretending a compact receipt is a historical execution configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ownership {
    pub publication_id: String,
    pub configuration_id: String,
    pub account_id: String,
    pub repository_id: String,
    pub repository_name: String,
    pub pull_request_id: String,
    pub number: u64,
    pub head_sha: String,
    pub assignment_id: String,
    pub agent_id: String,
    pub review_id: String,
    pub root_ids: Vec<String>,
    pub body_hashes: BTreeSet<String>,
}

pub trait Provenance {
    fn ownership(&self) -> Result<Ownership, ConnectionError>;
}

impl Provenance for Ownership {
    fn ownership(&self) -> Result<Ownership, ConnectionError> {
        Ok(self.clone())
    }
}

impl Provenance for Publication {
    fn ownership(&self) -> Result<Ownership, ConnectionError> {
        let receipt = self
            .receipts
            .last()
            .filter(|r| r.state == RemoteState::Commented)
            .ok_or(ConnectionError::InvalidResponse)?;
        let batch = self
            .batch
            .as_ref()
            .ok_or(ConnectionError::InvalidResponse)?;
        let job = &self.review.job;
        Ok(Ownership {
            publication_id: self.id.clone(),
            configuration_id: job.configuration_id.clone(),
            account_id: job.account_id.clone(),
            repository_id: job.repository_id.clone(),
            repository_name: job.repository_name.clone(),
            pull_request_id: job.pull_request_id.clone(),
            number: job.number,
            head_sha: job.head_sha.clone(),
            assignment_id: self.review.assignment_id.clone(),
            agent_id: self.review.selection.agent.id.clone(),
            review_id: receipt.review_id.clone(),
            root_ids: receipt.comment_ids.clone(),
            body_hashes: batch.comments.iter().map(|c| body_hash(&c.body)).collect(),
        })
    }
}

pub(crate) fn body_hash(body: &str) -> String {
    format!("{:x}", Sha256::digest(body.as_bytes()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    pub id: String,
    pub body: String,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub reply_to: Option<String>,
    pub review_id: Option<String>,
    pub original_commit: Option<String>,
    pub created_at: String,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thread {
    pub id: String,
    pub resolved: bool,
    pub can_reply: bool,
    pub comments: Vec<Comment>,
}

impl Thread {
    pub fn root(&self) -> Result<&Comment, ConnectionError> {
        self.comments
            .first()
            .filter(|c| c.reply_to.is_none())
            .ok_or(ConnectionError::InvalidResponse)
    }

    /// Root for observation only: the first comment when it is a root, else the
    /// earliest comment that has no parent. `None` means the thread is malformed.
    pub fn tolerant_root(&self) -> Option<&Comment> {
        self.root().ok().or_else(|| {
            self.comments
                .iter()
                .enumerate()
                .filter(|(_, comment)| comment.reply_to.is_none())
                .min_by_key(|(index, comment)| (&comment.published_at, *index))
                .map(|(_, comment)| comment)
        })
    }

    pub fn latest_external(&self, account: &str) -> Option<&Comment> {
        self.comments
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, comment)| {
                !(comment.author_id.as_deref() == Some(account)
                    && comment.body.contains("<!-- pr-sniper:reply:"))
            })
            .max_by_key(|(index, comment)| (&comment.published_at, *index))
            .map(|(_, comment)| comment)
    }

    pub fn latest_other_user(&self, account: &str) -> Option<&Comment> {
        self.comments
            .iter()
            .filter(|comment| {
                comment.author_id.as_deref().is_some_and(|id| id != account)
                    && !comment.body.contains("<!-- pr-sniper:")
            })
            .max_by_key(|comment| (&comment.published_at, &comment.id))
    }

    /// Owned when any saved proof verifies the root. Agent resolution is limited to these.
    pub fn owned_by_any(&self, proofs: &[Ownership]) -> bool {
        proofs.iter().any(|proof| self.owned_by(proof))
    }

    pub fn owned_by(&self, publication: &(impl Provenance + ?Sized)) -> bool {
        let Ok(root) = self.root() else {
            return false;
        };
        let Ok(proof) = publication.ownership() else {
            return false;
        };
        proof.root_ids.contains(&root.id)
            && root.author_id.as_deref() == Some(&proof.account_id)
            && root.review_id.as_deref() == Some(&proof.review_id)
            && root.original_commit.as_deref() == Some(&proof.head_sha)
            && proof.body_hashes.contains(&body_hash(&root.body))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopComment {
    pub id: String,
    pub body: String,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl TopComment {
    pub fn eligible_other_user(&self, account: &str) -> bool {
        self.author_id.as_deref().is_some_and(|id| id != account)
            && !self.body.contains("<!-- pr-sniper:")
            && !self.body.trim().is_empty()
    }
    pub fn mentions(&self, login: &str, _account: &str) -> bool {
        if self.body.contains("<!-- pr-sniper:") {
            return false;
        }
        let text = self.body.to_ascii_lowercase();
        let target = format!("@{}", login.to_ascii_lowercase());
        let identity = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        !login.is_empty()
            && text.match_indices(&target).any(|(index, _)| {
                text[..index]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !identity(c) && c != '@')
                    && text[index + target.len()..]
                        .chars()
                        .next()
                        .is_none_or(identity_negated)
            })
    }
}

fn identity_negated(c: char) -> bool {
    !(c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
