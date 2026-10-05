---
name: solid
description: "Review responsibilities, contracts, extension points, and dependency direction through the five SOLID principles."
scope: default-pr-sniper-doctrine
---

# SOLID Doctrine

Good design makes change local, deliberate, and unsurprising while preserving trustworthy behavior. Tangled responsibilities and dishonest contracts matter more than file length.

Review the proposed change through all five SOLID lenses. These are complementary questions, not a demand for interfaces, subclasses, or abstractions everywhere.

- **Single Responsibility Principle.** Does each module, component, or class have one coherent source of change? Keep behavior together for the same business reason; flag combinations pulled apart by different owners, policies, or timelines.
- **Open/Closed Principle.** Where variation already exists, can a new case extend behavior without repeatedly rewriting trusted logic? Look for earned extension boundaries, not speculative flexibility or preservation of obsolete paths.
- **Liskov Substitution Principle.** Can implementations substitute without surprising callers? Compare accepted inputs, outputs, invariants, side effects, and failures. Matching signatures do not prove matching behavioral contracts.
- **Interface Segregation Principle.** Do clients depend only on capabilities they need? Flag forced implementation, mocking, or knowledge of unrelated operations. Focused contracts help; fragmented forwarding interfaces may increase coupling instead.
- **Dependency Inversion Principle.** Does important policy define stable contracts, or depend directly on volatile mechanisms? Infrastructure may implement those contracts. A wrapper around one concrete dependency is not meaningful inversion without real policy or variation.

The principles reinforce one another: coherent responsibility reveals focused contracts; honest substitution supports safe extension; dependency direction protects policy as mechanisms change.

For each finding, identify the changed dependency or contract and its concrete failure or change-cost consequence. Recommend a focused correction that respects existing domain boundaries, simplicity, and control-flow clarity.

SOLID supplies judgment, not guaranteed scalability, reuse, testability, or maintainability. Do not manufacture layers or report a defect solely because a preferred pattern is absent.
