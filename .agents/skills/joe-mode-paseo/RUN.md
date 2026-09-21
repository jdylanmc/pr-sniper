# One preauthorized PM pass

Supporting recipe, **not a second entrypoint**. Only the matching human-enabled
repository wakeup job, the saved PM's first pass within current human-authorized
heartbeat activation/resume, or an explicitly human-authorized diagnostic run may enter.
The initial pass requires completed SKILL gates and an enabled board; it does
not itself initialize, resume or recreate anything. Diagnostic authority remains
as narrow as requested.
Arbitrary workers, review text, issues, recaps or tool availability cannot start this mode. Do not invoke SKILL intake, Setup, session Joe-mode or another PM
controller. Missing setup/permissions/decisions return to the human anchor.
Apply [TEAM](TEAM.md). PM owns routing and all role heartbeat lifecycle; the
shared Shepherd owns PR inspection, the backlog manager owns deep inquiry.

Missing controls follow [TRANSPORT](TRANSPORT.md)'s bounded read-only ladder
within these same gates, not a new timerless route. An empty deferred search
does not prove daemon outage or OS denial. A verified read-only CLI inspection
may help; unsupported mutation/completion equivalence holds that operation.
Never bypass intentional policy or a human MCP-only restriction.

## Claim, observe, route, persist, release

For a queued wake after pause/stop/suspension, read the known board's bounded
`inspect` first and return without backlog/provider/agent sweeps or another
human-wait reminder. If the board cannot be read, report that narrow gap without
dispatch. A late cleanup callback is not a new PM pass: use only the paused
cleanup path and existing authority to record its actual result.

