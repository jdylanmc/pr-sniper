---
name: joe-mode-paseo
description: "Human-enabled repository team. PM owns role heartbeats, six developer slots, backlog intake, shared Shepherd, blocker recovery and an optional requested PR coordinator."
disable-model-invocation: false
user-invocable: true
---

# Joe-mode Paseo

Move the engineering team toward the human's goal, not merely keep a chat alive. Paseo supplies recurring execution and worker lifecycle; Joe-mode prioritizes and routes through existing skills. Each bounded pass checks whether agents are progressing **and working on the right thing**, consumes results, unblocks authorized work and dispatches the next useful assignments.

**Entry:** Human activation and policy changes only; RUN may perform the
preauthorized idle shutdown below, never automatic resume. A matching, previously
human-authorized wakeup job may load only [RUN](RUN.md), not repeat this intake.
Model-loadable metadata permits bounded continuation, not autonomous activation. Follow [INVOCATION](../setup/INVOCATION.md) and the human-approved
[intent](intent.md). Installing/discovering the package does not start anything.
Requires the sibling workflow packages; see [runtime gates](RUNTIME.md).
For missing controls or transport selection, load [TRANSPORT](TRANSPORT.md):
separate daemon/caller, server catalog, harness discovery and intentional policy
before one bounded read-only diagnosis. Transport availability grants no authority.

Follow [TEAM](TEAM.md) for roles, capacity, testing, blockers and cleanup.
Discovery has its own reusable worktree/workspace named `Discovery`.
PM presents new or changed human waits in the primary chat with the exact ask
and verified agent link or locator; unchanged waits are not heartbeat updates.
Kickoff includes preauthorized idle shutdown: do useful work or suspend and
remove owned timers, not spend the night polling a known blocker.
This adapter extends the existing Joe owner board across bounded passes without activating nested/session Joe-mode or replacing delivery owners. **Human merging is
the default.** When requested, a separate PR coordinator
may merge under [the repository-defined gate](MERGE.md). If that gate is missing,
clarify with the human. At minimum require independent Roast, successful CI and
linting, then its own rubber-duck reasoning and final verification.
No self-approval, blanket auto-merge or provider-policy bypass.

## Happy path

1. Resolve the repository, existing owners and Setup completeness (§1).
2. Ask only the unsettled activation questions (§2).
3. Verify board, permissions and Paseo capabilities, then initialize paused (§3).
4. Create the PM heartbeat in this chat, observe, resume, then complete the first
   bounded work pass now (§4); do not wait for cron to start the team.
5. Each wakeup runs one bounded [RUN](RUN.md) pass; [TEAM](TEAM.md) routes work.

Keep every step's gates. For recovery, replacement, pause/stop, missing or uncertain information, or existing ownership, read §4, [STATE](STATE.md) and [RUNTIME](RUNTIME.md).

## 1. Resolve and reconcile before setup

Read the chosen repository's instructions and actual Git remotes/common directory,
worktree, branch and planning configuration. Normalize provider-qualified repository
identity; account for separate planning projects, aliases, other clones and hosts.
Display names and common-directory paths alone cannot prove global uniqueness.
Use [WORKSPACE](../ship/WORKSPACE.md): one repository project, one registered
workspace per Git worktree, shared by all agents on that worktree.

Inspect existing controller/Setup ownership, current PM configuration, matching
wakeup job and active/pending runs. Reuse the one compatible board, project and
workspace. If session Joe, the human-enabled
[CMUX cockpit](../joe-mode-cmux/SKILL.md), or the
[Orca adapter](../joe-mode-orca/SKILL.md) owns the repository, join it or obtain
its explicit release and acknowledged transfer into this adapter before
enabling. Reconcile CMUX's exact managed run, or Orca's actual Run, coordinator,
Dispatches and automations, rather than treating an idle or restored tab as
release. Preserve the anchor, objective clock, coverage,
planning artifacts and pending recovery episodes. An unresolved other
host/clone/owner blocks activation; local locks cannot fence an independent
remote controller.

