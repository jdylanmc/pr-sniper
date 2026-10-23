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
Preserve macOS behavior and all review/account/publication boundaries.
This does not authorize vNext features or imply Windows/release acceptance.

## Agent skills

### Issue tracker

GitHub Issues in `jdylanmc/pr-sniper` is the backlog. See
`docs/agents/issue-tracker.md` for scope, readiness, dependencies and authority.

### Triage labels

Use the five default role mappings in `docs/agents/triage-labels.md`.

### Domain docs

Single context: `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.

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

### Docs map

- Domain terms and invariants: [`CONTEXT.md`](CONTEXT.md). Read before domain work.
- Decisions: [`docs/adr/`](docs/adr/). Reference: [`docs/`](docs/) (`releases.md`, `windows-*.md`, `copilot-settings.md`).
- Specs: `docs/agent/`. Agent workflow: `docs/agents/`.
- Test notes: [`tests/settings/README.md`](tests/settings/README.md), `tests/macos-acceptance.md`, `tests/macos-notifications.md`.

### Layout

- `src/`: web UI (Vite, `safari15` target). `src-tauri/`: Rust (`capacity.rs`, `monitoring.rs`, `storage::diagnostics`).
- `src-tauri/doctrines`: app doctrine catalog, independent of `.agents/skills/doctrine`.
- `src-tauri/voices`: app Voice seeds ([ADR 0022](docs/adr/0022-voices.md)).
- `tests/settings/`: Playwright. `tests/release/`: release/packaging tests. `scripts/`: sign, release, icons, Windows.
- `site/`: static GitHub Pages site, no build step ([docs/site.md](docs/site.md)). `tests/site/`: its Playwright suite.

### Local development

- Prereqs: macOS 13.5+, Xcode CLT, Node 24.20.0, Rust 1.98.1 via rustup. Pins:
  `.node-version`, `rust-toolchain.toml`, `package-lock.json`, `src-tauri/Cargo.lock`.
  Commands install no globals.
- Rust needs `export PATH="$(dirname $(rustup which cargo)):$PATH"` in each shell.
- `npm ci`, then `npm run dev:signed`: stable macOS signing identity, so
  keychain "Always Allow" survives rebuilds. Override with
  `PR_SNIPER_DEV_SIGN_IDENTITY`. Plain `npm run tauri -- dev` is ad-hoc signed
  and re-prompts on every rebuild. Dev rebuilds and relaunches on every Rust
  edit: stop it before agents edit Rust, restart it after. Web-only edits
  hot-reload. Stop dev command and its frontend server when done.
- Isolated run (absolute data dir; never touches login items):

  ```sh
  PR_SNIPER_DATA_DIR="$(mktemp -d)" \
  PR_SNIPER_KEYCHAIN_SERVICE="com.jdylanmc.pr-sniper.tests.dev-$(uuidgen)" \
  npm run tauri -- dev
  ```

- The app is tray-only. The panel opens from the menu-bar icon or by launching
  the app again. `PR_SNIPER_PANEL_DEBUG=1` logs screen and placement geometry.
- This repository belongs to `jdylanmc`; `gh` may default to another account.
  Use `GH_TOKEN="$(gh auth token --user jdylanmc)"` for `gh` and a one-shot
  credential helper for `git push`; never change the global active account.
- Never use `pkill`; stop own processes by PID.
- Icons: `swift scripts/generate-icons.swift`. Source `src/crosshair.svg`; tray uses a transparent template PNG.
- Never put credentials in doctrines, prompts or config. Never log provider bodies, tokens or subprocess output.

### Commands

| Command                                          | Does                                                      |
| ------------------------------------------------ | --------------------------------------------------------- |
| `npm run dev`                                    | Vite only, 127.0.0.1                                      |
| `npm run build`                                  | `tsc --noEmit` + Vite build                               |
| `npm run lint`                                   | `tsc --noEmit` + clippy `-D warnings`                     |
| `npm run format` / `format:check`                | Prettier + `cargo fmt` (owned files only)                 |
| `npm test`                                       | `cargo test --locked`                                     |
| `npm run test:settings`                          | Playwright vs real Rust storage                           |
| `npm run test:site`                              | Site Playwright, Chromium + WebKit                        |
| `npm run test:skills`                            | Skill package/link + repo-policy contracts (checks links) |
| `npm run test:release`                           | Release script + workflow tests                           |
| `npm run bundle`                                 | `.app` with required local ad-hoc signature               |
| `npm run build:windows` / `test:release:windows` | Windows exe / release tests (PowerShell, Python 3.13)     |

Legacy CLI credential diagnostic, only when the user authorizes it (read-only;
prints identity, capability, counts; use your own account ID):
`cargo run --manifest-path src-tauri/Cargo.toml --locked --example github_read -- jdylanmc/pr-sniper 6954990 --metadata`.

### UI conventions

**No prose.** Never add helper text, notices, banners, hints, empty-state
sentences, tooltips-as-paragraphs or explanatory strings to the app, and never
write status sentences from the backend for the UI to show. State is shown by
controls, icons and values. README and docs stay short: tables and lists, no
narrative. If something needs explaining, the design is wrong.
Exception: the Tools list shows a one-line description per tool (owner-approved).

Icon-first, minimal prose: no helper paragraphs, empty-state text or hints.
Page title and back arrow live in the navy band; cards overlap it by 23px;
window corners 8px. Primary add actions are floating orange pills; destructive
row actions are icon buttons; lists drill in (Doctrines, Admission filters)
rather than use selects. Keep `prototypes/v2/` (visual POC, open `index.html`) in step with UI.

Focus: Back and dialog close restore the activating control; retain the logical
opener across redraws via stable resource IDs (removed resource: section
heading; removed assignment: its Add control). Background completion never
steals focus. Keep Vite target `safari15`; add no polyfills; keep
viewport/dialog fallbacks through minification.

### Testing

- `npm run test:settings` (Playwright, ~800 tests) takes ~11 minutes serially.
  Shard 4 ways (`--shard=N/4`) with a separate `SETTINGS_TEST_PORT` and a
  separate `--output` dir per shard. Never `pkill` leftover servers.
- CI shards it by spec file (`--shard=N/4` macOS Chromium + WebKit,
  `--shard=N/3` Windows, `--workers=2`). Run WebKit (`--browser=webkit`) before
  pushing UI changes that touch focus or layout.
- Text hidden with CSS stays in the DOM; do not assert visibility on it.
- When the UI changes on purpose, update the spec to the new intent. Never
  delete, skip or loosen a test to pass it.
- Browser tests simulate dialog/viewport APIs. Not native acceptance. Native
  proof: `tests/macos-acceptance.md` + `tests/macos-native-smoke.swift`; a bundle
  compile is not acceptance.
- Notification tests need the packaged `.app` (`npm run bundle`); the dev server
  is no native host. Per-app macOS authorization still applies:
  `tests/macos-notifications.md`.
- Live mutation (approve, merge, publish) only on an explicitly authorized
  disposable PR, with cleanup. Fixtures never claim a live pass. Never mutate to
  probe permissions.
- Never infer failure origin from message text; carry typed provenance.
- No user-facing `Blocked`; parked failures live in Queue > Needs attention.

### Releases

Follow [`docs/releases.md`](docs/releases.md). Merge the version PR, wait for the main-push **macOS
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
PR Sniper itself is a separate reviewer, not a development agent. On
2026-10-22 Dylan authorized PR Sniper's configured app assignments for
`jdylanmc/pr-sniper` (including its primary decider, The Maintainer) to review,
resolve their own threads, approve and merge PRs in this repository, including
PRs authored as `jdylanmc`, within the authority each assignment is configured
with in the app and only through the app's host-enforced gates (current head,
green required checks, not draft, mergeable and up to date, no unresolved
threads from other humans). This grant applies to the PR Sniper app's review
runtime only; it does not grant approval or merge power to PM, implementers,
Shepherd, coordinators or any other development agent, and it does not permit
bypassing provider protections. Dylan may revoke it by editing this section;
changes to this section remain human-merged.
Development agents (anything other than the PR Sniper app's configured review
runtime) must not submit approval votes, enable automatic merging or bypass
provider protections.

Use one repository PM and shared owner board, isolated delivery worktrees,
independent review, relevant verification and maintained PR custody. Preserve
unrelated changes. Route missing readiness or product decisions to the single
Discovery conversation rather than inventing requirements.

The human-authorized Joe team has a maximum of six developers, not a target.
Feature lanes reserve two slots; fixes, hardening and refactors reserve one.
Count every writing descendant within that cap. Support roles do not become
extra implementers. Start Discovery when no actionable work is available.

Joe-mode skills (`.agents/skills/joe-mode*`) are repo delivery agents, not app
review Agents. They, plus adapted Doctrine/Setup/Ship/Shepherd/Squadron, are
repo-owned: no `skills-lock.json` entries, no upstream restore. The repository ships core Joe-mode and the session-bound
[CMUX adapter](.agents/skills/joe-mode-cmux/SKILL.md). Paseo and Orca adapters
are retired; reconcile any surviving legacy owners/jobs before activation.
There is no CMUX heartbeat or unattended scheduler. Pause/stop prevents new assignments while
existing workers finish their bounded tasks and preserve their results. PM
accepts results and retires owned agents only after duties end or transfer;
questions and unresolved decisions return to the primary human chat.

Reconcile surviving legacy controllers/jobs before switching runtime; deleting
files stops nothing and authorizes no state deletion. `npm run test:skills` is
not proof of a live CMUX cockpit. Activation is human-only and still requires the Joe-mode CMUX capability and ownership gates.
Installing skills or granting merge permission does not activate Joe-mode,
start workers, configure app assignments or change live runtime permissions.
Keep runtime IDs, grants and receipts private, never in committed files.
Resolve the one private board beneath the absolute Git common directory at
`pr-sniper-team/board.json`; do not create a separate board per worktree.
