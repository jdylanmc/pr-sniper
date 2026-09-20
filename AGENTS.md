# PR Sniper

Build the approved macOS-first, GitHub-first MVP. Product authority is
`docs/agent/specs/pr-sniper-mvp.nano.md`; supporting requirements are in
`docs/agent/specs/pr-sniper-mvp.full.md`. Preserve the existing discovery and
specification artifacts in `docs/agent/`; workflow guidance lives in
`docs/agents/`. Do not treat requirements as implemented behavior.

## Agent skills

### Issue tracker

GitHub Issues in `jdylanmc/pr-sniper` is the backlog. See
`docs/agents/issue-tracker.md` for scope, readiness, dependencies and authority.

### Triage labels

Use the five default role mappings in `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout: root `CONTEXT.md` and `docs/adr/`, created lazily
through authorized domain work. See `docs/agents/domain.md`.

### Commit messages

Use the terse Conventional Commits policy in `docs/agents/commit-style.md`,
subject to explicit operator instructions and required trailers. The policy
does not authorize staging, committing or rewriting history.

### Doctrine

The complete local package is `.agents/skills/doctrine/`. Use `/doctrine`
for catalog, selection and verified loading. No additional repository-wide
doctrines are selected. PR-producing work requires `worktrees`; code Roast
requires `solid`. Orchestrators supply scoped selections and source digests;
workers load the full selected texts before applying them.

### Delivery coordination

The human merges PRs. Agents must not merge, approve on the human's behalf,
enable automatic merging or bypass provider protections.

Use one repository PM and shared owner board, isolated delivery worktrees,
independent review, relevant verification and maintained PR custody. Preserve
unrelated changes. Route missing readiness or product decisions to the single
Discovery conversation rather than inventing requirements.

The human-authorized Paseo team has a maximum of six developers, not a target.
Feature lanes reserve two slots; fixes, hardening and refactors reserve one.
Count every writing descendant within that cap. Support roles do not become
extra implementers. Start Discovery when no actionable work is available.

The approved runtime recipe is a five-minute heartbeat in the primary PM chat
on the current host until stopped. Pause/stop prevents new assignments while
existing workers finish their bounded tasks and preserve their results. PM
accepts results and retires owned agents only after duties end or transfer;
questions and unresolved decisions return to the primary human chat.

Activation still requires the Joe-mode Paseo capability and ownership gates.
Keep runtime IDs, grants and receipts private, never in committed files.
Resolve the one private board beneath the absolute Git common directory at
`pr-sniper-team/board.json`; do not create a separate board per worktree.
