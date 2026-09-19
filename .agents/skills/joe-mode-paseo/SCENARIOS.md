# Acceptance scenarios

These team scenarios supplement legacy scheduler cases. Run against the actual host before claiming unattended team operation; local state tests prove only named helper seams.

| Team case | Required observation |
| --- | --- |
| Primary chat starts Joe | That same agent is PM; one project per repo, one workspace per actual worktree |
| Human heartbeat startup/resume with eligible work | Same PM resumes, claims and completes one bounded work pass before returning; actual accepted worker identity/assignment, not just a timer or reservation; initial human pass is not counted as a heartbeat wake |
| Startup with only blocked work or uncertain dispatch | Exact eligibility/blocker or pending-operation evidence, responsible owner and next action in the existing pass receipt; no invented ready work, duplicate launch or successful-team claim |
| Paused legacy board has settled-ready returns but no lease | Human/reconciliation evidence permits record/settle/archive while still paused; no temporary resume/heartbeat to obtain cleanup authority; missing grants/live lease/unfinished duties reject without mutation |
| Legacy Discovery survives team conversion | Actual administrative-end acknowledgment and unanswered questions preserved; same agent retained/reassigned, not archived or declared aligned; one final verified heartbeat replacement, team resumes and first eligible work is accepted |
| Six developer slots | Three features, two features plus two fixes, or six fixes; support roles outside the pool; pending launches still counted |
| Parent Agent + Auto Accept, or human-selected Allow All | Child mode **and permission features** match before and after bootstrap; no stale-mode "repair" |
| Verified cross-provider target policy | Preflight recorded before launch with the compared capabilities and rationale; bind/staff rejects any later drift |
| Ambiguous or escalating cross-provider mapping | One precise pre-dispatch blocker naming the missing human choice; no fallback, fabricated snapshot, auto-approval or repeated child launches |
| Model selection for a lane | Current provider/model/profile discovery recorded with the assignment; frontier model for substantive work; no hardcoded or weak default |
| Routine pass reads the board | Bounded current view is enough to route work; settled workers remain in the actionable retirement queue until archived and worktrees removed or deliberately retained; explicitly request full history for recovery/audit; durable records unchanged |
| PRs and unclear backlog | One shared Shepherd and one backlog manager; each creates its own heartbeat, PM records exact target/ID/settings |
| Pause with all roles live | Board gate closes first; all owned role timers deleted with receipts; queued effects reconciled; no auto-resume |
| Discovery waits for human | Same question/input revision retained; no duplicate interview or repeated research; PM presents the exact question/link in the primary chat and repeats the outstanding reminder every pass |
| Discovery placement | One dedicated Git worktree and workspace named `Discovery` under the existing repository project, separate from PM; same inquiry conversation reused across questions/passes, no UI alias masquerading as isolation |
| Existing Discovery in PM workspace | Human-directed acknowledged placement transfer preserves questions, artifacts, live duties and heartbeat; no silent move, second interviewer or premature archival |
| Agreed domain knowledge ready to preserve | Within recorded publication scope, meaningful docs-only PR from Discovery's worktree to configured integration branch; canonical domain/context/ADR files, alignment and recording gates, independent review/checks/shared-Shepherd custody; no timer-quota PRs, private transcripts, intent/doctrine edits or self-merge |
| Any role or descendant awaits the human | Primary coordinator emits one concise Waiting on you list every pass with agent, exact action/question, affected work and verified link/locator; no unchanged-pass suppression or generic completion claim |
| Questions prepared but not presented | PM tracks its undelivered communication duty, actually presents the questions through the primary return channel and records available evidence; no claim that the human failed to answer an unseen internal artifact |
| Agent link unavailable / question unchanged / answer arrives | Exact project/workspace/title/ID fallback; repeat the reminder without repeating research; stop or update it only after actual answer/withdrawal/acknowledged reassignment for that question revision |
| Paused/busy pass or failed wait observation | Preserve dispatch gate, no automatic resume or extra reminder timer; any carried wait explicitly marked last verified with current uncertainty, not falsely fresh |
| First confirmed work blocker | Self-challenge plus independent investigator; answer guides existing worker, otherwise one retry whose agents and worktrees are fresh for every prior participant, including retired developers |
| Recurring duty inside a lane | Bounded timer created and deleted by that target agent with PM-recorded intent, ID and removal; developers otherwise run on callbacks and PM continuation, never a default timer |
| Second confirmed work blocker | Issue blocked/tagged/commented, delivery workers retired, next ready issue dispatched; Discovery takes missing answers |
| Push fails during cleanup | Local worktree and evidence kept; no success-shaped deletion |
| Requested PR coordinator | Create role without re-questioning choice; resolve adequate repo gate; verify and merge highest-impact eligible candidate, no self-approval |
| Standalone Ship / Joe feature / Refactor | TDD opt-in / preferred pair with legacy exception / one developer, useful tests, laziness and SOLID review |

