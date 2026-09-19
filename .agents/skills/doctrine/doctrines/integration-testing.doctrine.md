---
name: integration-testing
description: "Prove behavior across the real boundaries lower-scope tests cannot faithfully protect."
scope: shared-engineering-doctrine
---

# Integration Testing Doctrine

## Prime directive

Use real boundaries where substitutes would hide the failure you need to prevent.

## Position

Integration tests prove that separately correct parts still honor their contracts when connected to real persistence, shared state, processes, protocols, or external systems. Their value comes from fidelity unavailable at lower scopes; their cost comes from slowness, coordination, environmental dependence, and cleanup.

Use the maximum necessary reality, not the maximum available reality. Every real dependency should protect an important behavior that a smaller test cannot prove.

## Principles

- **Cross the boundary deliberately.** Name which real dependency matters and which contract the test proves. Do not build an end-to-end environment merely because integration is involved.
- **Reload persisted truth.** Verify durable results through an independent read rather than trusting the objects or responses that performed the write.
- **Specify exact effects.** Assert the values sent across unmanaged boundaries and maintain a permitted-call baseline that rejects unexpected effects.
- **Preserve production fidelity.** Database and protocol behavior should match the production technology where its semantics affect correctness. A convenient substitute that behaves differently proves the wrong system.
- **Separate Arrange, Act, and Assert contexts.** Setup, operation, and independent observation may require different connections or processes so the assertion does not inherit hidden state from the action.
- **Own shared-state cleanup.** Preserve immutable reference data, clear scenario-owned state before each test, and serialize tests whose correctness depends on shared mutable resources. Teardown or rollback alone cannot protect the next run after interruption.
- **Combine expensive operations only when natural.** Consecutive actions may share one test when splitting them would create greater external cost and the combined failure remains diagnosable.
- **Prove safety before omitting an edge case.** Skip a real-boundary scenario only when lower-scope evidence shows the failure occurs before persistent mutation or external effect.
- **Use end-to-end tests for irreducible gaps.** Reserve the broadest scope for critical behavior that unit and focused integration tests cannot protect.

## Boundary

Integration Testing owns evidence across real boundaries. Testing owns value, scope, and economics. Test Seams owns substitution and collaborator observation. Data doctrines own the guarantees being tested; this doctrine owns proving that the assembled system actually provides them.
