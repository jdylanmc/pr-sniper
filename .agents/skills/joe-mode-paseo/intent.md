# Intent: joe-mode-paseo

This is an opt-in, repository-bound project manager for Paseo. It keeps my
selected backlog moving through the existing skills and brings me reviewed,
green pull requests to approve and merge, or uses a separate PR coordinator
when I request it under that repository's gate. Installing it does not activate it.
It is a separate skill, not a change to Joe-mode's session-only lifetime.

This is the heartbeat of my engineering team, not a heartbeat whose purpose is
keeping a chat alive. It checks that agents are making progress and working on
the right thing, charts a course through the backlog, takes in new requirements,
and dispatches planning and implementation through Joe-mode's existing skills.
The choice of runner is an implementation detail: follow Paseo's recommended
recipes and explain the resulting behavior rather than making me choose APIs.

## Set it up with me

I choose the repository and invoke setup. It checks the existing Setup outputs
for real completeness and uses Setup where needed, preserving my choices and
exact-file approval. A few questions settle the selected backlog, scope and
non-goals, merge authority, and capacity. The default is six developer slots:
features cost two, bugs/hardening/refactors one. Support roles sit outside this
pool; repository identities and
backlog filters are activation choices, not library defaults.

Human merging remains the default. If I ask for a PR coordinator, create it.
It ranks merge-ready work by impact, checks it against the issue, then merges
within my repository grant. Do not ask if I really meant it. The merge
gate is defined per repository; if it is missing or unclear, clarify it with me.
At minimum it includes independent Roast, successful CI and linting, then
rubber ducking and verification by that coordinator. Workers do not approve
their own work, and this does not bypass repository protections or extend merge
authority to session Joe-mode, delivery workers or Shepherd.

## Recurring passes, durable ownership

Setup explains the runtime's capabilities and records the approved cadence,
five minutes by default, and runner selection. When I delegate the mechanics,
recommend Paseo's same-agent heartbeat for ongoing coordination. It retains one
primary-chat PM's team custody, returning to idle between bounded passes.
A fresh schedule
starts a new PM conversation each pass, but is available only when the deployed
runtime proves stable existing-workspace placement and safe workspace lifetime.
On an incompatible fresh scheduler, recommend the heartbeat and record my
consent or actual delegation; never silently override an explicit choice. If I insist on unsupported fresh mode,
do not activate. The host must be available. A stored job is not proof that
monitoring ran through a host or permissions outage.

Every pass observes the selected backlog, pull requests and checks, relevant
worktrees, live agents, permissions and ownership. It acts on changes, reuses
ongoing work, saves results and questions outside the conversation,
and releases its run claim. A busy pass prevents the next pass from becoming
another controller. There is one logical controller per repository, reconciled
with existing Joe-mode ownership, not one per worktree, tick or backlog slice.

Changes to goals and dependencies can need Chart-a-course, not just another
implementer. New requirements enter the one human Discovery conversation,
then the existing specification and ticket workflows when aligned. A running
agent is not automatically healthy or on course: compare its work with the
accepted assignment and goal, follow up with its owner, and preserve partial
work before any bounded recovery. Do not spawn identical planning on every tick.

One shared Shepherd handles all project PRs. The optional backlog manager owns
Discovery and interactive requirements. Both have their own role heartbeat while
they exist. PM establishes, monitors and removes all role heartbeats. Developers
and short-lived roasters are event-driven. PM coordinates routes; it does not replace their
implementation, nested workers, independent review, verification or approval
gates. Issue-backed recovery comes back through the existing delivery and
controller records, not another orchestrator.

## One conversation with me

Exactly one interactive Discovery agent is reserved per repository. Its
conversation persists while waiting for my answers and alignment; quiet or idle
does not free the lane. Independent noninteractive research can run beside it.
Discovery preserves the full aligned artifact, Specify produces requirements,
and Breakdown Tickets obtains approval before publication. An unaligned recap
does not become permission to execute.

## Keep the work and the UI honest

Developers first rubber-duck blockers. PM delegates a second independent lens,
keeping its own context small. If an answer exists, guide the same developer.
First confirmed work blocker: preserve work, retire the worker and try once
with a fresh context and fresh worktree. Second independent blockage: tag the
issue blocked, comment what answers are needed, return it to backlog, retire
its workers and give the slots to other work. The backlog manager works through
the questions with me; spawn it if missing. Never retry around denied authority.

Push recoverable branches and verify remote commits before removing exact
run-owned worktrees. Keep local copies when preservation fails. Use clear role
and issue names; no stalled idle developers clogging the project.

One Git repository has one Paseo project; each Git worktree has one workspace.
Agents sharing a worktree share its workspace. Separate writing deliveries get
separate worktrees under the same project. Read-only work does not need new
resources merely because another agent does it. Real Git and runtime identities
matter, not display names. Missing fresh-schedule mapping blocks fresh mode;
a heartbeat instead needs verified binding to the actual PM agent in the one
correct existing workspace and my explicit consent. Neither mode permits
another workspace or project every minute.

Health checks distinguish actual errors, cancellation, blocked permissions and
no progress from long tests, review and legitimate human waits. Age or idle
alone does not justify restart. Partial work, descendants and ownership must be
reconciled before replacement or freeing capacity. Repeated failures do not
produce issue or agent storms.

Finished owned agents disappear through supported archival after their results
are preserved and accepted and their responsibilities end or transfer. Needed
Discovery conversations, reviews, repairs and shared monitors stay alive.
Finished PM run agents should not accumulate forever, but archiving a parent
must not break its live children, visibility or reporting. Unsupported retirement
means a concrete retained duty and next action, never fake detachment or altered
runtime identity. Agent archival does not replace the separate safe-cleanup gate.
The heartbeat PM is not finished between passes: it retains its wakeup duty
without creating new PM agents. It retires only after stop/end of all duties,
verified deletion of its owned wakeup and accepted child/result handoff.

## Remain opt-in

Reinvocation inspects the matching configuration and wakeup, not
duplicates. Status is observational. Pause, resume and stop have real verified
wakeup effects and an explicit disposition for active children and preserved
work. Human pause is never automatically undone. Heartbeat pause/stop gates
local dispatch first, then deletes every owned role job; human resume may recreate
it only after old-job absence is proven and the same agent, scope, settings,
workers and fencing remain intact. A paused PM agent stays available for that
human-directed resume, not an automatic tick restart.

Setup verifies the chosen runtime's tools and narrow recurring authority.
Children inherit my current authorized permission mode and features. Verify
actual launch/readback; do not undo Allow All or Auto Accept I selected by
"restoring" a stale default. Missing grants return to me without repeated agents
or permission requests. Inheritance does not authorize broadening my choices.
Declarative choices and private runtime evidence stay separate; private IDs and
secrets do not belong in committed configuration. Setup success requires the
matching chosen-mode job binding and an actual initial observation. Proof of continued
delivery is separate from proof that setup succeeded.
