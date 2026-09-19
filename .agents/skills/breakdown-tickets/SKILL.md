---
name: breakdown-tickets
description: "Human or scoped agent use after Specify. Break a full requirements specification into actionable tickets; obtain human approval before publication."
disable-model-invocation: false
user-invocable: true
---

# Breakdown Tickets

**Entry:** Human or scoped agent use after Specify, with human approval before publication. Follow the [invocation contract](../setup/INVOCATION.md).

Use [doctrine selection and application](../doctrine/APPLY.md), preserving the parent deliverable's choices. With none, consider `sequencing` and `documentation` for dependency-aware, durable work packets. Carry scoped selections and required IDs into implementation handoffs. A separately authorized PR for local ticket files requires `worktrees`; tracker publication alone does not.

After [Specify](../specify/SKILL.md), break its completed requirements specification into **tickets**: tracer-bullet vertical slices, each declaring the tickets that **block** it. Human or coordinating-agent invocation both require human approval before publication. A raw conversation, exploratory plan, or unfinished Discovery artifact is not a requirements specification.

Read the configured tracker and triage vocabulary. Support only GitHub, Azure DevOps, and local Markdown. For missing or unsupported configuration, report it and direct the human to `/setup` to choose a supported destination. Do not automatically run Setup or replace existing configuration.

## Process

### 1. Gather context

Require the completed specification as an accessible artifact or tracker item. Read its full body, relevant comments and linked requirements, including approved full-spec attachments; record the canonical source, revision, and owner. Confirm acceptance criteria, non-goals, agreed testing expectations, and unresolved decisions. Return gaps in incomplete or unaligned requirements to Specify's owner; do not invent requirements or start a discovery interview.

Carry existing acceptance IDs exactly into slices. If absent, cite exact source criteria/section anchors and request stable IDs from the specification owner when needed; do not fabricate IDs or quietly edit the spec.

### 2. Explore the codebase (optional)

If not already explored, inspect the codebase's current state. Ticket titles and descriptions should use the project's domain glossary vocabulary and respect ADRs for the affected area.

Look for prefactoring opportunities to ease implementation. "Make the change easy, then make the easy change."

### 3. Draft vertical slices

Break the work into **tracer bullet** tickets.

<vertical-slice-rules>

- Each slice cuts a narrow but COMPLETE path through the relevant layers (for example schema, API, UI, tests): vertical, NOT a horizontal slice of one layer
- A completed slice is demoable or verifiable on its own
- Each slice is sized to fit in a single fresh context window
- Propose prefactoring first only when evidence shows it enables the accepted outcome; keep its verification and scope explicit

</vertical-slice-rules>

Give each ticket its **blocking edges**: tickets that must complete before it can start. No blockers means dependency-unblocked, not automatically ready or authorized to start.

Tie every slice to real specification acceptance criteria and agreed verification. Record only evidenced blocking edges; distinguish true prerequisites from preferred work order. Check coverage, cycles, and availability on each delivery's base. Unknown dependency identities remain unresolved references, not invented issue numbers.

**Wide mechanical refactors may need a coherent batch instead of vertical slices.** Do not split independently unshippable pieces into separate deliveries. An ordinary coordinated refactor alone does not justify compatibility scaffolding. Consider internal [Migration](../migration/SKILL.md) only with evidence of production use and an actual migration need (such as persisted data, deployed consumers, or rollout overlap). Pre-1.0 prototypes normally skip that ceremony; versions alone do not prove absence of users/data. Ask the owner about unknown production or migration need. If eligible, propose safe stages and real dependencies; later contraction requires agreed scope. Otherwise keep the ordinary change coherent and green without speculative expand–contract layers.

### 4. Quiz the user

Present a numbered breakdown. For each ticket, show:

- **Title**: short descriptive name
- **Blocked by**: which other tickets (if any) must complete first
- **What it delivers**: the end-to-end behaviour this ticket makes work
- **Acceptance and verification**: exact source criteria/IDs covered and the agreed proof
- **Readiness and gaps**: whether it is fully specified, any human-owned decisions, and genuine prerequisites
- **Delivery grouping and assignment**: one specification-owned PR or intentionally separate deliveries; include the proposed assignee policy when Joe-mode is scoped to assigned-to-me work

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the blocking edges correct: does each ticket only depend on tickets that genuinely gate it?
- Should any tickets be merged or split further?

