---
name: testing
description: "Protect behavior at the smallest trustworthy scope, through a seam that hides irrelevant implementation."
scope: default-pr-sniper-doctrine
---

# Testing Doctrine

Prime directive: protect worth-protecting behavior at smallest trustworthy scope. Seam reveals that behavior and hides irrelevant implementation.

Earns cost: catch meaningful regression, resist harmless refactor, fail near cause, stay understandable. Count and coverage describe a suite, not value. Unit is behavior, not a class or line.

Scope follows evidence. Unit: small, fast, no shared-state dependence. Real boundary only if smaller scope cannot faithfully exercise the contract. End-to-end only for a critical gap smaller scope cannot prove. Pyramid is context, not quota: many fast focused tests, fewer real-boundary tests, fewest end-to-end tests, only if risks match.

Seam: controlled input in, observable behavior out. Good: meaningful failure, cheap refactor. Bad: private structure, copied production logic, unseparated collaborators. Isolation is not the goal. Evidence is. Keep real collaborators when fast, deterministic, application-owned. Substitute when crossing is slow, unstable, destructive, unavailable, or cannot force required failure.

- Expectations: requirements, client goals, and independent domain knowledge. Structure finds gaps, not the oracle.
- Oracle stays independent. Do not compute expected results with logic under test or infer persistence from writer objects.
- Smallest sufficient scope. Broader tests cost more and localize less.
- One behavior. Several assertions may prove one outcome. Multiple Acts or branches hide separate scenarios.
- Show Arrange, Act, Assert: relevant facts, one invoke, explicit and implicit outcomes.
- Test-driven development: failing behavior first, minimal implementation, refactor while green. Later tests are regression work, not retroactive test-driven development.
- Judge protection, refactor resistance, speed, and maintainability together. No protection, no value.
- Repeat setup via focused factories with meaningful parameters. Keep scenario facts in the test. Split cases when differences turn opaque.
- Shared fixtures only for clear stable context. Harm: hidden facts, coupled tests, order-dependent failure.
- Ignored, retried, disabled, or flaky tests show weak evidence or uncontrolled dependencies. Fix the cause. Do not normalize distrust.
- Outcomes before conversations: output, then state, then collaborator talk only if it is the contract.
- Do not freeze call order, private methods, layout, or incidental collaborator counts unless the public contract makes them meaningful.
- Stubs answer. Mocks check expected commands. Spies record interaction for later checks. Model only the needed contract.
- Queries return information without changing observable state. Commands perform effects. Match checks to that split.
- Application-owned managed dependencies can stay real. Unmanaged systems, shared state, clocks, networks, and irreversible effects need an explicit seam.
- Mock pain is design feedback: mixed duties, hidden effects, wrong-level contract.
- Decisions in, effects out. External state as values. Decide from explicit inputs. Return intended effects. Apply outside if that proves behavior easier.
- No test-only architecture: partial mocks, test-only production switches, ambient time, mock-only wrappers, private members exposed only for asserts.
- Reflection or a narrow adapter may cover a non-public external contract when no safer observable seam exists. Keep that exception explicit.

Testing owns value, behavior, scope, economics, substitution, observation. Bounded Context and Data own proved behavior and guarantees. Code and SOLID own production design those seams reveal.
