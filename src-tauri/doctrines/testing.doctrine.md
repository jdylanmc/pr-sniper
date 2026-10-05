---
name: testing
description: "Review tests for meaningful regression protection, independent expectations, and trustworthy seams."
scope: default-pr-sniper-doctrine
---

# Testing Doctrine

Valuable tests catch meaningful regressions, resist harmless refactoring, localize failures quickly, and remain maintainable. Counts and coverage are not value; behavior, not classes, is the unit.

Review changed tests for trustworthy evidence, not isolation or volume.

Look for:

- **Independent expectations.** Requirements, client goals, and domain knowledge define outcomes. Structure finds gaps, not the oracle. Production logic cannot compute expected results; writer objects cannot prove persistence.
- **Sufficient scope.** Prefer fast units without shared-state dependence; real-boundary tests for contracts smaller scopes cannot prove; end-to-end for remaining critical gaps. Pyramid proportions follow risk, not quotas. Broader tests cost more and localize less.
- **Clear scenarios.** One behavior, visible Arrange/Act/Assert, one invocation, explicit and implicit outcomes; multiple assertions may serve one outcome. Keep scenario facts visible, factory parameters meaningful, parameterized cases distinct, fixtures clear and order-independent.
- **Behavioral observation.** Prefer output, then state, then contractual communication. Do not freeze private structure, call order, or incidental counts. Queries read; commands cause effects. Match assertions accordingly.
- **Honest substitutes.** Keep fast, deterministic, application-owned collaborators real. Substitute slow, unstable, destructive, unavailable dependencies or untestable failures. External systems, shared state, clocks, networks, and irreversible effects need seams. Stubs answer; mocks verify commands; spies record interactions. Model only needed contracts.
- **Design feedback.** Painful setup and brittle mocks expose mixed responsibilities or hidden effects. Explicit input values and returned effects separate decisions from execution. Reject partial mocks, test-only production switches, ambient time, mock-only wrappers, and exposed internals. Reflection or narrow adapters need a non-public external contract with no safer observable seam.
- **Credible evidence.** Repair ignored, retried, disabled, or flaky tests; do not normalize distrust. Test-driven development requires failure before implementation, minimal implementation, then green refactoring. Later tests are regression work.

Report the unprotected behavior or misleading assertion and smallest trustworthy correction, not indiscriminate demands for more tests.
