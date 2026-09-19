# Local board helper

[RUN](RUN.md) calls [scripts/state.mjs](scripts/state.mjs) to serialize bounded passes and reserve work. Node is the only dependency. This local atomic-state seam is not a daemon, external API client, approval system or proof of agent compliance. Preserve the existing Joe/lifecycle evidence
in the same board/linked packets; only the `pm` namespace is helper-owned.

Resolve the script from its installed package, not the target repo. Commands
accept the same private board path and one JSON request. The CLI returns the
**bounded current board** by default: `{"status":"…","view":{…}}` with mode,
capacity, lease credentials, wakeup binding, live workers, unresolved
operations, open blocker episodes and counts of the durable history it omits.
It also keeps a `retirement` queue: each settled worker whose duties are not
actually finished, with its key, kind, agent, worktree, `phase`
(`archive-pending`, `cleanup-pending` or `removal-pending`) and, once recorded,
the `recovery` branch and head. A worker leaves that queue only when genuinely terminal: archived with no owned worktree, or with its worktree actually removed or deliberately retained. Retirement is a duty, not mandatory deletion; record retention explicitly, never as an omission.
Add `"view":"full"` to any request for the complete `{"status":"…","state":{…}}`
envelope, including run receipts, settled workers, operation history and wakeup
or merge history. Unknown view values fail before the board is touched.
Nothing is discarded: the durable board keeps every record; the in-process `transact` API still returns full state for callers and tests.
Failures emit a diagnostic to stderr and exit 1.
Do not publish output containing private runtime IDs. For example, after
human-approved initialization:

```sh
node /installed/skills/joe-mode-paseo/scripts/state.mjs /owned/ignored/board.json '{"op":"inspect"}'
node /installed/skills/joe-mode-paseo/scripts/state.mjs /owned/ignored/board.json '{"op":"inspect","view":"full"}'
node /installed/skills/joe-mode-paseo/scripts/state.mjs /owned/ignored/board.json '{"op":"claim","owner":"actual-run-id","reconciliation":"accessible-live-ownership-evidence"}'
```

Use the bounded view routinely; explicitly request full view for recovery, audit or specific-history reconciliation. A bounded view is a
current-state projection, not proof that omitted history is unimportant.

Replace example paths/IDs with verified values. The containing directory must already exist and be owned, ignored and accessible. All repository worktrees and known clones must resolve this same authority; no independent per-worktree boards. Other hosts need reliable shared exclusion or activation
blocks. Do not claim the helper is a distributed lock.

While this adapter owns the repository board, serialize **all** board mutations
through the helper. Workers/retained parents write only their owned receipts and
return pointers for the claimed pass to record; they must not concurrently
read/modify/write this JSON through another tool. Preserving unrelated top-level fields cannot make unsynchronized external writes safe. Adopting an existing
board or changing its legacy fields requires paused, fenced writers and explicit
custody. If another required writer cannot follow that contract, block activation; never run two persistence protocols on the same file.

## Request contract

| Operation | Required inputs beyond `op` | Result/meaning |
| --- | --- | --- |
| `init` | `config` below | `initialized` paused; identical config `existing`; differing config rejected |
| `inspect` | None | `observed`, read-only board; empty object means not initialized |
| `resume` | `human` decision reference; `schedule` observation below; heartbeat replacement requires `replacement` below | `enabled` only after actual human-authorized job verification; never called by a tick |
| `pause`, `stop` | `human`, `disposition` reference for every active child | `paused` / `stopped`; local dispatch gate first, **not** proof of external operation |
| `configure-merge` | `human`, `reconciliation` of authority/owners/pending operations, `merge`, and `mergeGate` for orchestrator mode | Human management only, paused/stopped with released/fenced lease; changes only merge config, retains prior policy in `pm.mergeHistory`, never resumes or merges |
| `claim` | `owner`, `reconciliation` live ownership reference | `claimed` with new `state.pm.lease.token`, or `busy` / `paused` / `stopped` without dispatch authority |
| `recover` | `human`, exact old `token`, `fencing`, `reconciliation` | Clears only that stopped/fenced pass; preserves workers, records and mode |
| `reserve` | Lease credentials; `worker: {key, kind, coverage, packet, graph?}` | `reserved` or `reused`; kind `delivery`, `discovery`, `research`; coverage is nonempty unique qualified identities; `graph: true` reserves a delivery's parent before approved ticket publication |
| `cover` | Lease credentials; publication-group `key`, cumulative actual `coverage`, boolean `complete`, publication `evidence` | `covered`; monotonic, overlap-checked parent/child coverage, preserves original assignment and publication receipts; complete graph becomes immutable |
| `bind` | Lease credentials; `key`, `agentId`, `evidence` of actual first observation | `bound`; different bound identity rejected |
| `record` | Lease credentials or paused human management below; `key`, `status`, `evidence`; `receiver` for accepted | `recorded`; status `pending`, `blocked`, `observed`, `accepted`; one entry per stable operation/episode key |
| `settle` | Lease credentials or paused human management below; `key`, `noLiveWriters: true`, `noUntransferredDuties: true`, `evidence`, `result`, `acceptance`; Discovery also `discoveryEnded: true` | `settled`; frees capacity only after external reconciliation; preserves full result references |
| `archive` | Lease credentials or paused human management below; settled worker `key`, actual archive readback `evidence` | `archive-recorded`; records completed external archival, does not perform or authorize it |
| `release` | Lease credentials; preserved `result`, remaining `duties` references | `released`; appends run receipt and clears only pass ownership |

