# One team, one repository

Shared Joe team contract, not another entrypoint or controller. The human
starts Joe-mode; PM routes under that grant. This contract preserves PR
Sniper's existing team policy while keeping runtime mechanics in the
[CMUX adapter](../joe-mode-cmux/RUNTIME.md). Session Joe and CMUX have no
recurring heartbeat.

## Roles and developer slots

| Role | Job | Lifetime |
| --- | --- | --- |
| Project manager | Route ready work, accept results, surface decisions and settle owners | Original human chat; one PM |
| Shepherd | Observe all accepted PRs fairly, maintain current bases and route semantic repairs | One shared role while actual PR duties exist |
| Discovery / backlog manager | Research, aligned Discovery, planning and approved domain-document work | One human-facing inquiry lane on `discovery/<feat>` |
| Ship / Patch / Refactor | Implement the assigned issue or agreed graph | One bounded delivery lane |
| Roast | Independent whole-deliverable review with scoped follow-up | Retain through corrections; retire after accepted return |
| Blocker investigator | Challenge a concrete developer blocker with evidence | One bounded investigation |
| PR coordinator | Verify and merge only under [MERGE](MERGE.md) | Independently authorized role on `pr-sniper` |

Default pool: **six developers**, not six total agents or six delivery owners.
Features reserve **two** slots; bugs, hardening and refactors reserve **one**.
Every writing descendant counts, including a writing route owner. Do not
double-count an already reserved red/green pair or add hidden implementers.
Uncertain launch/termination still consumes capacity. Support roles are outside
the developer pool but inside the active runtime's stricter session/resource
limit. Reserve useful review and human-facing capacity before filling lanes.

Joe prefers paired TDD for features, especially greenfield: behavioral RED
before GREEN, independent evidence and serialized integration. With an agreed
legacy/non-TDD exception, the second developer provides useful acceptance
coverage; feature staffing still costs two. Standalone Ship is TDD opt-in.
Patch and Refactor use one developer without forced red/green ceremony.
Refactor preserves behavior and applies `laziness`, `solid` and PR `worktrees`.
No invented RED evidence or framework retrofit just to satisfy a ritual.

## Reach useful work

Human activation/resume includes a first bounded work pass, not merely a
layout or board. Reconcile owners, reserve actual eligible work, dispatch
within permissions, and verify returned identity and accepted assignment.
A reservation or launch acknowledgment is not a running visible worker.
Uncertain creation stays pending; reconcile before retrying.

Advance actionable candidates before opening new lanes. Use configured
readiness mappings, full requirements, dependencies and existing owners.
A green but unmerged prerequisite is not available on a consumer's main base.
Do not silently adopt stacked PRs, change grouping or waive acceptance.
Use Chart-a-course when the goal/dependency picture changes, not on every turn.
PM accepts compact evidence; deep investigation and review go to bounded roles.

No eligible work: identify the exact capability, ownership, scope or human
decision blocker and its owner. Do not create idle roles or busywork.
In-flight work retains its owner, expected evidence and next useful observation.
Generic "still running" does not renew an indefinite wait. Respect the active
runtime's supported event/wait contract; promise no between-turn execution.

## One inquiry lane and every human wait

Reuse the same Discovery conversation and dedicated worktree across passes.
Preserve complete findings, actual human answers and recording/publication
gates. Read-only research helpers are not additional Discovery owners.
Never silently relocate an existing interviewer or archive the primary chat.
Reconcile artifacts and questions before an acknowledged transfer.

Present every new or materially changed human wait in the primary PM
conversation: owner, exact question/input revision, affected work and verified
tab/surface/identity or supported link. An internal artifact or send receipt is
not delivery to the human. Only an actual answer, withdrawal or accepted
reassignment clears it. Deduplicate unchanged waits; remind only when requested.
Do not repeat research or open another interview on unchanged inputs.
Discovery blocks dependent work only; independent authorized work may continue.

