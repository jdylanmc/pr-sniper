//! Existing provider threads are finding identities. The model establishes truth;
//! the host enforces identity, revision, evidence and independent-verification contracts.

use super::duck::{DuckCall, DuckOutcome, EvidenceRef, TargetKind, Unserved, VerdictKind};
use crate::{
    exits::{Exit, ExitCall},
    github::threads::{Ownership, Provenance},
    monitoring::QueueJob,
    packet::ThreadFact,
    storage::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const CORRECTION: &str = "Correction: my earlier resolution was mistaken.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    thread_id: String,
    root_id: Option<String>,
    body: String,
    text: String,
    receipt: Option<String>,
    pending: bool,
}

impl Correction {
    fn confirmed_in(&self, thread: &ThreadFact) -> bool {
        !self.pending
            && thread.comments.first().map(|c| &c.id) == self.root_id.as_ref()
            && self.receipt.as_ref().is_some_and(|receipt| {
                thread
                    .comments
                    .iter()
                    .any(|comment| comment.id == *receipt && comment.body == self.body)
            })
    }
}

/// A compact view of existing write intents, not another persisted ledger.
pub(crate) fn corrections(
    writes: &[crate::exit_writes::ExitWrite],
    job: &QueueJob,
    assignment: &str,
) -> Vec<Correction> {
    use crate::exit_writes::{StepKind, StepState, LEGACY_SIGNATURE, SIGNATURE};
    let mut ordered: Vec<_> = writes
        .iter()
        .enumerate()
        .filter(|(_, write)| {
            crate::feedback::same_pr(&write.job, job) && write.assignment_id == assignment
        })
        .collect();
    ordered.sort_by_key(|(index, write)| (write.operation.initial_attempt_at, *index));
    let mut latest: BTreeMap<String, Vec<Correction>> = BTreeMap::new();
    for (_, write) in ordered {
        for (index, step) in write.steps.iter().enumerate() {
            if step.state == StepState::Planned {
                continue;
            }
            if step.kind == StepKind::Resolve {
                if step.state == StepState::Done && step.receipt.as_deref() == Some("already") {
                    continue;
                }
                // A later resolution is a new episode, even at the same head/base.
                latest.remove(&step.thread_id);
                if step.state == StepState::Done {
                    continue;
                }
                latest.insert(
                    step.thread_id.clone(),
                    vec![Correction {
                        thread_id: step.thread_id.clone(),
                        root_id: step.root_id.clone(),
                        body: String::new(),
                        text: String::new(),
                        receipt: None,
                        pending: true,
                    }],
                );
                continue;
            }
            if !step.body.starts_with(CORRECTION) {
                continue;
            }
            let Some(revision) = &step.revision else {
                continue;
            };
            // Writes saved before the signature changed keep their original suffix.
            let Some(text) = [SIGNATURE, LEGACY_SIGNATURE].iter().find_map(|signature| {
                step.body.strip_suffix(&format!(
                    "\n\n{}\n<!-- pr-sniper:revision:{revision} -->\n\n{signature}",
                    write.reply_marker(index)
                ))
            }) else {
                continue;
            };
            latest
                .entry(step.thread_id.clone())
                .or_default()
                .push(Correction {
                    thread_id: step.thread_id.clone(),
                    root_id: step.root_id.clone(),
                    body: step.body.clone(),
                    text: text.to_string(),
                    receipt: step.receipt.clone(),
                    pending: step.state != StepState::Done,
                });
        }
    }
    latest.into_values().flatten().collect()
}

fn correction_text(body: &str) -> String {
    if body.starts_with(CORRECTION) {
        body.trim_end().to_string()
    } else {
        format!("{CORRECTION}\n\n{}", body.trim_end())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    StillApplies,
    Fixed,
    Withdrawn,
    Unverifiable,
    /// The PR author replied won't-fix in the thread and the primary accepts that
    /// rationale on its merits.
    AuthorWontFix,
    /// Host-stamped by the primary's `lgtm` sign-off on an own resolved thread it
    /// did not classify; never model input.
    SignedOff,
    /// Re-assessed on a head changed since the thread was posted: the finding no
    /// longer applies to the current code (current-head evidence required).
    NoLongerApplies,
}

impl Disposition {
    pub fn id(self) -> &'static str {
        match self {
            Self::StillApplies => "still_applies",
            Self::Fixed => "fixed",
            Self::Withdrawn => "withdrawn",
            Self::Unverifiable => "unverifiable",
            Self::AuthorWontFix => "author_wont_fix",
            Self::SignedOff => "signed_off",
            Self::NoLongerApplies => "no_longer_applies",
        }
    }

    /// Accounts for an own open thread a sign-off resolves.
    pub fn signs_off(self) -> bool {
        matches!(
            self,
            Self::Fixed | Self::AuthorWontFix | Self::Withdrawn | Self::NoLongerApplies
        )
    }
}