Lease credentials are `owner` and `token` from the successful claim. Old tokens
cannot mutate after release/recovery. Paused owners can preserve returns/release
but cannot reserve or bind new work. Changed operation outcomes retain prior
status/evidence/receiver references in `history`; identical observations do not
append duplicates. Keep referenced full artifacts accessible.
Accepted records name actual receiving observations, not sender assertions; later blocked outcomes do not inherit stale acceptance claims.

On **legacy or team** boards, `record`, `settle` and `archive` may instead carry
`human` and current `reconciliation` references when paused/stopped with no live
pass lease. This cleanup-only path preserves every settlement/acceptance gate;
it never reserves, binds, dispatches, creates timers or resumes. A live lease
must release or be explicitly fenced first. Do not temporarily enable a paused
legacy board or create a heartbeat just to obtain a cleanup lease.

Before approved Breakdown publication, reserve the real parent identity with
`graph: true`. The initial assignment stays immutable for idempotent reservation replay; `cover` adds actual returned child IDs to effective group coverage. Partial receipts persist across passes with `complete: false`.
Any unresolved active graph blocks new delivery reservations and bindings:
RUN must check this before external creation, not discover bind rejection after creating. Existing work and non-delivery research/Discovery continue.
After complete tracker/edge/grouping reconciliation, `cover` with `complete: true`
allows the grouped owner to bind. Dropped IDs, overlapping owners, stale tokens
and changed complete graphs fail; repeated identical receipts do not accumulate.
`cover` can preserve already-issued publication results while paused but does not
grant permission to publish more tickets or start delivery. Human-directed
abandonment still requires `settle`'s reconciled custody/result acceptance.

