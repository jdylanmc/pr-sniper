# Copilot CLI · MCP control

## Install

| Step         | Action                                                                                |
| ------------ | ------------------------------------------------------------------------------------- |
| Prerequisite | PR Sniper 0.5.0+; Node 22+; Copilot CLI                                               |
| MCP          | Settings → Preferences → Copilot CLI → Install; restart Copilot CLI                   |
| Skill        | `npx skills add jdylanmc/pr-sniper --skill pr-sniper -g -a github-copilot`            |
| Discover     | `/skills reload`, then `/skills info pr-sniper`                                       |
| Invoke       | `/pr-sniper start watching this PR https://github.com/owner/repo/pull/123`            |
| Update       | App: update the app (registration follows). Skill: `npx skills update pr-sniper`      |
| Repair       | MCP **Incomplete** → Repair (moved app or removed entry)                              |
| Remove       | Trash button removes the owned MCP entry; `npx skills remove pr-sniper` removes skill |
| Conflict     | Preserve/move a foreign or customized `pr-sniper` entry yourself; no force overwrite  |

- Skill: [`skills/pr-sniper`](../skills/pr-sniper), thin orchestration only (no
  scripts). skills 1.7.2 installs it to `~/.agents/skills/pr-sniper`; Copilot CLI
  reads `~/.agents/skills` and `~/.copilot/skills`. Settings shows **Detected**
  read-only (links followed, nothing written). Protocol mismatch → update the app.
- Server: shipped by the app. macOS: signed `PR Sniper.app/Contents/Resources/mcp/mcp-server.mjs`.
  Windows: embedded bytes published to `%LOCALAPPDATA%\com.jdylanmc.pr-sniper\mcp\`
  (the installer owns only its executable and uninstaller).
- Registration: only `mcpServers.pr-sniper` in `~/.copilot/mcp-config.json`:
  `node <server> --app <app executable>`. No runtime npm download.
- Other servers and root configuration survive install/repair/removal. A
  foreign or customized own entry conflicts; no force overwrite or adoption.
- Moved/updated app: the owned entry shows **Incomplete**. An app in
  `/Applications`, `~/Applications` or `%LOCALAPPDATA%\PR Sniper` re-points it at
  launch; dev builds never rewrite it. A removed entry is never re-added implicitly.
- Install, Update, Repair and Remove require an installed app; any other build
  fails with `app_not_installed` (**App not installed**) and writes nothing.
- A link retargeted, or a `.copilot` folder swapped (macOS inode, Windows
  volume + file ID), around a write is a conflict, never reported as registered.
- Linked folders (macOS): a symlinked `~/.copilot` or `mcp-config.json` is
  written at its real target; the link stays. Each resolved ancestor must be
  owned by you or root and not group/world writable (sticky excepted). Dangling
  or retargeted links conflict; restore the link to recover.
- Linked folders (Windows): junctions/symlinks in `%USERPROFILE%\.copilot` are
  refused (**Linked folder**). Use a real folder and file.
- Failed setup/removal refreshes status once while retaining the original error.
  **Retry** only reads status; it never repeats the failed mutation.
- A durable intent joins the entry change and the ownership receipt. Reopen
  completes only an already-authorized intent; an external edit conflicts.
- A pre-release app-installed `~/.copilot/skills/pr-sniper` package is retired
  when its legacy receipt migrates (Repair or an owned-entry update), only if
  every file still matches its install receipt. Customized, extended or linked
  packages stay; delete them yourself, then run the skills command.
- Retirement moves the whole package into
  `~/.copilot/.pr-sniper-installer/legacy-skill-*`, re-verifies every member
  there (no links, same file identity and hash), then deletes it. A mismatch
  keeps it there and the legacy receipt; Settings shows **Moved aside:**
  `<path>` until you move or delete that folder.
- Native isolated profiles cannot read or change the real registration.
- Human login remains in Settings → Accounts. The skill binds existing GitHub
  and Copilot accounts; it neither reads tokens nor performs authentication.

## Behavior

| Request                      | Outcome                                                                  |
| ---------------------------- | ------------------------------------------------------------------------ |
| Watch configured PR          | Reuses its work; moves eligible queued jobs forward                      |
| Watch completed unchanged PR | Reports completed, no new pass                                           |
| Review again                 | New pass only after current work completes; no Wait bypass               |
| PR-only setup                | Explicit PR admission, never repository-wide review                      |
| New repository               | Choose accounts/model; optional SOLID + Security → Maintainer preset     |
| Disabled repository          | Explicit failure; choose scope and enable deliberately                   |
| Closed app                   | Background launch; explicit pause remains                                |
| Configuration edit           | Revision/expected-value checks; references and grants validated natively |
| Lost response                | Keep request ID and timestamp; inspect receipt, never blind retry        |

## Agent interface

- `/pr-sniper` orchestrates the discovered typed MCP tools, not shell JSON.
- `prepare_request` supplies retry-safe mutation identity; native receipts remain
  authoritative. SDK output preserves typed native errors in `structuredContent`.
- Separate tools for views, resources, connected accounts/models, grants,
  preferences, queue/health/evidence, watch and recovery; no generic execute tool.
- Desktop UI and MCP call the same native service handlers. The app remains
  the only configuration writer, permission enforcer and review scheduler.
- [Tool mapping and diagnostic protocol](../skills/pr-sniper/references/control.md).
- [Transport decision and trust boundary](adr/0025-local-agentic-control.md).
- `saved`, `queued`, `waiting`, `running`, `completed` are distinct outcomes.
- `inspect` returns bounded counts and control state. `accounts` carries safe
  identities/generations; `resources` pages configuration; `queue` pages operation
  identities, saved retry times, each item's exit, progress and typed
  `merge_pending` (approved; merge waits on `behind_base`/`conflicts`), each
  review's exit and denied exits with reasons, finding dispositions
  (`author_wont_fix`, `no_longer_applies`, `signed_off`, `carried_forward`) and
  typed Rubber Duck failure reasons; `detail` streams one item's evidence,
  including exit writes, as revision-checked JSON text. A closed or merged PR
  pruned automatically answers `detail` from its tombstone (`state: pruned`).
- `health` pages repository polling state, causes and exact retry IDs, even
  before any PR is detected. Use its retryable operation ID with `retry`.
- Ordinary failed/cancelled AI work uses `retry_review` with its candidate key.
  Completed work is refused, including stale retries; only explicit
  `watch.review_again` creates a fresh pass.
- A pending watch-configuration hold survives restart. After inspection,
  explicitly watch again with the intended assignment override map. Empty
  overrides preserve the hold; null fields on an addressed assignment select
  shared configuration and finish that explicit repair.
- The skill's human-invocation rule and host tool permissions remain necessary;
  the local protocol authenticates the OS user, not the author of a conversation.
- The bundled official MCP SDK supports legacy initialization and modern
  2026-07-28 stdio negotiation. Copilot CLI 1.0.100 accepts the local registration
  schema. SDK interoperability tests are not evidence of a signed-in host session.
- Internal `src-control/pr-sniper.mjs` remains for diagnostic/native acceptance
  tests only; it is not the skill's primary agent interface.
