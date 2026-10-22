//! Rubber duck contract: the primary's `rubber_duck` input, the duck's `submit_verdict`
//! exit, typed call failures and the evidence stored with the primary's result.
//! Agent text on both sides is untrusted data. Nothing here writes to a provider.

use crate::exits::{MAX_COMMENT_BYTES, MAX_FULL_BYTES, MAX_NANO_WORDS, MAX_PATH_BYTES};
use crate::storage::{Agent, Doctrine, DuckLimits};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The primary's tool id. Never offered to teammates or to the duck itself.
pub const TOOL: &str = "rubber_duck";
pub const MAX_EVIDENCE: usize = 10;
pub const MAX_CLAIM_WORDS: usize = 100;
/// Byte caps beside the word caps: one "word" can be arbitrarily long.
pub const MAX_CLAIM_BYTES: usize = 1024;
pub const MAX_VERDICT_NANO_BYTES: usize = 1024;
/// Primary time kept free for its exit after a duck call.
pub const EXIT_RESERVE_SECONDS: u64 = 120;
/// A call with less time than this left in its window is not started.
pub const MIN_CALL_SECONDS: u64 = 60;
/// Refused calls (no session) kept per attempt beyond the started ones; later
/// refusals only raise the last refusal's `repeats`.
pub const MAX_REFUSAL_ENTRIES: usize = 8;
/// A verify_fixed thread up to this size is served whole inside the duck packet.
pub const MAX_INLINE_THREAD_BYTES: usize = 24 * 1024;
/// Changed-file counts at which a call's timeout grows 1.5x and 2x.
pub const LARGE_PR_FILES: usize = 40;
pub const HUGE_PR_FILES: usize = 100;

/// The per-call timeout for a PR of `changed_files`: the saved preference, 1.5x from
/// 40 files and 2x from 100, never above the preference range. Calls stay serialized
/// and bounded by `max_calls`; only wall time grows with the PR.
pub fn call_timeout(configured: u32, changed_files: usize) -> u32 {
    let scaled = if changed_files >= HUGE_PR_FILES {
        configured.saturating_mul(2)
    } else if changed_files >= LARGE_PR_FILES {
        configured.saturating_add(configured / 2)
    } else {
        configured
    };
    scaled.min(*DuckLimits::CALL_TIMEOUT_SECONDS.end())
}

/// Primary window for own findings that may need a verify_fixed call: one applied
/// (PR-size scaled) call timeout per finding, at most `max_calls`, never past the
/// window preference range.
pub fn primary_window_seconds(
    limits: &DuckLimits,
    own_findings: usize,
    changed_files: usize,
) -> i64 {
    let calls = u32::try_from(own_findings)
        .unwrap_or(u32::MAX)
        .min(limits.max_calls);
    let seconds = u64::from(limits.primary_window_minutes) * 60
        + u64::from(calls) * u64::from(call_timeout(limits.call_timeout_seconds, changed_files));
    seconds.min(u64::from(*DuckLimits::PRIMARY_WINDOW_MINUTES.end()) * 60) as i64
}

/// The exit error a duck gets for an unproven verify_fixed confirmation.
pub fn refusal(reason: DuckReason) -> &'static str {
    match reason {
        DuckReason::ThreadUnread => "confirmed refused: read the whole original thread first (get_thread with its thread_id, every page). Then submit again, or submit rejected or unverifiable.",
        DuckReason::EvidenceMissing => "confirmed refused: cite the current-head lines you read in evidence (path, side head, line).",
        _ => "confirmed refused: cite only current-head lines served to you by read_source or read_changes after. Read them, then submit again.",
    }
}

pub const DESCRIPTION: &str = "Ask the operator's Rubber Duck Agent to verify one finding before you post it. Fresh session, same PR state, read-only. Returns a verdict (confirmed, revise, rejected, unverifiable) as data. You decide. Bounded calls; one at a time.";
pub const VERDICT_DESCRIPTION: &str =
    "Terminal exit: your verdict on the one claim. Caveman. Ends the run.";

