//! Find/replace tokens that Agent and repository prompts may use.
//! Unknown `{{tokens}}` are left untouched; values are inserted verbatim and
//! never re-expanded.

use crate::github::review::ReviewContext;

/// Token names accepted in prompts. Keep in sync with the editor's tip list.
pub const TOKENS: &[&str] = &[
    "pr_title",
    "pr_number",
    "pr_author",
    "head_sha",
    "base_sha",
    "changed_files",
    "changed_file_count",
    "agent_name",
];

fn value(name: &str, agent_name: &str, context: &ReviewContext) -> Option<String> {
    let pull = &context.pull;
    Some(match name {
        "pr_title" => pull.title.clone(),
        "pr_number" => pull.number.to_string(),
        "pr_author" => pull
            .author
            .as_ref()
            .map_or_else(|| "unavailable".into(), |a| a.login.clone()),
        "head_sha" => pull.head_sha.clone(),
        "base_sha" => pull.base_sha.clone(),
        "changed_files" => context
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        "changed_file_count" => context.files.len().to_string(),
        "agent_name" => agent_name.to_string(),
        _ => return None,
    })
}

pub fn expand(text: &str, agent_name: &str, context: &ReviewContext) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match value(name, agent_name, context) {
                    Some(replacement) => out.push_str(&replacement),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{
        metadata::{Lifecycle, PullRequest},
        review::{ReviewContext, ReviewFile},
        Identity,
    };
    use std::collections::BTreeMap;

    fn context() -> ReviewContext {
        ReviewContext {
            pull: PullRequest {
                mentioned: false,
                id: "1".into(),
                number: 42,
                title: "Fix {{pr_number}}".into(),
                author: Some(Identity {
                    id: "9".into(),
                    login: "octo".into(),
                }),
                requested_reviewers: vec![],
                requested_teams: vec![],
                state: Lifecycle::Open,
                draft: false,
                head_sha: "abc".into(),
                base_sha: "def".into(),
                head_repository_id: None,
                base_repository_id: "r".into(),
                updated_at: String::new(),
                files: vec![],
            },
            base_revision: "def".into(),
            files: vec![
                ReviewFile {
                    path: "a.rs".into(),
                    previous_path: None,
                    status: "modified".into(),
                    patch: None,
                },
                ReviewFile {
                    path: "b.rs".into(),
                    previous_path: None,
                    status: "added".into(),
                    patch: None,
                },
            ],
            head: BTreeMap::new(),
            base: BTreeMap::new(),
        }
    }

    #[test]
    fn replaces_known_tokens_and_keeps_unknown_ones() {
        let out = expand(
            "{{ pr_title }} #{{pr_number}} by {{pr_author}}: {{changed_file_count}} files; {{nope}} {{unterminated",
            "Snake",
            &context(),
        );
        assert_eq!(
            out,
            "Fix {{pr_number}} #42 by octo: 2 files; {{nope}} {{unterminated"
        );
    }

    #[test]
    fn lists_files_and_agent_name() {
        assert_eq!(
            expand("{{agent_name}}:\n{{changed_files}}", "Snake", &context()),
            "Snake:\na.rs\nb.rs"
        );
    }
}
