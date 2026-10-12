# Make provider actions opt-in, primary-only, fail-closed

## Context

Agents are reused across repos. Permissions must not travel with them. Approve and merge are the highest-risk writes.

## Decision

- Grants live on the repo assignment, not the Agent.
- Publish Comment: any assignment. Reply, Approve, Merge: primary only. Primary selection grants nothing.
- Approval control is a 3-way pill: off | Approve | & Merge. Merge includes approval and follows its confirmed receipt.
- Approve/merge need a distinct full final review by the primary, after clearance and fresh evidence. Findings or human judgment stop actions.
- One acting-account vote. Approve alone never merges.
- Missing, partial, or unsupported evidence blocks. Limits fail closed: 100 threads/comments, 100 check contexts.
- HAS_HOOKS, merge queue, unavailable rules, absent green CI: no direct-merge fallback. Admin bypass never used.
- Native only (ADR 0010). Legacy defaults and `approve` stay inert.

## Consequences

- No primary => no reply/approve/merge.
- GitHub lacks an expected-base merge condition. Base move caught by pre/post observation. Confirmed effects retained, not falsely undone.
- Obsolete grants are not promoted.
- A confirmed approval of the current head is `Approved`: out of the Queue, into Reviewed. Ready for you means signed off and not approved.
