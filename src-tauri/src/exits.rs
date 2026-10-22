//! Exits end a review run. The agent calls exactly one; the host is the only writer.
//! Payloads are validated here, gates in [`gates`]; effects live in their own pipelines.

pub mod consume;
pub mod gates;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_COMMENTS: usize = 100;
pub const MAX_REPLIES: usize = 100;
pub const MAX_RESOLUTIONS: usize = 100;
pub const MAX_COMMENT_BYTES: usize = 16 * 1024;
/// PR-level review body of `leave_comments`.
pub const MAX_REVIEW_BODY_BYTES: usize = 8 * 1024;
pub const MAX_SUMMARY_BYTES: usize = 4 * 1024;
pub const MAX_PANIC_REASON_BYTES: usize = 200;
pub const MAX_PANIC_DETAIL_BYTES: usize = 4 * 1024;
pub const MAX_FINDINGS: usize = 25;
pub const MAX_NANO_WORDS: usize = 100;
pub const MAX_FULL_BYTES: usize = 4 * 1024;
pub const MAX_FIX_BYTES: usize = 4 * 1024;
pub const MAX_PATH_BYTES: usize = 1024;
/// Rejected exit calls (malformed or over limit) tolerated before a contract violation.
pub const MAX_REJECTED_CALLS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    LeaveComments,
    ReadyForHuman,
    Wait,
    ApprovePullRequest,
    ApproveAndMerge,
    PanicExit,
    SubmitReport,
    /// The Rubber Duck's only exit. Never offered to a review assignment.
    SubmitVerdict,
    /// The primary's sign-off: resolve its own open threads, then act on the
    /// assignment's authority. Offered to the primary only, never a configured tool.
    Lgtm,
}