/// The Rubber Duck Agent at job start: immutable evidence, never current config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuckSnapshot {
    pub agent: Agent,
    /// In Agent-selected order, with the exact saved title and full body.
    pub doctrines: Vec<Doctrine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// An inline comment on a diff line.
    Line,
    /// A reply in an existing thread.
    Thread,
    /// A PR-level review body point.
    Pr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
}

/// One finding the primary wants verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuckInput {
    /// Independently test whether an existing finding is fixed, not whether a new defect exists.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verify_fixed: bool,
    pub kind: TargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub claim: String,
    pub proposed_body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Confirmed,
    Revise,
    Rejected,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub path: String,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
}

/// The duck's `submit_verdict`. Agent output: untrusted data for the primary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub verdict: VerdictKind,
    pub nano: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_line: Option<Anchor>,
}

/// Why a duck call returned no verdict. Typed; the primary sees the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuckFailure {
    InvalidInput,
    LimitReached,
    BudgetExhausted,
    ExitTaken,
    Unavailable,
    RateLimited,
    TimedOut,
    NoVerdict,
    /// verify_fixed confirmed without the whole original thread or served head evidence.
    Unverified,
    Failed,
}

/// Content-free cause of a `failed`, `no_verdict` or `unverified` call, for the
/// primary's tool result and diagnostics. Never inferred from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuckReason {
    SessionCreate,
    Intelligence,
    Catalog,
    Send,
    StreamLost,
    SourceRead,
    ReadLimit,
    ExitInvalid,
    ThreadUnread,
    EvidenceUnserved,
    EvidenceMissing,
    Task,
}

impl DuckReason {
    pub fn id(self) -> &'static str {
        match self {
            Self::SessionCreate => "session_create",
            Self::Intelligence => "intelligence",
            Self::Catalog => "catalog",
            Self::Send => "send",
            Self::StreamLost => "stream_lost",
            Self::SourceRead => "source_read",
            Self::ReadLimit => "read_limit",
            Self::ExitInvalid => "exit_invalid",
            Self::ThreadUnread => "thread_unread",
            Self::EvidenceUnserved => "evidence_unserved",
            Self::EvidenceMissing => "evidence_missing",
            Self::Task => "task",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::ThreadUnread => {
                "Duck confirmed without reading the whole original thread. Retry once or decide."
            }
            Self::EvidenceUnserved => {
                "Duck cited head lines it was not served. Retry once or decide."
            }
            Self::EvidenceMissing => {
                "Duck confirmed without head-line evidence. Retry once or decide."
            }
            Self::SourceRead | Self::ReadLimit => "Duck read failed. Retry once or decide.",
            _ => "Duck runtime failed. Retry once or decide.",
        }
    }
}