Pass actual approval with its scope. Do not re-ask settled choices at routine
handoffs; preserve genuinely missing authority, scope and publication gates.
Unknown ownership remains a global dispatch blocker. Silence grants nothing.

## Review, custody and knowledge

Use one shared Shepherd for all accepted PRs, with per-PR owners, observations,
due times and recovery records. PM consumes compact evidence instead of
duplicating routine queries; refresh stale/action-critical facts as needed.
The active adapter's session-bound observation contract overrides scheduler
examples in generic skills. No timer may be created from this package.

Shepherd performs only permitted mechanical rebase/regeneration/lint work.
Semantic repairs return to the same delivery owner and PR after observed
write-custody release/acceptance. Other PRs continue. Preserve independent
whole-candidate review and scoped fix review; no self-approval.

Approved Discovery knowledge may produce scoped documentation PRs under its
recorded publication grant and exact-file gates. Use canonical context/ADR
locations, an owned worktree, meaningful aligned deltas and serialized writes.
Follow DELIVERY's review/check/current-base/custody gates. Human-owned intent
and doctrine need separate edit authorization. Private transcripts and runtime
receipts stay private. Never manufacture documentation on a timer quota.

## Challenge blockers, then move on

1. Developer rubber-ducks reasonable alternatives and returns attempted work,
   evidence and the exact missing answer.
2. PM assigns one bounded independent investigator; PM does not repeat the
   same deep trace. Send supported guidance to the current owner.
3. First confirmed work blocker: preserve partial work, settle custody, retire
   the old lane, then try one fresh agent/lane on a fresh isolated worktree.
   Every previously staffed participant is excluded from that fresh attempt.
   Transfer facts and requirements, not the predecessor's conclusion.
4. Second independently confirmed failure: apply the configured blocked role
   with an approved explanation, return the issue to the backlog, retire its
   settled workers, and route the unresolved question to Discovery/the human.
   Preserve attempt history across restarts; unchanged events never reset it.

Permission denial, credentials and human decisions are not fresh-context
experiments. Do not evade them using another agent/provider. Runtime
cancellation first requires reconciliation of descendants and partial writes.
Report unavailable independent investigation rather than inventing a worker.

## Permissions follow the human

Use the active adapter's actual permission contract for every descendant,
replacement and support role. Preserve authorized allow/deny rules and
revocations; a prompt saying "inherit" is not runtime evidence.
Never enable broader permissions to make progress, silently downgrade a
human-granted mode or infer grants from saved defaults.

For CMUX, [RUNTIME](../joe-mode-cmux/RUNTIME.md) owns current-account checks,
native launch and explicit coordinator-only YOLO. Copy no credentials.
If a provider/runtime mapping cannot preserve the actual grant, block that
launch and ask only for the missing choice. Record the exact verified child
identity, worktree and selected model; respect configured preferences and
runtime defaults. Do not hardcode model IDs from imported packages.

## Pause, stop and preservation

Stop new dispatch first on human pause/stop. Reconcile actual children, PR
custody, pending effects, resources and any surviving legacy jobs. Removing
adapter files does not cancel jobs or transfer live ownership.
Pause is not evidence that external processes stopped. Report observation
gaps and precise recovery actions. Human answers, merges and callbacks do not
automatically resume a paused/stopped controller.

Retire accepted terminal owned agents only when duties end or transfer under
LIFECYCLE. Keep concrete waiters, not hypothetical future helpers. Close
interactive sessions normally before supported archive; no process killing
or terminal deletion to clean the cockpit.

Worktree removal is separately authorized: stop writers, inspect tracked,
untracked and ignored work, preserve safe evidence, push a recoverable branch
and verify exact remote/local commit IDs. Never push secrets or private runtime
records. Remove only the exact owned worktree after preservation readback.
Uncertain ownership, failed push or unpreserved work means **keep the local
copy** and report the gap. Archive is not permission to delete a branch,
project, workspace or the sole evidence copy. Record deliberate retention.
