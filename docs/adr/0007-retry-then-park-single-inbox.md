# Retry transient failures, then park in one inbox

## Context

Provider and AI failures are transient or permanent. Unbounded retry hides faults. A separate "blocked" state per cause multiplied UI states and left users guessing which one needs them.

## Decision

- Persist the operation (identity, attempt count, 15-minute deadline) before work. `RETRY_WINDOW_SECONDS` and `MAX_FAILED_ATTEMPTS = 3` live in `src-tauri/src/monitoring.rs`.
- Transient failure: backoff, retry. After 3 failed attempts or deadline: park (`ManualRetry`).
- Owned feedback changing during a review is a typed state toss, not a permanent
  failure: discard the assessment and duck evidence, count the attempt, and retry
  with fresh context through that same bounded budget. Relevant replies, closure
  and ownership remain checked; unrelated ledger/publication bookkeeping does not
  invalidate the captured contexts.
- One attempt-owned guard retains the first typed local rejection across
  preparation, runtime and duck reads, exit gates and final-state reads, including
  escaped client clones. It stays rejected even if inputs change back; final
  settlement never consumes the cause or re-evaluates policy after a provider
  error. Actual provider errors keep their own cause when no local guard refused.
- The outer review runtime commits its terminal error at its outcome selection,
  before abort/shutdown cleanup. Each poll that can select the outcome and the
  commit of a selected failure form one critical section against whole
  attempt-gate evaluations (re-entrant on the polling thread). Ordering rule: a
  refusal latched before that poll began (Tossed, or a pause/shutdown/cancel the
  runtime could have observed, including one that caused the failure) wins and a
  cancellation still refunds; any gate evaluation not finished by then waits and
  observes the committed failure, so late reads stay rejected but cannot replace
  it or refund its budget. Individual tool errors and duck verdicts are not
  commitments. Successful outcomes remain subject to finalization freshness checks.
- Rate limits (GitHub 403/429 primary + secondary, Copilot/model 429) are **backpressure**, not failure. No attempt, no failure budget, no park, never Needs attention. Work stays queued; its provider account is gated until Retry-After/reset, else 30s x2^n capped at 15m with deterministic jitter. A success clears the gate. See [ADR 0013](0013-shared-ai-capacity.md).
- Credential, permission, permanent failure: park at once.
- Superseded is not failure: a moved head, closed/merged/unadmitted PR or removed/replaced assignment, seen before or during a run, requeues without an attempt and never parks. It re-dispatches only after the repository's next successful poll observes the PR, which retargets or retires it ([ADR 0008](0008-one-review-per-head-sha.md)); a failing poll never loops it. A base move after an AI run waits for a poll that observed another base: at most one AI attempt per base.
- Every parked failure lands in one inbox: Queue > **Needs attention**. Cause chip: Credentials, Permission, Network, Failed.
- Row action: **Fix** for credentials (opens Accounts), **Retry** otherwise. **Retry all** pill.
- No user-facing "Blocked" state. Running tab shows active + queued only.
- User Retry starts a new operation (cooldown cycle 0). History kept.
- Auto-resume: resumable parked failures (transient, permission) requeue with a fresh 3-attempt budget after 30m, 1h, 2h, 4h, 8h; five cycles then human-only. Credentials resume when the GitHub or Copilot account connects again (a connect transition, or the first tick after start). Permanent never resumes. Superseded/closed/merged work is skipped.
- Agent-quality rejections ("execution failure") are Provider-class, so they retry. Not reading every file is not a rejection ([ADR 0020](0020-constrained-host-owned-review-tools.md)).
- Legacy parked `rate_limited` operations still load and resume through the cooldowns above; new code never produces one.
- AI budget starts at worker start. Cancel refunds only intentional cancel or discarded successful output. Real failure racing pause stays failure.
- Verified service outages suspend reviews and polls. Credential-free, redirect-free HEAD checks of the GitHub and Copilot API endpoints run at most once per service per 30-second interval (3-second connect / 5-second total bound); typed network/timeout failures also trigger confirmation. Any HTTP response proves reachability, not authorization. Authentication, permission, invalid-result and local runtime failures are not offline evidence.
- Suspension persists by provider/account. Retry deadlines, backoff and cooldown clocks freeze; genuine failure counts and causes remain. Workers keep their stopping slots until teardown, discard partial/late results, then wait. Recovery automatically resumes only eligible work; pause, cancellation, account bindings and publication receipts still apply. Running rows show Offline and the last successful reachability check.

## Consequences

- Restart-safe. No silent retry loops.
- A failed or admission-pending feedback observation is the same typed toss: the
  result is discarded and retried through the bounded budget, and no attempt starts
  while the repository's observation is unhealthy (no budget spent waiting).
- Historical packet reviews parked permanently by the exact owned-feedback-change
  or feedback-observation-failed rejection migrate once to the toss retry policy. Operation identity, spent
  attempts and deadline survive; expired windows enter the normal cooldown.
  Retired or missing work ends quietly; temporarily blocked live work keeps its
  operation and budget until ordinary dispatch gates permit it again.
  Legacy follow-ups and other permanent failures stay unchanged; no provider
  effects are replayed by this migration.
- Sleep/wake and network changes use the same bounded wall-clock checks: no catch-up burst and no OS network changes. Endpoint reachability is not a guarantee that every model route is healthy; a reachable endpoint leaves genuine request failures under the existing retry policy.
- Disabled or unassigned repositories with eligible tracked PRs retain reachability checks for their connected, exact account/repository binding. Recovery restores terminal lifecycle observation only; it never enables monitoring or admits reviews. Removed, unbound, mismatched and disconnected bindings add no lifecycle probe scope.
- AI cost is bounded: at most 3 attempts x 6 windows per failing work item before a human is needed.
- Cooldown state persists (serde-default `resume_cycle`, `resume_at`); old data loads.
- Repository polls (no queue item, no retry UI since 2026-10-14): a poll that was waiting or interrupted when its window ran out (quit, sleep, account rate limit) restarts with a fresh window, keeps its cause and never parks on the deadline alone. A parked poll is requeued at the next occurrence of its repository's schedule (one retry per occurrence; Credentials wait for reconnect) and by Check Now (any parked poll). A parked poll keeps its repository's schedule alive, except a credential-parked poll, which waits for reconnect ([ADR 0024](0024-repository-schedules.md)).
- Auto-resume covers review runs, repository poll operations and publication operations (same cooldowns; resume reconciles the original batch, never reposts). Legacy follow-up/primary-final operations are cancelled on startup ([ADR 0021](0021-review-packet.md)).
- Completed reviews stay Ready despite config drift or disabled repo.
- Rust-internal `blocked` reason may linger in `capacity`. Not surfaced.
- Code = 3 attempts total. Old prose said "three retries".
