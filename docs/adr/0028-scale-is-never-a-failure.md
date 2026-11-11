# 0028 — Scale is never a failure: narrowed, batched, resumable listing with shared retry

## Context

An Azure DevOps poll paged through every active PR and failed with `LimitExceeded` past 5,000. On a very large repository (more than 5,000 active PRs) a Filtered repository that matches a handful of PRs never completed a poll. The parked repository check also failed every tracked item of that repository, including explicitly watched PRs whose own reads succeeded. A review read both whole trees; on a very large repository that read timed out and parked the review as Network.

## Decision

- Repository size never fails a poll, a review or an item. No listing cap; a per-poll budget is a batch size.
- **Listing scope** (`ListingScope`, `admission::listing_scope`, `monitoring::listing_scope`): the PRs a poll lists, a superset of what admission can admit. Admission (`decide`) still runs on every listed PR, so first-match order and Exclude users are unchanged.

| Admission                 | Scope                                                                              |
| ------------------------- | ---------------------------------------------------------------------------------- |
| All                       | Every open PR                                                                      |
| Filtered                  | Enabled Include users authors, plus the account as reviewer if Assigned is enabled |
| Filtered, Mentioned on    | As above; a provider that detects mentions lists every open PR                     |
| Explicit                  | Nothing; tracked PRs only                                                          |
| Legacy, no watched author | Every open PR                                                                      |
| Legacy, watched authors   | Those authors, the account as reviewer with reviewer assignment, mentions          |

- Proof: `decide` admits only through Assigned (the account is a reviewer: in the reviewer query), Mentioned (a mention) or Include users (the author is listed: in that author's query). Exclude users only rejects. So every PR the whole listing would admit is in the scope.
- Azure DevOps: one `searchCriteria.creatorId` query per author, `searchCriteria.reviewerId` for the account, merged and de-duplicated. It detects no mentions (`CommentReader::top_comments` is empty), so Mentioned never widens its listing. IDs that are not identity GUIDs match no PR and are not queried.
- GitHub: the Search API, `repo:{repo} is:pr is:open` with `author:{login}` per author and `review-requested:{login}` for the account, ≤ 3 terms OR-ed per query (`advanced_search`). Logins resolve from user IDs per search (renames); a bot as `author:app/{name}`. `review-requested` also matches team requests: a superset of Assigned. Hits are read by number, so `decide` sees the same PR the listing would. A search past 1,000 results (the search cap), incomplete, or one that cannot name a user lists every open PR for the rest of that pass (`ListingCursor::wide`); the next pass searches again. Search allows 30 requests a minute: ≤ 3 search pages per poll.
- GitHub detects mentions, so Mentioning me (and legacy watched authors) lists every open PR. Not `mentions:@me`: a PR's first observation baselines its comments, so a PR first listed because of a mention would never be admitted by it; and the match is textual (`@login`, edits too), wider than GitHub's mention index.
- **Batched pass** (`PullRequestReader::poll_batch`, stepping shared in `provider::listing`): ≤ 10 pages of 100 per poll, newest first (GitHub: `pulls?state=open&sort=created&direction=desc`, numbered pages, one page of overlap). `ListingCursor` (query, skip) persists in the poll cursor and resets when admission changes. A resumed pass reads the head page (PRs opened since) and backs off 20 places (closures since). Each batch is admitted as it arrives. A PR missed by a moving listing is read in the next pass.
- **Tracked PRs** missing from a batch are read by ID, ≤ 200 per poll, rotating by number; closed tracked PRs are probed for a reopen, ≤ 20 per poll. Sticky admission never depends on the listing.
- **Path facts** of a PR observed closed are dropped at once; of one listed on no page of a whole pass, when the pass ends (`ListingCursor::passes`, `PollCursor::fact_pass`).
- **Retry**: a listing page that fails for a reason a later poll can clear (network, timeout, 5xx, a malformed page) ends the batch; pages read are kept and the cursor stays there; no page read: the shared `retry::TRANSIENT` backoff by consecutive stalls (`retry_at`), tracked reads go on. A throttled page (429, stated wait) is the poll's rate limit: shared backpressure gate for the account (no attempt spent, never Needs attention), a stated wait also holds the organization, nothing of the batch is committed, and the next poll resumes from the last committed cursor. Never restarts from zero, never fails the check. Access, identity and repository failures (401, 403, 404, moved repository, refused query) surface as today.
- One PR that does not parse is skipped, not fatal. A discussion past a size limit, or one that moved while read, on one PR is skipped like a malformed one (`ThreadSkipped`), including GitHub's mention scan. GitHub's batched heartbeat re-reads a failed batch per PR; a PR not readable whole (over 100 checks) has no state that tick.
- **Reviews** read only their PR. Whole trees too large or slow for one request: head and base hold the changed files only, from the iteration changes' object IDs (`objectId`, `originalObjectId`). A base kind is read (`items?path=`) when the head is not a blob or no earlier ID is named; otherwise a blob is assumed, and a failed read of it (patch or later content) reads the real kind. GitHub (recursive tree truncated past 100,000 entries): tree levels along the changed paths only; past the 3,000-file listing, the unlisted changes come from the bounded walk of differing subtrees (ADR 0027 step 3); cut off, the listed files only.

## Consequences

- A filter matching a few PRs on a very large repository polls in a few requests.
- All mode on a very large repository admits over several polls; new PRs show on the next poll. On GitHub that head page is also each new PR's first observation, so its mention baseline is set as before (≤ 100 PRs opened per poll).
- A listing that keeps failing retries forever with capped backoff and is not shown as a failure.
- On a very large Azure DevOps repository a review's tools see the changed files, not the whole tree.
- Follow-ups: Azure DevOps thread reads per admitted PR are unpaged; tree-too-large state is not remembered between reviews; GitHub's Settings metadata read still reads every PR, closed ones too; a GitHub PR past 1,000 top comments or review threads gets no mention or thread observation.
