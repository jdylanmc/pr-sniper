---
name: code
description: "Construct readable, verifiable code whose local behavior remains open to inspection."
scope: shared-engineering-doctrine
---

# Code Doctrine

## Prime directive

Choose the implementation that carries less defect risk and costs the next reader less effort to understand.

## Position

Working once is not finished construction. Before substantial coding, understand the requirement, architectural fit, language constraints, conventions, error policy, representation, reusable parts, integration path, and verification approach. When the ground is uncertain, build the smallest real slice that can expose the uncertainty.

Code is read more often than it is written. Prefer explicit behavior, visible control flow, related concepts kept together, and familiar project idioms over cleverness or compressed syntax.

## Principles

- **Make routines coherent.** A routine does one nameable thing, exposes a small interface, and resists incorrect use. Separate validation, computation, coordination, and effects when they represent different responsibilities.
- **Make data carry meaning.** Use names, types, units, ranges, and structures that reveal purpose. Keep scope small and initialization deliberate. Use a Boolean only for genuinely binary meaning.
- **Keep control flow visible.** Favor a clear normal path, shallow nesting, named conditions, plain loops, and explicit side effects. Table-driven logic earns its place only when the table makes the rule easier to inspect and validate.
- **Guard boundaries deliberately.** Validate where trust changes hands. Use assertions for programmer invariants and domain results for expected failure. Handle errors at the level that can interpret them while preserving diagnostic context.
- **Keep modules cohesive.** Hide representation and internal bookkeeping. Do not let unrelated persistence, formatting, business logic, and integration accumulate behind one name.
- **Simplify before extending.** Remove proven redundancy and accidental indirection before building on them. Once a replacement is proven, finish the refactor by removing superseded paths and temporary bridges whose obligations have ended.
- **Test the contract.** Cover normal behavior, boundaries, invalid input, defensive checks, promised outcomes, and edge cases suggested by the data. Tests should protect behavior rather than freeze implementation shape.
- **Refactor with evidence.** Place protection around risky or poorly understood behavior before restructuring it. Keep behavior changes separate when reviewability benefits.
- **Tune measured problems.** Set a performance target, measure the baseline, change one thing, and measure again. Keep the clearer form unless the demonstrated gain earns the complexity.

## Boundary

Code Doctrine owns local construction. Documentation owns durable explanation; Cyclomatic Complexity owns path pressure; Debugging owns causal investigation; Testing owns test economics; Laziness and Sequencing own change size and order. Do not duplicate their full authority here.
