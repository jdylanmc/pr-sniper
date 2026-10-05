---
name: solid
description: "Use five complementary design principles to prevent tangled responsibilities, contracts, and dependencies."
scope: shared-engineering-doctrine
---

# SOLID Doctrine

Prime directive: preserve trustworthy behavior while making change local, deliberate, and unsurprising.

Spaghetti code is not merely code with many lines. It is code where responsibilities overlap, changes ripple unpredictably, contracts lie, clients depend on things they do not use, and important policy is trapped inside volatile mechanisms.

SOLID provides five equal lenses for finding those pressures. They are not rituals. They do not require an interface, subclass, or abstraction everywhere. Apply them where they reduce change cost and clarify ownership.

- Single Responsibility Principle. Give a module, component, or class one coherent source of change. Keep behavior together when it changes for the same business reason. Separate behavior when different owners, policies, or timelines pull it apart.
- Open/Closed Principle. Protect stable behavior behind a deliberate extension boundary when real variation exists. Add a new case without repeatedly rewriting trusted logic. Do not predict hypothetical extensions or preserve obsolete paths.
- Liskov Substitution Principle. An interchangeable implementation must preserve the behavioral expectations of the contract it claims to satisfy. Inputs, outputs, invariants, side effects, and failure behavior matter more than matching a type signature.
- Interface Segregation Principle. Give clients focused contracts containing what they actually need. Do not force consumers to understand, implement, mock, or depend on unrelated capabilities. Small interfaces are valuable when they represent cohesive client needs, not when fragmentation creates forwarding ceremony.
- Dependency Inversion Principle. Important policy should define the stable contracts it needs instead of depending directly on volatile mechanisms. Infrastructure may implement those contracts. An abstraction must express real policy and variation. It must not hide one concrete dependency behind another name.

The principles reinforce one another. Coherent responsibility reveals the right contract. Focused contracts make substitution honest. Honest substitution enables safe extension. Dependency direction keeps policy stable while mechanisms change.

SOLID does not guarantee scalability, reuse, testability, or maintainability. It supplies questions for design judgment. Code, Bounded Context, Laziness, and Cyclomatic Complexity remain authoritative for their concerns. Use SOLID to reduce coupling, not to manufacture layers.
