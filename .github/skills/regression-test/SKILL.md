---
name: regression-test
description: "Author shared executable PR Sniper feature regressions and registration. Use for scoped implemented feature expectations, not VM setup, unit-only coverage or independent signoff."
user-invocable: true
disable-model-invocation: false
---

# Add shared feature coverage

Read [stored intent](intent.md), root [AGENTS.md](../../../AGENTS.md), the
[case and evidence contract](../../../docs/agents/vm-regression.md) and
[registry](../../../regression-suite/registry.mjs). This package is prepared,
not independently finalized; installed-app execution remains unproved.

1. Trace the approved feature expectation to current code and existing tests.
   Choose one bounded behavior and a wrong observable result it must reject.
2. Add/update executable cases under `regression-suite/cases/`, metadata
   (feature/ID, source, preconditions, expectations, cleanup) and registration.
   Use semantic actions/observations; native selectors belong in a later driver.
   Windows must consume the same assertions, never a copied feature library.
3. Add fixture contracts under `tests/regression/` showing right output, wrong
   output, unavailable interaction, stale identity and restoration failure.
   Run `npm run test:regression`; retain existing complementary gates.
4. Do not alter expected verdicts to accept broken behavior. Fixture PASS is
   not native PASS. New actions without a proven guest implementation remain
   explicitly unsupported; never add product test hooks or host UI fallback.
5. Submit candidate/test identities, selected scope and expected behavior to
   `regression-suite-mac` for a future fresh independent worker. While the native
   seam is unavailable, report **BLOCKED / unexecuted**, not author signoff.

Return IDs/files, requirement sources, fixture outcomes, coverage gaps and
pending independent/native evidence. Keep runtime/evidence ignored. Do not
provision, re-sign a submitted candidate, change product permissions, file bugs
automatically or modify imported personal workflows.
