# Polling and GitHub reads

Reference for scheduled detection, admission rules, the read-only GitHub connection and PR metadata reads. Decisions: [ADR-0007](adr/0007-retry-then-park-single-inbox.md), [ADR-0009](adr/0009-admission-filters-first-match-wins.md), [ADR-0014](adr/0014-single-global-cron-schedule.md).

## Schedule

- One global cron expression + timezone. One read per account/repo binding per run. Set in Settings > Preferences.
- **Check Now** (tray menu) runs an immediate scan. **Pause automation** halts polling.
- Schedule suspends when no repo is schedulable; resumes at the next occurrence.
- Poll operation persists before the read. Restart keeps it. Interruption past its deadline needs **Retry**.
- Retry: transient failures back off, 3 failed attempts, then park. Window 15 min. Credential/permission failures park immediately.
- Rate limit (403/429): not a failure. The poll waits until Retry-After/reset, else 30s x2^n (cap 15m); no attempt spent, no failure shown. Other repos on the same account wait too (`backpressure.json`, survives restart). A successful read clears it. Parked home: Queue > **Needs attention** ([review-queue.md](review-queue.md)).
- Schedule health keeps pending/success/failure per account/repo read. Surfaced in Status.
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

Replaces head-SHA-only detection for tracked open PRs. Cadence is the global cron; there is no settle window.

| Step         | Rule                                                                                                                                                                             |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Read         | One batched GraphQL read per tick for tracked open PRs (`GithubClient::pr_snapshots`, 10 PRs per query). Cost about 1 point per PR; 5,000 points/h/user.                         |
| Normalize    | Sorted, no timestamps, `UNKNOWN` mergeable ignored, CI as `(name, status, conclusion)` array. Hash compared, not fields.                                                         |
| Same hash    | Equal to baseline or to pending: nothing.                                                                                                                                        |
| New hash     | Records `pending` and bumps `state_seq`. Head change or reopen is a new Iteration; any other change keeps the Iteration.                                                         |
| Queue        | One entry per PR. A state that moves while queued only updates `pending`. A finished pass plus a new state adds one new entry.                                                   |
| Closed       | Merged or closed before its turn: leaves the queue, no run, Reviewed.                                                                                                            |
| Drafts       | Skipped unless the repository sets Review drafts.                                                                                                                                |
| Upgrade      | First read of a PR with a completed pass for its head sets the baseline. No re-review.                                                                                           |
| Consume      | A successful exit sets the baseline to the post-write state. Own comments and approvals never retrigger.                                                                         |
| Moved in run | State moved during a run: result tossed, error count +1, re-queued. Parks after 3 (the job's attempt budget).                                                                    |
| Own writes   | PR with an exit write in flight (packet batch posting, non-terminal exit write, approve/merge Prepared or Uncertain): no new state recorded. Head change or reopen still counts. |
| Reused run   | A queued or interrupted run takes the queue job's current `state_seq` at dispatch, so its exit consumes the state it read.                                                       |

Post-write consume (`exits::consume::settle_fresh`) compares the fresh read with the packet's `state_marks`. Only own writes (receipt comment ids, reply ids, threads we resolved, our approval): baseline = fresh, current state consumed, including a heartbeat bump that saw them. Any other change (new or edited comment, others' resolve, head, base, text, labels, CI, mergeable, review decision, others' approvals): baseline = packet hash, so the next heartbeat queues one review.

Persisted in `state/pr-state.json`. Own-write baseline and consume/toss calls: `pr_state::consume`, `pr_state::toss`.

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
- Errors, never partial success: head change mid-read, malformed page, duplicate entry, incomplete file count, more than 3,000 files, failed page (`src-tauri/src/github/metadata.rs`).
- No checkout, no execution. Metadata is not full diff text.
- Mention scan: top-level issue-comment pages, up to 1,000 comments, 1 MiB total body bytes, complete pages only; over a limit or duplicate ID = incomplete read, error (`github/conversation.rs`).

## Legacy CLI diagnostic

Only when the user explicitly authorizes. Prints public identity, capability and counts; never credentials.

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --example github_read -- jdylanmc/pr-sniper 6954990 --metadata
```