Local tests prove only implemented helper and shipped package contracts. These scenarios need a separately authorized compatible runtime and actual observations; never activate anything to make a library PR appear green. Record each as observed, blocked or unverified with exact evidence.

| Scenario | Required observation |
| --- | --- |
| Install or individually select PM alongside prerequisites | Entrypoint/support copied, no setup output, schedule, workspace or activation |
| Human setup with complete custom configuration | Real contents/identity accepted unchanged; ask only unsettled activation questions |
| Missing setup / ambiguous provider / unavailable human | Existing Setup gates preserved, no reset, no schedule or delivery while blocked |
| PR coordinator requested with a defined repository gate | Preserve human grant/exact policy; independent Roast, successful CI/lint, then coordinator rubber-duck reasoning and verification before guarded provider merge; actual merged readback before completion |
| Missing/ambiguous repository merge gate, commands or grant | Clarify with the human; no invented policy, skipped CI/lint, experimental-label inference or merge |
| Worker claims ready; evidence is stale, checks fail, review blocks or target advances | Coordinator withholds merge, reconciles with existing owner/Shepherd, refreshes affected evidence; no self-approval, policy bypass or worker merge authority |
| Merge response uncertain or provider queues candidate | Preserve one pending PR/head operation, inspect before retry; queued is not merged; only actual merged state advances dependencies |
| Existing human-mode board or session Joe | Human merging unchanged; no automatic authority/config upgrade or new merger |
| Human changes a paused board's merge policy | Reconcile owners/pending merges and release/fence lease; `configure-merge` preserves workers, wakeup, pending/run history and other config; repeat is idempotent and does not resume; enabled/live-lease updates fail |
| Known incompatible upstream fresh mapping, operator insists on fresh | Activation blocked **before** creation; no invented workspace/project parameters, heartbeat or hidden fallback |
| Same incompatible fresh host, heartbeat offered | After capability inspection, explain conversation/lifetime difference, recommend heartbeat, record explicit consent and actual bound PM identity before creating one owned job; refusal leaves inactive |
| Compatible runtime, configured-cadence fresh schedule | Read back one matching job/existing binding plus safe workspace lifetime; distinguish initial observation from later fresh-run receipt |
| Human delegates runner mechanics | Explain/record primary-chat PM heartbeat and approved cadence; no forced API-choice interview or silent reinterpretation of explicit fresh-only requests |
| Five-minute default or human-selected cadence | Stored job cron matches board config; repeated init/resume cannot change cadence; legacy one-minute board remains unchanged |
| Consented heartbeat with correct existing workspace | Actual job targets primary/reused PM, not disposable bootstrap/reviewer; complete join to correct project/workspace/cwd; distinguish initial observation from later deliveries |
| Heartbeat create returns its active summary but schedule inspection rejects the ID | Validate creation receipt fields/actual PM mapping; enable after initial observation, preserve job, report recurring operation unverified; no schedule APIs for heartbeat readback |
| Empty schedule list after heartbeat creation | Not evidence of heartbeat absence; listing filters out heartbeats; use the saved receipt and actual wakeups, never create a duplicate |
| Multiple actual heartbeat deliveries | Same PM agent receives bounded passes, new fencing token each time, returns/idles between prompts; same delivery/Discovery agents; no new PM agents/workspaces, loops or nested wakeups |
| Wrong target / unknown mode / missing consent | Fail before dispatch; actual owner reconciles any wrong-target creation; never adopt it as PM or replace blindly |
| Reinvoke after uncertain schedule-create response | Matching operation inspected/adopted, never duplicated; failed query not empty |
| Two ticks / another worktree / live session Joe | One reconciled logical owner and one claimed pass, no duplicate workers; explicit custody transfer from session Joe |
| Previous pass busy or crashed | Busy skip; no age takeover; explicit stopped-owner fencing and descendant/partial-work reconciliation for recovery |
| Six/seven developer slots; cancelled parent with live writer | Seventh slot blocked; pending launches and unreconciled descendants retain capacity; support roles remain bounded outside developer pool |
| Idle Discovery awaiting human across several ticks | Same conversation/lane per repository; second repository may have its own; noninteractive research continues |
| Unaligned recap / unpublished breakdown | No execution shortcut, preserve complete Discovery/Specify artifacts and actual publication approval |
| Approved breakdown publishes children across partial returns | Reserve parent group first; add actual returned IDs monotonically with receipts; hold all new delivery launches until full graph/edges/grouping reconciliation; retries reuse group, reject overlapping child reservations, existing workers continue |
| Changed goal or backlog dependencies | One bounded Chart-a-course assignment for current goal/input revision; consume cited critical path and missing-work findings; no identical fresh agent every tick |
| New requirement arrives while deliveries run | Route to the one existing interactive Discovery lane, retain real human questions and full artifacts; independent eligible delivery continues |
| Worker is running but implementing the wrong thing | Compare accepted scope/actual artifacts with current goal/path; send bounded correction to existing owner, verify receipt, preserve work; no competing writer |
| Completed callback was missed | Next pass accepts preserved result and advances existing assignment once; no duplicate delivery or artificial activity |
| Worktree placement and shared read-only research | Same repository project; independent deliveries isolated; same-worktree agents share one workspace |
| Long tests, idle review, missing permission, true no-progress | Correct classification by live evidence, not age; permission wait and failure do not spawn replacement storms |
| Repeated recovery notification / target moves | One episode/issue/repair under RECOVERY, actual intake and return acknowledgment; same PR, no second controller |
| Human pause races already-dispatched pass | Local gate first; fresh schedule paused via supported interface or heartbeat deleted by bound owner and absence verified; queued prompts return without dispatch, already-issued effects reconciled |
| Heartbeat human resume after pause/stop | Exact old-ID deletion acknowledged, prior pass released/fenced, new creation receipt targets same PM with unchanged scope/settings/lifetime; old-ID/human/reconciliation evidence accepted, workers/config/pending outcomes preserved, old token rejected; no schedule inspection required |
| Heartbeat uncertain deletion or creation / lost receipt | Stay gated; reconcile supported heartbeat-specific evidence or ask human before retry; no false absence from schedule-only errors, competing job, repeated create, fabricated pause API or tick resume |
| Fresh schedule resume / stop | Human-only same-ID supported resume; deletion does not use heartbeat replacement exception; stop verifies only owned job absence and preserves work/monitor responsibilities |
| Terminal worker return | Actually accept complete result, settle current descendants/duties, verify supported agent archival; preserve resources |
| Fresh PM parent with live children or pending self-report | Explicit retention or proven supported acknowledged transfer; no fake detach, orphan assumptions or workspace archive |
| Terminal fresh PM parents with no remaining duties | Next owner actually accepts receipts and archives; no indefinite run-agent accumulation |
| Heartbeat PM at pass end or human pause | Lease released but agent not terminal; retains own wakeup or human-resume duty, children and reporting; no automatic archival |
| Heartbeat PM after explicit stop/end | Owned wakeup absence verified, child/results accepted or handed off and all duties ended before supported agent archival; never archive a shared workspace |
| Host down / denied external reads | Honest observation gap, human-visible narrow permission request, no claimed continued monitoring |
| Dedicated PM errored or terminated | Never claim heartbeat heals controller; report observation gap to human and use explicit fenced takeover preserving workers/results |

Also execute the shared [lifecycle scenarios](../squadron/LIFECYCLE-SCENARIOS.md)
and [observation/recovery scenarios](../shepherd/SCENARIOS.md) for affected
integration contracts. Setup proof and recurring-delivery proof remain separate.
