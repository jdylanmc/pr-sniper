---
name: setup
description: "Human-directed repository setup for GitHub, Azure DevOps, or local Markdown. Human-started Joe-mode may bootstrap missing/incomplete setup under one owner; human choices and exact-file approval remain required."
disable-model-invocation: false
user-invocable: true
---

# Setup

**Entry:** Human-directed, with one named automatic caller: the human-started Joe-mode controller bootstrapping absent/incomplete repository setup after establishing repository-wide ownership. Follow the [invocation contract](INVOCATION.md). Model-loadable metadata permits that subflow, not unrelated autonomous callers.

Accept a direct human request or that bounded Joe-mode bootstrap only. Other
callers report what is missing and tell the human to invoke `/setup`, then wait.
Either permitted entry authorizes exploration and proposals, not provider/label
choices on the human's behalf or bypassing exact-file write confirmation.

Human activation of [joe-mode-paseo](../joe-mode-paseo/SKILL.md) can carry
that direct human setup request here, preserving its repository/Setup owner,
settled choices and exact-file approval. Its scheduled RUN is **not** a bootstrap
caller: missing configuration goes back to the human setup owner. Installation
of that adapter does not activate Setup or a schedule.

Reconcile repository identity and any active Setup owner before starting, across
worktrees/sessions. Join or resume that owner rather than creating a duplicate;
uncertain ownership blocks a new run. For Joe-mode, retain its controller,
original anchor/exclusions, objective-start evidence, configuration gaps, and
return destination. Setup is not a new Joe controller or delivery owner.

Use [doctrine selection and application](../doctrine/APPLY.md) within this setup's existing approval gates. If these configuration changes will be delivered in a PR, require `worktrees` before preparing them. No doctrine selection authorizes setup writes or changes to global instructions.

Scaffold the per-repo configuration that the engineering skills assume:

- **Issue tracker**: where issues live (GitHub, Azure DevOps, or local Markdown only)
- **Triage labels**: the strings used for the five canonical triage roles
- **Domain docs**: where `CONTEXT.md` and ADRs live, and the consumer rules for reading them

This is a prompt-driven skill, not a deterministic script. Explore, present what you found, confirm with the user, then write.

The full skill pack also carries its shared [invocation policy](INVOCATION.md),
[commit policy](COMMIT-STYLE.md), [notices and licenses](NOTICE.md), and
[original import provenance](provenance/skills-lock.json) in this package.
Installing those resources does not run this workflow or authorize configuration
writes. Keep all sibling packages together; this support is not a separate skill.

## Joe-mode bootstrap readiness

Inspect the repository's actual guidance and referenced configuration, including
explicitly configured equivalent locations. Completeness means usable content
for the existing Setup outputs, not matching seeds or finding files on disk:

- Issue-tracker guidance identifies a supported provider and concrete repository,
  planning scope, or local paths. Joe's selected backlog/default view and
  authenticated-identity method are defined where applicable, along with
  provider-required type/state/relation conventions.
- Triage guidance maps all five canonical roles to the provider's representation,
  including a usable `ready-for-agent` value. Custom human vocabulary is valid.
- Domain guidance defines the chosen layout and consumer rules; repository
  instructions point to the tracker, triage, domain, commit-style policy, and
  doctrine guidance described in steps 3–4. Preserve explicit repository overrides.
  An honestly recorded unavailable doctrine package is a downstream capability
  blocker, not a reason to repeat Setup. Actual `CONTEXT.md`/ADR content is created
  lazily and its absence does not make setup incomplete.

Missing outputs, blank mappings, or unresolved template placeholders are gaps.
Existing unsupported providers or conflicting/ambiguous human choices are instead
decision blockers: leave them intact and seek a human decision, never reset them
as "not set up." Tracker authentication/access failures are capability blockers,
not missing setup. No setup marker, new schema, or live backlog mutation is needed.

For the Joe bootstrap, return without changes if already complete. Otherwise
propose only the missing/incomplete portions, preserving settled human choices.
No automatic migration, global changes, issue creation, assignment, or readiness
updates follow from bootstrap authorization.

## Process

### 1. Explore

Look at the current repo to understand its starting state. Read whatever exists; don't assume:

