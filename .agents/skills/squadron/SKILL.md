---
name: squadron
description: "Human or scoped agent use. Dispatch independent bounded agents with isolated writes and explicit owners; Joe-mode uses it aggressively for distinct Ship, Patch, Refactor, and Shepherd assignments."
disable-model-invocation: false
user-invocable: true
---

# Squadron

**Entry:** human request or agent selection within authorized work, under the
[invocation contract](../setup/INVOCATION.md). Dispatch bounded workers, not a
replacement Joe-mode or delivery controller.

Use [Doctrine](../doctrine/SKILL.md) and its [packet contract](../doctrine/APPLY.md).
Preserve scoped operator choices and required IDs. Without a preselection,
choose relevant standards from catalog metadata. Send IDs, reasons, required
flags, source paths, and digests; each applying worker loads the actual texts.
Every PR-producing worker requires `worktrees`; code reviewers require `solid`.

Load and execute [LIFECYCLE](LIFECYCLE.md) for dispatch, accepted returns,
recovery, and retirement. Keep evidence in the existing work packet;
no new controller or ledger.

## 1. Find truly independent work

Separate work by outcome and ownership, not file count. Useful assignments
include independent investigations, a ready feature shipped by one worker, a
bug repaired by another, and maintenance of a different existing PR.

Joe-mode should use this aggressively where capacity and dependencies permit:
keep several distinct deliveries or Shepherds moving rather than personally
performing every job in sequence. A small lookup does not need an agent.
Related failures or one continuous causal trace usually belong together.

Before dispatch, reconcile existing issue/PR owners and dependency state.
A prerequisite in another PR must be on the consumer's agreed base; green but
unmerged is not enough. One specification graph with one PR has one delivery
owner, which schedules its own workers. Do not also dispatch its child tickets
as competing root deliveries.

Do not turn an ordinary Squadron invocation into authority to start Ship,
Patch, or Joe-mode. Each selected skill must be eligible under the actual
human/parent request. Human-started Joe-mode may select Refactor as an internal
delivery route; scoped restructuring within another delivery returns to it.

## 2. Give each worker a complete bounded packet

Supply the actual source evidence, not an assumption that the worker inherited
the conversation. Include:

- Outcome, scope, exclusions, requirements, dependencies, and acceptance proof.
- Selected skill and actual human/parent authority; decisions still pending.
- Owner/controller and objective identity, original timing evidence when known,
  issue numbers **and titles**, PR coverage, and parent/return relationships.
- Verified repository/worktree and harness project/workspace mapping, branch/base,
  owned files/resources, and integration owner.
- Doctrine packet, [commit style](../setup/COMMIT-STYLE.md), stop conditions,
  expected return, and follow-up/monitor custody.

Independent writers need distinct isolated Git worktrees, not disjoint files
sharing one checkout and Git index. Use the existing delivery's
[workspace procedure](../ship/WORKSPACE.md), including Paseo's one repository
project and one workspace per worktree. Read-only workers share compatible
sources/registration; dispatch alone needs no new isolation.
Coordinate other shared resources too: ports, databases, fixtures, and services.

Every modifying worker uses [Changelog](../changelog/SKILL.md). Assign one owner
for the integrated changelog; other workers return categorized entry proposals
instead of racing to edit it. Prefer terse exact messages without changing
the human's Caveman mode or dropping required packet fields.

## 3. Dispatch concurrently within real capacity

Use the harness's actual concurrent/background dispatch tools and current
schemas. Issue independent launches together where supported. Multiple calls
are not proof of parallel execution; confirm returned IDs and runtime states.
Use configured model preferences and defaults, never invented model names.

Keep bounded capacity for integration, independent review, human questions,
and existing monitors. Do not launch a second monitor for an already-owned PR
or a nested Joe-mode. Missing concurrency is an explicit capability limit;
do not pretend serial work was parallel or install a runtime without permission.

Continue independent work while agents run. Consume completion notifications
or the supported wait mechanism; do not repeatedly poll known workers.
Reuse agents retained for concrete pending follow-up where supported; retire
accepted terminal workers under LIFECYCLE. Queue human questions
through the parent instead of letting workers manufacture answers.

## 4. Reconcile and return ownership

A worker returns its concrete result, changed artifacts/commits/PR IDs,
acceptance evidence tied to state, actual doctrine coverage, unresolved
decisions, changelog proposal/status, and any running descendants/monitors.
For failures, preserve the partial work and exact blocker; do not hide errors
behind an empty success-shaped report.

Verify the decisive result and integrate through the one designated owner.
Inspect overlap and run the smallest combined checks covering interactions,
escalating only when impact or repository guidance requires it. A worker's
passing local tests do not prove the integrated result.

Do not cherry-pick into another delivery's live branch, take over its review,
or declare its PR ready from a dispatch summary. Ship/Patch/Refactor own
reviewed delivery, Shepherd owns ongoing PR maintenance, and Joe owns routing.
Keep custody until the receiver observes the actual state and acknowledges
acceptance under LIFECYCLE, not just message delivery. Never approve or merge
on the human's behalf.

Retire terminal owned agents after accepted/preserved results and completed/
transferred duties, not as workspace/worktree cleanup. Return messages/idle status
prove neither. Cancellation/runtime stop: reconcile live owners/children/partial
work under LIFECYCLE before replacement. Report retained duties/capability limits;
never abandon drafts or call mixed unfinished batches delivered.
