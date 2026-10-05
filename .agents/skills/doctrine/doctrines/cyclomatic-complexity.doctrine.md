---
name: cyclomatic-complexity
description: "Bound independent control-flow paths so code remains understandable to humans and agents."
scope: shared-engineering-doctrine
---

# Cyclomatic Complexity Doctrine

## Prime directive

Keep cyclomatic complexity at five or less. Do not exceed ten without explicit human approval.

## Position

Cyclomatic complexity counts independent control-flow paths through a routine. For a connected routine, it is roughly the number of decisions plus one, though exact treatment of cases, compound conditions, and exceptions depends on the measuring tool.

Each path adds another condition a maintainer must understand and another behavior the system may need to prove. Humans and agents may fail differently, but they share the same code. Do not grant agents a higher tolerance merely because they can enumerate more branches; hidden state, side effects, vague names, and dispersed context still obscure meaning.

## Principles

- **Use one shared standard.** Code should be easy for a human to understand and an agent to reason about. Optimize for their shared need: visible, cohesive behavior.
- **Target five or less.** A routine at or below five usually leaves enough room to understand its decisions without building a mental simulator.
- **Treat ten as the ceiling.** Complexity above ten requires an explicit human exception grounded in a cohesive domain rule, invariant, safety boundary, compatibility obligation, or other evidence that decomposition would make the code worse.
- **Investigate increases.** New paths should trigger a design conversation. Look for several decisions trapped in one routine, tangled policy and mechanism, or a missing domain concept.
- **Decompose meaningfully.** Split responsibilities and concepts. Moving branches into tiny forwarding functions lowers a score without reducing cognitive burden.
- **Protect semantic quality.** Never weaken correctness, cohesion, types, validation, diagnostics, security, accessibility, compatibility, or tests to satisfy the number.
- **Explain necessary complexity.** A justified exception remains visible and reviewable. Measurement is pressure toward clarity, not permission to stop thinking.

## Boundary

Cyclomatic complexity measures paths, not understanding. Use the number to expose risk and prompt better design. Never confuse a low score with good code or a high score with automatic failure.
