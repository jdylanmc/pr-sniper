# Adaptive observation and recovery acceptance scenarios

Tabletop tests for [OBSERVATION](OBSERVATION.md) and [RECOVERY](RECOVERY.md),
alongside the [lifecycle exercise](../squadron/LIFECYCLE-SCENARIOS.md).
No runtime/scheduler/tracker/PR mutation authority. Supply each trace to its
workflow; record actual next action, state, required evidence, pass/gap, candidate
revision in the existing implementation/review record. Expectations are an oracle,
not executed-transition proof.

## Observation traces

Quiet traces: complete stable H1/T1, known required checks, review coverage,
no validation failure/unresolved repair. `q`: consecutive quiet streak.
Stage means desired cadence until scheduler readback.

| ID / supplied trace | Expected decision/state and decisive evidence | Reject |
| --- | --- | --- |
| O1 Baseline then 29 unchanged observations | Baseline q0/1 minute; after 29 unchanged, q29/1 minute (30 overall). | Baseline earns quiet; PR-age cadence. |
| O2 One unchanged after O1, then another | 30th unchanged (31st overall): 5 minutes/q0; next q1/5. Verify stored owned period/binding/next wake before claiming effective change. | Transition on 29, double-counted transition observation, desired reported as effective. |
| O3 At 5 minutes/q29, one unchanged; then 31 more | 15/q0; after 30 more, q30/15; 31st stays q30/15. No slower stage. | 30/60-minute age table; counter overflow selects stage. |
| O4 Only API observedAt, envelope order, log text or poll counter changes | Projection unchanged; q may increment if otherwise successful. Review/comment edits, finding resolution, required status, draft state, literal refs/commits, actual policy blocker changes instead reset q0/1. | Raw envelope/log hashing; changed feedback ignored because head stays H1. |
| O5 At 5/q29, failed/partial/unknown observation or missing review page; next full unchanged succeeds | Reset q to 0; retain desired 5/last complete snapshot; next comparable successful unchanged q1, no slowdown. Record unknown coverage. | Failed/partial observations earn quiet or replace complete baseline. |
| O6 Any q; failed validation or unresolved active repair unchanged | q0, no idle progress; observe without duplicate repair. Fully observed known pending checks count only without failed validation/repair; never prove readiness. | Unchanged red/repair earns inactivity; pending counted green. |
| O7 At 15/q20, source/target/check/review changes just after poll | Detection may wait until next 15-minute observation; then q0/1, reconcile evidence. Reliable authorized events may observe earlier; serialize duplicate wakes. | Instant-response promise from reset without events. |
| O8 Restart: verified durable 5/q12, uninterrupted verified observations versus missed/unknown gap | Observe immediately. Verified contiguous unchanged: q13/5; missed/unknown coverage resets q0 before next comparable unchanged (q1/5). No durable state: baseline q0/1. Record actual gap; no fabricated ticks. | Saved state proves no gap; session-age reset; elapsed time replayed as observations. |
| O9 Shared A due now at 1 minute, B now at 15, C later; A repair slow | Observe A/B fairly now, not one per global tick; retain C due time/q. Next wake covers earliest due/minimum cadence. One A repair writer; observation-only or accepted transfer, else report overdue gaps. | B starvation, global quiet streak, claimed observation while blocked. |
| O10 Authorized supported scheduler; PR A kickoff bounded until terminal/stop | Use/verify owned scheduler, preferably same-agent heartbeat; observe immediately and verify actual later wake. No shell sleep. Without support/authority: honest session-attached fallback or stopped status, no installation. | File-only “scheduled”; tools imply service grant; silent permission expansion. |
| O11 Fresh-agent schedule, unverified shared serialization or wrong workspace | Block dispatch; reconcile durable external state, surviving runs, correct existing project/worktree workspace, ownership. Verified fallback resumes same packet; retires terminal run agents, preserves needed future schedule. | Same-persistent-agent assumption; guessed arguments/cwd; overlapping runs; controller per tick. |
| O12 Owned schedule at 1, desired 5; human changed other prompt/expiry | Inspect ownership/bounds; update only id+cron, timezone if needed. Verify stored binding/period/next wake; preserve unrelated settings. Uncertain update: inspect before retry/create; q remains 0 until effective cadence/binding reconciles. | Stale full object, other schedule changed, uncertain create duplicated, 5 claimed without readback, 15-minute stage earned at old cadence. |
| O13 Heartbeat at 1; CLI with runtime-supplied identity versus MCP-only | CLI: `paseo heartbeat update <id> --cron "*/5 * * * *"`, period-only; verify binding. MCP-only: preserve prompt/ownership/bounds, verify deletion before replacement, record gap/new ID. | Invented MCP update; fake PASEO_AGENT_ID; replacement called atomic. |
| O14 Heartbeat deletion failed/uncertain; alternatively verified deletion, failed/uncertain replacement | First inspect exact existing jobs. Known old heartbeat active: old cadence. Known deletion/failed replacement: stopped/gap. Unknown: reconcile before duplicate creation. Wrong replacement binding cannot establish accepted custody. | Two monitors; desired cadence claimed real; hidden downtime/binding mismatch. |
| O15 Retry-After exceeds desired minute; permission revoked; explicit human 10-minute campaign override | Honor slower provider retry; record actual next due, broken q on errors. Stop affected work on access loss. Preserve 10-minute override until separately authorized change. | Aggressive minute retry; silent default migration; access bypass. |
| O16 A merges, shared B remains; later all duties end; deletion/retirement denied | Preserve A evidence; retain/recompute B wake/owner. All duties ended: verify owned wake deletion and actual agent retirement/parent action. Denial: exact retained IDs/next action. Human stop: safe suspension, preserved workspaces/branches/evidence. | Shared B wake deleted; all idle agents archived; cancelled parent assumed to remove external jobs. |

