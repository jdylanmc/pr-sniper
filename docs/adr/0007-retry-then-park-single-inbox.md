# Retry transient failures, then park in one inbox

## Context

Provider and AI failures are transient or permanent. Unbounded retry hides faults. A separate "blocked" state per cause multiplied UI states and left users guessing which one needs them.

## Decision

- Persist the operation (identity, attempt count, 15-minute deadline) before work. `RETRY_WINDOW_SECONDS` and `MAX_FAILED_ATTEMPTS = 3` live in `src-tauri/src/monitoring.rs`.
- Transient failure: backoff, retry. After 3 failed attempts or deadline: park (`ManualRetry`).
- Rate limits (GitHub 403/429 primary + secondary, Copilot/model 429) are **backpressure**, not failure. No attempt, no failure budget, no park, never Needs attention. Work stays queued; its provider account is gated until Retry-After/reset, else 30s x2^n capped at 15m with deterministic jitter. A success clears the gate. See [ADR 0013](0013-shared-ai-capacity.md).
- Credential, permission, permanent failure: park at once.
- Every parked failure lands in one inbox: Queue > **Needs attention**. Cause chip: Credentials, Permission, Network, Failed.
- Row action: **Fix** for credentials (opens Accounts), **Retry** otherwise. **Retry all** pill.
- No user-facing "Blocked" state. Running tab shows active + queued only.
- User Retry starts a new operation (cooldown cycle 0). History kept.
- Auto-resume: resumable parked failures (transient, permission) requeue with a fresh 3-attempt budget after 30m, 1h, 2h, 4h, 8h; five cycles then human-only. Credentials resume when the GitHub or Copilot account connects again (a connect transition, or the first tick after start). Permanent never resumes. Superseded/closed/merged work is skipped.
- Agent-quality rejections ("execution failure") are Provider-class, so they retry. Not reading every file is not a rejection ([ADR 0020](0020-constrained-host-owned-review-tools.md)).
- Legacy parked `rate_limited` operations still load and resume through the cooldowns above; new code never produces one.
- AI budget starts at worker start. Cancel refunds only intentional cancel or discarded successful output. Real failure racing pause stays failure.

## Consequences

- Restart-safe. No silent retry loops.
- AI cost is bounded: at most 3 attempts x 6 windows per failing work item before a human is needed.
- Cooldown state persists (serde-default `resume_cycle`, `resume_at`); old data loads.
- Repository-level poll failures without a queue item have no manual retry surface since the Status screen was removed (2026-10-14); they rely on auto-resume.
- Auto-resume covers review runs, repository poll operations and publication operations (same cooldowns; resume reconciles the original batch, never reposts). Legacy follow-up/primary-final operations are cancelled on startup ([ADR 0021](0021-review-packet.md)).
- Completed reviews stay Ready despite config drift or disabled repo.
- Rust-internal `blocked` reason may linger in `capacity`. Not surfaced.
- Code = 3 attempts total. Old prose said "three retries".
