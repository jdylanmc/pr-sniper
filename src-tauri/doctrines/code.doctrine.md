---
name: code
description: "Construct readable, verifiable code whose local behavior remains open to inspection."
scope: default-pr-sniper-doctrine
---

# Code Doctrine

Prime directive: choose the implementation with less defect risk and less effort for the next reader.

Working once is not finished construction. Before substantial coding, understand the requirement, architectural fit, language constraints, conventions, error policy, representation, reusable parts, integration path, and verification approach. When the ground is uncertain, build the smallest real slice that can expose the uncertainty.

Code is read more often than it is written. Prefer explicit behavior, visible control flow, related concepts kept together, and familiar project idioms. Reject cleverness and compressed syntax.

- A routine does one nameable thing, exposes a small interface, and resists incorrect use. Separate validation, computation, coordination, and effects when they are different responsibilities.
- Names, types, units, ranges, and structures reveal purpose. Keep scope small and initialization deliberate. Use a Boolean only for genuinely binary meaning.
- Favor a clear normal path, shallow nesting, named conditions, plain loops, and explicit side effects. Table-driven logic earns its place only when the table makes the rule easier to inspect and validate.
- Validate where trust changes hands. Use assertions for programmer invariants and domain results for expected failure. Handle errors at the level that can interpret them. Preserve diagnostic context.
- Hide representation and internal bookkeeping. Do not let unrelated persistence, formatting, business logic, and integration accumulate behind one name.
- Remove proven redundancy and accidental indirection before extending. Once a replacement is proven, remove superseded paths and temporary bridges whose obligations have ended.
- Test the contract: normal behavior, boundaries, invalid input, defensive checks, promised outcomes, and edge cases suggested by the data. Tests protect behavior. They do not freeze implementation shape.
- Place protection around risky or poorly understood behavior before restructuring it. Keep behavior changes separate when that makes review easier.
- Tune measured problems. Set a performance target, measure the baseline, change one thing, and measure again. Keep the clearer form unless the demonstrated gain earns the complexity.
