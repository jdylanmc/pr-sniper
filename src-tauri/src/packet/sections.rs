//! Section renderers. Fixed order: Meta, Rules, Pull request, Conversation, Files,
//! Teammate reports, Directive, Voice, Exits. Data is fenced; nothing is cut without a marker.

use super::{rules, AuthorRelation, CommentFact, PacketInput, Role, ThreadFact, SCHEMA};
use crate::exits::{gates::Offer, Exit};
use std::fmt::Write;

/// Inline cap per comment; longer text is marked and pulled with `get_thread`.
pub const COMMENT_INLINE_BYTES: usize = 2 * 1024;
pub const DESCRIPTION_INLINE_BYTES: usize = 16 * 1024;
pub const MAX_INLINE_THREADS: usize = 40;
pub const MAX_INLINE_COMMENTS: usize = 40;

/// Defuse tag-like `<untrusted` or `</untrusted` so content cannot close or open a fence.
/// Everything else passes through unchanged.
pub fn defuse(text: &str) -> String {
    defuse_tag(text, "untrusted")
}

/// Defuse tag-like `<tag` or `</tag` (ASCII, case-insensitive).
fn defuse_tag(text: &str, tag: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut safe = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in lower.match_indices(tag) {
        let before = &lower[..at];
        let start = if before.ends_with("</") {
            at - 2
        } else if before.ends_with('<') {
            at - 1
        } else {
            continue;
        };
        if start < last {
            continue;
        }
        safe.push_str(&text[last..start]);
        safe.push_str("&lt;");
        last = start + 1;
    }
    safe.push_str(&text[last..]);
    safe
}

