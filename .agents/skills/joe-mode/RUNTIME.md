# Runtime and orchestration

Joe-mode targets GitHub Copilot using tools exposed by the current harness. Tool names and capabilities are runtime facts, not assumptions from another agent product.

## Skills and workers

- Use [Doctrine's catalog and metadata selection](../doctrine/SKILL.md) to assign standards without reading full bodies in the orchestrator. Follow [the common packet contract](../doctrine/APPLY.md): preserve scoped operator choices, add required IDs, and send work plus ID/reason/path/digest metadata. The applying sub-agent loads the full text and reports what it actually used.
- Invoke available skills through the harness's skill tool. If a local skill is unregistered and repository instructions permit direct loading, read its local `SKILL.md` and required references; do not substitute an upstream version or search a Claude plugin installation.
- Use the exposed agent-dispatch tool for bounded workers. In a Copilot session exposing `task`, `read_agent`, and `write_agent`, use those tools according to their current schemas. Other installations may expose different names or no worker support.
- Use background agents for genuinely concurrent discovery, planning, and delivery. Coordinate independent work meanwhile; consume notifications rather than polling for reassurance. Resume the known worker for follow-up when supported.
- Respect configured model preferences and runtime defaults. Do not hardcode model IDs, reasoning effort, or unverified context-window sizes from imported skills.
- Track work with session state/todo tools when available, otherwise a unique artifact in the session workspace or OS temporary directory. Do not create `TODO.md` in the repository as a silent fallback.

If required delegation, independent review, or monitoring is unavailable, name the gap and request direction for the affected path. Do not claim heavy orchestration while secretly doing every role inline, install plugins automatically, or fabricate background workers. Other supported work can continue.

## Lifecycle

Load/execute [LIFECYCLE](../squadron/LIFECYCLE.md) for every dispatch, return/
transfer, recovery, terminal retirement; record evidence on this board.
Custody requires receiver observation/acknowledgment, not launch/send success or idle.

One runtime may host explicit PR assignments. Share execution, not human intent;
never silently reassign. Preserve each PR's Shepherd cadence/sole owner. Merge/closure ends
only that scope; retain idle heartbeat waiters/other active PR owners. After
acceptance/preservation and safe transfer, actually retire terminal owned agents,
including analysis/implementation workers. Record concrete retention reasons/
archive limits, never indefinite "remain available" defaults.

For PR custody, load [OBSERVATION](../shepherd/OBSERVATION.md); use authorized
scheduler, preferably same-agent heartbeat. Preserve per-PR adaptive streak/due
time; fairly service all due PRs. Fresh runs require verified durable ownership/
serialization and correct existing workspace binding—not guessed create arguments
or controllers per tick. Persist desired/observed cadence and exact job IDs here.

Keep the controller's human-facing conversation available while workers run. Queue worker questions with their owner and affected scope. Only actual human responses clear human-decision gates.

Use the harness's documented notification/wait contract. Some runtimes wake a controller on completion; others require an explicit event wait. Do not copy `Task`, `TodoWrite`, `/clear`, `/compact`, or another runtime's wait syntax into a tool call unless that interface actually exists.

Joe-mode is session-long, not an installed service. A board on disk does not schedule work. After cancellation/runtime loss, use LIFECYCLE recovery: report the observation gap, inspect surviving owners/children, PRs and partial work, and explicitly restart only confirmed missing ownership. Persistent services require separate authorization and verified runtime support.

For issue-backed requests, load [RECOVERY](../shepherd/RECOVERY.md); consume this
board's pending episode, acknowledge actual observed intake. If this controller is absent, wake/recover
only this previously human-authorized controller under recorded grant, after
reconciling surviving agents/jobs/partial work. Missing grant or uncertain/duplicate
ownership blocks dispatch, never permits fresh broad Joe-mode. Preserve root human
conversation/stop gates; issues grant no startup authority.

## Isolation and shared resources

Give each independent writer its own authorized Git worktree. Follow
[WORKSPACE](../ship/WORKSPACE.md) for placement: with Paseo, one repository
project and one workspace per worktree; agents sharing a worktree reuse its
registration. Read-only dispatch alone creates no new worktree/project/workspace.
Discovery/research sources stay read-only; POC writes stay in its agreed scratch
environment. Domain/ADR writers must not edit an active implementer's checkout.

The selected Ship, Patch, or Refactor owner owns its delivery branch, integration queue, and nested workers. Joe-mode uses Squadron for distinct assignments and owns their non-overlapping coverage, not cherry-picks into their branches. Transfer artifacts and permissions through the owner, with one writer/integrator per shared mutable target. Serialize Changelog updates through that integrator.

Read actual repository worktree guidance before creating any workspace. An existing linked worktree is not automatically safe for several writers. Retire agents only after LIFECYCLE's terminal checks; agent archival never substitutes for separately authorized branch/worktree/workspace cleanup.

Every PR-producing lane requires `worktrees`, including domain/ADR and documentation deliveries. Route workspace operations to the owning workflow's [workspace procedure](../ship/WORKSPACE.md); doctrine selection does not create a workspace or authorize publication.

## Copilot-specific restraint

Do not create `.claude/`, `CLAUDE.md`, Claude hooks, or Claude permission configuration to make Joe-mode work. Honor any repository guidance already present, but do not privilege Claude files over the instructions the current Copilot harness actually supplies.

Provider access uses the configured GitHub CLI or Azure DevOps integration and authenticated identity. A Copilot subscription, Git author email, or an available shell is not proof of tracker permissions.

Give all commit-producing workers the [shared commit-message policy](../setup/COMMIT-STYLE.md). It applies independently of chat style, respects repository/operator requirements, and authorizes no additional Git actions. Keep it with the library; do not install it into global Copilot configuration as a side effect.

Follow [invocation and communication contracts](../setup/INVOCATION.md). Prefer
terse exact worker messages without changing the human's chat mode. Caller
restrictions still apply when the runtime ignores invocation metadata.

Preserve objective-start evidence, parent/descendant ownership, cycle state,
and deduplicated report events in existing session storage. Status Report uses
available runtime events; missing objective timing/tool counts stay unavailable.
Do not install a recorder or infer a complete fleet from a partial tool view.
