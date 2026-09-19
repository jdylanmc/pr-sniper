---
name: idempotency
description: "Make retries, restarts, and partial failure converge safely or stop honestly."
scope: shared-engineering-doctrine
---

# Idempotency Doctrine

## Prime directive

Design every mutation so repetition and interruption have deliberate outcomes.

## Position

Retries happen after timeouts, crashes, lost responses, duplicate delivery, and uncertain completion. Treating them as exceptional leaves the most dangerous state transitions undefined.

Idempotency is one answer, not a synonym for recovery. Some operations can repeat with the same observable effect. Others must reconcile toward desired state, deduplicate effects, compensate, or stop for human recovery. The mechanism follows the guarantees the system can actually make.

## Principles

- **Define invariants before mutation.** Name the desired state, authoritative source, admissible starting states, ownership, commit boundary, and effects that escape it. Recovery cannot be clearer than the contract it recovers.
- **Preserve logical identity during retry.** Repeating one command keeps its original identity and intent. Recomputing from current authority is reconciliation, not retry. Confusing them defeats deduplication and changes what “same operation” means.
- **Classify durable state before acting.** After interruption, distinguish complete, partial, stale, conflicting, corrupt, and ambiguous state. Resume, repair, replace, or refuse according to evidence—not creation order or optimism.
- **Prove authority before writing or deleting.** Locks, leases, tokens, and ownership records matter only when they demonstrate who may act now. Never clean up state whose ownership or equivalence is uncertain.
- **Protect external effects separately.** A local transaction cannot promise that a remote effect happened once. Use only guarantees the complete path supports, and name when outcomes are duplicated, compensatable, manually reconcilable, or unknowable.
- **Prefer convergence over repeated hope.** A recovery loop should move classifiable state toward one intended outcome, stop on permanent failure, and refuse to spin forever through uncertainty.
- **Expose ambiguity.** Unknown completion, violated invariants, incompatible versions, or unprovable ownership are real states. Surface them rather than manufacturing a success-shaped answer.

## Boundary

Do not promise exactly-once behavior from systems that cannot provide it. Do not replace available atomicity with eventual repair, or availability with silent corruption. Safety and evidence outrank the appearance of self-healing progress.
