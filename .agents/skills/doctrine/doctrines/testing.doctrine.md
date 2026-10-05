---
name: testing
description: "Build valuable behavioral regression protection at the smallest trustworthy scope."
scope: shared-engineering-doctrine
---

# Testing Doctrine

## Prime directive

Test behavior worth protecting at the smallest scope that can provide trustworthy evidence.

## Position

A test earns its lifetime cost by detecting a meaningful regression, resisting harmless refactoring, failing close to the cause, and remaining understandable enough to maintain. Test count and coverage percentage are evidence about a suite, not measures of its value.

The unit under test is a behavior, not a class or line. Choose scope according to the evidence required: a unit test protects a small fast behavior without shared-state dependence; an integration test crosses a meaningful real boundary; an end-to-end test protects a critical gap no smaller scope can prove.

## Principles

- **Protect observable behavior.** Write expectations from requirements, client goals, and independent domain knowledge. Use structural knowledge to find gaps, not as the oracle.
- **Keep the oracle independent.** Do not calculate expected results with the production logic being tested or infer persistence from the objects that performed the write.
- **Prefer the smallest sufficient scope.** Broad tests cost more and localize less. Use them when lower scopes cannot faithfully exercise the contract.
- **Test one meaningful behavior.** Several assertions may prove one outcome, but multiple Acts and branches usually hide several scenarios inside one test.
- **Make Arrange, Act, and Assert visible.** Setup should expose the facts that matter, invoke the behavior once, and verify relevant explicit and implicit outcomes.
- **Use Test-Driven Development at the right time.** Write the failing behavior before the implementation exists, implement minimally, then refactor while the tests remain green. Tests written after behavior exists are regression work, not retroactive Test-Driven Development.
- **Balance four qualities.** Judge regression protection, refactoring resistance, feedback speed, and maintainability together. A test with no meaningful protection has no value however fast or tidy it is.
- **Treat the pyramid as context, not quota.** Prefer many fast focused tests, fewer real-boundary tests, and the fewest end-to-end tests only when that shape matches the system’s risks.
- **Keep setup explicit.** Use focused factories with meaningful parameters when setup repeats. Keep scenario facts in the test and split parameterized cases once their differences become opaque.
- **Use fixtures deliberately.** Shared fixtures help when they preserve clear stable context; they harm when they hide facts, couple unrelated tests, or make failures depend on execution order.
- **Listen to failures.** Repeatedly ignored, retried, disabled, or flaky tests signal weak evidence or uncontrolled dependencies. Repair the cause rather than normalizing distrust.

## Boundary

Testing owns value, behavior, scope, and economics. Test Seams owns doubles, substitution, and collaborator observation. Integration Testing owns persistence, shared-state, and external-boundary evidence. Domain and Data doctrines own the behavior and guarantees being proved.
