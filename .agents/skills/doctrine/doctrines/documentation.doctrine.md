---
name: documentation
description: "Authority and trust for knowledge that must outlive the moment."
scope: shared-engineering-doctrine
---

# Documentation Doctrine

## Position

Documentation should preserve knowledge, not cast a prose shadow over the code.

Source code and executable behavior are the authority for what an implementation currently does. A handwritten walkthrough of classes, functions, files, or control flow creates a second account of the same fact. The prose is not compiled with the code, so it drifts. When the two disagree, both humans and agents waste time deciding which story to trust.

Code cannot carry every truth. It rarely explains why an alternative lost, what a product intends, what a public contract promises, which words a domain owns, how an operator recovers a system, or which obligations require evidence. Preserve those truths in artifacts suited to them.

## Principles

- **One concern, one authority.** Name the artifact that owns each durable fact. Other representations are generated, validated, or clearly derived from it.
- **Let code explain behavior.** Make ownership, boundaries, contracts, and entry points discoverable in the implementation. Comments preserve intent, invariants, constraints, and surprising decisions; they do not narrate syntax.
- **Document what code cannot.** Product intent, architecture rationale, domain language, public contracts, user guidance, operations, migrations, and governance have legitimate homes outside implementation code.
- **Make every document earn its maintenance cost.** A permanent document needs a unique purpose, an owner or canonical source, and a credible way to stay current. Generate reference material from validated sources when practical.
- **Treat disagreement as drift.** Identify which concern owns the disputed fact, trust that authority, and repair the other artifact. Never average conflicting accounts or choose the convenient one.
- **Optimize retrieval, not document count.** Keep entry points small and navigable. Link to focused authority and load detail only when relevant. Deleting unique knowledge is not context economy.

## Boundary

“Self-explanatory code” never excuses missing rationale, public contracts, recovery procedures, user guidance, or required security, privacy, accessibility, licensing, compliance, and audit records.
