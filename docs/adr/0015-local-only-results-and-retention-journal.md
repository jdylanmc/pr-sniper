# Keep results local; clean up through a retention journal

## Context

Review detail grows without bound. Wrong deletion loses evidence. A database adds weight for a menu-bar app.

## Decision

- Results stay on the machine. No upload, no archive.
- Typed JSON collections under `state/`. No DB, no per-PR store. Paging reads a summary index with versioned keyset cursors, not review bodies.
- Only explicit provider-confirmed closure/merge authorizes cleanup. Missing scan, disabled config, lost account, completed pass never qualify.
- `retention.json` journal (`src-tauri/src/retention.rs`): write safety receipts first, fence access, remove terminal detail, roll forward at startup/maintenance. Cutover holds the Store lock.
- Receipts kept: iteration/work/operation/publication/action IDs, provider ownership IDs, root-body SHA-256, sticky human-closed concerns. Prompts, snapshots, findings, thread bodies dropped.
- Atomic file replace: unique private stage + flushed ownership record before payload. Rename commits. Startup discards only proven-abandoned stages.
- Missing evidence never fabricates identity or clearance. Shown as unavailable.

## Consequences

- Some stale detail stays until provider confirms.
- Reopen before cutover cancels cleanup. After = new iteration, no restore.
- No purge UI. Not atomic with GitHub.
- Guarantees cover process interruption, not power loss.

## Amendment ([ADR 0021](0021-review-packet.md))

- Cleanup is human-triggered: Reviewed **Clean** per PR and **Clean all**. No timer deletes terminal detail. Terminal PRs only.
- Also deletes the PR's `pr-state.json` record. Uncertain publication and action records stay (ADR 0015 reconciliation). Ad hoc scope, owner review requested.
