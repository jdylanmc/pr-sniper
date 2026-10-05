---
name: joe-mode
description: "Human-only activation, one controller per repository. Loop backlog, discovery, planning, and delivery until stopped; aggressively use Squadron for distinct delivery and Shepherd assignments, with event status snapshots."
disable-model-invocation: true
user-invocable: true
---

# Joe-mode

Use [the shared team contract](../joe-mode-paseo/TEAM.md): six developer slots
by default, feature two, bug/hardening/refactor one; support roles are separate.
Prefer TDD for features, with useful legacy exceptions. Delegate blocker
investigation, retry one confirmed work blocker with fresh context/worktree,
then return repeated blockers to the single backlog manager. Session Joe has
no recurring heartbeat; the Paseo adapter alone owns that runtime extension.

Hand the human pull requests to review. Loop existing skills, not a second implementation process. The human-authored [intent](intent.md) defines the purpose.

Use [Doctrine](../doctrine/SKILL.md) under the [common application contract](../doctrine/APPLY.md). Scope explicit selections to the named delivery, not every unrelated backlog item. With none preselected, choose appropriate doctrines per worker from catalog metadata; each work packet carries IDs, required flags, reasons, source locations, and digests. Applying workers retrieve the texts. Require `worktrees` for each PR-producing lane and preserve role-specific requirements such as `solid` for code Roast.

Joe-mode starts only when requested and stays active in this session until paused or stopped. Route subsequent turns within the anchor. A side question does not stop work; explicit redirection does. Bounded workers must not activate another Joe-mode controller. Follow the [invocation contract](../setup/INVOCATION.md): one controller per repository, not per issue, branch, worktree, or selected scope.

The separate human-enabled [Paseo PM adapter](../joe-mode-paseo/SKILL.md)
does not extend this session mode's lifetime. Before taking repository control,
reconcile its saved activation, wakeup job and current pass on the **same owner
board**. Join an existing logical controller or arrange observed release and
acknowledged transfer; an idle gap between scheduled passes is not absence of
ownership. Preserve its paused/stopped state, persistent Discovery lane and
delivery/recovery custody. Unknown cross-session/host ownership blocks competing
dispatch. The adapter may reuse the routing sections below only within its
separate human grant and RUN contract, never by invoking another Joe controller.

