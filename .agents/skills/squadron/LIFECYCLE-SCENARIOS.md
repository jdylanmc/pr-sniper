# Lifecycle acceptance scenarios

Review changes to [LIFECYCLE](LIFECYCLE.md), its callers,
[placement](../ship/WORKSPACE.md), or [readiness](../ship/DELIVERY.md) with these
scenarios; add [Shepherd scenarios](../shepherd/SCENARIOS.md) for adaptive
scheduling/issue-backed continuation.

Supply observations to the applicable route; request next action, delivery/
custody/runtime state, and decisive pre-claim evidence. Compare expected outcomes;
record actual response, pass/gap, source revision in the existing review/task record.
Simulate provider/harness observations: no real PR mutation, agent archival, or
resource creation. These are neither live-operation instructions, enforcement,
nor executed-transition proof. Label tabletop walkthroughs as weaker than
authorized live evidence.

| Case and supplied observations | Expected action and observable postcondition | Reject |
| --- | --- | --- |
| **Ready draft:** acceptance met, current whole-candidate independent review, required checks green at H1 containing latest observed T1, no blockers; Shepherd observes H1/T1, acknowledges custody | Accepted Shepherd promotes through supported operation. Readback: non-draft, unchanged H1/T1, covering checks. Only then report ready for human signoff/next observation. | Readiness from draft URL, promotion request without readback, sender-authored custody; agent approval/merge. |
| **Blocked draft:** required check failed/missing, acceptance gap, unresolved human decision despite mergeability | Record actual draft/blocker; retain/transfer explicit duties; authorized repair returns to existing route. No readiness. | Promotion for report consistency; mergeability/empty checks counted as success. |
| **Mixed batch:** PR A verified ready, PR B draft building, PR C blocked draft | Report separate states/owners; B/C unfinished, with next actions. Reconciled cycles may include honest blockers, not “all delivered.” | Abandoned drafts in delivered batch; cycle completion equated with every PR ready. |
| **Send only:** transfer succeeds, recipient idle, no observation/response | Pending transfer; sender retains responsibility without competing writes. Obtain receiver observation/acknowledgment of actual scope/candidate/duties. | Custody from send success, idle status, local owner field. |
| **Accepted transfer:** receiver observes H2/T2, accepts named PR/maintenance duties with next observation; outgoing writer stopped | Preserve acknowledgment; one accepted scope owner, no competing monitor. Retire terminal sender only after remaining duties/evidence checks. | Discarded evidence; receiver writes before outgoing writer stops. |
| **Base moves:** H1/T1 reviewed/green; fresh target T2 before promotion/announcement | Invalidate readiness; rebase on T2; refresh affected proof/review/provider checks; re-observe refs. If promoted, report actual draft flag, never stale readiness. | T1 proof reused for green mergeability; promotion success equated with readiness. |
| **Cancelled owner/child:** parent cancelled after partial diff; child may write, another monitor may survive | Disclose gap; inspect owners/children, Git/provider state, partial work/wakeups. Preserve work; resume/replace only confirmed missing ownership through explicit transfer. | Cancellation assumed to stop descendants; replacement writer/monitor with uncertain ownership. |
| **Placement:** repository R/project P/worktree W1/workspace S1; independent writer needs W2, two read-only agents inspect W1 | Reuse P/S1 for W1 readers; establish writer W2/S2 under P. No project per worktree or workspace spanning worktrees. Verify returned mapping and each agent's actual cwd/Git state. | Per-agent UI resources; independent writers sharing mutable checkout; title/branch/guessed cwd trusted. |
| **Mapping mismatch:** create_workspace returns wrong project/path or ownership is ambiguous | Block affected writes; reconcile mapping with owner; preserve resources/unrelated work. | Main fallback, blind project creation, deleted registrations, widened permissions. |
| **Terminal worker vs idle monitor:** parent accepts/preserves analysis/implementation result, no duties remain; separate Shepherd awaits next heartbeat idle | Archive terminal owned worker through supported operation; verify archived state/active-view removal. Retain live Shepherd with next-observation evidence. Parent archives if self-reporting would be interrupted. | Cleanup candidates only; all idle agents archived; indefinite terminal retention for hypothetical fixes. |
| **Shared monitor:** one runtime owns PR A/B; A merges, B open | End A's scope, preserve evidence; retain B's ownership/cadence and agent until all scopes/duties end. | Runtime archived on A merge; silently slower B observation. |
| **No archive capability:** terminal accepted owned worker; archival unsupported/denied | Record retained ID, exact limit, responsible owner/next action; no retirement success claim. | Guessed API, bypass, silent retention, workspace-archive substitution. |
| **Preserve resources:** terminal owned agent eligible; branch/worktree/evidence still needed; archive_workspace may delete worktree | Supported archival of exact agent only; verify result; preserve branch/worktree/workspace/project and accessible evidence. Git cleanup requires separate authority. | Workspace/project archival/deletion or branch deletion to declutter agents. |

Pack tests prove released-CLI discovery, copy install/reinstall, support reachability,
not decisions/enforcement. Independent review records scenarios; delivery owners
provide live readiness/custody/retirement evidence.
