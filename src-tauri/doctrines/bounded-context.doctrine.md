---
name: bounded-context
description: "Review whether changes preserve business meaning, local model coherence, and deliberate boundaries."
scope: default-pr-sniper-doctrine
---

# Bounded Context Doctrine

Business meaning belongs to the context that owns it. Think dollhouse: coherent world inside, deliberate doors for exchange. Walls preserve meaning, not isolation.

Review the proposed change against business scenarios and established language. Models grow through conversation, implementation, and refactoring; tables, screens, frameworks, and diagrams do not define them.

Look for:

- **Language drift.** Code, tests, documentation, and business conversation should name local concepts consistently. Different meanings across contexts are legitimate, not duplication to erase.
- **Misplaced decisions.** Policies, calculations, constraints, processes, and criteria belong in the model, not hidden in presentation, persistence, transport, or coordination. Infrastructure supports distinctive business behavior.
- **Wrong building blocks.** Identity suggests entities; immutable descriptive attributes suggest values; business operations without natural objects suggest services; related concepts form modules. Judge meaning, not pattern count.
- **Broken invariants.** Mutually consistent objects need an aggregate root that outside callers cannot bypass. Construction establishes valid state before exposure. Retrieval hides storage mechanics.
- **Leaking contracts.** Persistence and transport must preserve identity, value semantics, invariants, and retrieval needs. Operations express business intent, distinguish queries from commands, and expose preconditions and postconditions.
- **Unowned crossings.** Shared state or imported models can obscure authority. Cross-context collaboration needs explicit contracts and translation wherever meaning differs.
- **Wrong boundaries.** Teams, repositories, services, and databases may align with contexts but do not define them. Universal models collapse distinctions; excessive fragmentation adds translation cost. Changed purpose or ownership warrants revisiting contracts.
- **Missing behavioral evidence.** Check legal construction, invariants, allowed and rejected transitions, and meaningful outcomes, not plumbing alone.

Tie findings to changed behavior and concrete ownership or meaning loss. Recommend the smallest correction preserving coherence and intentional collaboration. Do not demand domain patterns or architectural isolation without a demonstrated need.
