# Controller and role worktrees

Joe-mode and Joe-mode CMUX use this placement contract. It specializes the
shared team defaults; a Git worktree is not a CMUX pane or workspace.

| Role | Git placement |
| --- | --- |
| Project Manager / orchestrator | The repository's owned `main` checkout |
| Discovery agent | Dedicated worktree on `discovery/<feat>` |
| Authorized PR/auto-merge coordinator | Dedicated `pr-sniper` worktree and branch |
| Implementation or PR repair owner | Its own delivery worktree and branch |

## Keep the orchestrator on main

Once a repository is resolved, verify the Project Manager's actual working
directory, branch, common Git directory, and existing worktree ownership.
Use the existing owned `main` checkout. If `main` is already checked out
elsewhere, reconcile that checkout rather than forcing a second checkout or
repurposing a worker branch.

The orchestrator does not implement features, resolve delivery conflicts, or
commit worker changes on `main`. Keep its board in session/private state.
If the harness cannot use the verified main checkout, report that placement
blocker; do not claim that a UI title or a command's `-C` argument moved the
conversation. Preserve the current human conversation and any uncommitted work
when arranging a supported working-directory transition.

At activation, before a new dispatch pass, and after each confirmed merge,
fetch the resolved repository remote's `main` and compare its tip to local
`main`. Observe again after a merge even if the last fetch was recent.

For a verified `origin` remote, the ordinary update is:

```sh
git -C "$MAIN_WORKTREE" fetch origin main
git -C "$MAIN_WORKTREE" merge --ff-only refs/remotes/origin/main
```

Run the update only after checking that the checkout is on `main`, clean
(including untracked work), has no active Git operation or competing writer,
and that local `main` is an ancestor of the fetched remote tip. Use the
actual configured remote, not an assumed `origin`.

If tips match, no update is needed. If local main is ahead or diverged, dirty,
has an operation in progress, or remote state cannot be observed, preserve it
and report the blocker. Never reset, stash, rebase, force-checkout, or create
a reconciliation merge on main to hide the problem.

Verify branch and resulting tip after the fast-forward and record the fetched
base on the board. New workers start from that observed main. Existing delivery
owners refresh their own branches under their delivery/Shepherd contract; the
Project Manager must not rebase another active writer's worktree. Never
describe an older worker base as current simply because main advanced.

## Discovery placement

Every repository-backed Discovery agent works on `discovery/<feat>`, even
when its current pass is read-only. `<feat>` is the recorded feature/anchor
slug. Resolve a platform-appropriate isolated path using
[delivery workspace guidance](../ship/WORKSPACE.md); do not create it inside
another live writing checkout.

Reuse the same feature's owned Discovery worktree and conversation across
passes. Different Discovery owners need distinct slugs/worktrees; never put
two writers on the same discovery branch or move them back onto main.
Read-only research helpers are not automatically new Discovery owners.
Preserve the shared team's single human-facing inquiry lane unless the human
explicitly requests independent Discovery lanes.

Discovery may begin without a repository; establish this placement when the
repository becomes known. Do not invent a Git repository merely for research.

## PR coordinator placement

When the human explicitly authorizes a PR coordinator or auto-merge role, use
its own `pr-sniper` branch/worktree, distinct from main, Discovery, and delivery
worktrees. Reconcile any existing owner before reuse; the name is not a lock.

This worktree holds the coordinator's review/merge activity, not a substitute
for each PR's isolated branch. Functional repairs return to the delivery owner.
An authorized merge is performed through the verified provider target, never by
merging all feature branches into `pr-sniper` and pushing that branch to main.

Placement grants no merge permission. Preserve the explicitly authorized
repository merge gate, independent review, exact-head validation, and
concurrency checks. After a confirmed merge, notify the Project Manager to
advance its main checkout using the guarded procedure above. Without merge
authority, leave final merging to the human.

## CMUX adaptation

Pass each worker's exact Git worktree through the active adapter's public
launch interface; the CMUX adapter supplies native launch `cwd`.
All cockpit roles remain in the existing CMUX workspace; a Discovery Git
worktree does not mean a second CMUX workspace. Keep the Project Manager's
existing surface, with its working directory verified as main. Native messaging
does not change Git placement or grant branch/lifecycle ownership.