/// Host-stamped at exit, never model input: what a resolution relied on, so a later
/// state can tell whether anything changed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionAnchor {
    /// The reviewed base; a base move re-opens the finding.
    pub base_sha: String,
    /// Head blob sha ("" when absent) of every cited file, the thread's file and every
    /// file the primary or its Rubber Duck read in that attempt.
    #[serde(default)]
    pub blobs: BTreeMap<String, String>,
    /// Digest of every non-ordinary file (manifests, lockfiles, build/CI/deploy and any
    /// other config, generated, vendored, unclassified) at that head.
    pub manifests: String,
    /// Thread comment id -> sha256 of its body at exit time: an edit is a change.
    #[serde(default)]
    pub comments: BTreeMap<String, String>,
    /// Provider account id of the PR author whose won't-fix reply was accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
}

/// sha256 hex of a comment body.
pub(crate) fn body_hash(body: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(body.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Files whose change can alter behaviour without touching cited code: manifests,
/// lockfiles, build, CI and deployment config, generated and vendored code. Named
/// here when their extension would otherwise look like ordinary source or text.
pub(crate) fn dependency_file(path: &str) -> bool {
    if matches!(
        crate::packet::file_kind(path),
        "lockfile" | "generated" | "vendored"
    ) {
        return true;
    }
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    matches!(
        name,
        "cargo.toml"
            | "package.json"
            | "go.mod"
            | "go.work"
            | "pyproject.toml"
            | "setup.py"
            | "setup.cfg"
            | "conftest.py"
            | "noxfile.py"
            | "tox.ini"
            | "pipfile"
            | "gemfile"
            | "rakefile"
            | "podfile"
            | "package.swift"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "gradle.properties"
            | "directory.build.props"
            | "directory.build.targets"
            | "directory.packages.props"
            | "nuget.config"
            | "global.json"
            | "makefile"
            | "gnumakefile"
            | "dockerfile"
            | "jenkinsfile"
            | "cmakelists.txt"
            | "meson.build"
            | "meson_options.txt"
            | "build"
            | "build.bazel"
            | "workspace"
            | "workspace.bazel"
            | "module.bazel"
            | "pnpm-workspace.yaml"
            | "lerna.json"
            | "nx.json"
            | "turbo.json"
            | "deno.json"
            | "vcpkg.json"
            | ".gitlab-ci.yml"
            | ".npmrc"
            | ".yarnrc"
            | ".yarnrc.yml"
            | ".tool-versions"
            | ".python-version"
            | ".ruby-version"
            | "rust-toolchain"
            | "rust-toolchain.toml"
            | ".node-version"
            | ".nvmrc"
            | "build.rs"
            | "action.yml"
            | "action.yaml"
    ) || name.starts_with("requirements") && name.ends_with(".txt")
        || name.starts_with("tsconfig") && name.ends_with(".json")
        || name.starts_with("azure-pipelines")
            && (name.ends_with(".yml") || name.ends_with(".yaml"))
        || name.starts_with("conanfile.")
        || name.starts_with("dockerfile.")
        || name.ends_with(".dockerfile")
        || name.ends_with(".gemspec")
        || name.ends_with(".csproj")
        || name.ends_with(".fsproj")
        || name.ends_with(".vbproj")
        || name.ends_with(".sln")
        || name.ends_with(".props")
        || name.ends_with(".targets")
        || name.ends_with(".cmake")
        || name.ends_with(".bzl")
        || name.ends_with(".mk")
        || [".config.js", ".config.ts", ".config.mjs", ".config.cjs"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        || lower.starts_with(".github/")
        || lower.starts_with(".cargo/")
        || lower.starts_with(".devcontainer/")
}

/// Ordinary source, test and documentation files by extension. Everything else,
/// including config of any format, is conservatively a dependency.
pub(crate) fn ordinary_file(path: &str) -> bool {
    const ORDINARY: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "kts", "cs", "fs",
        "vb", "c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx", "m", "mm", "swift", "rb", "php",
        "scala", "dart", "ex", "exs", "erl", "hs", "clj", "lua", "r", "jl", "zig", "vue", "svelte",
        "css", "scss", "sass", "less", "html", "md", "mdx", "rst", "adoc",
    ];
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    !dependency_file(path)
        && name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && ORDINARY.contains(&ext))
}

/// Digest of the head blob of every file that is not ordinary source, test or
/// documentation (`path`, `sha`). With the base fixed, any PR change to such a file,
/// classified or not, changes the digest.
pub(crate) fn manifest_digest<'a>(tree: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut entries: Vec<_> = tree
        .into_iter()
        .filter(|(path, _)| !ordinary_file(path))
        .collect();
    entries.sort_unstable();
    let mut text = String::new();
    for (path, sha) in entries {
        text.push_str(path);
        text.push('\0');
        text.push_str(sha);
        text.push('\n');
    }
    body_hash(&text)
}

/// One thread comment as any reader sees it.
pub(crate) struct Seen<'a> {
    pub id: &'a str,
    pub author_id: Option<&'a str>,
    pub body: &'a str,
}

impl<'a> From<&'a crate::packet::CommentFact> for Seen<'a> {
    fn from(c: &'a crate::packet::CommentFact) -> Self {
        Self {
            id: &c.id,
            author_id: c.author_id.as_deref(),
            body: &c.body,
        }
    }
}

