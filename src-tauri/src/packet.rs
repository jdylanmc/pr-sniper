//! The review packet: everything the agent is told, built deterministically by the host.
//! Same input, same text, same hash. Content is fenced as untrusted, never silently cut.

pub mod duck;
pub mod reads;
pub mod rules;
pub mod sections;

use crate::exits::{gates::Offer, Exit, TeammateReport};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Bumped with any change to the fixed text or section layout.
pub const SCHEMA: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Teammate,
    /// A Rubber Duck call: one claim, `submit_verdict` only.
    Duck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorRelation {
    Owner,
    Bot,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckFact {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    /// Provider check id (`get_checks` `id`); a check run's id is its Actions job id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A GitHub Actions job: `read_check_log` can read its log.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub log: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentFact {
    pub id: String,
    pub author: String,
    /// Provider account id of the author; absent when the provider did not say.
    pub author_id: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadFact {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub resolved: bool,
    pub outdated: bool,
    /// Rooted by this agent's earlier feedback.
    pub mine: bool,
    pub new_since_last_review: bool,
    pub targeted_revision: Option<String>,
    pub comments: Vec<CommentFact>,
    /// PR Sniper resolved this own finding earlier and nothing it relied on changed.
    pub carried: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversation {
    pub threads: Vec<ThreadFact>,
    pub comments: Vec<CommentFact>,
}

/// Read-only data behind the pull tools (`get_thread`, `get_checks`). Static per run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolFacts {
    pub description: Option<String>,
    pub conversation: Conversation,
    pub checks: Vec<CheckFact>,
    pub corrections: Vec<crate::review::findings::Correction>,
    pub findings: FindingFacts,
}

/// Provider identities the finding contract checks author won't-fix replies against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindingFacts {
    /// The PR author's provider account id.
    pub pr_author_id: Option<String>,
    /// The acting provider account id.
    pub account_id: String,
    /// Comment ids PR Sniper wrote on this PR (publication roots and reply receipts).
    pub own_comments: std::collections::BTreeSet<String>,
    /// Standing resolution evidence of every carried-forward thread.
    pub carried: Vec<crate::review::findings::Assessment>,
    /// What a sign-off does after resolving: the assignment's authority.
    pub lgtm_action: crate::exits::LgtmAction,
    /// Own thread id -> head the finding was published on (ownership proof).
    pub thread_heads: std::collections::BTreeMap<String, String>,
    /// Host id (`T<k>#<n>`) of every teammate finding in this run's packet.
    pub teammate_findings: Vec<String>,
    /// This assignment already published findings on this exact head and the human
    /// did not ask to review again: no new findings until a new head.
    pub revision_reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrFacts {
    pub title: String,
    /// None when the caller could not read it; the packet says so.
    pub description: Option<String>,
    pub author: String,
    pub author_relation: AuthorRelation,
    pub state: String,
    pub draft: bool,
    pub base_ref: String,
    pub head_ref: String,
    pub base_sha: String,
    pub head_sha: String,
    pub labels: Vec<String>,
    pub size: Option<(u64, u64)>,
    pub checks: Vec<CheckFact>,
    pub mergeable: String,
    pub review_decision: Option<String>,
    pub approvals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub status: String,
    pub previous_path: Option<String>,
    /// `(part, bytes)`; bytes are None when the host cannot know them without a read.
    pub parts: Vec<(String, Option<usize>)>,
    /// text, binary, generated, lockfile or vendored. Information only.
    pub kind: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Directive {
    /// Ordered doctrines `(title, body)` from the Selection.
    pub doctrines: Vec<(String, String)>,
    pub agent_prompt: String,
    pub repository_prompt: String,
    pub preset: Option<String>,
    pub signature: String,
    /// The Agent's Voice for human-facing text. Primary packets only.
    pub voice: Option<crate::storage::Voice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateEntry {
    pub agent: String,
    /// Ordered doctrine titles: what that teammate looks for.
    pub lens: Vec<String>,
    pub report: TeammateReport,
}

#[derive(Debug, Clone)]
pub struct PacketInput {
    pub state_id: String,
    pub revision: u32,
    pub created_at: String,
    pub admitted_by: Option<String>,
    pub acting_login: String,
    pub include_self: bool,
    pub role: Role,
    pub tools: Vec<String>,
    pub exits: Vec<(Exit, Offer)>,
    pub pr: PrFacts,
    pub conversation: Conversation,
    pub files: Vec<FileEntry>,
    /// Paths changed since the last consumed state (revision above 1).
    pub delta: Option<Vec<String>>,
    pub directive: Directive,
    pub teammates: Vec<TeammateEntry>,
    /// This assignment already published on this head and the pass is not the human's
    /// explicit Review again: the packet says no new findings.
    pub revision_reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitDenial {
    pub exit: Exit,
    pub reason: String,
}

/// Content-free record stored with the run: ids, hash and sets, never packet text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketRecord {
    pub state_id: String,
    pub hash: String,
    pub schema: u32,
    pub tool_set: Vec<String>,
    pub exits_offered: Vec<Exit>,
    #[serde(default)]
    pub exits_denied: Vec<ExitDenial>,
    /// Hash of the PR state the packet was built from; what an honoured exit baselines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<String>,
    /// Compact packet-time state; tells own writes from external change at consume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_marks: Option<crate::github::state::StateMarks>,
    /// Fresh normalized state for an offered Wait; may be newer than the last poll.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_snapshot: Option<crate::github::state::NormalizedSnapshot>,
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub text: String,
    pub hash: String,
    pub record: PacketRecord,
}

pub fn build(input: &PacketInput) -> Packet {
    // The hash ignores wall-clock time so the same PR state always hashes the same.
    let stable = sections::render(input, "", false);
    let digest = Sha256::digest(stable.as_bytes());
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let packet_id = hash[..16].to_string();
    let text = sections::render(input, &packet_id, true);
    let record = PacketRecord {
        state_id: input.state_id.clone(),
        hash: hash.clone(),
        schema: SCHEMA,
        tool_set: input.tools.clone(),
        exits_offered: input
            .exits
            .iter()
            .filter(|(_, offer)| offer.is_offered())
            .map(|(exit, _)| *exit)
            .collect(),
        exits_denied: input
            .exits
            .iter()
            .filter_map(|(exit, offer)| match offer {
                Offer::Denied(reason) => Some(ExitDenial {
                    exit: *exit,
                    reason: reason.clone(),
                }),
                Offer::Offered => None,
            })
            .collect(),
        state_hash: None,
        state_marks: None,
        wait_snapshot: None,
    };
    Packet { text, hash, record }
}

/// Information-only file kind from the path. Never a policy input.
pub fn file_kind(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if matches!(
        name,
        "cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "poetry.lock"
            | "gemfile.lock"
            | "go.sum"
            | "composer.lock"
    ) {
        "lockfile"
    } else if lower.starts_with("vendor/")
        || lower.contains("/vendor/")
        || lower.contains("node_modules/")
    {
        "vendored"
    } else if name.contains(".min.")
        || name.contains(".generated.")
        || name.ends_with(".pb.go")
        || name.ends_with(".snap")
        || lower.starts_with("dist/")
        || lower.contains("/dist/")
    {
        "generated"
    } else if [
        ".png", ".jpg", ".jpeg", ".gif", ".ico", ".pdf", ".zip", ".gz", ".woff", ".woff2", ".ttf",
        ".icns", ".webp",
    ]
    .iter()
    .any(|ext| name.ends_with(ext))
    {
        "binary"
    } else {
        "text"
    }
}

#[cfg(test)]
mod tests;