impl Exit {
    pub const ALL: [Exit; 9] = [
        Exit::LeaveComments,
        Exit::ReadyForHuman,
        Exit::Wait,
        Exit::ApprovePullRequest,
        Exit::ApproveAndMerge,
        Exit::PanicExit,
        Exit::SubmitReport,
        Exit::SubmitVerdict,
        Exit::Lgtm,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Exit::LeaveComments => "leave_comments",
            Exit::ReadyForHuman => "ready_for_human",
            Exit::Wait => "wait",
            Exit::ApprovePullRequest => "approve_pull_request",
            Exit::ApproveAndMerge => "approve_and_merge",
            Exit::PanicExit => "panic_exit",
            Exit::SubmitReport => "submit_report",
            Exit::SubmitVerdict => "submit_verdict",
            Exit::Lgtm => "lgtm",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|exit| exit.id() == id)
    }

    /// Exits that write to the provider (never offered to teammates).
    pub fn effect(self) -> bool {
        matches!(
            self,
            Exit::LeaveComments | Exit::ApprovePullRequest | Exit::ApproveAndMerge | Exit::Lgtm
        )
    }

    pub fn description(self) -> &'static str {
        match self {
            Exit::LeaveComments => "Terminal exit: post new inline comments on diff lines, a PR-level review body (body), reply to threads, resolve your own threads. Ends the run. Call once.",
            Exit::ReadyForHuman => "Terminal exit: nothing to post; hand the PR to the human with a short local summary. Ends the run.",
            Exit::Wait => "Terminal exit: yield for a host-observed external prerequisite to a permitted action. The host polls on the repository schedule and queues a fresh review when it changes. Not for defects, uncertainty or tool failures.",
            Exit::ApprovePullRequest => "Terminal exit: approve the PR head with an approval comment. Ends the run.",
            Exit::ApproveAndMerge => "Terminal exit: merge the reviewed PR when offered. For another author's PR, the host approves first. For the acting account's own PR with Include self enabled, the host merges without an approval vote. Ends the run.",
            Exit::PanicExit => "Terminal exit: unrecoverable problem. State reason and detail. Ends the run.",
            Exit::SubmitReport => "Terminal exit: submit your findings report to the primary reviewer. Caveman. Ends the run.",
            Exit::SubmitVerdict => crate::review::duck::VERDICT_DESCRIPTION,
            Exit::Lgtm => "Terminal exit: sign off. No new findings and nothing pending. The host posts comment as a reply on each of your own open threads and resolves them, then acts on the assignment's authority after fresh gates: hand to the human, approve, or approve and merge. Refused while others' threads are open, CI is not green, or the PR is draft or conflicting. Ends the run.",
        }
    }

    pub fn schema(self) -> Value {
        let comment = json!({"type":"object","additionalProperties":false,
            "properties":{"path":{"type":"string"},"line":{"type":"integer","minimum":1},
                "finding_id":{"type":["string","null"],"description":"null for a distinct new finding; existing thread id updates that finding in its original thread, including mistaken resolutions."},
                "side":{"type":"string","enum":["base","head"]},"body":{"type":"string"}},
            "required":["path","line","body","finding_id"]});
        let reply = json!({"type":"object","additionalProperties":false,
            "properties":{"thread_id":{"type":"string"},"body":{"type":"string"}},
            "required":["thread_id","body"]});
        let finding = json!({"type":"object","additionalProperties":false,
            "properties":{"file":{"type":"string"},"line":{"type":"integer","minimum":1},
                "nano":{"type":"string","description":"At most 100 words, caveman."},
                "full":{"type":"string","description":"At most 4096 bytes."},
                "fix":{"type":"string","description":"At most 4096 bytes."}},
            "required":["file","line","nano","full","fix"]});
        let (mut properties, required) = match self {
            Exit::LeaveComments => (
                json!({"new_comments":{"type":"array","items":comment},
                    "body":{"type":"string","description":"PR-level review body. At most 8192 bytes. For points not on a diff line."},
                    "reply_comments":{"type":"array","items":reply},
                    "resolve_own_threads":{"type":"array","items":{"type":"string"}}}),
                json!([]),
            ),
            Exit::ReadyForHuman => (json!({"summary":{"type":"string"}}), json!(["summary"])),
            Exit::Wait => (
                json!({"reason":{"type":"string","enum":["ci","draft","required_reviews"]}}),
                json!(["reason"]),
            ),
            Exit::ApprovePullRequest | Exit::ApproveAndMerge => {
                (json!({"comment":{"type":"string"}}), json!(["comment"]))
            }
            Exit::PanicExit => (
                json!({"reason":{"type":"string"},"detail":{"type":"string"}}),
                json!(["reason", "detail"]),
            ),
            Exit::SubmitReport => (
                json!({"findings":{"type":"array","items":finding}}),
                json!(["findings"]),
            ),
            Exit::SubmitVerdict => return crate::review::duck::verdict_schema(),
            Exit::Lgtm => (
                json!({"comment":{"type":"string","description":"Short sign-off rationale, posted on each own thread resolved. At most 4096 bytes."}}),
                json!(["comment"]),
            ),
        };
        if matches!(
            self,
            Exit::LeaveComments
                | Exit::ReadyForHuman
                | Exit::Wait
                | Exit::ApprovePullRequest
                | Exit::ApproveAndMerge
        ) {
            properties["finding_dispositions"] = crate::review::findings::assessment_schema();
        }
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewComment {
    pub path: String,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyComment {
    pub thread_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportFinding {
    pub file: String,
    pub line: u64,
    pub nano: String,
    pub full: String,
    pub fix: String,
}

/// A teammate's stored report. Agent output: always untrusted data downstream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeammateReport {
    pub findings: Vec<ReportFinding>,
}

/// A validated exit call as the agent made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitCall {
    Wait {
        reason: WaitReason,
    },
    LeaveComments {
        new_comments: Vec<NewComment>,
        /// PR-level review body, posted with the same review as `new_comments`.
        body: Option<String>,
        reply_comments: Vec<ReplyComment>,
        resolve_own_threads: Vec<String>,
    },
    ReadyForHuman {
        summary: String,
    },
    ApprovePullRequest {
        comment: String,
    },
    ApproveAndMerge {
        comment: String,
    },
    Panic {
        reason: String,
        detail: String,
    },
    SubmitReport {
        report: TeammateReport,
    },
    SubmitVerdict {
        verdict: crate::review::duck::Verdict,
    },
    /// The model supplies `comment`; the host fills the threads and the action.
    Lgtm {
        comment: String,
        resolve_own_threads: Vec<String>,
        action: LgtmAction,
    },
}

/// What a sign-off does after its thread resolutions: the assignment's authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LgtmAction {
    /// No approve authority: hand to the human.
    #[default]
    Ready,
    Approve,
    /// Approve (unless self-authored), then merge.
    Merge,
}

