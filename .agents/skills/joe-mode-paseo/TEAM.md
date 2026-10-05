# One team, one repository

Shared Joe team contract, not another entrypoint or controller. The human starts Joe-mode; PM routes under that grant. Paseo adds durable role heartbeats.

## Runtime-specific mechanics

The roles, capacity, discovery, review, human-wait, recovery and preservation
rules here apply to every Joe adapter. References below to Paseo's STATE
operations, agent-bound heartbeats, permission APIs and workspace registration
describe **Paseo mechanics**, not prerequisites for other runtimes.
The [Orca adapter](../joe-mode-orca/SKILL.md) uses its
[RUNTIME](../joe-mode-orca/RUNTIME.md) and [RUN](../joe-mode-orca/RUN.md) for
native bindings, durable owner records, messaging, permission verification,
recurrence and release. Those contracts preserve the same policy without
invoking Paseo's helpers or inventing equivalent APIs. Orca uses configured
model defaults unless the human selected an override; discovery is evidence,
not permission to replace that choice. Session Joe and CMUX acquire no timers.

## Roles and developer slots

| Role | Job | Lifetime |
| --- | --- | --- |
| Project manager | Chart the goal, route ready tickets, accept results, watch agents and clean up | Original human chat by default; one PM |
| Shepherding | Watch all project PRs fairly, rebase and apply small mechanical fixes; requeue real development through PM | One shared agent while PR duties exist |
| Backlog manager | Interactive Discovery, Research, POC, Specify, ticket breakdown and approved domain-document PRs | One conversation in the dedicated `Discovery` worktree/workspace; spawn when backlog is unclear, empty or blocked |
| Ship / Patch / Refactor | Implement the assigned issue or agreed graph | One bounded delivery lane |
| Roast | Independent review against requirements and selected doctrine | Fresh reviewer per implementation cycle; retire after accepted return |
| Blocker investigator | Challenge a developer's blocker; return a small answer with evidence | One bounded investigation, not another manager |
| PR coordinator | Human proxy: rank merge-ready PRs by impact, verify issue fulfillment, merge under MERGE | Only when requested; human otherwise |

Do not re-question a PR coordinator request. Resolve missing repository merge policy, not the human's intent to delegate.
Do not activate one merely because a repository looks experimental.

Default pool: **six developers**, not six total agents or six delivery owners.
A feature lane reserves **two** slots. Bug, hardening and refactor each reserve
**one**. Three features, two features plus two fixes, or six fixes fill the pool.
Support roles and reviewers are outside the pool, bounded by useful work.
Without a spare slot, queue the next feature; do not pretend it costs one.

Count every code-writing descendant, including a writing route owner, inside
its lane's reservation. Do not double-count an already-reserved red/green pair.
Reserve before launch; uncertain launch or termination still consumes slots.
Record actual developer IDs and isolated write worktrees. Do not add hidden
implementers beneath a route. A lane may schedule a graph serially within its two slots, not gain unlimited parallel capacity.

Joe prefers paired TDD for features, especially greenfield: one developer proves
red, the other makes it green. Preserve independent evidence and serialize
integration; never have two writers share an index. With an agreed non-TDD exception, the second developer builds useful tests/acceptance coverage against the first's implementation. No invented red claim or forced legacy
test-framework retrofit. Feature staffing still costs two.
Standalone Ship is TDD opt-in. Patch and Refactor use one developer without a
forced red/green workflow. All routes still need useful proof.

Refactor applies `laziness` (KISS and YAGNI) and `solid`, plus `worktrees` for
PR work. There are no separate `kiss` or `yagni` catalog IDs. Preserve behavior,
improve structure, use tests where they help. Initial Roast covers preservation,
ownership/interfaces and needless complexity as distinct angles. Use independent reviewers where separate expertise is needed, not a standing committee.
Resolve conflicting findings with evidence; unresolved semantics return to the
human. Follow-up Roast checks fixes, affected behavior and new breakage while
retaining whole-candidate coverage.

## PM stays small

PM reads compact results and source pointers, not every transcript and diff.
Delegate deep backlog analysis, blocker investigation and code review. Use
Chart-a-course when the goal or dependency picture changes, not every tick.
Dispatch through existing ready-for-agent mappings and actual dependencies.
Blocked tickets do not become eligible just because their ready label remains.

