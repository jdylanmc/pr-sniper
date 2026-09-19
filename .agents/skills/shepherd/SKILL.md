---
name: shepherd
description: "Human or scoped delivery-agent use. Maintain one existing PR with independent review, green checks, and rebase on target-base advancement; invoke conflict resolution internally and leave final sign-off/merge human-owned."
disable-model-invocation: false
user-invocable: true
---

# Shepherd

Own one published pull request (PR) beyond a green snapshot. Follow the common [invocation policy](../setup/INVOCATION.md) for human invocation or machine handoff. Humans may invoke `/shepherd` on conflicted PRs; the resolver stays internal. Observe, rebase whenever the target advances, and return functional work to the existing Ship, Patch, or Refactor route owner. Never merge, approve, enable auto-merge, accept product risk, or delete the delivery branch. See the human-authored [intent](intent.md).

One PR scope per ordinary invocation. Under Joe's
[TEAM](../joe-mode-paseo/TEAM.md), one shared Shepherd accepts all project PR
scopes, with separate owners, observations and due times. PM owns its heartbeat
lifecycle; the bound Shepherd executes create/delete and returns receipts.
Runtime sharing preserves explicit assignments,
sole owners and per-PR cadence. Load/execute [LIFECYCLE](../squadron/LIFECYCLE.md)
for accepted custody/recovery/retirement; idle proves no end of maintenance.
Before custody, load/execute [OBSERVATION](OBSERVATION.md): scheduler-first
adaptive cadence, durable per-PR state, fair shared wakes, owned cleanup.

Preserve the delivery's [doctrine selection](../doctrine/APPLY.md) through maintenance and repair handoffs. **Require `worktrees` before preparing PR changes** and use the [workspace procedure](../ship/WORKSPACE.md) to reuse the owned delivery workspace. Invocation/handoff grants bounded maintenance within established ownership; an explicit observation-only request does not. Load applied standards; pass metadata and pinned digests to the route owner. The monitor need not read every worker doctrine.

## Take ownership

