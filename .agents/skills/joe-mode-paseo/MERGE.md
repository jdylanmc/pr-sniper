# Repository-defined orchestrator merging

Only the Joe team's explicitly requested **PR coordinator** uses this contract, within the human's repository-scoped merge grant. Human merging remains default. Create the requested role without re-questioning it. PM routes work; this separate human proxy performs final review and merge.
Session Joe-mode, delivery workers and Shepherd gain no merge or approval
authority. Experimental repositories are a use case, not inferred permission or a required classification flag. Never approve your own implementation.

Legacy non-team boards retain their recorded final-orchestrator grant until
human-approved paused transfer. Installation does not silently transfer merge authority to a new agent.

## Resolve the repository gate

Read the target repository's actual instructions and agreed merge policy.
If the gate or its authority is missing, ambiguous or weaker than the minimum
below, clarify with the human. Do not invent required jobs, assume an empty
check list passes, or impose this library's commands on another repository.
Reuse an adequate gate without repeating the interview. Recording/changing repository policy still requires applicable write approval.

Save `merge: "orchestrator"` and `mergeGate` in the existing private activation
config. Its nonempty references are `source` (the repository policy and revision),
`authority` (the human's scope/lifetime grant), `roast`, `ci`, `lint`,
`rubberDuck` and `verification`. References may point into the same policy.
The minimum is independent Roast, successful CI and linting, then rubber-duck
reasoning and final verification by the final orchestrator. The repository defines actual commands, required jobs, acceptance criteria and merge method, and may require more. Linting within CI counts when its successful execution is visible. Missing lint/CI configuration needs clarification, not a skipped check.

The helper validates reference presence, not policy truth or PR eligibility.
Initialization stays paused and never merges. Existing human-mode
boards keep their authority; do not reset or directly edit a live board to change
mode. Use [STATE's `configure-merge`](STATE.md) only in human-directed management,
after pause/stop, released/fenced lease and reconciliation of owners and pending
merge effects. It preserves workers, history and other config and does not resume
the board. A tick cannot change its policy or widen its grant.

## Finish one candidate

Query all open in-scope PRs, select those meeting the gate, then rank by impact on the agreed goal and blocked dependencies. Verify the implementation
against its issue, not just a green check list. Review outside PM's context.
No extra heartbeat: PM dispatch/results wake this role.

Use the existing delivery/Shepherd packet and `record` operations, not another
approval ledger. Coordinate with the current delivery owner and Shepherd: no concurrent source rewrite or second merger.

1. Confirm this PR is in the granted repository/backlog scope and the grant and
   repository gate still apply. Reconcile prior pending merges against live provider state before considering another attempt.
2. Receive the reviewed candidate from its owner. Inspect the independent
   Roast findings and their resolution, actual successful required CI jobs and
   lint output, and repository-specific criteria. Evidence must cover actual source head and current target; changed code/base invalidates affected proof.
   Drafts, unresolved findings/threads, required votes, missing/pending/failed
   checks, unknown mergeability or unmet repository policies block merging.
3. **Rubber duck, then verify:** the PR coordinator walks through the change,
   intended behavior, failure paths, risks and evidence against the repository
   gate. Record that reasoning and the final criterion verdicts in the packet;
   a worker's "ready" or a green icon is not this step. Unresolved semantics or
   accepted-risk decisions return to the human, not an agent vote.
4. Ask PM to record a bounded pending merge operation for this exact candidate.
   PM serializes board writes; the coordinator returns receipts, never takes PM's lease or directly writes the board. Hold that short operation's lease until outcome reconciliation, never during deep review.
   Recheck the claimed pass's mode/token, human authority, live source/target
   refs and provider eligibility immediately before the merge. Follow
   [current-base readiness](../ship/DELIVERY.md#current-base-readiness-and-real-custody).
   If refs or policy changed, return to maintenance and refresh affected Roast,
   CI/lint and final verification. Record a pending operation keyed by the PR
   and candidate before issuing the supported provider merge with its expected
   head guard and repository-approved method. Respect target/merge-queue policy;
   never use admin/bypass, cast approval votes or enable blanket auto-merge.
   If the provider cannot protect required candidate/base conditions, stop for the human; never promise atomic check-and-merge.
5. Read back provider state, actual merged commit and target before recording
   completion or advancing dependencies. A queued merge is pending, not merged.
   After uncertain responses, inspect first; never blindly repeat the merge.
   Return the verified outcome to the existing Shepherd/owner for duty settlement.

Human mode still returns ready PRs for human signoff. This exception delegates merge execution to the requested coordinator under the agreed gate, not implementation, independent review or provider-required human approvals.