Iterate until the user approves the breakdown.

### 5. Publish the tickets to the configured tracker

Publish approved tickets to the tracker `/setup` configured. Tickets stay the same; only blocking-edge representation changes:

- **Local Markdown** → write one file per ticket at configured paths (default `.scratch/<feature-slug>/issues/<NN>-<slug>.md`), numbered from `01` for a new graph in dependency order (blockers first). Preserve existing files and numbering when continuing a graph. Each file's "Blocked by" lists actual dependency numbers/titles. Include exact paths and writes in approval. Use the per-ticket template below: one ticket per file, never a combined file.
- **GitHub or Azure DevOps** → publish one item per ticket in dependency order (blockers first), using the configured planning scope, work-item type, and required fields. Resolve actual returned IDs before writing native parent/dependency relationships or "Blocked by" references. Never presume draft slice numbers are tracker IDs. Use the approved assignee policy; do not silently reassign work.

Apply the configured `ready-for-agent` mapping only to fully specified, actionable slices without unresolved human-owned requirements. Creation does not establish readiness. A specified slice may be blocked by another delivery; retain real edges and do not dispatch until prerequisites are available. Keep incomplete approved drafts explicitly non-ready under the configured convention; preserve unrelated labels/tags and provider workflow states.

For local files, substitute the configured role value in the status field below too. For Azure DevOps use the [provider reference](../setup/issue-tracker-azure-devops.md), not GitHub commands or assumed process fields.

For authorized local ticket writes, use [Changelog](../changelog/SKILL.md) in the correct repository/component: curate notable `Unreleased` entries under Keep a Changelog 1.1.0 or report no entry needed. Include proposed entries in write approval. Do not write during proposals/reviews, dump commits, create versions/releases, or add recursive entries for changelog-only changes. Use an isolated PR worktree only when separately authorized to deliver local files in a PR; external tracker publication does not require one.

Return the source revision, all created IDs/URLs/paths, acceptance coverage, dependency edges, readiness, and grouping to the coordinating owner. In Joe-mode, reserve the parent specification before publication and reconcile the entire graph before dispatch: either one Ship owns the spec and children or suppress the parent from dispatch while intentionally independent child deliveries run. Do not launch both or start Joe-mode merely because tickets exist. After uncertain publication, reconcile actual tracker results before retrying; report partial graphs rather than duplicate tickets.

Work the **frontier**: any ready, owned ticket whose blockers are complete and available on its delivery base, not merely green on an unmerged branch. For a purely linear chain, work top to bottom.

Do NOT close or rewrite any parent issue. Only explicitly approved parent/child relationship operations may touch its links.

<local-ticket-template>

# <NN>: <Ticket title>

**Specification:** <canonical path/URL and revision>

**What to build:** the end-to-end behaviour this ticket makes work, from the user's perspective, not a layer-by-layer implementation list.

**Blocked by:** the numbers/titles of the tickets that gate this one, or "None".

**Status:** <configured role value matching actual readiness>

**Acceptance:** <exact source IDs or criterion/section references>

- [ ] <Source criterion and this slice's observable coverage>

**Verification:** <agreed tests/checks and expected evidence>

**Open decisions:** <owner and blocking impact, or none>

</local-ticket-template>

<issue-template>

## Parent

A reference to the actual parent specification on the tracker, when present. Always identify the canonical specification location and revision even if it is a document rather than a tracker parent.

## What to build

The end-to-end behaviour this ticket makes work, from the user's perspective, not layer-by-layer implementation.

## Acceptance criteria

- [ ] <Exact source criterion ID/reference and this slice's observable coverage>

## Verification and readiness

Agreed tests/checks and expected evidence. State unresolved decisions, their owners,
and blocking impact, or none; distinguish readiness from dependency availability.

## Blocked by

- A reference to each blocking ticket, or "None".

</issue-template>

In either form, avoid specific file paths or code snippets: they go stale fast. Exception: inline a prototype snippet if it encodes a decision more precisely than prose (state machine, reducer, schema, type shape); briefly note its prototype origin. Keep only decision-rich parts, not a working demo.
