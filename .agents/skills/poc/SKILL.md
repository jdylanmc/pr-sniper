---
name: poc
description: "Machine-first, human invocation also allowed. Run bounded isolated proof-of-concept experiments for feasibility, compatibility, performance, logic, or UI questions; return observed findings, not product promotion."
disable-model-invocation: false
user-invocable: true
---

# Proof of Concept

**Entry:** Machine-first; human invocation also allowed. Run bounded, isolated experiments for feasibility, compatibility, performance, logic, or UI questions. Return observed findings, not product promotion. Follow the [invocation contract](../setup/INVOCATION.md).

Use [doctrine selection and application](../doctrine/APPLY.md), preserving the inquiry's choices. With none, consider `scout` and `machine` for evidence-buying experiments; add testing guidance only where the agreed experiment warrants it. Apply loaded rules within scratch isolation: they neither make throwaway work production code nor waive the human's learning budget.

Buy real information cheaply. The prototype is usually throwaway; the learning is not. The retained [intent](intent.md) defines the purpose.

## 1. Frame the experiment

Identify the question, tested approach, and observations that would support or contradict it. Ideas, libraries, frameworks, engines, integrations, data models, and UI behavior are all eligible. Do not force technical feasibility into a visual demo.

Before building, resolve missing decisions with the human: learning goal, relevant environment, success/failure observations, and time or effort limit. Reuse supplied boundaries. Name excluded questions to prevent silent expansion into a build.

When discovery calls, take one bounded question and return evidence to that session. Do not assume authority over its alignment, domain model, handoff, compaction, tracker, or next cycle.

## 2. Isolate and choose the smallest useful shape

Default to a uniquely named session or OS-temporary directory outside the product checkout. If app context is needed, first agree on a separate scratch copy or isolated worktree. Do not edit the working product checkout, its dependency manifests, live services, or databases, or silently save experiment artifacts in the repository.

Use synthetic inputs, fake credentials, local stubs, and scratch stores; no real secrets or personal data. If those limits prevent testing the real integration, test the safe subset and mark the integration unverified. A stub does not prove the external system works.

Choose the form that answers the question:

- **Technical feasibility:** a minimal executable, script, harness, or small app using the actual candidate technology. Exercise the API, compatibility boundary, failure case, or measured threshold at issue.
- **Interactive logic/state:** [LOGIC.md](LOGIC.md), when a shareable HTML demo helps a person explore transitions and edge cases.
- **UI exploration:** [UI.md](UI.md), when alternative layouts or interactions need human comparison.
- **Another form:** agree a similarly small, runnable experiment. The listed forms are not a closed menu.

Read only the chosen form's reference. Its presentation guidance does not override this skill's isolation, execution, or no-product-change boundaries.

## 3. Build and run

Clearly mark code experimental. Use existing tools where practical; explain necessary dependencies and install only inside the agreed scratch environment. Ask before using external services, paid resources, or broader environment changes.

Consult [Changelog](../changelog/SKILL.md) for modifying work. Temporary experiments normally need no product changelog entry; return that reason rather than writing release history or promoting the experiment.

Skip production polish, general-purpose abstractions, and unrelated infrastructure. Include assertions, diagnostics, error handling, or small tests needed to trust the experiment. Building alone does not answer a runtime question.

Run the experiment. Record exact invocation, relevant versions and environment, inputs, expected observations, and actual outputs. Exercise the main case and edge or failure cases that could overturn the conclusion. For performance or compatibility claims, measure the agreed threshold on the relevant environment; report limitations rather than generalizing from another.

For interactive demos, exercise controls or routes yourself when tools permit, then ask the human for judgments requiring them. Never invent feedback. If execution or feedback is unavailable, report missing evidence and an inconclusive or partial result, not successful proof.

Stop at the agreed limit. An unanswered question is valid; ask before extending. Stop run-owned servers when finished unless the human asks to keep them available. State remaining scratch resources.

## 4. Return the findings

Produce a concise findings packet with:

- The question, scope, and tested approach.
- How to reproduce it: artifact location, commands, versions, and inputs.
- Observations and evidence, including failures, edge cases, and gaps.
- Human feedback, distinguished from agent observations; mark it pending when absent.
- A supported, contradicted, or inconclusive verdict against the agreed question, with limitations and possible next investigations.

Keep findings in the conversation or beside the scratch experiment unless another destination was explicitly authorized. Mark scratch artifacts temporary; do not promise survival after cleanup. Retain evidence the receiver needs through handoff.

Return the packet to discovery if it called; otherwise to the human for discovery, specification, or implementation. Do not promote prototype code into product code, create tickets or specs, deploy, commit, or publish the experiment as a side effect. Product implementation is separate work.
