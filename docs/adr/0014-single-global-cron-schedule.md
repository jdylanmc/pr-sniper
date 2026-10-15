# Poll on one global cron schedule

Status: superseded by [ADR 0024](0024-repository-schedules.md) (per-repository cron, one global time zone).

## Context

Per-repo and per-Agent timers multiplied provider reads, drifted, and burst after sleep.

## Decision

- One saved five-field cron. Default `*/15 * * * *`. Saved IANA timezone.
- Scheduler consumes cron. Dispatcher consumes capacity (ADR 0013). Separate.
- One read per account/repository binding fans out to Agent jobs.
- Bindings on the same remote drain serially.
- Missed ticks collapse to one check.
- **Check Now** coalesces. Never bypasses Retry-After or budget.
- DST: skipped time runs at first valid time; repeated time runs once.
- Schedule suspends when no repo is schedulable. Resumes at next occurrence.
- Legacy intervals stay readable, never silently converted.
- Detection never clones, executes, or mutates repo code.

## Consequences

- Cheap provider load, no catch-up burst.
- All repos share one cadence. No per-repo polling UI.
- New assignment waits for next global scan.
