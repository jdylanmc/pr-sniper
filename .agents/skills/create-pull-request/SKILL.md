---
name: create-pull-request
description: "Agent-invocable fallback for creating a pull request when no repository-specific create-PR skill or active delivery workflow owns publication. Prefer the repository's own PR skill when one exists."
disable-model-invocation: false
user-invocable: true
---

# Create Pull Request

**Entry:** Human or scoped agent use. Create or update one pull request (PR) only when no repository-specific PR skill, explicit publication workflow, or active delivery owner already owns that action. Follow the [invocation contract](../setup/INVOCATION.md).

This is the generic fallback, not the highest-priority route.

## 1. Resolve the owning route

Before acting, inspect the repository guidance and available skills:

1. Use an explicitly requested PR workflow.
2. Otherwise use a repository-specific create-PR skill or documented publication procedure.
3. Otherwise return publication to the active Ship, Patch, Refactor, Shepherd, release, or equivalent delivery owner.
4. Use this skill only when none of those applies.

Do not run two publication workflows, take over an owned branch, or use this fallback to bypass a repository's review, template, provider, or approval rules. If ownership is unclear, stop for clarification.

Creating a PR authorizes only the requested publication work. It does not authorize implementation, unrelated fixes, tracker changes, approval, auto-merge, merging, branch deletion, or release actions.

## 2. Inspect the candidate

Read repository guidance, the target branch, remote/provider configuration, PR templates, changed commits and files, current status, and available validation evidence. Resolve the actual source and target refs; do not assume `main`, `origin`, GitHub, or same-repository publication.

Before preparing PR changes, preserve any caller doctrine selection and require `worktrees` under [Doctrine](../doctrine/APPLY.md). Use the existing owned isolated workspace. Do not relocate, discard, stash, reset, or overwrite unrelated work.

Check for an existing open PR from the source branch or for the same delivery. Reuse and update the matching PR. Clarify multiple plausible matches. After an uncertain create/update result, query provider state before retrying.

If the branch has no meaningful committed change, report that no PR can be created rather than manufacturing an empty commit. If commits, push access, required validation, or the provider destination are missing, report the exact blocker.

## 3. Prepare the PR body

Follow the repository template when present. Otherwise use this compact structure:

```markdown
## Summary

<short explanation plus the smallest useful diagram, diff sketch, call tree, or file tree>

## Evidence

- **Before:** <prior behavior, failing check, screenshot, or not applicable with reason>
- **After:** <current behavior, passing check, screenshot, or unavailable with reason>

## Merge Danger

**Door:** <one-way or two-way>

**Blast Radius:** <concise affected surface>

<rollback limits, migration needs, or material ramifications>
```

Keep prose brief and use the repository's domain language. Include linked issue or requirement references, user-visible behavior, validation commands/results, known limitations, and follow-up work when relevant. Never claim checks ran, screenshots exist, or requirements are satisfied without evidence.

Choose the smallest visual that clarifies the change:

- pseudocode for logic;
- a call tree for runtime flow;
- a component tree for user interface structure;
- a shallow file tree for responsibility changes;
- a compact `diff` sketch for before/after shape;
- Mermaid only when relationships are otherwise unclear.

Use no visual when plain text is clearer. Do not overwhelm the PR with every possible representation.

For **Merge Danger**, call a reversible, cheaply backed-out change a two-way door. Use one-way door only for destructive, externally committed, irreversible, or migration-heavy changes. Describe realistic blast radius rather than minimizing risk.

## 4. Publish and verify

Honor an explicit draft/ready request. Otherwise follow repository convention; when no convention or completion evidence exists, create a draft.

Use the configured provider:

- GitHub: use the available GitHub integration or `gh` with an explicit repository.
- Azure DevOps: use the configured integration and full source/target refs.
- Other providers: use their supported equivalent or report missing capability.

Push only the owned source branch to its resolved remote. Never force-push unless the caller already owns a separately authorized history rewrite with an exact expected-head lease.

Create or update the PR with the prepared title/body and correct base/head. Then read it back and report:

- URL and provider;
- draft/ready state;
- source and target refs;
- observed head commit;
- validation evidence included;
- unresolved blockers or human decisions.

Provider request success is not verification. If readback differs from the requested state, report the actual state. Stop after publication; do not approve, merge, enable auto-merge, or invent ongoing monitoring. A separately authorized Shepherd or repository workflow owns continued maintenance.

## Attribution

Adapted from Matt Pocock's MIT-licensed
[`pr`](https://github.com/mattpocock/skills/blob/main/skills/engineering/pr/SKILL.md)
skill, which credits Dex Horthy and HumanLayer's MIT-licensed
[`show-me`](https://github.com/humanlayer/skills/blob/main/plugins/show-me/skills/show-me/SKILL.md)
skill for the concise visual-summary approach. See the bundled
[third-party notice](../setup/NOTICE.md),
[Matt Pocock license](../setup/licenses/mattpocock-skills.LICENSE), and
[HumanLayer license](../setup/licenses/humanlayer-skills.LICENSE).
