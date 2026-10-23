# Active results

Reference for the native `result_page` / `result_detail` commands, summary index paging, mention intent in results and terminal cleanup.

## `result_page`

Request (cursors start `null`):

```ts
{ request: { limit: 25, cursor: null, unavailable_cursor: null } }
```

- `limit`: integer `1..200` (`src-tauri/src/retention/paging.rs`). No total result cap.
- Returns `results`, `next_cursor`, `unavailable`, `unavailable_count`, `next_unavailable_cursor`. Each array has at most `limit` entries.
- Result row: exact iteration, job, activity sequence, completed normal-pass count, review-attempt count, conversation count. Retries are separate attempts, not normal passes.
- Continue each stream with its own cursor. A `null` next cursor ends only that stream; sending `null` restarts it. Do not append a restarted stream after a finished one. Streams do not share a page budget.
- Healthy older results stay reachable when other entries are ambiguous.
- `unavailable_count` = current total, repeated on every page. Resolved entries drop without invalidating cursors. New ones appear on a new traversal.

## `result_detail`

- Request `{ destination }`. Returns `available`, `cleaned` or `missing`. Storage failure rejects.
- Saved panel, notification and provider-link destinations explain cleaned detail. Never open another PR or iteration.
- Published discussion stays on GitHub. Unpublished terminal detail is not restored. No purge control, no history screen.
- On an `unavailable` entry's destination: shows retained mention intent. No fabricated execution.

## Summary index

- Order: newest activity first, ties by item ID ascending. Unchanged polling adds no activity. Execution progress is new activity.
- Pages read the index (`state/result-index.json`), not review bodies. Cursors are versioned keysets, not offsets or snapshots.
- Legacy results index deterministically; no invented completion timestamps. An interrupted index rebuilds from durable evidence (`result-index-pending.json`).
- Open PR jobs, passes, earlier iterations and owned feedback stay durable across scans and restarts.

## Mention intent and unavailable entries

- Mention intent belongs to its exact iteration. It advances the result even when primary routing is blocked.
- Tracking supplies identity, also with no Agents. Deferred routing needs a job on the exact current iteration.
- A superseded unstarted intent is retained: no block, no replay.
- Uncertain writes, missing execution history and outstanding human concerns block clearance. Repeated scans and later linkage never double count.
- Baseline unlinked mentions associate only with the unchanged first tracked iteration. Intervening revisions: unavailable. Never matched by latest job or timestamp.
- Feedback load pins provable identity before returning. Queue replacement passes the load boundary before overwriting old tracking proof. Migration failure is an explicit error.
- `unavailable` entries carry exact destinations, scope and missing evidence. Sorted by immutable mention work ID ascending. Job-backed counts exclude unassociated intent.
- An ambiguous mention or a result with no job-backed row never disables healthy results.

## Terminal cleanup

- Cleanup is automatic for PRs observed closed or merged: scheduled maintenance and each finished poll prune up to `retention::PRUNE_PER_PASS` PRs per pass, after their workers settle. Reviewed **Clean** (one PR) and **Clean all** run the same cleanup on demand. Only terminal PRs qualify; `pr-state.json` records go with them.
- Automatic pruning skips a PR whose owned-publication provenance needs repair (detail kept); **Clean** reports it.
- Only explicit provider-confirmed closure or merge admits detail cleanup. Not: missing scan, disabled or deleted config, lost accounts, completed passes.
- Maintenance first signals terminal AI workers to stop and waits for reservations to release. Excludes live publication, reply, action, notification and scan snapshots.
- Pending reviews and uncertain writes keep recovery records. A stopped local job is not a settled provider mutation.
- Legacy closed/merged records without authority are revalidated by a gated provider scan. Missing, rejected or failed reads keep detail. Migration never invents terminal confirmation.
- Journal, receipts, roll-forward: [local-data.md](local-data.md#retention-journal).
- On reopen, a paginated GitHub read verifies old roots from compact provenance before owner-only follow-ups or new feedback. Human closure persists against the current tracked binding and head, even with no Agents or jobs. Closure is monotonic across restart and reassignment. Missing or changed roots block; no invented clearance. Uncertain or incomplete replies block readiness.
