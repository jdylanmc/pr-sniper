//! Rubber duck packet: one claim, the primary's PR state, the duck Agent's lens.
//! Deterministic like the review packet. No Conversation push, no Teammate reports,
//! no Voice: duck text is agent-to-agent caveman.

use super::{rules, sections, Directive, Packet, PacketInput, PacketRecord, Role, SCHEMA};
use crate::exits::Exit;
use crate::review::duck::DuckInput;
use sha2::{Digest, Sha256};
use std::fmt::Write;

pub struct DuckPacketInput<'a> {
    /// The primary packet's inputs: same PR facts and files.
    pub base: &'a PacketInput,
    pub primary_hash: &'a str,
    pub ordinal: u32,
    /// verify_fixed: the whole original thread, served inline when small enough.
    pub thread: Option<&'a str>,
    pub claim: &'a DuckInput,
    /// The duck Agent's Doctrines and prompts; preset, signature and Voice are ignored.
    pub directive: &'a Directive,
    pub tools: &'a [String],
    pub created_at: &'a str,
}

pub fn build(input: &DuckPacketInput<'_>) -> Packet {
    let stable = render(input, "", false);
    let hash: String = Sha256::digest(stable.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let text = render(input, &hash[..16], true);
    Packet {
        text,
        hash: hash.clone(),
        record: PacketRecord {
            state_id: input.base.state_id.clone(),
            hash,
            schema: SCHEMA,
            tool_set: input.tools.to_vec(),
            exits_offered: vec![Exit::SubmitVerdict],
            exits_denied: vec![],
            state_hash: None,
            state_marks: None,
            wait_snapshot: None,
        },
    }
}

fn render(input: &DuckPacketInput<'_>, packet_id: &str, with_time: bool) -> String {
    let mut out = String::from("# PR Sniper rubber duck packet\n\n## Meta\n");
    if with_time {
        let _ = writeln!(out, "packet_id: {packet_id}");
    }
    let _ = writeln!(out, "schema: {SCHEMA}");
    let _ = writeln!(out, "state_id: {}", input.base.state_id);
    let _ = writeln!(out, "primary_packet: {}", input.primary_hash);
    let _ = writeln!(out, "call: {}", input.ordinal);
    if with_time {
        let _ = writeln!(out, "created_at: {}", input.created_at);
    }
    let _ = writeln!(out, "role: rubber_duck");
    let _ = writeln!(out, "{}", rules::DUCK_INTRO);
    let _ = writeln!(out, "tools: {}", input.tools.join(", "));
    out.push_str("\n## Rules\n");
    for (index, rule) in rules::rules(Role::Duck, false, false, false)
        .iter()
        .enumerate()
    {
        let _ = writeln!(out, "{}. {rule}", index + 1);
    }
    sections::pull_request(&mut out, input.base);
    sections::files(&mut out, input.base, Role::Duck);
    out.push_str("\n## Claim\n");
    out.push_str(&sections::fence("primary", &input.claim.render()));
    out.push('\n');
    if input.claim.verify_fixed {
        out.push_str("\n## Original thread\n");
        match input.thread {
            Some(thread) => {
                out.push_str("complete: true. Served whole; counts as read.\n");
                out.push_str(&sections::fence("thread", thread));
                out.push('\n');
            }
            None => out.push_str(
                "complete: false. Too large to inline: get_thread every page before confirmed.\n",
            ),
        }
    }
    let d = input.directive;
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
    out.push_str("\n## Exits\n");
    let _ = writeln!(
        out,
        "- {}: offered. {}",
        Exit::SubmitVerdict.id(),
        Exit::SubmitVerdict.description()
    );
    out
}
