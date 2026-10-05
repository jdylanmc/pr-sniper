---
name: worktrees
description: "Prepare pull requests in owned, isolated worktrees without disturbing another delivery or the default checkout."
scope: shared-engineering-doctrine
---

# Worktree Doctrine

## Prime directive

Every pull request starts from an isolated, owned workspace. Documentation and
specification changes deserve the same protection as product code.

## Position

A branch name separates history, not concurrent working state. Agents sharing
a checkout also share its index, uncommitted changes, and accidental consequences.
Isolation makes ownership visible and keeps one delivery from rewriting another.

An existing appropriate worktree is already useful isolation. Creating another
one by ritual adds confusion rather than safety.

## Principles

- **Inspect before creating.** Establish the repository, worktree, branch, base,
  and current owner. Account for submodules and harness-managed workspaces.
  Do not infer isolation from a directory name or a single Git signal.
- **Reuse only compatible ownership.** Reuse an isolated workspace for the same
  delivery when no competing writer owns it. Do not share a mutable checkout
  between independent writing agents or repurpose the default-branch workspace.
- **Respect the environment.** Prefer an available harness-native mechanism
  when it supplies the needed isolation; use Git worktrees otherwise. Honor
  approved placement and verify the actual destination is excluded from source
  control when it lives inside the repository.
- **Keep failure visible.** Failure to establish isolation is a reason to seek
  direction, not permission to work in the user's main checkout. A fresh
  directory is not proof of a clean baseline or working dependencies.
- **Preserve custody through delivery.** Record the workspace owner and starting
  state. Serialize integration, retain unrelated changes, and keep the delivery
  worktree available while its pull request needs maintenance.
- **Clean up only what is finished and owned.** Confirm integration, absence of
  active writers, and preservation of uncommitted work before removing a
  run-owned worker worktree.

## Boundary

Isolation does not authorize dependency installation, ignore-file edits,
commits, history rewriting, publication, or merging. The calling workflow owns
those permissions and the human owns exceptions. Do not turn cleanup into
deleting another person's work.
