---
name: wrap-up
description: "Human-invoked checkpoint. After a burst of ad-hoc work, freeze scope, make tests, lint, docs and the visual POC true, true-up with main, and land everything as one green, undrafted PR. Never merges or approves."
disable-model-invocation: true
user-invocable: true
---

# Wrap Up

Turn ad-hoc work into one reviewable checkpoint: nothing new, everything
true, current with `main`, one PR with green CI and ready for human review.

**Entry:** direct human invocation only. That invocation authorizes, for the
current branch only: commits, push, one PR, and marking that PR ready for
review once CI is green. It grants no approval, merge, auto-merge, protection
bypass or history rewrite of shared branches. Never merge.

## Rules

- **Freeze scope.** No new functionality, refactors or polish. Only fixes that
  make existing changes correct, tested, documented and green. Park new ideas
  in the final report.
- **Fix the cause.** Failing tests get the product or test fixed to match
  intended behavior. Never delete, skip or loosen a test to pass. When
  intended behavior is unclear, ask the human.
- **Honest evidence.** Report only runs on the current head. Name anything not run.
- Preserve unrelated changes. Do not stage secrets, logs, `.skill-log/`,
  scratch files or generated output.

## Steps

### 1. Inventory

Read `AGENTS.md`. Record branch, base, `git status`, and the diff against
`origin/main`. Group changes by area (UI, native, docs, tests, skills, config).
List new user-visible behavior; it drives docs and POC work below.

### 2. True-up with main

`git fetch origin`. Merge `origin/main` into the branch (no rebase of a branch
already pushed). Resolve conflicts preserving both intents; ask on semantic
conflicts. Re-run steps 3-4 after any merge that touches code.

### 3. Format and lint

Run, from the repository root, exactly what CI runs for the target. Read the
workflows in `.github/workflows/` and mirror them; for this repo:

```text
npm run format         # then review the diff it produced
npm run format:check
npm run lint           # tsc + clippy -D warnings
```

Fix every warning at its cause. Do not add `allow` attributes or ignore rules.

### 4. Tests

Run the full suites CI runs, not a sample:

```text
npm run test:skills
npm test
npm run test:release
npm run test:settings        # Playwright; long, run it to completion
```

Triage each failure: stale spec (behavior changed on purpose) → update the
spec to the new intent; real regression → fix the code. Re-run the failing
specs, then the full suite once more at the end. A flaky test is rerun in
isolation and reported, not hidden.

### 5. Documentation

Make docs describe what the code does now.

- A `.changes/*.md` entry (never `CHANGELOG.md` where `.changes/` exists; use [changelog](../changelog/SKILL.md)).
- Every doc, README, ADR or spec note that names a changed screen, control,
  label, flow, command, config key or behavior. Search for old strings.
- Skills and `AGENTS.md` when workflow policy changed.
- Docs say what is true, not what was tried. Do not narrate history.

### 6. Visual POC

When user-visible UI changed, update `prototypes/v2/` to match: layout,
controls, copy (or its removal), states, and the POC's own tests and
`README.md` evidence. Run the POC tests. The POC is the design reference;
it must not describe the old UI.

### 7. Commit

Re-run `git status`; stage deliberately. Use terse Conventional Commits per
`docs/agents/commit-style.md`, grouped by intent (for example native fix, UI,
tests, docs, skill). No `Co-authored-by` trailers. Never amend or force-push
history others may hold.

### 8. Pull request

Use [pull-request](../pull-request/SKILL.md) for the body. Reuse an existing PR
for the branch. Push, then create it as **draft**. Read the stored body back.

### 9. CI to green

Watch the checks on the current head (`gh pr checks --watch`). For each
failure read the log, fix the cause, commit, push, watch again. Do not
re-run to mask a real failure; report a confirmed flake with evidence. Stop and
ask if a failure needs a product decision or is outside this branch's changes.

### 10. Ready for review

Only when every required check is green on the current head and the branch is
current with `main`: mark the PR ready (`gh pr ready`). Re-read the PR state.
Do not request, submit or enable approval or auto-merge. Leave merging to the
human.

## Final report

Short, bulleted: PR URL and state, checks (names and result on head), docs and
POC updated, tests changed and why, parked ideas, anything not run.