impl<'a> From<&'a crate::github::threads::Comment> for Seen<'a> {
    fn from(c: &'a crate::github::threads::Comment) -> Self {
        Self {
            id: &c.id,
            author_id: c.author_id.as_deref(),
            body: &c.body,
        }
    }
}

/// A PR Sniper reply from the acting account (host marker).
pub(crate) fn own_marked(comment: &Seen<'_>, account_id: &str) -> bool {
    comment.author_id == Some(account_id) && comment.body.contains("<!-- pr-sniper:")
}

/// The thread still reads as anchored: every anchored comment present with the same
/// body, and every other comment PR Sniper's own.
pub(crate) fn snapshot_holds(
    anchor: &ResolutionAnchor,
    comments: &[Seen<'_>],
    own: impl Fn(&Seen<'_>) -> bool,
) -> bool {
    anchor.comments.iter().all(|(id, hash)| {
        comments
            .iter()
            .any(|c| c.id == id && body_hash(c.body) == *hash)
    }) && comments
        .iter()
        .all(|c| anchor.comments.contains_key(c.id) || own(c))
}

/// An accepted author won't-fix still holds: the cited reply is unchanged and still the
/// PR author's (provider identity, also unchanged), and nothing but PR Sniper's own
/// marked replies came after the accepted snapshot.
pub(crate) fn author_reply_holds(
    evidence: &Assessment,
    comments: &[Seen<'_>],
    author: Option<&str>,
    account_id: &str,
) -> bool {
    let (Some(author), Some(anchor), Some(cited)) = (
        author,
        evidence.anchor.as_ref(),
        evidence.author_comment_id.as_deref(),
    ) else {
        return false;
    };
    anchor.author_id.as_deref() == Some(author)
        && anchor.comments.contains_key(cited)
        && comments
            .iter()
            .position(|c| c.id == cited)
            .is_some_and(|index| index > 0 && comments[index].author_id == Some(author))
        && snapshot_holds(anchor, comments, |c| own_marked(c, account_id))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub thread_id: String,
    pub disposition: Disposition,
    pub head_sha: String,
    pub base_sha: String,
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duck_call: Option<u32>,
    /// `author_wont_fix` only: the PR author's reply the primary accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_comment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<ResolutionAnchor>,
}

impl Assessment {
    pub(crate) fn duck_confirms(&self, state_id: &str, ducks: &[DuckCall]) -> bool {
        ducks.iter().any(|d| {
            Some(d.ordinal) == self.duck_call
                && d.state_id == state_id
                && d.input.kind == TargetKind::Thread
                && d.input.thread_id.as_deref() == Some(&self.thread_id)
                && d.input.verify_fixed
                && d.input.claim == self.rationale
                && d.input.evidence == self.evidence
                && matches!(&d.outcome, DuckOutcome::Verdict { verdict }
                    if verdict.verdict == VerdictKind::Confirmed && !verdict.evidence.is_empty())
        })
    }
}

pub(crate) fn action_clearance(assessments: &[Assessment]) -> Result<(), String> {
    if assessments.iter().any(|a| {
        matches!(
            a.disposition,
            Disposition::StillApplies | Disposition::Unverifiable
        )
    }) {
        Err("Applicable or unverifiable findings prevent approval and merge; use ready_for_human or leave_comments.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn proofs(
    store: &Store,
    job: &QueueJob,
    assignment: &str,
) -> Result<Vec<Ownership>, String> {
    let mut proofs: Vec<_> = store
        .load_publications()?
        .iter()
        .filter(|p| crate::feedback::same_pr(&p.review.job, job))
        .filter_map(|p| p.ownership().ok())
        .collect();
    proofs.extend(
        crate::retention::load(store)?
            .receipts
            .into_iter()
            .flat_map(|r| r.owned)
            .filter(|o| o.matches(job))
            .map(|o| o.proof),
    );
    proofs.retain(|p| p.assignment_id == assignment && p.account_id == job.account_id);
    Ok(proofs)
}

pub(crate) fn stamp_ownership(
    threads: &mut [ThreadFact],
    proofs: &[Ownership],
    assignment: &str,
    account: &str,
) {
    // Identity survives edits; resolution still needs the full ownership proof.
    for thread in threads {
        thread.mine = thread.comments.first().is_some_and(|root| {
            proofs.iter().any(|proof| {
                proof.assignment_id == assignment
                    && proof.account_id == account
                    && proof.root_ids.contains(&root.id)
            })
        });
    }
}

pub fn assessment_schema() -> Value {
    json!({"type":"array","items":{"type":"object","additionalProperties":false,
        "properties":{
            "thread_id":{"type":"string"},
            "disposition":{"type":"string","enum":["still_applies","fixed","withdrawn","unverifiable","author_wont_fix","no_longer_applies"]},
            "head_sha":{"type":"string"},"base_sha":{"type":"string"},
            "rationale":{"type":"string","maxLength":4096},
            "evidence":{"type":"array","maxItems":10,"items":super::duck::reference_schema()},
            "duck_call":{"type":"integer","minimum":1},
            "author_comment_id":{"type":"string","maxLength":256,"description":"author_wont_fix only: the PR author's latest reply in that thread."}},
        "description":"no_longer_applies: only when the head changed since the thread was posted, with current-head evidence.",
        "required":["thread_id","disposition","head_sha","base_sha","rationale","evidence"]}})
}

/// Remove host metadata before the legacy exit parser. Existing identities become
/// replies before diff mapping or publication can ever create another root.
pub(crate) fn extract(args: &Value) -> Result<(Value, Vec<Assessment>), String> {
    let mut args = args.clone();
    let object = args
        .as_object_mut()
        .ok_or("Exit arguments must be an object.")?;
    let dispositions = object.remove("finding_dispositions");
    if dispositions
        .as_ref()
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.get("anchor").is_some()))
    {
        return Err("Malformed finding_dispositions.".into());
    }
    let assessments = dispositions
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "Malformed finding_dispositions.")?
        .unwrap_or_default();
    if let Some(comments) = object.get_mut("new_comments").and_then(Value::as_array_mut) {
        let mut replies = Vec::new();
        let mut fresh = Vec::new();
        for mut comment in comments.drain(..) {
            match comment.as_object_mut().and_then(|c| c.remove("finding_id")) {
                Some(Value::String(id)) if !id.trim().is_empty() => {
                    let body = comment
                        .get("body")
                        .cloned()
                        .ok_or("Finding update needs body.")?;
                    // Validate the full original comment too; do not silently accept unknown fields.
                    serde_json::from_value::<crate::exits::NewComment>(comment)
                        .map_err(|_| "Malformed finding update.")?;
                    replies.push(json!({"thread_id":id,"body":body}));
                }
                Some(Value::Null) | None => fresh.push(comment),
                _ => return Err("finding_id must be null (new) or an existing thread id.".into()),
            }
        }
        *comments = fresh;
        if !replies.is_empty() {
            let existing = object.entry("reply_comments").or_insert_with(|| json!([]));
            existing
                .as_array_mut()
                .ok_or("reply_comments must be an array.")?
                .extend(replies);
        }
    }
    Ok((args, assessments))
}

pub(crate) struct Contract {
    pub head: String,
    pub base: String,
    pub state_id: String,
    pub threads: Vec<ThreadFact>,
    pub duck_required: bool,
    pub corrections: Vec<Correction>,
    pub identities: crate::packet::FindingFacts,
    /// `ready_for_human` was already refused once in this run as an `lgtm` that is due.
    pub handoff_nudged: std::sync::atomic::AtomicBool,
}

/// Refusal of `ready_for_human` when the review is clear and `lgtm` is offered.
pub const LGTM_DUE: &str = "Nothing here needs a human: the review is clear and lgtm is offered. Call lgtm; the host acts on your authority, and pending CI only parks it until green. A defect goes in leave_comments. Only a concern you cannot post on the diff needs ready_for_human: name it in the summary and call ready_for_human again.";

/// A comment PR Sniper wrote: a saved receipt, or the acting account's comment with
/// a host marker or signature. The acting account may also be the PR author: its
/// unmarked comments are the author's, and a marker from another account is not ours.
fn own_comment(ids: &crate::packet::FindingFacts, comment: &crate::packet::CommentFact) -> bool {
    ids.own_comments.contains(&comment.id)
        || (comment.author_id.as_deref() == Some(ids.account_id.as_str())
            && (comment.body.contains("<!-- pr-sniper:")
                || [
                    crate::exit_writes::SIGNATURE,
                    crate::exit_writes::LEGACY_SIGNATURE,
                ]
                .iter()
                .any(|signature| comment.body.trim_end().ends_with(signature))))
}

impl Contract {
    /// The cited won't-fix reply is the PR author's latest word in the thread, by
    /// provider identity, and no one else spoke after it. Its text is evidence for
    /// the primary's judgment, never an instruction to the host.
    fn author_reply(&self, thread: &ThreadFact, id: &str) -> Result<(), String> {
        let ids = &self.identities;
        let author = ids
            .pr_author_id
            .as_deref()
            .ok_or("PR author identity unavailable; author_wont_fix cannot be verified.")?;
        let index = thread
            .comments
            .iter()
            .position(|c| c.id == id)
            .filter(|index| *index > 0)
            .ok_or("author_comment_id must be a reply in that thread, not its root.")?;
        let by_author = |c: &crate::packet::CommentFact| c.author_id.as_deref() == Some(author);
        let cited = &thread.comments[index];
        if !by_author(cited) || own_comment(ids, cited) {
            return Err(
                "author_comment_id is not a reply by the PR author (provider identity).".into(),
            );
        }
        for later in &thread.comments[index + 1..] {
            if own_comment(ids, later) {
                continue;
            }
            return Err(if by_author(later) {
                "Cite the PR author's latest reply in that thread."
            } else {
                "Someone else replied after the author; answer that concern. Use still_applies, unverifiable or ready_for_human."
            }
            .into());
        }
        Ok(())
    }

    /// A reply answers someone: the thread's latest comment is not PR Sniper's, or it is
    /// a reply the finding contract requires. Anything else restates an open finding or
    /// re-announces unchanged state (status chatter).
    fn answers(&self, reply: &crate::exits::ReplyComment, assessments: &[Assessment]) -> bool {
        let Some(thread) = self.threads.iter().find(|t| t.id == reply.thread_id) else {
            return true; // Unknown ids are refused by the exit gate.
        };
        let addressed = thread
            .comments
            .last()
            .is_some_and(|last| !own_comment(&self.identities, last));
        // A correction of a mistaken resolution, or the acceptance of an author's won't-fix.
        let required = thread.mine
            && assessments.iter().any(|a| {
                a.thread_id == thread.id
                    && reply.body.contains(&a.rationale)
                    && match a.disposition {
                        Disposition::StillApplies => thread.resolved,
                        Disposition::AuthorWontFix => true,
                        _ => false,
                    }
            });
        addressed || required
    }

    pub fn prepare(
        &self,
        call: &mut ExitCall,
        assessments: &[Assessment],
        ducks: &[DuckCall],
        served: impl Fn(&EvidenceRef) -> Result<(), Unserved>,
    ) -> Result<(), String> {
        self.validate(call, assessments, ducks, served)?;
        if let ExitCall::LeaveComments { reply_comments, .. } = call {
            for a in assessments.iter().filter(|a| {
                a.disposition == Disposition::StillApplies
                    && self
                        .threads
                        .iter()
                        .any(|t| t.id == a.thread_id && t.resolved)
            }) {
                let Some(reply) = reply_comments
                    .iter_mut()
                    .find(|r| r.thread_id == a.thread_id && r.body.contains(&a.rationale))
                else {
                    continue;
                };
                reply.body = correction_text(&reply.body);
                if reply.body.len() > crate::exits::MAX_COMMENT_BYTES {
                    return Err("Correction reply exceeds 16384 bytes; shorten it.".into());
                }
            }
        }
        Ok(())
    }

    /// The primary's sign-off. Every own open thread needs a resolvable disposition
    /// (fixed, author_wont_fix, withdrawn, no_longer_applies) under the usual
    /// evidence rules, nothing may still apply, and every teammate finding of this
    /// run is dismissed with a rationale. Only accounted own open threads are
    /// resolved; an own resolved thread left unclassified is recorded `signed_off`.
    pub fn sign_off(
        &self,
        call: &mut ExitCall,
        assessments: &mut Vec<Assessment>,
        ducks: &[DuckCall],
        served: impl Fn(&EvidenceRef) -> Result<(), Unserved>,
    ) -> Result<(), String> {
        let ExitCall::Lgtm {
            dismissed_teammate_findings: dismissed,
            ..
        } = &*call
        else {
            return Ok(());
        };
        if self.corrections.iter().any(|c| {
            !self
                .threads
                .iter()
                .any(|t| t.id == c.thread_id && c.confirmed_in(t))
        }) {
            return Err("An earlier correction outcome is unresolved; lgtm cannot proceed. Use ready_for_human.".into());
        }
        let teammates = &self.identities.teammate_findings;
        if let Some(unknown) = dismissed.iter().find(|d| !teammates.contains(&d.id)) {
            return Err(format!(
                "Unknown teammate finding {}. Copy its exact id (T<k>#<n>) from Teammate reports.",
                unknown.id
            ));
        }
        let mut seen = BTreeSet::new();
        if let Some(twice) = dismissed.iter().find(|d| !seen.insert(d.id.as_str())) {
            return Err(format!("Dismiss teammate finding {} once.", twice.id));
        }
        let undismissed: Vec<&str> = teammates
            .iter()
            .filter(|id| !dismissed.iter().any(|d| d.id == **id))
            .map(String::as_str)
            .collect();
        if !undismissed.is_empty() {
            return Err(format!(
                "lgtm cannot pass over teammate findings: dismiss each in dismissed_teammate_findings with a rationale, or publish it with leave_comments. Undismissed: {}.",
                undismissed.join(", ")
            ));
        }
        self.validate(call, assessments, ducks, served)?;
        let applicable: Vec<&str> = assessments
            .iter()
            .filter(|a| {
                matches!(
                    a.disposition,
                    Disposition::StillApplies | Disposition::Unverifiable
                )
            })
            .map(|a| a.thread_id.as_str())
            .collect();
        if !applicable.is_empty() {
            return Err(format!(
                "lgtm cannot clear findings that still apply or are unverifiable: {}. Use leave_comments or ready_for_human.",
                applicable.join(", ")
            ));
        }
        let open: Vec<&ThreadFact> = self
            .threads
            .iter()
            .filter(|t| t.mine && !t.resolved && !t.carried)
            .collect();
        let unaccounted: Vec<&str> = open
            .iter()
            .filter(|t| {
                !assessments
                    .iter()
                    .any(|a| a.thread_id == t.id && a.disposition.signs_off())
            })
            .map(|t| t.id.as_str())
            .collect();
        if !unaccounted.is_empty() {
            return Err(format!(
                "lgtm needs a finding_dispositions entry for every own open thread: fixed (verified evidence), author_wont_fix, withdrawn (your rationale) or no_longer_applies (head changed since the thread, with evidence). Unaccounted: {}.",
                unaccounted.join(", ")
            ));
        }
        let ExitCall::Lgtm {
            comment,
            resolve_own_threads,
            action,
            ..
        } = call
        else {
            return Ok(());
        };
        let unclassified: Vec<Assessment> = self
            .threads
            .iter()
            .filter(|t| t.mine && t.resolved && !t.carried)
            .filter(|t| !assessments.iter().any(|a| a.thread_id == t.id))
            .map(|t| Assessment {
                thread_id: t.id.clone(),
                disposition: Disposition::SignedOff,
                head_sha: self.head.clone(),
                base_sha: self.base.clone(),
                rationale: comment.clone(),
                evidence: Vec::new(),
                duck_call: None,
                author_comment_id: None,
                anchor: None,
            })
            .collect();
        assessments.extend(unclassified);
        *resolve_own_threads = open.iter().map(|t| t.id.clone()).collect();
        *action = self.identities.lgtm_action;
        Ok(())
    }

    /// Deterministically clear: `lgtm` was offered, no teammate finding, unsettled
    /// duck call or unconfirmed correction, nothing still applies or is unverifiable,
    /// and every own open thread has a sign-off disposition.
    fn lgtm_due(&self, assessments: &[Assessment], ducks: &[DuckCall]) -> bool {
        self.identities.lgtm_offered
            && self.identities.teammate_findings.is_empty()
            && super::duck::unsettled(ducks).next().is_none()
            && self.corrections.iter().all(|c| {
                self.threads
                    .iter()
                    .any(|t| t.id == c.thread_id && c.confirmed_in(t))
            })
            && action_clearance(assessments).is_ok()
            && self
                .threads
                .iter()
                .filter(|t| t.mine && !t.resolved && !t.carried)
                .all(|t| {
                    assessments
                        .iter()
                        .any(|a| a.thread_id == t.id && a.disposition.signs_off())
                })
    }

    pub fn validate(
        &self,
        call: &ExitCall,
        assessments: &[Assessment],
        ducks: &[DuckCall],
        served: impl Fn(&EvidenceRef) -> Result<(), Unserved>,
    ) -> Result<(), String> {
        if call.exit() == Exit::ReadyForHuman
            && self.lgtm_due(assessments, ducks)
            && !self
                .handoff_nudged
                .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(LGTM_DUE.into());
        }
        if call.exit() == Exit::Wait && super::duck::unsettled(ducks).next().is_some() {
            return Err("Unsettled Rubber Duck evidence cannot become an external wait.".into());
        }
        if matches!(
            call.exit(),
            Exit::PanicExit | Exit::SubmitReport | Exit::SubmitVerdict
        ) {
            return if assessments.is_empty() {
                Ok(())
            } else {
                Err("Finding dispositions are primary-only.".into())
            };
        }
        if let ExitCall::LeaveComments {
            new_comments,
            reply_comments,
            ..
        } = call
        {
            if self.identities.revision_reviewed && !new_comments.is_empty() {
                return Err("One review per revision: you already published on this head. No new findings until a new head. Answer only questions asked of you; else ready_for_human.".into());
            }
            if let Some(reply) = reply_comments
                .iter()
                .find(|r| !self.answers(r, assessments))
            {
                return Err(format!("No status chatter on {}: reply only to answer a comment addressed to you. An open thread already states its finding; omit the reply, or ready_for_human.", reply.thread_id));
            }
        }
        if let ExitCall::LeaveComments { reply_comments, .. } = call {
            for reply in reply_comments {
                let previous: Vec<_> = self
                    .corrections
                    .iter()
                    .filter(|c| c.thread_id == reply.thread_id)
                    .collect();
                if previous.is_empty() {
                    continue;
                }
                let thread = self.threads.iter().find(|t| t.id == reply.thread_id);
                if previous
                    .iter()
                    .any(|c| !thread.is_some_and(|t| c.confirmed_in(t)))
                {
                    return Err("Earlier correction outcome is unresolved or its receipt is unreadable; no replacement reply will be posted. Omit that reply or choose ready_for_human.".into());
                }
                if previous
                    .iter()
                    .any(|c| correction_text(&reply.body) == c.text)
                {
                    return Err("That correction is already confirmed on the original thread. Omit the duplicate reply or choose ready_for_human; revise only for new information.".into());
                }
            }
        }
        let own: BTreeSet<_> = self
            .threads
            .iter()
            .filter(|t| t.mine)
            .map(|t| t.id.as_str())
            .collect();
        let mut seen = BTreeSet::new();
        for a in assessments {
            if !own.contains(a.thread_id.as_str()) || !seen.insert(a.thread_id.as_str()) {
                return Err("Classify each own finding once, using its exact thread id.".into());
            }
            if a.head_sha != self.head || a.base_sha != self.base {
                return Err("Finding evidence must name this packet's exact head and base.".into());
            }
            if a.rationale.trim().is_empty() || a.rationale.len() > 4096 || a.evidence.len() > 10 {
                return Err(
                    "Finding rationale is required (<=4096 bytes), evidence <=10 refs.".into(),
                );
            }
            super::duck::evidence(&a.evidence)?;
            if a.duck_call == Some(0) {
                return Err("duck_call must be a positive call ordinal.".into());
            }
            if a.disposition == Disposition::SignedOff {
                return Err("signed_off is host-only; call lgtm to sign off.".into());
            }
            let thread = self.threads.iter().find(|t| t.id == a.thread_id).unwrap();
            match (a.disposition, a.author_comment_id.as_deref()) {
                (Disposition::AuthorWontFix, Some(id)) => {
                    self.author_reply(thread, id)?;
                }
                (Disposition::AuthorWontFix, None) => {
                    return Err("author_wont_fix needs author_comment_id: the PR author's latest reply in that thread.".into());
                }
                (_, Some(_)) => {
                    return Err("author_comment_id only with author_wont_fix.".into());
                }
                _ => {}
            }
            if a.disposition == Disposition::NoLongerApplies {
                if a.evidence.is_empty() {
                    return Err(format!("no_longer_applies {}: needs current-head code evidence actually read in this attempt.", a.thread_id));
                }
                if let Some(reason) = super::duck::unserved(&a.evidence, &served) {
                    return Err(format!("no_longer_applies {}: needs current-head code evidence actually read in this attempt. {reason}", a.thread_id));
                }
                if self
                    .identities
                    .thread_heads
                    .get(&a.thread_id)
                    .is_none_or(|posted| *posted == self.head)
                {
                    return Err(format!("no_longer_applies {}: the head has not changed since that thread was posted. Use fixed, withdrawn or still_applies.", a.thread_id));
                }
            }
            if a.disposition == Disposition::Fixed {
                if a.evidence.is_empty() {
                    return Err(format!("Fixed {}: needs current-head code evidence actually read in this attempt; outdated alone is not evidence.", a.thread_id));
                }
                if let Some(reason) = super::duck::unserved(&a.evidence, &served) {
                    return Err(format!("Fixed {}: needs current-head code evidence actually read in this attempt. {reason}", a.thread_id));
                }
                if self.duck_required && !a.duck_confirms(&self.state_id, ducks) {
                    return Err("Fixed needs a confirmed rubber_duck verify_fixed call for this thread, rationale and evidence on this state.".into());
                }
            }
            if thread.resolved
                && a.disposition == Disposition::StillApplies
                && !self.corrections.iter().any(|c| c.thread_id == a.thread_id)
            {
                let corrected = matches!(call, ExitCall::LeaveComments { reply_comments, .. }
                    if reply_comments.iter().any(|r| r.thread_id == a.thread_id && r.body.contains(&a.rationale)));
                if !corrected {
                    return Err("Earlier resolution was mistaken: reply to that existing thread with the correction rationale, never post a new root.".into());
                }
            }
        }
        let carried: BTreeSet<_> = self
            .threads
            .iter()
            .filter(|t| t.mine && t.carried)
            .map(|t| t.id.as_str())
            .collect();
        // A sign-off accounts for its own open threads itself (`sign_off`).
        if call.exit() != Exit::Lgtm && own.difference(&seen).any(|id| !carried.contains(id)) {
            return Err("Classify every own finding, including resolved/outdated threads, except carried_forward ones: still_applies, fixed, withdrawn, unverifiable, author_wont_fix or no_longer_applies.".into());
        }
        if let ExitCall::LeaveComments {
            resolve_own_threads,
            reply_comments,
            ..
        } = call
        {
            for id in resolve_own_threads {
                match assessments.iter().find(|a| a.thread_id == *id) {
                    Some(a) if a.disposition == Disposition::Fixed => {}
                    Some(a) if a.disposition == Disposition::AuthorWontFix => {
                        if !reply_comments
                            .iter()
                            .any(|r| r.thread_id == *id && r.body.contains(&a.rationale))
                        {
                            return Err("Resolving an author won't-fix needs a reply on that thread containing your acceptance rationale.".into());
                        }
                    }
                    _ => return Err("Only own findings classified fixed with current-code evidence, or author_wont_fix, may be resolved.".into()),
                }
            }
        }
        if matches!(
            call.exit(),
            Exit::ApprovePullRequest | Exit::ApproveAndMerge | Exit::Wait
        ) {
            action_clearance(assessments)?;
        }
        Ok(())
    }
}

/// The latest PR Sniper resolution of one own thread, with its own later replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub thread_id: String,
    pub evidence: Assessment,
    pub replies: BTreeSet<String>,
}

