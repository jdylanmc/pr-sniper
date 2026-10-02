---
name: regression-suite-mac
description: "Prepare selected or full registered PR Sniper Mac regressions for independent guest verification. Missing setup or the unimplemented native driver means BLOCKED, never host or fixture fallback."
user-invocable: true
disable-model-invocation: false
---

# Coordinate independent Mac verification

Read [stored intent](intent.md), [worker contract](WORKER.md), root
[AGENTS.md](../../../AGENTS.md) and the
[preparation guide](../../../docs/agents/vm-regression.md).
This package is preparation, not an operational native runner.

1. Record ad-hoc versus owning feature context, exact candidate/test inputs and
   requested feature names or `full`. Run the read-only preparation command.
   Surface unknown features, unselected cases and registered-suite limits.
2. Return **BLOCKED / zero executed** while setup, identity, driver or independent
   dispatch is unavailable. Do not spawn an execution worker merely to rediscover
   the current missing driver. Missing setup may be offered to an interactive
   human; only their consent activates `setup-regression-suite-mac`.
3. After separately delivered native tooling and human setup, obtain one fresh
   independent execution worker with the contract below. Retain actual dispatch
   evidence, not two actor-name strings. No independent context means BLOCKED.
4. Before guest mutation, that worker must claim guest-wide exclusive ownership.
   An existing claim, concurrent request or uncertain cleanup means visible
   **busy / BLOCKED**. Never steal a lock, queue invisibly, or terminate its owner.
5. Preserve raw per-attempt PASS/FAIL/BLOCKED and evidence, negative-control intent,
   executed/unexecuted scope and cleanup uncertainty. Do not rerun until green.
6. Return potential bugs to the main development agent. It validates evidence
   before ad-hoc triage or the owning feature flow; no automatic tracker writes.
   Environment/harness faults and expected negative controls are not app bugs.

Do not patch app/tests, replace or re-sign candidates, use personal credentials,
enable login items, invoke paid inference or publish provider actions. Headless
means a usable **guest GUI without a host viewer or host input**, not a browser
fixture. This development workflow grants no product review-Agent permissions.
