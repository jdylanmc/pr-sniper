---
name: patch
description: "Human kickoff or human-started Joe-mode only. Reproduce a bug or regression, establish cause, repair, review, and shepherd a green PR current with its target. Not planned behavior changes; explicit diagnosis-only stays read-only."
disable-model-invocation: false
user-invocable: true
---

Use [doctrine selection and application](../doctrine/APPLY.md), requiring `debugging` for diagnosis and `worktrees` before preparing PR changes. Preserve inherited selections; with none, consider `code` and `testing` for the actual repair. Load selected texts before applying them, without expanding diagnosis-only or mutation boundaries. Code Roast additionally requires `solid`.

# Patch

Separate the observed symptom from the inferred cause. Establish a mechanism that explains the evidence before changing product behavior. See the human-approved [intent](intent.md) and common [invocation policy](../setup/INVOCATION.md).

## Scope and safety

- **Human or human-started Joe-mode selects this root route.** Direct Patch invocation or Joe-mode selection authorizes in-scope isolation, repair, commits, push/PR, review/fixes, and Shepherd custody without repeated implementation/publication questions. Patch owns bug/regression delivery, not planned behavior changes; return those to Joe-mode/the human for routing.
- **Diagnosis-only when requested.** A request only to explain, investigate, or diagnose does not authorize a fix, even when routed through this skill. Stop when the evidence establishes the cause or an exact blocker. Automatic skill selection does not convert a read-only request into repair permission.
- **Continue, do not duplicate.** An existing Patch-owned PR may return from Shepherd for bounded repair. A scoped diagnostic assignment within another delivery returns evidence to that owner, not a second route, PR, or monitor. Other skills must not autonomously select Patch as a new root delivery.
- Read repository guidance, relevant `CONTEXT.md`, and architectural decisions before exploring. Respect the agreed scope.
- Redact secrets from commands, output, and captured artifacts. Use environment variables rather than embedding credentials. Capture only the evidence needed.
- Use existing checks, read-only inspection, and isolated reproduction artifacts during diagnosis. Ask before changing product code for instrumentation, touching production, or running destructive probes. Inspect bundled scripts and their inputs before using them.
- Preserve other people's changes and evidence. Clean up only artifacts this investigation created and is authorized to remove.

