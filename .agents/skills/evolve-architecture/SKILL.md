---
name: evolve-architecture
description: "Human or human-started Joe-mode only. Diagnose evidenced architectural friction, challenge it, explore consequential alternatives, and propose a bounded evolution before authorized delivery."
disable-model-invocation: false
user-invocable: true
---

# Evolve Architecture

**Entry:** Human or human-started Joe-mode only. Diagnose evidenced architectural friction, challenge it, explore consequential alternatives, and propose a bounded evolution before authorized delivery. Follow the [invocation contract](../setup/INVOCATION.md).

Own architectural diagnosis and the evolution proposal, not discovery or delivery. The human-authored [intent](intent.md) defines the purpose. Default to a decision-ready proposal with no product, repository, tracker, or architecture-record writes.

Use [Doctrine](../doctrine/SKILL.md) and the [shared application/worker-packet contract](../doctrine/APPLY.md). Preserve the task's preselection. With none, use catalog metadata to choose relevant guidance such as `solid`, `boundaries`, `laziness`, or `test-seams`; these are candidates, not a mandatory bundle. Applying workers load their assigned texts. Code Roast adds required `solid`; consequential exploration uses Discovery's required `scout`.

## 1. Find concrete friction

Resolve the repository, concern, desired outcome, exclusions, and available evidence. Read project terminology and relevant existing architecture decisions. Use the human's named area; do not scan everything.

If no area was named, inspect recent changes for likely hot spots and propose a bounded investigation. Frequency is a lead, not proof of architectural harm. Confirm material scope choices before broadening.

Trace representative changes and behavior through real interfaces and owners. Look for consequential coupling, unclear responsibility, fragile behavioral tests, duplicated policy, or boundaries that impede ordinary work. Cite locations and actual cost or failure paths; label unmeasured consequences as hypotheses.

Use a bounded exploration worker when substantial independent reading warrants it. Supply the question, evidence scope, non-goals, read-only permissions, and doctrine packet. No redesign or implementation. Keep its report accessible without copying the whole investigation into the coordinator.

## 2. Challenge the diagnosis

Use [Roast](../roast/SKILL.md) to test the diagnosis against its code/evidence, requirements, and existing safeguards. Follow Roast's independent-review rules for your own work. Ask whether the friction is real and matters to the requested outcome, and what evidence could disprove it.

Reconcile findings, not votes or reviewer confidence. Drop unsupported candidates; keep uncertainty visible. A deep module, abstraction, or deletion is not inherently an improvement. Do not invent a candidate count or force architectural work without a supported consequential problem.

If several worthwhile candidates remain, briefly present their evidence and consequences; let the human choose the bounded evolution. An existing explicit choice need not be requested again.

## 3. Explore the consequential alternatives

If the direction is genuinely uncertain, use [Discovery](../discovery/SKILL.md) with the diagnosis, agreed constraints, existing decisions, and doctrine packet. Discovery owns its evidence/alignment/domain/frontier/persistence cycle and routes focused interrogation, research, and isolated POC work when useful. Do not reproduce an interview loop here.

Compare meaningful routes, including keeping the current design, simplifying, deleting, changing ownership, and staged compatibility work when viable. Distinguish evidence from taste. Discovery's Scout stopping case governs breadth; do not stop at a cosmetic set of options or extend exploration merely to satisfy a quota.

Read Discovery's aligned foundation and handoff before proposing the evolution. An already-clear, bounded direction can use existing confirmed evidence without reopening settled choices or forcing another discovery cycle.

## 4. Present the evolution proposal

Use project vocabulary; explain unfamiliar terms. Write for the human choosing the direction and the potential implementer. Include:

- **Friction and evidence:** the relevant current behavior, its cost, and confidence.
- **Proposed change:** the responsibility or boundary that moves, simplifies, or disappears.
- **Preservation contract:** observable behavior, public interfaces, ownership guarantees, and non-goals.
- **Alternatives and tradeoffs:** why this route is recommended, what lost, and what remains uncertain.
- **Transition and verification:** bounded steps, compatibility/rollback considerations, and the observations or tests that would demonstrate the intended improvement.
- **Decision state:** confirmed choices, pending questions, and whether implementation or recording is authorized.

Review the proposal against its evidence and preservation contract. Do not silently resolve contradictory requirements or claim projected benefits were measured. Surface affected existing architecture decisions; do not discard them.

Use before/after visuals only when they clarify a change: dependency or sequence diagrams for interactions, ownership boxes for responsibilities, or a cross-section for layers. Use comparable scope and terminology on both sides; do not invent edges or proportions. Keep explanation and uncertainty even when the diagram is compact.

Default to the conversation. For a requested separate artifact, use an agreed destination and preserve existing files; repository writes need explicit authority. HTML is optional, not a required scaffold, browser session, dependency installation, or external CDN. A visual aid does not approve the proposal.

Ask the human to confirm the direction or identify changes. A decision worth preserving may be recorded through [domain-modeling](../domain-modeling/SKILL.md) only with established significance and recording authority. No automatic domain-recording during exploratory questions.

## 5. Stop or transfer authorized execution

Default to returning the proposal, evidence pointers, decision state, and next action. No supported improvement, missing evidence, or unresolved human choice is a legitimate stopping result. Do not manufacture tickets or code.

When execution is already or separately authorized, give the existing owner the
bounded scope, preservation contract, verification, transition plan, and doctrine
packet. Under Joe-mode, return it for selection of the appropriate delivery
route. [Refactor](../refactor/SKILL.md) owns behavior-preserving structural
delivery, or works within an existing owner's scope; a standalone human
proposal can be delivered by a human-directed [Ship](../ship/SKILL.md).
Use [Migration](../migration/SKILL.md) only for established production use with
a real migration obligation, not automatically for pre-1.0 prototypes.
New product behavior requires explicit requirements, never disguised cleanup.

The selected delivery owner retains integration, independent review, publication,
and Shepherd custody on one PR. Do not start a competing loop or mutate its
workspace. Any authorized documentation PR also requires `worktrees` and
[Changelog](../changelog/SKILL.md). If further planning is needed, pass the
aligned Discovery artifact to Specify, then the full requirements specification
to Breakdown Tickets under its approval gate; a proposal alone is not either
input.

Under Joe-mode, return decision-ready work to that orchestrator to advance the existing delivery lane within current authority. Do not claim an agent was dispatched or a handoff accepted unless it happened.
