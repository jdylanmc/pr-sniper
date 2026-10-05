---
name: tdd
description: "Internal test-first helper for any authorized task. Prove red then green through real behavior, allow small behavior-preserving cleanup after green, and rerun affected tests."
disable-model-invocation: false
user-invocable: false
---

# Test-Driven Development

**Entry:** Internal test-first helper for any authorized task. Prove red then green through real behavior, allow small behavior-preserving cleanup after green, and rerun affected tests. Follow the [invocation contract](../setup/INVOCATION.md).

Follow [doctrine selection and application](../doctrine/APPLY.md), **requiring `testing`**. Preserve the work packet's selections. With no preselection, consider `test-seams` when choosing boundaries/doubles and `code` for implementation; load the selected full texts before applying them. Doctrine does not waive the observed red/green loop or grant unrelated refactoring authority.

Work in vertical slices: one behavior, a demonstrated failure, the smallest implementation, and a demonstrated pass. Small, behavior-preserving refactoring is allowed **after green**, followed by another test run.

Read repository guidance, relevant `CONTEXT.md`, and architectural decisions before choosing test names or interfaces. Follow the project's existing test tools and conventions.

## Agree what to test

A **seam** is the public boundary where a caller observes behavior without reaching into the implementation.

Use seams and acceptance behavior already agreed in the task or specification. If they are not settled, identify them and confirm with the user before writing tests. If the interface itself is uncertain, inspect existing interfaces and decisions, then agree the observable contract rather than inventing test-only entry points.

Cover the agreed critical paths, complex logic, and relevant errors or edge cases. Do not require a separate test for every private function or trivial forwarding method.

## Write tests worth keeping

- Test behavior through the agreed public interface, not private structure or internal collaborator calls.
- Name the realistic production defect the test should catch before writing it.
- Derive expected results independently: a specification, hand-checked example, known literal, or independently verified fixture. Never recompute them using the implementation or its helpers.
- Test one behavior per case. Use multiple assertions when they jointly establish that behavior.
- Prefer real components. Use controlled doubles at system boundaries, keeping required side effects real. Assert arguments, order, or counts only when they are part of the observable boundary contract.
- Keep test-only setup and cleanup in test utilities; do not add production methods solely to support a test.

Use [tests.md](tests.md) for examples, [mocking.md](mocking.md) for boundary choices, and [writing-good-tests.md](writing-good-tests.md) for deeper guidance on independent expectations and meaningful failure sensitivity.

## The loop

### 1. Red

Write one test for one missing or incorrect behavior. Run the focused test with the project's existing runner.

Observe a failure caused by that behavior, not by a broken test harness, missing import, or unavailable environment. Read the result; a test that was never executed is not a demonstrated red.

If it passes immediately, check that the runner selected the test and whether the behavior already exists. Choose an actually missing behavior, or describe the test as characterization of existing behavior. Do not manufacture a wrong expectation merely to produce red.

### 2. Green

Write only the code needed for that behavior. Do not anticipate later tests, add speculative options, or refactor unrelated code.

Run the new test and affected existing tests. Fix the implementation when a valid assertion fails; do not weaken the assertion to force a pass. Correct a test only when its expectation or setup is demonstrably wrong.

Distinguish a passing focused run from a passing full suite. Broaden checks when the change's impact or repository guidance requires it.

### 3. Refactor, if useful

Only after green, make small changes that preserve the agreed behavior: clarify a name, remove duplication, or simplify the code just exercised.

Rerun affected tests after the cleanup. Restore green before starting another behavior. A new feature, changed interface contract, or broad architectural rewrite is not cleanup; return to requirements and the red step for behavior changes, and obtain direction for work outside the task.

### 4. Repeat

Choose the next agreed behavior using what the last slice taught you. Do not write a bulk suite of imagined tests and then implement everything horizontally.

## Bugs and existing work

For a bug, reproduce the original symptom and turn it into a regression test at
a seam that exercises the real interaction. If cause or reproduction is unclear,
return the evidence gap to the existing delivery owner. An active Patch owner
continues its diagnosis; Joe-mode or the human chooses a new Patch delivery
when appropriate. Do not automatically activate a separate Patch route from
this helper. Resume the test-first loop after the cause is established, and
verify both the minimized test and the original scenario after the fix.

If production code already exists before the test, preserve the user's work and say so. Do not delete or rewrite others' changes to manufacture a test-first history. Add honest characterization or regression coverage; demonstrating a regression against a pre-fix state requires an isolated or explicitly authorized method.

If no meaningful test boundary or runnable environment is available, state the blocker and the evidence needed. Do not substitute a shallow test, source-text assertion, or assumed pass.

Exceptions to test-first implementation, such as a throwaway prototype or generated output, require agreement with the user. Do not silently claim the exception followed the loop.

## Completion

Use `verify` to check the agreed acceptance conditions. Confirm the relevant tests actually ran, the final state remains green after any refactor, and important regression scenarios are covered.

Report missing coverage or blocked checks explicitly. Ship the tests with the implementation, without adding unrelated tests, cleanup, or features after the scoped work is complete.

Use [Changelog](../changelog/SKILL.md) for notable outcomes, returning entry
proposals to the integration owner when tests are part of another delivery.
This internal helper does not start another PR or Shepherd.
