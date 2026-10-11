# PR Sniper

Build the approved macOS-first, GitHub-first MVP. Product authority is
`docs/agent/specs/pr-sniper-mvp.nano.md`; supporting requirements are in
`docs/agent/specs/pr-sniper-mvp.full.md`. Preserve the existing discovery and
specification artifacts in `docs/agent/`; workflow guidance lives in
`docs/agents/`. Do not treat requirements as implemented behavior.

For #51, apply the scoped
[`SPEC-PR-SNIPER-VNEXT` amendment](docs/agent/specs/pr-sniper-vnext.nano.md)
and its [supporting reconciliation](docs/agent/specs/pr-sniper-vnext.full.md).
It supersedes only the explicitly mapped MVP behavior, including polling,
primary-Agent routing and opt-in provider actions. Other MVP requirements remain
in force. Product permissions do not grant development agents permission to
approve, merge or bypass repository protections.

The human-authorized 2026-09-29 Windows follow-up under #12 extends the
platform scope to existing-app system-tray parity, Windows CI and Chocolatey
distribution (#57-#65). Merge the honest CI bootstrap (#65) before native
port deliveries; extend it to native checks as those paths become runnable.
Preserve macOS behavior and all existing review/account/publication boundaries.
This does not authorize vNext features or imply Windows/release acceptance.

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

### Pull requests

Every agent that creates or updates a PR in this repository invokes
`/pull-request` (`.agents/skills/pull-request/`) to write the body: terse
What/Why/How, Evidence, and Blast Radius. Use the repository PR template headings
if one exists. Ship, Patch, Refactor, Shepherd and `create-pull-request`
publication use that body; do not hand-write a different format. The skill
grants no approval, ready, or merge authority.

### Local development

- Rust needs `export PATH="$(dirname $(rustup which cargo)):$PATH"` in each shell.
- Run the app with `npm run dev:signed` (stable macOS signing identity, so
  keychain "Always Allow" survives rebuilds). Plain `tauri dev` is ad-hoc signed
  and re-prompts on every rebuild. `tauri dev` also rebuilds and relaunches on
  every Rust edit: stop it before agents edit Rust, restart it after. Web-only
  edits hot-reload without a rebuild.
- The app is tray-only. The panel opens from the menu-bar icon or by launching
  the app again. `PR_SNIPER_PANEL_DEBUG=1` logs screen and placement geometry.
- This repository belongs to `jdylanmc`; `gh` may default to another account.
  Use `GH_TOKEN="$(gh auth token --user jdylanmc)"` for `gh` and a one-shot
  credential helper for `git push`; never change the global active account.
- Never use `pkill`; stop only your own processes by PID.

### UI conventions

Icon-first, minimal prose: no helper paragraphs, empty-state text or hints.
Page title and back arrow live in the navy band; cards overlap it by 23px.
Primary add actions are floating orange pills; destructive row actions are icon
buttons; lists drill in (Doctrines, Reviewer requests) rather than use selects.
Keep `prototypes/v2/` in step with UI changes. Text hidden with CSS stays in the
DOM; do not assert visibility on it.

### Testing

- `npm run test:settings` (Playwright, ~800 tests) takes ~11 minutes serially.
  Shard it for speed: separate `SETTINGS_TEST_PORT` and `--output` per shard.
- CI also runs it on WebKit (`--browser=webkit --workers=2`); run it before
  pushing UI changes that touch focus or layout.
- When the UI changes on purpose, update the spec to the new intent. Never
  delete, skip or loosen a test to pass it.

### Releases

Follow `docs/releases.md`. Merge the version PR, wait for the main-push **macOS
foundation** run on that exact commit, then push the annotated tag. Never move a
published tag.

### Wrap-up

`/wrap-up` (`.agents/skills/wrap-up/`) is a human-invoked checkpoint: freeze
scope, make tests, lint, docs and the visual POC true, true-up with `main`, and
land one green PR. The human's direct invocation authorizes marking that one PR
ready once CI is green; it never authorizes approval or merging.

### Doctrine

The complete local package is `.agents/skills/doctrine/`. Use `/doctrine`
for catalog, selection and verified loading. No additional repository-wide
doctrines are selected. PR-producing work requires `worktrees`; code Roast
requires `solid`. Orchestrators supply scoped selections and source digests;
workers load the full selected texts before applying them.

### Delivery coordination

The human merges PRs by default. Exception: Dylan's explicitly designated
independent PR coordinator may review and merge PRs in `jdylanmc/pr-sniper`
as `jdylanmc` under the [merge gate](.agents/skills/joe-mode/MERGE.md)
until Dylan revokes the grant; changes to this exception remain human-merged.
The 2026-10-08 human authorization carries that same gate to the CMUX
coordinator; it does not grant merge power to PM, implementers or Shepherd.
PR Sniper may be pointed at its own repository through separately configured
app assignments, but that does not bypass the coordinator gate or authorize
an agent to approve its own implementation. This policy-changing PR remains
human-merged.
Agents must not submit approval votes, enable automatic merging or bypass
provider protections.

Use one repository PM and shared owner board, isolated delivery worktrees,
independent review, relevant verification and maintained PR custody. Preserve
unrelated changes. Route missing readiness or product decisions to the single
Discovery conversation rather than inventing requirements.

The human-authorized Joe team has a maximum of six developers, not a target.
Feature lanes reserve two slots; fixes, hardening and refactors reserve one.
Count every writing descendant within that cap. Support roles do not become
extra implementers. Start Discovery when no actionable work is available.

The repository ships core Joe-mode and the session-bound
[CMUX adapter](.agents/skills/joe-mode-cmux/SKILL.md). Paseo and Orca adapters
are retired; reconcile any surviving legacy owners/jobs before activation.
There is no CMUX heartbeat or unattended scheduler. Pause/stop prevents new assignments while
existing workers finish their bounded tasks and preserve their results. PM
accepts results and retires owned agents only after duties end or transfer;
questions and unresolved decisions return to the primary human chat.

Activation still requires the Joe-mode CMUX capability and ownership gates.
Installing skills or granting merge permission does not activate Joe-mode,
start workers, configure app assignments or change live runtime permissions.
Keep runtime IDs, grants and receipts private, never in committed files.
Resolve the one private board beneath the absolute Git common directory at
`pr-sniper-team/board.json`; do not create a separate board per worktree.
