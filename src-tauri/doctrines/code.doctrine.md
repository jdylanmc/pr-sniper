---
name: code
description: "Review code for defect risk, readable behavior, coherent responsibilities, and trustworthy evidence."
scope: default-pr-sniper-doctrine
---

# Code Doctrine

Good construction reduces defect risk and effort for the next reader. Code is read more often than written; working once is not sufficient evidence.

Review the change against requirements, architectural fit, language constraints, project conventions, error policy, reusable parts, and integration obligations. Uncertain behavior warrants focused evidence from a small real slice, not confident speculation.

Look for:

- **Coherent routines.** One nameable purpose, small interface, difficult misuse. Validation, computation, coordination, and effects separate when responsibilities differ.
- **Meaningful data.** Names, types, units, ranges, and structures expose intent. Scope stays small, initialization deliberate, Booleans genuinely binary.
- **Visible control flow.** Clear normal path, shallow nesting, named conditions, plain loops, explicit effects. Tables help only when rules become easier to inspect and validate.
- **Honest failures.** Validation occurs where trust changes. Assertions protect programmer invariants; domain results represent expected failure. Error handling preserves diagnostic context at a level able to interpret it.
- **Cohesive modules.** Representation and bookkeeping stay private. Unrelated persistence, formatting, business decisions, and integration must not accumulate behind one name.
- **Finished replacements.** Proven redundancy and accidental indirection disappear. Superseded paths and temporary bridges remain only while compatibility obligations justify them.
- **Behavioral protection.** Evidence covers normal outcomes, boundaries, invalid input, defensive checks, and data-driven edge cases without freezing implementation. Risky restructuring needs protection; separate behavior changes when that clarifies review.
- **Measured optimization.** Performance complexity needs a target, baseline, isolated change, and demonstrated gain. Prefer the clearer implementation otherwise.

Identify the concrete failure or comprehension cost introduced by the change. Recommend a focused correction using existing idioms, not stylistic churn or unrelated cleanup.
