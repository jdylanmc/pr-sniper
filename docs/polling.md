# Polling and GitHub reads

Reference for scheduled detection, admission rules, the read-only GitHub connection and PR metadata reads. Decisions: [ADR-0007](adr/0007-retry-then-park-single-inbox.md), [ADR-0009](adr/0009-admission-filters-first-match-wins.md), [ADR-0024](adr/0024-repository-schedules.md) (supersedes [ADR-0014](adr/0014-single-global-cron-schedule.md)).

## Schedule

- One cron per repository (repository editor builder, [settings.md](settings.md#repository-schedule)), all evaluated in the one Preferences time zone. One read per account/repo binding per occurrence.
- Each repository keeps its own next occurrence (`scheduled_run` in `monitoring_snapshot`). Missed occurrences collapse into one poll; a new occurrence waits until the previous poll finishes (`scan_pending`).
- **Check Now** (tray menu) polls every enabled repository now and retries any parked repository poll; a running poll gets one follow-up (`scan_requested`). Each repository keeps its schedule. The header **Monitoring** switch halts polling.
- A repository's schedule suspends while it is not schedulable (a parked poll counts unless it waits for a credential reconnect); resumes at its next occurrence. Editing its cron re-times only that repository.
- Legacy `global_scan` state folds into each repository on restore: shared next run, pending and request.
- Poll operation persists before the read. Restart keeps it. A waiting or interrupted poll whose window ran out before it could run (quit, sleep, account rate limit) restarts with a fresh window and keeps its cause; it never parks on the deadline alone. An account rate limit also extends the window of every poll on that account.
- Retry: transient failures back off (5s·2^n, cap 5m, full jitter; `src-tauri/src/retry.rs`), 3 failed attempts, then park. Window 15 min. Credential/permission failures park immediately. A parked poll is requeued at its repository's next occurrence (once per occurrence); credential failures wait for reconnect.
- A revision changing during a poll requeues that same poll after 30 seconds without spending an attempt or showing a failure. Saved PRs, cursors and the last successful poll stay unchanged until a complete fresh read.
- Rate limit (403/429): not a failure. The poll waits until Retry-After/reset, else seeded full jitter in [30s, 30s x2^n] (cap 15m, `retry::BACKPRESSURE`); no attempt spent, no failure shown. Other repos on the same account wait too, whatever their schedule (`backpressure.json`, survives restart). A successful read clears it. Parked home: Queue > **Needs attention** ([review-queue.md](review-queue.md)).
- Schedule health keeps pending/success/failure per account/repo read in `monitoring_snapshot`; no Status screen.
- Detection never clones, executes repo code, mutates GitHub or publishes.

## Admission

Per repository, set in the repo editor.

- **All pull requests**, or **Filtered pull requests**: ordered, sortable rules. First enabled matching rule decides.
- Rules: assigned to me (**Assigned to me for review**), **Mentioning me in comments**, include users, exclude users, include paths, exclude paths.
- Path rules: prefix or glob, e.g. `docs/`, `src/**/*.ts`.
- Repositories migrated once from old activation modes. Monitoring was switched off until reviewed.
- Repository Save is atomic. It reads no backlog. A separate polling cache can recover authorization after a failed write or restart. Sync failures are visible.
- Acting-account mention can admit a PR for primary follow-up: [provider-writes.md](provider-writes.md#mentions).

## Heartbeat (PR state)

Replaces head-SHA-only detection for tracked open PRs. Cadence is the repository's cron; there is no settle window.

| Step         | Rule                                                                                                                                                                             |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Read         | One batched GraphQL read per tick for tracked open PRs (`GithubClient::pr_snapshots`, 10 PRs per query). Cost about 1 point per PR; 5,000 points/h/user.                         |
| Normalize    | Sorted, no timestamps, `UNKNOWN` mergeable ignored, CI as `(name, status, conclusion)` array. The hash covers the review revision only: CI, mergeable and labels are left out.   |
| Same hash    | Equal to baseline or to pending: nothing. Check runs completing on the same head never queue, supersede or re-run teammates. Legacy full-snapshot hashes translate on read.      |
| New hash     | Records `pending` and bumps `state_seq`. Head change or reopen is a new Iteration; any other change keeps the Iteration.                                                         |
| Queue        | One entry per PR. A state that moves while queued only updates `pending`. A finished pass plus a new state adds one new entry.                                                   |
| Closed       | Merged or closed before its turn: leaves the queue, no run, Reviewed.                                                                                                            |
| Drafts       | Skipped unless the repository sets Review drafts.                                                                                                                                |
| Upgrade      | First read of a PR with a completed pass for its head sets the baseline. No re-review.                                                                                           |
| Consume      | A successful exit sets the baseline to the post-write state. Own comments and approvals never retrigger.                                                                         |
| Moved in run | State moved during a run: result tossed, error count +1, re-queued. Parks after 3 (the job's attempt budget).                                                                    |
| Own writes   | PR with an exit write in flight (packet batch posting, non-terminal exit write, approve/merge Prepared or Uncertain): no new state recorded. Head change or reopen still counts. |
| Reused run   | A queued or interrupted run takes the queue job's current `state_seq` at dispatch, so its exit consumes the state it read.                                                       |

Post-write consume (`exits::consume::settle_fresh`) compares the fresh read with the packet's `state_marks`. Only own writes (receipt comment ids, reply ids, threads we resolved, our approval): baseline = fresh, current state consumed, including a heartbeat bump that saw them. Any other change (new or edited comment, others' resolve, head, base, text, lifecycle, review decision, others' approvals): baseline = packet hash, so the next heartbeat queues one review. CI, mergeable and labels are not changes, except that a Ready sign-off (no later action to re-check CI) settles on the packet hash when the fresh read shows CI pending or failed.

Persisted in `state/pr-state.json`. Own-write baseline and consume/toss calls: `pr_state::consume`, `pr_state::toss`.

### Merge-readiness transitions

- Primary `wait` persists the prerequisite, reviewed snapshot, assignment and state sequence in `pr-state.json`, without clearance. No extra timer or AI session; the existing repository schedule reads the tracked PR and its checks.
- The packet records its fresh normalized `wait_snapshot` when Wait is offered; it may be newer than the last poll. Missing legacy evidence never fabricates a wait or clearance.
- Final Wait validation re-reads the snapshot. Pending-only CI progress is accepted using the same predicate as polling; the validated final snapshot is stored on the result without rewriting packet evidence. An intervening poll is absorbed only while CI remains pending and non-CI state is unchanged.
- Required-check names are read from branch protection with every snapshot. An absent required context is pending even when reported checks are complete or absent; reporting progress keeps the wait, while completion or failure wakes review. Policy changes invalidate the wait; partial check reads are not absence evidence.
- A required-review wait never substitutes for a missing approval this assignment can post itself; the offered approval remains the next clean-review exit.
- While waiting on CI, check progress/fan-out alone does not queue reviews. Completion (success or failure), head/base, discussion or other review-revision change wakes one fresh state; mergeable and labels do not. Required-review and draft changes also wake; every normal permission, finding and action gate still applies.
- Restart and missing/failed polls retain the wait. Closed/merged PRs retire it; removed tools/actions invalidate draft and required-review waits on polling; a CI wait needs only the primary role. Pause, disabled repositories and removed assignments cannot dispatch. No linked-PR dependency watching.

- Draft → ready changes the hash, even at the same head/base. CI does not: a primary whose sign-off or merge is blocked only by pending CI takes `wait`, and the wait wakes on completion. A completed pass gets fresh work on the next successful poll.
- CI wake: a consumed exit that is not a sign-off (`ready_for_human`, comments) while CI was not settled green (no checks reported, pending or red) at the exit-time read or the last poll arms `pr_state.ci_wake`. The first poll that sees CI settled green on the same review revision wakes the primary once, keeping `revision_seq` like a CI wait. Any new state, wait or later consume clears it; the woken primary saw green, so nothing re-arms and a later red → green does not wake again. Covers no rollup right after a push (Unknown → green) and a red run re-run green on the same head.
- A CI wait holds while no check is reported (GitHub may not have created them yet). `wait` is only offered on pending CI, so a repository with no CI never waits: its primary exits once and the PR stays quiet until a review-revision change; CI appearing and turning green later wakes it once.
- Every teammate must report for that state before the primary runs; a failed teammate still blocks. A CI wait woken on an unchanged review revision keeps that revision (`pr_state.revision_seq`): the primary runs again, teammate reports hold. No other report reuse across states.
- Any primary, whatever its authority or tool set, is offered `wait` (reason `ci`) when pending CI alone blocks its sign-off (`ci_blocks_only_sign_off`), judged as at packet build: own open findings resolved, behind base or conflicts never refusing. Green: it re-runs and signs off per authority (local Ready, approve, approve and merge). Red: the normal path.
- `lgtm` stays callable while pending CI alone denies it; its exit-time gate reads CI fresh, so green mid-run signs off. A local Ready sign-off needs CI still settled green at the final read: pending or red tosses it.
- Dispatch does not wait for CI: teammates and the primary run while checks are pending. Exit calls and native actions read CI fresh; pending CI denies sign-off and merge, red denies approve too.
- Packet preparation and exit calls read current provider gates. Only the primary's chosen `approve_and_merge` exit authorizes merge; offering it never overrides `ready_for_human`.
- Saved review evidence includes `packet.exits_denied` (exact blocker), `packet.exits_offered`, and `result.output.exit` (selected exit). The native action ledger retains intents and receipts.
- Native execution re-reads gates before intent and again before transmission. Changed head/base, draft, red CI, unresolved threads, queues and insufficient permissions still block.
- Fixture regression: `actions::tests::readiness` covers poll → teammate barrier → fresh packet → selected exit → approval → merge. No live provider writes or model inference.

## Lifecycle reads

- Complete open listing, then explicit reads for admitted active PRs missing from it.
- Reopens are found by later listings without refetching every terminal PR. Stable creation-order paging.
- Merged or closed PRs move to Reviewed. Cleanup rules: [active-results.md](active-results.md#terminal-cleanup).

## GitHub connection

- Connect in Settings > Accounts > Git Repository via the PR Sniper GitHub OAuth App; scope `repo` ([ADR-0003](adr/0003-github-browser-oauth.md)). Confirm identity. Select acting account + repo, Save.
- Verify in the repo editor. Uses saved bindings, not rename drafts. Retargeting a repo invalidates the connection. Copilot credentials never set the acting identity.
- Verify reads: `/user`, repo metadata, a real PR read. Read success is not comment capability. Fine-grained tokens: comment capability stays unverified. No mutation probe.
- Failures (auth, permission, rate limit, timeout, network, provider) are visible. No auto retry, no auto-enable. (Account connection flows only; queue and polling rate limits are backpressure, above.)
- HTTPS credential sits only in a sensitive authorization header. Redirects disabled. Diagnostics get fixed success/failure events only; never bodies, tokens or output.

## PR metadata read

- Rechecks account and remote repo ID. Reads all PR pages, per-PR changed-file pages, requested users/teams. Includes closed, merged, deleted-account and fork PRs.
- Errors, never partial success: head change mid-read, malformed page, duplicate entry, incomplete file count, more than 3,000 files, failed page (`src-tauri/src/github/metadata.rs`). Incomplete reads (duplicate entry, count mismatch) are transient and retry; size limits (`LimitExceeded`: more than 3,000 files, truncated tree, body cap) are permanent.
- No checkout, no execution. Metadata is not full diff text.
- Mention scan: top-level issue-comment pages, up to 1,000 comments, 1 MiB total body bytes, complete pages only; over a limit or duplicate ID = incomplete read, error (`github/conversation.rs`).

## Legacy CLI diagnostic

Only when the user explicitly authorizes. Prints public identity, capability and counts; never credentials.

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --example github_read -- jdylanmc/pr-sniper 6954990 --metadata
```
