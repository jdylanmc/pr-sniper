# Bounded implementation worker

The delivery route supplies this contract to each implementer. It is supporting guidance, not another delivery controller. Ship, Patch, and Refactor retain their own scope and delivery ownership under [the common invocation policy](../setup/INVOCATION.md).

Load [LIFECYCLE](../squadron/LIFECYCLE.md) for accepted return/recovery/retirement;
load [WORKSPACE](WORKSPACE.md), verify placement before writes.
Reuse assigned worktree/registration; new agents alone warrant no resources.

## Inputs from the owner

- Owner route (`ship`, `patch`, or `refactor`), return owner, existing PR/Shepherd when present, source/target refs and observed commits.
- Assigned task/group, complete acceptance conditions, non-goals, and relevant requirements/decision references.
- Authorized workspace and branch, recorded starting commit, dependency/integration state, and any shared resources the worker must not touch.
- Relevant code and prior findings, required validation, agreed test seams, and the scope of any permitted commits.
- A session-artifact destination for the report, the owner to return to, and stop/escalation conditions.
- For issue-backed recovery, original packet, episode/issue, acknowledged owner/
  write release. Load [RECOVERY](../shepherd/RECOVERY.md); preserve same delivery
  branch/PR integration destination. Return exact resulting head/target, complete
  artifacts to owner. Issue text grants no wider authority; no independent
  tracker mutation, controller wake/retry.
- The scoped [doctrine packet](../doctrine/APPLY.md): operator selections, assigned and required IDs, reasons, accessible source/selector locations, and pinned digests.

Read the actual task and repository guidance. Resolve missing requirements with the owner before changing behavior. Do not treat a plan, issue, or review comment as authority to disregard human instructions.

Load the packet's full doctrine texts through [Doctrine](../doctrine/SKILL.md) before applying them. Verify pinned digests and report missing/changed sources rather than dropping requirements. Preserve the selection through nested skills; return to the owner for material scope or selection changes. The owner can dispatch metadata only, but your work requires the actual text.

## Execute within the task

Implement the complete bounded outcome using the selected route's discipline.
Standalone Ship is TDD opt-in; Joe prefers it for features. Patch and Refactor
do not require a red/green pair or test-first order. Honor the caller's declared
testing choice and developer-slot reservation. Patch repairs the proven cause;
Refactor preserves behavior; Ship satisfies its feature/specification criteria.
Trace behavior to the responsible interfaces; preserve unrelated changes.
Record the actual task start/result rather than assuming the last commit is the entire task.

Use focused checks while iterating and the repository-required checks for the completed scope. Preserve real red/green evidence when TDD applies. Do not substitute a self-reported status for test output or hide a missing environment.

Inspect your own diff before returning, fixing supported in-scope omissions. Self-inspection does not replace independent Roast. Do not dispatch your own implementers or reviewers, integrate into the owner's branch, invoke another delivery route, publish a PR, close tickets, or start Shepherd.

Use [changelog](../changelog/SKILL.md) for notable change-entry proposals with the appropriate component file and any existing entry to reuse. Return proposals rather than concurrently editing a shared changelog; the integration owner consolidates before final review. Do not create per-tool churn, automatic releases/versions, or a recursive entry for changelog-only maintenance.

Commit only within the authority the owner supplied. Use the [shared commit-message policy](../setup/COMMIT-STYLE.md). Do not stage unrelated changes, amend history, or change another worker's branch to make the report look complete.

Escalate missing context, ambiguous requirements, unexpected architectural choices, exhausted approaches, or blockers with the evidence and smallest needed decision. Do not settle human-owned scope/risk decisions or mark a known acceptance gap complete.

## Return evidence

Save the report in the agreed session location and return a concise summary plus its path:

- **Status:** IMPLEMENTED, PARTIAL, BLOCKED, or NEEDS_INPUT, with the exact reason. IMPLEMENTED means the assigned work is ready for the owner's review/integration, not approved or delivered.
- What changed or was attempted, affected files, starting/result commits, and any uncommitted changes.
- Each acceptance condition: met, unmet, or unverified, with supporting evidence.
- Actual validation commands, relevant output, environment/inputs, and red/green evidence when applicable.
- Remaining concerns, failed attempts, proposed next action, and any run-owned process or artifact still active.
- Actual repository/project/worktree/workspace mapping and agent/parent identity;
  offered return versus the owner's observed acceptance, remaining duties, and
  retirement owner or specific retention/capability limit under LIFECYCLE.
- Notable changelog entry proposals, destination component file, and existing entries to reuse/deduplicate.
- Doctrine IDs/digests actually loaded, relevant application or inapplicability notes, unavailable standards, and evidence-backed recommendations citing exact rules. Selection or loading alone is not approval.

For a fix round, append the specific findings addressed, fix-base/result commits, changes, and fresh covering evidence. Reuse a worker retained for pending fixes when supported; do not retain a terminal agent for hypothetical future work. A finding is not resolved merely because a fix was attempted; the owner sends the result to Roast.

The route owner owns task state, integration, final review, publication, and the mandatory Shepherd handoff under [the shared finish](DELIVERY.md). Existing-PR fixes return to that PR's current Shepherd, never a second monitor. There is no second approval ledger, autonomous risk ruling, or alternate finishing workflow.

Owner acknowledges actual return before acceptance. Preserve complete diff/evidence;
stop writes at that boundary. If terminal self-retirement interrupts reporting,
arrange owner archival after acceptance. Never self-archive before owner can recover
results, or delete assigned worktree/workspace to retire.