impl DuckFailure {
    pub fn id(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::LimitReached => "limit_reached",
            Self::BudgetExhausted => "budget_exhausted",
            Self::ExitTaken => "exit_taken",
            Self::Unavailable => "unavailable",
            Self::RateLimited => "rate_limited",
            Self::TimedOut => "timed_out",
            Self::NoVerdict => "no_verdict",
            Self::Unverified => "unverified",
            Self::Failed => "failed",
        }
    }

    /// Short caveman directive for the primary.
    fn hint(self) -> &'static str {
        match self {
            Self::InvalidInput => "Fix input to match schema and limits.",
            Self::LimitReached => "No duck calls left. You decide.",
            Self::BudgetExhausted => "No time left for a duck. You decide. Exit soon.",
            Self::ExitTaken => "You already exited. Stop.",
            Self::Unavailable => "Duck unavailable now. You decide.",
            Self::RateLimited => "Duck rate limited. You decide.",
            Self::TimedOut => "Duck timed out. Retry once or decide.",
            Self::NoVerdict => "Duck gave no verdict. Retry once or decide.",
            Self::Unverified => "Duck confirmed fixed without proof it read. Retry once or decide.",
            Self::Failed => "Duck failed. Retry once or decide.",
        }
    }

    /// The error text returned to the primary: id first, then a directive.
    pub fn message(self, detail: Option<&str>) -> String {
        match detail {
            Some(detail) => format!("duck_failure: {}. {detail}", self.id()),
            None => format!("duck_failure: {}. {}", self.id(), self.hint()),
        }
    }

    /// The error text with its typed cause, when known.
    pub fn explained(self, reason: Option<DuckReason>) -> String {
        match reason {
            Some(reason) => format!(
                "duck_failure: {} (reason: {}). {}",
                self.id(),
                reason.id(),
                reason.hint()
            ),
            None => self.message(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum DuckOutcome {
    Verdict { verdict: Verdict },
    Failure { failure: DuckFailure },
}

/// One duck call, stored with the primary's result. Local evidence only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuckCall {
    pub ordinal: u32,
    pub input: DuckInput,
    pub outcome: DuckOutcome,
    /// Limits applied to this call (Saved Preferences at attempt start).
    pub limits: DuckLimits,
    pub agent: String,
    pub model: String,
    /// The primary packet's PR state: a verdict is never valid for another.
    pub state_id: String,
    /// Content-free duck packet record; absent when no session was started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet: Option<crate::packet::PacketRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Typed cause of a failed call; absent on verdicts and older records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DuckReason>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub tool_calls: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<super::ReadCoverage>,
    pub duration_ms: u64,
    /// Further refusals with this entry's failure folded into it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub repeats: u32,
    /// Refusals of other kinds folded in once the refusal entry cap was reached.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub others: BTreeMap<DuckFailure, u32>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl DuckCall {
    /// Refused before any session: no packet was built.
    pub fn refused(&self) -> bool {
        self.packet.is_none() && matches!(self.outcome, DuckOutcome::Failure { .. })
    }
}

/// Append one call to an attempt's log, keeping it bounded: a refused call keeps only
/// its target and claim, repeats of the last refusal fold into it, and at most
/// [`MAX_REFUSAL_ENTRIES`] refusals are kept. Started calls are bounded by `max_calls`.
pub fn log_call(log: &mut Vec<DuckCall>, mut call: DuckCall) {
    if call.refused() {
        call.input.proposed_body.clear();
        call.input.evidence.clear();
        let refusals = log.iter().filter(|c| c.refused()).count();
        let same = log
            .last()
            .is_some_and(|last| last.refused() && last.outcome == call.outcome);
        if same || refusals >= MAX_REFUSAL_ENTRIES {
            if let Some(last) = log.iter_mut().rev().find(|c| c.refused()) {
                if last.outcome == call.outcome {
                    last.repeats = last.repeats.saturating_add(1);
                } else if let DuckOutcome::Failure { failure } = call.outcome {
                    let count = last.others.entry(failure).or_default();
                    *count = count.saturating_add(1);
                }
                return;
            }
        }
    }
    log.push(call);
}

fn side_ok(side: &Option<String>) -> bool {
    matches!(side.as_deref(), None | Some("base" | "head"))
}

fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn text(value: &str, max: usize, what: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{what} is empty."));
    }
    if value.len() > max {
        return Err(format!("{what} over {max} bytes. Shorten."));
    }
    Ok(())
}

fn range(line: Option<u64>, end_line: Option<u64>) -> Result<(), String> {
    match (line, end_line) {
        (Some(0), _) => Err("line must be >= 1.".into()),
        (Some(line), Some(end)) if end < line => Err("end_line must be >= line.".into()),
        (None, Some(_)) => Err("end_line needs line.".into()),
        _ => Ok(()),
    }
}

pub(crate) fn evidence(refs: &[EvidenceRef]) -> Result<(), String> {
    if refs.len() > MAX_EVIDENCE {
        return Err(format!("At most {MAX_EVIDENCE} evidence refs."));
    }
    for r in refs {
        text(&r.path, MAX_PATH_BYTES, "evidence path")?;
        range(r.line, r.end_line)?;
        if !side_ok(&r.side) {
            return Err("evidence side must be base or head.".into());
        }
    }
    Ok(())
}

impl DuckInput {
    /// Parse and bound the primary's arguments. `changed` and `threads` are this packet's
    /// changed paths and thread ids: a target must name one of them.
    pub fn parse(args: &Value, changed: &[String], threads: &[String]) -> Result<Self, String> {
        let input: Self = serde_json::from_value(args.clone()).map_err(|_| {
            "Malformed rubber_duck arguments; match the schema exactly.".to_string()
        })?;
        if input.verify_fixed && input.kind != TargetKind::Thread {
            return Err("verify_fixed requires kind thread and its stable thread_id.".into());
        }
        if input.claim.trim().is_empty()
            || words(&input.claim) > MAX_CLAIM_WORDS
            || input.claim.len() > MAX_CLAIM_BYTES
        {
            return Err(format!(
                "claim is empty or over {MAX_CLAIM_WORDS} words or {MAX_CLAIM_BYTES} bytes. Caveman."
            ));
        }
        text(&input.proposed_body, MAX_COMMENT_BYTES, "proposed_body")?;
        evidence(&input.evidence)?;
        range(input.line, input.end_line)?;
        if !side_ok(&input.side) {
            return Err("side must be base or head.".into());
        }
        match input.kind {
            TargetKind::Line => {
                let path = input.path.as_deref().ok_or("kind line needs path.")?;
                text(path, MAX_PATH_BYTES, "path")?;
                if input.line.is_none() {
                    return Err("kind line needs line.".into());
                }
                if !changed.iter().any(|p| p == path) {
                    return Err("path is not a changed file. Use a Files path.".into());
                }
            }
            TargetKind::Thread | TargetKind::Pr => {
                if input.path.is_some()
                    || input.line.is_some()
                    || input.end_line.is_some()
                    || input.side.is_some()
                {
                    return Err("path, line, end_line and side only with kind line.".into());
                }
            }
        }
        match (input.kind, input.thread_id.as_deref()) {
            (TargetKind::Thread, Some(id)) if threads.iter().any(|t| t == id) => {}
            (TargetKind::Thread, Some(_)) => {
                return Err("Unknown thread_id. Copy it from the Conversation section.".into())
            }
            (TargetKind::Thread, None) => return Err("kind thread needs thread_id.".into()),
            (_, Some(_)) => return Err("thread_id only with kind thread.".into()),
            (_, None) => {}
        }
        Ok(input)
    }

    /// The claim as the duck packet shows it, one fact per line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "kind: {}\n",
            match self.kind {
                TargetKind::Line => "line",
                TargetKind::Thread => "thread",
                TargetKind::Pr => "pr",
            }
        ));
        if let Some(path) = &self.path {
            let line = match (self.line, self.end_line) {
                (Some(l), Some(e)) => format!(":{l}-{e}"),
                (Some(l), None) => format!(":{l}"),
                _ => String::new(),
            };
            out.push_str(&format!(
                "at: {path}{line} ({})\n",
                self.side.as_deref().unwrap_or("head")
            ));
        }
        if let Some(thread) = &self.thread_id {
            out.push_str(&format!("thread: {thread}\n"));
        }
        if self.verify_fixed {
            out.push_str("verify_fixed: Independently check this existing finding against current head code. confirmed means the original defect is fixed; rejected means still applies; unverifiable means insufficient evidence. Outdated is not proof. Read the original thread and cite current-head code yourself.\n");
        }
        out.push_str(&format!("claim: {}\n", self.claim));
        out.push_str(&format!("proposed_body:\n{}\n", self.proposed_body));
        for r in &self.evidence {
            out.push_str(&format!("evidence: {}\n", reference(r)));
        }
        out.trim_end().to_string()
    }
}

