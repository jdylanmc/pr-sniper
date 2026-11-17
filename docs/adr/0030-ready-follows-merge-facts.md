# 0030 — Ready follows current merge facts

## Context

A signed-off PR stayed in **Ready for you** after its base moved and it began to conflict. The heartbeat already read mergeability, CI, review decision and draft state for every tracked open PR, but these are not review-revision facts: they never queue a review, and the Queue never re-checked a signed-off item against them. Drafts reviewed under Review drafts could also land in Ready through `ready_for_human`.

## Decision

- **Ready hold** (`queue::hold`): a `MachineSignedOff` item whose latest heartbeat snapshot of the same head is not mergeable shows `WaitingForAuthor` with a typed `hold`, in order: `draft`, `conflicts`, `changes_requested`, `ci_failed`, `behind_base`. An approval whose merge waits on the branch (`MergePending*`) follows the same rule; an approve or merge in flight keeps its progress.
- Facts: normalized `lifecycle`, `mergeable` (`ok`, `conflicting`, `behind`), `review_decision`, CI verdict. GitHub `behind` = `mergeStateStatus: BEHIND` outside a merge queue; Azure DevOps `mergeStatus: conflicts`, `isDraft`, a negative vote and red statuses map to the same facts.
- Projection only. No review, comment, write or notification. Merge state is not a finding. Not Needs attention, not a rest.
- The next read that clears the fact returns the item to Ready. The Ready notice stays once per Iteration, so a hold and return never re-notify; a draft that was never Ready gets its one notice when a later pass signs it off.
- Draft and requested changes are review-revision facts: leaving draft or a new review decision queues the normal fresh pass. A new head is a new Iteration.
- The automatic approve and merge gates are unchanged: merge needs `MERGEABLE`/`CLEAN`; draft and conflicts deny approve.

## Consequences

- Ready means signed off and mergeable as last read; the heartbeat cadence bounds how stale that is.
- Held items show in Running as **Waiting for author · <reason>** PR rows.
- One more GitHub heartbeat field pair (`mergeStateStatus`, `isMergeQueueEnabled`); review hashes are unchanged.