- `git remote -v` and `.git/config`: which provider, code repository, and project does the remote identify? Is a separate planning backlog already configured?
- The current harness's repository guidance, including `AGENTS.md` for Copilot: is there already an `## Agent skills` section? Honor other instruction files when the harness or repository explicitly references them; do not assume Claude.
- `CONTEXT.md` and `CONTEXT-MAP.md` at the repo root
- `docs/adr/` and any `src/*/docs/adr/` directories
- `docs/agents/`: does this skill's prior output already exist?
- `.scratch/`: a sign that a local-markdown issue tracker convention is already in use
- Is `triage` or `joe-mode` installed? Either needs the configured readiness-role mapping in Section B.
- Is the complete `doctrine` package available to this repository and its workers? Record its actual location and any existing repository-required IDs; do not read all bodies merely to inspect the catalog.
- Monorepo signals: a `pnpm-workspace.yaml`, a `workspaces` field in `package.json`, or a populated `packages/*` with its own `src/`. These are present only in a genuinely large multi-package repo; their absence means single-context, which is almost every repo.

### 2. Present findings and ask

Summarise what's present and what's missing. Then take the sections in order. One section, one answer, then the next.

Lead each section with the recommended answer so the user can accept it in a word. Give a one-line explainer only when the choice genuinely branches; skip the section entirely when exploration already settled it (Section B when neither `triage` nor `joe-mode` is installed, Section C when there's no monorepo). During a Joe bootstrap, reuse settled provider/layout/label choices; ask only for missing decisions, never reconfirm defaults as a reason to overwrite custom values.

**Section A: Issue tracker.**

> Explainer: The "issue tracker" is where issues live for this repo. Skills like `breakdown-tickets`, `triage`, and `specify` read from and write to it. They need to know whether to use GitHub Issues, Azure DevOps work items, or local Markdown files. Pick the supported place you actually track work for this repo.

Propose a supported provider identified by the configured remote, not GitHub regardless of evidence. Detection identifies a candidate code location, not planning scope or organization-wide authority. Confirm ambiguous remotes and code-versus-planning locations. If the remote points at Azure DevOps, use the [Azure DevOps template](issue-tracker-azure-devops.md) to resolve its organization/project/repository and separately confirm the planning scope. Offer only:

- **GitHub**: issues live in the repo's GitHub Issues (uses the `gh` CLI)
- **Azure DevOps**: work items live in a configured planning project/backlog; code may live in another project (uses the available Azure DevOps integration)
- **Local markdown**: issues live as files under `.scratch/<feature>/` in this repo (good for solo projects or repos without a remote)

If existing human configuration uses an unsupported tracker, state that it is unsupported and ask the human to choose one of these supported destinations. Leave that configuration and its work untouched until the human approves the exact replacement. Do not silently translate, migrate, or discard an existing backlog. An unsupported code remote does not select a planning provider or confer PR capabilities.

Record the choice in `docs/agents/issue-tracker.md`, including the actual host/repository or planning scope, or the confirmed local paths (the seed defaults are not mandatory). The GitHub template carries a "PRs as a request surface" flag, defaulted **off**. Leave it off and don't raise it: a user who wants external PRs in the triage queue can flip the flag in the file later.

For Joe-mode, record the selected repository/project/area or saved query, how authenticated identity is resolved, and whether the default view is that full selected backlog or assigned-to-me. An explicit invocation can narrow that selection. Do not silently broaden an issue/folder anchor to the full organization. Record work-item type/state and relation conventions when the provider requires them, plus any existing shared claim convention. Setup does not assign work or grant a new blanket mutation permission.

**Section B: Triage label vocabulary.** Skip only when neither `triage` nor `joe-mode` is installed.

When either skill is installed and no vocabulary is already settled, ask exactly one question:

> Do you want to keep the default triage labels? (recommended: **yes**)

The defaults are the five canonical roles, each label string equal to its name: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. On **yes**, write them as-is. Only if the user says no, usually because their tracker already uses other names (e.g. `bug:triage` for `needs-triage`), collect the overrides so `triage` applies existing labels instead of creating duplicates.

For a partial existing mapping, preserve its values and ask for the missing roles
instead of replacing the table with defaults. All writes still wait for step 3.

For Azure DevOps, record the corresponding tag values or explicitly configured field representation; do not replace workflow states with these role names. On GitHub they are labels; local trackers record the mapped value in the agreed status convention.

**Section C: Domain docs.** Default to **single-context** (one `CONTEXT.md` + `docs/adr/` at the repo root). This fits almost every repo; write it without asking.

Offer **multi-context** (a root `CONTEXT-MAP.md` pointing to per-context `CONTEXT.md` files) only when exploration found monorepo signals. Then confirm which layout they want.

### 3. Confirm and edit

Show the user a draft of:

- The `## Agent skills` block for the actual repository instructions used by the harness (see step 4)
- The contents of `docs/agents/issue-tracker.md`, `docs/agents/domain.md`, and `docs/agents/triage-labels.md` (the last when `triage` or `joe-mode` is installed)
- A repository-local `docs/agents/commit-style.md` copy of the [shared commit-message policy](COMMIT-STYLE.md), retaining applicable attribution/license and the target repository's explicit overrides
- A doctrine guidance subsection pointing to the actual installed package, listing any operator-confirmed repository-required IDs, and explaining scoped worker selections. Do not copy/rewrite doctrine sources or install a missing package as a setup side effect.

Let them edit, then obtain explicit approval of the exact proposed files and changes before writing. Existing configuration is human-owned; never overwrite it merely to match a seed.

Read the shared policy before proposing its copy. If the library was copied without that dependency, obtain its location rather than inventing an equivalent or reinstalling the retired formatter. Reuse or reconcile an existing repository policy; do not overwrite human customizations. This setup never edits global Copilot instructions.

Make the proposed local copy self-contained: read the policy's attribution and applicable license sources, include the required notices in the copied document, and replace library-relative references that would not resolve in the target repository. Missing license sources block distributing the copy; ask for their location rather than dropping attribution.

### 4. Write

**Pick the file to edit:**

- For Copilot, use the repository's existing `AGENTS.md` or the instruction file explicitly supplied by its current setup.
- Do not choose `CLAUDE.md` merely because it exists, or create Claude configuration for a Copilot workflow.
- If the intended instruction destination is absent or ambiguous, propose the appropriate file and obtain confirmation before creating or editing it.

If an `## Agent skills` block already exists in the chosen file, update its contents in-place rather than appending a duplicate. Don't overwrite user edits to the surrounding sections.

The block:

```markdown
## Agent skills

### Issue tracker

[one-line summary of where issues are tracked]. See `docs/agents/issue-tracker.md`.

### Triage labels

[one-line summary of the label vocabulary]. See `docs/agents/triage-labels.md`.

### Domain docs

[one-line summary of layout: "single-context" or "multi-context"]. See `docs/agents/domain.md`.

### Commit messages

Use the library's terse Conventional Commits default in `docs/agents/commit-style.md`, subject to this repository's explicit conventions and required trailers. This formatting policy grants no staging, commit, or history-rewrite authority.

### Doctrine

Use `/doctrine` at <verified package location>: no arguments lists catalog metadata; named IDs retrieve verified text; orchestrators send scoped selection packets and workers load their assigned doctrines. Repository-required IDs: <confirmed IDs or none>. Code Roast requires `solid`; PR-producing workflows require `worktrees`. Preserve operator choices for the named delivery without applying them to unrelated work.
```

Resolve the doctrine placeholders before writing. If the package is unavailable, record that limitation and the needed installation/location instead of claiming doctrine loading is configured. Keep its bundled manifest, sources, and helper together; never change global configuration to make the path work.

Include the `### Triage labels` sub-block, and write `docs/agents/triage-labels.md`, when Section B ran for `triage` or `joe-mode`. Otherwise omit both.

Then write the docs files using the seed templates in this skill folder as a starting point:

- [issue-tracker-github.md](./issue-tracker-github.md): GitHub issue tracker
- [issue-tracker-azure-devops.md](./issue-tracker-azure-devops.md): Azure DevOps work items, planning/code scopes, readiness tags, and PR operations
- [issue-tracker-local.md](./issue-tracker-local.md): local-markdown issue tracker
- [triage-labels.md](./triage-labels.md): label mapping (when `triage` or `joe-mode` is installed)
- [domain.md](./domain.md): domain doc consumer rules + layout
- [shared commit-message policy](COMMIT-STYLE.md): reviewed local commit-style guidance; preserve its attribution/license and approved repository-specific differences

For the authorized repository changes, use [Changelog](../changelog/SKILL.md) for the correct repository/component: curate notable `Unreleased` entries in Keep a Changelog 1.1.0 format, not a commit dump, version bump, or release. Include any proposed entry in the write approval; no notable change may mean no entry needed. A changelog-only edit does not generate another entry. Do not write a changelog during read-only setup exploration.

### 5. Done

Read back the approved outputs before reporting completion. Tell the user which
engineering skills will read them and that human-owned configuration can be
edited directly later; rerunning Setup is optional for repairing gaps or changing
choices, not required merely because Joe-mode starts again.

For a Joe bootstrap, return to the same controller with completed/reused,
waiting-for-human, or blocked status, the actual files/workspace and remaining
gaps, decisions, and Setup ownership. Human unavailability, refusal, failed
invocation, or failed/partial writes are not completion: report what is needed,
preserve existing configuration and any actual partial result, and wait without
an automatic retry loop. Joe re-reads the configuration before resuming its
original anchor, readiness mapping, and objective timing; Setup never marks
backlog items ready or widens that scope.