For authorized repair, establish an owned isolated workspace using [the workspace procedure](../ship/WORKSPACE.md) before preparing PR changes. Record `patch` as owner route in [the shared delivery packet](../ship/DELIVERY.md#one-delivery-packet-one-owner), including return owner, source/target refs, requirements, validation, and doctrine sources/digests. This shared finish does not invoke Ship. Clarify material missing requirements, semantic conflicts, and scope changes; invocation does not authorize destructive probes or production-data access.

For dispatch/return, load and execute [LIFECYCLE](../squadron/LIFECYCLE.md);
give implementers [WORKER](../ship/WORKER.md). Preserve diagnosis-only scope.
Even terminal read-only agents require owner-accepted returns before retirement.
Repair deliveries execute DELIVERY's accepted custody and verified provider
non-draft gate; never stop at draft publication.

## 1. Establish the failure

Read the complete relevant error and stack trace. Record expected behavior, actual behavior, inputs, environment, version, and the exact failing command or interaction.

Check recent code, dependency, configuration, and environment changes. Find a working example or known-good state to compare against. A nearby failure is not necessarily the reported bug.

Establish why the expected behavior is the existing contract or a previously working behavior. If the request actually adds planned behavior rather than fixes a bug/regression, return it for routing; do not manufacture a defect to keep it in Patch.

## 2. Build and minimize a feedback loop

Find one repeatable command that exercises the real failing path and can distinguish the user's symptom from success. Run it and observe the failure before proposing a repair.

Choose the smallest useful mechanism:

- A focused unit, integration, or end-to-end test at a public interface.
- An HTTP or CLI invocation with a fixture and an independent expected result.
- A browser interaction with an assertion on the actual behavior.
- A redacted trace replay or isolated harness for the affected code path.
- Differential testing or bisection between known-good and failing states.
- A property or stress loop for intermittent behavior.
- A human-assisted loop only when the interaction cannot be automated; adapt [the template](scripts/hitl-loop.template.sh) to the approved environment.

Tighten the signal: narrow setup, pin controllable inputs, and assert the specific symptom rather than "did not crash." Minimize inputs, steps, and dependencies one at a time, rerunning after each reduction.

For intermittent failures, record attempts and failures under a controlled workload; increase reproduction frequency without claiming one successful run proves a fix. For performance failures, establish a baseline and an explicit comparison or threshold before changing anything.

If no usable loop is possible, report the attempts and exact blocker. Ask for the smallest missing access, redacted capture, or instrumentation permission. Do not disguise an untested hypothesis as a diagnosis.

## 3. Explain the cause

Trace inputs, state transitions, and ownership boundaries backward from the symptom. Compare the failing path with working code and list the relevant differences.

Rank plausible hypotheses by evidence and the cost of falsifying them. State each prediction: "If X causes this, changing Y should produce Z." Do not manufacture alternatives when the evidence already distinguishes the mechanism.

Test **one hypothesis and one variable at a time**. Prefer debugger inspection and targeted probes over broad logging. Tag temporary instrumentation so it can be found and removed. For performance regressions, use measurements and profiling rather than log volume.

Record what each experiment rules in or out. A failed hypothesis is new evidence, not a reason to stack speculative fixes. Stop exploration once a credible mechanism explains the observations and a discriminating experiment supports it.

**Diagnosis-only exit:** report the cause, proof, confidence, and any remaining uncertainty. A proposed fix is a recommendation, not permission to implement it.

## 4. Repair only when authorized

Use one developer; no mandatory paired TDD or test-first workflow. Turn the
reproduction into useful regression coverage where a real test seam exists.
Invoke [tdd](../tdd/SKILL.md) only when selected, retaining this Patch owner.
A shallow test that cannot reproduce the actual interaction gives false confidence.

If no suitable test boundary exists, document that limitation and retain the reproduction as evidence. Discuss the missing boundary rather than adding unrelated architecture or pretending the bug is covered.

Change the narrowest responsible layer that owns the incorrect behavior. Preserve surrounding behavior, interfaces outside scope, and the user's changes. Do not add retries, timeouts, validation layers, renaming, cleanup, or abstractions merely because they might suppress the symptom or improve unrelated code.

If the fix fails, return to the evidence and revise the hypothesis instead of layering on another fix. After three failed repair attempts, stop and discuss the assumptions and architecture with the user before attempting another. Repeated failure is a reason to reconsider the approach, not proof that the architecture is wrong.

## 5. Verify the repair

Use [verify](../verify/SKILL.md) for evidence freshness and completion claims; it does not expand the repair's scope.

- Rerun the original, unminimized reproduction and the regression test.
- Check the affected behavior and relevant surrounding tests. Use the same workload and acceptance threshold for performance or intermittent failures.
- Remove only this investigation's temporary instrumentation and disposable artifacts; preserve evidence that still matters.
- State the actual result. Distinguish a verified fix from an untested change, partial mitigation, unavailable check, or blocked investigation.
- Report the cause and decisive proof, any authorized changes, and unresolved risk. Preserve the causal explanation in the delivery's commit or PR description.

## 6. Deliver under Patch ownership

For a repair delivery, execute [the shared finish and maintenance contract](../ship/DELIVERY.md): independent Roast, required integrated validation, criterion verdicts, one PR, current-target readiness, and actual Shepherd custody until a terminal state or explicit blocker. A local verified diff or internal draft is not the final handoff. Patch performs its own delivery; do not invoke Ship to finish it.

Every modifying agent uses [changelog](../changelog/SKILL.md); isolated workers return proposals and the integration owner consolidates/deduplicates the notable entry before review. Use the [shared commit-message policy](../setup/COMMIT-STYLE.md), keeping the causal explanation when consequential.

On functional feedback, recover the current PR and original bug evidence, classify the finding within Patch's bug/regression scope, and repeat the affected diagnosis/repair/proof and independent review on that same branch/PR. Return head/results to its existing Shepherd rather than starting a second monitor. Different-kind or scope-changing work returns to Joe-mode/the human. Explicit diagnosis-only or scoped worker requests return their evidence without publication; never treat them as repair authority. No deployment, self-approval, merge, or automatic merge.

## Supporting techniques

Load these only when relevant; examples do not expand the task's mutation authority or justify unrelated changes:

- [Root-cause tracing](root-cause-tracing.md): follow a bad value to its origin; includes the test-pollution helper.
- [Condition-based waiting](condition-based-waiting.md): replace guessed delays with bounded waits for an observable condition, except when elapsed time is itself the behavior under test.
- [Defense in depth](defense-in-depth.md): choose additional validation only for demonstrated bypass paths and clear ownership of invariants.

The `test-academic.md` and `test-pressure-*.md` files are evaluation prompts, not instructions to execute against a real environment. `CREATION-LOG.md` records upstream history, not validation of this merged skill.
