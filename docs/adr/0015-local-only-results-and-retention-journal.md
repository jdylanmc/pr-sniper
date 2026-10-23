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

- Cleanup is human-triggered: Reviewed **Clean** per PR and **Clean all**. No timer deletes terminal detail. Terminal PRs only. (Superseded for closed and merged PRs: see Automatic terminal pruning below.)
- Also deletes the PR's `pr-state.json` record. Uncertain publication and action records stay (ADR 0015 reconciliation). Ad hoc scope, owner review requested.

## Review segments (2026-10-22)

- `reviews.json` becomes a manifest of content-addressed run files under `state/reviews/`; repeated `selection` and `feedback_context` snapshots are stored once. Still typed JSON; not a database, per-PR store or archive.
- Saving one run's progress writes that run and the manifest. Cleanup removes the same runs, then their files.

## Automatic terminal pruning (2026-10-22)

Supersedes the ADR 0021 amendment's "human-triggered only; no timer" for closed and merged PRs (owner decision: old reviews are dropped once the PR is no longer open).

- Scheduled maintenance and each finished poll run the cleanup above for PRs observed closed or merged, at most a few per pass, once their workers settle. Same receipts, journal, fences and reopen rules; uncertain publication and action records still stay.
- A PR needing provenance repair is skipped, not reported each pass. **Clean** / **Clean all** remain for on-demand cleanup and report it.
- Open PRs are never pruned. Reopen = new iteration with fresh state.
- `monitoring.json` keeps only the 64 most recent retired poll operations (never read back).

## Local stale-queue dismissal (2026-10-19)

- **Dismiss stale review** acknowledges one stale state, not terminal cleanup.
- `queue-dismissals.json`: item IDs and state fingerprints only; no evidence deletion.
- Exact state revalidated under the Store lock before atomic save. New head/base
  or normalized PR-state changes do not inherit dismissal.
- In-flight or uncertain work stays visible; no provider write, baseline consume,
  blanket PR ignore, permission change or retry.
