# Retry transient failures, then park in one inbox

## Context

Provider and AI failures are transient or permanent. Unbounded retry hides faults. A separate "blocked" state per cause multiplied UI states and left users guessing which one needs them.

## Decision

- Persist the operation (identity, attempt count, 15-minute deadline) before work. `RETRY_WINDOW_SECONDS` and `MAX_FAILED_ATTEMPTS = 3` live in `src-tauri/src/monitoring.rs`.
- Transient failure: backoff, retry. After 3 failed attempts or deadline: park (`ManualRetry`).
- Credential, permission, permanent failure: park at once.
- Every parked failure lands in one inbox: Queue > **Needs attention**. Cause chip: Credentials, Permission, Network, Failed.
- Row action: **Fix** for credentials (opens Accounts), **Retry** otherwise. **Retry all** pill.
- No user-facing "Blocked" state. Running tab shows active + queued only.
- User Retry or Fix starts a new operation. History kept. Resume never resets counts.
- AI budget starts at worker start. Cancel refunds only intentional cancel or discarded successful output. Real failure racing pause stays failure.

## Consequences

- Restart-safe. No silent retry loops.
- Parked work waits for a human. Nothing auto-resumes.
- Completed reviews stay Ready despite config drift or disabled repo.
- Rust-internal `blocked` reason may linger in `capacity`. Not surfaced.
- Code = 3 attempts total. Old prose said "three retries".
