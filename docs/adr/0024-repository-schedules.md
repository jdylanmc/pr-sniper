# Poll each repository on its own cron schedule

Status: accepted. Supersedes [ADR 0014](0014-single-global-cron-schedule.md). Amends the repository-poll recovery of [ADR 0007](0007-retry-then-park-single-inbox.md).

## Context

Owner decision (2026-10-15): repositories need different cadences (busy repo every 15 min on work hours, quiet repo daily). One global cron forced every repository onto one cadence. ADR 0014's concerns stay: no per-Agent timers, no burst after sleep, no drift.

## Decision

- **One cron per repository.** `Repository.cron`: five-field cron, the single source of truth. Default for new repositories `*/30 * * * *`. Validated by the backend (parses, has an occurrence) on every Save.
- **One global time zone.** `Settings.time_zone` (IANA). Every repository cron is evaluated in it. Preferences keeps only AI capacity and time zone.
- **Builder, not a second format.** The repository editor builds the cron: Every (5, 10, 15, 30 min; 1, 2, 4 h), Hourly at `:MM`, Daily at `HH:MM` list, plus days of week and an hours window (not with Daily), or Advanced (raw cron). Rust `schedule::Builder` encodes and decodes; anything else opens as Advanced. Daily times must form one minute × hour grid (one cron).
- **Backend preview.** `preview_schedule` returns the cron, its builder and the next 3 runs in the saved zone. `describe_schedules` decodes saved crons for list values. No cron evaluation in TypeScript.
- **Per-repository scheduler state.** `ScheduleHealth` carries `scheduled_run` (next occurrence), `scan_pending` (owed poll), `scan_requested` (Check Now during a running poll). Each repository starts its own occurrences; missed occurrences collapse into one; a new one waits for the previous poll to finish.
- **Recovery per repository.** A parked poll retries once per occurrence of its own schedule; credential parks wait for reconnect; Check Now polls every enabled repository and retries any parked poll. Waiting/interrupted polls past their window get a fresh window. An account rate limit gates and extends every poll on that account, whatever its schedule.
- **Unchanged.** One read per account/repository binding, fanned out to Agent jobs. Same-remote bindings drain serially. Pause blocks all polling. Check Now never bypasses Retry-After or budget. DST: skipped time runs at the first valid time; repeated time runs once. Detection never clones, executes or mutates repository code. Editing a cron re-times only that repository and never invalidates an in-flight poll.
- **Migration** (load and whole-settings saves; idempotent; written once). A saved global cron becomes every repository's cron and its zone the saved time zone; the global field is removed. A file without a saved time zone predates this ADR; with no saved global schedule it ran on `*/15 * * * *` UTC. A legacy global interval keeps repository crons and moves its zone. Invalid legacy schedules are rejected, not dropped. Legacy `global_scan` monitoring state folds into each repository's health on restore. Legacy assignment schedules stay readable; new assignments carry none.

## Consequences

- Provider load scales with each repository's cadence; account backpressure still caps bursts when many repositories share a schedule.
- Repository rows show a compact schedule value (`Every 30 min`, `Mon–Fri 9–18 · every 15 min`); unrepresentable crons show as themselves.
- A new repository waits only for its own next occurrence, not a global scan.
- Older builds cannot read the new settings shape (no downgrade migration).
