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
- Schedule health keeps pending/success/failure per account/repo read in `monitoring_snapshot`; no Status screen. A malformed read names its response in `last_failure`, e.g. `InvalidResponse(IterationChanges)`.
- Detection never clones, executes repo code, mutates GitHub or publishes.

## Admission

Per repository, set in the repo editor.

- **All pull requests**, or **Filtered pull requests**: ordered, sortable rules. First enabled matching rule decides.
- Rules: assigned to me (**Assigned to me for review**), **Mentioning me in comments**, include users, exclude users. Path scoping is the separate [Paths](#paths) gate; old path rules migrated there (enabled ones only; a Filtered config left with only an include path rule becomes All).
- Repositories migrated once from old activation modes. Monitoring was switched off until reviewed.
- Repository Save is atomic. It reads no backlog. A separate polling cache can recover authorization after a failed write or restart. Sync failures are visible.
- Acting-account mention can admit a PR for primary follow-up: [provider-writes.md](provider-writes.md#mentions).

## Paths

[ADR-0027](adr/0027-path-globs.md). Globs: include patterns plus `!pattern` excludes, gitignore-style, last match wins per file (either name of a rename); only excludes = everything else. Pattern syntax as admission, 1–256 chars; ≤ 200 per list (both old path rules fit).

| Glob                        | Rule                                                                                                                                                                                                                             |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Repository `paths` (gate)   | AND on top of admission and explicit intake. Re-evaluated per revision: a tracked PR whose new revision does not match gets no new work; existing entries stay. Empty: no gate.                                                  |
| `paths_mode: "or"`          | Rules admit, or a changed file matches; Exclude users beats a match. Paths never block. No effect in Explicit. Lists every open PR; facts for ≤ 100 other PRs per poll, newest first. [ADR-0029](adr/0029-paths-or-admission.md) |
| Secondary `paths` (trigger) | Files changed since that assignment's last completed review of the PR (none: whole PR). No match: entry `skipped_no_matching_files`, item **Skipped: no matching files**.                                                        |
| Secondary `paths` (scope)   | A triggered review sees only matching changed files (whole PR, same patterns). Comments/findings elsewhere: exit refused `out_of_scope`. `read_source` reads any file.                                                           |
| Primary                     | None: reviews everything the gate admits. Skips never block its barrier, exits, readiness, approval or merge.                                                                                                                    |

Facts per revision (`monitoring.json` `path_facts`): read once per new head, base (retarget) or changed input; same revision and inputs are reused.

Complete changed-file list ladder; first complete wins, no file-count limit of ours:

| Step                | GitHub                                                                                                                                                                                                     | Azure DevOps                                                                                                  |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| 1. Listing          | `changed_files` (every new head); ≤ 3,000: `/pulls/{n}/files` paged to the end when a glob needs it. Incremental: `compare/{old}...{head}` (not `ahead` or missing old head: whole PR; 300 files: step 2). | Iteration changes paged to the end (`$compareTo` 0 or the old head's iteration). No reported total: complete. |
| 2. Tree diff        | `git/trees/{sha}?recursive=1` of merge base (`compare` `merge_base_commit`) or old head and head; blob sha/mode differences.                                                                               | `items?recursionLevel=Full` of both commits.                                                                  |
| 3. Scoped tree diff | Globs only. Level-by-level walk under each include's fixed prefix, only into differing subtrees (≤ 5,000 tree reads).                                                                                      | Not needed (uncapped listing).                                                                                |
| 4. Park             | No review; Queue › Needs attention **Can't see all N changed files: needs a human** with the strategies tried; next revision re-evaluates.                                                                 |                                                                                                               |

- Without globs only completeness is checked: the count, and steps 2 then 4 when it exceeds the listing cap.
- A scoped list (step 3) can keep a PR out; one it admits parks (step 4), since the review needs every file.
- The review reads the same list: GitHub past 3,000 files keeps the listed files' patches and adds the rest of the merge-base tree diff without a patch (no addressable line). A whole tree GitHub truncates: the review's trees hold only the changed paths, read level by level, and the unlisted rest comes from the step 3 walk over every differing subtree (cut off: listed files only). Azure DevOps reviews every iteration change; past 300 files, no patch.
- An incremental read that cannot complete falls back to the whole-PR list (may run, never misses).
- Rate limits and network failures retry under the poll's retry policy. No read failure decides a gate, a skip or a park.
- One PR whose reads are malformed, too large, cut short or moved while read waits for the next poll (`PathFactsSkipped`), which reads it by ID (`ListingCursor::held`) wherever the listing resumes; its mention, thread and owned-feedback observations wait with it; the rest are admitted and the poll succeeds. Azure DevOps: a deleted file may have no item path (`"path": null`); its `originalPath` names it.

## Heartbeat (PR state)

Replaces head-SHA-only detection for tracked open PRs. Cadence is the repository's cron; there is no settle window.

| Step              | Rule                                                                                                                                                                                                                                                        |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Read              | One batched GraphQL read per tick for tracked open PRs (`GithubClient::pr_snapshots`, 10 PRs per query). Cost about 1 point per PR; 5,000 points/h/user. A batch error is re-read per PR; a PR not readable whole (over 100 checks) has no state this tick. |
| Normalize         | Sorted, no timestamps, `UNKNOWN` mergeable ignored, CI as `(name, status, conclusion)` array. The hash covers the review revision only: CI, mergeable and labels are left out.                                                                              |
| Same hash         | Equal to baseline or to pending: nothing. Check runs completing on the same head never queue, supersede or re-run teammates. Legacy full-snapshot hashes translate on read.                                                                                 |
| New hash          | Records `pending` and bumps `state_seq`. Head change or reopen is a new Iteration; any other change keeps the Iteration.                                                                                                                                    |
| Queue             | One entry per PR. A state that moves while queued only updates `pending`. A finished pass plus a new state adds one new entry, trigger `state_changed`.                                                                                                     |
| Closed            | Merged or closed before its turn: leaves the queue, no run, Reviewed.                                                                                                                                                                                       |
| Drafts            | Skipped unless the repository sets Review drafts.                                                                                                                                                                                                           |
| Upgrade           | First read of a PR with a completed pass for its head sets the baseline. No re-review.                                                                                                                                                                      |
| Consume           | A successful exit sets the baseline to the post-write state. Own comments and approvals never retrigger.                                                                                                                                                    |
| Moved in run      | State moved during a run: result tossed, error count +1, re-queued. Parks after 3 (the job's attempt budget).                                                                                                                                               |
| Closed on arrival | A thread first seen already resolved (status/policy bot) is no review input: a move made only of such threads joins the baseline or `pending`. Recorded as it arrived in `quiet_threads`; reopening it, a reply or an edit in it is a change.               |
| Own writes        | PR with an exit write in flight (packet batch posting, non-terminal exit write, approve/merge Prepared or Uncertain): no new state recorded. Head change or reopen still counts.                                                                            |
| Reused run        | A queued or interrupted run takes the queue job's current `state_seq` at dispatch, so its exit consumes the state it read.                                                                                                                                  |

Post-write consume (`exits::consume::settle_fresh`) compares the fresh read with the packet's `state_marks`. Only own writes (receipt comment ids, reply ids, threads we resolved, our approval): baseline = fresh, current state consumed, including a heartbeat bump that saw them. Our approval is matched by the account the provider names reviewers with: login on GitHub, account id on Azure DevOps. Threads that arrived already resolved are not changes. Any other change (new or edited comment, others' resolve, head, base, text, lifecycle, review decision, others' approvals): baseline = packet hash, so the next heartbeat queues one review. CI, mergeable and labels are not changes, except that a Ready sign-off (no later action to re-check CI) settles on the packet hash when the fresh read shows CI pending or failed.

Persisted in `state/pr-state.json`. Own-write baseline and consume/toss calls: `pr_state::consume`, `pr_state::toss`.

### Merge-readiness transitions

- Primary `wait` persists the prerequisite, reviewed snapshot, assignment and state sequence in `pr-state.json`, without clearance. No extra timer or AI session; the existing repository schedule reads the tracked PR and its checks.
- The packet records its fresh normalized `wait_snapshot` when Wait is offered; it may be newer than the last poll. Missing legacy evidence never fabricates a wait or clearance.
- Final Wait validation re-reads the snapshot. Pending-only CI progress is accepted using the same predicate as polling; the validated final snapshot is stored on the result without rewriting packet evidence. An intervening poll is absorbed only while CI remains pending and non-CI state is unchanged.
- Required-check names are read from branch protection with every snapshot. An absent required context is pending even when reported checks are complete or absent; reporting progress keeps the wait, while completion or failure wakes review. Policy changes invalidate the wait; partial check reads are not absence evidence.
- A required-review wait never substitutes for a missing approval this assignment can post itself; the offered approval remains the next clean-review exit.
- While waiting on CI, check progress/fan-out alone does not queue reviews. Completion (success or failure), head/base, discussion or other review-revision change wakes one fresh state; mergeable and labels do not. Required-review and draft changes also wake; every normal permission, finding and action gate still applies.
- Restart and missing/failed polls retain the wait. Closed/merged PRs retire it; removed tools/actions invalidate draft and required-review waits on polling; a CI wait needs only the primary role. Pause, disabled repositories and removed assignments cannot dispatch. No linked-PR dependency watching.

- **Parked LGTM**: a primary with approve or merge authority whose sign-off is blocked only by pending CI calls `lgtm`; it is accepted and parked on that revision (see [Sign-off](review-packet.md#sign-off)). No wait record and no AI wake: the action pipeline re-observes the PR each heartbeat and acts once CI is green on the same head and state sequence. A new head, base change or any review-revision change (human comment or reply, new or reopened thread, review, description) bumps the state and revokes it before its first effect. The action heartbeat also compares human comments, replies, threads and others' reviews with the adopted observation (own marked writes and the sign-off's own resolutions excluded), so a change between polls revokes it too; red CI before its first effect revokes it and bumps the state (pr-state saved first, so a crash fails closed); after an effect, red CI only blocks the merge until green. Either way the next scan queues a normal review. Restart keeps it (`actions.json`, `pr-state.json`).
- Draft → ready changes the hash, even at the same head/base. CI does not: a primary with no approve or merge authority whose local sign-off is blocked only by pending CI takes `wait`, and the wait wakes on completion. A completed pass gets fresh work on the next successful poll.
- CI wake: a consumed local `ready_for_human` while CI was not settled green (no checks reported, pending or red) at the exit-time read or the last poll arms `pr_state.ci_wake`. The first poll that sees CI settled green on the same review revision wakes the primary once, keeping `revision_seq` like a CI wait. Any new state, wait or later consume clears it; the woken primary saw green, so nothing re-arms and a later red → green does not wake again. Covers no rollup right after a push (Unknown → green) and a red run re-run green on the same head. Provider writes (comments, sign-off) never arm it: published findings end the revision until a new head (no **Status chatter**).
- A CI wait holds while no check is reported (GitHub may not have created them yet). `wait` is only offered on pending CI, so a repository with no CI never waits: its primary exits once and the PR stays quiet until a review-revision change; CI appearing and turning green later wakes it once.
- Every teammate must report for that state before the primary runs; a failed teammate still blocks. A CI wait woken on an unchanged review revision keeps that revision (`pr_state.revision_seq`): the primary runs again, teammate reports hold. No other report reuse across states.
- Any primary, whatever its authority or tool set, is offered `wait` (reason `ci`) when pending CI alone blocks its sign-off (`ci_blocks_only_sign_off`), judged as at packet build: own open findings resolved, behind base or conflicts never refusing. Green: it re-runs and signs off per authority (local Ready, approve, approve and merge). Red: the normal path.
- `lgtm` stays callable while pending CI alone denies it; its exit-time gate reads CI fresh, so green mid-run signs off. A local Ready sign-off needs CI still settled green at the final read: pending or red tosses it.
- Dispatch does not wait for CI: teammates and the primary run while checks are pending. Exit calls and native actions read CI fresh; pending CI denies sign-off and merge, red denies approve too.
- Packet preparation and exit calls read current provider gates. Only the primary's chosen `approve_and_merge` exit authorizes merge; offering it never overrides `ready_for_human`.
- Saved review evidence includes `packet.exits_denied` (exact blocker), `packet.exits_offered`, and `result.output.exit` (selected exit). The native action ledger retains intents and receipts.
- Native execution re-reads gates before intent and again before transmission. Changed head/base, draft, red CI, unresolved threads, queues and insufficient permissions still block.
- Fixture regression: `actions::tests::readiness` covers poll → teammate barrier → fresh packet → selected exit → approval → merge. No live provider writes or model inference.

## Azure DevOps

Same schedule, admission, retry and parking as GitHub, through the provider read interfaces (`azure_devops/read/pulls.rs`). Auth: Azure CLI token source ([ADR-0026](adr/0026-azure-devops-auth-via-azure-cli.md)). Live acceptance: [tests/azure-devops-acceptance.md](../tests/azure-devops-acceptance.md).

| Step          | Rule                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| List          | `GET …/pullrequests?searchCriteria.status=active`, newest first, `$top`=100 / `$skip`. Only the [listing scope](adr/0028-scale-is-never-a-failure.md): one `searchCriteria.creatorId` query per Include users person, `searchCriteria.reviewerId` = account for Assigned, none in Explicit; All = every PR. Batched: ≤ 10 pages per poll, resumable cursor, no cap; duplicates dropped. |
| Tracked       | Missing from the batch: read on its own (`completed` → Merged, `abandoned` → Closed), ≤ 200 per poll, rotating; closed ones probed for a reopen, ≤ 20. Never needs the listing. A throttled listing is the poll's rate limit; the pass resumes from the last committed cursor.                                                                                                          |
| Revision      | Head = `lastMergeSourceCommit` (latest iteration's source commit): each push is a new Iteration. Base = `lastMergeTargetCommit`.                                                                                                                                                                                                                                                        |
| Drafts        | `isDraft`: skipped unless Review drafts.                                                                                                                                                                                                                                                                                                                                                |
| Admission     | Author, include/exclude users by identity GUID; Assigned = PR reviewer (groups are requested teams). No mention scan. [Paths](#paths) read iteration changes.                                                                                                                                                                                                                           |
| Changed files | Latest iteration (must equal head, else `RevisionChanged`), `.../iterations/{n}/changes?$compareTo=0`, `nextSkip` pages to the end (no file limit; a `nextSkip` that does not advance = `IncompleteRead`).                                                                                                                                                                              |
| Content       | Trees: `items?recursionLevel=Full&versionDescriptor.version={sha}&versionDescriptor.versionType=commit`; one too large or slow for the request: changed files only, from iteration change blob IDs. Files: `blobs/{objectId}?$format=octetstream`. Over 8 MiB = `LimitExceeded`.                                                                                                        |
| Patches       | No provider patch text: merge base and head blobs diffed locally (3 context lines), first 300 files, ≤ 1 MiB text per side; others have no addressable line.                                                                                                                                                                                                                            |
| Requests      | `X-TFS-FedAuthRedirect: Suppress`, no redirects; 401, 203, 3xx or a sign-in page (`X-TFS-FedAuth*`) drops the token and retries once; any other `text/html` answer to a JSON request = Network (blobs are returned unchanged). [Throttling](#azure-devops-throttling).                                                                                                                  |
| Offline       | A Network/Timeout failure probes `HEAD https://dev.azure.com/` (any response = reachable). Unreachable suspends the account's polls and reviews with no attempt spent; a reachable 30-second check resumes them. GitHub reachability never holds or frees them.                                                                                                                         |
| Responses     | Omitted enum defaults read as their default (status `state` = `notSet`, policy `status` = `queued`, change type `none`). A body that still does not parse = `MalformedResponse(<response>)`: typed provenance, shown as Invalid response.                                                                                                                                               |
| Heartbeat     | Tracked open PRs: threads, reviewer votes, policy evaluations and PR statuses ([provider writes](provider-writes.md#azure-devops)).                                                                                                                                                                                                                                                     |
| Explicit PRs  | `…/_git/{repo}/pullrequest/{id}` on `dev.azure.com` or `{org}.visualstudio.com` (optional `DefaultCollection`). Resolved to `{org}/{project ID}/{repository ID}`; must equal the saved repository, else `target_mismatch`. Same gates as GitHub.                                                                                                                                        |
| Reviews       | Run and publish like GitHub ([provider writes](provider-writes.md#azure-devops)). Approve is a reviewer vote (approve-only); approve-and-merge never.                                                                                                                                                                                                                                   |

Parked causes (Queue › **Needs attention**):

| Cause       | From                                                                                                                     |
| ----------- | ------------------------------------------------------------------------------------------------------------------------ |
| Credentials | NotLoggedIn, NoAccountInTenant, TenantNotFound, InteractionRequired, AzMissing/AzTooOld, 401 after retry, wrong identity |
| Permission  | TenantAccessBlocked, 403, 404                                                                                            |
| Network     | Network, Timeout (backoff, then park; auto-resume never runs out)                                                        |
| Failed      | Other, MalformedOutput                                                                                                   |

A credential failure marks the bound identity disconnected: no further poll or Azure CLI run until Settings › Azure DevOps sees it connected again, which resumes the parked poll once.

### Azure DevOps throttling

| Signal                                                          | Effect                                                                                         |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| 429; a read's 503 with a stated wait                            | Rate limit: shared backpressure gate, no attempt spent, never parked as a failure              |
| `Retry-After`                                                   | Delta-seconds or HTTP-date                                                                     |
| `X-RateLimit-Remaining` ≤ 0                                     | Wait until `X-RateLimit-Reset`                                                                 |
| 2xx with `Retry-After`, `X-RateLimit-Delay` or exhausted budget | Next request of the organization held; ≤ 30s pauses inline, longer = rate limit with that wait |
| Stated wait                                                     | Wins over jitter (`retry::BACKPRESSURE` otherwise), capped at 24h                              |
| Write's 503                                                     | Provider failure, uncertain: the write may have landed; reconciles before any retry            |

## GitHub listing

[ADR 0028](adr/0028-scale-is-never-a-failure.md) (`github/listing.rs`, shared stepping `provider/listing.rs`).

| Scope                                    | Query                                                                                                                                                                      |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| All; Mentioning me on; legacy authors    | `pulls?state=open&sort=created&direction=desc&per_page=100&page=N`: ≤ 10 pages per poll                                                                                    |
| Include users, Assigned to me            | `search/issues?q=repo:{repo} is:pr is:open (author:{login} OR review-requested:{login})`, ≤ 3 terms per query (`advanced_search`), ≤ 3 pages per poll; hits read by number |
| Search over 1,000, incomplete or refused | That pass lists every open PR (`wide`); the next pass searches again                                                                                                       |
| Explicit                                 | Nothing; tracked PRs only                                                                                                                                                  |

- Logins resolved from user IDs each search (`/user/{id}`); a bot author searches as `author:app/{name}`.
- Mentions are not searched: a PR's first observation baselines its comments, and the textual match is wider than GitHub's mention index.
- Resumed pass: head page plus one page of overlap. Duplicates dropped; a PR missed by a moving listing is read next pass. A PR that does not parse is skipped.
- Network, timeout, 5xx or a malformed page: pages read are kept; no page read backs off (`retry::TRANSIENT`), tracked reads go on. 403/429 rate limit: backpressure, nothing committed, resumes from the last committed cursor. Only auth, permission and repository failures surface.

## Lifecycle reads

- Batched listing, then explicit reads for admitted active PRs missing from the batch: ≤ 200 per poll, rotating.
- Reopens are found by later listings without refetching every terminal PR. Closed tracked PRs are also probed by ID, ≤ 20 per poll, rotating: a narrowed scope may not list them.
- Merged or closed PRs move to Reviewed. Cleanup rules: [active-results.md](active-results.md#terminal-cleanup).

## GitHub connection

- Connect in Settings > Accounts > Git Repository via the PR Sniper GitHub OAuth App; scope `repo` ([ADR-0003](adr/0003-github-browser-oauth.md)). Confirm identity. Select acting account + repo, Save.
- Verify in the repo editor. Uses saved bindings, not rename drafts. Retargeting a repo invalidates the connection. Copilot credentials never set the acting identity.
- Verify reads: `/user`, repo metadata, a real PR read. Read success is not comment capability. Fine-grained tokens: comment capability stays unverified. No mutation probe.
- Failures (auth, permission, rate limit, timeout, network, provider) are visible. No auto retry, no auto-enable. (Account connection flows only; queue and polling rate limits are backpressure, above.)
- HTTPS credential sits only in a sensitive authorization header. Redirects disabled. Diagnostics get fixed success/failure events only; never bodies, tokens or output.

## PR metadata read

- Rechecks account and remote repo ID. Reads all PR pages, per-PR changed-file pages, requested users/teams. Includes closed, merged, deleted-account and fork PRs.
- Errors, never partial success: head change mid-read, malformed page, incomplete file count, failed page (`src-tauri/src/github/metadata.rs`). A PR repeated by a moving listing is read once; past 3,000 files, the files GitHub lists.
- No checkout, no execution. Metadata is not full diff text.
- Mention scan: top-level issue-comment pages, up to 1,000 comments, 1 MiB total body bytes, complete pages only (`github/conversation.rs`). Over a limit, a duplicate ID or a moved head skips that PR's mention and thread reads this poll (`ThreadSkipped`); the repository scan goes on.

## Legacy CLI diagnostic

Only when the user explicitly authorizes. Prints public identity, capability and counts; never credentials.

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --example github_read -- jdylanmc/pr-sniper 6954990 --metadata
```
