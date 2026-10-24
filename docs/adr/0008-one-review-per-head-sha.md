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
- ~~Once-per-lens automatic re-review of a discarded batch.~~ Superseded by [ADR 0021](0021-review-packet.md): packet toss/re-queue. `JobOperation::rereview_lens` stays readable, never written.
- Stale only on new head/base. Config drift or disabled repo does not stale a completed review.
- Merged/closed PRs move to Reviewed. Tracked PRs of disabled repos keep light lifecycle refresh.

## Proposed change ([ADR 0021](0021-review-packet.md))

One review per head SHA generalizes to **one successful exit per PR state**. A state covers head, comments and edits; CI is read fresh by exit gates and waited on with `wait`, never a new state (amended 2026-10-23). Revisions and new comments are just new states. The queue holds the PR; the packet is built from its latest state when an agent takes it. A state is consumed when the primary's exit succeeds; teammate runs for that state are inputs and do not consume it. Detection is implemented: [polling.md](../polling.md#heartbeat-pr-state) (no shadow mode, no head-only fallback). Packet, exits and consume hooks are not.
