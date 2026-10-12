# Polling and GitHub reads

Reference for scheduled detection, admission rules, the read-only GitHub connection and PR metadata reads. Decisions: [ADR-0007](adr/0007-retry-then-park-single-inbox.md), [ADR-0009](adr/0009-admission-filters-first-match-wins.md), [ADR-0014](adr/0014-single-global-cron-schedule.md).

## Schedule

- One global cron expression + timezone. One read per account/repo binding per run. Set in Settings > Preferences.
- **Check Now** (tray menu) runs an immediate scan. **Pause automation** halts polling.
- Schedule suspends when no repo is schedulable; resumes at the next occurrence.
- Poll operation persists before the read. Restart keeps it. Interruption past its deadline needs **Retry**.
- Retry: transient failures back off, 3 failed attempts, then park. Window 15 min. Credential/permission failures park immediately. Parked home: Queue > **Needs attention** ([review-queue.md](review-queue.md)).
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

## Lifecycle reads

- Complete open listing, then explicit reads for admitted active PRs missing from it.
- Reopens are found by later listings without refetching every terminal PR. Stable creation-order paging.
- Merged or closed PRs move to Reviewed. Cleanup rules: [active-results.md](active-results.md#terminal-cleanup).

## GitHub connection

- Connect in Settings > Accounts > Git Repository via the PR Sniper GitHub OAuth App; scope `repo` ([ADR-0003](adr/0003-github-browser-oauth.md)). Confirm identity. Select acting account + repo, Save.
- Verify in the repo editor. Uses saved bindings, not rename drafts. Retargeting a repo invalidates the connection. Copilot credentials never set the acting identity.
- Verify reads: `/user`, repo metadata, a real PR read. Read success is not comment capability. Fine-grained tokens: comment capability stays unverified. No mutation probe.
- Failures (auth, permission, rate limit, timeout, network, provider) are visible. No auto retry, no auto-enable.
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
