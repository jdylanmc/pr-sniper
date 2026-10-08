use super::Publication;
use crate::{
    github::review::ReviewContext,
    review::{Decision, Failure, Severity},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InlineComment {
    pub path: String,
    pub line: u64,
    pub side: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_position: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub commit_id: String,
    pub body: String,
    pub comments: Vec<InlineComment>,
    pub unmappable: Vec<usize>,
}

impl Batch {
    pub fn prepare(run: &Publication, context: &ReviewContext) -> Result<Self, Failure> {
        let result = run
            .review
            .result
            .as_ref()
            .ok_or_else(|| Failure::permanent("A complete local review is required."))?;
        if context.pull.head_sha != run.review.job.head_sha
            || result.reviewed_base_sha.as_deref() != Some(&context.pull.base_sha)
        {
            return Err(Failure::permanent(
                "The reviewed diff is stale. Run a new review.",
            ));
        }
        let paths: Vec<_> = context.files.iter().map(|f| f.path.clone()).collect();
        crate::review::validate_output(
            &serde_json::to_string(&result.output)
                .map_err(|_| Failure::permanent("Invalid saved review."))?,
            &paths,
        )?;
        let locations: BTreeMap<_, _> = context
            .files
            .iter()
            .map(|file| {
                (
                    file.path.as_str(),
                    file.patch
                        .as_deref()
                        .and_then(diff_positions)
                        .unwrap_or_default(),
                )
            })
            .collect();
        let mut comments = Vec::new();
        let mut unmappable = Vec::new();
        for (index, finding) in result.output.findings.iter().enumerate() {
            if !locations
                .get(finding.path.as_str())
                .is_some_and(|lines| lines.contains_key(&(finding.side.clone(), finding.line)))
            {
                unmappable.push(index);
                continue;
            }
            let severity = match finding.severity {
                Severity::Critical => "critical",
                Severity::High => "high",
                Severity::Medium => "medium",
                Severity::Low => "low",
            };
            let body = format!(
                "{}: {}\n\n{}\n\nConfidence: {}%.\n\n<!-- pr-sniper:finding:{}:{index} -->",
                severity, finding.title, finding.explanation, finding.confidence, run.id,
            );
            check_body(&body)?;
            comments.push(InlineComment {
                path: finding.path.clone(),
                line: finding.line,
                side: if finding.side == "base" {
                    "LEFT"
                } else {
                    "RIGHT"
                }
                .into(),
                body,
                diff_position: locations
                    .get(finding.path.as_str())
                    .and_then(|lines| lines.get(&(finding.side.clone(), finding.line)))
                    .copied(),
            });
        }
        let decision = match result.output.decision {
            Decision::MachineSignOff => {
                "Machine sign-off: no findings. This is not human approval."
            }
            Decision::HumanInputRequired => {
                "Human input required. Inspect the findings before completing your review."
            }
        };
        let body = format!(
            "Automated review by PR Sniper.\n\n{}\n\n{decision}\n\n{} inline finding(s). {} finding(s) could not be mapped and remain visible in the local Review Queue; this batch is not a complete clearance when findings are unmapped.\n\nAutomated review completed. Please perform final human review.\n\n{}\n\n\u{f05b} PR Sniper",
            result.output.synopsis, comments.len(), unmappable.len(), run.marker(),
        );
        check_body(&body)?;
        Ok(Self {
            commit_id: run.review.job.head_sha.clone(),
            body,
            comments,
            unmappable,
        })
    }
}

fn check_body(body: &str) -> Result<(), Failure> {
    if body.chars().count() > 65_536 {
        return Err(Failure::permanent(
            "Review comment exceeds GitHub's body limit; output was not truncated.",
        ));
    }
    Ok(())
}

fn range(value: &str, prefix: char) -> Option<(u64, u64)> {
    let value = value.strip_prefix(prefix)?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}

// Accept only complete, internally consistent unified hunks. Missing patches stay local.
pub(crate) fn diff_positions(patch: &str) -> Option<BTreeMap<(String, u64), u64>> {
    let mut lines = BTreeMap::new();
    let (mut old, mut new, mut old_left, mut new_left) = (0_u64, 0_u64, 0_u64, 0_u64);
    let mut hunk = false;
    let mut position = 0_u64;
    for line in patch.lines() {
        // GitHub counts every line after the first hunk header, including later headers.
        if hunk {
            position = position.checked_add(1)?;
        }
        if line.starts_with("@@ ") {
            if old_left != 0 || new_left != 0 {
                return None;
            }
            let mut parts = line.split_whitespace();
            if parts.next()? != "@@" {
                return None;
            }
            (old, old_left) = range(parts.next()?, '-')?;
            (new, new_left) = range(parts.next()?, '+')?;
            if parts.next()? != "@@" {
                return None;
            }
            hunk = true;
            continue;
        }
        if line == "\\ No newline at end of file" {
            continue;
        }
        if !hunk {
            return None;
        }
        match line.as_bytes().first()? {
            b' ' => {
                old_left = old_left.checked_sub(1)?;
                new_left = new_left.checked_sub(1)?;
                if old == 0 || new == 0 {
                    return None;
                }
                if lines.insert(("base".into(), old), position).is_some()
                    || lines.insert(("head".into(), new), position).is_some()
                {
                    return None;
                }
                old = old.checked_add(1)?;
                new = new.checked_add(1)?;
            }
            b'-' => {
                old_left = old_left.checked_sub(1)?;
                if old == 0 {
                    return None;
                }
                if lines.insert(("base".into(), old), position).is_some() {
                    return None;
                }
                old = old.checked_add(1)?;
            }
            b'+' => {
                new_left = new_left.checked_sub(1)?;
                if new == 0 {
                    return None;
                }
                if lines.insert(("head".into(), new), position).is_some() {
                    return None;
                }
                new = new.checked_add(1)?;
            }
            _ => return None,
        }
    }
    (hunk && old_left == 0 && new_left == 0).then_some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_sides_context_and_multiple_hunks_but_not_truncated_patches() {
        let lines = diff_positions("@@ -1,2 +1,2 @@\n same\n-old\n+new\n@@ -9 +9 @@ fn\n-a\n+b\n\\ No newline at end of file").unwrap();
        for side in ["base", "head"] {
            for n in [1, 2, 9] {
                assert!(lines.contains_key(&(side.into(), n)));
            }
        }
        assert_eq!(lines[&("base".into(), 2)], 2);
        assert_eq!(lines[&("head".into(), 2)], 3);
        assert_eq!(lines[&("base".into(), 9)], 5);
        assert_eq!(lines[&("head".into(), 9)], 6);
        assert!(!lines.contains_key(&("head".into(), 8)));
        assert!(diff_positions("@@ -1,2 +1,2 @@\n-old\n+new").is_none());
        assert!(diff_positions("@@ -0,0 +1 @@\n+new")
            .unwrap()
            .contains_key(&("head".into(), 1)));
        assert!(diff_positions("@@ -1 +0,0 @@\n-old")
            .unwrap()
            .contains_key(&("base".into(), 1)));
        assert!(diff_positions("not a diff").is_none());
        let lines = diff_positions("@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n@@ -9 +9 @@\n-a\n+b").unwrap();
        assert_eq!(lines[&("head".into(), 1)], 3);
        assert_eq!(lines[&("head".into(), 9)], 7);
        assert!(diff_positions("@@ -1 +1 @@\n-a\n+b\n@@ -1 +1 @@\n-a\n+b").is_none());
    }
}
