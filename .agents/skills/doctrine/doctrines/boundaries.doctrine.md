---
name: boundaries
description: "Protect locally coherent domain models through deliberate boundaries, ownership, and translation."
scope: shared-engineering-doctrine
---

# Boundaries Doctrine

## Prime directive

A model means what it means inside the boundary that owns it.

## Position

A bounded context is like a dollhouse. The things inside belong to one coherent little world: the kitchen, dining table, and bathtub make sense together because the house gives them a place and a purpose. A road sign or a tree belongs to a different world.

The walls protect that coherence. Doors and windows permit deliberate exchange. A missing wall is not openness; it is an uncontrolled place where foreign meanings, assumptions, and ownership leak in until the model no longer explains itself.

Enterprise software cannot sustain one universal model for every business purpose. The same word may carry different data, behavior, and obligations in different contexts. Those differences are not duplication to eliminate. They are meaning to own.

## Principles

- **Keep meaning local.** Inside one boundary, language, behavior, invariants, and lifecycle should agree. A term has one owned meaning there, even when another context uses the same word differently.
- **Put only belonging things inside.** A model should contain what its business purpose needs. Foreign concerns, framework shapes, transport formats, and neighboring models do not become native merely because they are convenient to reuse.
- **Build walls with care.** Boundaries should make ownership and authority visible. Gaps create shared state, ambiguous responsibility, and accidental coupling.
- **Design doors and windows.** Contexts collaborate through explicit contracts and deliberate translation. Crossing a boundary should reveal that meaning may change rather than pretending both sides share one model.
- **Protect autonomy without demanding isolation.** A boundary permits independent evolution while preserving intentional relationships with the larger enterprise.
- **Align structure when useful, not by definition.** A context may align with a team, service, repository, or database, but none of those automatically defines the model boundary.
- **Avoid both universal models and shattered ones.** One model for everything collapses distinct meanings. Too many tiny contexts replace coherence with translation overhead.
- **Evolve boundaries deliberately.** When language, ownership, or business purpose changes, revisit the walls and contracts instead of letting the model drift across them.

## Boundary

The dollhouse is a lens, not a complete architecture. Boundaries exist to preserve meaning and ownership while enabling controlled collaboration—not to wall every component away from the rest of the system.
