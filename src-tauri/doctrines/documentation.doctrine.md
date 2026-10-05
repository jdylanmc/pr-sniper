---
name: documentation
description: "Review whether documentation preserves unique knowledge with clear authority and maintainable sources."
scope: default-pr-sniper-doctrine
---

# Documentation Doctrine

Documentation preserves knowledge, not a prose shadow of code. Each concern needs one authority.

Review both documentation changes and knowledge obligations created by implementation changes. Source code and executable behavior establish what implementation currently does; requirements and public contracts establish what it must do. A specification is not implementation evidence.

Look for:

- **Competing accounts.** Handwritten inventories of classes, files, functions, or control flow drift independently of code. Prefer discoverable implementation and generated or validated reference material over another manually synchronized description.
- **Lost rationale.** Code rarely explains rejected alternatives, product intent, architecture decisions, domain language, or accepted constraints. Check that changed decisions retain their reason and legitimate durable home.
- **Useful comments.** Comments should expose intent, invariants, constraints, and surprising decisions, not narrate syntax. Misleading comments are worse than absent narration.
- **Maintained authority.** A lasting document needs a unique purpose, owner or canonical source, and credible update path. Derived material must identify its source instead of claiming independent authority.
- **Contract drift.** Compare changed behavior with user guidance, public interfaces, recovery procedures, migration instructions, and governance. Resolve disagreement by identifying which concern owns the fact, never by averaging accounts or choosing convenient prose.
- **Retrievability.** Small navigable entry points should link to focused authority. Repeated explanations increase ambiguity; deleting unique knowledge to reduce context destroys value.

Flag concrete omissions, contradictions, or maintenance traps caused by the change. Recommend the authoritative location and smallest useful correction, not documentation for its own sake.

"Self-explanatory code" never excuses missing rationale, user guidance, recovery procedures, public contracts, or required security, privacy, accessibility, licensing, compliance, and audit evidence.
