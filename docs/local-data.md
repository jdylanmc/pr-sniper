# Local data

Reference for the on-disk layout, atomic replacement, recovery and retention journal. Everything stays local.

## Root

- macOS: `~/Library/Application Support/com.jdylanmc.pr-sniper/`
- Windows: `%LOCALAPPDATA%\com.jdylanmc.pr-sniper` ([ADR-0004](adr/0004-windows-persistence-foundations.md))
- Override: `PR_SNIPER_DATA_DIR` (absolute path). Requires `PR_SNIPER_KEYCHAIN_SERVICE`, and the reverse. Isolated dev run:

```sh
PR_SNIPER_DATA_DIR="$(mktemp -d)" PR_SNIPER_KEYCHAIN_SERVICE="com.jdylanmc.pr-sniper.tests.dev-$(uuidgen)" npm run tauri -- dev
```

- The override never touches login items. Enable launch-at-login only from Settings in a normal installed run.
- Credentials live in the OS credential store, never in these files ([ADR-0001](adr/0001-macos-foundation.md)).

## Files

| Path                                                                                | Holds                                                                                          |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `config/settings.json`                                                              | Non-secret saved config: repos (with `cron`), `time_zone`, startup request, other settings     |
| `state/queue.json`                                                                  | Tracked lifecycle, iterations, immutable work identity + admission cause, order, pass ordinals |
| `state/pr-state.json`                                                               | Per-PR heartbeat baseline, pending state, `state_seq`, error count, local summary              |
| `state/queue-selection.json`                                                        | Selected queue item                                                                            |
| `state/automation.json`                                                             | Pause + capacity gate                                                                          |
| `state/notifications.json`                                                          | Request identities, observed transitions, send intent, outcomes                                |
| `state/actions.json`                                                                | Approve/Merge: immutable basis, FIFO identity, attempts, result                                |
| `state/publications.json`                                                           | Comment publication ledger                                                                     |
| `state/follow-ups.json`                                                             | Frozen reply input/output, operation budgets, attempted mutation, reply receipt                |
| `state/feedback.json`                                                               | Owned roots, current-head observations, missing-root errors, closure tombstones                |
| `state/retention.json`                                                              | Terminal-cleanup journal                                                                       |
| `state/result-index.json`, `state/result-index-pending.json`                        | Summary index for paging; rebuild-pending flag                                                 |
| `state/diagnostics.jsonl` (+ `diagnostics.previous.jsonl`), `state/attempts*.jsonl` | Logs: [diagnostics.md](diagnostics.md)                                                         |

- Legacy arrays and older shapes stay readable. Review/publication keys, receipts and old queue destinations survive adoption. Embedded originating reviews prevent replay when an older queue or review file is missing.
- Legacy follow-ups load into typed `target` and `context` without writes on read. Retained original review supplies provenance. Legacy `trust_confirmed` is historical evidence only; never a gate.

## Config rules

- Repos accept `owner/repository` or an HTTPS `github.com` URL. Case and clone suffix normalized; duplicates merged.
- Rename, disable, re-enable, remove act on stable local repo identity. No GitHub contact.
- `root_folder` is kept for backward compatibility only. Never used for intake.
- Invalid or unreadable settings are reported, never reset or overwritten. Save is refused until repaired.
- Failed config write is visible. Config committed but diagnostics write failed: committed state plus a separate warning.
- Older builds may not read newer config. No downgrade migration.
- Schedule migration (load and whole-settings saves; idempotent, written once): a saved global `defaults.schedule` cron becomes every repository's `cron`, its zone becomes `time_zone`, and the global field is dropped. No `time_zone` in the file = pre-0024 file; without a saved schedule it ran on `*/15 * * * *` UTC. A legacy interval, or a legacy cron that never fires, moves its zone and leaves every repository paused (`enabled: false`) on `*/30 * * * *`. New repositories get `*/30 * * * *` ([ADR 0024](adr/0024-repository-schedules.md)).

## Library references

- `doctrines[]`: `{id, title, body}`. `voices[]`: `{id, title, body, guardrails}`. `id` is a UUID v4, never changed.
- Agents: `doctrine_ids` (ordered) and `voice_id`. Renames never rewrite Agents.
- Load migration (idempotent, written once): ids for id-less entries; legacy Agent `doctrine` / `doctrines` titles to ids (trimmed, case-insensitive; list wins over single). An unmatched title is kept as the reference and shown as unavailable. Legacy fields are not written back.
- Job evidence (`state/reviews.json`, `state/follow-ups.json`) keeps its recorded shape: older entries without ids or with title references load as recorded and compare by content (exact title, body), so they keep publishing and running.

## Atomic replace

- Writes stage into a unique private file, with a locked ownership record flushed before the payload.
- Rename is the commit point. No uncommitted failure after rename. Claim locks release on scope exit; unlock errors are best-effort diagnostics.
- Deferred directory sync or unlink stays claimed for the next write or startup recovery.
- Startup discards only proven abandoned stages, then repeats cleanup journals. A partial stage is never promoted. Staged bytes never replace committed data.
- Visible errors: unowned fixed `.tmp`, links, inaccessible stages, corrupt ownership records.
- Bootstrap interruption may leave an empty stage and a small incomplete ownership record. It holds no deletion proof and no PR detail.
- Windows: payload handles close before replace; private ACLs required ([ADR-0004](adr/0004-windows-persistence-foundations.md)).
- Ordinary reads take no writer lock and do no housekeeping writes (read-only config dirs work). Only a missing committed file needs recovery before absence or defaults are reported.
- Recovery failures are explicit. Recovery never undoes committed data. Corrupt committed data or applying cleanup journals block reads.
- Guarantees cover process interruption, not power loss.

## Retention journal

- Cleanup uses typed JSON collections. No database, per-PR store or archive ([ADR-0015](adr/0015-local-only-results-and-retention-journal.md)).
- `state/retention.json` records safety receipts first, then fences access, then removes terminal detail from reviews, publications, conversations, final-review, action state, feedback, notifications, queue jobs and the result index.
- Cross-file interruption rolls forward at startup or maintenance. Failure is visible, never success. Settings and account resources are untouched.
- Reopening before the destructive cutover cancels cleanup. Cutover holds the Store lock (local serialization, not GitHub atomicity).
- Reopen after cutover: new iteration, even at the same SHA. No restore.
- Receipts survive cleanup: iteration, work, operation, publication and action receipts; provider ownership IDs; root-body SHA-256 fingerprints; sticky human-closed concerns.
- Not retained: full prompts, config snapshots, findings, guides, thread bodies, provider observations.
- Cleanup trigger and results API: [active-results.md](active-results.md).

## Queue compaction and idle scheduling

- Startup and each maintenance pass drop retired `queue.json` entries (`monitoring::retired`) that nothing reads again. Kept: live, `repository_disabled` and legacy entries; entries matched by saved reviews, publications or follow-ups; each PR assignment's newest pass; one entry per revision; PRs with unresolved legacy mentions. Saved via the atomic queue write; idempotent; never lowers enqueue order.
- The 5 s scheduler always runs connectivity and poll checks. Coordinators (capacity, publication, exit writes, follow-ups, actions, notifications, maintenance) run only when saved state changed or the 60 s heartbeat elapses (`src-tauri/src/wake.rs`).
- Liveness probes, the result index and the queue selection do not wake coordinators. An unchanged pass writes nothing, so a quiet profile stays idle.
