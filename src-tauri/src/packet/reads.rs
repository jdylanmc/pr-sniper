//! Fill packet facts from the reads the host already makes: `review_context` and the
//! provider observation. M3's PR snapshot can replace these without touching the packet.

use super::{
    file_kind, AuthorRelation, CheckFact, CommentFact, Conversation, FileEntry, PrFacts,
    ThreadFact, ToolFacts,
};
use crate::github::{actions::Observation, review::ReviewContext};
use serde_json::Value;

pub const OWN_MARKER: &str = "<!-- pr-sniper:";

fn str_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

pub fn conversation(observation: &Observation) -> Conversation {
    Conversation {
        threads: observation
            .threads
            .iter()
            .map(|thread| {
                let comments: Vec<_> = thread
                    .comments
                    .iter()
                    .map(|c| CommentFact {
                        id: match &c["fullDatabaseId"] {
                            Value::Number(n) => n.to_string(),
                            other => str_of(other),
                        },
                        author: str_of(&c["author"]["login"]),
                        body: str_of(&c["body"]),
                    })
                    .collect();
                ThreadFact {
                    id: thread.id.clone(),
                    path: None,
                    line: None,
                    resolved: thread.resolved,
                    outdated: thread.outdated,
                    mine: comments
                        .first()
                        .is_some_and(|c| c.body.contains(OWN_MARKER)),
                    new_since_last_review: false,
                    targeted_revision: None,
                    comments,
                }
            })
            .collect(),
        comments: observation
            .comments
            .iter()
            .map(|c| CommentFact {
                id: c.id.clone(),
                author: c.author_login.clone().unwrap_or_default(),
                body: c.body.clone(),
            })
            .collect(),
    }
}

pub fn checks(observation: &Observation) -> Vec<CheckFact> {
    observation
        .check_contexts
        .iter()
        .map(|c| match c["__typename"].as_str() {
            Some("StatusContext") => CheckFact {
                name: str_of(&c["context"]),
                status: "COMPLETED".into(),
                conclusion: str_of(&c["state"]),
            },
            _ => CheckFact {
                name: str_of(&c["name"]),
                status: str_of(&c["status"]),
                conclusion: str_of(&c["conclusion"]),
            },
        })
        .collect()
}

pub fn files(context: &ReviewContext) -> Vec<FileEntry> {
    context
        .files
        .iter()
        .map(|file| {
            let before_path = file.previous_path.as_deref().unwrap_or(&file.path);
            let mut parts = vec![];
            if let Some(patch) = &file.patch {
                parts.push(("patch".to_string(), Some(patch.len())));
            }
            if !(file.status == "added"
                || (file.status == "copied" && !context.base.contains_key(before_path)))
            {
                parts.push(("before".to_string(), None));
            }
            if file.status != "removed" {
                parts.push(("after".to_string(), None));
            }
            FileEntry {
                path: file.path.clone(),
                status: file.status.clone(),
                previous_path: file.previous_path.clone(),
                parts,
                kind: file_kind(&file.path),
            }
        })
        .collect()
}

/// `description` is not in the provider reads yet; callers pass it when they have it.
pub fn pr_facts(
    context: &ReviewContext,
    observation: &Observation,
    description: Option<String>,
    acting_login: &str,
) -> PrFacts {
    let pull = &context.pull;
    let author = pull
        .author
        .as_ref()
        .map(|a| a.login.clone())
        .unwrap_or_default();
    PrFacts {
        title: pull.title.clone(),
        description,
        author_relation: if author.eq_ignore_ascii_case(acting_login) {
            AuthorRelation::Owner
        } else if author.ends_with("[bot]") {
            AuthorRelation::Bot
        } else {
            AuthorRelation::Other
        },
        author,
        state: observation.state.clone(),
        draft: observation.draft,
        base_ref: observation.base_name.clone(),
        head_ref: "-".into(),
        base_sha: pull.base_sha.clone(),
        head_sha: pull.head_sha.clone(),
        labels: vec![],
        size: None,
        checks: checks(observation),
        mergeable: format!("{}/{}", observation.mergeable, observation.merge_state),
        review_decision: observation.review_decision.clone(),
        approvals: observation
            .reviews
            .iter()
            .filter(|r| r.state == "APPROVED")
            .map(|r| r.actor_id.clone())
            .collect(),
    }
}

pub fn tool_facts(
    description: Option<String>,
    conversation: &Conversation,
    checks: &[CheckFact],
) -> ToolFacts {
    ToolFacts {
        description,
        conversation: conversation.clone(),
        checks: checks.to_vec(),
    }
}