/// Resolutions still standing per thread: a later correction, attempted resolution
/// or missing anchor ends the carried episode. Built from existing write intents.
pub(crate) fn resolutions(
    writes: &[crate::exit_writes::ExitWrite],
    job: &QueueJob,
    assignment: &str,
) -> Vec<Resolved> {
    use crate::exit_writes::{StepKind, StepState};
    let mut ordered: Vec<_> = writes
        .iter()
        .enumerate()
        .filter(|(_, write)| {
            crate::feedback::same_pr(&write.job, job) && write.assignment_id == assignment
        })
        .collect();
    ordered.sort_by_key(|(index, write)| (write.operation.initial_attempt_at, *index));
    let mut latest: BTreeMap<String, Option<Assessment>> = BTreeMap::new();
    let mut replies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (_, write) in ordered {
        for step in &write.steps {
            if step.state == StepState::Planned {
                continue;
            }
            match step.kind {
                StepKind::Reply => {
                    if let Some(receipt) = &step.receipt {
                        replies
                            .entry(step.thread_id.clone())
                            .or_default()
                            .insert(receipt.clone());
                    }
                    if step.body.starts_with(CORRECTION) {
                        latest.insert(step.thread_id.clone(), None);
                    }
                }
                StepKind::Resolve => {
                    let standing = step
                        .resolution_evidence
                        .clone()
                        .filter(|a| step.state == StepState::Done && a.anchor.is_some());
                    latest.insert(step.thread_id.clone(), standing);
                }
            }
        }
    }
    latest
        .into_iter()
        .filter_map(|(thread_id, evidence)| {
            Some(Resolved {
                replies: replies.remove(&thread_id).unwrap_or_default(),
                evidence: evidence?,
                thread_id,
            })
        })
        .collect()
}