/// What a successful run stored. Panic never completes a run, so it has no outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "exit", rename_all = "snake_case")]
pub enum ExitOutcome {
    Wait {
        reason: WaitReason,
    },
    Comments {
        new_comments: Vec<NewComment>,
        /// PR-level review body. Absent in records written before it existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        reply_comments: Vec<ReplyComment>,
        resolve_own_threads: Vec<String>,
    },
    ReadyForHuman {
        summary: String,
    },
    Approve {
        comment: String,
    },
    ApproveAndMerge {
        comment: String,
    },
    SubmitReport,
    Lgtm {
        comment: String,
        resolve_own_threads: Vec<String>,
        action: LgtmAction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitReason {
    Ci,
    Draft,
    RequiredReviews,
}

impl ExitCall {
    pub fn exit(&self) -> Exit {
        match self {
            Self::Wait { .. } => Exit::Wait,
            Self::LeaveComments { .. } => Exit::LeaveComments,
            Self::ReadyForHuman { .. } => Exit::ReadyForHuman,
            Self::ApprovePullRequest { .. } => Exit::ApprovePullRequest,
            Self::ApproveAndMerge { .. } => Exit::ApproveAndMerge,
            Self::Panic { .. } => Exit::PanicExit,
            Self::SubmitReport { .. } => Exit::SubmitReport,
            Self::SubmitVerdict { .. } => Exit::SubmitVerdict,
            Self::Lgtm { .. } => Exit::Lgtm,
        }
    }

    /// Outcome and (for teammates) report to store on a completed run.
    pub fn outcome(&self) -> Option<(ExitOutcome, Option<TeammateReport>)> {
        Some(match self {
            Self::Wait { reason } => (ExitOutcome::Wait { reason: *reason }, None),
            Self::LeaveComments {
                new_comments,
                body,
                reply_comments,
                resolve_own_threads,
            } => (
                ExitOutcome::Comments {
                    new_comments: new_comments.clone(),
                    body: body.clone(),
                    reply_comments: reply_comments.clone(),
                    resolve_own_threads: resolve_own_threads.clone(),
                },
                None,
            ),
            Self::ReadyForHuman { summary } => (
                ExitOutcome::ReadyForHuman {
                    summary: summary.clone(),
                },
                None,
            ),
            Self::ApprovePullRequest { comment } => (
                ExitOutcome::Approve {
                    comment: comment.clone(),
                },
                None,
            ),
            Self::ApproveAndMerge { comment } => (
                ExitOutcome::ApproveAndMerge {
                    comment: comment.clone(),
                },
                None,
            ),
            Self::SubmitReport { report } => (ExitOutcome::SubmitReport, Some(report.clone())),
            Self::Lgtm {
                comment,
                resolve_own_threads,
                action,
            } => (
                ExitOutcome::Lgtm {
                    comment: comment.clone(),
                    resolve_own_threads: resolve_own_threads.clone(),
                    action: *action,
                },
                None,
            ),
            Self::Panic { .. } | Self::SubmitVerdict { .. } => return None,
        })
    }

    pub fn parse(name: &str, args: &Value) -> Result<Self, String> {
        let exit = Exit::parse(name).ok_or("Unknown exit.")?;
        let bad = |_| format!("Malformed {name} arguments; match the schema exactly.");
        match exit {
            Exit::Wait => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    reason: WaitReason,
                }
                let a: Args = serde_json::from_value(args.clone()).map_err(bad)?;
                Ok(Self::Wait { reason: a.reason })
            }
            Exit::LeaveComments => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    #[serde(default)]
                    new_comments: Vec<NewComment>,
                    #[serde(default)]
                    body: Option<String>,
                    #[serde(default)]
                    reply_comments: Vec<ReplyComment>,
                    #[serde(default)]
                    resolve_own_threads: Vec<String>,
                }
                let a: Args = serde_json::from_value(args.clone()).map_err(bad)?;
                if a.new_comments.is_empty()
                    && a.body.is_none()
                    && a.reply_comments.is_empty()
                    && a.resolve_own_threads.is_empty()
                {
                    return Err("Nothing to post. Call ready_for_human instead.".into());
                }
                if a.new_comments.len() > MAX_COMMENTS
                    || a.reply_comments.len() > MAX_REPLIES
                    || a.resolve_own_threads.len() > MAX_RESOLUTIONS
                {
                    return Err("Too many items. Keep the highest value ones and retry.".into());
                }
                for c in &a.new_comments {
                    check_text(&c.body, MAX_COMMENT_BYTES, "comment body")?;
                    check_text(&c.path, MAX_PATH_BYTES, "comment path")?;
                    if c.line == 0 || !matches!(c.side.as_deref(), None | Some("base" | "head")) {
                        return Err("Comment line must be >= 1 and side base or head.".into());
                    }
                }
                if let Some(body) = &a.body {
                    check_text(body, MAX_REVIEW_BODY_BYTES, "body")?;
                }
                for r in &a.reply_comments {
                    check_text(&r.body, MAX_COMMENT_BYTES, "reply body")?;
                    check_text(&r.thread_id, MAX_PATH_BYTES, "thread_id")?;
                }
                if a.resolve_own_threads.iter().any(|id| id.trim().is_empty()) {
                    return Err("resolve_own_threads ids must be non-empty.".into());
                }
                Ok(Self::LeaveComments {
                    new_comments: a.new_comments,
                    body: a.body,
                    reply_comments: a.reply_comments,
                    resolve_own_threads: a.resolve_own_threads,
                })
            }
            Exit::ReadyForHuman => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    summary: String,
                }
                let a: Args = serde_json::from_value(args.clone()).map_err(bad)?;
                check_text(&a.summary, MAX_SUMMARY_BYTES, "summary")?;
                Ok(Self::ReadyForHuman { summary: a.summary })
            }
            Exit::ApprovePullRequest | Exit::ApproveAndMerge => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    comment: String,
                }
                let a: Args = serde_json::from_value(args.clone()).map_err(bad)?;
                check_text(&a.comment, MAX_COMMENT_BYTES, "approval comment")?;
                Ok(if exit == Exit::ApprovePullRequest {
                    Self::ApprovePullRequest { comment: a.comment }
                } else {
                    Self::ApproveAndMerge { comment: a.comment }
                })
            }
            Exit::PanicExit => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    reason: String,
                    detail: String,
                }
                let a: Args = serde_json::from_value(args.clone()).map_err(bad)?;
                check_text(&a.reason, MAX_PANIC_REASON_BYTES, "reason")?;
                check_text(&a.detail, MAX_PANIC_DETAIL_BYTES, "detail")?;
                Ok(Self::Panic {
                    reason: a.reason,
                    detail: a.detail,
                })
            }
            Exit::SubmitReport => {
                let report: TeammateReport = serde_json::from_value(args.clone()).map_err(bad)?;
                if report.findings.len() > MAX_FINDINGS {
                    return Err(format!(
                        "At most {MAX_FINDINGS} findings. Keep the highest value ones."
                    ));
                }
                for (index, f) in report.findings.iter().enumerate() {
                    let n = index + 1;
                    if f.line == 0 {
                        return Err(format!("Finding {n}: line must be >= 1."));
                    }
                    check_text(&f.file, MAX_PATH_BYTES, "file")?;
                    if f.nano.trim().is_empty()
                        || f.nano.split_whitespace().count() > MAX_NANO_WORDS
                    {
                        return Err(format!(
                            "Finding {n}: nano is empty or over {MAX_NANO_WORDS} words. Shorten, do not cut mid-thought."
                        ));
                    }
                    if f.full.len() > MAX_FULL_BYTES || f.fix.len() > MAX_FIX_BYTES {
                        return Err(format!(
                            "Finding {n}: full and fix at most 4096 bytes each. Shorten."
                        ));
                    }
                }
                Ok(Self::SubmitReport { report })
            }
            Exit::SubmitVerdict => Ok(Self::SubmitVerdict {
                verdict: crate::review::duck::Verdict::parse(args)?,
            }),
            Exit::Lgtm => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    comment: String,
                }
                let a: Args = serde_json::from_value(args.clone()).map_err(bad)?;
                check_text(&a.comment, MAX_SUMMARY_BYTES, "comment")?;
                Ok(Self::Lgtm {
                    comment: a.comment,
                    resolve_own_threads: Vec::new(),
                    action: LgtmAction::Ready,
                })
            }
        }
    }
}

fn check_text(text: &str, max: usize, what: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{what} is empty."));
    }
    if text.len() > max {
        return Err(format!("{what} over {max} bytes. Shorten and retry."));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