fn reference(r: &EvidenceRef) -> String {
    let line = match (r.line, r.end_line) {
        (Some(l), Some(e)) => format!(":{l}-{e}"),
        (Some(l), None) => format!(":{l}"),
        _ => String::new(),
    };
    format!("{}{line} ({})", r.path, r.side.as_deref().unwrap_or("head"))
}

impl Verdict {
    pub fn parse(args: &Value) -> Result<Self, String> {
        let verdict: Self = serde_json::from_value(args.clone()).map_err(|_| {
            "Malformed submit_verdict arguments; match the schema exactly.".to_string()
        })?;
        if verdict.nano.trim().is_empty()
            || words(&verdict.nano) > MAX_NANO_WORDS
            || verdict.nano.len() > MAX_VERDICT_NANO_BYTES
        {
            return Err(format!(
                "nano is empty or over {MAX_NANO_WORDS} words. Shorten, do not cut mid-thought."
            ));
        }
        if let Some(full) = &verdict.full {
            text(full, MAX_FULL_BYTES, "full")?;
        }
        evidence(&verdict.evidence)?;
        match (verdict.verdict, &verdict.revised_body) {
            (VerdictKind::Revise, None) => {
                return Err("revise needs revised_body.".into());
            }
            (VerdictKind::Revise, Some(body)) => text(body, MAX_COMMENT_BYTES, "revised_body")?,
            (_, Some(_)) => return Err("revised_body only with revise.".into()),
            _ => {}
        }
        if let Some(anchor) = &verdict.revised_line {
            if verdict.verdict != VerdictKind::Revise {
                return Err("revised_line only with revise.".into());
            }
            text(&anchor.path, MAX_PATH_BYTES, "revised_line path")?;
            range(Some(anchor.line), anchor.end_line)?;
            if !side_ok(&anchor.side) {
                return Err("revised_line side must be base or head.".into());
            }
        }
        Ok(verdict)
    }
}

