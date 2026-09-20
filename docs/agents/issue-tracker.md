# Issue tracker: GitHub

## Anchor and scope

- Host: `github.com`.
- Repository and planning backlog: `jdylanmc/pr-sniper`.
- Provider tooling: `gh`; pass `--repo jdylanmc/pr-sniper` on issue/PR commands.
- Resolve the authenticated identity with
  `gh api --hostname github.com user --jq .login`, not Git author settings.
- Integration branch: `main`. Humans merge PRs.
- Default view: the repository's open issues, not only issues assigned to the
  authenticated user. Hydrate candidates and select only the approved MVP.

The approved scope is the macOS-first, GitHub-first MVP in
`../agent/specs/pr-sniper-mvp.nano.md`, explained by
`../agent/specs/pr-sniper-mvp.full.md`. Existing issue descriptions must be
reconciled with that authority before dispatch; broad historical requests do
not expand it. Windows release, Azure DevOps implementation, leaderboard,
embedded diff review, hosted team service, native OAuth, production update
feeds and provider approval submission remain outside this MVP.

GitHub Issues anchors work and dependencies. Preserve the existing complete
discovery/specification documents and link their exact acceptance criteria
from tickets; do not move or replace those documents during setup.

## Readiness and ownership

Use `triage-labels.md`. Query open issues with the mapped `ready-for-agent`
label, then verify scope, complete requirements, dependencies and ownership.
An issue without that role is not automatically actionable. Empty ready work
routes to the one Discovery/backlog-manager conversation.

Fetch complete pages or explicitly report partial coverage. Hydrate relevant
issue bodies, comments, dependency edges and linked PRs before assignment.
A ready label alone does not override a blocker, existing owner or missing
human decision. Do not duplicate an existing delivery.

Use GitHub native issue dependencies where available. If unavailable, retain
explicit `Blocked by: #N` references and verify their live state. An unmerged
dependency is not available merely because its checks are green.

Reserve either an approved parent/specification graph or distinct child
deliveries, never both. Obtain human approval of ticket publication and
dependency/PR grouping through Breakdown Tickets before publishing a new
graph. Preserve partial publication receipts and reconcile uncertain writes.

The private Joe owner board records active delivery claims. Do not reassign
someone else's issue to create a claim. Preserve unrelated labels and state.
Routine scoped comments and readiness updates follow the invoked workflow's
authority; this file is not a blanket tracker-mutation grant.

## Publication and completion

Publish complete approved ticket content, source/revision and acceptance
references. If provider limits prevent that, obtain approval for a linked
accessible artifact rather than silently truncating it.

Create PRs against `main` from owned isolated worktrees. Independent review,
relevant checks and current-base evidence are required before claiming
readiness. Missing application CI or lint is a foundation task, not a passed
check. Only the human merges; setup does not close or mark existing issues
ready automatically.

**PRs as a request surface: no.**