## Startup must reach useful work

Human heartbeat activation/resume includes a first bounded work pass, not just
timer setup. Reconcile existing owners, then reserve/dispatch eligible work now
within capacity and authority. Observe the returned identity and accepted
assignment; for retained roles, verify accepted reassignment instead of spawning
duplicates. Start Shepherd only for actual PR duties and reuse the backlog
manager for unresolved questions. Do not create idle roles to fill six slots.

Before returning, the existing pass receipt names accepted initial work and
owners, or the exact pending launch, missing capability, human decision,
ownership conflict or evidence that no selected work is eligible. Name its
responsible owner and next action. Uncertain creation stays pending, not retried;
a reservation or timer alone is not a running worker. If a result is still
pending, report startup incomplete and keep its callback/next-pass duty.

Discovery questions block their dependent work, not every independent eligible
ticket. Preserve real global gates such as unknown ownership or unresolved
publication coverage; never bypass them to manufacture startup progress.
Once the outcome is recorded, release the pass and await actual callbacks/wakes.

## Keep inquiry and maintenance scoped

The backlog manager is the single human-facing inquiry lane, retaining full
Discovery artifacts, actual human answers and existing recording/publication
gates. PM routes new questions there; no competing interrogation chats.
Give it one dedicated Git worktree and registered workspace named **`Discovery`**
in the existing repository project, separate from PM and delivery worktrees.
Name its agent `Discovery` or `Backlog manager` so the conversation is identifiable.
Reuse that workspace and conversation across questions and passes, not one per
issue. Follow WORKSPACE's actual Git/base/ownership checks; a UI alias of the PM
worktree is not isolation. Other bounded read-only helpers may share an appropriate
existing workspace; Discovery's persistent role is the exception.

For an existing Discovery agent in the PM workspace, do not silently relocate
it or create a second interviewer. Human-directed management must reconcile
its active work, questions, artifacts and heartbeat, then use supported movement
or an acknowledged handoff to the dedicated workspace. Preserve the old
conversation until custody transfers; report unavailable relocation explicitly.
Never spoof placement or archive the primary human chat.

While awaiting the human, retain the exact question/input revision. Recheck
changed answers/evidence; do not repeat the research or open another interview
on unchanged inputs. A heartbeat costs a turn; it is not free event delivery.
Pass the actual human approval and its scope to the receiving role. Do not add
a publication or execution hold merely because the work crosses a handoff.
Where approval genuinely covers only requirements, preserve the remaining
publication gate; ask only for that missing action, not approval of the same
requirements again. Narrower explicit human restrictions always win.

### Surface every human wait

Present each new or materially revised human wait in the primary coordinator
conversation, then notify on resolution or a meaningful blocker change.
Deduplicate by waiting owner and question/input revision, not by pass ID.
Do not repeat unchanged questions on each heartbeat. Remind only at a separately
human-requested interval while the team is otherwise doing useful work; a
reminder is never a reason to keep timers alive.
Consolidate due notifications into one concise **Waiting on you** list:
agent/role, exact question or action needed,
affected work, and a direct agent/conversation link when the runtime supplies a
verified one. Otherwise give its exact project, workspace, agent title and ID;
never invent a URL or block the reminder because links are unavailable.

The waiting agent supplies its question and context; Discovery owns the detailed
interview. PM makes it findable from the primary chat, including permission,
approval and product-decision waits from any role or descendant, not only Discovery.
Consume callbacks promptly and retain outstanding waits until
an actual answer, withdrawal or acknowledged reassignment changes them.
Match answers to the question revision; a timer, sent prompt or completed
monitoring pass is not an answer.

Preserve in the existing assignment/pass evidence: question/input revision,
waiting agent identity/locator, whether the question is only prepared or has
actually been presented in the primary chat, the presentation evidence and
the actual answer/resolution when received. An internal question artifact or
handoff is not delivery to the human. Do not claim the human saw or answered it
from a send receipt. A question not yet presented is a PM communication duty,
not a failure by the human to respond.

If pause, a busy owner or unavailable reads prevent fresh reconciliation,
respect the dispatch gate and label any carried wait with its last verified
state and current uncertainty. Never claim fresh waiting/completion evidence.
This uses existing passes/callbacks, not extra timers or automatic resume.
Unchanged passes need no status chatter, including already-presented human waits. Distinguish
monitoring completed, work blocked, implementation finished, PR ready and merged.