pub(crate) fn reference_schema() -> Value {
    json!({"type":"object","additionalProperties":false,
        "properties":{"path":{"type":"string","maxLength":1024},"side":{"type":"string","enum":["base","head"]},
            "line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},
        "required":["path"]})
}

/// `rubber_duck` parameters.
pub fn input_schema() -> Value {
    json!({"type":"object","additionalProperties":false,
        "properties":{
            "verify_fixed":{"type":"boolean","description":"kind thread only. Independently verify an existing finding is fixed on current head; confirmed means fixed, not a real defect."},
            "kind":{"type":"string","enum":["line","thread","pr"],"description":"line: inline comment; thread: reply; pr: review body point."},
            "path":{"type":"string","maxLength":1024,"description":"kind line only: a changed file."},
            "line":{"type":"integer","minimum":1},
            "end_line":{"type":"integer","minimum":1},
            "side":{"type":"string","enum":["base","head"]},
            "thread_id":{"type":"string","maxLength":1024,"description":"kind thread only: a thread id."},
            "claim":{"type":"string","maxLength":1024,"description":"The defect you assert. At most 100 words and 1024 bytes, caveman."},
            "proposed_body":{"type":"string","maxLength":16384,"description":"The comment as you would post it. At most 16384 bytes."},
            "evidence":{"type":"array","items":reference_schema(),"description":"At most 10 refs you rely on."}},
        "required":["kind","claim","proposed_body"]})
}

/// `submit_verdict` parameters.
pub fn verdict_schema() -> Value {
    json!({"type":"object","additionalProperties":false,
        "properties":{
            "verdict":{"type":"string","enum":["confirmed","revise","rejected","unverifiable"]},
            "nano":{"type":"string","maxLength":1024,"description":"Why. At most 100 words and 1024 bytes, caveman."},
            "full":{"type":"string","maxLength":4096,"description":"At most 4096 bytes, caveman."},
            "evidence":{"type":"array","items":reference_schema(),"description":"At most 10 refs you read."},
            "revised_body":{"type":"string","maxLength":16384,"description":"revise only: better comment. At most 16384 bytes."},
            "revised_line":{"type":"object","additionalProperties":false,
                "properties":{"path":{"type":"string","maxLength":1024},"line":{"type":"integer","minimum":1},
                    "end_line":{"type":"integer","minimum":1},"side":{"type":"string","enum":["base","head"]}},
                "required":["path","line"]}},
        "required":["verdict","nano"]})
}

/// The tool result the primary sees: host-shaped JSON fenced as untrusted agent output.
pub fn result_text(call: &DuckCall, verdict: &Verdict) -> String {
    let body = json!({
        "call": call.ordinal,
        "verdict": verdict.verdict,
        "nano": verdict.nano,
        "full": verdict.full,
        "evidence": verdict.evidence,
        "revised_body": verdict.revised_body,
        "revised_line": verdict.revised_line,
        "duck": {"agent": call.agent, "model": call.model},
        "state_id": call.state_id,
    });
    crate::packet::sections::fence("rubber_duck", &body.to_string())
}

#[cfg(test)]
mod tests;