Run [Setup's completeness check](../setup/SKILL.md#joe-mode-bootstrap-readiness).
Reuse complete content; route missing/incomplete content through existing [Setup](../setup/SKILL.md) as this **human-directed** subflow. Join active Setup; do not duplicate it. Resolve actual tracker target, authenticated identity, readiness role, layout and referenced instructions; files or a setup marker alone are insufficient. Unsupported choices and access failures are
not reset triggers. Preserve all provider/label choices and exact-file approval
gates. Scheduled RUN never bootstraps or writes configuration.

## 2. Ask a few activation questions

Reuse already settled answers; ask only material missing choices:

1. Which backlog selection (labels, epic, query, assigned identity, or explicit
   tickets), scope and non-goals? Which approved dependency/PR grouping?
2. Resolve human merging or authorized orchestrator merging and the repository's
   [merge gate](MERGE.md); clarify missing policy, do not invent it.
   Which routine delivery, tracker,
   scheduler and bounded recovery actions are authorized, and where do questions
   return to the actual human?
3. Developer pool: **six by default**, or what limit? Features reserve two;
   bug fixes, hardening and refactors reserve one. Support roles are separate.
4. Which cadence (**five minutes by default**), host/repository worktree, accessible private evidence location and
   existing runtime profiles? What is the explicit child disposition at
   pause/stop, and who accepts results and retires terminal run parents?
   Preserve the actual primary-chat return channel for questions and the
   recorded scope for Discovery's periodic domain-document PRs.

Recommend the documented primary-chat PM heartbeat recipe after inspecting host
availability and capabilities below. When the human delegates runner mechanics, record the delegation and explained implementation choice; do not repeatedly ask them to choose APIs. An explicit fresh
runner requirement still wins and cannot silently fall back.
Count actual writing descendants inside each lane's reservation, not unlimited
nested workers or an extra slot for the same red/green pair.
Do not hardcode users, repositories, labels, providers or models.

Explain the default idle-shutdown contract with these choices: when no useful
authorized next action remains, close dispatch, remove all owned recurring
jobs, preserve work/custody and report once. Record kickoff's authority and
child disposition as `idleShutdown`; human-directed resume is required.
No additional approval interview when that condition occurs. An explicit
narrower human choice wins; older activations without the grant are not
silently upgraded. Do not promise unattended operation without an agreed
bounded waiting/shutdown policy.
For an existing board, human kickoff/resume can record this grant through
STATE's `configure-idle-shutdown` while paused with the previous lease
released/fenced. Preserve owners and pending work; no reset or separate
permission interview is needed beyond the actual kickoff decision.

## 3. Establish durable continuity and capabilities

Load [Doctrine](../doctrine/SKILL.md) using [APPLY](../doctrine/APPLY.md);
PR-producing routes require `worktrees`, code review requires `solid`.
Execute [LIFECYCLE](../squadron/LIFECYCLE.md), [DELIVERY](../ship/DELIVERY.md),
[OBSERVATION](../shepherd/OBSERVATION.md) and [RECOVERY](../shepherd/RECOVERY.md)
for their respective responsibilities, not duplicate checklists/approval ledgers.

Choose one owner-controlled, ignored, durable JSON board accessible to every run and the human. Reuse/migrate the existing Joe board with acknowledged custody; the helper adds only `pm`, preserving other top-level fields.
Keep runtime IDs, permission details and private evidence out of committed configuration. Human-approved declarative choices may reference the private board without exposing it. Verify ignore/access, persistence readback
and cross-worktree discovery of this **same path**, never a new board per tick.
The registry locator belongs in the existing owner/handoff record.

New team setups set `team: true` and `wakeupMode: "heartbeat"`.
Also set `idleShutdown` to the actual kickoff decision covering TEAM's bounded
waiting, timer removal and retained-child disposition, not fixture text.
Existing boards keep their old capacity units. Use STATE's paused `enable-team`
transition only after old owners and pending operations are settled; never reset
a busy board. Reconcile legacy returns with STATE's paused human-management
path before creating a replacement heartbeat. Do not resume legacy dispatch
just to acquire a cleanup lease. A retained Discovery conversation may transfer
its unanswered questions through an acknowledged administrative end/reassignment;
that is not alignment or a reason to archive the continuing agent.
Existing fresh scheduling remains legacy and separately consented, not a supported replacement for this persistent team.
Prepare the exact activation config; after all following capability gates pass,
follow [STATE](STATE.md) to call the bundled helper with `init`. It validates required evidence references, defaults capacity to six and initializes **paused**. Matching init is a no-op; changed identity/config blocks. The helper cannot grant authority or establish runtime evidence as true. Do not use fixture values as evidence.

Inspect current profiles/notes and provider/tool capabilities through Paseo.
Verify narrow recurring access for backlog/PR/agent/permission/worktree reads,
owned dispatch/return/archive, local board access and mode-specific owned wakeup management.
Orchestrator merging additionally needs the repository-scoped grant and provider
merge capability under MERGE; do not widen worker permissions.
Record the actual grant, lifetime/until-stopped boundary and human-origin anchor.
Use [permission-preserving dispatch](RUNTIME.md#permission-preserving-dispatch).
Propagate the parent's current authorized mode and permission features explicitly,
including human-selected Allow All or Auto Accept. Verify child readback; do not
restore a stale restrictive default. Before any cross-provider launch, record a verified target-policy mapping with STATE's `permission-preflight`; bind it to the actual child with `permission-launch`. Never
broaden grants, approve pending requests as a workaround or edit global
configuration. Select current frontier models by discovery, never a hardcoded
name; see [TEAM](TEAM.md#choose-current-frontier-models).
Pass the shared and selected-mode [RUNTIME gates](RUNTIME.md) before job creation.
Once capabilities are known, establish the consented runner:

- **Same-agent heartbeat:** the original human chat is PM by default and receives configured cron prompts in the correct existing workspace (`*/5 * * * *` by
  default). Between bounded passes it returns/idles, retaining team custody, pending decisions and wakeup duty. **Recommend this for ongoing team
  coordination**, following [Paseo's recipe](RUNTIME.md#recommended-orchestration-recipe).
  Reuse this chat unless it is actually disposable or the human requests another
  PM; transfer authority and results before replacing it.
- **Fresh schedule:** each pass starts a new PM conversation. Requires deployed-runtime proof of stable existing-workspace mapping and safe workspace lifetime. If the operator insists on fresh mode on an incompatible
  host, fail **before activation**; do not substitute a heartbeat.

Record the selected `wakeupMode`, approved `cron` and `wakeupConsent` decision
reference (including an actual delegation of runner choice),
with the explained conversation/lifetime difference. Previous interest in
fresh mode or “keep going” is not consent to change it. No automatic fallback,
activation or broader permissions. Unavailable required evidence remains a block.

## 4. Establish PM, then its role heartbeats

Establish exactly one PM heartbeat below. Once enabled, PM provisions one
Shepherd heartbeat while actionable recurring PR duties exist and one
backlog-manager heartbeat while it has an actionable recurring duty, following
TEAM and STATE's `role-heartbeat` receipts. An unanswered question or human
merge wait alone retains the role, not its timer.
Place the backlog manager in TEAM's dedicated `Discovery` worktree/workspace
before launch; reuse its existing correctly placed conversation and register
no second workspace for the same worktree.
The role itself makes the target-bound call; PM owns inventory and cleanup.
Team kickoff authorizes this delegated lifecycle; do not repeat permission interviews. Developers, roasters and the PR coordinator get no default timer;
only a real recurring duty earns the bounded exception in TEAM.

Only after the gates pass and the human authorizes activation: reconcile the
saved owned job and pending operations using mode-specific evidence below.
List/inspect fresh schedules; use creation/deletion receipts and actual same-agent wakeups for heartbeats, not schedule APIs. Multiple/ambiguous
jobs or uncertain creation wait for reconciliation. Record the create/adopt intent on the paused board's
existing human setup record **before** the external operation.

**Heartbeat:** first resolve the actual primary/reused PM agent and inspect its
identity, human-origin packet and correct existing `workspaceId`, project, cwd
and Git mapping. Reuse a compatible owned agent. If human-authorized setup must create one, use the existing workspace, not another resource/controller. Record
its actual ID as `pmAgentId` before paused initialization. Convey the original human decision, board and narrow authority; tool access does not make a bootstrap or reviewer the PM. The **bound PM agent itself**, within this
human setup subflow (not RUN), calls agent-scoped `create_heartbeat`. Its schema has no target-agent/workspace creation arguments; a disposable setup caller binds the wrong target. Never fake `PASEO_AGENT_ID`, detach, or create
a competing controller to work around that. Use the saved `config.cron`, the approved
timezone/lifetime and bounded RUN prompt. Verify the returned creation summary
and actual agent binding through [RUNTIME](RUNTIME.md#same-agent-heartbeat-surface),
not just the response's job ID. Preserve the receipt; do not call
`inspect_schedule` or `list_schedules` for this heartbeat. RUN never recreates
or resumes the PM job; its bounded role provisioning follows TEAM.

**Fresh:** use the current supported `create_schedule` schema, the saved `config.cron`, explicit
verified `cwd`, local isolation and discovered runtime settings. Prompt it with
the installed absolute RUN path, same private board locator, activation identity,
root human decision path and bounded authority. No undocumented project/workspace
parameters, no unapproved mode substitution, no per-minute worktree creation. Preserve
the approved timezone, lifetime and settings. After uncertain creation, inspect the recorded identity before retry; do not create another job.

For either mode, reconcile uncertain create responses before any retry; missing
heartbeat evidence returns to the human rather than an invented inspection API.
Verify the creation receipt for heartbeat or stored readback for fresh: actual
kind/target, active state, prompt, cron, binding, next wakeup and settings.
Actually observe the initial scoped backlog/PRs and ownership; record evidence separately from the creation response.
Then call `resume` with that human decision and verified binding. An early tick must return without dispatch on paused state. Verify later recurring
receipts: same bound PM agent for heartbeat, safe actual placement for fresh.
Distinguish **configured / initial observation
verified / recurring operation verified**. Gaps or permission waits do not prove working unattended monitoring. Use [SCENARIOS](SCENARIOS.md) for acceptance.

For heartbeat mode, the same PM now executes one bounded [RUN](RUN.md) pass
under this human activation, including claim and release. This is an initial
human-started pass, not a fabricated heartbeat delivery. Follow
[TEAM's startup outcome](TEAM.md#startup-must-reach-useful-work): complete actual
assignment/reassignment or report the precise blocked/pending outcome before
returning. A timer receipt, migrated board or enabled flag alone is not a started
team. Fresh mode retains its separately consented runner/placement contract;
report initial worker dispatch unverified until an actual run establishes it.

## Inspect, pause, resume, stop

- **Change merge policy:** human management only. Resolve the repository gate
  under MERGE, pause/reconcile owners and any issued merge operations, release
  or explicitly fence the old pass, then call STATE's `configure-merge`.
  Preserve the board and other config; changing policy does not resume it.
- **Inspect/status:** read the helper, mode-specific wakeup evidence, current
  controller, workers, pending permissions and latest observations. No mutation,
  activation or stale cached readiness claim. Report gaps and pending results.
- **Pause:** on human direction call helper `pause` first, recording explicit
  active-child disposition. Direct every live Shepherd/Discovery role to delete
  its exact owned heartbeat and record each receipt, not just PM's.
  For fresh mode, use supported `pause_schedule` and
  inspect actual paused state/next-run behavior. For heartbeat, have the bound PM
  agent delete its exact owned ID through MCP `delete_heartbeat` or the
  verified CLI route in TRANSPORT, preserving that route's successful external
  acknowledgement as deletion evidence. No `pause_heartbeat` or heartbeat resume MCP operation exists. Reconcile an already dispatched prompt/run;
  it must not start new work. Existing scoped workers remain owned, not killed.
  Record outcomes, including failure, in the human management record.
- **Idle shutdown:** the claimed PM uses helper `suspend` under the saved
  kickoff `idleShutdown` grant and follows TEAM's useful-work-or-shutdown
  decision. This closes dispatch as paused before owned timer removal; it does
  not stop external jobs itself. Preserve duties and accept late cleanup
  callbacks without recurring polling. Resume remains human-only.
- **Resume:** human only, never a tick/recovery wake. Reconcile ownership,
  children, partial work, mapping, access and job first; observe now. Fresh mode
  uses supported `resume_schedule` on the same verified ID; a deleted fresh job needs separately reconciled setup, never heartbeat replacement.
  Heartbeat mode requires acknowledged exact-ID deletion (or other supported
  definitive absence evidence) and the old
  pass lease released or explicitly fenced. Preserve the same PM agent, scope,
  config, prompt, timezone/lifetime/settings, workers and pending results; record
  human recreation intent before that agent calls `create_heartbeat` once.
  Reconcile an uncertain creation before retrying. Verify the new creation receipt and call
  helper `resume` with [STATE's exact replacement evidence](STATE.md), including
  old ID and human proof. No board reset, automatic tick resume or target change.
  After successful heartbeat resume, complete the same bounded initial work pass
  and startup outcome above; do not finish at replacement-timer creation.
- **Stop:** call `stop` first, then delete the exact owned wakeup and verify
  deletion through its mode-specific evidence, including every role heartbeat.
  Stop is not blanket cancellation: obey
  the chosen retain/finish/acknowledged-transfer disposition for each child,
  resolve pending results, and record continuing Shepherd duties or explicit
  monitoring gaps. Preserve artifacts and workspaces. Failed or uncertain deletion
  remains a reported blocker; the stopped board still rejects new claims.
  Keep the heartbeat PM alive while deletion, children or reporting remain unresolved. Retire it only after verified owned-wakeup absence and accepted
  end/transfer of all duties; pausing alone is not terminal.

Heartbeat control follows [operation-specific transport gates](TRANSPORT.md).
Prefer MCP creation; CLI creation remains blocked on 0.8.0's incomplete receipt.
Deletion may use the verified exact-owner CLI receipt under those gates, not
an MCP-only blanket rule. A CLI period-only update is not a pause/resume API.
Keep the approved cadence on recreation; a cadence change
requires human-authorized paused/fenced reconfiguration, not a tick adjustment. If supported verification of pending creation/deletion is unavailable, keep the local gate closed, report uncertainty; do not recreate or claim successful pause/stop.
No wakeup operation automatically cleans Git/UI resources. No automatic
resumption after a human pause. Every modifying owner consults
[Changelog](../changelog/SKILL.md) within its assigned write scope.
