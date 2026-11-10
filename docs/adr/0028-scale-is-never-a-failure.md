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
- Azure DevOps: one `searchCriteria.creatorId` query per author, `searchCriteria.reviewerId` for the account, merged and de-duplicated. It detects no mentions (`CommentReader::top_comments` is empty), so Mentioned never widens its listing. IDs that are not identity GUIDs match no PR and are not queried. GitHub lists every open PR and ignores the scope.
- **Batched pass** (`PullRequestReader::poll_batch`, Azure DevOps): ≤ 10 pages of 100 per poll, newest first. `ListingCursor` (query, skip) persists in the poll cursor and resets when admission changes. A resumed pass reads the head page (PRs opened since) and backs off 20 places (closures since). Each batch is admitted as it arrives. A PR missed by a moving listing is read in the next pass.
- **Tracked PRs** missing from a batch are read by ID, ≤ 200 per poll, rotating by number; closed tracked PRs are probed for a reopen, ≤ 20 per poll. Sticky admission never depends on the listing.
- **Path facts** of a PR observed closed are dropped at once; of one listed on no page of a whole pass, when the pass ends (`ListingCursor::passes`, `PollCursor::fact_pass`).
- **Retry**: a listing page that fails for a reason a later poll can clear (network, timeout, 5xx, a malformed page) ends the batch; pages read are kept and the cursor stays there; no page read: the shared `retry::TRANSIENT` backoff by consecutive stalls, tracked reads go on. A throttled page (429, stated wait) ends the poll's reads, holds the organization for a stated wait, commits only the cursor at the batch's start (`PollResult::deferred`) and waits under `retry::BACKPRESSURE`; the next poll re-reads that batch. Never restarts from zero, never fails the check. Access, identity and repository failures (401, 403, 404, moved repository, refused query) surface as today.
- One PR that does not parse is skipped, not fatal. A discussion past a size limit on one PR is skipped like a malformed one (`ThreadSkipped`).
- **Reviews** read only their PR. Whole trees too large or slow for one request: head and base hold the changed files only, from the iteration changes' blob IDs (`objectId`, `originalObjectId`).

## Consequences

- A filter matching a few PRs on a very large repository polls in a few requests.
- All mode on a very large repository admits over several polls; new PRs show on the next poll.
- A listing that keeps failing retries forever with capped backoff and is not shown as a failure.
- On a very large Azure DevOps repository a review's tools see the changed files, not the whole tree.
- Follow-ups: GitHub's open listing is unbatched (no cap; one request per 100 PRs per poll); thread reads per admitted PR are unpaged; tree-too-large state is not remembered between reviews.
