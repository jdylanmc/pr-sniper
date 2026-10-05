# Delivery workspace

The PR-producing owner executes this procedure within its existing authority after loading `worktrees` through [Doctrine](../doctrine/SKILL.md). It replaces the standalone worktree skill, not the caller's approval, setup, or publication rules.

## Inspect and reuse

Read repository/harness guidance and inspect actual Git state:

```sh
git status --short --branch
git worktree list --porcelain
git rev-parse --path-format=absolute --git-dir --git-common-dir
git rev-parse --show-superproject-working-tree
```

Resolve the target repository, delivery, base, current branch, path, and owner. Different Git/common directories alone do not establish safe isolation; submodules and harness-managed workspaces need interpretation. A directory name is not evidence of isolation.

Reuse an existing worktree only when it belongs to this delivery, has no competing writer, preserves unrelated changes, and is not the default-branch workspace. Do not create nested worktrees merely because another skill was invoked. A detached harness-managed workspace needs an agreed delivery branch before committing/publishing; never repurpose another owner's branch.

## Create only when needed

Use an available harness-native worktree mechanism when it provides the required isolation and ownership. Otherwise use `git worktree add` with an explicit approved path, delivery branch, and base. Follow platform/repository placement rules; ask for unresolved decisions rather than inventing a global layout.

For a repository-local worktree directory, check that exact destination with `git check-ignore` before creating it. Do not test some other candidate directory. If it is not ignored, propose an authorized ignore change or an external destination; do not automatically edit `.gitignore` and commit it.

Inspect and report creation failures. Do not fall back silently to the user's main checkout or share a live worker's directory. Obtain direction when the required workspace cannot be established.

## Establish evidence and custody

Record path, branch, actual base/start commit, owner, and relevant pre-existing changes/failures. Run the smallest repository-required baseline checks for the planned work, not a guessed full suite. Use existing environment setup; install dependencies only when separately warranted and authorized, never merely because a worktree is new.

Preserve baseline failures and missing checks in the report; neither is a clean baseline. The owner resolves their impact before making completion claims.

Independent write deliveries need distinct Git worktrees, not branches/UI entries.
Serialize integration/shared resources; read-only agents may share sources without new
worktrees. Record placement/custody under [lifecycle contract](../squadron/LIFECYCLE.md).

## Paseo placement, when used

Use exactly **one Paseo project per Git repository and one Paseo workspace per
Git worktree**. Same-worktree agents share registration; independent writers
use distinct worktrees/workspaces under that project. New agents alone require
no project/workspace/worktree.

Resolve repository identity/common directory, actual worktree paths, existing
registrations; reuse compatible ones. Distinguish UI project/workspace, Git
repository/common directory, branch, mutable state. Before creation, resolve
ambiguous/duplicate mappings with owner; never force consistency by deleting registrations.

Inspect schemas. Needed worktree/workspace creation always specifies existing
repository `projectId`; establish one only if absent and authorized.
Native creation must honor approved layout/ownership. Otherwise create Git
worktree above, register explicit path:
`create_workspace({isolation: 'local', path, projectId, title})`.
Result may report `isolation: 'worktree'`; before use verify actual path,
project/workspace IDs, Git worktree/branch.

Place agents with verified `workspaceId` in `create_agent`. No invented cwd argument
or assumed controller-path inheritance: each inspects actual cwd/Git paths/branch/
starting state. Mismatch/missing mapping capability blocks writes, never permits
main, project-per-worker, or permission widening. Deliberate inheritance of
human-selected Allow All is not a placement workaround; follow the actual
[permission contract](../joe-mode-paseo/RUNTIME.md#permission-preserving-dispatch).

## Preserve resources at retirement

Agent archival follows LIFECYCLE, not Git/UI cleanup. Before separately authorized
worktree removal, verify run ownership, integration, no live writer, no uncommitted/
unpreserved work. Remove only specific completed run-owned worker worktrees.
Keep delivery workspace while PR/Shepherd needs it; never archive projects/workspaces
or delete branches/worktrees merely to clear finished agents.

For Joe's explicitly authorized blocked-work cleanup, verified recoverable
remote branches replace the integration prerequisite, not the preservation
checks. Follow [TEAM](../joe-mode-paseo/TEAM.md): stop writers, inspect tracked,
untracked and ignored files, preserve safe work/evidence, verify exact remote
commits, then remove only the owned worktree. Push failure or unpreserved data
means keep the local copy. Never delete the only copy or a live PR repair workspace.