Read repository guidance and resolve the PR, provider, delivery branch/worktree, requirements, and declared validation. Reuse the [delivery packet](../ship/DELIVERY.md#one-delivery-packet-one-owner), including owner route, return owner, source/target refs and doctrine digests. Do not refuse an inspectable PR for a missing old handoff. Recover the route from original scope and live evidence; ask Joe-mode/the human when unclear, never default to Ship. Read the actual PR target (usually main, or its explicit target); do not guess the default branch.

Ensure no other agent is implementing or maintaining this delivery branch. Wait for transfer while its route is building. If a live Shepherd owns it, confirm and return its status; do not duplicate the loop. A stale progress file does not prove live ownership; resolve uncertain worker status before competing work.

Observe actual PR immediately; acknowledge observed head/target, scope, duties,
next observation to sender. Preserve acknowledgment here before claiming custody.
Same-session takeover also records actual first observation/role acceptance.

Use the harness's session storage for a small progress file, or an approved repository-local ignored session location when unavailable. Do not commit it or silently change ignore rules. Report its absolute path. Record the PR URL, route/return owner, maintenance owner, worktree, creation time, last observation, observed base/head, validation/review coverage and invalidations, pending human signoff, handled findings, active repair, and next observation time. Add OBSERVATION's snapshot, quiet stage/streak and desired/observed scheduler state, and any [recovery episode](RECOVERY.md), to this same record. Keep credentials and private log bodies out of it.

Add LIFECYCLE's agent/parent, repository/project/worktree/workspace mapping,
accepted custody evidence, retirement/retention status. Read back updates;
persistence failure means reported error, not resumability. On cancellation/resume,
reconcile live owners/children, partial work/wakeups before monitor/repair
replacement. Record observation gap; reconstruct missing PR history with limits explicit.

## Observe the PR

Use the provider's available read operations to inspect:

- Open, merged, or closed state; draft status; current base/head; mergeability and repository branch policy.
- Required checks, including pending, failed, cancelled, and unavailable results.
- Review decisions, general comments, and inline review feedback.

For GitHub, resolve `REPO` as `owner/repo` and `PR` as its number, then use:

```sh
gh pr view "$PR" --repo "$REPO" --json url,state,isDraft,createdAt,headRefName,baseRefName,headRefOid,baseRefOid,mergeable,mergeStateStatus,reviewDecision
gh pr checks "$PR" --repo "$REPO"
gh pr view "$PR" --repo "$REPO" --comments
gh api --paginate "repos/$REPO/pulls/$PR/comments"
```

Read check details/logs for diagnosis. Nonzero `gh pr checks` status can mean failed or pending checks; inspect output before calling it a provider outage.

For Azure DevOps, use the configured integration and the [provider reference](../setup/issue-tracker-azure-devops.md). Inspect the code-project PR's `status`, `isDraft`, `mergeStatus`, reviewer votes, threads, PR statuses, and applicable blocking policy evaluations. `active` is open, `completed` is merged, and `abandoned` is closed without merge. A successful merge calculation or an empty check list is not approval. Inspect live refs where the last merge-calculation commits lag, and distinguish missing policy evidence from success. Report pre-existing auto-completion rather than silently relying on human-only merging. For another host, use equivalent configured operations; report missing capabilities instead of guessing endpoints.

Compare with the last observation. Unchanged check failures or handled comments are not new repair work. Reopen only when new evidence warrants it; a failed remedy is an explicit blocker, not an identical redispatch.

Use [the shared current-base readiness gate](../ship/DELIVERY.md#current-base-readiness-and-real-custody). Do not call unknown mergeability, pending checks, missing required evidence, or an old-base head ready. No check results is not proof of success: establish what the repository requires. Separate **ready for human signoff** from actual approval/merge eligibility; report outstanding human votes without casting them. Blocking findings/reviews or other unmet policies prevent readiness. Immediately before promotion/announcement, reread actual remote source and target refs plus provider state; if either changed, invalidate the claim and reconcile again. Report the observed head/base/time, not a guarantee against the next base race, and continue watching after green.

## Act on meaningful changes

| Observation | Action |
| --- | --- |
| PR merged or closed | Record and finish this PR's duties; retire the owned terminal agent under LIFECYCLE only after all its scopes/duties end and evidence is preserved. |
| Open with no meaningful changes, or only pending checks | Record the observation and wait for the next interval. |
| Target advanced, even while PR remains mergeable/policy-compliant | Rebase the owned branch onto the latest fetched target; invalidate stale proof and refresh it below. |
| Conflicted/unmergeable, or policy needs maintenance | Perform bounded branch maintenance; invoke the internal resolver for actual conflicts. |
| Unexpected source-head movement or target retarget/rewrite | Reconcile actual ownership and intent before mutation; do not overwrite concurrent work or silently replay onto a different target. |
| Scoped formatting/linter failure with an unambiguous mechanical fix | Apply only that fix under the branch-maintenance gate; validate and review the current candidate. |
| In-scope review feedback or check failure requiring functional code/test changes | Return through PM to the existing route's feedback continuation on this same PR. |
| Concrete incompatibility or failed acceptance requires Joe re-routing/reimplementation/refactoring | Execute [issue-backed recovery](RECOVERY.md) within recorded authority: one episode, existing controller, same PR. Target diff size alone cannot justify this route. |
| Cancelled check, missing runner/tool, or service outage | Distinguish infrastructure from code failure. Report the blocker; use only authorized provider recovery actions. |
| Changed requirements, architecture, scope, accepted risk, or a semantic conflict | Present the decision to the human and stop. |
| Provider access, branch ownership, or required evidence becomes unavailable | Record what is known, report the blocker, and stop rather than claim readiness. |

### Branch maintenance

1. Inspect Git status, worktrees, any in-progress operation, branch ownership, and provider source/target repositories and refs (including forks). Resolve the actual push remote; do not assume `origin` is the PR source. Record the live remote source head as the **expected head** before rewriting and fetch source/target explicitly. Reconcile local commits against that head and the packet. Unknown divergence, competing writers, changed ownership, or a moved source during maintenance stops automatic mutation; do not reset, stash, discard, or overwrite someone else's work.

2. In the owned isolated workspace, rebase onto the newest fetched PR target whenever it advances, even if the provider says mergeable and no policy demands it. An already-contained target is a verified no-op, not a reason for an empty rewrite. Reconcile an unexpected retarget or rewritten target with the owner/human before replaying onto a different intent. Preserve pre-rebase head/ref evidence and unrelated work; dirty or uncertain state must be resolved safely before starting.

3. For actual conflicts, **invoke [conflicts](../conflicts/SKILL.md) internally** with the PR, operation, scoped paths, both sides, maintenance authority, and return owner. It may make only mechanical, unambiguous resolutions. Regenerate derived output from its source; for independent validation registrations, preserve both additions and every trusted-base check, then run complete repository validation. Semantic conflicts return both sides to the human without staging guesses. Owner-directed abort is allowed only when it preserves work. No silent ours/theirs choice, new product fix, or blanket staging.

4. Invalidate pre-rebase head/base check results and independent review coverage; old green and approvals are not proof of the rebased candidate. Every modifying agent/helper uses [changelog](../changelog/SKILL.md). The maintenance/integration owner alone consolidates proposals before fresh review; reuse/deduplicate existing meaningful entries. Record notable maintenance consequences, not one entry per rebase or regeneration. No recursive changelog-only entry or automatic release/version bump.

5. Run affected and repository-required validation and [Verify](../verify/SKILL.md), including complete validation for combined registrations. Obtain fresh independent [Roast](../roast/SKILL.md) coverage (`solid` for code) for the rebased candidate including the consolidated changelog; use scoped fix-review when justified, recording how whole-deliverable coverage is preserved. Mechanical maintenance stays here, not a new route invocation. Functional failures return to the existing route owner.

6. Recheck remote source against the recorded expected head before publishing. For rewritten history on an explicitly owned branch, use an explicit expected-head lease, for example `git push --force-with-lease="<source-ref>:<expected-head>" <source-remote> HEAD:<source-ref>` with resolved full ref and literal recorded commit ID. Never blind `--force`, an unspecified tracking-ref lease, or a newly refreshed lease used to overwrite concurrent commits. If the source changed or the lease fails, preserve local work, report the race, and stop for ownership reconciliation rather than retrying forcefully. Use normal fast-forward push for non-rewritten updates.

7. Re-observe provider checks/policies for the published head; rerun required provider checks when needed and supported, never reuse old-head successes. Reread live source/target before readiness. If target moved again, withdraw readiness and repeat maintenance/proof, not a stale green handoff. If provider access or safe update capability is unavailable, report the blocker and stop.

Use the [shared commit-message policy](../setup/COMMIT-STYLE.md) for newly authored maintenance messages. Preserve existing messages during replay/rebase; formatting does not broaden maintenance authority.

### Feedback repair

Call the existing owner route—[Ship](../ship/SKILL.md), [Patch](../patch/SKILL.md), or [Refactor](../refactor/SKILL.md)—with the same PR, original requirements/kind, new evidence, current source/target, workspace, validation, doctrine packet, and this Shepherd as return owner. Record the repair worker before waiting; neither modify the branch concurrently nor duplicate repair of the same findings.

The route classifies evidence, performs bounded implementation and independent review, validates, and updates the same PR. It never invokes Ship as a generic finish or starts a nested Shepherd. On return, reconcile actual head, review coverage, check state, and addressed findings before resuming observation. A missing result, failed repair, or human-owned decision is reported explicitly and ends safe automatic remediation. If concrete evidence requires Joe re-routing/reimplementation/refactoring, load and execute [RECOVERY](RECOVERY.md): one linked issue/episode, actual existing-controller intake acknowledgment and serialized repair/return, not an automatic route switch. Missing authority or a human product decision remains a blocker.

If a draft's outstanding delivery work needs completing, return it to its existing route under the same ownership rule. Do not promote while acceptance, independent review, current-base proof, or required checks remain incomplete. A blocked draft is not the final handoff.

With DELIVERY's full gate/accepted custody, actually promote; verify provider
non-draft readback/current candidate before announcing readiness. Send/request
success is insufficient. After accepted terminal repair return, arrange LIFECYCLE
retirement, preserving monitoring scope/worktree.

## Observation rhythm

In the shared Joe team, PM's explicit role cadence (five minutes by default)
overrides the standalone adaptive schedule below. Keep per-PR due/coverage
records and notify PM of urgency or gaps. Do not create a timer per PR or
change the PM timer. Retire the shared agent only when all scopes end and PM
has recorded successful deletion of its exact role heartbeat.

Execute [OBSERVATION](OBSERVATION.md), never an age table: observe immediately;
default 1 minute, then 5 and 15 only after each stage's 30 consecutive successful,
complete, unchanged observations. Meaningful change resets streak/fast cadence;
preserve human overrides. Use authorized supported scheduler, preferably same-agent
heartbeat, never shell wait loops. Persist/verify cadence changes and binding;
report unavailable monitoring.

On meaningful changes, report the PR, checks/readiness, action, and next observation. On merge, closure, human decision, operator stop, or runtime loss, leave the latest truthful record. A crash ends observation; last green does not prove continued monitoring. Stop only run-owned waits/workers when safe; preserve unfinished work and report uncertain worker termination.

Finish OBSERVATION's owned wakeup cleanup and LIFECYCLE scope reconciliation.
Retain or arrange accepted transfer for active idle waiters, pending fixes/permissions, other
owned PRs; never archive them. All duties terminal: preserve results, actually
retire owned agent, arrange parent-performed self-retirement when needed.
Record unavailable archival; never substitute workspace archival or
branch/worktree/evidence deletion.
