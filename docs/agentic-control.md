# Copilot CLI control

## Install

| Step         | Action                                                                      |
| ------------ | --------------------------------------------------------------------------- |
| Prerequisite | PR Sniper with protocol 1 support; Node 22+; Copilot CLI                    |
| Install      | Settings → Preferences → Copilot CLI → Install                              |
| Discover     | `/skills reload`, then `/skills info pr-sniper`                             |
| Invoke       | `/pr-sniper start watching this PR https://github.com/owner/repo/pull/123`  |
| Update       | Update the app, then the same Settings section → Update                     |
| Remove       | Trash button in the same section; customized files are never deleted        |
| Conflict     | Preserve/move the existing skill yourself, then install; no force overwrite |

- Bundle location: `~/.copilot/skills/pr-sniper` (Windows: `%USERPROFILE%\.copilot\skills\pr-sniper`).
- Source/skills.sh-compatible package: [`skills/pr-sniper`](../skills/pr-sniper).
  Install only this skill for Copilot CLI; no other host is required or configured.
- Human login remains in Settings → Accounts. The skill binds existing GitHub
  and Copilot accounts; it neither reads tokens nor performs authentication.
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

## CLI

```sh
node ~/.copilot/skills/pr-sniper/scripts/pr-sniper.mjs describe
node ~/.copilot/skills/pr-sniper/scripts/pr-sniper.mjs inspect
```

PowerShell: `node "$HOME\.copilot\skills\pr-sniper\scripts\pr-sniper.mjs" inspect`.

- One JSON operation on stdin; one JSON response on stdout.
- [Operation and error contract](../skills/pr-sniper/references/control.md).
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
