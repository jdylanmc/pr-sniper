---
name: debugging
description: "Find and change the causal mechanism instead of teaching the system to hide its symptoms."
scope: shared-engineering-doctrine
---

# Debugging Doctrine

## Prime directive

The symptom is evidence. It is not automatically the defect.

## Position

A failure becomes understandable when an explanation accounts for what happened, why it became possible, and why the proposed change prevents it from happening again. Debugging is the work of earning that explanation.

Reproduce the failure when practical. When direct reproduction is impossible, use the strongest available observations and state the uncertainty honestly. A confident guess is not stronger than incomplete evidence.

## Principles

- **Seek the mechanism.** Distinguish what triggered the failure from the defect that made it possible, the conditions that helped it occur, and the missing containment that made it worse. These distinctions clarify thought; they are not ceremony.
- **Preserve evidence before changing state.** A restart, cache clear, retry, rollback, or reset may restore service while destroying the best clue. Recover urgently when needed, but do not mistake recovery for understanding.
- **Separate mitigation from correction.** Containment can stop damage before the cause is known. It remains a workaround until the mechanism is repaired.
- **Instrument instead of guessing.** Add observation when evidence is weak, but remember that instrumentation can change timing, cost, privacy, and behavior. Gather only what the investigation can justify.
- **Use guards to protect, not conceal.** A guard is sound when it enforces a contract, contains damage, exposes invalid state, or fails safely. It is dangerous when it converts a violated invariant into apparent success.
- **Generalize by cause, not appearance.** Similar text and similar symptoms do not prove the same defect. Broaden a repair only where the same mechanism and contract are established.
- **Prove more than disappearance.** A fix is complete when evidence supports the causal explanation, the regression is prevented or made observable, and adjacent behavior still holds.

## Boundary

Do not delay urgent containment while pursuing perfect certainty. Do not destroy evidence, hide degraded behavior, or call a cleared symptom a root-cause repair. Several causes may coexist; debugging should reduce uncertainty rather than force a tidy story.
