# Intent: joe-mode

Joe-mode exists to hand me pull requests to review. It turns the existing
idea-to-ship workflow into a continuous, heavily orchestrated loop, rather than
stopping after recommending a skill, producing a plan, or finishing one phase.

## Anchor the work

Joe-mode can anchor on anything that gives the work a focus: an idea, a folder,
a single issue, a specification, or a backlog. It normally resolves that anchor
to a Git repository, identifies GitHub or Azure DevOps, and uses the provider
and repository configuration to find and manage the relevant backlog.

The selected scope matters. It might be the full backlog for that effort,
only work assigned to me, or a narrower selection. Finding a repository does
not grant permission to take on its entire organization. Ambiguous scope or
identity needs clarification, not a guess.

Readiness comes from the agent-ready label role established when the skills
were set up. Joe-mode uses that configured vocabulary rather than inventing a
new readiness rubric. Dependencies and existing ownership still determine
which ready work can actually run.

## Keep the workflow moving

Once invoked, Joe-mode stays active for the session until I pause or stop it.
There is one active Joe-mode controller per repository, not one per worktree
or backlog slice. It owns backlog management, discovery, implementation, and
delivery, returning to the loop rather than stopping after one issue.
It chooses and triggers the appropriate skills automatically, while preserving
their human decisions and approval boundaries. I should not have to ask for
each routine transition.

Agents work on what is known while another works with me on discovery,
charting the path for more agents. Research and bounded proof-of-concept
experiments inform that discovery. Aligned findings feed domain decisions,
architecture decision records where warranted, specifications, and actionable
backlog items. Those activities are triggered as needed, not left as an
implicit gap between discovery and implementation.

These are concurrent paths, not a global waterfall. One slice can be delivered
while another is being specified and a third is still being explored. Existing
ready work does not wait for the whole problem to be understood. When nothing
is defined, discovery and planning establish the backlog before delivery.
New evidence can send an affected slice backward without stopping unrelated
work.

## Orchestrate toward review

Joe-mode owns navigation, coordination, and the current picture of the work.
Specialist skills and agents own their bounded jobs. It should use substantial
delegation and parallel work where dependencies permit, without competing
coordinators, duplicate deliveries, or agents editing each other's work.
Use Squadron aggressively to give independent agents different work to ship
or shepherd, while keeping one owner for each delivery.

Choose Ship, Patch, or Refactor according to the issue's kind, scope, and size.
Each route carries its work through implementation, verification, independent
review, pull-request publication, and shepherding. Starting Joe-mode authorizes
those routine steps for its selected work without repeated permission prompts.
Bring me reviewed, green pull requests current with latest main or their agreed
target, ready for final sign-off. Distinguish drafts and blocked work from that
handoff, keep branches rebased as the base advances, and route my feedback back
to the same delivery. Final approval and merging remain human-owned.

Keep my attention on discovery, decisions, blockers, and pull requests needing
review. Do not manufacture decisions on my behalf, bury me in routine
coordination, or describe a plan as completed work.
Give a concise Status Report after a full cycle before the next begins, and
after a confirmed major-feature merge. Do not duplicate the same event or reset
the objective's elapsed time merely because a new cycle starts.

When nothing can progress, remain available and explain what is needed rather
than inventing backlog items or spinning an idle loop. Session-long operation
does not mean pretending to run after the session or its agents have stopped.
