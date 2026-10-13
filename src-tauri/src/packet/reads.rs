//! Fill packet facts from the reads the host already makes: `review_context` and the
//! provider observation. M3's PR snapshot can replace these without touching the packet.

use super::{
    file_kind, AuthorRelation, CheckFact, CommentFact, Conversation, FileEntry, PrFacts,
    ThreadFact, ToolFacts,
};
use crate::github::{actions::Observation, review::ReviewContext, state::PrSnapshot};
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
                let root = thread.comments.first();
                ThreadFact {
                    id: thread.id.clone(),
                    path: root.and_then(|c| c["path"].as_str()).map(str::to_string),
                    line: root.and_then(|c| c["line"].as_u64().or(c["originalLine"].as_u64())),
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

/// A thread is new since the last review when any comment was written or edited after
/// the PR's last consumed state. No consumed state yet: everything is new.
pub fn stamp_new_since(
    conversation: &mut Conversation,
    observation: &Observation,
    last_consumed_at: Option<i64>,
) {
    for thread in &mut conversation.threads {
        thread.new_since_last_review = match last_consumed_at {
            None => true,
            Some(since) => observation
                .threads
                .iter()
                .find(|t| t.id == thread.id)
                .is_some_and(|t| {
                    t.comments.iter().any(|c| {
                        c["updatedAt"]
                            .as_str()
                            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                            .is_none_or(|at| at.timestamp() > since)
                    })
                }),
        };
    }
}

/// Per-comment targeted revision (AC-041): each thread root's provider commit mapped to
/// the local iteration with that head, else `unknown`. `known` is (head, iteration).
pub fn stamp_targeted_revisions(
    conversation: &mut Conversation,
    observation: &Observation,
    known: &[(String, u64)],
) {
    for thread in &mut conversation.threads {
        let commit = observation
            .threads
            .iter()
            .find(|t| t.id == thread.id)
            .and_then(|t| t.comments.first())
            .and_then(|c| c["originalCommit"]["oid"].as_str());
        thread.targeted_revision = Some(crate::exit_writes::targeted_revision(commit, known));
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
                id: None,
                log: false,
            },
            _ => {
                let id = c["databaseId"].as_u64().map(|id| id.to_string());
                CheckFact {
                    name: str_of(&c["name"]),
                    status: str_of(&c["status"]),
                    conclusion: str_of(&c["conclusion"]),
                    log: id.is_some()
                        && c["checkSuite"]["app"]["slug"].as_str() == Some("github-actions"),
                    id,
                }
            }
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

/// Additions and deletions counted from the changed files' patches.
fn patch_size(context: &ReviewContext) -> Option<(u64, u64)> {
    let mut totals = (0_u64, 0_u64);
    let mut any = false;
    for patch in context.files.iter().filter_map(|f| f.patch.as_deref()) {
        any = true;
        for line in patch.lines() {
            match line.as_bytes().first() {
                Some(b'+') => totals.0 += 1,
                Some(b'-') => totals.1 += 1,
                _ => {}
            }
        }
    }
    any.then_some(totals)
}

/// Description, labels and branch come from the fresh PR snapshot.
pub fn pr_facts(
    context: &ReviewContext,
    observation: &Observation,
    snapshot: &PrSnapshot,
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
        description: Some(snapshot.body.clone()),
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
        head_ref: if snapshot.head_ref.is_empty() {
            "-".into()
        } else {
            snapshot.head_ref.clone()
        },
        base_sha: pull.base_sha.clone(),
        head_sha: pull.head_sha.clone(),
        labels: snapshot.labels.clone(),
        size: patch_size(context),
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
