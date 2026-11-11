# Paths mode: repository paths AND or OR admission

## Context

[ADR 0027](0027-path-globs.md) made repository `paths` an AND gate on admission: "these people, and only in this path". Owners also need "these people, or anyone touching this path": follow a team on a repository, plus every change under one directory whoever wrote it. The old Include paths rule could express OR only as a first-match rule, coupled to rule order and to changed-file reads at decision time; ADR 0027 removed it.

## Decision

- `Repository.paths_mode`: `and` (default) or `or`. Absent = `and`; saved only when `or`. Type `paths::PathsMode`; effective test `Repository::paths_also_admit` (OR, `paths` non-empty, admission not Explicit).

| Mode | New PR admitted when                                                     | A revision outside `paths`   |
| ---- | ------------------------------------------------------------------------ | ---------------------------- |
| AND  | rules admit and a changed file matches (ADR 0027)                        | gets no new work             |
| OR   | rules admit, or a changed file matches and Exclude users does not reject | gets work: paths never block |

- **Precedence**: rules first, unchanged first-match order. A path match admits only a PR whose first matching rule is not Exclude users (`AdmissionConfig::excludes`): Exclude users beats a path match; a rule matching before it still admits.
- Empty `paths`: no path involvement in either mode. OR with no enabled admitting rule: paths alone decide. All + OR: everything (the rules admit all).
- **Explicit** admission: OR has no effect (only explicit intake admits); the UI disables the control. Explicit intake on an OR repository is never gated by paths; on AND it is (ADR 0027).
- Path facts decide OR exactly as the gate: per revision, the same complete-list ladder, persisted per PR (`path_facts`). A parked list (incomplete) admits and parks for a human: never missed.
- **Listing (ADR 0028)**: the narrowed author/reviewer listing is a superset only of what the rules admit. In OR any author's PR touching `paths` qualifies, and neither GitHub search nor Azure DevOps `searchCriteria` filters open PRs by changed path, so OR lists every open PR (`ListingScope::All`, batched, resumable). AND keeps the narrowed listing.
- **Fact budget**: besides tracked and rule-admitted PRs, a poll reads facts for at most `PATH_CANDIDATE_READS` (100) other open PRs, newest first, skipping excluded authors (`monitoring::fact_reads`). Facts persist per revision, so successive polls of a pass cover the rest; rate limits and failures keep ADR 0028's retry and backpressure.
- The mode is part of the trigger key: switching it resets the listing cursor and invalidates queued work like any admission change.

## Consequences

- One OR repository on a very large repository lists all open PRs over several polls and reads up to 100 changed-file lists per poll until each revision has facts.
- A PR admitted by paths alone stays tracked (sticky admission) and is reviewed on later revisions, even ones that leave `paths`.
- Older builds reject a settings file holding `paths_mode: "or"` (unknown field); `and` repositories are byte-identical.
- The activation preview lists rule matches only; existing PRs admitted only by paths join on their next revision.
