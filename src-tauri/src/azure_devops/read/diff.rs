//! Unified diffs of changed files. Azure DevOps serves no patch text, so the
//! review context diffs both blobs locally: the same hunk format GitHub
//! serves, which maps inline comments to diff lines.

use similar::TextDiff;
use std::time::Duration;

/// Larger sides get no patch: their lines stay unaddressable.
pub(super) const MAX_SIDE_BYTES: usize = 1024 * 1024;
const CONTEXT_LINES: usize = 3;
const DEADLINE: Duration = Duration::from_secs(2);

/// Hunks from `old` to `new` (`None`: the side does not exist), without file
/// headers. `None` when a side is not text or too large, or nothing changed.
pub(super) fn patch(old: Option<&[u8]>, new: Option<&[u8]>) -> Option<String> {
    let (old, new) = (text(old)?, text(new)?);
    let diff = TextDiff::configure()
        .timeout(DEADLINE)
        .diff_lines(old, new);
    let mut unified = diff.unified_diff();
    unified.context_radius(CONTEXT_LINES);
    let patch: String = unified.iter_hunks().map(|hunk| hunk.to_string()).collect();
    let patch = patch.trim_end_matches('\n');
    (!patch.is_empty()).then(|| patch.to_owned())
}

fn text(side: Option<&[u8]>) -> Option<&str> {
    match side {
        None => Some(""),
        Some(bytes) if bytes.len() > MAX_SIDE_BYTES || bytes.contains(&0) => None,
        Some(bytes) => std::str::from_utf8(bytes).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::diff_positions;

    #[test]
    fn diffs_modified_added_and_deleted_files_into_addressable_hunks() {
        let old = (1..=20).map(|n| format!("line {n}\n")).collect::<String>();
        let new = old.replace("line 10\n", "line ten\n");
        let hunks = patch(Some(old.as_bytes()), Some(new.as_bytes())).unwrap();
        assert!(hunks.starts_with("@@ -7,7 +7,7 @@"), "{hunks}");
        let lines = diff_positions(&hunks).unwrap();
        assert!(lines.contains_key(&("head".into(), 10)));
        assert!(lines.contains_key(&("base".into(), 10)));
        assert!(!lines.contains_key(&("head".into(), 2)));

        let added = patch(None, Some(b"a\nb")).unwrap();
        let lines = diff_positions(&added).unwrap();
        assert!(lines.contains_key(&("head".into(), 2)));

        let deleted = patch(Some(b"a\nb\n"), None).unwrap();
        assert!(diff_positions(&deleted)
            .unwrap()
            .contains_key(&("base".into(), 1)));

        assert_eq!(patch(Some(b"same\n"), Some(b"same\n")), None);
        assert_eq!(patch(Some(b"\0bin"), Some(b"text")), None);
        assert_eq!(patch(None, Some(&vec![b'a'; MAX_SIDE_BYTES + 1])), None);
    }
}
