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

## Proposed change ([ADR 0021](0021-review-packet.md))

- With several assignments the primary runs last, after every teammate (non-primary) has submitted a report. The distinct primary final review run is replaced by the primary's turn.
- Grants apply to the primary's exits: Publish Comment and Reply Comment gate `leave_comments`, Approve gates `approve_pull_request`, Approve & Merge gates `approve_and_merge`. A non-primary assignment's grants are ignored and the settings do not offer them. A teammate ends with `submit_report`, which is not a provider write and needs no grant, so it only produces a report.
- Approve & Merge gets an **Include self** option, default off (ad hoc decision, owner review requested). It lifts the "not the author" gate. Whether GitHub accepts an author approving their own PR is unverified; if it does not, the option has no effect and that is reported.
- Approve gate relaxations: others' unresolved threads and others' changes requested no longer block approve; merge gates are unchanged. Not offered while a PR is a draft.
- Evidence limits, HAS_HOOKS, merge queue and no-bypass rules stay.
- Approve and merge implemented: effective permission = grant AND the exit chosen (`approve_pull_request` never merges). Own PR: approve denied, merge only with Include self. Our approval already on the head: success, no write, state consumed. No `PrimaryFinal` work is created for packet-mode assignments; legacy records still load. Reply and resolve execution: M5c.