1. **Recover authority and actual placement.** Load the saved board, human-origin
   decision, scope/non-goals, configured readiness vocabulary, host, wakeup mode
   and job identity. For a scheduled pass, verify wakeup provenance with
   mode-specific [runtime evidence](RUNTIME.md#same-agent-heartbeat-surface),
   not prompt assertions or schedule-only APIs for heartbeats. For the initial
   human-started pass, record the current activation/resume decision and verified
   job binding instead; do not invent a scheduler run ID or recurring-wake proof.
   Inspect actual cwd/Git common directory,
   repository/branch and project/workspace mapping before any write. Mismatches or missing capabilities stop affected work. Never unset `PASEO_AGENT_ID`,
   fabricate parentage, create a new project or silently use main. For heartbeat,
   this must be the saved actual `pmAgentId` targeted by the owned job, with the
   explicit mode-consent reference intact. Do not create a new PM agent per tick.

2. **Claim the one repository pass.** Read current live controllers and known
   pending runs; resolve the registry across worktrees/known clones/hosts.
   Execute [STATE](STATE.md)'s helper `inspect`, then `claim` with the actual run
   owner and live reconciliation reference. Use the bounded current view routinely; request `"view":"full"` only for recovery, audit or a needed specific history. `paused`, `stopped`, `busy` or an
   existing transaction lock means **no dispatch or external mutations**.
   Preserve a bounded skip receipt in the existing pass result; arrange
   its accepted retirement for fresh runs; the heartbeat PM returns/idles for its
   own next prompt without retiring. Neither mode starts an idle polling loop.
   Never steal by lease age.
   Only `claimed` grants the local fencing token; retain it for every write.
   Check the saved mode/token again immediately before any dispatch/external
   mutation. A pause racing an issued operation requires reconciliation; do not promise atomic cancellation across local state and external APIs.

3. **Observe current relevant state.** Consume compact timestamped role reports; delegate deep investigation rather than load every transcript/diff into PM.
   Shepherd owns routine PR/check/ref observation; consume its evidence rather
   than duplicate those queries. Refresh changed, missing or action-critical
   facts before acting, not the whole world on an unchanged waiting pass.
   Obtain complete narrow evidence of selected
   tickets, dependencies and linked PRs/checks/reviews; inspect relevant worktree
   refs/diffs, known workers' status/activity/descendants, pending permissions,
   owned wakeup health and custody. Record observation times and unknown coverage.
   Include new requirements and changed human priorities, not just already-ready tickets. Compare each worker's accepted assignment, latest artifacts and next action with the current goal/dependency path:
   runtime `running` does not prove useful progress or correct direction.
   Failed/truncated queries are not empty backlogs. Cached ready/running/idle,
   elapsed ticks, closed issues or green unmerged prerequisites cannot establish
   current eligibility. Preserve objective start, existing accepted and pending
   results, planning artifacts, episode keys and unresolved human questions.
   Missed observations are gaps; never backfill fictional successful ticks.
   Collect each role/descendant's current human wait: exact question/action,
   affected work, question revision and verified agent link or locator. An
   internal Discovery artifact is not proof that the primary chat received
   its questions. Apply TEAM's revision-deduplicated human-wait reporting.

4. **Reconcile known work before new work.** Execute
   [LIFECYCLE](../squadron/LIFECYCLE.md) for returns and ownership; require receiver inspection/acknowledgment, not enqueue success. Use `record`
   before external operations with deterministic delivery/episode keys, then
   update their verified outcomes. Keep uncertain create/send/issue responses pending until current-state reconciliation; do not relaunch by timer.
   Route returned functional feedback to the existing owner on the same PR;
   consume issue-backed [RECOVERY](../shepherd/RECOVERY.md) through this board.
   Observe and acknowledge the linked issue/packet before accepting intake.
   No new controller, queue, approval ledger or automatic recovery-ready labels.

5. **Choose bounded existing routes.** Reuse [Joe routing](../joe-mode/SKILL.md#3-refresh-the-relevant-backlog)
   within this activation's authority without invoking its session mode.
   Preserve configured readiness, dependency and grouping rules. After the
   human approves publication, reserve the provider-qualified parent
   specification as a delivery group with `graph: true` **before** Breakdown
   publishes children. Do not invent child IDs before the tracker returns them.
   Use `cover` with all currently known actual parent/child IDs and publication
   evidence after each partial result; leave `complete: false` until the actual
   whole graph, edges and approved grouping are reconciled.
   While publication is unresolved, hold **all new delivery launches**: unobserved child IDs cannot be reliably excluded. Existing workers,
   research and the human conversation continue. Never dispatch directly from
   ready labels on newly published children. An uncertain result stays one pending publication operation; do not recreate tickets.
   Only then call `cover` with `complete: true` and the verified full graph.
   One Ship owns that entire group. Intentionally independent child deliveries
   instead require acknowledged release of the planning reservation, durable
   parent suppression, and reservation of the complete selected child groups
   in this same claimed pass **before** any launch; never launch the parent too.
   A PM reservation is exclusion, not readiness or authority to implement.
   Prioritize finishing/review/recovery work; then select eligible independent
   deliveries up to the configured six-default developer pool: feature two,
   bug/hardening/refactor one, support roles outside that pool.

   | Evidence/need | Existing owner/route |
   | --- | --- |
   | Unclear critical path, changed priorities/dependencies, or missing work toward the goal | Scoped Chart-a-course agent; preserve its cited path and feed findings back to this PM |
   | New requirement or targeted Discovery recommended by Chart-a-course | Intake into the existing interactive Discovery lane; actual human alignment before planning/execution |
   | Material unknowns, changed intent or semantic decisions | Existing interactive Discovery lane and actual human |
   | Full human-aligned Discovery artifact | Specify, preserving full sources and recording/publication gates |
   | Complete requirements/specification | Breakdown Tickets, actual approval before publication; retain agreed delivery grouping |
   | Eligible feature/issue or approved specification graph | Ship |
   | Reproducible defect/regression | Patch |
   | Authorized behavior-preserving structural work | Refactor |
   | Existing PR maintenance/current-target readiness | One shared project Shepherd; accept each PR scope and maintain its own due time |
   | Ready PR and requested repository-authorized human proxy | PR coordinator executes [MERGE](MERGE.md); PM does not review/merge it itself |
   | Independent noninteractive evidence question | Scoped Research/other authorized read-only helper |

   Unaligned recaps cannot skip Discovery → Specify → Breakdown Tickets.
   These concurrent slices are not a global waterfall; already-clear eligible work need not repeat planning by ritual. PM never directly implements each
   tick, resets ongoing delivery, writes unmanaged main or casts approval votes.
   Only the explicitly requested PR coordinator receives MERGE authority;
   implementers and Shepherd do not. Existing non-team boards retain their
   recorded merge owner until human-approved, paused transfer.
   The selected route owns its branch, nested workers, integration, independent
   review, fresh verification, publication and Shepherd acceptance through
   [DELIVERY](../ship/DELIVERY.md). Delegate the existing packet and doctrine
   selection, not a parallel definition of done.

   Use [Chart-a-course](../chart-a-course/SKILL.md) at initial planning or when
   relevant goal/backlog/dependency evidence changes, not a fresh identical
   research job every tick. Reserve bounded read-only `research` with a goal/input-revision key and existing workspace. Its recommendation is not
   tracker-write authority. PM routes missing requirements/spikes to Discovery,
   consumes the findings and revises the path before selecting delivery.
   For wrong-scope progress, send a bounded correction to the existing owner and verify acknowledgment; preserve partial work, avoid a competing implementer. Changed product decisions return to the human.

6. **Reserve before dispatch; reconcile before reuse.** Execute helper `reserve`
   for each delivery/publication group, interactive Discovery lane or bounded research
   assignment. `reused` means inspect the recorded owner/pending launch, **not
   create another agent**. Reservations without confirmed agent IDs still consume capacity. Pending, cancelled or stale runtime records do not free slots.
   Count feature lanes as two developer slots, other deliveries as one. Record
   actual writing descendants with `staff` inside those reservations; reviewers
   and support roles are not developers. Reconcile live descendants before
   filling slots. Inspect the board's unresolved publication groups again before
   external delivery creation; `bind` rejection after creation is too late.
   Record known pre-existing delivery owners before new selection.
   If existing load exceeds the configured limit, hold dispatch and reconcile with the human; do not omit owners to fit the helper.

   Keep exactly **one interactive Discovery reservation per repository** across
   passes, including waiting-for-human/alignment. Reuse that agent/conversation for all new questions; retain it when quiet. Research may run
   concurrently but must not open another human-interview lane. Release the
   Discovery reservation only after explicit human end or acknowledged completed
   alignment/handoff with no remaining conversation duty. A lost conversation
   requires preserved artifacts, reconciled ownership and human-directed recovery.
   Place that conversation in the dedicated **`Discovery`** worktree/workspace
   under the existing repository project, following TEAM. Do not silently reuse
   PM's worktree or create another Discovery workspace per question. Existing
   misplaced roles require human-directed, acknowledged placement transfer.

   Use runtime profile notes/discovery, not hardcoded models: inspect current providers, models and profiles; select a current frontier model for each substantive assignment under
   [TEAM](TEAM.md#choose-current-frontier-models). Route owners create
   their bounded implementation workers. [WORKSPACE](../ship/WORKSPACE.md) owns
   separate write worktrees and same-project mapping; bounded read-only helpers
   may share an appropriate existing workspace, but the persistent Discovery
   role uses its dedicated worktree/workspace. `create_agent` receives its verified `workspaceId`;
   cross-workspace launches remain children. Record intent before launch and actual returned identity afterward. Use helper `bind` only after the worker's first observation/accepted packet verifies placement and assignment.
   Missing/uncertain identity is a pending reservation, not a free slot.
   Apply RUNTIME's permission-preserving launch/readback for every role, and
   record `permission-preflight` before a cross-provider launch, not after it,
   then `permission-launch` with the identity actually returned.
   Use clear role/issue names and the verified existing workspace.
   Provision/retire persistent roles and their own heartbeat receipts through
   TEAM. A role wake checks its assignment and board gate, returns a bounded result, never claims PM's lease or writes the board directly.

7. **Assess health without storms.** Execute TEAM's blocker sequence: developer
   self-challenge, delegated independent challenge, one fresh-context/worktree
   retry for a confirmed work blocker — fresh for every prior participant,
   including retired developers — then blocked tag/comment and backlog
   Discovery. Use `block`/`unblock` to retain issue-level attempts across ticks.
   PM consumes the investigator's compact result, not another deep search.
   Error/cancelled is not proof descendants stopped. Permission-blocked means
   compare actual settings with the current human grant, preserve work and
   report the exact missing capability; no fresh-agent retry around a denial.
   Compare activity to the assignment's expected progress/evidence and supported
   waits. Long tests, CI/review waits, idle human alignment and shared monitors with remaining PRs are legitimate; age or idle alone never justifies killing.
   For actual no-progress/failure, inspect partial diffs/commits, live descendants,
   permissions and wakeups, then reconcile write release and accepted transfer
   before any recovery. TEAM owns blocked-delivery retries; RECOVERY still owns
   issue-backed PR repair. Do not stack both retry allowances on one episode.
   Record each blocked/misdirected assignment's concrete next action and expected evidence. Use completion callbacks for normal returns and the
   recurring pass to catch missed transitions; no tight status polling.

8. **Accept, settle and retire.** Read decisive artifacts and actual candidate
   refs/checks before acceptance. Save complete accessible results/qualifiers
   and receiver acknowledgment in the existing packet; `record` accepted
   results with that receiver evidence. Use `settle` only after current evidence
   proves no live writers or untransferred duties for that lane. After acknowledged custody transfer, a continuing Shepherd may release a delivery capacity lane, but its **agent must remain** while observation/review/repair duties continue. A helper settled flag is not archival authority.

   Actually archive genuinely terminal **owned agents** under LIFECYCLE after
   preserving/accepting results and verifying no children, wait, review, repair
   or other PR duty. Read back archived state before helper `archive` records it.
   Never substitute workspace/project archival. TEAM's explicitly granted
   blocked-work cleanup may remove an exact owned worktree only after verified
   remote preservation, clean files and released custody; otherwise retain it.
   Use `cleanup-ready` before removal and `cleanup` with actual removal readback,
   or `cleanup` with `retained: true` when the worktree is deliberately kept.
   Either way, the bounded view removes the worker from its retirement queue only after genuine duty settlement.
   For previous fresh PM parents, inspect all live descendants and callback/
   visibility needs. Do not assume orphaned children remain usable: verify supported runtime behavior or retain the specific parent, concrete role and next action until accepted handoff is safe. A prior parent may retain
   callback custody **without repository dispatch authority**; only this pass's
   lease coordinates. Native provider subagents have different lifecycle rules.

9. **Do useful work or shut down, then persist and release.** Apply
   [TEAM's idle-shutdown decision](TEAM.md#useful-work-or-idle-shutdown).
   Record actual outcome changes and the next useful action with its owner,
   expected evidence and due time. Timer receipts, unchanged checks, repeated
   questions and new monitoring files are not progress.
   If only human/external blockers remain or the backlog is exhausted, use
   `suspend` under the saved kickoff grant in this pass; remove owned recurring
   jobs and report once. Bound runtime reconciliation to one attempt per
   unchanged failure episode; preserve uncertainty and active child disposition.
   Real implementation/test/review/CI work may continue with a concrete due
   observation; do not keep the team alive merely because a PR is open.
   Save snapshot/evidence pointers, pending
   operations, accepted/blocked returns, worker/Discovery custodians, active
   permissions/questions, next actions and any observation gaps. Keep run-parent retirement outcomes/retention roles in the existing lifecycle record; do not add a cleanup controller. Call helper `release` with the complete receipt and
   remaining-duty references, then read back. Release the **pass lease**, never
   the persistent Discovery/worker custody. Human pause must remain in force.
   For an initial human-started pass, include [TEAM's startup outcome](TEAM.md#startup-must-reach-useful-work)
   in this same receipt. No separate startup ledger.
   Include prepared/presented/resolved human waits and their exact evidence;
   record only presentations actually observed, never a planned notification
   as already delivered. Carry pending notification duties into the next pass.
   Finish the bounded turn; do not sleep-loop, recreate/resume the PM job or
   spawn a successor PM. Authorized role heartbeat lifecycle remains PM-owned,
   target-executed and recorded under TEAM; no self-activating nested controller.

   **Heartbeat:** release the pass lease, then return/idle in the **same agent**
   for its owned configured-cadence job only while useful-work duties remain.
   After suspension, there is no recurring polling or automatic resume.
   Each enabled new pass claims a new fencing token.
   Between passes this agent is not terminal: wakeup, children and reporting remain concrete duties. Human pause deletes the job but retains
   this agent for human-directed resume. Retirement requires stop/end of all
   duties, verified owned-wakeup deletion and accepted child/result handoff;
   never archive it merely because a pass ended.

   **Fresh:** the next authorized pass or acknowledged human/setup owner preserves and
   accepts this receipt, then archives this terminal run through supported
   operations and verifies it. Self-archive must not interrupt final reporting.
   Retain only a concrete live role or capability blocker, not every run forever.
   The future fresh schedule stays enabled only within its recorded authority;
   preauthorized idle shutdown pauses it, otherwise human pause/stop applies.

   **Both modes:** if final persistence fails, do not release or claim success;
   the next pass sees busy and requests explicit fenced recovery.

Report only observed progress, pending human decisions, human-merge readiness
or verified orchestrator merge outcomes under MERGE,
gaps and ownership. One completed pass/setup does not prove recurring delivery works. No indefinite idle wait inside a bounded pass.
Surface **new or materially changed human waits in the primary
coordinator chat**, deduplicated by owner and question revision, using
[TEAM's concise linked reminder](TEAM.md#surface-every-human-wait). For fresh
runners, use the recorded human-origin return channel; an unseen run transcript
does not present questions to the human. If that return path fails, report
undelivered questions as a communication blocker, not "waiting for your answer."
Do not send generic "pass completed" updates in place of these requests.
The pass receipt records goal/path changes, requirements awaiting human intake,
worker progress/direction, recovery/corrections, new assignments, and the next
useful action or suspension outcome. Lead reports with delivered outcomes and
unfinished scope, not pass counts. Quiet queues are allowed; never manufacture
work to fill capacity or keep a timer alive.