### Useful work or idle shutdown

Do not confuse a recurring observation duty with useful progress. On each pass,
consume returns, advance authorized review/repair/merge work, then check for
independent eligible deliveries or a bounded, decision-relevant planning task.
Challenge an apparent dependency against the consumer's actual agreed base:
unmerged does not by itself mean unavailable, but do not silently adopt stacked
PRs, change grouping or waive acceptance. Ask for a changed base only if needed;
continue other authorized work meanwhile. Do not invent busywork to fill slots.

Shepherd owns routine PR/check/ref observation; PM consumes its compact,
timestamped report instead of repeating those provider queries. PM refreshes
only missing, changed, stale-for-the-next-action or action-critical evidence.
No new action and unchanged inputs need no exhaustive revalidation. Each role
reports its next useful action, expected evidence and next due time, not just
"running" or a successful heartbeat. A retained PR or unanswered question alone
does not justify five-minute monitoring indefinitely.

For Paseo, kickoff includes **idle shutdown**: explain and record authority to
close dispatch, remove owned recurring jobs and retain work when no useful
authorized next action remains. New activations save STATE's `idleShutdown`
reference; it includes the child disposition and human-only resume boundary.
This is part of kickoff, not another permission interview at the point of
exhaustion. Do not infer this grant for an older board or another adapter.

Apply this decision before releasing every pass:

- **Useful work available:** dispatch or continue it within the existing grant.
  Human-blocked work holds only its dependent scope, not unrelated deliveries.
- **Real work in flight:** keep custody for implementation, review, tests or CI
  with an owner, expected result and a concrete next observation time. Do not
  kill a long-running task because its commit has not changed. At that time,
  check the expected evidence and route an actual stall through bounded blocker
  recovery. Generic "still running" never renews the wait indefinitely.
- **Only human/external blockers or an exhausted backlog:** suspend in this
  pass after the bounded eligibility check. No extra unchanged passes, overnight
  monitoring or separate investigation solely to confirm a known human wait.
- **Runtime/observation failure:** preserve uncertainty, not an empty-backlog
  claim. Make at most one supported reconciliation attempt per unchanged
  failure episode, recorded with `record` across passes. If safe useful work
  remains, isolate the affected scope; otherwise suspend with the gap and exact
  human recovery action. A new tick or a rewritten receipt is not new evidence.

Suspension uses STATE's lease-fenced `suspend`, which sets the existing board
to **paused**, not a new controller or auto-resumable waiting mode. Then delete
the PM heartbeat and direct every owned timer-bearing role/descendant to delete
its exact timer using the existing lifecycle. Legacy fresh mode pauses its
owned schedule through the supported API. Record successful receipts and
uncertain failures separately; a local pause does not stop external billing.
Delete working timers even if another deletion fails. No automatic retry timer,
replacement coordinator timer or archival-as-deletion shortcut.

Preserve conversations, branches, PRs, worktrees, pending results and custody.
Apply the recorded child disposition; do not abandon active children, cancel
writes or declare obligations completed. An unresolved owner/cleanup operation
is retained for callbacks or human recovery, not another polling loop. Late
deletion/result callbacks use STATE's paused cleanup-only path under the saved
human grant after the pass releases; they cannot resume work. Report once:
delivered outcomes, unfinished scope, active work if any, precise blocker/ask,
timer cleanup status, monitoring gaps and the human action needed to resume.
Say **suspended; timer cleanup incomplete** when absence is unverified, never
"stopped" merely because the board is paused. Finish the bounded turn.

Human stop preempts planning/reporting: close dispatch and remove owned timers
first, reconcile child disposition, then produce any requested retrospective.
Neither a human answer, PR merge, role callback nor queued wake automatically
resumes a suspended team; use the human-directed resume procedure.

### Preserve agreed knowledge through PRs

Discovery may periodically open scoped documentation PRs for agreed findings,
domain language/glossaries and architecture decisions within its recorded
documentation-publication grant. Preserve actual alignment, recording and
exact-file approval gates; do not present proposals as decisions. Use the
repository's canonical context/domain/ADR locations, not a competing knowledge
tree. Human-owned intent/doctrine still require their separate explicit edit
authorization. Private transcripts, credentials and runtime receipts stay private.