/// Every reply receipt PR Sniper holds on this PR, any assignment.
pub(crate) fn reply_receipts(
    writes: &[crate::exit_writes::ExitWrite],
    job: &QueueJob,
) -> BTreeSet<String> {
    writes
        .iter()
        .filter(|write| crate::feedback::same_pr(&write.job, job))
        .flat_map(|write| &write.steps)
        .filter(|step| step.kind == crate::exit_writes::StepKind::Reply)
        .filter_map(|step| step.receipt.clone())
        .collect()
}

/// Current state a standing resolution is compared with.
pub(crate) struct Current<'a> {
    pub base_sha: &'a str,
    pub manifests: &'a str,
    pub head_blob: &'a dyn Fn(&str) -> Option<String>,
}

/// Mark own resolved threads whose standing resolution still holds: provider thread
/// still resolved, same base, same dependency files, every anchored file the same
/// head blob, every anchored comment unedited and no comment beyond them but PR
/// Sniper's own replies. Anything else re-opens the finding for a fresh
/// classification (and, for fixed, a fresh duck check).
pub(crate) fn stamp_carried(
    threads: &mut [ThreadFact],
    resolutions: &[Resolved],
    now: &Current<'_>,
) {
    for thread in threads {
        thread.carried = thread.mine
            && thread.resolved
            && resolutions.iter().any(|r| {
                let Some(anchor) = r.evidence.anchor.as_ref() else {
                    return false;
                };
                let comments: Vec<Seen<'_>> = thread.comments.iter().map(Seen::from).collect();
                r.thread_id == thread.id
                    && matches!(
                        r.evidence.disposition,
                        Disposition::Fixed
                            | Disposition::AuthorWontFix
                            | Disposition::SignedOff
                            | Disposition::Withdrawn
                            | Disposition::NoLongerApplies
                    )
                    && anchor.base_sha == now.base_sha
                    && anchor.manifests == now.manifests
                    && !anchor.blobs.is_empty()
                    && anchor
                        .blobs
                        .iter()
                        .all(|(path, sha)| (now.head_blob)(path).unwrap_or_default() == *sha)
                    && snapshot_holds(anchor, &comments, |c| r.replies.contains(c.id))
            });
    }
}