/// Wrap untrusted text in a fence.
pub fn fence(source: &str, text: &str) -> String {
    let safe = defuse(text);
    let source: String = source
        .chars()
        .map(|c| {
            if c == '"' || c == '<' || c == '>' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!("<untrusted source=\"{source}\">\n{safe}\n</untrusted>")
}

fn bounded(source: &str, id: &str, text: &str, max: usize) -> String {
    if text.len() <= max {
        return fence(source, text);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[{} more bytes not shown; read with get_thread id={}]",
        fence(source, &text[..end]),
        text.len() - end,
        defuse(id)
    )
}

pub fn render(input: &PacketInput, packet_id: &str, with_time: bool) -> String {
    let mut out = String::new();
    meta(&mut out, input, packet_id, with_time);
    out.push_str("\n## Rules\n");
    let can_read_logs = input.tools.iter().any(|t| t == "read_check_log");
    let can_duck = input.tools.iter().any(|t| t == crate::review::duck::TOOL);
    for (index, rule) in rules::rules(
        input.role,
        !input.teammates.is_empty(),
        can_read_logs,
        can_duck,
    )
    .iter()
    .enumerate()
    {
        let _ = writeln!(out, "{}. {rule}", index + 1);
    }
    pull_request(&mut out, input);
    conversation(&mut out, input);
    files(&mut out, input);
    teammates(&mut out, input);
    directive(&mut out, input);
    voice(&mut out, input);
    exits(&mut out, input);
    out
}

fn meta(out: &mut String, input: &PacketInput, packet_id: &str, with_time: bool) {
    out.push_str("# PR Sniper review packet\n\n## Meta\n");
    if with_time {
        let _ = writeln!(out, "packet_id: {packet_id}");
    }
    let _ = writeln!(out, "schema: {SCHEMA}");
    let _ = writeln!(out, "state_id: {}", input.state_id);
    let _ = writeln!(out, "revision: {}", input.revision);
    if with_time {
        let _ = writeln!(out, "created_at: {}", input.created_at);
    }
    if let Some(rule) = &input.admitted_by {
        let _ = writeln!(out, "admitted_by: {}", rule.replace('\n', " "));
    }
    let _ = writeln!(out, "acting_identity: {}", input.acting_login);
    if input.role == Role::Primary {
        let _ = writeln!(out, "include_self: {}", input.include_self);
    }
    let _ = writeln!(
        out,
        "role: {}",
        match input.role {
            Role::Primary => "primary",
            Role::Teammate => "teammate",
            Role::Duck => "rubber_duck",
        }
    );
    let _ = writeln!(out, "{}", rules::INTRO);
    let _ = writeln!(out, "tools: {}", input.tools.join(", "));
}

pub(super) fn pull_request(out: &mut String, input: &PacketInput) {
    let pr = &input.pr;
    out.push_str("\n## Pull request\n");
    out.push_str(&fence("pull_request.title", &pr.title));
    out.push('\n');
    let relation = match pr.author_relation {
        AuthorRelation::Owner => "owner",
        AuthorRelation::Bot => "bot",
        AuthorRelation::Other => "other",
    };
    let _ = writeln!(out, "author: {} ({relation})", defuse(&pr.author));
    let _ = writeln!(out, "state: {}", defuse(&pr.state));
    let _ = writeln!(out, "draft: {}", pr.draft);
    let _ = writeln!(out, "base: {} @ {}", defuse(&pr.base_ref), pr.base_sha);
    let _ = writeln!(out, "head: {} @ {}", defuse(&pr.head_ref), pr.head_sha);
    let _ = writeln!(out, "revision: {}", input.revision);
    let _ = writeln!(out, "labels: {}", defuse(&pr.labels.join(", ")));
    if let Some((additions, deletions)) = pr.size {
        let _ = writeln!(out, "size: +{additions} -{deletions}");
    }
    let _ = writeln!(out, "mergeable: {}", pr.mergeable);
    let _ = writeln!(
        out,
        "review_decision: {}",
        pr.review_decision.as_deref().unwrap_or("none")
    );
    let _ = writeln!(out, "approvals: {}", defuse(&pr.approvals.join(", ")));
    let can_read_logs = input.tools.iter().any(|t| t == "read_check_log");
    let _ = writeln!(out, "checks ({}):", pr.checks.len());
    for check in &pr.checks {
        let log = match (&check.id, can_read_logs && check.log) {
            (Some(id), true) => format!(" (log: read_check_log id={})", defuse(id)),
            _ => String::new(),
        };
        let _ = writeln!(
            out,
            "- {}: {} / {}{log}",
            defuse(&check.name),
            defuse(&check.status),
            defuse(&check.conclusion)
        );
    }
    match &pr.description {
        Some(text) if text.trim().is_empty() => out.push_str("description: empty\n"),
        Some(text) => {
            out.push_str("description:\n");
            out.push_str(&bounded(
                "pull_request.description",
                "description",
                text,
                DESCRIPTION_INLINE_BYTES,
            ));
            out.push('\n');
        }
        None => out.push_str("description: not available to the host for this packet\n"),
    }
}

fn thread_text(thread: &ThreadFact) -> String {
    let mut out = String::new();
    let place = match (&thread.path, thread.line) {
        (Some(path), Some(line)) => format!("{}:{line}", defuse(path)),
        (Some(path), None) => defuse(path),
        _ => "-".into(),
    };
    let _ = writeln!(
        out,
        "thread {} at {place} resolved={} outdated={} mine={} new_since_last_review={} targeted_revision={}",
        defuse(&thread.id),
        thread.resolved,
        thread.outdated,
        thread.mine,
        thread.new_since_last_review,
        thread.targeted_revision.as_deref().unwrap_or("-"),
    );
    for comment in &thread.comments {
        out.push_str(&comment_text(
            &format!("thread:{}", thread.id),
            &thread.id,
            comment,
        ));
    }
    out
}

fn comment_text(scope: &str, id: &str, comment: &CommentFact) -> String {
    format!(
        "{} by {}:\n{}\n",
        defuse(&comment.id),
        defuse(&comment.author),
        bounded(
            &format!("{scope}:{}", comment.id),
            id,
            &comment.body,
            COMMENT_INLINE_BYTES
        )
    )
}

fn conversation(out: &mut String, input: &PacketInput) {
    let c = &input.conversation;
    out.push_str("\n## Conversation\n");
    let _ = writeln!(out, "threads: {}", c.threads.len());
    for thread in c.threads.iter().take(MAX_INLINE_THREADS) {
        out.push_str(&thread_text(thread));
    }
    if c.threads.len() > MAX_INLINE_THREADS {
        let _ = writeln!(
            out,
            "[{} more threads not shown; list them with get_thread (no id)]",
            c.threads.len() - MAX_INLINE_THREADS
        );
    }
    let _ = writeln!(out, "comments: {}", c.comments.len());
    for comment in c.comments.iter().take(MAX_INLINE_COMMENTS) {
        out.push_str(&comment_text("comment", &comment.id, comment));
    }
    if c.comments.len() > MAX_INLINE_COMMENTS {
        let _ = writeln!(
            out,
            "[{} more comments not shown; list them with get_thread (no id)]",
            c.comments.len() - MAX_INLINE_COMMENTS
        );
    }
}

pub(super) fn files(out: &mut String, input: &PacketInput) {
    out.push_str("\n## Files\n");
    let _ = writeln!(out, "changed_files: {}", input.files.len());
    out.push_str("Read with read_changes (patch, before, after) and read_source. Paged, up to 32000 bytes per chunk. The host records what you read; reading is never enforced.\n");
    for file in &input.files {
        let parts: Vec<String> = file
            .parts
            .iter()
            .map(|(part, bytes)| match bytes {
                Some(bytes) => format!("{part}={bytes}"),
                None => format!("{part}=unknown"),
            })
            .collect();
        let renamed = file
            .previous_path
            .as_ref()
            .map(|p| format!(" (from {})", defuse(p)))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "- {}{renamed} [{}] kind={} parts: {}",
            defuse(&file.path),
            defuse(&file.status),
            file.kind,
            parts.join(" ")
        );
    }
    if let Some(delta) = &input.delta {
        let _ = writeln!(out, "delta since last consumed state ({}):", delta.len());
        for path in delta {
            let _ = writeln!(out, "- {}", defuse(path));
        }
    }
}

/// Operator-configured labels are not fenced, but never carry tag characters.
fn label(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '"' | '<' | '>') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn teammates(out: &mut String, input: &PacketInput) {
    if input.role != Role::Primary || input.teammates.is_empty() {
        return;
    }
    out.push_str("\n## Teammate reports\n");
    out.push_str(rules::TEAMMATE_REPORTS_RULE);
    out.push('\n');
    for teammate in &input.teammates {
        let _ = writeln!(
            out,
            "\n### {} (lens: {})",
            label(&teammate.agent),
            teammate
                .lens
                .iter()
                .map(|title| label(title))
                .collect::<Vec<_>>()
                .join("; ")
        );
        let mut body = String::new();
        if teammate.report.findings.is_empty() {
            body.push_str("no findings");
        }
        for (index, f) in teammate.report.findings.iter().enumerate() {
            let _ = write!(
                body,
                "{}. {}:{}\nnano: {}\nfull: {}\nfix: {}\n",
                index + 1,
                f.file,
                f.line,
                f.nano,
                f.full,
                f.fix
            );
        }
        out.push_str(&fence(
            &format!("teammate:{}", teammate.agent),
            body.trim_end(),
        ));
        out.push('\n');
    }
}

fn directive(out: &mut String, input: &PacketInput) {
    let d = &input.directive;
    out.push_str("\n## Directive\n");
    for (title, body) in &d.doctrines {
        let _ = writeln!(out, "### Doctrine: {title}\n{body}");
    }
    if !d.agent_prompt.trim().is_empty() {
        let _ = writeln!(out, "agent_prompt:\n{}", d.agent_prompt);
    }
    if !d.repository_prompt.trim().is_empty() {
        let _ = writeln!(out, "repository_prompt:\n{}", d.repository_prompt);
    }
    if let Some(preset) = &d.preset {
        let _ = writeln!(out, "preset:\n{preset}");
    }
    let _ = writeln!(out, "signature: {}", d.signature);
}

/// Operator-configured style for human-facing text. Primary only; the host's
/// rules, gates and exits stay in force whatever the body says.
fn voice(out: &mut String, input: &PacketInput) {
    let Some(voice) = input.directive.voice.as_ref() else {
        return;
    };
    if input.role != Role::Primary {
        return;
    }
    out.push_str("\n## Voice\n");
    out.push_str(rules::VOICE_RULE);
    out.push('\n');
    let _ = writeln!(
        out,
        "<voice name=\"{}\">\n{}\n</voice>",
        label(&voice.title),
        defuse(&defuse_tag(&voice.body, "voice"))
    );
    if voice.guardrails {
        out.push_str(rules::VOICE_GUARD);
        out.push('\n');
    }
}

fn exits(out: &mut String, input: &PacketInput) {
    out.push_str("\n## Exits\n");
    out.push_str("Call exactly one. Host re-checks gates when you call it.\n");
    if input.role == Role::Primary {
        if let Some(preferred) = [
            Exit::ApproveAndMerge,
            Exit::Wait,
            Exit::ApprovePullRequest,
            Exit::ReadyForHuman,
        ]
        .into_iter()
        .find(|exit| {
            input
                .exits
                .iter()
                .any(|(candidate, offer)| candidate == exit && offer.is_offered())
        }) {
            let _ = writeln!(out, "clean_review_exit: {}", preferred.id());
        }
        if input.include_self && input.pr.author_relation == AuthorRelation::Owner {
            out.push_str("Include self is enabled for this repository assignment. If approve_and_merge is offered and review is clear, use it: the host merges without an approval vote. Do not hand off solely because approve_pull_request is unavailable. If merge is not offered, state its actual gate reason.\n");
        }
    }
    for (exit, offer) in &input.exits {
        match offer {
            Offer::Offered => {
                let _ = writeln!(out, "- {}: offered. {}", exit.id(), exit.description());
            }
            Offer::Denied(reason) => {
                let _ = writeln!(out, "- {}: not offered. {reason}", exit.id());
            }
        }
    }
}
