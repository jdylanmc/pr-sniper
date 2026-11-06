# Path globs: repository gate and secondary triggers

## Context

Admission path rules (Include paths, Exclude paths) were ordinary first-match rules: a matching user rule could admit a PR that touches no wanted path, and no rule could keep a teammate off unrelated changes. Changed files were read only when a path rule was reached; a PR whose files could not all be listed failed its review late.

## Decision

- **Repository gate**: `Repository.paths`. AND gate on top of admission (All, Filtered, Explicit and explicit intake). Empty = no gate.
- **Secondary trigger**: `Assignment.paths` on a non-primary assignment. The primary has none (validation rejects it): it reviews everything the gate admits.
- Syntax (`src-tauri/src/paths.rs`): include patterns plus `!pattern` excludes, gitignore-style. Per file, the last matching pattern wins; a list of only excludes starts from "everything". A PR matches when any changed file (either name of a rename) is selected. Pattern syntax reuses admission (`glob_matches`, `MAX_PATTERN_LENGTH`); a list holds up to `2 × MAX_VALUES_PER_RULE` (200) so both migrated path rules fit.
- Migration: enabled path rules move into the gate (includes, then `!excludes`), disabled ones vanish, both kinds leave admission. A Filtered repository whose only enabled rules were path rules, one an include, becomes All. Idempotent on every load; monitoring state is kept. Path rule kinds still deserialize, but saves reject them.
- **Path facts** are read per revision before work is queued (poll and explicit intake) and stored per PR in the monitoring state (`path_facts`): head and base, gate verdict, per-secondary verdict (with its patterns) and the head each was compared from, or the incomplete count and strategies tried. They are re-read when the head, the base (retarget) or any input (patterns, a secondary's last reviewed head) changes; a skip needs facts for the current patterns.
- **Incremental**: a secondary matches against files changed since the head of its own last completed review of this PR. None yet: whole PR. Same head as that review: no new files. GitHub: `compare/{old}...{new}`; not `ahead` (rebase, force-push) or a missing old head: whole PR. Azure DevOps: iteration changes `$compareTo` the old head's iteration; none: whole PR. An incremental read no strategy completes falls back to the whole-PR list (may run, never misses).
- **Skip**: the secondary's entry is retired as `skipped_no_matching_files` and listed on the item as **Skipped: no matching files**, also after a newer revision makes it stale. A skip on the same head as the secondary's completed report (the PR state moved, not the files) is not listed: that report stays on the item. Retired entries never block the primary, its exits, readiness, approval or merge.
- **Complete lists only**, no file-count limit of our own. Ladder, first complete wins: (1) provider listing paged to the end (GitHub: listed = `changed_files`, no cap hit; Azure DevOps reports no total, so its paged listing is complete); (2) tree diff of merge base (or old head) and head, blob sha/mode; (3) scoped tree diff, globs only: level-by-level walk under each include's fixed prefix, entering only differing subtrees; it may keep a PR out, but a PR it admits parks (step 4), since the review needs every file; (4) park for a human: no review, Needs attention `incomplete_files` "Can't see all N changed files: needs a human" with the strategies tried, re-evaluated on the next revision. Applies to every repository. Without globs only completeness runs: the count first, then step 2 when it exceeds the listing cap.
- Rate limits, network failures and raced reads fail the poll and retry under the shared policy; they never decide a gate, a skip or a park.

## Consequences

- Empty globs keep today's queue, plus one GitHub detail read per new revision for the file count.
- A PR past every strategy needs a human instead of failing its review late. A local git step (#305) was declined: the ladder ends at park.
- The review reads the same complete list (#307): on GitHub past the 3,000-file listing, the listed files keep their patches and the merge-base tree diff adds the rest without a patch (no addressable line).
- Path scoping no longer interacts with rule order.
- A secondary does not re-review a discussion-only state change after reviewing that head.
