# Admit PRs through ordered filters; first enabled match wins

## Context

Watching a whole repo floods the queue. Per-purpose toggles (watched author, reviewer request, mention) could not express excludes or path scoping.

## Decision

- Per repository, admission mode is `All` or `Filtered` (`src-tauri/src/admission.rs`).
- Filtered = ordered rules: assigned, mentioned, include/exclude users, include/exclude paths.
- First enabled matching rule decides. Exclude before include blocks; include before exclude admits. Order is user-owned.
- Limits: `MAX_VALUES_PER_RULE = 100`, `MAX_PATTERN_LENGTH = 256`. Unknown fields rejected.
- Path rules need the changed-file list. Fetch only when an enabled, reachable path rule exists.
- Repo with no `admission` keeps legacy watched-author / reviewer / mention behavior.
- Admission is sticky per iteration. Later filter edits reset the cursor.
- Migration turned monitoring off on existing repos. User re-enables with a chosen mode.

## Consequences

- Simple mental model, testable ordering (`first_match_wins_across_orderings`).
- No rule-priority UI beyond order. No negation groups.
- Legacy activation modes stay readable, inert.
