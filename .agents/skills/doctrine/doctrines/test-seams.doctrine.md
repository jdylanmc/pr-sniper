---
name: test-seams
description: "Choose test boundaries and substitutes that protect behavior without coupling tests to implementation shape."
scope: shared-engineering-doctrine
---

# Test Seams Doctrine

## Prime directive

Test through a boundary that reveals behavior and hides irrelevant implementation detail.

## Position

A test seam is where controlled input enters and observable behavior leaves. A good seam makes failures meaningful and refactoring cheap. A bad seam forces tests to know private structure, reproduce production logic, or orchestrate collaborators the system itself has not separated cleanly.

Isolation is not the goal. Trustworthy evidence is. Use real collaborators when they are fast, deterministic, and owned by the application. Substitute dependencies when crossing them would make the test slow, unstable, destructive, unavailable, or unable to exercise required failure behavior.

## Principles

- **Prefer outcomes over conversations.** Verify returned output first, then observable state, then collaborator communication only when the communication itself is the contract.
- **Test behavior, not construction.** Do not freeze call order, private methods, object layout, or incidental collaborator counts unless the public contract makes them meaningful.
- **Choose doubles by role.** Stubs provide answers. Mocks verify expected commands. Spies record interaction for later assertions. A substitute should model only the contract the test needs.
- **Respect command and query semantics.** Queries return information without changing observable state. Commands perform effects. Verification should match that distinction.
- **Know the application edge.** Managed dependencies under application control can often remain real. Unmanaged systems, shared state, clocks, networks, and irreversible effects need an explicit seam.
- **Treat mock pain as design feedback.** Large setup graphs and fragile interaction assertions often reveal mixed responsibilities, hidden effects, or contracts drawn at the wrong level.
- **Move decisions inward and effects outward.** Represent external state as values, decide using explicit inputs, return intended effects, and apply them at an outer boundary when that makes behavior easier to prove.
- **Reject test-only architecture.** Avoid partial mocks, production switches used only by tests, ambient time, wrappers created solely for mocking, and exposing private members merely to assert them.
- **Allow controlled exceptions.** Reflection or a narrow adapter may be justified for a non-public external contract when no safer observable seam exists. Keep the exception explicit.

## Boundary

Test Seams owns substitution and observation choices. Testing owns value, economics, and test scope. Integration Testing owns evidence across real persistence, shared state, and external boundaries. SOLID and Code own the production design those seams reveal.
