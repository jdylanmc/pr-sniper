# Scheduled, adaptive observation

Shepherd loads this before taking/resuming custody. It owns cadence/wakeups,
not readiness or new service authority.
[LIFECYCLE](../squadron/LIFECYCLE.md) owns custody acceptance/retirement.
Use the existing per-delivery custody/session record throughout.

## Establish a real observer

At authorized kickoff, record bounded job: PR scopes, owner, permitted
observation/maintenance, lifetime, stop conditions, scheduler grant and human
cadence override. Tools grant no service setup, installation, or unrelated jobs.
Observation-only assignments remain PR-read-only.

Authorized supported scheduler available: **establish and use it**; prefer
same-persistent-agent heartbeat/custody. First inspect owned wakeups; verify exact
job ID, agent/project/workspace binding, stored cron/timezone, next wakeup.
Observe now; verify subsequent wakes resume actual duties. Report missed wakes.
Neither local records, creation responses, nor stored cron prove observation.

A **fresh agent each run** is not a heartbeat. Require accessible durable state,
verified shared ownership/serialization, and existing repository project/worktree
workspace mapping under [WORKSPACE](../ship/WORKSPACE.md). Inspect schemas;
never invent `create_schedule` arguments or assume inherited cwd. Before new-run
observation/writes, reconcile survivors/partial work. Unproven exclusive ownership
blocks dispatch. After accepted return, retire finished run agents, not schedules
still needed.

No shell busy/sleep substitute for supported scheduling. If unavailable/
unauthorized, report the limit; use supported session-attached interruptible
waits only while running. Otherwise record **monitoring stopped**, last observation,
resumption action. Never install schedulers, widen permissions, or promise
unattended monitoring. External lifetime needs authority; honor explicit stop/access
boundaries at every wake.

## Meaningful snapshots and consecutive quiet observations

Observe immediately at takeover/resume. Defaults: **1, 5, 15 minutes**; start at
1 minute, cap at 15. Record/honor human overrides; never silently migrate them.

Project complete provider observations into stable per-PR snapshots:
- Source/target repository, literal refs/commits.
- Review/comment/finding identities, content, resolution; exposed edits/deletions.
- Required check/status identities/conclusions.
- Draft/open/merged/closed state, mergeability, policies, actual readiness blockers.

Sort unordered collections. Exclude observation timestamps, volatile API envelopes,
job logs, polling counters, incidental order. Keep observation time separately;
fetch logs for diagnosis only, never inactivity comparison. Missing pages, unknown
required status, or incomplete review/check coverage are incomplete observations;
never replace the last complete snapshot with them.

Transition on actual observations, never elapsed ticks:

| Observation | Quiet streak and desired cadence |
| --- | --- |
| First complete baseline; no verified previous snapshot | Save baseline: streak 0, 1 minute; no unchanged history. |
| Complete successful unchanged observation; no failed validation/unresolved repair | Increment streak. Exactly 30 at 1 minute: change to 5 minutes, reset 0; exactly 30 at 5 minutes: change to 15, reset 0. |
| Complete successful unchanged observation at 15 minutes | Cap at 15; saturate streak at 30, no further cadence increase. |
| Meaningful change | Save complete snapshot when available; reset streak 0, desired cadence 1 minute; then reconcile/act within authority. |
| Incomplete/failed observation, unknown required state, failed validation, unresolved repair | Break streak to 0; no slower stage/inactivity claim. Retain desired stage unless meaningful change detected. |
| Cadence/binding update pending, failed, unverified | Streak 0 until effective cadence/binding reconciled. Old-period observations cannot earn another slower stage by assuming update success. |

Baseline plus 30 successful unchanged observations moves to 5 minutes at
observation 31 overall, not 30. Next successful unchanged observation: streak 1, not
another transition. Pending checks count only with fully known status and no
validation failure/unresolved repair; never prove readiness. Provider Retry-After/
rate limits override cadence: record throttled next observation and permission
stops, never accelerate error retries.

Verified durable snapshots/stages/streaks survive restart: observe immediately,
reconcile ownership/actual wakeups, record gaps. Continue verified contiguous
history; missed/unknown coverage resets streak to 0. Never invent ticks or
age-reset stages. Without verified state: new baseline at 1 minute.

At 15-minute cadence, detection may take 15 minutes. Reset on detection is not
push immediacy.
Use reliable authorized events for earlier observation when available;
serialize event/timer duplicates. Polling alone proves no event coverage.

## Persist and reconcile the wakeup

Persist per PR: normalized snapshot/evidence; quiet streak/stage; source/target
literal refs/commits, observation time; owner; exact scheduler/heartbeat ID,
type/binding; desired versus observed cadence, cron/timezone, next wakeup;
last successful/attempted observation, gaps; override/lifetime; pending repair,
transfer, cadence-update result. Read back before claiming resumability.
Ticks resume known work, never restart active repairs.

Keep independent per-PR stages/due times, derived from its own observations;
other PR ticks earn no quiet streak. Schedule earliest due wake or minimum needed
periodic cadence; observe **all due PRs fairly**, not one per global tick.
Cron alignment must wake no later than earliest due: use finer authorized cadence
or earlier observation, never round later. Record unsupported precision.
Slow repairs still owe observation-only coverage, accepted transfer, or reported
overdue gaps/safe suspension. Never claim blocked execution observed on time.

Update only owned periods within approved bounds. First read binding; preserve
prompt, scope, lifetime/expiry, permissions, provider and other settings.
Afterward verify stored cron, unchanged binding, next wakeup.
Desired cadence becomes effective only after readback.

- **Paseo schedules:** confirm current schema; verified update fields: `id`,
  `cron`, `timezone` when needed. Send only needed fields, never stale whole
  objects overwriting concurrent settings.
- **Paseo heartbeats:** MCP supports create/delete, not update. With actual CLI,
  update period in place: `paseo heartbeat update <id> --cron "*/5 * * * *"`.
  Requires runtime-supplied agent-scoped `PASEO_AGENT_ID`; never fake it or borrow
  identity. Verify ownership/resulting binding.
- **Heartbeat MCP fallback:** reconcile exact owned heartbeat; preserve prompt/
  binding/bounds before delete/recreate. Verify deletion before replacement—no
  competing monitors. Record non-atomic gap; verify replacement, persist new ID.
  Inspect uncertain deletion/creation before retry, never blindly duplicate.

Failed/uncertain update: inspect stored state. Retain/report old cadence if
running; **monitoring stopped** if removed but replacement failed. Never claim
desired cadence succeeded. Unverified state: report unknown ownership, reconcile
before create/update. Never silently restore/change unrelated settings.

## End only finished duties

Persist terminal PR evidence. Cancel/delete and verify only unneeded owned
wakeups. Retain shared wakeups/agents needed by another PR, repair, or accepted
transfer; recompute remaining due times. Human stop/decision/access-loss:
safely suspend affected duties; record retained resources, owner, resumption gate.
Session cancellation does not prove external-job termination.

All duties ended: remove unneeded owned wakeup; arrange actual LIFECYCLE
retirement, including parent-performed self-retirement. Failed deletion/retirement:
report exact retained IDs and next action. Preserve worktrees/workspaces/branches/evidence.

Exercise [adaptive/recovery scenarios](SCENARIOS.md) for changes.
Package checks prove reachability, not scheduler execution/compliance.
