# Run one review per head SHA

## Context

Pushes, polls, restarts, and comment activity all re-detect the same PR. Re-reviewing identical code wastes AI capacity and spams the PR with duplicate feedback.

## Decision

- One review per PR head SHA (revision). `job.head_sha` in `src-tauri/src/review.rs` keys the job.
- New head or base = new revision = permission for one more review. Old result goes Stale.
- Same revision: Agent only replies to new comments. No second full review.
- Agent context = PR description + changed files + ALL comments (+ prior feedback).
- Reopen = new iteration.
- Poll, restart, push cannot replay a job. Conversation trigger keys exclude responder role and head.

## Consequences

- Cheap, deterministic. Duplicate detection is a no-op.
- Force-push with identical tree still costs one review.
- A discarded, never-published batch (changed lens, diverged comments), or a completed review of a commenting assignment whose lens changed before any batch existed, earns one automatic re-review of that head per lens: `JobOperation::rereview_lens` holds the lens hash. A published receipt, a moved head or a closed PR never re-reviews. Replaces the old result in the Queue.
- Stale only on new head/base. Config drift or disabled repo does not stale a completed review.
- Merged/closed PRs move to Reviewed. Tracked PRs of disabled repos keep light lifecycle refresh.