`config`: `id`, normalized `repository`, `commonDir`, `cwd`, `projectId`,
`workspaceId`, `humanOrigin`, `anchor`, `setupEvidence`, `authority`,
`capabilities`, `mapping`, `retirement` are nonempty strings/evidence references;
`merge` is `human` (normal setup default) or `orchestrator`; unknown values fail.
Orchestrator mode requires `mergeGate` with nonempty `source`, `authority`,
`roast`, `ci`, `lint`, `rubberDuck` and `verification` references under
[MERGE](MERGE.md). Missing gate information requires human clarification.
These references describe the repository gate, not completed candidate checks or bypass permission. Human-mode boards retain existing semantics without needing a gate object. For existing activation, use `configure-merge` after human-directed pause/reconciliation, never reinitialization or direct JSON editing. A matching update is idempotent; workers, schedule, pending results,
run history and unrelated config remain intact. Positive integer `capacity` defaults to **6**.
New setups explicitly save `cron`, defaulting to `*/5 * * * *`: supported values
are `* * * * *` or `*/N * * * *` for integer N from 1 to 59, using Paseo's
minute-field cron semantics. Omission preserves existing one-minute boards; the helper never silently changes an adopted job's cadence. Other cron forms
are rejected for this bounded frequent-check recipe.
`wakeupMode` is `fresh` or `heartbeat` (omission preserves legacy fresh configs;
unknown values fail). New setup records its choice and explicit `wakeupConsent`
decision reference. Heartbeat requires that nonempty consent reference and
`pmAgentId`, the actual bound PM agent ID; a fresh config cannot contain
`pmAgentId`. Reinitializing with different mode/config is not a fallback.
These are machine-local activation data, never committed defaults.
`schedule`: actual `id`, `cron` matching the configured value (legacy omission
means `* * * * *`), matching `cwd`, `projectId`,
`workspaceId`, `enabled: true`, `evidence` for mode-specific verification and `observation`
for the initial actual observation. Fresh uses `kind: "schedule"` (legacy
omission accepted) and no `targetAgentId`. Heartbeat requires `kind: "heartbeat"`,
`targetAgentId` equal to `config.pmAgentId`, and nonempty `settings`: a reference
to the exact approved prompt, timezone, lifetime/run budget and runtime settings.
For heartbeat, preserve the successful `create_heartbeat` receipt and join its
returned target to actual PM/agent/workspace/Git observations. Verify returned
prompt, cadence, active status, next run and lifetime against the approved request
using [RUNTIME](RUNTIME.md#same-agent-heartbeat-surface); no schedule inspection
API is needed. Fresh mode still requires actual schedule readback.
The helper checks reference equality, not external truth. Unknown kind, wrong target/mapping/cadence or unapproved
fallback fails. This is **observed state**, not parameters for `create_schedule`
or `create_heartbeat`; cwd/project/workspace are joined observations, not invented
heartbeat creation arguments. Stored `enabled` describes the last verified binding, not current external health after pause/deletion.

Heartbeat `claim.owner` must equal the bound actual PM agent ID, including on
diagnostic passes; fresh claims continue to use each actual fresh run owner.
Every successful claim mints a new token, even for the same agent. `release`
clears only that lease: the heartbeat PM returns/idles for its job and is **not
terminal**. Durable delivery/Discovery IDs and reservations stay intact. This helper never launches/archives agents, creates/deletes jobs or proves recurring delivery; these require supported runtime operations and separate observations.

### Human-only heartbeat recreation

`pause`/`stop` first closes the local gate and preserves the active lease/children.
Then the bound agent deletes its owned heartbeat through MCP and preserves the
successful acknowledgement for that exact ID as deletion evidence. Schedule
listing/inspection cannot verify heartbeat absence. No heartbeat pause/resume
API is assumed. Uncertain deletion stays gated; reconcile through a supported
heartbeat-specific surface or the human, never duplicate. A resumed heartbeat uses a **new** verified ID
on the same PM agent, not a new agent/config/board. Record human creation intent
and settings before the external call and reconcile uncertain creation.

Helper `resume` accepts that replacement only while paused/stopped, with no live
lease (old owner released or human-fenced recovery), matching existing config,
kind/target/cadence and unchanged `settings`, plus:

```json
{
  "replacement": {
    "oldId": "exact-previous-owned-heartbeat-id",
    "human": "explicit-human-recreation-decision",
    "absence": "exact-owned-id-successful-delete-receipt",
    "reconciliation": "current-ownership-children-scope-target-settings-evidence"
  }
}
```

These are required evidence references, not self-authenticating approval.
The caller verifies actual human origin, acknowledged exact-ID deletion (or other supported definitive absence evidence) and grant lifetime; invented strings cannot give a tick authority. Preserve original expiry
and remaining run budget, not a fresh grant on recreation. The helper appends
old job/replacement evidence to `pm.wakeupHistory`, preserves config/workers/
pending outcomes/run history, and rejects stale tokens. Replaying replacement,
resuming the deleted ID, changing target/settings or unproved replacement fails.
Fresh schedules retain same-ID supported pause/resume; their IDs **cannot** use
this exception. A deleted fresh job needs separately reconciled human setup.

## Failure and recovery

Each mutation exclusively creates `<board>.write-lock`, reads current state,
validates the transition and writes/fsyncs a private `<board>.next` file before
atomic rename. Readers see the previous or next complete JSON, not partial
writes. This does not guarantee power-loss durability on every filesystem.
An existing lock or leftover next file fails closed; do not retry a tight loop.

There is **no TTL**, lease extension on heartbeat delivery or automatic lock takeover. For
an abandoned pass, the human recovery owner must prove the old controller cannot
act, inspect live descendants/wakeups/partial work, preserve all results and
sequence acknowledged custody; only then call `recover` with the exact token
and accessible fencing evidence. The caller verifies runtime assertions, not this offline helper. All commands serialize, so an old token fails after
recovery, but an already-issued external operation still needs reconciliation.

An abandoned **transaction** lock or `.next` file instead needs specific
human-authorized filesystem repair after every possible writer is stopped:
preserve both candidate files, inspect which complete state committed, restore
the authoritative board and remove only those exact owned stale artifacts.
The helper deliberately has no force-unlock/TTL escape. Never bypass uncertainty by deleting the board, erasing reservations, killing unrelated processes or reinitializing. Unknown/corrupt state is a blocker.

Unit/CLI tests exercise filesystem exclusion, transitions, custody records and
capacity. [SCENARIOS](SCENARIOS.md) separately covers actual runtime/agent
behavior; a passing test is not an activation receipt.

## Team mode

New setups explicitly use `config.team: true` with the persistent PM heartbeat.
Capacity means **developer slots**, not legacy delivery owners. Keep the same board, lease and operation records; no separate team daemon.

Old configs retain old units and behavior. To adopt the team, pause, fence/release
the pass, settle all old workers and pending operations, reconcile all existing
jobs, then call `enable-team` with `human` and `reconciliation` references.
It preserves old settled assignments/history, staying paused. Do not reinterpret
active reservations or discard them to fit the new limit. A legacy fresh runner cannot be converted this way: obtain an explicit stopped, reconciled handoff to the persistent PM; never change a live job's agent target.

Use the paused human-management path above for legacy outcomes and settlement.
Preserve a retained Discovery agent's actual acknowledgment, unanswered questions
and temporary receiver custody; administrative end is not completed alignment
or permission to archive that continuing agent. After conversion, verify old
heartbeat absence, create its replacement once and use the normal human
`resume` contract. Then claim the initial work pass, reassign retained roles
and route eligible work under SKILL/TEAM. Conversion itself never enables dispatch.

Team `reserve` adds `work` for `kind: "delivery"`:
`feature` costs two, `bug`, `hardening` and `refactor` cost one.
Missing/unknown work fails. `research`, `roast` and `investigator` are bounded
support assignments; `discovery`, `shepherd` and `coordinator` are singleton
roles outside developer capacity. Coordinator needs the configured orchestrator
merge grant plus the actual human request in its packet. No automatic grant.

Team `bind` requires `permissions` and, for delivery, the observed `worktree`.
The permission proof is:

```json
{
  "parent": {"provider": "actual-provider", "modeId": "actual-mode", "features": {}},
  "child": {"provider": "actual-provider", "modeId": "actual-mode", "features": {}},
  "authority": "current-human-grant",
  "evidence": "actual-parent-and-child-readback"
}
```

Use the actual permission features, including `auto_accept` when exposed;
`{}` does not mean "ignore features." Same-provider launches keep identical
inheritance: parent and child snapshots must match exactly.

A **cross-provider** launch instead needs a `permission-preflight` recorded
**before** the launch, and the proof adds `preflight`, `parentAgentId` and
`workspaceId` naming it:

| Operation | Inputs beyond lease and `op` | Meaning |
| --- | --- | --- |
| `permission-preflight` | Live reservation `key`, stable `launchId`, current `parentAgentId`, target `workspaceId`, planned `worktree` (required for a delivery `bind` or any `staff`), observed `parent` and `target` snapshots, `authority`, `evidence`, `purpose` (`bind` default or `staff`), and `mapping` when providers differ | Record the verified target-policy mapping and the exact planned placement before dispatch. `bind` intent must precede binding; `staff` needs the bound delivery. Replaying the identical plan is idempotent; a changed plan needs a new `launchId`. |
| `permission-launch` | Same reservation `key`, existing `launchId`, the `agentId` the runtime actually created, the observed `workspaceId` and `worktree`, and creation-receipt `evidence` | Bind the plan to the child that was really launched under it. The plan cannot know that ID beforehand, so record the receipt after creation and before binding or staffing. A placement differing from the plan, or a second receipt, fails. |

`mapping.kind` is `equivalent` for a verified same-meaning target policy or
`authorized-mapping` with the human's `authority` for an approved difference.
It also needs `preservesChoices: true`, plus `sourceCapabilities`,
`targetCapabilities`, `rationale` and `evidence` references naming the actual
provider policies compared. Ambiguous or escalating mappings have no verified
equivalent: queue the launch and ask the human for that one decision instead.
At bind or staff time the helper rejects any drift between the recorded plan
and the observed parent/target snapshots, authority, parent agent or workspace.
It also requires the recorded launch receipt and rejects any agent or worktree
other than the planned worktree and the actually launched child, so one plan
cannot authorize a different developer, worktree or later launch. Reuse the mapping policy only with a fresh `launchId` per child. This is
truthful mapping evidence, not proof of transferred approvals, credentials or
provider policy. Never manufacture matching snapshots or launch receipts, and
never widen the target policy to launch. Helper evidence strings point to, but do not prove, authority, external settings or filesystem state.

These operations use the same PM `owner`/`token`. Roles return receipts to PM, never write this board.

| Operation | Inputs beyond lease and `op` | Meaning |
| --- | --- | --- |
| `staff` | Delivery `key`, actual `agentId`, isolated `worktree`, `permissions`, `evidence` | Bind a real writing descendant inside the reserved one/two slots. Duplicate writers/worktrees, excess staffing and any prior blocker participant's agent or worktree fail. Include a route owner here if it writes. |
| `retire-developer` | Delivery `key`, member `agentId`, `noLiveWriters: true`, `noUntransferredDuties: true`, preserved `result`, receiver `acceptance`, actual `archive` readback and `evidence` | End one developer binding after verified retirement; preserve its history and the lane's outer reservation. New task/integration workers may fill that slot; uncertain retirement cannot. |
| `role-heartbeat` | Role `key`, `action`, `evidence`; fields below | Record target-executed heartbeat lifecycle. Does not call Paseo. |
| `block` | Delivery `key`, covered qualified `issue`, independent `investigator`, `selfReview`, `challenge`, `missing`, `category`, `evidence` | `category` is `work`, `permission` or `human`. First work blocker returns `retry`; second returns `blocked`. Other categories block without retry. Replayed identical attempt does not increment. |
| `unblock` | `issue`, `resolution`, `readiness`; `human` for permission/decision blockers | Close that episode after actual answer/readiness; preserve history. Does not change tracker labels. |
| `cleanup-ready` | Settled/archived `key`, `noLiveWriters: true`, `clean: true`, full remote `branch`, matching `localHead`/`remoteHead`, `evidence` | Record verified preservation **before** exact owned-worktree removal. Dirty files, missing/unequal remote proof fail. An identical replay is idempotent; a changed one fails, and a removed or retained worktree is terminal. |
| `cleanup` | `key`, actual removal `evidence`, or `retained: true` with the deliberate-retention `evidence` | Record performed cleanup only after preservation, or record that the worktree is deliberately kept. Does not delete anything. Retention and removal exclude each other; an accepted receipt never changes, so reconcile custody instead of replacing it. |

`role-heartbeat` applies only to bound Shepherd/Discovery workers:

- `action: "plan"` needs approved `settings` reference. Save before the role
  creates its job. An existing pending/active/uncertain job blocks another plan.
- `action: "created"` needs returned `id` and `targetAgentId` matching that
  worker, plus complete creation receipt evidence. Record late create receipts after pause so PM can direct exact-ID deletion.
- `action: "observed"` records an actual bounded wake. No synthetic health
  from a cron string. `action: "uncertain"` records a failed/unknown operation.
- `action: "deleted"` needs the exact owned `id` and successful deletion
  evidence. Uncertain deletion does not settle the role. Plan recreation only
  after definite absence, remaining grant and enabled PM are reconciled.
- `action: "absent"` needs definitive `absence` evidence: creation failed
  without an external effect, or supported reconciliation proves no job exists.
  Include the exact `id` when one was known. This permits settlement or a new authorized plan without fabricated creation/deletion receipts. Generic
  not-found and transport errors are not proof of absence.
- On a team board after human pause/stop with no pass lease, this operation may instead carry
  `human` and current `reconciliation` references to preserve deletion/late
  receipts. The same cleanup-only path permits `record`, `settle`, `archive`,
  `retire-developer`, `cleanup-ready` and `cleanup`. It never grants dispatch,
  staffing, timer creation or resume. A live lease must first release or be
  explicitly fenced; stale tokens remain invalid.

PM's heartbeat stays in `pm.schedule`; support jobs stay on their owning worker.
Together they form the project heartbeat inventory; PM checks every one on pause/stop and role retirement. `settle` rejects any unresolved role heartbeat.
The helper does not verify a provider deletion merely because evidence is text.

Blocker episodes live in `pm.blockers`, keyed by qualified issue, not tick.
Retry requires the predecessor settled and archived. Each attempt records its
actual `participants`: the bound role plus every developer it staffed,
including already retired ones. Binding/staffing refuse **any** prior participant's agent or worktree; confirmation also requires a fresh investigator. Changed or missing participant history fails closed.
After escalation, helper reservation refuses covered blocked issues even if the
tracker still says ready. PM separately records tag/comment/backlog writes and
spawns/reuses Discovery under TEAM. Keep uncertain tracker operations pending.