Use its dedicated worktree and the repository's configured integration branch,
not a hardcoded `main`; clarify unresolved target policy rather than retargeting.
Serialize document writes with any open PR on that branch; reuse a matching PR
instead of generating one per heartbeat. Publish only a meaningful agreed delta,
not on a timer quota. Follow DELIVERY for independent review, relevant checks,
current-base proof and accepted shared-Shepherd custody. Discovery and Shepherd
explicitly transfer branch-write ownership; inquiry may continue without source
writes while maintenance owns the branch. No self-review approval, merge or
automatic closure. After terminal PR reconciliation, reuse the Discovery
workspace for the next bounded documentation branch without discarding work.

Shepherd does not implement features, rewrite architecture or debug broken tests.
It may do mechanical rebases, regeneration and scoped linter fixes under the
existing maintenance gate. Route anything semantic through PM to a developer.
Each PR retains its own owner, due time and recovery record. Before a developer
writes, Shepherd releases write custody and remains observation-only for that PR.
After reviewed repairs, it observes and accepts custody back. Other PRs continue.

## Challenge blockers, then move on

1. Developer rubber-ducks the blocker. Check known sources and reasonable
   alternatives. A missing guessed filename or two equivalent choices is insufficient. Return attempts, evidence and the exact missing answer.
2. PM assigns a short independent blocker investigator. It checks the claim from another lens and returns guidance or confirmed blockage with pointers. PM does not repeat deep investigation in its own context.
3. If the answer exists, send it to the current developer and verify receipt.
4. First confirmed work blocker: preserve partial work and evidence, settle
   custody, archive the worker, then try one fresh agent on a fresh worktree.
   Fresh applies to **every** previous participant: lane owner and each staffed developer, including retired ones. Reusing any such agent or worktree is not fresh and is refused.
   Pass requirements and facts, not the predecessor's conclusion. Keep the old
   result available for recovery without priming the new diagnosis.
5. Second independently confirmed attempt on that issue: apply its configured
   blocked tag, comment why and what answers are needed, return it to backlog,
   retire all its delivery workers and fill capacity with the next ready issue.
   Track uncertain tracker writes on the same board; never duplicate them.
6. Reuse or spawn the backlog manager to unblock it with the human. Preserve
   the issue's attempt history across workers, ticks and restarts. New answers plus verified readiness close the episode; unchanged ticks never reset it.

Permission denial, missing credentials and explicit human decisions are not
fresh-context experiments. Record the exact blocker once and repeat PM's
human-wait notification only on change; apply idle shutdown if no useful work
remains. Wait for the real grant or answer, never evade
it with another agent/provider. Runtime cancellation is
not automatically a work blocker: reconcile descendants and partial writes first.

Before removing a run-owned worktree, stop writers, preserve relevant tracked,
untracked and ignored work, push a recoverable branch, and compare exact local
and remote commit IDs. Never push secrets or private runtime evidence; preserve them separately in an approved location. Verify no unpreserved files remain,
then remove only that exact owned worktree and read back the result.
Failed push, dirty state, uncertain ownership or inaccessible evidence means
**keep the local copy**, report the cleanup blocker, and move on safely.
Archiving an agent is not permission to delete its workspace or branch.
Retirement is a duty, not a deletion quota: record deliberate worktree retention with `cleanup` and `retained: true` as a decided board outcome. Accepted cleanup receipts are final: identical replay is allowed; changed branch, head or evidence fails rather than overwriting history. Each settled worker remains in the bounded view's retirement queue with its recovery locator until archived and its worktree actually removed or retained.

## Permissions follow the human

