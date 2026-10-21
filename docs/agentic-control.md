# Copilot CLI · MCP control

## Install

| Step         | Action                                                                                 |
| ------------ | -------------------------------------------------------------------------------------- |
| Prerequisite | PR Sniper with protocol 1 support; Node 22+; Copilot CLI                               |
| Install      | Settings → Preferences → Copilot CLI → Install                                         |
| Discover     | Restart Copilot CLI for MCP discovery; `/skills reload`, then `/skills info pr-sniper` |
| Invoke       | `/pr-sniper start watching this PR https://github.com/owner/repo/pull/123`             |
| Update       | Update the app, then the same Settings section → Update                                |
| Remove       | Trash button removes owned MCP entry and skill; customized content is never deleted    |
| Conflict     | Preserve/move the existing skill yourself, then install; no force overwrite            |

- Bundle location: `~/.copilot/skills/pr-sniper` (Windows: `%USERPROFILE%\.copilot\skills\pr-sniper`).
- MCP registration: only `mcpServers.pr-sniper` in `~/.copilot/mcp-config.json`;
  local `node <bundle>/scripts/mcp-server.mjs`, no runtime npm download.
- Explicit Install/Update/Repair owns the skill and server entry. Other servers
  and root configuration survive updates/removal. A foreign or customized own
  entry conflicts; no force overwrite or implicit adoption.
- Failed setup/removal refreshes status once while retaining the original error.
  If status remains unavailable, **Retry** only reads status; it never repeats
  the failed mutation or overwrites customized content.
- A durable integration intent joins package activation and host registration.
  Reopen completes only an already-authorized intent. Missing registration is
  shown as **Incomplete** with **Repair**, not a successful installation.
- Registration-only Repair reuses an exactly verified current package; it does
  not republish unchanged assets. Customized content still fails verification.
- Source/skills.sh-compatible package: [`skills/pr-sniper`](../skills/pr-sniper).
  Skill discovery alone does not register MCP; use the app's explicit setup.
  Only Copilot CLI is configured; no additional host is required.
- Human login remains in Settings → Accounts. The skill binds existing GitHub
  and Copilot accounts; it neither reads tokens nor performs authentication.
- Installer updates stage and verify a complete package outside skill discovery,
  under `~/.copilot/.pr-sniper-installer`. A locked journal records ownership
  before activation. Status/install/remove recover interrupted work: unfinished
  staging rolls back; a verified ready package finishes activation; interrupted
  cleanup resumes only for matching owned files.
- Directory replacement is recoverable, not a zero-gap atomic swap. The previous
  package stays in an owned backup until the new package and activation record
  are verified. Read current installation status/revision after an interruption;
  an old expected revision is never silently accepted.
- Unexpected, customized or linked content remains untouched and causes a
  conflict. Unclaimed pre-journal artifacts are preserved, never guessed to be
  owned. Linked installation/profile ancestry is intentionally unsupported;
  fixture-only canonicalization is not redirected-profile support.
- Incomplete per-file bootstrap claims inside an owned staging package are
  preserved together in a `.preserved` quarantine, never individually deleted
  on filename evidence. Rollback completes only while the prior live package
  remains verified unchanged; later install/update/removal can then proceed.
- Transaction storage and the skill destination must permit same-filesystem
  directory renames. Windows process-interruption recovery is tested separately
  from power-loss durability; no portable directory-flush guarantee is claimed.
- Native isolated profiles cannot read/recover or install/remove the real
  user-wide skill. Synthetic tests use explicitly owned fixture destinations.
- Existing installations without protocol 1 produce an unavailable/not-ready
  error; update the app rather than editing settings files.

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
  identities; `detail` streams one item's evidence as revision-checked JSON text.
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
- Internal `scripts/pr-sniper.mjs` remains for diagnostic/native acceptance
  tests only; it is not the skill's primary agent interface.