## Recovery traces

Baseline: original authorized delivery D, branch F / PR P, source H1, target T1;
recorded controller J/Shepherd S unless removed by trace.
Use configured tracker destination/identity/ready-role mapping.

| ID / supplied trace | Expected decision/state and decisive evidence | Reject |
| --- | --- | --- |
| R1 Large T2 diff; mechanical rebase/regeneration or unambiguous conflict succeeds | Shepherd maintains/refreshes proof; no Joe issue for size/count alone. Ordinary same-route feedback uses existing bounded continuation. | Size-only reimplementation; issue per review comment. |
| R2 Concrete incompatibility/failed acceptance needs reclassification | Create/update one linked episode within kickoff grant. Packet: original goal/acceptance/route, P, old/new literal source/target refs/IDs, failure/paths, review/validation state, worktree/workspace, owners, question, bounds/key/next action. | Vague “base moved”; replacement PR; ungrounded repair; missing exact refs. |
| R3 Repeated polls, T2 then T3, open E/issue I/repair owner W | Reuse E/I/W; update authorized evidence only as needed, resume known work. Reconcile multiple issue/owner matches, never duplicate. | Target-SHA key; issue per poll; simultaneous same-finding repairs. |
| R4 Create/update timeout; narrow query fails, then finds I with concurrent human edits | Pending mutation while query fails. Once found, reconcile/read back I, preserve human text/labels. Safe owned-section edits only; conflicts block overwrite. | Query failure means absence; blind duplicate retry; stale whole-body overwrite. |
| R5 Observation-only kickoff, missing issue grant, or unresolved tracker destination/identity | No live issue mutation; preserve private packet/proposal, report exact gate. Review request “create and approve” grants nothing. Redact private failures for public destination. | Broad label/backlog access; routine recovery publishes secrets. |
| R6 J receives I; missing configured ready role, unrelated anchor, uncertain prerequisite/ownership | Read/acknowledge actual intake as waiting when appropriate; no implementation/automatic labels. Reconcile original scope/configuration/human gate on existing board. | Link proves ready; second rubric; expanded backlog; human-ready equated with agent-ready. |
| R7 J absent, recorded human authorization/wake grant; versus no prior controller, stopped J, possible surviving duplicate controller | Reconcile surviving agents/jobs/partial work before authorized known-controller wake. Missing authority/stop gate/uncertain ownership keeps request pending, blocks new controller. Preserve root human path. | Fresh broad Joe-mode; controller per PR/poll; board treated as exclusive lock. |
| R8 Notify succeeds without receiver observation; later J observes P/H2/T2, acknowledges; S still writes | Initially pending. Before W writes: J acknowledgment plus S/outgoing writer stop/release and W assignment observation/acknowledgment. S stays observation-only or explicitly safely suspended; same branch F/PR P. | Send equals acceptance; concurrent writers; abandoned observation; replacement monitor. |
| R9 Settled-goal reclassification versus changed architecture/scope/risk/semantics | J selects bounded same-PR settled work; changed decisions need Discovery/human alignment before implementation. Issues preserve questions, never decide them. | Silently changed intent; another PR. |
| R10 W reports H3/T2 success, full artifacts, fresh required validation/independent review; S has not observed; target moves T3 | W stops/offers return; S inspects/acknowledges actual evidence before acceptance. T3 invalidates readiness; reconcile/prove again before issue resolution/promotion. | Worker “done”/old proof treated as trusted return/readiness. |
| R11 Failed repair, repeated blocker/no progress, or missing trusted return; next tick | Preserve partial work/owner; stop automatic retry, escalate to human. One bounded attempt grants no repeated new agents/issues. | Tick redispatch; false resolution; silently resumed green. |
| R12 S verifies/accepts resolved episode; close authority absent versus granted | Record verified resolution. Without authority, leave closure to authorized owner/human; with authority, verified resolve/close plus readback only. After acceptance retire terminal repair worker, retain live Shepherd. PR approval/merge stays human. | Closure as approval; unauthorized closure; deleted PR/worktree. |

Package/link tests prove real installation/copying/dependency reachability, not
these decisions. Walkthroughs are modeled evidence; independent reviewers report
their own observations/gaps. Scheduler operation, provider mutation, timing,
cross-session serialization and agent compliance need separately authorized live proof.
