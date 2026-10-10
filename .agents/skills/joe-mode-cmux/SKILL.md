---
name: joe-mode-cmux
description: "Human-only session Joe-mode cockpit in CMUX. Reconcile one repository controller, launch pinned Maestro workers, and arrange PM, Discovery, stacked developers, and support without claiming unattended execution."
disable-model-invocation: true
user-invocable: true
---

# Joe-mode CMUX

Run Joe-mode as a visible, interactive CMUX cockpit. This is an adapter around
[Joe-mode](../joe-mode/SKILL.md), not another project-management policy or
controller. The human-authored [intent](intent.md) defines its purpose.

Use Joe's [delivery pace](../joe-mode/DELIVERY-PACE.md) and
[invoker doctrine selection](../doctrine/APPLY.md#joe-review-operations).

Read [LAYOUT](LAYOUT.md) before changing CMUX topology and [RUNTIME](RUNTIME.md)
before registering or spawning through Maestro. Apply Joe-mode's
[runtime guidance](../joe-mode/RUNTIME.md), shared
[team contract](../joe-mode/TEAM.md), and
[agent lifecycle](../squadron/LIFECYCLE.md). Independent writing deliveries
still follow [worktree placement](../ship/WORKSPACE.md), and pull-request
handoffs still follow [delivery readiness](../ship/DELIVERY.md) and
[Shepherd observation](../shepherd/OBSERVATION.md).

Use [Joe role worktrees](../joe-mode/WORKTREES.md): Project Manager stays on
`main`, Discovery uses `discovery/<feat>`, and an explicitly authorized
PR/auto-merge coordinator uses `pr-sniper`. Fast-forward clean owned main
when remote main changes and after each confirmed merge. All role worktrees
remain in the same CMUX workspace; pass their exact paths to Maestro.

## Entry and ownership

**Human activation only.** Installing, discovering, restoring, or mentioning
this package does not start it. Once activated, it remains the session's
Joe-mode presentation until the human pauses, stops, or re-anchors it.

Resolve the repository, anchor, selected backlog, scope, exclusions, tracker,
and human authority exactly as Joe-mode requires. Reconcile the same repository
owner board with session Joe-mode and any surviving legacy runtime owners
before registering CMUX or dispatching. There is one logical controller per repository
across these entrypoints. Removing old adapter files does not retire live
agents/jobs or transfer their authority. Join the existing controller or obtain observed release and
acknowledged transfer. Unknown ownership blocks activation.

The invoking conversation remains Project Manager and owns the human
conversation. Do not invoke another Joe-mode controller. Use Joe-mode's current
local routing sections directly under this adapter's human grant.

## Activate the cockpit

1. Verify the Project Manager is in the selected repository's owned `main`
   checkout and reconcile remote main using the guarded worktree procedure.
   Read its guidance and current owner board. Never switch or reset a dirty
   delivery checkout to manufacture main placement.
2. Verify exact caller identity and all blocking checks in [RUNTIME](RUNTIME.md):
   this conversation must already be a managed coordinator with native peer
   and launch tools. Missing communication blocks activation, not merely layout.
3. Reuse its exact managed identity; do not register another owner. Present it
   as `PM · Joe Mode` with `md-meditation` (`󱅻`) in teal only through supported
   owned metadata operations. Keep control tokens private.
4. Inspect the existing workspace tree. Reconcile the four areas from
   [LAYOUT](LAYOUT.md) additively when [RUNTIME](RUNTIME.md)'s placement
   capability passes, without creating a new workspace/window, stealing focus,
   or mutating an unowned surface. Otherwise keep managed workers beside the
   Project Manager and report the degraded layout.
5. Rename the repository workspace and publish an `active` Joe status only
   after ownership, CMUX identity, and Maestro readiness are verified.
6. Verify that the peer tools actually work in this session. Registration does
   not make the Project Manager a recipient. Do not replace missing native
   communication with human relay or generic harness helpers.
7. Complete one useful bounded Joe-mode pass now: refresh the selected work,
   launch needed roles, and report actual dispatch, blockers, or human waits.
   A layout alone is not an activated team.

## Place useful roles

Keep one interactive Discovery owner when the anchor has unsettled product,
architecture, requirements, or planning questions. Its blue compass surface
stays in the Discovery pane while waiting for the human when placement is
supported; otherwise it remains beside the Project Manager. Do not create
several workers asking competing questions.

Launch developers only for selected non-overlapping delivery assignments.
Every launch uses native `maestro_spawn`, verified parent-account inheritance,
explicit model selection, and a complete first task.
When [RUNTIME](RUNTIME.md)'s placement capability is verified, move the
returned exact surface into the developer pane and stack additional developers
there as tabs. Otherwise leave workers beside the Project Manager and report
the degraded layout. Name each worker for its delivery at spawn. Sharing a pane
does not permit sharing a writing worktree.

Launch one green shield-check Shepherd while accepted pull-request duties
exist. Put it in Support when placement is supported; otherwise leave it beside
the Project Manager. Support may also display review, test, log, or preview
surfaces. Do not create an idle Shepherd merely to fill the pane.

Respect Joe-mode's six developer slots by default and Maestro's stricter
eight-live-session workspace bound. Effective developer capacity is the smaller
of six and `8 - live managed non-developer sessions (including PM) - retained
resources`. Reserve capacity for Discovery, Shepherd, and any required
interactive reviewer before filling the developer pane. Non-agent test, log,
and preview surfaces do not consume the Maestro worker bound. Finishing an
initial task does not free an open interactive session or retained terminal.

## Route work through Joe-mode

Use Joe-mode's backlog, Discovery, planning, Ship, Patch, Refactor, Roast,
Verify, and Shepherd routes without duplicating them here. Preserve their human
decisions, publication gates, independent review, permissions, ownership,
worktrees, and merge boundaries.

Every nested role retains the Maestro runtime contract, including support and
blocker investigation. Never substitute an SDK/task agent, even after native
launch failure. Report truthful prepared/launching/failed states instead of
counting those assignments as a running visible team.

The cockpit changes presentation, not authority. PR Sniper's independently
requested PR coordinator may merge only through the
[repository merge contract](../joe-mode/MERGE.md); implementers, PM and
Shepherd gain no merge power. Installing this adapter does not activate that
role or configure the PR Sniper app's own approval/merge capabilities.

The same restraint applies to all presentation:

- role names, icons, colors, statuses, progress, and logs never prove success;
- worker launch does not prove task completion;
- terminal exit does not prove accepted work;
- CMUX restoration does not prove live supervision;
- the adapter does not grant tracker writes, approval, merge, production, or
  destructive authority.

Use CMUX status and logs for verified coordination events. Notify the human only
for decisions, material blockers, or pull requests ready for final review.
Keep routine chatter in role surfaces.

## Reconcile and continue

Use Maestro `status`, the Joe owner board, Git/provider state, and actual
artifacts to reconcile outcomes. Use the separately installed `/maestro`
guide for native peer discovery, one fire-and-forget follow-up, and ordinary
replies when this session participates. The Project Manager cannot inject
terminal input or silently read worker conversations. A send is not a receipt,
custody transfer, or task completion; verify the returned evidence and artifacts
under the selected Joe route. When a worker needs human input, identify its
exact tab and surface. Missing native tools do not permit route-file inspection
or a keystroke fallback.

Continue bounded Joe-mode passes while this human conversation remains active.
Do not promise work between turns or after the session ends. If unattended
recurring operation is requested, stop and offer the separately authorized
human to establish a separately reviewed runtime and wake contract; none is
shipped here. Never invent CMUX scheduling.

On pause or stop, follow Joe-mode ownership transfer and Maestro lifecycle
rules. Preserve active deliveries and Shepherd custody. Close interactive
sessions normally before archive; never kill processes, delete terminals, or
discard work to make the UI look clean.
