---
name: cyclomatic-complexity
description: "Review control-flow complexity for meaningful decomposition without weakening behavior or safeguards."
scope: default-pr-sniper-doctrine
---

# Cyclomatic Complexity Doctrine

Control-flow paths cost understanding and proof. Aim for cyclomatic complexity of five or less per routine. Above ten is a red flag to raise in review.

Review changed routines for added paths and increased reasoning burden. Humans and agents share one standard; an agent's ability to enumerate branches does not justify harder code.

Cyclomatic complexity counts independent paths, roughly decisions plus one for a connected routine. Cases, compound conditions, and exceptions depend on the measuring tool. Distinguish measured results from estimates; state the convention behind a numerical finding.

Look for:

- **Accumulating decisions.** New branches may reveal mixed responsibilities, tangled policy and mechanism, or a missing domain concept. Explain which decisions deserve separate ownership.
- **Hidden complexity.** Shared state, side effects, vague names, and scattered context can make low-scoring code difficult. A lower number alone does not establish clarity.
- **Meaningful decomposition.** Extract cohesive behavior or concepts. Moving branches into tiny forwarding functions merely relocates the mental burden.
- **Elevated complexity.** Always flag changed routines above ten and explain their reasoning and verification burden. If cohesive domain rules, invariants, safety boundaries, or compatibility obligations justify complexity, present that tradeoff alongside the flag. Justification does not erase the concern.
- **Preserved guarantees.** A numerical improvement must not weaken correctness, cohesion, types, validation, diagnostics, security, accessibility, compatibility, or behavioral tests.

Describe the affected routine, changed paths, and consequence for understanding or verification. Recommend the smallest meaningful simplification. Keep necessary complexity and its justification visible.

Numbers expose risk; they are not a substitute for judgment. Neither a low score proves good code nor a high score alone proves a functional defect. Do not expand a local change into repository-wide metric cleanup.
