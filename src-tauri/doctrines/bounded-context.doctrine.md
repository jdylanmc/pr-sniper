---
name: bounded-context
description: "Model business meaning inside one owned boundary, and translate deliberately when meaning crosses it."
scope: default-pr-sniper-doctrine
---

# Bounded Context Doctrine

Prime directive: model the business inside the boundary that owns its meaning. Not machinery. Not one universal model.

A domain model is shared business understanding in language and running code. Tables, screens, wires, frameworks, and diagrams represent pieces. None is the model by existing. Grows by talk between business knowers and software changers. Scenarios show missing concepts. Implementation shows weak explanations. Refactoring gives meaning a name and a home.

A bounded context is a dollhouse. Inside things share one coherent world. Road sign or tree belongs elsewhere. Walls protect coherence. Doors and windows allow deliberate exchange. Missing wall is not openness. Foreign meaning, assumptions, and ownership leak until the model cannot explain itself. Dollhouse is a lens, not full architecture. Do not wall every component.

One enterprise model cannot hold every purpose. The same word may carry different data, behavior, and obligations across contexts. Own that difference. Do not delete it as duplication.

- One local language in code, tests, writing, planning, and business talk. A term has one owned meaning there, even if another context uses the word differently.
- Include only what the purpose needs. Foreign concerns, framework shapes, transport formats, and neighbor models do not become native because reuse is easy.
- Business decisions live in the model. Presentation, storage, messaging, frameworks, and workflow may carry a decision. They must not secretly own it.
- Identity means entity. Immutable descriptive attributes mean value object. An operation with no natural object means domain service. Concepts understood together mean module.
- Mutually consistent objects sit behind one aggregate root. Outside code names that root and does not reach through it.
- Show ownership and authority. Gaps make shared state, ambiguous responsibility, and accidental coupling.
- Cross by explicit contract and deliberate translation. A crossing shows meaning may change. Do not pretend one shared model.
- Construction makes valid state before an object is reachable. Retrieval hides storage and answers in business terms.
- Persistence and transport keep identity, value semantics, invariants, and retrieval needs. Convenient shapes must not leak in.
- Name operations for business intent. Separate questions from commands. Show preconditions, postconditions, and invariants.
- Allow independent change and keep intentional ties to the larger enterprise. Isolation is not required.
- A team, service, repository, or database may align with a context. None defines the boundary alone.
- One model for everything collapses distinct meanings. Too many tiny contexts trade coherence for translation cost.
- Model a policy, calculation, constraint, process, or criterion directly when it has business meaning. Do not bury it in branches.
- Prove legal construction, required invariants, allowed and rejected transitions, and meaningful outcomes before plumbing tests.
- Commodity mechanisms support the distinctive model. They do not crowd it out.
- When language, ownership, or purpose changes, revisit walls and contracts. Do not let the model drift across them.

Owns meaning, behavior, and where contexts begin, relate, and exchange meaning. Data owns storage guarantees. Testing owns test economics and mechanics.
