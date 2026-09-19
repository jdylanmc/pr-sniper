---
name: refactor
description: "Internal behavior-preserving delivery route selected by Joe-mode, or scoped structural work under an existing delivery owner. Review and shepherd a green current PR without creating competing ownership."
disable-model-invocation: false
user-invocable: false
---

# Refactor

Follow the common [invocation policy](../setup/INVOCATION.md). This route is internal-only: human-started Joe-mode selects it for behavior-preserving structural work. Scoped structural work may also be delegated within an existing delivery; then return to that owner without starting a second PR or custody loop. A Shepherd continuation of an existing Refactor delivery is not a new root selection.

Joe-mode's selected delivery grants in-scope worktree, changes, commits, push/PR, review/fixes, and Shepherd authority. Do not repeatedly ask whether to implement or publish; do ask for missing material requirements, scope changes, semantic conflicts, destructive probes, or production-data access. Explicit narrower assignments stay narrower. Refactor is a peer route, not a wrapper around Ship.

Use [doctrine selection and application](../doctrine/APPLY.md), preserving the
task's selections. Require `laziness` and `solid` for restructuring, and
`worktrees` for PR changes. Laziness supplies KISS and YAGNI: keep it simple,
do not build what is not needed. These are not separate catalog IDs.
Code Roast also requires `solid`. Apply the loaded rules without weakening
behavior preservation.

## Establish the boundary and baseline

Read repository guidance, requirements, actual callers/interfaces, and existing branch/PR state. Define the structural outcome, non-goals, and observable behavior to preserve, including failures, side effects, ordering, compatibility, and relevant performance constraints. Missing behavior evidence is uncertainty to resolve, not permission to change it.

Use [workspace isolation](../ship/WORKSPACE.md), reusing only compatible ownership. Record `refactor` as route owner in [the shared delivery packet](../ship/DELIVERY.md#one-delivery-packet-one-owner), with return owner, source/target refs, requirements, workspace, validation, and doctrine IDs/digests. For a scoped worker, retain the outer route owner instead.

Load and execute [LIFECYCLE](../squadron/LIFECYCLE.md) for dispatch, accepted
return/custody, cancellation recovery, and terminal owned-agent retirement.
Keep its evidence in the packet. Root delivery must execute DELIVERY's provider
promotion/readback gate; scoped workers return, never claim delivery.

Use one developer slot under Joe. Refactoring evolves architecture; it does not
require TDD, a red/green pair or forced test-first order. Discover and run relevant
baseline checks. Add/update useful characterization or regression tests where
the change benefits from them, before risky edits when that protects behavior.
Report baseline failures and unavailable proof.

## Restructure in bounded steps

- Keep feature changes outside refactor.
- Move one ownership boundary at a time.
- Preserve public interfaces, failure behavior, ordering, and compatibility; planned observable changes require routing back to Joe-mode/the human, not a cleanup exception.
- Keep intermediate states buildable and testable.
- Avoid dependency or configuration growth without correctness need.
- Remove superseded paths once preservation is proven; do not leave temporary bridges as an unreported partial result.

Run the same behavior proof after each meaningful step and at the end. Inspect the complete diff for accidental behavior changes. Every modifying agent uses [changelog](../changelog/SKILL.md); isolated workers return notable-entry proposals for the integration owner to consolidate before review. No per-tool churn.

## Deliver, or return to the existing owner

For a Joe-mode-selected Refactor delivery, execute [the shared finish](../ship/DELIVERY.md): independent whole-deliverable Roast, required validation, criterion verdicts, the same PR, latest-target synchronization, and real Shepherd custody. Local preservation proof and an internal draft are not the final handoff. Do not invoke Ship, approve your own work, merge, or enable automatic merge.

Roast from distinct preservation, ownership/SOLID and simplicity/KISS/YAGNI
angles. Use separate reviewers when their expertise needs separate context.
Reconcile findings against evidence before proceeding; do not count votes or
hide disagreement behind "consensus." Review fixes and affected behavior again.

For an existing Refactor PR, classify feedback against the original preservation boundary, apply in-scope structural fixes, and repeat proof/review on that branch/PR. Return to its existing Shepherd, not a nested monitor. A new bug/regression or requested behavior change that requires a different route returns to Joe-mode/the human. Pure rebase/regeneration remains Shepherd work.

A scoped structural worker follows [the bounded worker contract](../ship/WORKER.md) and returns commits/diff, before/after evidence, uncertainty, and changelog proposals to its integration owner. It never publishes or claims the outer delivery complete. Missing independent review, required proof, ownership, or monitoring runtime is an explicit blocker, not success.
