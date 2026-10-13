//! Tool registry: one table of `(name, description, schema)`, host handlers elsewhere.
//! Add a tool = add an entry here and an arm in `ReadTools::execute_inner`.

use super::runtime::{chunk, window, MAX_TOOL_BYTES};
use super::Failure;
use crate::packet::ToolFacts;
use serde_json::{json, Value};

/// Investigation tools a Packet-mode assignment may be granted, in registry order.
pub const INVESTIGATION: [&str; 6] = [
    "read_changes",
    "read_source",
    "get_thread",
    "get_checks",
    "list_tree",
    "search_code",
];

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub properties: Value,
    pub required: Vec<&'static str>,
}

impl ToolDef {
    pub fn schema(&self) -> Value {
        json!({"type":"object","properties":self.properties,"required":self.required,"additionalProperties":false})
    }
}

pub fn definition(name: &str) -> Option<ToolDef> {
    let (name, description, properties, required): (&'static str, &'static str, Value, Vec<&'static str>) = match name {
        "read_changes" => ("read_changes", "Read one part (patch, before or after) of one changed file in byte chunks. Responses carry content, offset, next_offset, total_bytes and complete. Page with offset=next_offset until complete=true for any part you read.", json!({"path":{"type":"string"},"part":{"type":"string","enum":["patch","before","after"]},"offset":{"type":"integer","minimum":0},"max_bytes":{"type":"integer","minimum":1}}), vec!["path","part"]),
        "read_source" => ("read_source", "Read one source file by exact path from immutable base or head in byte chunks (content, offset, next_offset, total_bytes, complete); repeat with offset=next_offset until complete=true. Symlinks are read as text, never followed.", json!({"side":{"type":"string","enum":["base","head"]},"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"max_bytes":{"type":"integer","minimum":1}}), vec!["side","path"]),
        "get_thread" => ("get_thread", "Read a review thread, a top-level comment, or the PR description in full. thread_id is a thread id, a comment id or `description`; omit it to list ids (paged: threads, offset, next_offset, total, complete). Text is paged like read_source.", json!({"thread_id":{"type":"string"},"offset":{"type":"integer","minimum":0},"max_bytes":{"type":"integer","minimum":1}}), vec![]),
        "get_checks" => ("get_checks", "List the head's CI checks (name, status, conclusion), paged (checks, offset, next_offset, total, complete). No logs are available.", json!({"offset":{"type":"integer","minimum":0}}), vec![]),
        "list_tree" => ("list_tree", "List repository paths under a prefix from immutable base or head (default head), sorted and paged (paths, offset, next_offset, total, complete).", json!({"side":{"type":"string","enum":["base","head"]},"prefix":{"type":"string"},"offset":{"type":"integer","minimum":0}}), vec![]),
        "search_code" => ("search_code", "Find a literal, case-sensitive substring in the head content of changed files. Matches carry path, line and a snippet of at most 200 bytes (truncated=true when cut); paged (matches, offset, next_offset, total, complete).", json!({"query":{"type":"string"},"offset":{"type":"integer","minimum":0}}), vec!["query"]),
        _ => return None,
    };
    Some(ToolDef {
        name,
        description,
        properties,
        required,
    })
}

/// A run's tool ids: granted investigation tools (registry order), then offered exits.
pub fn tool_set(granted: &[String], offered: &[crate::exits::Exit]) -> Vec<String> {
    INVESTIGATION
        .iter()
        .filter(|name| granted.iter().any(|g| g == *name))
        .map(|name| name.to_string())
        .chain(offered.iter().map(|exit| exit.id().to_string()))
        .collect()
}

fn start_offset(args: &Value) -> Result<usize, Failure> {
    window(args).map(|(offset, _)| offset)
}

/// Page a list so the serialized response fits the limit. Never cut: `next_offset` continues.
pub fn page_items(
    key: &str,
    items: &[Value],
    args: &Value,
    rejected_by_limit: &mut bool,
) -> Result<Value, Failure> {
    let start = start_offset(args)?;
    if start > items.len() {
        return Err(Failure::permanent(
            "Offset must be a previous next_offset within the results.",
        ));
    }
    let mut end = start;
    let mut size = 200;
    while end < items.len() {
        let item = items[end].to_string().len() + 1;
        if size + item > MAX_TOOL_BYTES && end > start {
            break;
        }
        size += item;
        end += 1;
    }
    let output = json!({
        key: &items[start..end],
        "offset": start,
        "next_offset": (end < items.len()).then_some(end),
        "total": items.len(),
        "complete": end == items.len(),
    });
    if output.to_string().len() > MAX_TOOL_BYTES {
        *rejected_by_limit = true;
        return Err(Failure::permanent(
            "A result cannot fit the response limit; the review cannot claim complete context.",
        ));
    }
    Ok(output)
}

pub fn get_thread(
    facts: &ToolFacts,
    args: &Value,
    rejected_by_limit: &mut bool,
) -> Result<Value, Failure> {
    let Some(id) = args["thread_id"].as_str() else {
        let c = &facts.conversation;
        let items: Vec<Value> = c
            .threads
            .iter()
            .map(|t| json!({"id":t.id,"kind":"thread","resolved":t.resolved,"comments":t.comments.len(),"bytes":t.comments.iter().map(|c| c.body.len()).sum::<usize>()}))
            .chain(c.comments.iter().map(|c| json!({"id":c.id,"kind":"comment","author":c.author,"bytes":c.body.len()})))
            .collect();
        return page_items("threads", &items, args, rejected_by_limit);
    };
    let (kind, text) = if id == "description" {
        (
            "description",
            facts
                .description
                .clone()
                .ok_or_else(|| Failure::permanent("No description is available."))?,
        )
    } else if let Some(thread) = facts.conversation.threads.iter().find(|t| t.id == id) {
        (
            "thread",
            thread
                .comments
                .iter()
                .map(|c| format!("{} by {}:\n{}\n", c.id, c.author, c.body))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else if let Some(comment) = facts.conversation.comments.iter().find(|c| c.id == id) {
        (
            "comment",
            format!("{} by {}:\n{}\n", comment.id, comment.author, comment.body),
        )
    } else {
        return Err(Failure::permanent(
            "Unknown id. List ids with get_thread and no thread_id.",
        ));
    };
    let mut meta = serde_json::Map::new();
    meta.insert("id".into(), json!(id));
    meta.insert("kind".into(), json!(kind));
    chunk(&text, args, meta, rejected_by_limit).map(|(value, _, _)| value)
}

pub fn get_checks(
    facts: &ToolFacts,
    args: &Value,
    rejected_by_limit: &mut bool,
) -> Result<Value, Failure> {
    let items: Vec<Value> = facts
        .checks
        .iter()
        .map(|c| json!({"name":c.name,"status":c.status,"conclusion":c.conclusion}))
        .collect();
    page_items("checks", &items, args, rejected_by_limit)
}

pub const SNIPPET_BYTES: usize = 200;

/// A snippet of one line, cut on a char boundary; the flag says whether it was cut.
pub fn snippet(line: &str) -> (String, bool) {
    if line.len() <= SNIPPET_BYTES {
        return (line.to_string(), false);
    }
    let mut end = SNIPPET_BYTES;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    (line[..end].to_string(), true)
}

#[cfg(test)]
mod tests;
