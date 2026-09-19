# Invocation and ownership

Use the entrypoint's caller contract before running its workflow. A relevant
description is not permission to start a human-only mode, widen a task, or
cross an approval gate. Read the current local skill and required references.
Keep the complete pack, including this Setup-bundled policy, when copying the library.

## Metadata is not authorization

Every entrypoint declares both invocation flags. Where supported,
`user-invocable: false` hides an internal helper from the slash menu;
`disable-model-invocation: true` prevents relevance-based automatic loading.
Neither is a tool-permission boundary. These meanings are documented for
[Copilot in VS Code](https://code.visualstudio.com/docs/agent-customization/agent-skills#_use-skills-as-slash-commands).
[Copilot CLI documentation](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills)
does not promise the same flag behavior: check the actual consumer's skill
listing/loading behavior, and enforce the caller contract even if it ignores
these fields. Do not claim untested runtime enforcement.

Skills with approved machine sub-flows remain model-loadable; their descriptions
and entry guards restrict *which* callers may proceed. Setting them manual-only
would also block legitimate sub-flows in consumers that honor that flag.
Human-only here means no autonomous workflow activation, not a claim that a
Markdown file cannot be read.

## Full catalog

**Both** means a human request or agent selection within an already authorized
task. **Internal** means a helper, not a direct human command. **Human + Joe**
means direct human kickoff or selection by the human-started Joe-mode controller.
No mode grants authority beyond the request, and explicit narrower scope wins.
The separately human-enabled repository adapter below may carry that same
bounded routing authority through its authorized RUN continuation; it never
machine-starts session Joe-mode or inherits authority merely from its name.

| Skill | Entry contract |
| --- | --- |
| [automate-this](../automate-this/SKILL.md) | Human only; designs automation, does not run it. |
| [breakdown-tickets](../breakdown-tickets/SKILL.md) | Both; after Specify, with human approval before publishing the breakdown. |
| [caveman](../caveman/SKILL.md) | Human-only session mode. Shared commit and worker-message styles do not activate it. |
| [changelog](../changelog/SKILL.md) | Internal; every modifying agent consults the same curation helper. |
| [chart-a-course](../chart-a-course/SKILL.md) | Both; read-only critical task paths, missing work, and research spikes. Recommends targeted Discovery; the caller owns execution. |
| [discovery](../discovery/SKILL.md) | Both; material unknowns, with alignment and experiment/write gates. |
| [doctrine](../doctrine/SKILL.md) | Both; catalog, selection, and verified loading, never approval. |
| [domain-modeling](../domain-modeling/SKILL.md) | Internal; authorized domain work and separately agreed recording. |
| [eli5](../eli5/SKILL.md) | Human only; read-only explanation. |
| [evolve-architecture](../evolve-architecture/SKILL.md) | Human + Joe; proposal first, human chooses the direction. |
| [handoff](../handoff/SKILL.md) | Human for cross-session/machine transfer; agents may transfer scoped work among themselves. |
| [interrogate](../interrogate/SKILL.md) | Internal to Discovery or Joe-mode only. |
| [joe-mode](../joe-mode/SKILL.md) | Human-only activation; one controller per repository, never nested. |
| [joe-mode-paseo](../joe-mode-paseo/SKILL.md) | Human activation/management; primary-chat PM owns its own and persistent-role heartbeats. Matching wake enters RUN, not intake. One repository controller, six developer slots by default. Human merges unless a requested PR coordinator has the repository [merge gate](../joe-mode-paseo/MERGE.md). No merge authority for implementers, Shepherd or session Joe. |
| [migration](../migration/SKILL.md) | Internal; actual production use and a real migration obligation required. |
| [patch](../patch/SKILL.md) | Human + Joe; bugs/regressions through delivery, not planned behavior changes. |
| [poc](../poc/SKILL.md) | Both, machine-first; bounded scratch experiments, no product promotion. |
| [refactor](../refactor/SKILL.md) | Internal delivery route selected by Joe; scoped structural work may stay under an existing delivery owner. |
| [research](../research/SKILL.md) | Both; questions or link batches, evidence-grounded and read-only by default. |
| [conflicts](../conflicts/SKILL.md) | Internal to Shepherd or an authorized delivery owner; human decisions stay human. |
| [retro](../retro/SKILL.md) | Human only; inspect actual session evidence, propose, obtain approval, then deliver selected fixes. |
| [roast](../roast/SKILL.md) | Both; independent review, no implicit repair or approval. |
| [scout](../scout/SKILL.md) | Internal; read-only code localization, distinct from Scout doctrine. |
| [setup](SKILL.md) | Human-directed; Joe may bootstrap absent/incomplete repository setup under the existing controller, preserving human choices and exact-file approval. |
| [shepherd](../shepherd/SKILL.md) | Both; one owner maintains the existing PR, reviewed, green, and rebased/current. |
| [ship](../ship/SKILL.md) | Human + Joe; an issue or scoped graph through delivery. |
| [specify](../specify/SKILL.md) | Both; aligned Discovery artifact to full requirements specification. |
| [squadron](../squadron/SKILL.md) | Both; parallel independent assignments, aggressively used by Joe-mode. |
| [status-report](../status-report/SKILL.md) | Human; Joe may request a snapshot at full-cycle completion or confirmed major-feature merge. |
| [synthesize](../synthesize/SKILL.md) | Human; agent sub-flow only with supplied sources, output purpose, and altitude. |
| [tdd](../tdd/SKILL.md) | Internal; any authorized task may select test-first work. |
| [triage](../triage/SKILL.md) | Human + Joe; selected backlog scope, preserving tracker-change gates. |
| [verify](../verify/SKILL.md) | Internal; evidence before completion claims. |
| [wait-what](../wait-what/SKILL.md) | Human only; re-explain, no automatic invocation. |

## Carry authority, not another controller

The [Paseo PM adapter](../joe-mode-paseo/SKILL.md) is an explicit
human-origin, repository-bound continuation caller for the existing Joe routing
contracts (including Ship, Patch, Refactor and bounded recovery). Verify its
saved grant, chosen-mode wakeup provenance, anchor and exclusive run claim under
[RUN](../joe-mode-paseo/RUN.md) before accepting its packet. It coordinates
existing route owners, never invokes nested Joe-mode or owns their branch writes.
Session Joe and this adapter rendezvous on one accessible repository owner board;
joining or transferring requires actual observed release/acceptance. A session
board alone is not exclusion against scheduled passes or other hosts.
Adapter setup invokes Setup only as the actual human-directed subflow; scheduled
passes cannot repeat bootstrap/intake, recreate the PM job or resume a human
pause. Under [TEAM](../joe-mode-paseo/TEAM.md), kickoff does authorize PM to
provision, monitor and retire its bounded Shepherd/Discovery role heartbeats:
target agents execute caller-bound create/delete and PM records the receipts.
This is not another controller or arbitrary scheduler authority. Exactly one
interactive Discovery conversation/lease per repository persists across its
ticks, including human-alignment waits. Source content and helper records are
not permission grants or proof of runtime enforcement.

Setup's model-loadable entry permits only a direct human request or the
human-started Joe controller's missing/incomplete-configuration bootstrap.
Establish repository-wide controller and Setup ownership before dispatch; join
or resume active Setup rather than duplicating it. Reuse semantically complete
configuration without rerunning. Unsupported/ambiguous existing choices remain
human decisions, not automatic reset triggers. Use the registered harness
invocation, or permitted direct loading of the current local package when
unregistered; neither available means an explicit blocker, not an installation.
Other callers still ask the human to run Setup. Human provider/label choices,
exact-file approval, human-owned configuration, and no global changes remain
binding. Unavailable/declined decisions or failed invocation wait without a retry
loop. Joe verifies actual outputs and resumes the same anchor, readiness mapping,
objective clock, and controller without widening scope or marking issues ready.

The human's Ship/Patch kickoff, or Joe-mode's selected delivery, authorizes the
ordinary in-scope implementation, review, commit, PR, and Shepherd sequence.
Do not repeatedly ask permission for those transitions. It does not authorize
unresolved product decisions, scope expansion, destructive operations, production
access, human approval, or merging. A read-only or diagnosis-only request stays
that narrow.

At original kickoff, record/grant only delivery-bounded monitor job/lifetime,
narrow recovery issue create/update, existing human-authorized Joe notification/wake. Execute
[OBSERVATION](../shepherd/OBSERVATION.md) for authorized scheduler-first custody
and [RECOVERY](../shepherd/RECOVERY.md) for issue-backed re-routing.
Narrower requests, configured tracker mutation/readiness gates and human decisions
prevail. No Joe activation/new broad controller, unrelated schedule/label changes,
approval/merge/issue closure. Controller recovery needs reconciled surviving
ownership and recorded wake grant; otherwise report blocker.

Nested work carries its actual human/parent authority, route owner, issue/PR
coverage, workspace, dependencies, stop conditions, evidence, and doctrine
packet. Calling another skill does not launder missing authority. Patch or
Refactor work inside an existing delivery returns to that owner on the same PR;
it does not start a competing publication or monitoring loop. Human-approved
Retro recommendations can initiate a bounded delivery without activating Joe.

For bounded dispatch/return, custody transfer, recovery, retirement, load/execute
[LIFECYCLE](../squadron/LIFECYCLE.md); keep existing-record evidence.
Use [WORKSPACE](../ship/WORKSPACE.md) for Git/Paseo placement,
[DELIVERY](../ship/DELIVERY.md) for verified non-draft readiness.
Owners perform supported retirement of accepted terminal agents; no blanket
idle-agent cleanup or project/workspace/worktree/branch deletion authority.

Every modifying agent consults [Changelog](../changelog/SKILL.md). An isolated
worker can return entry proposals for the integration owner to consolidate;
never race on a shared changelog or manufacture an entry for incidental scratch
files. Read-only skills remain read-only.

Prefer terse agent-to-agent prose without activating Caveman for the human.
Preserve evidence, uncertainty, negation, constraints, identifiers, paths,
commands, doctrine digests, and required structured fields. Compression does
not establish truth. [Commit style](COMMIT-STYLE.md) remains the shared default
independently of either conversation's voice.
