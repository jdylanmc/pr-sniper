---
name: laziness
description: "Review whether a change solves the whole problem with the least safe maintenance burden."
scope: default-pr-sniper-doctrine
---

# Laziness Doctrine

Code is cheap to generate and expensive to inherit. Favor the smallest complete solution with the least safe maintenance burden. Think like a tired maintainer, not a line-count optimizer.

Review what the change adds, preserves, and makes future readers coordinate. Laziness rejects unnecessary work, never investigation, correctness, validation, security, accessibility, compatibility, diagnostics, or proof.

Look for:

- **Avoidable additions.** Redundant behavior, obsolete paths, purposeless forwarding, duplicated choices, and representation leaks may already provide a simpler route. Recommend deletion only where evidence establishes safety.
- **Incomplete minimalism.** A small diff still must solve the root problem and affected contracts, errors, tests, migrations, documentation, and safeguards. Leaving coupled work broken is not economy.
- **Unnecessary indirection.** Trace ownership and behavior across functions, types, schemas, pipelines, and services. More than three files or layers invites inspection, not automatic rejection. A rich interface hiding substantial work is not inherently a deep call chain.
- **Unearned structure.** Layers, abstractions, wrappers, and generalized mechanisms need a current purpose: invariant enforcement, responsibility, change isolation, stable contracts, visible failure, compatibility, or safe testing. Speculative flexibility is not evidence.
- **Unsafe flattening.** A direct route must preserve ownership, validation, types, authorization, observability, consistency, and lifecycle semantics. Mixing responsibilities or deleting safeguards merely conceals cost.
- **Scope creep.** Adjacent cleanup stays separate unless necessary for the authorized result. Generated or framework-required structure may be legitimate; do not demand its removal to shrink the diff.

Explain the concrete comprehension or maintenance cost and a simpler complete alternative. Prefer fewer obligations, not merely fewer lines. Keep useful boundaries and proven safeguards even when they make the implementation longer.