Kickoff authorizes ordinary delivery of selected in-scope work: repairs, commits, PR publication, review, and shepherding. Do not ask again at routine transitions. Preserve explicit narrower requests, human product decisions, scope changes, and every planning/recording approval gate. Never delegate human approval or merging.
Record each delivery's narrow monitoring/recovery authority in the
[existing packet](../ship/DELIVERY.md#one-delivery-packet-one-owner), including
this controller and root human conversation. Wake authority never permits
another controller.

## 1. Resolve the anchor

Accept an idea, folder, repository, issue, specification, backlog, or other concrete reference. Identify the goal and exclusions. Resolve a containing or explicitly linked Git repository when one exists; ideas need not arrive as tickets.

Read repository guidance and existing `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, and domain configuration when present. Inspect Git remotes read-only. GitHub remotes normally identify an owner/repository; Azure DevOps remotes identify an organization/project/repository. Code hosting does not prove where planning lives: honor configured separate planning projects or trackers.

Establish the backlog selection from the request and configuration:

- A named issue or specification anchors that deliverable and relevant relationships, not the whole repository backlog.
- A full-backlog request means the selected repository/project/area/query, never the entire organization by inference.
- An assigned-to-me request adds the authenticated provider user's assignee filter; Git commit identity is not proof of that identity.
- A folder or idea may need repository and backlog scope clarified. Ask the smallest material question; do not silently choose full backlog.

Resolve ambiguous remotes, planning scopes, and identities before dispatch. Inspect actual configuration against [Setup's completeness criteria](../setup/SKILL.md#joe-mode-bootstrap-readiness), not file existence or a remembered setup run. Record absent/incomplete configuration for the bootstrap in step 2; do not invoke Setup before repository-wide ownership is established. Its [GitHub](../setup/issue-tracker-github.md), [Azure DevOps](../setup/issue-tracker-azure-devops.md), and [local Markdown](../setup/issue-tracker-local.md) references describe the supported mechanisms. Unsupported or ambiguous existing configuration needs a human choice, not an automatic reset or migration.

Discovery can start without a repository. Report backlog/provider discovery as unresolved; defer tracker publication and delivery until destinations are agreed. An unavailable tracker is not an empty backlog.

## 2. Establish one controller and a work board

Use harness session storage or a uniquely named session/OS-temporary artifact, not a new repository planning file. Record the repository identity, controller ID, anchor and actual objective start evidence, provider and planning scope, assignee filter, mapped readiness role, permissions, active owners and parent relationships, worktree/branch locations, covered item IDs, dependencies, evidence pointers, human questions, PR status, cycle boundaries, and reported event identities.

Reconcile any prior board with live agents and provider state before reusing it. Resolve the common Git directory and normalized repository/provider identity so another worktree or clone is not mistaken for a different repository. Do not duplicate another active Joe-mode owner even for disjoint scopes in that repository: join the current controller or arrange explicit transfer. If visibility or ownership is uncertain, resolve it before dispatch rather than racing another session. A local board is coordination state, not a cross-session lock. An idea without a repository may begin discovery; check repository-wide ownership when its repository is resolved.

Read [runtime guidance](RUNTIME.md) and execute [LIFECYCLE](../squadron/LIFECYCLE.md) before dispatch, transfer, recovery, or retirement. Keep its placement, delivery, custody, and runtime evidence on this board. Confirm the harness supports the requested agents and background work. Use a bounded capacity appropriate to available tools and resources; retain capacity for the human-facing discovery path and for completion/review work rather than filling every slot with new implementation.

### Bootstrap missing setup under this owner

Before invoking Setup, reconcile repository-wide Setup and Joe controller
ownership, including other worktrees/sessions. Join or resume the existing Setup
owner; route its questions through the controller, never start a second run.
Uncertain visibility or ownership blocks dispatch. Record the Setup owner,
workspace, actual configuration gaps, pending decisions, and outcome on the
existing board. Provider/scope fields may stay unresolved during setup; they
do not permit broadening the anchor.

Reuse complete setup without rerunning Setup. When required configuration is
absent/incomplete and no Setup owner is active, automatically attempt current
local [Setup](../setup/SKILL.md) as this human-started controller's bounded
bootstrap subflow. Use the harness skill invocation when registered; otherwise,
if repository guidance permits, directly read and follow its local `SKILL.md`
and required references. Do not merely recommend `/setup`. If neither route is
available, report the missing package or invocation/read capability; do not
install a package or substitute an upstream workflow.

Pass Setup the repository, original anchor/exclusions, objective-start evidence,
controller/Setup ownership, configuration gaps, and existing human choices.
Provider/label decisions and exact-file write approval remain human gates, not
routine delivery permissions. If the human is unavailable or declines, or
invocation fails, record the waiting/blocker state and needed action. Reconcile
partial writes; do not retry unchanged failures in the loop or treat invocation
acknowledgement as success.

After Setup reports completion, re-read actual repository guidance and
configuration against its completeness criteria. If its approved workspace
differs, coordinate availability through its owner; do not copy unapproved files
into this checkout or claim unseen configuration is usable. Resolve remaining
gaps or conflicting/unsupported choices with the human; wait on affected paths.
Resume the original anchor and step 3 only with usable configuration. Refresh
provider scope, identity filter, and readiness-role mapping without widening the
backlog, marking issues ready, restarting the controller, or resetting the
objective clock. Independent work not needing setup may continue within scope.

### Dispatch through the controller

Joe-mode owns routing and the human conversation. Use [Squadron](../squadron/SKILL.md)
for useful independent work within TEAM's developer pool. Reuse one shared
Shepherd for all accepted PR scopes and one optional backlog manager for deep
inquiry. Disposable roasters and blocker investigators return compact evidence.
Each route retains integration; do not launch competing workers beneath it.
Do not delegate this controller. The optional PR coordinator requires the
separately human-enabled Paseo merge contract; session Joe grants no merge power.

## 3. Refresh the relevant backlog

Query only the anchored selection, with the configured `ready-for-agent` role mapping. It may be a GitHub label, an Azure DevOps tag, or a configured local equivalent. Do not invent a second readiness checklist, hardcode a replacement label, or silently mark existing issues ready.

Use narrow queries, pagination, and batch detail retrieval. Load full requirements, dependencies, existing PR associations, and ownership only for candidates or related items needed for scheduling. Distinguish complete empty results from failed or truncated queries. Failed queries cannot justify a replacement backlog.

Readiness is eligibility, not a bypass of dependencies or ownership. Exclude closed/completed items, work reserved by an active delivery, and items blocked by prerequisites absent from their intended base. Do not steal assignments or treat "assigned to me" as proof no other agent is working on the item.

Recheck candidates and known PRs immediately before reserving and dispatching. Use the configured shared claim mechanism if available and authorized; preserve existing ownership. If reliable exclusive ownership cannot be established, report and resolve that limit before overlapping work. Never claim a session-only reservation protects against every external actor.

### Choose non-overlapping deliveries

Use provider-qualified item identities; record each delivery group's full coverage, not just its parent ID.

- **One specification, one PR:** reserve the specification and its child graph for one Ship owner. Ship schedules its internal frontier; Joe-mode does not also launch child Ship jobs.
- **Intentionally separate deliveries:** reserve non-overlapping ticket groups, suppress the spec parent as an implementation candidate, and observe cross-delivery dependencies. Choose this only when the slices are intentionally separate PRs.

Reserve a spec's group before ticket breakdown: `specify` may already have applied the readiness label. Newly published children must not race a parent delivery or dispatch before the approved graph and grouping are recorded. Ask if grouping is materially ambiguous.

A dependency across separate PRs is satisfied only when the required changes are available on the consumer's agreed base, normally after the prerequisite merges. A green but unmerged PR or a closed tracker item alone is insufficient. Inside one Ship graph, Ship's integrated-commit and validation rules govern.

### Consume linked recovery intake

On Shepherd notification/board reconciliation, load/execute
[RECOVERY](../shepherd/RECOVERY.md). Read linked issue/original packet; verify
live refs/ownership/episode; acknowledge actual observed intake before acceptance.
Reuse board coverage/owner, no second queue/controller. Links neither widen anchors,
prove configured readiness, authorize labels, nor permit parallel live repair.

Eligible settled-intent recovery stays bounded, same branch/PR. Changed
requirements/architecture/risk/semantics need Discovery/the human.
Before writes, record outgoing release/receiver acknowledgment. Shepherd remains
observation-only or explicitly safely suspended. Ticks resume the episode, not
new issues/workers. Return fresh artifacts/head/target, validation/independent
review; Shepherd reconciles/accepts. Failed/no-progress repair escalates, no automatic
redispatch.

## 4. Run concurrent paths through the existing flow

Fill available capacity with independent work. Do not stop all delivery during discovery of the next slice or wait for the whole backlog to be specified before dispatching known work.

| Situation | Route and return contract |
| --- | --- |
| A named goal needs its critical task path, missing work, or research spikes identified | [chart-a-course](../chart-a-course/SKILL.md): read-only goal-centered path, proposed tasks, evidence gaps, and one next planning recommendation. Joe owns acting on the result; the map is not tracker publication or delivery authorization. |
| Unsettled question; no defined backlog yet | [discovery](../discovery/SKILL.md): aligned findings, domain understanding, frontier, full foundation and compact handoff. It can request [research](../research/SKILL.md) or [poc](../poc/SKILL.md); those return evidence, not product changes. |
| A focused human question | [interrogate](../interrogate/SKILL.md): actual human answers. During discovery, use its conversation-only intake and let discovery own the alignment gate. |
| Aligned terminology or a consequential architectural choice needs a record | [domain-modeling](../domain-modeling/SKILL.md): glossary and Architecture Decision Records (ADRs) when its criteria warrant one. Distinguish proposals from human decisions; do not generate ceremonial ADRs for every ticket. |
| An aligned Discovery artifact is ready | [specify](../specify/SKILL.md): consume the full accessible foundation and evidence, then produce the full requirements specification. A conversation summary is not a substitute; missing source decisions return to Discovery. |
| An approved spec needs actionable slices | [breakdown-tickets](../breakdown-tickets/SKILL.md): human-approved vertical slices, blocking edges, and configured readiness labels. Reserve the delivery group before publication and reconcile the resulting IDs afterward. |
| External requests need classification | [triage](../triage/SKILL.md): apply the configured workflow to incoming external work. Do not retriage generated, already-ready tickets. |
| A feature, behavior change, or spec graph is ready | [ship](../ship/SKILL.md): scoped implementation and integration through reviewed delivery. A human may ship one issue; Joe may assign several independent deliveries, never overlapping graphs. |
| A bug or regression is ready | [patch](../patch/SKILL.md): reproduce, establish cause, repair, review, publish, and shepherd. Planned behavior changes belong to Ship. |
| A behavior-preserving structural change is ready | [refactor](../refactor/SKILL.md): preserve the contract while restructuring, then review, publish, and shepherd. |
| A published PR needs attention | Its existing [Shepherd](../shepherd/SKILL.md): observe checks/reviews/policies, rebase on base advancement even if mergeable, and return functional feedback to its recorded delivery owner on that same PR. Join the current owner rather than starting another monitor. |
| New evidence invalidates a slice | Return to its owner: defect diagnosis within the current delivery, or Discovery for unsettled requirements. Explicit diagnosis-only scope stays read-only; do not spawn another Patch PR for the same work. |

These are paths through the decision tree, not a mandatory global sequence. When specifications are needed, the order is Discovery artifact, Specify requirements, then Breakdown Tickets. A small, understood issue need not create another spec, ADR, or discovery run. Choose the delivery route by kind, scope, and size; Patch and Refactor are peer routes, not mandatory wrappers around Ship. All share [reviewed delivery completion](../ship/DELIVERY.md), independent [Roast](../roast/SKILL.md), [Verify](../verify/SKILL.md), and mandatory Shepherd. Once a route yields its required output, automatically reevaluate within existing permissions.

When Chart-a-course recommends Discovery for an issue or epic, invoke Discovery
within the existing anchor and authority; do not merely tell the human to invoke
it. Pass the target, unresolved question, evidence, affected tasks, and learning
exit condition. Reuse any existing investigation owner. Preserve Discovery's
human-alignment, experiment, and write gates. An unpublished issue proposal may
anchor an inquiry without a tracker ID; creating it still needs applicable
publication approval. Resolve scope expansion or missing authority with the
human; recommendations are not permission. Feed returned findings and approved
task identities back into Chart-a-course when they change the path. Do not rerun
it on unchanged evidence or treat a provisional path as delivery readiness.

Keep one human-facing discovery/planning conversation moving alongside delivery. A discovery worker returns real questions and waits; Joe-mode presents them to the human and returns actual answers. Never simulate the human's side or let several workers ask competing questions simultaneously.

ADRs and domain documents are repository changes, not incidental scratch notes. Give their worker an agreed isolated documentation/delivery workspace, serialize shared-file edits, and carry approved records into the corresponding reviewed PR or an explicitly scoped documentation PR. Every modifying worker uses [Changelog](../changelog/SKILL.md); the integration owner consolidates notable entries instead of letting workers race on the shared file. Do not let planning workers edit a live delivery worktree concurrently. Where a downstream skill needs repository files before publication, arrange that workspace explicitly rather than copying unapproved documents into the product checkout.

An anchor spanning ready and uncertain work should have both paths active when capacity and human availability allow. When no work is defined, discovery feeds specification and ticketing first. When the human is unavailable, pause only the paths needing their decisions; independently authorized delivery continues.

## 5. Reconcile outcomes and keep looping

Give each worker the anchor subset, concrete inputs and evidence paths, selected skill, owner and workspace, permitted mutations, dependencies, stop conditions, and return contract. Returns must identify produced artifacts/provider IDs, observed checks, unresolved decisions, blockers, and active child or monitor ownership.

On a completion, human answer, PR event, or meaningful backlog change:

1. Read the result; verify decisive artifacts or provider state. A worker's "done" does not prove a published PR, human approval, or completed prerequisite.
2. Reconcile owned item coverage, dependencies, permissions, and pending questions. Record partial writes before retrying; inspect the provider after uncertain publication to avoid duplicate specs, tickets, or PRs.
3. Route newly ready work; release capacity only after receiver-observed, acknowledged transfer or accepted work completion. Retain workers for concrete pending follow-up; retire terminal owned agents under LIFECYCLE, preserving evidence and all remaining PR scopes.
4. Surface review-ready PRs and material human questions; keep unrelated work moving.

If findings contradict an active delivery, notify its owner and pause affected work at a safe boundary. Reconcile scope with the human; do not change requirements underneath a worker or restart the entire backlog. Preserve unrelated progress.

Auto-transition is not blanket approval. Preserve alignment, ticket-breakdown, and explicitly retained recording/publication gates; do not reintroduce a separate permission prompt for routine delivery already authorized at kickoff. Coordinate meaningful approval requests with the proposed change. Never accept product risk, supply a human decision, merge, approve, or enable auto-merge on their behalf.

Use completion notifications or the runtime's documented wait mechanism. Refresh the scoped backlog after relevant events and at an appropriate bounded interval only while a real observer is running. Do not busy-poll, spawn idle agents, or imply a final response leaves an unscheduled loop executing.

### Status at full-cycle and major-merge boundaries

Track a finite cycle: refresh the selected backlog, record the pass's actionable
cohort and planning questions, dispatch eligible work, then reconcile each
assignment's outcome or explicit blocked/human-wait state. A reviewed current PR
with confirmed continuing Shepherd custody is reconciled; do not wait for its
monitor to terminate. A launch acknowledgement or running implementation is not
a completed assignment.

After the full cycle, request one [Status Report](../status-report/SKILL.md)
before returning to the start. Supply the controller's objective, original start
evidence, known descendants, outcome evidence, and cycle identity. Not every
poll or notification is a cycle; do not repeat reports for an unchanged
empty/blocked board.

Also request a snapshot after a **confirmed major-feature merge** in scope.
Establish significance from the agreed feature/spec, not every PR label or agent
claim; verify merged state with the provider. Deduplicate by repository, PR, and
merge identity, and by completed cycle ID. One snapshot may cover coincident
events. Record successful reporting only after a snapshot returns; retry failures
against the same event without resetting the objective clock. Reporting stays
read-only, never another controller.

## 6. Hand over PRs, not just progress

For each delivery, surface the actual PR URL, covered issue/spec references, concise change summary, acceptance/check evidence, outstanding decisions, and confirmed Shepherd owner/status. Distinguish **draft/in progress**, **blocked**, and **ready for human review** using the shared delivery contract and current provider state. The final ready handoff is reviewed, GREEN, and rebased/current with latest main or the explicit target, with checks tied to the current head and a freshly observed base. "PR created", mergeable, or yesterday's green result does not mean ready. Human final sign-off remains outstanding.

Require DELIVERY's actual promotion and provider non-draft readback plus
receiver-observed, acknowledged Shepherd custody. Reconcile each selected PR:
mixed ready/draft/blocked batches are progress, never **all delivered** while
scoped work is unfinished. Do not promote blocked drafts to clear the board.

Every PR handed to the human must have a Roast covering its current candidate, whether produced by this run or supplied by a coworker. Reuse a still-applicable review; otherwise route to Roast without taking over the PR's delivery owner or silently authorizing edits. Review a draft's available candidate with its incomplete scope explicit. Missing review capability requires reporting the gap and seeking direction, not a clean-review or review-ready claim.

Return human feedback to the same owner and PR. A review-ready PR does not end Joe-mode or discovery. After merging/closure, reconcile backlog and dependencies before dispatching more work; do not manufacture follow-up work or close unrelated tracker items.

No progress possible: explain wait; keep this controller available for events/
user turns while Joe-mode is active. Retire accepted terminal workers under
LIFECYCLE; retain concrete waiters/blocked owners with next actions, never invent
tickets to occupy an idle fleet. On pause/stop, stop dispatch; coordinate active
owners' explicit pause/accepted transfer; preserve work/monitoring state; report
still-running owners. Never abandon Shepherd or claim persistence after shutdown.

On re-anchoring, settle active ownership first. Do not silently expand the old scope or cancel its workers. On context pressure, use [phase-boundary guidance](PHASE-BOUNDARIES.md) and preserve the board, decisions, evidence pointers, pending questions, and monitor ownership. Resume by reconciling real state, not replaying stale instructions.

## Other requests inside Joe-mode

For commit-message drafting, apply the [shared commit-message policy](../setup/COMMIT-STYLE.md) directly; no separate formatter skill or Caveman chat mode needed. Drafting grants no Git mutation authority. Separately requested synthesis retains its input/altitude rules.

Select a permitted relevant skill rather than forcing every turn through delivery.
[Evolve Architecture](../evolve-architecture/SKILL.md) can propose an evidenced
improvement; Migration requires actual production migration obligations.
Synthesize requires supplied sources, output purpose, and altitude.
Agent-to-agent Handoff preserves current scope; cross-session or machine
transfer is human-directed. Do not automatically invoke human-only Automate-this,
Caveman session mode, ELI5, Retro, or Wait-what. They run only when the
human requests them. Setup's only automatic entry is the owned missing/incomplete
configuration bootstrap in step 2; it is not a general routing option.
Codebase-health findings can feed Discovery only within
the anchor; ask before expanding scope. Communication preferences grant no
additional work authority.

Read and use the route's current local skill, not a remembered or upstream workflow. Choose process guidance for the actual problem; do not force Discovery for already-ready work or call every loosely related skill. Missing skills or capabilities block their route; they do not permit invented tools or silently dropping required review.
