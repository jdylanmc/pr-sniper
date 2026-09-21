# Paseo capability gates

Load [TRANSPORT](TRANSPORT.md) for missing-tool diagnosis, verified CLI selection
and operation-specific fallback. Server catalog and harness discovery are
different layers; provider approvals are not OS permissions. Diagnose within
existing authority, without changing runtime configuration or dispatch gates.

## Recommended orchestration recipe

Paseo's current [orchestration workflows](https://paseo.sh/docs/orchestration-workflows.md)
pair periodic continuation through a same-agent heartbeat with worker delegation,
progress checks, follow-up prompts, isolated implementations and independent
review. Its [schedules guide](https://paseo.sh/docs/schedules.md) distinguishes
heartbeats for reassessing ongoing work from fresh agents for recurring jobs.

For this ongoing team, use **the primary human chat as PM with a heartbeat**, five minutes by default, with an explicitly recorded minute-step cron. PM provisions the shared Shepherd and optional backlog manager's own
heartbeats under [TEAM](TEAM.md); each target executes its own create/delete.
Each prompt executes RUN once, using Joe-mode's decisions and shared board.
Completion callbacks handle normal progress; recurring passes catch missed
returns, stalls, scope drift, new requirements and changes to the backlog path.
Kickoff records idle-shutdown authority. Apply TEAM's useful-work-or-shutdown
decision on every pass: known human waits do not justify keeping paid agent
turns alive. Remove owned timers when no useful authorized work remains;
retained conversations and PRs are not recurring monitoring requirements.
Fresh scheduling remains an option only when its placement/lifetime gates pass.

Use only operation-specific supported MCP or CLI routes under TRANSPORT, not
universal CLI equivalence; this recipe needs no custom SDK service. The [TypeScript SDK](https://paseo.sh/docs/sdk.md)
is a client of the same daemon, not a separate scheduling guarantee. Never invent SDK scheduling methods or install a long-running service merely to wrap an available tool.

A heartbeat **does not repair its own dead PM agent** or survive every provider,
permission or host failure. Reconcile the bound agent and wakeup evidence when resuming, report
the observation gap, and require explicit fenced takeover if the PM is lost.
Do not claim perpetual supervision from a stored cron record.

The legacy scheduling notes below review primary sources; the permission section also records a bounded live probe, not a rollout. Consult the current
[official index](https://paseo.sh/llms.txt),
[orchestration](https://paseo.sh/docs/orchestration.md),
[workflows](https://paseo.sh/docs/orchestration-workflows.md),
[schedules](https://paseo.sh/docs/schedules.md),
[schedule CLI](https://paseo.sh/docs/schedules-cli.md) and
[MCP reference](https://paseo.sh/docs/mcp.md). Use the installed Paseo reference
for operations and Paseo-help for product questions when available. Missing documentation/tool discovery limits capability; it does not permit guessed APIs.

## Known incompatible fresh-schedule mapping

At upstream source **799c91b39d6a8dad6fb4ccf385d789285a29e260**, inspected
2026-09-14:

- [Schedule service, lines 884–982](https://github.com/getpaseo/paseo/blob/799c91b39d6a8dad6fb4ccf385d789285a29e260/packages/server/src/server/schedule/service.ts#L884-L982)
  creates a workspace for each new-agent run; local isolation calls the directory
  factory rather than accepting an existing workspace binding.
- [Bootstrap, lines 1270–1284](https://github.com/getpaseo/paseo/blob/799c91b39d6a8dad6fb4ccf385d789285a29e260/packages/server/src/server/bootstrap.ts#L1270-L1284)
  calls directory provisioning with cwd/title, not project or workspace IDs.
- [Provisioning, lines 201–224](https://github.com/getpaseo/paseo/blob/799c91b39d6a8dad6fb4ccf385d789285a29e260/packages/server/src/server/session/workspace-provisioning/workspace-provisioning-service.ts#L201-L224)
  generates a fresh workspace ID without an explicit context ID. Same cwd does
  **not** establish reuse. The service can also archive the entire run workspace
  in its `finally` path, depending on configuration.

**Fresh-mode activation is blocked on this implementation.** The schedule schema available
to the reviewed workflow has no explicit `workspaceId`/`projectId`; never invent
those arguments. Even with repository project reuse, a new workspace every minute violates the required mapping. Do not quietly use worktree isolation,
manual agent loops or a heartbeat instead. Setup may recommend a **same-agent
heartbeat with explicit operator consent**, preserving the one-workspace rule.
Required fresh mode needs supported mapping in the deployed version; refusing the alternative leaves activation blocked. This skill neither
patches Paseo nor silently changes the chosen mode.

## Same-agent heartbeat surface

At the same source pin, the
[heartbeat CLI](https://github.com/getpaseo/paseo/blob/799c91b39d6a8dad6fb4ccf385d789285a29e260/packages/cli/src/commands/heartbeat/index.ts)
creates a schedule with `target: {type: "agent", agentId}` from the actual
calling agent identity and checks that ownership before update/delete. Its
update command changes cadence/timezone only; it is not a pause/resume operation.
Current discovered MCP exposes `create_heartbeat` (cron, prompt, optional name,
timezone, maxRuns, expiresIn) and `delete_heartbeat` (id), **not** a target-agent
creation argument or `pause_heartbeat`/`resume_heartbeat`.

Prefer MCP creation with complete evidence. CLI creation remains blocked on
Paseo 0.8.0's lossy receipt; verified CLI deletion is permitted only under
[TRANSPORT's exact-owner gates](TRANSPORT.md#heartbeat-evidence-at-paseo-080).
This replaces the former adapter MCP-only blanket, not an explicit human
MCP-only restriction. The actual target role must execute its own create/delete
within its human-authorized lifecycle, including deletion under a recorded
idle-shutdown grant; bootstrap/reviewer calls bind the wrong agent.
Never alter `PASEO_AGENT_ID` or fake detach.
**Do not use schedule APIs to verify heartbeats.** The current
[MCP reference](https://paseo.sh/docs/mcp.md) limits schedule listing, inspection,
logs, pause/resume and run-once to new-agent schedules. At source
**174055a63c4651bff2a5ab5a316e8d7a403a3444**, inspected 2026-09-14,
[the MCP handlers](https://github.com/getpaseo/paseo/blob/174055a63c4651bff2a5ab5a316e8d7a403a3444/packages/server/src/server/agent/tools/paseo-tools.ts#L2568-L2683)
return the created heartbeat summary, filter agent-target jobs out of
`list_schedules`, and reject them in `inspect_schedule`.

Use the successful **creation receipt** as configuration evidence. Verify its
actual `id`, `target.type: "agent"`, `target.agentId`, `status: "active"`,
`prompt`, `cadence.expression`/timezone, `nextRunAt`, `expiresAt` and `maxRuns`
against the approved request. Join its target to the actual PM's current
agent/workspace/project/cwd/Git observations. Runtime settings belong to that agent, not fresh-schedule config. The helper's flattened `kind`, `enabled`,
`cron` and mapping are this verified join, **not API creation fields**.
Save the full request/receipt and human decision in the existing private packet.

A complete creation receipt plus actual initial backlog/ownership observation suffices to enable the local board. Schedule inspection rejection alone does not justify deleting a successfully configured heartbeat. This proves
**configured**, not **recurring operation verified**. Later actual wakeups in
the same PM conversation, with runtime provenance and bounded-pass receipts,
establish recurring delivery. A copied prompt or saved cron is not a wakeup.
Report status from those receipts and current agent state, stating observation gaps; never fabricate a heartbeat-list/inspect API or require one to finish setup.
Configured also does not prove workers started. Human heartbeat activation/resume
continues through SKILL's first bounded work pass and TEAM's explicit startup
outcome; record initial dispatch separately from later recurring-wake evidence.
The PM prompt must require TEAM's revision-deduplicated human-wait notifications
and useful-work-or-shutdown decision, not unchanged reminders on every pass.
Use a verified runtime-provided agent/conversation link when available; if no
link surface exists, retain exact project/workspace/title/ID navigation details.
Link support is optional, but presenting the actual question in the primary
human conversation is not. For fresh runners, verify the human-origin return
channel rather than assuming the human reads each disposable run's transcript.

For adoption/recovery, inspect the saved operation receipt and actual caller
identity, pending changes and received wakeups. A stale receipt alone cannot prove a heartbeat remains active. For lost creation responses or uncertain current job identity/state, keep the board gated; ask the human or use a separately verified heartbeat-specific runtime surface. Empty schedule lists or schedule-only rejections prove neither heartbeat absence nor failure.
Do not create another job to probe the uncertainty.

Pause/stop, including preauthorized idle shutdown, closes the local board gate **before** deleting the exact owned
heartbeat; queued prompts must return without dispatch. Keep the PM agent for
pause/resume and unresolved duties. Human recreation requires verified complete acknowledged deletion of the exact old ID, released/fenced old pass and preserved target/scope/settings. `delete_heartbeat` returns `{success: true}`
after deleting the caller-owned heartbeat at the source above. The version-verified
CLI instead returns `{id, status: "deleted"}`; require its full ID to match the
exact owned request and actual caller under TRANSPORT. Preserve the distinct
external receipt as deletion evidence without a second schedule query. A transport error,
generic not-found or wrong-target error is not that acknowledgement. Reconcile
uncertain delete/create responses through supported heartbeat-specific evidence
or the human before retry/replacement. Bound unchanged runtime failure
reconciliation to TEAM's one attempt; do not keep other working timers alive
to repeatedly rediscover a lost owner's deletion failure. Report incomplete
cleanup and retain its exact ownership/evidence without inventing absence.
Preserve applicable absolute expiry and remaining run budget; recreation must not silently extend the original grant. Stop/end permits retirement only
after owned-wakeup absence and accepted transfer/end of all children/reporting.

## Required activation evidence

Before any job creation, identify actual daemon version, supported schemas/source behavior, PM placement and granted capabilities below.
After creating/adopting the one job while locally paused, verify its mode-specific
binding evidence and initial observation before enabling claims. Only subsequent receipts prove recurring delivery; future results are not precreation evidence.

1. **Fresh only:** same explicit cwd resolves each fresh scheduled run to the **existing**
   correct project/workspace, without per-run duplicates. Verify the effective
   repository/common directory/branch mapping, not only schedule configuration.
2. **Fresh only:** the schedule cannot auto-archive a shared workspace at finish/failure/restart.
   Use only supported settings verified against that runtime; do not guess an
   `archiveOnFinish` parameter from a different surface's implementation.
3. **Heartbeat only:** the human explicitly chose this conversation/lifetime mode.
   Verify the primary/reused PM agent is already in the one correct existing
   workspace/project/cwd, not a disposable bootstrap/reviewer, and the supported
   API binds the caller. After creation, verify the receipt's actual target
   equals that agent. Same-agent delivery must not provision/retire workspaces per tick. This mode needs no fresh-workspace-factory proof, but must prove actual agent binding and safe ongoing lifetime on the deployed host.
4. **Both modes:** PM passes can read/write the same durable board, inspect known agents,
   descendants, permissions and mode-specific wakeup evidence, and preserve/report results
   with narrow human-granted tools. Scheduler availability is not tool access.
5. **Both modes:** parent/child archival and callback visibility are known, or each still-needed
   parent is explicitly retained with a bounded role. `archive_agent` interrupts
   a running agent; `archive_workspace` may remove an owned Git worktree. Agent
   placement does not detach parentage. Never fake detachment or change
   `PASEO_AGENT_ID` to escape ownership.
6. **Both modes:** host/daemon availability and actual initial observation are verified. Later
   recurring receipts establish coverage, with explicit gaps; for heartbeat,
   observe multiple receipts on the **same PM agent**, not new agent launches. Tool permission prompts can indefinitely block unattended reads; report missing approval to the human, not successful monitoring.

Evidence strings in the local helper are references, **not proof**. Unknown
mapping, access or retirement behavior stays unknown. Installation and local
tests can succeed while runtime activation remains blocked.

## Permission-preserving dispatch

Inspect the invoking parent's live provider, `currentModeId` and permission
features. Record the actual human grant and later UI changes/revocations, not merely setup-era mode. A feature such as `auto_accept` is separate from
`allow-all`. Never infer either from a mode label alone.

Before every role/worker launch:

1. Inspect provider capabilities, available models and configured profiles.
   Choose a current frontier model for substantive work by discovery, not a
   remembered name. Profiles may select model/reasoning, never silently replace the human's permission choice.
2. For the same provider, pass the authorized `settings.modeId` and permission
   `settings.features` explicitly before the initial prompt. Do not copy
   unrelated provider features blindly. Carry narrower task limits in the packet.
3. For a different target provider, compare the actual target policy with the
   parent's authorized one and record STATE's `permission-preflight` before the
   launch: `equivalent` when the target expresses the same meaning, or a
   human-authorized mapping for a genuine difference. Plan the exact workspace/worktree there; no child ID exists yet. After creation, record `permission-launch` with the actual returned ID, binding the plan to that child in that worktree. Ambiguous or escalating
   mappings wait for the human. Fabricated matching snapshots or launch receipts
   are never mapping evidence, and no mapping transfers approvals, credentials
   or provider policy.
4. Read child mode/features after startup and workflow initialization. Join its
   actual ID/workspace/cwd to the assignment. Bind only after readback matches
   the recorded plan. A mismatch holds the reservation and affected work; diagnose once, avoid a permission storm. Do not call a broad mode "drift"
   merely because an old default was narrower.
5. Respect later human changes. Verify provenance; if a recorded grant and live
   settings disagree without a known human change, ask once rather than
   downgrade, upgrade or replay approval. Pause affected child work for a
   revocation. Parent mode changes do not automatically update existing children.

Do not claim individual approvals, credentials or provider-specific policy
transfer across sessions. Cross-provider launch needs an explicit equivalent
policy supported by the target, with human approval for any difference.
No equivalent capability: queue it and report the precise missing choice.
Never edit global config, auto-approve a pending request or swap providers to
work around denial. Chooser-style permission requests may still need a human.

### Deployed 0.7.2 evidence

Inspected installed Paseo **0.7.2** on 2026-09-15. Corresponding upstream tag:
[`9400a49`](https://github.com/getpaseo/paseo/tree/9400a49af670fdb5db4af58e73f8df98588dbea9).
Inspected compiled paths below are under the installed server's
`dist/server/server/`; the tag identifies the release, not a source-map proof:

- `agent/create-agent-mode.js` and `agent/provider-snapshot-manager.js`:
  same-provider omitted mode inherits the parent's current mode. This is not
  blanket settings inheritance.
- `agent/providers/acp-agent.js`: an unattended parent can supply omitted
  `auto_accept`; explicit false is needed when prompting is intended.
  Auto Accept handles allowed non-chooser permission requests independently of
  Copilot's Allow All setting. It does not drain existing pending requests.
- `agent/providers/copilot-acp-agent.js`: `allow-all` maps to Copilot's
  `allow_all` session option. Leaving it turns that option off.
- `agent/tools/paseo-tools.js`: creation maps `settings.modeId` and
  `settings.features` before the initial prompt. Heartbeat creation binds the
  caller; deletion checks that same owner. Named MCP creation is replace-by-name
  for that target, not permission to retry unknown outcomes.

A disposable same-provider child received explicit Agent mode plus
`features.auto_accept: true`, matching its parent's observed settings.
Child readback matched; a constant-output shell call succeeded without a pending
request. It created one self-targeted heartbeat with expiry before its next
scheduled tick, deleted the exact ID successfully, then was archived.
This proves explicit configuration and target-bound create/delete worked on that host. It does **not** prove automatic inheritance, recurring wakes,
cross-provider equivalence or production rollout. Recheck deployed capabilities.

A second bounded probe on 2026-09-15 used two current frontier models through
the same provider. Both children received the parent's actual Agent mode with
`auto_accept`, which their readbacks matched; no pending permission request
appeared. A delivery child and an independent investigator each reported the
same missing input, and a distinct fresh agent on a fresh worktree confirmed the
second blocker. One self-bound timer fired on its target and that same caller
deleted it with an acknowledged success receipt. All four probe agents were
archived with verified closed/archived state.

That probe proves these **primitives** on that host. It does not prove board-helper
integration, nested launches beneath a route owner, a six-slot pool over long
runs, pause/resume across all three role heartbeats, or cross-provider policy
equivalence. Agent plus `auto_accept` is also not Allow All, and no observed
native-provider bypass has been shown equivalent to it; never infer a wider
grant from these results.
