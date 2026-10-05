---
name: domain-modeling
description: "Internal helper for authorized domain work. Sharpen terminology and record agreed glossaries or architecture decisions only within confirmed recording scope."
disable-model-invocation: false
user-invocable: false
---

# Domain Modeling

**Entry:** internal to an authorized task under the
[invocation contract](../setup/INVOCATION.md), not a direct human command.
Before recording, establish explicit authority for the selected glossary or
decision-record destination and its writing owner. Conversation modeling alone
does not authorize files. Reuse confirmed recording scope; ask only when missing
or changed. Keep human decisions distinct from proposals.

Use [doctrine selection and application](../doctrine/APPLY.md); preserve caller/operator selections. With none, consider `domain`, `boundaries`, and `documentation` for executable concepts, context boundaries, or durable records respectively. PR-bound changes require `worktrees` before writing; recording gates still apply.

Actively build and sharpen the project's domain model during design: challenge terms, invent edge-case scenarios, and record the glossary and decisions as they crystallise. Merely reading `CONTEXT.md` for vocabulary is a one-line habit any skill can use, not this skill. Use this skill to change the model, not just consume it.

## File structure

Most repos have a single context:

```
/
├── CONTEXT.md
├── docs/
│   └── adr/
│       ├── 0001-event-sourced-orders.md
│       └── 0002-postgres-for-write-model.md
└── src/
```

A root `CONTEXT-MAP.md` indicates multiple contexts and points to each:

```
/
├── CONTEXT-MAP.md
├── docs/
│   └── adr/                          ← system-wide decisions
├── src/
│   ├── ordering/
│   │   ├── CONTEXT.md
│   │   └── docs/adr/                 ← context-specific decisions
│   └── billing/
│       ├── CONTEXT.md
│       └── docs/adr/
```

Create files only when there is something to write: a missing `CONTEXT.md` when the first term is resolved; a missing `docs/adr/` when the first ADR is needed.

## During the session

### Challenge against the glossary

Immediately flag user terms that conflict with `CONTEXT.md`. "Your glossary defines 'cancellation' as X, but you seem to mean Y. Which is it?"

### Sharpen fuzzy language

For vague or overloaded user terms, propose a precise canonical term. "You're saying 'account': do you mean the Customer or the User? Those are different things."

### Discuss concrete scenarios

Stress-test domain relationships under discussion with specific invented edge-case scenarios that force the user to clarify concept boundaries.

### Cross-reference with code

Check user claims about how something works against the code. Surface contradictions: "Your code cancels entire Orders, but you just said partial cancellation is possible. Which is right?"

### Update CONTEXT.md inline

Update `CONTEXT.md` as each term is resolved; don't batch. Use [CONTEXT-FORMAT.md](./CONTEXT-FORMAT.md).

`CONTEXT.md` is a glossary only: no implementation details, specifications, scratch notes, or implementation decisions.

### Offer ADRs sparingly

Only offer to create an ADR when all three are true:

1. **Hard to reverse**: the cost of changing your mind later is meaningful
2. **Surprising without context**: a future reader will wonder "why did they do it this way?"
3. **The result of a real trade-off**: there were genuine alternatives and you picked one for specific reasons

If any is missing, skip the ADR. Use [ADR-FORMAT.md](./ADR-FORMAT.md).

For authorized records, use [Changelog](../changelog/SKILL.md) and return notable
entry proposals to the existing delivery owner. Serialize shared glossary,
decision, and changelog edits; do not write into an active implementer's worktree.
