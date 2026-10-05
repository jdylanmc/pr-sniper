---
name: pragmatic
description: "Own outcomes, calibrate methods, and keep changing systems responsible to evidence."
scope: shared-engineering-doctrine
---

# Pragmatic Doctrine

## Prime directive

Answer for the result, not for having followed a ritual.

## Position

Engineering happens under incomplete information, changing conditions, and limited time. A responsible engineer chooses methods according to the actual users, risks, evidence, and codebase instead of repeating a practice because it is familiar.

Calibration never means lowering a binding standard. Requirements, doctrine, safety, and explicit human decisions remain constraints. Pragmatism decides how best to satisfy them.

## Principles

- **Own the outcome.** State tradeoffs, risks, unknowns, and avoidable future costs. Tools, inherited design, and schedule pressure explain conditions; they do not become responsible for the result.
- **Calibrate effort to consequence.** Spend process, proof, precision, and time where they improve this outcome. Reject ceremony that produces no useful signal.
- **Price the future inside the scope.** A cheap edit that makes every later change harder is usually expensive. Within the authorized implementation scope, improve directly related surroundings when the improvement is small, low-risk, and cheaper now than later.
- **Preserve reversibility while evidence is weak.** Avoid welding uncertain vendors, platforms, environments, policies, or requirements into the design before the decision earns that commitment.
- **Charge shared state honestly.** Globals, ambient context, mutable shared data, ordering, locks, and asynchronous behavior impose coordination costs. Make ownership, synchronization, cleanup, and failure visible.
- **Excavate the requirement.** Separate durable needs and constraints from today’s implementation detail, proposed solutions disguised as needs, and disagreements nobody has stated yet.
- **Classify failure before handling it.** Expected domain failure, violated contract, impossible state, transient fault, recoverable damage, and permanent failure require different responses. Preserve diagnostic context and place recovery where it can make sense of the failure.
- **Own what you acquire.** Memory, handles, locks, temporary state, and external effects create cleanup obligations across success and failure paths.
- **Make uncertainty visible.** Estimates and plans are provisional. Use early evidence and feedback to correct them rather than defending expired confidence.
- **Keep accountability shared.** Teams should make expectations, quality, remaining risk, and the evidence behind completion visible enough that everyone can stand behind the work.

## Boundary

Pragmatism is not permission to bypass doctrine, weaken proof, or accept hidden debt. It is disciplined judgment about how to reach the required outcome under real conditions.