Use [RUNTIME's permission contract](RUNTIME.md#permission-preserving-dispatch)
for every developer, reviewer, investigator, support role and replacement.
Copy the parent's actual authorized mode and permission features before the
first prompt, then inspect the child. An "inherit" prompt is not a setting.
Do not undo a human's current Allow All/Auto Accept choice using a stale board
default. Do not enable either merely to make progress. Respect revocations and
narrower assignments.

Same provider, same snapshot. For a **different** target provider, verify the
target's actual policy against the parent's before launching and record that
mapping with STATE's `permission-preflight`: an `equivalent` mapping when the
target expresses the same meaning, or a human-authorized mapping for a real
difference. Plan the exact workspace and worktree, then record
`permission-launch` with the identity the runtime actually created, so one
approval cannot cover a different developer, worktree or later launch.
Preserve the human's choices; never fabricate a matching snapshot,
widen the target or downgrade a granted mode silently. Ask the human only for genuinely ambiguous or authority-escalating mappings, not every mixed-provider task. Unmapped or unsupported equivalence queues that launch
with the precise missing choice.

## Choose current frontier models

At selection time, inspect the host's actual providers, models and configured profiles. Pick a current capable frontier model for
implementation, review, investigation and planning; profiles may set model and
reasoning, but never replace the human's permission choice. Cheap or small
models are for narrow mechanical lookups, not features, diagnosis or review.
Record the actual selected model with the assignment. Do not hardcode model
names in this skill, a prompt or the board; an aging identifier in old notes is
a hint to re-inspect, not a selection. Report unavailable discovery; do not guess a name.

## Role heartbeats belong to PM

PM owns heartbeat inventory/lifecycle: its own, plus Shepherd's and the backlog
manager's while they have actionable recurring duties. PM's heartbeat targets **this
same PM conversation** and continues Joe-mode; it never reruns setup or starts
another controller. Its prompt names Joe-mode continuation, the repository,
workspace, board path, PM ownership and the pause/stop gates, then one bounded
pass: check workers, blockers, permissions, PRs and free slots; take authorized
next steps; reuse existing assignments; record compact progress; present new or
changed human waits with verified links/locators; apply useful-work-or-shutdown
before returning. Notify only meaningful changes, not unchanged reminders.

Keep role timers only while there is an actionable recurring duty. If Discovery
is solely awaiting a human answer, or Shepherd is solely awaiting human merge,
delete that role's timer and retain its conversation/custody. Other eligible
work may keep PM active; it explicitly continues the role when inputs change.
Role existence alone is not a reason to recreate its timer on the next pass.

Developers, roasters and the PR coordinator have **no default timer**: they use
completion notifications and PM's explicit continuation. A bounded developer
timer is allowed only for a real recurring duty (such as a long external wait
the lane must poll), created and deleted by that target agent, with PM
recording its intent, ID and removal like any other role job.
Default cadence is five minutes; preserve explicit cadence and lifetime choices.
Shepherd uses its configured override instead of changing PM's timer to match
every PR. New/urgent PR events can request an immediate bounded observation.

The actual role calls agent-bound create/delete. PM sends the bounded task, records intent before creation, then receives and records the exact target/ID/settings receipt on the same board. No identity spoofing or schedule-only queries.
Each role checks the board's pause gate and its own assignment before acting.
Role callbacks carry results; only PM writes the shared board under its lease.
PM records actual wakes and gaps, checks role health, and reuses known jobs.
Fence uncertain create/delete; do not retry into duplicates.
An old external reviewer's unknown timer does not by itself require a new
coordinator timer or block unrelated development. Reconcile actual overlapping
write/merge authority; fence the affected scope when ownership is uncertain.
If it prevents all useful work, use bounded recovery and idle shutdown rather
than repeatedly reporting the same coordinator-activation blocker.

Project pause, preauthorized idle shutdown or human stop closes dispatch first,
then PM directs **all three** bound roles and any timer-bearing descendants
to delete their owned jobs. Reconcile in-flight effects. Retain agents needed for deletion reporting or child preservation; PM's deletion alone never proves a clean pause. Human resume verifies old absence and surviving duties before
recreating the needed jobs with the remaining grant. Roles cannot self-resume.
No backlog/PR duties left: delete that role's timer, accept its final result,
archive its agent and verify active-view removal.

Keep the primary workspace clean. Use names such as `Ship #42 small-search`,
`Patch #43 empty-result`, `Roast #42 small-search`, `Shepherding`,
`Discovery`, `Backlog manager`, `Project manager`, `PR coordinator`. One project per Git
repository; one workspace per actual worktree, shared by its assigned agents.
Discovery uses its dedicated named worktree/workspace, not the PM workspace.
Do not retain stalled developers for hypothetical work. Preserve and transfer actual duties before retirement; quiet persistent roles with live duties are not stale. Never archive a whole project to hide one agent.
