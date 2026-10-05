---
name: domain
description: "Shape executable software around business language, behavior, invariants, and lifecycle."
scope: shared-engineering-doctrine
---

# Domain Doctrine

## Prime directive

Model the business, not the machinery surrounding it.

## Position

A domain model is the shared understanding of a business problem expressed in language and running code. Tables, screens, wire formats, frameworks, and diagrams may represent pieces of that understanding, but none of them becomes the model merely by existing.

The model grows through a repeated conversation between people who know the business and people changing the software. Concrete scenarios reveal missing concepts. Implementation exposes weak explanations. Refactoring gives important meaning a name and a home.

## Principles

- **Speak one local language.** Inside one bounded context, code, tests, written material, planning, and business conversation should use the same words for the same concepts.
- **Put business decisions in the model.** Presentation, storage, messaging, frameworks, and workflow coordination may carry a decision, but they should not secretly own it.
- **Choose building blocks by meaning.** Identity suggests an entity. Descriptive immutable attributes suggest a value object. A business operation with no natural object may be a domain service. Concepts understood together form a module.
- **Protect invariants at the right boundary.** Objects that must remain mutually consistent belong behind one aggregate root. Outside code refers to that root rather than reaching through it.
- **Create whole objects.** Construction should establish valid state before an object becomes reachable. Retrieval should hide storage mechanics and answer questions in business terms.
- **Make infrastructure adapt.** Persistence and transport preserve domain identity, value semantics, invariants, and retrieval needs. Their convenient shapes must not leak back into the model.
- **Design interfaces around purpose.** Name operations for business intent. Separate questions from state-changing commands. Make preconditions, postconditions, and invariant obligations visible.
- **Refactor toward understanding.** When a policy, calculation, constraint, process, or criterion carries business meaning, model it directly instead of leaving it buried in procedural branches.
- **Test in business language.** Prove legal construction, required invariants, allowed transitions, rejected transitions, and meaningful outcomes before testing supporting plumbing.
- **Defend the distinctive model.** Commodity mechanisms and reusable infrastructure should support the business model without crowding it out.

## Boundary

This doctrine owns meaning and behavior inside one bounded context. Boundaries doctrine owns where contexts begin, how their models relate, and how meaning crosses between them. Data doctrine owns storage guarantees; Testing doctrine owns test economics and mechanics.
