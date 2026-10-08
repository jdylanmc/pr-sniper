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
The separately human-enabled CMUX adapter below carries that same bounded
routing authority within the active human session; it never machine-starts
Joe-mode or inherits authority merely from its name.

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
| [joe-mode-cmux](../joe-mode-cmux/SKILL.md) | Human-only session adapter from a Maestro-managed coordinator. Public native launch/messaging and verified invoking account; all roles stay visible. No scheduler or hidden-agent fallback. Only the independently authorized PR coordinator may merge under [MERGE](../joe-mode/MERGE.md), never PM, implementers or Shepherd. |
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

The [CMUX adapter](../joe-mode-cmux/SKILL.md) is an explicit human-started,
session-bound presentation of the same Joe controller. It launches interactive
Maestro roles only from a messaging-enabled managed coordinator through native
account-inheriting launch with explicit model selection. It carries no
scheduler, heartbeat, tracker or approval authority. Any repository merge grant
belongs only to the independently designated coordinator under MERGE.

Registration alone does not make a coordinator a messaging recipient.
The adapter must not inject keystrokes, infer delivery/completion from a send,
or claim it consumed unseen worker output. Joe and CMUX role placement keeps
PM on current `main`, Discovery on `discovery/<feat>`, and an explicitly
authorized merge coordinator on `pr-sniper`; placement grants no merge power.
Missing communication blocks activation. Propagate the runtime contract
through nested roles; generic harness examples never authorize fallback.
Joe core stays runtime-independent; only the adapter depends on Maestro.

Both entrypoints reconcile the same repository owner board, including
surviving legacy owners/jobs. Removing an adapter grants no transfer or job
cancellation. Join or transfer only after observed release/acceptance. A local
board is not exclusion against another run or host. Preserve one interactive
Discovery lane and existing human waits. Installation never starts Setup,
Joe-mode, a scheduler or a merge role.

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
Use [WORKSPACE](../ship/WORKSPACE.md) for Git/CMUX placement,
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