/// What an accepted disposition relied on at exit.
pub(crate) struct Relied<'a> {
    pub base_sha: &'a str,
    pub manifests: &'a str,
    /// Every path the primary or its Rubber Duck was served in the attempt.
    pub read: &'a BTreeSet<String>,
    pub pr_author_id: Option<&'a str>,
    pub head_blob: &'a dyn Fn(&str) -> Option<String>,
}

/// Stamp the anchor of every resolvable disposition at exit: base, dependency digest,
/// head blobs of the cited files, the thread's file and every file read, the thread's
/// comment body hashes and, for author_wont_fix, the verified PR author id.
pub(crate) fn stamp_anchors(
    assessments: &mut [Assessment],
    threads: &[ThreadFact],
    relied: &Relied<'_>,
) {
    for a in assessments {
        if !matches!(
            a.disposition,
            Disposition::Fixed
                | Disposition::AuthorWontFix
                | Disposition::SignedOff
                | Disposition::Withdrawn
                | Disposition::NoLongerApplies
        ) {
            continue;
        }
        let Some(thread) = threads.iter().find(|t| t.id == a.thread_id) else {
            continue;
        };
        let blobs = a
            .evidence
            .iter()
            .filter(|e| e.side.as_deref().unwrap_or("head") == "head")
            .map(|e| e.path.as_str())
            .chain(thread.path.as_deref())
            .chain(relied.read.iter().map(String::as_str))
            .map(|path| {
                (
                    path.to_string(),
                    (relied.head_blob)(path).unwrap_or_default(),
                )
            })
            .collect();
        a.anchor = Some(ResolutionAnchor {
            base_sha: relied.base_sha.to_string(),
            blobs,
            manifests: relied.manifests.to_string(),
            comments: thread
                .comments
                .iter()
                .map(|c| (c.id.clone(), body_hash(&c.body)))
                .collect(),
            author_id: (a.disposition == Disposition::AuthorWontFix)
                .then(|| relied.pr_author_id.map(str::to_string))
                .flatten(),
        });
    }
}

#[cfg(test)]
mod tests;
